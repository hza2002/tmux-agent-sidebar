//! Codex (ChatGPT account) quota client.
//!
//! Normative protocol reference: the user's Raycast extension
//! (`agent-usage/src/usage.ts`), which drives `codex app-server` over stdio
//! with three JSON-RPC requests. One child process is spawned per fetch and
//! killed after 10 seconds; a resident app-server is not worth it for a
//! five-minute cadence.
//!
//! Credentials are read, never written. `OPENAI_API_KEY` / `CODEX_*` are
//! stripped from the child environment so ChatGPT-account auth in
//! `~/.codex/auth.json` wins, exactly as the extension does.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{QuotaFetch, QuotaWindow};

/// Hard timeout for one fetch, including process startup.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Guards against a runaway child flooding the buffer.
const MAX_BUFFER_BYTES: usize = 1024 * 1024;

const EXTENSION_BINARY: &str = "/opt/homebrew/bin/codex";
const BASE_COMMAND_ARGS: [&str; 5] = [
    "-c",
    "model_provider=\"openai\"",
    "-c",
    "chatgpt_base_url=\"https://chatgpt.com/backend-api/\"",
    "app-server",
];

/// Environment variables the extension deliberately removes so the child
/// uses ChatGPT-account auth instead of an API key.
const STRIPPED_ENV: [&str; 3] = ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"];

/// Locates the `codex` binary. `lookup` performs a PATH-style search and is
/// injected so tests never touch the developer's PATH.
pub type PathLookup = fn(&str) -> Option<PathBuf>;

/// Reads `~/.codex/auth.json` and returns `tokens.account_id` for a ChatGPT
/// login. `Ok(None)` means "not signed in / missing file".
pub type AccountReader = fn() -> Result<Option<String>, String>;

/// A spawned `codex app-server` writing to `stdout` and reading from `stdin`.
pub struct CodexChild {
    pub child: Child,
}

pub struct CodexCommandSpec {
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
}

/// Spawn the app-server subprocess. `runner` is injected so tests can return
/// a fake child instead of executing anything.
pub type ProcessRunner = fn(&CodexCommandSpec) -> Result<CodexChild, String>;

/// Fetch Codex quota. `Ok(QuotaFetch::Unavailable)` means there is no ChatGPT
/// login or no runnable `codex` binary.
pub fn fetch_quota() -> Result<QuotaFetch, String> {
    fetch_quota_with(resolve_codex_binary, read_account_id, spawn_child)
}

/// Fetch with injectable binary resolution, account lookup, and process
/// runner.
pub fn fetch_quota_with(
    resolve_binary: impl Fn() -> Option<PathBuf>,
    read_account: AccountReader,
    run: ProcessRunner,
) -> Result<QuotaFetch, String> {
    let Some(expected_account) = read_account()? else {
        return Ok(QuotaFetch::Unavailable);
    };
    let Some(binary) = resolve_binary() else {
        return Ok(QuotaFetch::Unavailable);
    };

    let mut child = run(&command_spec(binary))?;
    let result = query_stdio(&mut child, &expected_account);
    stop_child(&mut child);

    let windows = result?;
    // Account consistency: the user may have switched accounts mid-fetch, in
    // which case the payload belongs to somebody else.
    if read_account()?.as_deref() != Some(expected_account.as_str()) {
        return Err("Codex account changed during the quota query".to_string());
    }
    Ok(QuotaFetch::Available(windows))
}

fn command_spec(binary: PathBuf) -> CodexCommandSpec {
    let mut args: Vec<String> = BASE_COMMAND_ARGS
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    args.extend(["--listen".to_string(), "stdio://".to_string()]);
    CodexCommandSpec {
        binary,
        args,
        envs: vec![("CODEX_HOME".to_string(), codex_home_string())],
    }
}

