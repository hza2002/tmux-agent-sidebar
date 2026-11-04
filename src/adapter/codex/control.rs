//! Read-only, deadline-bound Codex control-socket queries.
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

const MESSAGE_LIMIT: usize = 1024 * 1024;
const ID_LIMIT: usize = 16_384;
const UNSOLICITED_LIMIT: usize = 32;

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Codex inventory deadline expired"))
}

// A timeout per WebSocket message is insufficient: a fragmented/trickled
// message can cause many underlying reads. Check the shared deadline for each.
struct DeadlineStream {
    stream: UnixStream,
    deadline: Instant,
}

fn wait_fd(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    loop {
        let timeout = remaining(deadline)?
            .as_millis()
            .min(i32::MAX as u128)
            .max(1) as i32;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: descriptor is initialized; poll receives one live fd record.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}
impl Read for DeadlineStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            remaining(self.deadline)?;
            match self.stream.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    wait_fd(self.stream.as_raw_fd(), libc::POLLIN, self.deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            remaining(self.deadline)?;
            match self.stream.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    wait_fd(self.stream.as_raw_fd(), libc::POLLOUT, self.deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        remaining(self.deadline)?;
        self.stream.flush()
    }
}

fn connect(path: &Path, deadline: Instant) -> io::Result<DeadlineStream> {
    remaining(deadline)?;
    // Codex's advertised long path can be a symlink to a short physical socket.
    let path = std::fs::canonicalize(path)?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: sockaddr_un is a plain C address structure, zero initialization
    // provides the trailing NUL and initializes every platform field.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Unix socket path",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dst, src) in address.sun_path.iter_mut().zip(bytes) {
        *dst = *src as libc::c_char;
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        address.sun_len = std::mem::size_of_val(&address) as u8;
    }
    // SAFETY: successful socket returns a fresh owned descriptor.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw was just returned by socket and has no other owner.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: fd is live; these fcntl commands do not require pointer arguments.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: address is initialized and its supplied length is exact.
    let result = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS) | Some(libc::EAGAIN)
        ) {
            return Err(error);
        }
        wait_fd(fd.as_raw_fd(), libc::POLLOUT, deadline)?;
    }
    let stream = UnixStream::from(fd);
    if let Some(error) = stream.take_error()? {
        return Err(error);
    }
    stream.set_nonblocking(true)?;
    remaining(deadline)?;
    Ok(DeadlineStream { stream, deadline })
}

/// Sanitized errors shared by the quota reader and the existing inventory wrapper.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ControlError {
    Timeout,
    Transport,
    Data,
    AccountChanged,
    Rpc { code: i64 },
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("Codex control query timed out"),
            Self::Transport => f.write_str("Codex control transport failed"),
            Self::Data => f.write_str("Invalid Codex control response"),
            Self::AccountChanged => f.write_str("Codex account changed during quota query"),
            Self::Rpc { code, .. } => write!(f, "Codex control RPC rejected ({code})"),
        }
    }
}

impl From<io::Error> for ControlError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            Self::Timeout
        } else {
            Self::Transport
        }
    }
}

impl From<tungstenite::Error> for ControlError {
    fn from(error: tungstenite::Error) -> Self {
        match error {
            tungstenite::Error::Io(error) => error.into(),
            _ => Self::Transport,
        }
    }
}

fn request(
    socket: &mut WebSocket<DeadlineStream>,
    id: u64,
    method: &str,
    params: Value,
    unsolicited: &mut usize,
) -> Result<Value, String> {
    request_detailed(socket, id, method, params, unsolicited).map_err(|e| e.to_string())
}

fn request_detailed(
    socket: &mut WebSocket<DeadlineStream>,
    id: u64,
    method: &str,
    params: Value,
    unsolicited: &mut usize,
) -> Result<Value, ControlError> {
    remaining(socket.get_ref().deadline)?;
    socket.send(Message::text(
        json!({"id":id,"method":method,"params":params}).to_string(),
    ))?;
    loop {
        remaining(socket.get_ref().deadline)?;
        let message = socket.read()?;
        if let Message::Text(text) = message {
            let value: Value = serde_json::from_str(&text).map_err(|_| ControlError::Data)?;
            if !value.is_object() {
                return Err(ControlError::Data);
            }
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    return Err(ControlError::Rpc {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    });
                }
                return value.get("result").cloned().ok_or(ControlError::Data);
            }
        } else if matches!(message, Message::Close(_) | Message::Binary(_)) {
            return Err(ControlError::Transport);
        }
        *unsolicited += 1;
        if *unsolicited > UNSOLICITED_LIMIT {
            return Err(ControlError::Data);
        }
    }
}

