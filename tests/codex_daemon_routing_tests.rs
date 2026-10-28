//! Real hook processes against private tmux servers and a fake read-only daemon.
//! No real Codex, model, user config, or live tmux server is used.
use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::Message;

const A: &str = "01a12ada-edfc-75a3-a451-d6312dd93d6d";
const B: &str = "01a12adb-edfc-75a3-a451-d6312dd93d6d";
const COLLISION: &str = "01a12ada-edfc-75a3-a451-d6312dd9ffff";

struct Server {
    root: tempfile::TempDir,
    socket: PathBuf,
    first: String,
    second: String,
}
impl Server {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("sidebar-route.")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = root.path().join("tmux");
        let fake = root.path().join("codex");
        std::fs::copy("/bin/sleep", &fake).unwrap();
        let mut server = Self {
            root,
            socket,
            first: String::new(),
            second: String::new(),
        };
        let cmd = format!("exec {} 120", fake.display());
        server.first = server.read(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "route",
            "-x",
            "100",
            "-y",
            "30",
            "-P",
            "-F",
            "#{pane_id}",
            &cmd,
        ]);
        server.read(&["set-option", "-g", "@sidebar_notifications", "off"]);
        server.second = server.read(&[
            "split-window",
            "-d",
            "-t",
            "route",
            "-P",
            "-F",
            "#{pane_id}",
            &cmd,
        ]);
        // Wait for exec to complete before relying on the real ps snapshot.
        for _ in 0..50 {
            let commands = server.read(&["list-panes", "-a", "-F", "#{pane_current_command}"]);
            if commands.lines().all(|c| c == "codex") {
                return server;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("fake interactive Codex processes did not start");
    }
    fn command(&self, args: &[&str]) -> Output {
        Command::new("tmux")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .arg("-S")
            .arg(&self.socket)
            .args(args)
            .output()
            .unwrap()
    }
    fn read(&self, args: &[&str]) -> String {
        let out = self.command(args);
        assert!(
            out.status.success(),
            "isolated tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn title(&self, pane: &str, title: &str) {
        self.read(&["select-pane", "-t", pane, "-T", title]);
    }
    fn option(&self, pane: &str, key: &str) -> String {
        self.read(&["display-message", "-p", "-t", pane, &format!("#{{{key}}}")])
    }
    fn hook(&self, control: &Path, event: &str, session: &str, turn: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
            .args(["hook", "codex", event])
            .env("TMUX", format!("{},0,0", self.socket.display()))
            // Deliberately stale: routing must never assume this pane owns B.
            .env("TMUX_PANE", &self.first)
            .env("SIDEBAR_CODEX_CONTROL_SOCKET", control)
            .env("TMUX_AGENT_ACTIVITY_DIR", self.root.path())
            .env("XDG_DATA_HOME", self.root.path().join("data"))
            .env("TMPDIR", self.root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = json!({"session_id":session,"turn_id":turn,"source":"startup","cwd":self.root.path(),
            "prompt":"test prompt","last_assistant_message":"done","tool_name":"Bash","tool_input":{"command":"true"}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "hook failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.command(&["kill-session", "-t", "route"]);
    }
}

struct Daemon {
    _root: tempfile::TempDir,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Daemon {
    fn new(ids: Vec<&str>) -> Self {
        let root = tempfile::Builder::new()
            .prefix("sidebar-daemon.")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = root.path().join("rpc");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let ids: Vec<String> = ids.into_iter().map(str::to_owned).collect();
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let Ok(mut ws) = tungstenite::accept(stream) else {
                            continue;
                        };
                        while let Ok(message) = ws.read() {
                            let Message::Text(text) = message else {
                                continue;
                            };
                            let value: Value = serde_json::from_str(&text).unwrap();
                            let method = value["method"].as_str().unwrap();
                            let result = match method {
                                "initialize" => json!({"userAgent":"fixture"}),
                                "initialized" => continue,
                                "account/read" => {
                                    assert_eq!(value["params"], json!({"refreshToken":false}));
                                    json!({"account":{"type":"chatgpt"},"requiresOpenaiAuth":true,"workspaceRouting":{"chatgptAccountId":"fixture-account"}})
                                }
                                "thread/loaded/list" => {
                                    assert_eq!(value["params"], json!({}));
                                    json!({"data":ids,"nextCursor":null})
                                }
                                _ => panic!("unexpected mutating daemon request: {method}"),
                            };
                            if ws
                                .send(Message::text(
                                    json!({"id":value["id"],"result":result}).to_string(),
                                ))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fake daemon accept: {error}"),
                }
            }
        });
        Self {
            _root: root,
            socket,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn stop_response(output: Output) {
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"continue\":true}\n"
    );
}

#[test]
#[ignore = "requires isolated local tmux"]
fn stale_inherited_pane_does_not_override_title_and_late_start_preserves_turn() {
    let tmux = Server::new();
    let daemon = Daemon::new(vec![A, B]);
    tmux.title(&tmux.first, &format!("{}...", &A[..29]));
    tmux.title(&tmux.second, &format!("{}... ⠋", &B[..29]));
    tmux.hook(&daemon.socket, "session-start", B, "");
    assert_eq!(tmux.option(&tmux.first, "@pane_agent"), "");
    assert_eq!(tmux.option(&tmux.second, "@pane_session_id"), B);
    assert_eq!(tmux.option(&tmux.second, "@pane_codex_routing"), "title-v1");
    tmux.hook(&daemon.socket, "user-prompt-submit", B, "turn-b");
    tmux.hook(&daemon.socket, "session-start", B, "");
    assert_eq!(tmux.option(&tmux.second, "@pane_turn_id"), "turn-b");
    stop_response(tmux.hook(&daemon.socket, "stop", B, "turn-b"));
    assert_eq!(
        tmux.option(&tmux.second, "@pane_completed_turn_id"),
        "turn-b"
    );
    assert_eq!(tmux.option(&tmux.first, "@pane_agent"), "");
}

#[test]
#[ignore = "requires isolated local tmux"]
fn fanout_emits_one_stop_response_and_stop_does_not_bootstrap() {
    let tmux = Server::new();
    let unavailable = tmux.root.path().join("absent-control");
    tmux.title(&tmux.first, A);
    tmux.title(&tmux.second, A);
    stop_response(tmux.hook(&unavailable, "stop", A, "t0"));
    assert_eq!(tmux.option(&tmux.first, "@pane_agent"), "");
    assert_eq!(tmux.option(&tmux.second, "@pane_agent"), "");
    tmux.hook(&unavailable, "user-prompt-submit", A, "t1");
    for pane in [&tmux.first, &tmux.second] {
        assert_eq!(tmux.option(pane, "@pane_session_id"), A);
        assert_eq!(tmux.option(pane, "@pane_turn_id"), "t1");
    }
    stop_response(tmux.hook(&unavailable, "stop", A, "t1"));
    for pane in [&tmux.first, &tmux.second] {
        assert_eq!(tmux.option(pane, "@pane_completed_turn_id"), "t1");
    }
    stop_response(tmux.hook(&unavailable, "stop", B, "unseen"));
}

#[test]
#[ignore = "requires isolated local tmux"]
fn permission_and_activity_require_established_title_routed_turn() {
    let tmux = Server::new();
    tmux.title(&tmux.first, A);
    tmux.title(&tmux.second, "no thread");
    let unavailable = tmux.root.path().join("absent");
    for event in ["permission-request", "activity-log"] {
        tmux.hook(&unavailable, event, A, "unknown-turn");
        assert_eq!(tmux.option(&tmux.first, "@pane_agent"), "");
        assert_eq!(tmux.option(&tmux.first, "@pane_wait_reason"), "");
    }
    assert!(!std::fs::read_dir(tmux.root.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("tmux-agent-activity-")
    }));
    // Matching legacy context cannot grant daemon observation rights.
    for (key, value) in [("@pane_session_id", A), ("@pane_turn_id", "legacy-turn")] {
        tmux.read(&["set-option", "-p", "-t", &tmux.first, key, value]);
    }
    for event in ["permission-request", "activity-log", "stop"] {
        tmux.hook(&unavailable, event, A, "legacy-turn");
        assert_eq!(tmux.option(&tmux.first, "@pane_wait_reason"), "");
        assert_eq!(tmux.option(&tmux.first, "@pane_completed_turn_id"), "");
    }
    tmux.read(&["set-option", "-pu", "-t", &tmux.first, "@pane_turn_id"]);
    tmux.hook(&unavailable, "session-start", A, "");
    tmux.hook(&unavailable, "permission-request", A, "unknown-turn");
    assert_eq!(tmux.option(&tmux.first, "@pane_wait_reason"), "");
    tmux.hook(&unavailable, "user-prompt-submit", A, "current-turn");
    let client = tmux.option(&tmux.first, "@pane_codex_client_pid");
    tmux.read(&[
        "set-option",
        "-p",
        "-t",
        &tmux.first,
        "@pane_codex_client_pid",
        "0",
    ]);
    tmux.hook(&unavailable, "permission-request", A, "current-turn");
    assert_eq!(tmux.option(&tmux.first, "@pane_wait_reason"), "");
    tmux.read(&[
        "set-option",
        "-p",
        "-t",
        &tmux.first,
        "@pane_codex_client_pid",
        &client,
    ]);
    tmux.hook(&unavailable, "permission-request", A, "current-turn");
    assert_eq!(tmux.option(&tmux.first, "@pane_wait_reason"), "permission");
    tmux.hook(&unavailable, "activity-log", A, "current-turn");
    assert!(std::fs::read_dir(tmux.root.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("tmux-agent-activity-")
    }));
}

#[test]
#[ignore = "requires isolated local tmux"]
fn colliding_prefixes_and_socket_outage_fail_closed() {
    let tmux = Server::new();
    let daemon = Daemon::new(vec![A, COLLISION]);
    tmux.title(&tmux.first, &format!("{}...", &A[..29]));
    tmux.title(&tmux.second, &format!("{}...", &A[..29]));
    tmux.hook(&daemon.socket, "user-prompt-submit", A, "ambiguous");
    for pane in [&tmux.first, &tmux.second] {
        assert_eq!(tmux.option(pane, "@pane_agent"), "");
    }
    stop_response(tmux.hook(&daemon.socket, "stop", A, "ambiguous"));
    let unavailable = tmux.root.path().join("absent-control");
    tmux.hook(&unavailable, "user-prompt-submit", A, "outage");
    for pane in [&tmux.first, &tmux.second] {
        assert_eq!(tmux.option(pane, "@pane_agent"), "");
    }
    stop_response(tmux.hook(&unavailable, "stop", A, "outage"));
}

#[test]
#[ignore = "requires isolated local tmux"]
fn explicit_server_scope_separates_identical_pane_numbers() {
    let one = Server::new();
    let two = Server::new();
    assert_eq!(one.first, two.first);
    let daemon = Daemon::new(vec![A, B]);
    one.title(&one.first, A);
    two.title(&two.first, B);
    one.title(&one.second, "not a thread");
    two.title(&two.second, "not a thread");
    one.hook(&daemon.socket, "user-prompt-submit", A, "server-one");
    assert_eq!(one.option(&one.first, "@pane_session_id"), A);
    assert_eq!(two.option(&two.first, "@pane_agent"), "");
    two.hook(&daemon.socket, "user-prompt-submit", B, "server-two");
    assert_eq!(two.option(&two.first, "@pane_session_id"), B);
    assert_eq!(one.option(&one.first, "@pane_turn_id"), "server-one");
    stop_response(two.hook(&daemon.socket, "stop", B, "server-two"));
    assert_eq!(one.option(&one.first, "@pane_completed_turn_id"), "");
}

#[test]
#[ignore = "subprocess-only isolated refresh worker"]
fn refresh_worker() {
    let Some(root) = std::env::var_os("SIDEBAR_ROUTING_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.starts_with("/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("sidebar-route.")
    );
    let root = std::fs::canonicalize(root).unwrap();
    let encoded = std::env::var("TMUX").unwrap();
    assert_eq!(
        std::fs::canonicalize(encoded.split(',').next().unwrap()).unwrap(),
        root.join("tmux")
    );
    assert_eq!(
        std::fs::canonicalize(std::env::var_os("TMUX_AGENT_ACTIVITY_DIR").unwrap()).unwrap(),
        root
    );
    let pane = std::env::var("SIDEBAR_ROUTING_TEST_STALE_PANE").unwrap();
    let legacy = std::env::var("SIDEBAR_ROUTING_TEST_LEGACY_PANE").unwrap();
    let activity = tmux_agent_sidebar::activity::log_file_path(&pane);
    std::fs::write(&activity, "12:00|Bash|old activity\n").unwrap();
    let sessions = tmux_agent_sidebar::tmux::query_sessions();
    let panes: Vec<_> = sessions
        .iter()
        .flat_map(|s| &s.windows)
        .flat_map(|w| &w.panes)
        .collect();
    assert!(
        !panes.iter().any(|p| p.pane_id == pane),
        "stale titled binding appeared in projection"
    );
    assert!(
        panes.iter().any(|p| p.pane_id == legacy),
        "legacy markerless client was incorrectly hidden"
    );
    assert!(!activity.exists(), "stale activity must be removed");
}

#[test]
#[ignore = "requires isolated local tmux"]
fn switch_cleanup_clears_projection_and_activity_without_touching_legacy() {
    let tmux = Server::new();
    let absent = tmux.root.path().join("absent");
    tmux.title(&tmux.first, A);
    tmux.hook(&absent, "user-prompt-submit", A, "old-turn");
    tmux.hook(&absent, "activity-log", A, "old-turn");
    stop_response(tmux.hook(&absent, "stop", A, "old-turn"));
    tmux.title(&tmux.first, B);
    // Markerless Codex metadata represents the supported in-process path.
    for (key, value) in [
        ("@pane_agent", "codex"),
        ("@pane_session_id", A),
        ("@pane_prompt", "legacy prompt"),
    ] {
        tmux.read(&["set-option", "-p", "-t", &tmux.second, key, value]);
    }
    tmux.title(&tmux.second, B);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "refresh_worker", "--ignored", "--nocapture"])
        .env("TMUX", format!("{},0,0", tmux.socket.display()))
        .env("TMUX_PANE", &tmux.first)
        .env("SIDEBAR_ROUTING_TEST_ROOT", tmux.root.path())
        .env("SIDEBAR_ROUTING_TEST_STALE_PANE", &tmux.first)
        .env("SIDEBAR_ROUTING_TEST_LEGACY_PANE", &tmux.second)
        .env("TMUX_AGENT_ACTIVITY_DIR", tmux.root.path())
        .env("TMPDIR", tmux.root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "refresh worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for key in [
        "@pane_agent",
        "@pane_session_id",
        "@pane_turn_id",
        "@pane_completed_turn_id",
        "@pane_prompt",
        "@pane_wait_reason",
        "@pane_attention",
        "@pane_subagents",
        "@pane_cwd",
        "@pane_codex_routing",
        "@pane_codex_client_pid",
    ] {
        assert_eq!(
            tmux.option(&tmux.first, key),
            "",
            "stale option {key} survived switch"
        );
    }
    assert_eq!(tmux.option(&tmux.second, "@pane_session_id"), A);
    assert_eq!(tmux.option(&tmux.second, "@pane_prompt"), "legacy prompt");
}