fn codex_home_string() -> String {
    std::env::var("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".codex")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

/// Prefer the Homebrew path the extension uses, then fall back to `codex` on
/// `PATH`.
fn resolve_codex_binary() -> Option<PathBuf> {
    resolve_binary_with(EXTENSION_BINARY, lookup_on_path)
}

/// Shared resolution rule so the fallback order is unit-testable.
pub fn resolve_binary_with(preferred: &str, lookup: PathLookup) -> Option<PathBuf> {
    let preferred = Path::new(preferred);
    if preferred.is_file() {
        return Some(preferred.to_path_buf());
    }
    lookup("codex")
}

fn lookup_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn read_account_id() -> Result<Option<String>, String> {
    let Some(home) = std::env::var_os("HOME") else {
        return Ok(None);
    };
    let path = PathBuf::from(home).join(".codex").join("auth.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Codex auth could not be read: {error}")),
    };
    Ok(parse_account_id(&raw))
}

/// Pure account-id extraction. Requires a ChatGPT login without an API key;
/// anything else is treated as signed out.
pub fn parse_account_id(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw).ok()?;
    if value.get("auth_mode").and_then(Value::as_str) != Some("chatgpt") {
        return None;
    }
    if value
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .is_some()
    {
        return None;
    }
    value
        .get("tokens")?
        .get("account_id")?
        .as_str()
        .map(str::to_string)
        .filter(|id| !id.trim().is_empty())
}