fn canonical_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}

fn initialized_socket(path: &Path, deadline: Instant) -> Result<WebSocket<DeadlineStream>, String> {
    initialized_socket_detailed(path, deadline).map_err(|e| e.to_string())
}

fn initialized_socket_detailed(
    path: &Path,
    deadline: Instant,
) -> Result<WebSocket<DeadlineStream>, ControlError> {
    let stream = connect(path, deadline)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MESSAGE_LIMIT))
        .max_frame_size(Some(MESSAGE_LIMIT));
    let (mut socket, _) =
        tungstenite::client::client_with_config("ws://localhost/rpc", stream, Some(config))
            .map_err(|_| {
                if Instant::now() >= deadline {
                    ControlError::Timeout
                } else {
                    ControlError::Transport
                }
            })?;
    let mut unsolicited = 0;
    let initialized = request_detailed(
        &mut socket,
        1,
        "initialize",
        json!({
            "clientInfo":{"name":"tmux_agent_sidebar","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true}
        }),
        &mut unsolicited,
    )?;
    if !initialized.is_object() {
        return Err(ControlError::Data);
    }
    socket.send(Message::text(json!({"method":"initialized"}).to_string()))?;
    Ok(socket)
}

pub(crate) fn loaded_thread_ids(path: &Path, deadline: Instant) -> Result<Vec<String>, String> {
    let mut socket = initialized_socket(path, deadline)?;
    let mut unsolicited = 0;
    let result = request(
        &mut socket,
        2,
        "thread/loaded/list",
        json!({}),
        &mut unsolicited,
    )?;
    if result.get("nextCursor") != Some(&Value::Null) {
        return Err("Incomplete loaded-thread inventory".into());
    }
    validate_inventory(&result, deadline).inspect(|_| {
        // Best effort only; never wait for a peer's closing handshake.
        if remaining(deadline).is_ok() {
            let _ = socket.close(None);
        }
    })
}

/// Observe the daemon login, without refreshing credentials. This is not
/// per-request billing telemetry; callers must keep its runtime provenance.
pub(crate) fn account_id(path: &Path, deadline: Instant) -> Result<Option<String>, String> {
    let mut socket = initialized_socket(path, deadline)?;
    let result = request(
        &mut socket,
        2,
        "account/read",
        json!({"refreshToken":false}),
        &mut 0,
    )?;
    let account = parse_account(&result);
    let _ = socket.close(None);
    Ok(account)
}

/// Fetch through the running daemon, keeping identity and quota on one connection.
/// The server may refresh its own credentials; the sidebar never reads or writes them.
pub(crate) fn scoped_rate_limits(
    path: &Path,
    deadline: Instant,
) -> Result<Option<(String, Value)>, ControlError> {
    let mut socket = initialized_socket_detailed(path, deadline)?;
    let mut unsolicited = 0;
    let before = request_detailed(
        &mut socket,
        2,
        "account/read",
        json!({"refreshToken":false}),
        &mut unsolicited,
    )?;
    let Some(account) = parse_account(&before) else {
        let _ = socket.close(None);
        // Only an explicit sign-out or API-key login is unavailable. Unknown
        // schemas must not hide the last usable official quota snapshot.
        return if before.get("account") == Some(&Value::Null)
            || matches!(
                before.pointer("/account/type").and_then(Value::as_str),
                Some("apiKey" | "apikey")
            ) {
            Ok(None)
        } else {
            Err(ControlError::Data)
        };
    };
    let quota = request_detailed(
        &mut socket,
        3,
        "account/rateLimits/read",
        json!({"excludeResetCreditDetails":true}),
        &mut unsolicited,
    )?;
    // Missing quota identity is not enough to bind a result across a login switch.
    if quota.get("accountId").and_then(Value::as_str) != Some(account.as_str()) {
        return Err(ControlError::AccountChanged);
    }
    let after = request_detailed(
        &mut socket,
        4,
        "account/read",
        json!({"refreshToken":false}),
        &mut unsolicited,
    )?;
    if parse_account(&after).as_deref() != Some(account.as_str()) {
        return Err(ControlError::AccountChanged);
    }
    let _ = socket.close(None);
    Ok(Some((account, quota)))
}

