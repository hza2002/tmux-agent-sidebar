//! Read-only, deadline-bound Codex control-socket inventory.
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

fn request(
    socket: &mut WebSocket<DeadlineStream>,
    id: u64,
    method: &str,
    params: Value,
    unsolicited: &mut usize,
) -> Result<Value, String> {
    remaining(socket.get_ref().deadline).map_err(|e| e.to_string())?;
    socket
        .send(Message::text(
            json!({"id":id,"method":method,"params":params}).to_string(),
        ))
        .map_err(|e| e.to_string())?;
    loop {
        remaining(socket.get_ref().deadline).map_err(|e| e.to_string())?;
        let message = socket.read().map_err(|e| e.to_string())?;
        if let Message::Text(text) = message {
            let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if !value.is_object() {
                return Err("Invalid RPC envelope".into());
            }
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if value.get("error").is_some() {
                    return Err("Codex inventory RPC error".into());
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "Missing RPC result".into());
            }
        } else if matches!(message, Message::Close(_) | Message::Binary(_)) {
            return Err("Unexpected Codex inventory message".into());
        }
        *unsolicited += 1;
        if *unsolicited > UNSOLICITED_LIMIT {
            return Err("Too many unsolicited messages".into());
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

pub(super) fn loaded_thread_ids(path: &Path, deadline: Instant) -> Result<Vec<String>, String> {
    let stream = connect(path, deadline).map_err(|e| e.to_string())?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MESSAGE_LIMIT))
        .max_frame_size(Some(MESSAGE_LIMIT));
    let (mut socket, _) =
        tungstenite::client::client_with_config("ws://localhost/rpc", stream, Some(config))
            .map_err(|e| e.to_string())?;
    let mut unsolicited = 0;
    let initialized = request(
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
        return Err("Invalid initialize result".into());
    }
    socket
        .send(Message::text(json!({"method":"initialized"}).to_string()))
        .map_err(|e| e.to_string())?;
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