fn guarded_worker_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("SIDEBAR_ROUTING_NATIVE_WORKER_ROOT")?);
    assert!(root.is_absolute());
    assert!(root.starts_with("/tmp"));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("sidebar-route.")
    );
    let canonical = std::fs::canonicalize(&root).unwrap();
    let tmux = std::env::var("TMUX").unwrap();
    assert_eq!(
        std::fs::canonicalize(tmux.split(',').next().unwrap()).unwrap(),
        canonical.join("tmux")
    );
    assert!(std::env::var_os("SIDEBAR_CODEX_CONTROL_SOCKET").is_none());
    Some(root)
}
fn native_worker() {
    let Some(root) = guarded_worker_root() else {
        return;
    };
    let mut outputs = Vec::new();
    for event in ["user-prompt-submit", "stop"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
            .args(["hook", "codex", event])
            .env("XDG_DATA_HOME", root.join("data"))
            .env("SIDEBAR_CODEX_CONTROL_SOCKET", root.join("absent-control"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                json!({"session_id":A,"turn_id":"native-turn","cwd":root,"prompt":"native"})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        let out = child.wait_with_output().unwrap();
        outputs.push(json!({"ok":out.status.success(),"stdout":String::from_utf8(out.stdout).unwrap(),"stderr":String::from_utf8(out.stderr).unwrap()}));
    }
    let pending = root.join("native-result.pending");
    std::fs::write(&pending, json!(outputs).to_string()).unwrap();
    std::fs::rename(pending, root.join("native-result.json")).unwrap();
    // Keep the modeled client alive until the owning fixture cleans its session.
    thread::sleep(Duration::from_secs(5));
}
#[test]
#[ignore = "subprocess-only native TUI ancestry worker"]
fn legacy_tui_worker() {
    native_worker();
}
#[test]
#[ignore = "subprocess-only native exec ancestry worker"]
fn r#exec() {
    native_worker();
}

fn start_native_worker(server: &Server, worker: &str) -> Value {
    let native = server.root.path().join("native");
    std::fs::create_dir(&native).unwrap();
    let binary = native.join("codex");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    let command = format!(
        "exec env -u SIDEBAR_CODEX_CONTROL_SOCKET TMUX={},0,0 TMUX_PANE={} SIDEBAR_ROUTING_NATIVE_WORKER_ROOT={} TMUX_AGENT_ACTIVITY_DIR={} TMPDIR={} {} {} --exact --ignored --nocapture",
        server.socket.display(),
        server.first,
        server.root.path().display(),
        server.root.path().display(),
        server.root.path().display(),
        binary.display(),
        worker
    );
    server.read(&["respawn-pane", "-k", "-t", &server.first, &command]);
    let result = server.root.path().join("native-result.json");
    for _ in 0..250 {
        if let Ok(text) = std::fs::read_to_string(&result) {
            return serde_json::from_str(&text).unwrap();
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "native worker failed: {}",
        server.read(&["capture-pane", "-p", "-t", &server.first])
    );
}

#[test]
#[ignore = "requires isolated local tmux"]
fn native_embedded_ancestry_with_scoped_hooks_migrates_old_title_marker() {
    let tmux = Server::new();
    tmux.title(&tmux.first, A);
    tmux.hook(
        &tmux.root.path().join("absent"),
        "user-prompt-submit",
        A,
        "title-turn",
    );
    assert_eq!(tmux.option(&tmux.first, "@pane_codex_routing"), "title-v1");
    // Same thread is also displayed by an unrelated shared-server client.
    tmux.title(&tmux.first, &format!("{}...", &A[..29]));
    tmux.title(&tmux.second, A);
    let result = start_native_worker(&tmux, "legacy_tui_worker");
    assert_eq!(result[0]["ok"], true);
    assert_eq!(result[1]["stdout"], "{\"continue\":true}\n");
    assert_eq!(tmux.option(&tmux.first, "@pane_turn_id"), "native-turn");
    assert_eq!(
        tmux.option(&tmux.first, "@pane_completed_turn_id"),
        "native-turn"
    );
    assert_eq!(tmux.option(&tmux.first, "@pane_codex_routing"), "");
    assert_eq!(tmux.option(&tmux.first, "@pane_codex_client_pid"), "");
    assert_eq!(tmux.option(&tmux.second, "@pane_agent"), "");
    let observations: Vec<Value> = std::fs::read_dir(
        tmux.root
            .path()
            .join("data/tmux-agent-sidebar/usage.attribution"),
    )
    .unwrap()
    .map(|entry| serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap())
    .filter(|value: &Value| value["turn"] == "native-turn")
    .collect();
    assert_eq!(observations.len(), 2);
    assert!(observations.iter().all(|value| {
        value["session"] == A && value["runtime"] == "embedded" && value["account"].is_null()
    }));
}

#[test]
#[ignore = "requires isolated local tmux"]
fn native_exec_ancestor_cannot_claim_inherited_pane() {
    let tmux = Server::new();
    tmux.title(&tmux.first, A);
    let result = start_native_worker(&tmux, "exec");
    assert_eq!(result[0]["ok"], true);
    assert_eq!(result[1]["stdout"], "{\"continue\":true}\n");
    assert_eq!(tmux.option(&tmux.first, "@pane_agent"), "");
    assert_eq!(tmux.option(&tmux.first, "@pane_session_id"), "");
}

#[test]
#[ignore = "requires isolated local tmux"]
fn contended_pane_lock_preserves_state_and_still_emits_stop_response() {
    use std::os::fd::AsRawFd;
    use std::time::Instant;
    let tmux = Server::new();
    tmux.title(&tmux.first, A);
    tmux.title(&tmux.second, "no thread");
    let unavailable = tmux.root.path().join("absent");
    tmux.hook(&unavailable, "user-prompt-submit", A, "locked-turn");
    // The first real hook creates its secure private directory and lock file.
    let uid = unsafe { libc::geteuid() };
    let lockdir = tmux.root.path().join(format!("tmux-agent-sidebar-{uid}"));
    let locks: Vec<_> = std::fs::read_dir(lockdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(locks.len(), 1);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&locks[0])
        .unwrap();
    // SAFETY: lock owns the live descriptor and this fixture releases it below.
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let start = Instant::now();
    let output = tmux.hook(&unavailable, "stop", A, "locked-turn");
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("lock unavailable"));
    stop_response(output);
    assert_eq!(tmux.option(&tmux.first, "@pane_completed_turn_id"), "");
    assert_eq!(tmux.option(&tmux.first, "@pane_turn_id"), "locked-turn");
    // SAFETY: unlock the same descriptor held by this fixture.
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
}