fn parse_account(result: &Value) -> Option<String> {
    if result.pointer("/account/type").and_then(Value::as_str) != Some("chatgpt")
        || result.get("requiresOpenaiAuth").and_then(Value::as_bool) != Some(true)
    {
        return None;
    }
    result
        .pointer("/workspaceRouting/chatgptAccountId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .map(str::to_owned)
}

fn validate_inventory(result: &Value, deadline: Instant) -> Result<Vec<String>, String> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or("Missing loaded thread IDs")?;
    if data.len() > ID_LIMIT {
        return Err("Loaded-thread inventory exceeds limit".into());
    }
    let mut seen = HashSet::with_capacity(data.len());
    let mut ids = Vec::with_capacity(data.len());
    for value in data {
        let id = value
            .as_str()
            .filter(|id| canonical_uuid(id))
            .ok_or("Malformed loaded thread ID")?;
        if !seen.insert(id) {
            return Err("Duplicate loaded thread ID".into());
        }
        ids.push(id.to_owned());
    }
    remaining(deadline).map_err(|e| e.to_string())?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    const ID: &str = "01a12ada-edfc-75a3-a451-d6312dd93d6d";

    #[test]
    fn account_observation_requires_explicit_chatgpt_routing() {
        let result = json!({"account":{"type":"chatgpt","email":"not-retained"},"requiresOpenaiAuth":true,"workspaceRouting":{"chatgptAccountId":"account-a"}});
        assert_eq!(parse_account(&result).as_deref(), Some("account-a"));
        for result in [
            json!({"account":{"type":"apiKey"},"requiresOpenaiAuth":true,"workspaceRouting":{"chatgptAccountId":"account-a"}}),
            json!({"account":{"type":"chatgpt"},"requiresOpenaiAuth":false,"workspaceRouting":{"chatgptAccountId":"account-a"}}),
            json!({"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}),
        ] {
            assert_eq!(parse_account(&result), None);
        }
    }

    fn accept_fixture(listener: &UnixListener) -> (UnixStream, std::os::unix::net::SocketAddr) {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok(connection) => {
                    connection.0.set_nonblocking(false).unwrap();
                    return connection;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    wait_fd(listener.as_raw_fd(), libc::POLLIN, deadline).unwrap()
                }
                Err(e) => panic!("fake server accept failed: {e}"),
            }
        }
    }

    fn server_response(socket: &mut WebSocket<UnixStream>, id: u64, result: Value) {
        socket
            .send(Message::text(json!({"id":id,"result":result}).to_string()))
            .unwrap();
    }
    fn receive(socket: &mut WebSocket<UnixStream>) -> Value {
        serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
    }
    fn fixture(result: Value) -> Result<Vec<String>, String> {
        let dir = tempfile::Builder::new()
            .prefix("sidebar-ws.")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("rpc");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = accept_fixture(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let initialize = receive(&mut socket);
            assert_eq!(initialize["method"], "initialize");
            assert_eq!(initialize["id"], 1);
            assert_eq!(
                initialize["params"]["capabilities"]["experimentalApi"],
                true
            );
            server_response(&mut socket, 999, json!({}));
            server_response(&mut socket, 1, json!({"userAgent":"fixture"}));
            let notification = receive(&mut socket);
            assert_eq!(notification["method"], "initialized");
            let request = receive(&mut socket);
            assert_eq!(request["id"], 2);
            assert_eq!(request["method"], "thread/loaded/list");
            assert_eq!(request["params"], json!({}));
            server_response(&mut socket, 2, result);
        });
        let result = loaded_thread_ids(&path, Instant::now() + Duration::from_millis(250));
        server.join().unwrap();
        result
    }

    fn quota_account(id: &str) -> Value {
        json!({"account":{"type":"chatgpt"},"requiresOpenaiAuth":true,
            "workspaceRouting":{"chatgptAccountId":id}})
    }

    fn quota_fixture(
        before: Value,
        quota: Value,
        after: Option<Value>,
    ) -> Result<Option<(String, Value)>, ControlError> {
        let dir = tempfile::Builder::new()
            .prefix("sidebar-quota.")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("rpc");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = accept_fixture(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            assert_eq!(receive(&mut socket)["method"], "initialize");
            server_response(&mut socket, 1, json!({}));
            assert_eq!(receive(&mut socket)["method"], "initialized");
            let account = receive(&mut socket);
            assert_eq!(account["method"], "account/read");
            assert_eq!(account["params"], json!({"refreshToken":false}));
            let official = parse_account(&before).is_some();
            server_response(&mut socket, 2, before);
            if !official {
                return;
            }
            let query = receive(&mut socket);
            assert_eq!(query["method"], "account/rateLimits/read");
            assert_eq!(query["params"], json!({"excludeResetCreditDetails":true}));
            socket
                .send(Message::text(json!({"id":3, "result":quota}).to_string()))
                .unwrap();
            if let Some(after) = after {
                let query = receive(&mut socket);
                assert_eq!(query["method"], "account/read");
                assert_eq!(query["params"], json!({"refreshToken":false}));
                server_response(&mut socket, 4, after);
            }
        });
        let result = scoped_rate_limits(&path, Instant::now() + Duration::from_secs(1));
        server.join().unwrap();
        result
    }

    #[test]
    fn quota_binds_the_running_account_before_result_and_after() {
        let quota = json!({"accountId":"a", "rateLimits":{"primary":{"usedPercent":4,"windowDurationMins":10080}}});
        let (account, actual) =
            quota_fixture(quota_account("a"), quota.clone(), Some(quota_account("a")))
                .unwrap()
                .unwrap();
        assert_eq!(account, "a");
        assert_eq!(actual, quota);
        assert_eq!(
            quota_fixture(quota_account("a"), json!({"accountId":"b"}), None),
            Err(ControlError::AccountChanged)
        );
        assert_eq!(
            quota_fixture(quota_account("a"), json!({}), None),
            Err(ControlError::AccountChanged)
        );
        assert_eq!(
            quota_fixture(
                quota_account("a"),
                json!({"accountId":"a"}),
                Some(quota_account("b"))
            ),
            Err(ControlError::AccountChanged)
        );
    }

    #[test]
    fn quota_skips_backend_read_for_non_official_login() {
        assert_eq!(
            quota_fixture(json!({"account":{"type":"apiKey"}}), json!({}), None).unwrap(),
            None
        );
    }

    #[test]
    fn quota_rejects_unknown_official_account_schema() {
        assert_eq!(
            quota_fixture(json!({}), json!({}), None),
            Err(ControlError::Data)
        );
        assert_eq!(
            quota_fixture(
                json!({"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}),
                json!({}),
                None
            ),
            Err(ControlError::Data)
        );
    }

    #[test]
    fn quota_timeout_and_generic_rpc_failure_remain_distinct() {
        let (client, server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut socket = WebSocket::from_raw_socket(
            DeadlineStream {
                stream: client,
                deadline,
            },
            tungstenite::protocol::Role::Client,
            None,
        );
        let worker = thread::spawn(move || {
            let mut socket =
                WebSocket::from_raw_socket(server, tungstenite::protocol::Role::Server, None);
            receive(&mut socket);
            socket.send(Message::text(json!({"id":3,"error":{"code":-32603,"message":"sensitive server text","data":null}}).to_string())).unwrap();
        });
        let error = request_detailed(&mut socket, 3, "account/rateLimits/read", json!({}), &mut 0)
            .unwrap_err();
        assert_eq!(error, ControlError::Rpc { code: -32603 });
        assert!(!error.to_string().contains("sensitive"));
        worker.join().unwrap();
        drop(socket);
        let (client, _server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        let mut socket = WebSocket::from_raw_socket(
            DeadlineStream {
                stream: client,
                deadline: Instant::now() + Duration::from_millis(30),
            },
            tungstenite::protocol::Role::Client,
            None,
        );
        assert_eq!(
            request_detailed(&mut socket, 3, "account/rateLimits/read", json!({}), &mut 0),
            Err(ControlError::Timeout)
        );
    }

    #[test]
    fn complete_inventory_and_rpc_identity() {
        assert_eq!(
            fixture(json!({"data":[ID],"nextCursor":null})).unwrap(),
            vec![ID]
        );
    }

    #[test]
    fn invalid_inventory_is_rejected() {
        for value in [
            json!({"data":[ID,ID],"nextCursor":null}),
            json!({"data":["bad"],"nextCursor":null}),
            json!({"data":[ID],"nextCursor":"page2"}),
            json!({"data":[ID]}),
            json!({"data":[5],"nextCursor":null}),
            json!({"data":null,"nextCursor":null}),
        ] {
            assert!(fixture(value).is_err());
        }
    }

    #[test]
    fn symlink_socket_and_control_frames() {
        let dir = tempfile::Builder::new()
            .prefix("sidebar-ws.")
            .tempdir_in("/tmp")
            .unwrap();
        let physical = dir.path().join("rpc");
        let nested = dir.path().join("x".repeat(150));
        std::fs::create_dir(&nested).unwrap();
        let advertised = nested.join("socket");
        std::os::unix::fs::symlink(&physical, &advertised).unwrap();
        let listener = UnixListener::bind(&physical).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = accept_fixture(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            receive(&mut socket);
            socket.send(Message::Ping(vec![1].into())).unwrap();
            server_response(&mut socket, 1, json!({}));
            assert!(matches!(socket.read().unwrap(), Message::Pong(_)));
            assert_eq!(receive(&mut socket)["method"], "initialized");
            receive(&mut socket);
            server_response(&mut socket, 2, json!({"data":[ID],"nextCursor":null}));
        });
        assert_eq!(
            loaded_thread_ids(&advertised, Instant::now() + Duration::from_secs(1)).unwrap(),
            vec![ID]
        );
        server.join().unwrap();
    }

    #[test]
    fn silent_handshake_is_bounded() {
        let dir = tempfile::Builder::new()
            .prefix("sidebar-ws.")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("rpc");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (_stream, _) = accept_fixture(&listener);
            thread::sleep(Duration::from_millis(180));
        });
        let start = Instant::now();
        assert!(loaded_thread_ids(&path, start + Duration::from_millis(40)).is_err());
        assert!(start.elapsed() < Duration::from_millis(150));
        server.join().unwrap();
    }

    #[test]
    fn fragmented_trickle_obeys_shared_deadline() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            // Unmasked server frame, fragmented JSON string; never completes.
            for bytes in [
                &b"\x01\x01{"[..],
                &b"\x00\x01\""[..],
                &b"\x00\x01a"[..],
                &b"\x00\x01b"[..],
            ] {
                if server.write_all(bytes).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        let start = Instant::now();
        client.set_nonblocking(true).unwrap();
        let stream = DeadlineStream {
            stream: client,
            deadline: start + Duration::from_millis(45),
        };
        let mut socket =
            WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, None);
        assert!(socket.read().is_err());
        assert!(start.elapsed() < Duration::from_millis(110));
        drop(socket);
        worker.join().unwrap();
    }

    #[test]
    fn rpc_errors_and_unsolicited_messages_are_rejected() {
        for frames in [
            vec![json!({"id":2,"error":{"message":"no"}})],
            vec![json!({"id":2})],
            vec![json!({"method":"notice"}); UNSOLICITED_LIMIT + 1],
        ] {
            let (client, server) = UnixStream::pair().unwrap();
            let worker = thread::spawn(move || {
                let mut socket =
                    WebSocket::from_raw_socket(server, tungstenite::protocol::Role::Server, None);
                socket.read().unwrap();
                for value in frames {
                    if socket.send(Message::text(value.to_string())).is_err() {
                        break;
                    }
                }
            });
            client.set_nonblocking(true).unwrap();
            let stream = DeadlineStream {
                stream: client,
                deadline: Instant::now() + Duration::from_secs(1),
            };
            let mut socket =
                WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, None);
            assert!(request(&mut socket, 2, "thread/loaded/list", json!({}), &mut 0).is_err());
            drop(socket);
            worker.join().unwrap();
        }
    }

    #[test]
    fn oversized_frame_and_inventory_are_rejected() {
        let result = json!({"data":vec![ID; ID_LIMIT + 1]});
        assert!(validate_inventory(&result, Instant::now() + Duration::from_secs(1)).is_err());
        let (client, mut server) = UnixStream::pair().unwrap();
        // A frame header claiming >1MiB must be rejected without reading its body.
        let mut header = vec![0x81, 0x7f];
        header.extend_from_slice(&((MESSAGE_LIMIT + 1) as u64).to_be_bytes());
        server.write_all(&header).unwrap();
        client.set_nonblocking(true).unwrap();
        let stream = DeadlineStream {
            stream: client,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let config = WebSocketConfig::default()
            .max_message_size(Some(MESSAGE_LIMIT))
            .max_frame_size(Some(MESSAGE_LIMIT));
        let mut socket =
            WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, Some(config));
        assert!(matches!(
            socket.read(),
            Err(tungstenite::Error::Capacity(_))
        ));
    }

    #[test]
    fn malformed_upgrade_and_expired_deadline_fail() {
        let dir = tempfile::Builder::new()
            .prefix("sidebar-ws.")
            .tempdir_in("/tmp")
            .unwrap();
        assert!(loaded_thread_ids(&dir.path().join("none"), Instant::now()).is_err());
        let path = dir.path().join("rpc");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = accept_fixture(&listener);
            stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        assert!(loaded_thread_ids(&path, Instant::now() + Duration::from_secs(1)).is_err());
        server.join().unwrap();
    }
}