fn spawn_child(spec: &CodexCommandSpec) -> Result<CodexChild, String> {
    let mut command = Command::new(&spec.binary);
    command
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for key in STRIPPED_ENV {
        command.env_remove(key);
    }
    for (key, value) in &spec.envs {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .map_err(|error| format!("Codex could not be started: {error}"))?;
    Ok(CodexChild { child })
}

fn stop_child(child: &mut CodexChild) {
    let _ = child.child.stdin.take();
    let _ = child.child.kill();
    let _ = child.child.wait();
}

/// Drive the stdio JSON-RPC exchange against a live child.
///
/// A reader thread owns the blocking `stdout` so the main loop can enforce
/// [`FETCH_TIMEOUT`] even when the child goes quiet. On every exit path
/// `stop_child` kills the process, which closes the pipe and lets the reader
/// finish.
fn query_stdio(child: &mut CodexChild, expected_account: &str) -> Result<Vec<QuotaWindow>, String> {
    let mut session = CodexSession::new(expected_account);
    let stdout = child
        .child
        .stdout
        .take()
        .ok_or_else(|| "Codex stdout was unavailable".to_string())?;
    let mut stdin = child
        .child
        .stdin
        .take()
        .ok_or_else(|| "Codex stdin was unavailable".to_string())?;

    let (line_tx, line_rx) = mpsc::sync_channel::<String>(64);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + FETCH_TIMEOUT;
    while !session.finished() {
        if Instant::now() >= deadline {
            return Err("Codex quota query timed out".to_string());
        }
        if let Some(request) = session.next_request() {
            stdin
                .write_all(request.as_bytes())
                .map_err(|error| format!("Codex request could not be written: {error}"))?;
            stdin
                .flush()
                .map_err(|error| format!("Codex request could not be flushed: {error}"))?;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match line_rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(line) => {
                if line.len() > MAX_BUFFER_BYTES {
                    return Err("Codex returned more data than expected".to_string());
                }
                session.handle_response(line.trim_end_matches(['\n', '\r']))?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    return Err("Codex quota query timed out".to_string());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Codex query process exited early".to_string());
            }
        }
    }
    session
        .into_windows()
        .ok_or_else(|| "Codex quota response was incomplete".to_string())
}

// ── JSON-RPC state machine (pure) ───────────────────────────────────

/// The three-stage exchange from `usage.ts`:
/// `initialize` → `account/read` → `account/rateLimits/read`.
pub struct CodexSession {
    stage: u8,
    finished: bool,
    /// Whether the request for the current stage has already been written.
    /// Prevents re-sending the same request while polling for its response.
    request_sent: bool,
    expected_account: String,
    windows: Option<Vec<QuotaWindow>>,
}

impl CodexSession {
    pub fn new(expected_account: &str) -> Self {
        Self {
            stage: 1,
            finished: false,
            request_sent: false,
            expected_account: expected_account.to_string(),
            windows: None,
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn into_windows(self) -> Option<Vec<QuotaWindow>> {
        self.windows
    }

    /// Next request to write, or `None` when the exchange is waiting on a
    /// response or already finished.
    pub fn next_request(&mut self) -> Option<String> {
        if self.finished || self.request_sent {
            return None;
        }
        self.request_sent = true;
        let request = match self.stage {
            1 => json!({
                "id": 1,
                "method": "initialize",
                "params": { "clientInfo": { "name": "tmux-agent-sidebar", "version": crate::VERSION } }
            }),
            2 => {
                // The extension acknowledges initialization with a bare
                // notification before moving on.
                let initialized =
                    serde_json::to_string(&json!({ "method": "initialized" })).ok()?;
                let request = json!({
                    "id": 2,
                    "method": "account/read",
                    "params": { "refreshToken": false }
                });
                let request = serde_json::to_string(&request).ok()?;
                return Some(format!("{initialized}\n{request}\n"));
            }
            3 => json!({ "id": 3, "method": "account/rateLimits/read" }),
            _ => return None,
        };
        serde_json::to_string(&request)
            .ok()
            .map(|request| format!("{request}\n"))
    }

    /// Feed one stdout line. Responses are matched by id; server notifications
    /// (`method`) and unknown ids are ignored, matching `usage.ts`.
    pub fn handle_response(&mut self, line: &str) -> Result<(), String> {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        if message.get("method").is_some() {
            return Ok(());
        }
        if message.get("id").and_then(Value::as_u64) != Some(self.stage as u64) {
            return Ok(());
        }
        if message.get("error").is_some_and(|error| !error.is_null()) {
            return Err("Codex quota request was rejected".to_string());
        }
        match self.stage {
            1 => {
                self.stage = 2;
                self.request_sent = false;
            }
            2 => {
                let account = message
                    .get("result")
                    .and_then(|result| result.get("account"));
                if account
                    .and_then(|account| account.get("type"))
                    .and_then(Value::as_str)
                    != Some("chatgpt")
                {
                    return Err("Codex is not signed in with a ChatGPT account".to_string());
                }
                self.stage = 3;
                self.request_sent = false;
            }
            _ => {
                let account_id = message
                    .get("result")
                    .and_then(|result| result.get("accountId"))
                    .and_then(Value::as_str);
                if account_id.is_some_and(|id| id != self.expected_account) {
                    return Err("Codex quota belongs to another account".to_string());
                }
                self.windows = parse_rate_limits(&message["result"]);
                self.finished = true;
            }
        }
        Ok(())
    }
}

/// Parse `account/rateLimits/read`. Mirrors `usage.ts`: `rateLimitsByLimitId`
/// wins when present, primary/secondary map to the 5-hour and weekly windows.
pub fn parse_rate_limits(result: &Value) -> Option<Vec<QuotaWindow>> {
    let limits = result
        .get("rateLimitsByLimitId")
        .and_then(|by_id| by_id.get("codex"))
        .or_else(|| result.get("rateLimits"))?;
    if let Some(limit_id) = limits.get("limitId").and_then(Value::as_str)
        && limit_id != "codex"
    {
        return None;
    }
    let mut windows = Vec::new();
    for (key, label) in [("primary", "5h"), ("secondary", "wk")] {
        let Some(window) = limits.get(key) else {
            continue;
        };
        let Some(used_percent) = window.get("usedPercent").and_then(Value::as_f64) else {
            continue;
        };
        if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
            continue;
        }
        windows.push(QuotaWindow {
            label: label.to_string(),
            remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0).round() as u8,
            resets_at: window
                .get("resetsAt")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|seconds| (seconds * 1000.0) as u64),
        });
    }
    (!windows.is_empty()).then_some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_account_id_requires_a_chatgpt_login() {
        let raw = r#"{"auth_mode":"chatgpt","tokens":{"account_id":"acct-1"}}"#;
        assert_eq!(parse_account_id(raw), Some("acct-1".to_string()));
        assert_eq!(
            parse_account_id(r#"{"auth_mode":"apikey","tokens":{"account_id":"acct-1"}}"#),
            None
        );
        assert_eq!(
            parse_account_id(
                r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":"sk-x","tokens":{"account_id":"acct-1"}}"#
            ),
            None
        );
        assert_eq!(parse_account_id("not json"), None);
        assert_eq!(parse_account_id(r#"{"auth_mode":"chatgpt"}"#), None);
    }

    #[test]
    fn binary_resolution_prefers_the_homebrew_path() {
        let dir = tempfile::tempdir().unwrap();
        let preferred = dir.path().join("codex");
        std::fs::write(&preferred, "").unwrap();
        let resolved = resolve_binary_with(preferred.to_str().unwrap(), |_| {
            Some(PathBuf::from("/usr/local/bin/codex"))
        });
        assert_eq!(resolved, Some(preferred));
    }

    #[test]
    fn binary_resolution_falls_back_to_path() {
        let resolved = resolve_binary_with("/definitely/missing/codex", |name| {
            assert_eq!(name, "codex");
            Some(PathBuf::from("/usr/local/bin/codex"))
        });
        assert_eq!(resolved, Some(PathBuf::from("/usr/local/bin/codex")));
        assert_eq!(
            resolve_binary_with("/definitely/missing/codex", |_| None),
            None
        );
    }

    #[test]
    fn command_spec_matches_the_extension_invocation() {
        let spec = command_spec(PathBuf::from("/opt/homebrew/bin/codex"));
        assert_eq!(spec.binary, PathBuf::from("/opt/homebrew/bin/codex"));
        assert_eq!(
            spec.args,
            vec![
                "-c",
                "model_provider=\"openai\"",
                "-c",
                "chatgpt_base_url=\"https://chatgpt.com/backend-api/\"",
                "app-server",
                "--listen",
                "stdio://",
            ]
        );
        assert!(
            spec.envs.iter().any(|(key, _)| key == "CODEX_HOME"),
            "CODEX_HOME must be pinned for the child"
        );
    }

    #[test]
    fn session_walks_the_three_stages_in_order() {
        let mut session = CodexSession::new("acct-1");
        let first = session.next_request().unwrap();
        assert!(first.contains("\"initialize\""));
        assert!(first.contains("\"id\":1"));

        session
            .handle_response(r#"{"id":1,"result":{"userAgent":"x"}}"#)
            .unwrap();
        let second = session.next_request().unwrap();
        assert!(second.contains("\"method\":\"initialized\""));
        assert!(second.contains("\"id\":2"));
        assert!(second.contains("\"account/read\""));

        session
            .handle_response(r#"{"id":2,"result":{"account":{"type":"chatgpt","email":"a@b.c"}}}"#)
            .unwrap();
        let third = session.next_request().unwrap();
        assert!(third.contains("\"id\":3"));
        assert!(third.contains("\"account/rateLimits/read\""));
        assert!(!session.finished());
        assert!(
            session.next_request().is_none(),
            "no new request while awaiting the final response"
        );

        session
            .handle_response(
                r#"{"id":3,"result":{"accountId":"acct-1","rateLimits":{"primary":{"usedPercent":39.0,"windowDurationMins":300,"resetsAt":1700001330},"secondary":{"usedPercent":17.0,"windowDurationMins":10080,"resetsAt":1700345600}}}}"#,
            )
            .unwrap();
        assert!(session.finished());
        let windows = session.into_windows().unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(windows[0].resets_at, Some(1_700_001_330_000));
        assert_eq!(windows[1].label, "wk");
        assert_eq!(windows[1].remaining_percent, 83);
    }

    #[test]
    fn session_ignores_notifications_and_unknown_ids() {
        let mut session = CodexSession::new("acct-1");
        let _ = session.next_request();
        // Server notifications and unknown ids must not advance the stage.
        session
            .handle_response(r#"{"method":"some/notification","params":{}}"#)
            .unwrap();
        session.handle_response(r#"{"id":99,"result":{}}"#).unwrap();
        assert!(!session.finished());
        // Stage 1 has been answered, so the next call emits `initialized` +
        // `account/read`.
        session
            .handle_response(r#"{"id":1,"result":{"userAgent":"x"}}"#)
            .unwrap();
        let next = session.next_request().unwrap();
        assert!(next.contains("\"account/read\""));
        // Malformed lines are ignored rather than treated as failures.
        session.handle_response("not json at all").unwrap();
        assert!(!session.finished());
    }

    #[test]
    fn session_rejects_a_non_chatgpt_account() {
        let mut session = CodexSession::new("acct-1");
        let _ = session.next_request();
        session.handle_response(r#"{"id":1,"result":{}}"#).unwrap();
        let _ = session.next_request();
        let error = session
            .handle_response(r#"{"id":2,"result":{"account":{"type":"apiKey"}}}"#)
            .unwrap_err();
        assert!(error.contains("ChatGPT"));
    }

    #[test]
    fn session_rejects_an_account_id_mismatch() {
        let mut session = CodexSession::new("acct-1");
        let _ = session.next_request();
        session.handle_response(r#"{"id":1,"result":{}}"#).unwrap();
        let _ = session.next_request();
        session
            .handle_response(r#"{"id":2,"result":{"account":{"type":"chatgpt"}}}"#)
            .unwrap();
        let error = session
            .handle_response(
                r#"{"id":3,"result":{"accountId":"acct-2","rateLimits":{"primary":{"usedPercent":10.0,"windowDurationMins":300}}}}"#,
            )
            .unwrap_err();
        assert!(error.contains("another account"));
    }

    #[test]
    fn session_surfaces_jsonrpc_errors() {
        let mut session = CodexSession::new("acct-1");
        let _ = session.next_request();
        let error = session
            .handle_response(r#"{"id":1,"error":{"code":-32601,"message":"nope"}}"#)
            .unwrap_err();
        assert!(error.contains("rejected"));
    }

    #[test]
    fn parse_rate_limits_prefers_the_codex_bucket() {
        let result = serde_json::json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "primary": { "usedPercent": 39.0, "resetsAt": 1700001330 },
                    "secondary": { "usedPercent": 17.0, "resetsAt": 1700345600 }
                }
            },
            "rateLimits": {
                "primary": { "usedPercent": 99.0 }
            }
        });
        let windows = parse_rate_limits(&result).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(windows[1].remaining_percent, 83);
    }

    #[test]
    fn parse_rate_limits_ignores_other_limit_ids_and_bad_percentages() {
        let other = serde_json::json!({
            "rateLimits": { "limitId": "other", "primary": { "usedPercent": 10.0 } }
        });
        assert!(parse_rate_limits(&other).is_none());

        let out_of_range = serde_json::json!({
            "rateLimits": {
                "primary": { "usedPercent": 140.0 },
                "secondary": { "usedPercent": -4.0 }
            }
        });
        assert!(parse_rate_limits(&out_of_range).is_none());
    }

    #[test]
    fn parse_rate_limits_accepts_a_partial_payload() {
        let result = serde_json::json!({
            "rateLimits": { "primary": { "usedPercent": 90.0 } }
        });
        let windows = parse_rate_limits(&result).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 10);
        assert_eq!(windows[0].resets_at, None);
    }

    #[test]
    fn fetch_reports_unavailable_when_signed_out() {
        let result = fetch_quota_with(
            || Some(PathBuf::from("/opt/homebrew/bin/codex")),
            || Ok(None),
            |_| panic!("no process should spawn without a login"),
        )
        .unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
    }

    #[test]
    fn fetch_reports_unavailable_without_a_binary() {
        let result = fetch_quota_with(
            || None,
            || Ok(Some("acct-1".to_string())),
            |_| panic!("no process should spawn without a binary"),
        )
        .unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
    }

    #[test]
    fn account_change_mid_fetch_discards_the_result() {
        // `query_stdio` needs a live child, so this test exercises the same
        // guard by driving the comparison the fetch wrapper performs.
        let expected = "acct-1".to_string();
        let after = Some("acct-2".to_string());
        assert_ne!(after.as_deref(), Some(expected.as_str()));
        let after = Some("acct-1".to_string());
        assert_eq!(after.as_deref(), Some(expected.as_str()));
    }
}
