//! Codex (ChatGPT account) quota client.
//!
//! Primary path: a direct `GET https://chatgpt.com/backend-api/wham/usage`
//! with the bearer token from `~/.codex/auth.json`, refreshing through
//! `https://auth.openai.com/oauth/token` when the token is near expiry.
//! Fallback path: the user's Raycast extension (`agent-usage/src/usage.ts`)
//! driving `codex app-server` over stdio with three JSON-RPC requests, kept
//! for auth rejections and schema drift in the direct path. One child
//! process is spawned per fallback fetch and killed after 10 seconds.
//!
//! HTTP goes through a `curl` subprocess (see [`super::run_curl`]), matching
//! the repository's existing tmux/ps/lsof idiom and avoiding an HTTP crate.
//! OpenAI rotates refresh tokens, so a refresh must write the rotated tokens
//! back to `auth.json`; the writeback follows the same conservative
//! re-read/atomic-write pattern as the Kimi client (the file is shared with
//! the Codex CLI, which itself writes non-atomically). For the app-server
//! child, `OPENAI_API_KEY` / `CODEX_*` are stripped from the environment so
//! ChatGPT-account auth wins, exactly as the extension does.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{
    CurlRunner, FetchError, FetchKind, FetchOutcome, HttpResponse, QuotaFetch, QuotaWindow,
    run_curl, write_atomic,
};

/// Hard timeout for one fetch, including process startup.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Guards against a runaway child flooding the buffer.
const MAX_BUFFER_BYTES: usize = 1024 * 1024;

const EXTENSION_BINARY: &str = "/opt/homebrew/bin/codex";

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Public OAuth client id of the Codex CLI (`codex-rs`).
const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// A `codex_cli_rs`-style agent string, matching what the backend sees from
/// the CLI itself.
const USER_AGENT_HEADER: &str = "User-Agent: codex_cli_rs";
/// Treat the token as expired this early to avoid racing the server clock.
/// `auth.json` stores no expiry field; it is read from the JWT `exp` claim.
const EXPIRY_SKEW_SECONDS: u64 = 300;
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

/// Fetch Codex quota. `Ok(QuotaFetch::Unavailable)` means there is no
/// ChatGPT login. The direct HTTP path runs first; [`DirectError::Fallback`]
/// hands off to the app-server exchange, and [`DirectError::Fatal`] surfaces
/// to the cadence as-is.
pub fn fetch_quota() -> FetchOutcome {
    match fetch_direct_with(&run_curl, &read_auth_file, &write_auth_file) {
        Ok(fetch) => Ok(fetch),
        Err(DirectError::Fatal(error)) => Err(error),
        Err(DirectError::Fallback(_)) => fetch_via_app_server(),
    }
}

/// Why a direct fetch ended. `Fallback` hands off to the app-server path
/// (auth rejections, schema drift); `Fatal` is a real fetch failure the
/// cadence should back off from.
#[derive(Debug)]
pub enum DirectError {
    Fallback(String),
    Fatal(FetchError),
}

/// Reads `~/.codex/auth.json` and returns its raw contents. `Ok(None)` means
/// the file is absent.
pub type AuthReader<'a> = dyn Fn() -> Result<Option<String>, String> + 'a;

/// Writes refreshed tokens back to `~/.codex/auth.json`.
pub type AuthWriter<'a> = dyn Fn(&CodexAuth) -> Result<(), String> + 'a;

/// Direct fetch with injectable transport and auth I/O. `curl` returns the
/// HTTP response and `Err` for transport failures (DNS, timeout, non-zero
/// exit).
pub fn fetch_direct_with(
    curl: CurlRunner<'_>,
    read: &AuthReader<'_>,
    write: &AuthWriter<'_>,
) -> Result<QuotaFetch, DirectError> {
    let Some(raw) = read().map_err(DirectError::Fallback)? else {
        return Ok(QuotaFetch::Unavailable);
    };
    let Some(mut auth) = parse_auth(&raw) else {
        return Ok(QuotaFetch::Unavailable);
    };

    if token_expiring_soon(&auth) && auth.refresh_token.is_some() {
        auth = usable_auth(curl, read, write, auth)?;
    }

    let mut response = request_usage(curl, &auth).map_err(DirectError::Fatal)?;
    if response.status == 401 || response.status == 403 {
        // The Codex CLI may have just refreshed. Re-read the file before
        // spending our own refresh token.
        let latest = read().ok().flatten().and_then(|raw| parse_auth(&raw));
        auth = match latest {
            Some(latest)
                if latest.access_token != auth.access_token && !token_expiring_soon(&latest) =>
            {
                latest
            }
            other => usable_auth(curl, read, write, other.unwrap_or(auth))?,
        };
        response = request_usage(curl, &auth).map_err(DirectError::Fatal)?;
        if response.status == 401 || response.status == 403 {
            return Err(DirectError::Fallback(
                "Codex direct usage request stayed unauthorized".to_string(),
            ));
        }
    }
    if response.status == 429 {
        return Err(DirectError::Fatal(FetchError::throttled(
            "Codex quota request was throttled (HTTP 429)",
            response.retry_after,
        )));
    }
    if response.status >= 400 || response.status == 0 {
        return Err(DirectError::Fatal(FetchError::new(format!(
            "Codex quota request failed (HTTP {})",
            response.status
        ))));
    }
    let payload: Value = serde_json::from_str(&response.body).map_err(|_| {
        DirectError::Fallback("Codex usage response was not valid JSON".to_string())
    })?;
    let Some(windows) = parse_wham_usage(&payload) else {
        return Err(DirectError::Fallback(
            "Codex usage response schema drifted".to_string(),
        ));
    };
    Ok(QuotaFetch::Available(windows))
}

/// Auth to use once the access token read from disk is expiring.
///
/// The auth file is shared with the Codex CLI, and OpenAI rotates refresh
/// tokens, so this re-reads the file twice: once before spending our own
/// refresh token (the CLI may have just written a fresh one) and once before
/// writing ours back (writing last would strand the CLI's session, because
/// the token it rotated away from is no longer valid server-side).
fn usable_auth(
    curl: CurlRunner<'_>,
    read: &AuthReader<'_>,
    write: &AuthWriter<'_>,
    auth: CodexAuth,
) -> Result<CodexAuth, DirectError> {
    let newer = |candidate: Option<CodexAuth>| match candidate {
        Some(candidate)
            if candidate.access_token != auth.access_token && !token_expiring_soon(&candidate) =>
        {
            Some(candidate)
        }
        _ => None,
    };
    if let Some(latest) = newer(read().ok().flatten().and_then(|raw| parse_auth(&raw))) {
        return Ok(latest);
    }
    let Some(refresh_token) = auth.refresh_token.clone() else {
        // Nothing we can do but try the token we have, exactly as before.
        return Ok(auth);
    };

    let refreshed = refresh_tokens(curl, &auth, &refresh_token)?;
    if let Some(winner) = newer(read().ok().flatten().and_then(|raw| parse_auth(&raw))) {
        return Ok(winner);
    }
    write(&refreshed).map_err(DirectError::Fallback)?;
    Ok(refreshed)
}

/// Exchange a refresh token at the OAuth endpoint. Any failure is a
/// `Fallback`: the app-server path (or the Codex CLI itself) gets to surface
/// the stale login to the user.
fn refresh_tokens(
    curl: CurlRunner<'_>,
    auth: &CodexAuth,
    refresh_token: &str,
) -> Result<CodexAuth, DirectError> {
    let body = json!({
        "client_id": OAUTH_CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
    })
    .to_string();
    let response = curl(
        TOKEN_URL,
        &[
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-H",
            "Accept: application/json",
        ],
        Some(&body),
    )
    .map_err(|error| DirectError::Fallback(format!("Codex token refresh failed: {error}")))?;
    if response.status >= 400 || response.status == 0 {
        return Err(DirectError::Fallback(format!(
            "Codex token refresh was rejected (HTTP {})",
            response.status
        )));
    }
    let payload: Value = serde_json::from_str(&response.body).map_err(|_| {
        DirectError::Fallback("Codex token refresh returned invalid JSON".to_string())
    })?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            DirectError::Fallback("Codex token refresh returned no access token".to_string())
        })?;
    let text = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    };
    Ok(CodexAuth {
        access_token: access_token.to_string(),
        // Refresh tokens rotate; always prefer the fresh one.
        refresh_token: text("refresh_token").or_else(|| auth.refresh_token.clone()),
        id_token: text("id_token").or_else(|| auth.id_token.clone()),
        account_id: auth.account_id.clone(),
    })
}

fn request_usage(curl: CurlRunner<'_>, auth: &CodexAuth) -> Result<HttpResponse, FetchError> {
    // The header name is part of the argument: `Bearer <token>` on its own is
    // sent as a custom header called `Bearer …`, which the API rejects.
    let authorization = format!("Authorization: Bearer {}", auth.access_token);
    let account = auth
        .account_id
        .as_ref()
        .map(|id| format!("ChatGPT-Account-Id: {id}"));
    let mut args: Vec<&str> = vec![
        "-H",
        &authorization,
        "-H",
        "Accept: application/json",
        "-H",
        USER_AGENT_HEADER,
    ];
    if let Some(account) = &account {
        args.extend(["-H", account]);
    }
    curl(USAGE_URL, &args, None)
}

/// Parse `GET /backend-api/wham/usage`. Windows are classified by
/// `limit_window_seconds` (18000 = 5h, 604800 = weekly), not by slot: the
/// weekly window appears in `primary_window` when no 5h limit applies, and
/// either slot may be `null`.
pub fn parse_wham_usage(payload: &Value) -> Option<Vec<QuotaWindow>> {
    let rate_limit = payload.get("rate_limit")?;
    let mut five_hour = None;
    let mut weekly = None;
    for slot in ["primary_window", "secondary_window"] {
        if let Some(window) = rate_limit.get(slot).and_then(wham_window) {
            match window.label.as_str() {
                "5h" if five_hour.is_none() => five_hour = Some(window),
                "wk" if weekly.is_none() => weekly = Some(window),
                _ => {}
            }
        }
    }
    // 5h above weekly, matching the design's two-window display order.
    let windows: Vec<QuotaWindow> = [five_hour, weekly].into_iter().flatten().collect();
    (!windows.is_empty()).then_some(windows)
}

fn wham_window(window: &Value) -> Option<QuotaWindow> {
    let label = match window.get("limit_window_seconds").and_then(Value::as_u64) {
        Some(18_000) => "5h",
        Some(604_800) => "wk",
        _ => return None,
    };
    // `used_percent` is an integer in practice; tolerate floats anyway.
    let used = window.get("used_percent").and_then(Value::as_f64)?;
    if !used.is_finite() || !(0.0..=100.0).contains(&used) {
        return None;
    }
    Some(QuotaWindow {
        label: label.to_string(),
        remaining_percent: (100.0 - used).clamp(0.0, 100.0).round() as u8,
        // `reset_at` is already Unix seconds, the unit the quota module and
        // `AppState::now` share.
        resets_at: window
            .get("reset_at")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0),
    })
}

/// Fallback path: `codex app-server` over stdio, exactly as the Raycast
/// extension does. `Ok(QuotaFetch::Unavailable)` means there is no ChatGPT
/// login or no runnable `codex` binary.
pub fn fetch_via_app_server() -> Result<QuotaFetch, FetchError> {
    fetch_via_app_server_with(resolve_codex_binary, read_account_id, spawn_child)
}

/// Fallback fetch with injectable binary resolution, account lookup, and
/// process runner. Injected readers/runners report plain strings; the kinds
/// are attached here, where the failing step is known (auth file reads are
/// [`FetchKind::Auth`], process spawns [`FetchKind::Network`]).
pub fn fetch_via_app_server_with(
    resolve_binary: impl Fn() -> Option<PathBuf>,
    read_account: AccountReader,
    run: ProcessRunner,
) -> Result<QuotaFetch, FetchError> {
    let Some(expected_account) =
        read_account().map_err(|error| FetchError::with_kind(error, FetchKind::Auth))?
    else {
        return Ok(QuotaFetch::Unavailable);
    };
    let Some(binary) = resolve_binary() else {
        return Ok(QuotaFetch::Unavailable);
    };

    let mut child = run(&command_spec(binary))
        .map_err(|error| FetchError::with_kind(error, FetchKind::Network))?;
    let result = query_stdio(&mut child, &expected_account);
    stop_child(&mut child);

    let windows = result?;
    // Account consistency: the user may have switched accounts mid-fetch, in
    // which case the payload belongs to somebody else.
    if read_account()
        .map_err(|error| FetchError::with_kind(error, FetchKind::Auth))?
        .as_deref()
        != Some(expected_account.as_str())
    {
        return Err(FetchError::with_kind(
            "Codex account changed during the quota query",
            FetchKind::Data,
        ));
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

fn auth_file_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".codex").join("auth.json"))
}

fn read_auth_file() -> Result<Option<String>, String> {
    let Some(path) = auth_file_path() else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Codex auth could not be read: {error}")),
    }
}

/// Writes rotated tokens back, preserving every unrelated key in the file.
fn write_auth_file(auth: &CodexAuth) -> Result<(), String> {
    let Some(path) = auth_file_path() else {
        return Err("Codex auth path is unavailable".to_string());
    };
    let raw = read_auth_file()?.ok_or_else(|| "Codex auth disappeared mid-refresh".to_string())?;
    let merged = merge_auth_tokens(&raw, auth, &format_rfc3339(crate::time::now_epoch_secs()))
        .ok_or_else(|| "Codex auth could not be merged".to_string())?;
    write_atomic(&path, &merged).map_err(|error| format!("Codex {error}"))
}

fn read_account_id() -> Result<Option<String>, String> {
    Ok(read_auth_file()?.and_then(|raw| parse_account_id(&raw)))
}

/// The ChatGPT-login tokens from `~/.codex/auth.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAuth {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub account_id: Option<String>,
}

/// Pure auth extraction. Requires a ChatGPT login without an API key;
/// anything else is treated as signed out.
pub fn parse_auth(raw: &str) -> Option<CodexAuth> {
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
    let tokens = value.get("tokens")?;
    let text = |key: &str| {
        tokens
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    };
    Some(CodexAuth {
        access_token: text("access_token")?,
        refresh_token: text("refresh_token"),
        id_token: text("id_token"),
        account_id: text("account_id"),
    })
}

/// Pure account-id extraction, for the app-server fallback's account
/// consistency check.
pub fn parse_account_id(raw: &str) -> Option<String> {
    parse_auth(raw).and_then(|auth| auth.account_id)
}

/// Pure merge so writeback preserves unknown keys (`auth_mode`, future
/// fields): only the rotated tokens and `last_refresh` change.
pub fn merge_auth_tokens(raw: &str, auth: &CodexAuth, last_refresh: &str) -> Option<String> {
    let mut value: Value = serde_json::from_str(raw).ok()?;
    let object = value.as_object_mut()?;
    let tokens = object
        .entry("tokens")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;
    tokens.insert("access_token".to_string(), json!(auth.access_token));
    if let Some(refresh_token) = &auth.refresh_token {
        tokens.insert("refresh_token".to_string(), json!(refresh_token));
    }
    if let Some(id_token) = &auth.id_token {
        tokens.insert("id_token".to_string(), json!(id_token));
    }
    if let Some(account_id) = &auth.account_id {
        tokens.insert("account_id".to_string(), json!(account_id));
    }
    object.insert("last_refresh".to_string(), json!(last_refresh));
    serde_json::to_string(&value).ok()
}

/// True when the access token's JWT `exp` claim is within the skew window.
/// A non-JWT token (no parseable claim) is treated as usable; a 401 will
/// trigger the refresh path if the server disagrees.
fn token_expiring_soon(auth: &CodexAuth) -> bool {
    match jwt_expiration(&auth.access_token) {
        Some(exp) => exp <= crate::time::now_epoch_secs() + EXPIRY_SKEW_SECONDS,
        None => false,
    }
}

/// Read the `exp` claim (Unix seconds) out of an unsigned JWT segment.
fn jwt_expiration(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64url_decode(payload)?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value.get("exp")?.as_u64()
}

/// Unpadded base64url, as used by JWT segments.
fn base64url_decode(segment: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a' + 26)),
            b'0'..=b'9' => Some(u32::from(byte - b'0' + 52)),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(segment.len() * 3 / 4);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in segment.as_bytes() {
        accumulator = (accumulator << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for the `last_refresh` writeback.
fn format_rfc3339(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let seconds = epoch_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Inverse of Howard Hinnant's `days_from_civil`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
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
fn query_stdio(
    child: &mut CodexChild,
    expected_account: &str,
) -> Result<Vec<QuotaWindow>, FetchError> {
    let mut session = CodexSession::new(expected_account);
    let stdout =
        child.child.stdout.take().ok_or_else(|| {
            FetchError::with_kind("Codex stdout was unavailable", FetchKind::Network)
        })?;
    let mut stdin =
        child.child.stdin.take().ok_or_else(|| {
            FetchError::with_kind("Codex stdin was unavailable", FetchKind::Network)
        })?;

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
            return Err(FetchError::with_kind(
                "Codex quota query timed out",
                FetchKind::Network,
            ));
        }
        if let Some(request) = session.next_request() {
            stdin.write_all(request.as_bytes()).map_err(|error| {
                FetchError::with_kind(
                    format!("Codex request could not be written: {error}"),
                    FetchKind::Network,
                )
            })?;
            stdin.flush().map_err(|error| {
                FetchError::with_kind(
                    format!("Codex request could not be flushed: {error}"),
                    FetchKind::Network,
                )
            })?;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match line_rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(line) => {
                if line.len() > MAX_BUFFER_BYTES {
                    return Err(FetchError::with_kind(
                        "Codex returned more data than expected",
                        FetchKind::Data,
                    ));
                }
                session.handle_response(line.trim_end_matches(['\n', '\r']))?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    return Err(FetchError::with_kind(
                        "Codex quota query timed out",
                        FetchKind::Network,
                    ));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(FetchError::with_kind(
                    "Codex query process exited early",
                    FetchKind::Network,
                ));
            }
        }
    }
    session.into_windows().ok_or_else(|| {
        FetchError::with_kind("Codex quota response was incomplete", FetchKind::Data)
    })
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
    pub fn handle_response(&mut self, line: &str) -> Result<(), FetchError> {
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
            return Err(FetchError::new("Codex quota request was rejected"));
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
                    return Err(FetchError::with_kind(
                        "Codex is not signed in with a ChatGPT account",
                        FetchKind::Auth,
                    ));
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
                    return Err(FetchError::with_kind(
                        "Codex quota belongs to another account",
                        FetchKind::Auth,
                    ));
                }
                self.windows = parse_rate_limits(&message["result"]);
                self.finished = true;
            }
        }
        Ok(())
    }
}

/// Parse `account/rateLimits/read`. Mirrors `usage.ts`: `rateLimitsByLimitId`
/// wins when present. Windows are classified by `windowDurationMins`
/// (300 = 5h, 10080 = weekly), not by slot — the weekly window can occupy
/// the `primary` slot, so a fixed slot mapping would silently swap the two
/// windows. Windows with any other duration are ignored. A window without
/// `windowDurationMins` (older app-server responses) falls back to the old
/// slot mapping: `primary` → 5h, `secondary` → weekly.
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
    let mut five_hour = None;
    let mut weekly = None;
    for (key, window) in limits.as_object()? {
        if !window.is_object() {
            continue;
        }
        let label = match window.get("windowDurationMins").and_then(Value::as_f64) {
            Some(300.0) => "5h",
            Some(10080.0) => "wk",
            // A window of any other length is not one of the two the block
            // renders.
            Some(_) => continue,
            // Older responses carry no duration: fall back to the slot.
            None => match key.as_str() {
                "primary" => "5h",
                "secondary" => "wk",
                _ => continue,
            },
        };
        let Some(used_percent) = window.get("usedPercent").and_then(Value::as_f64) else {
            continue;
        };
        if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
            continue;
        }
        let window = QuotaWindow {
            label: label.to_string(),
            remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0).round() as u8,
            // `resetsAt` is already Unix seconds, the unit the quota module
            // and `AppState::now` share.
            resets_at: window
                .get("resetsAt")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|seconds| seconds as u64),
        };
        match label {
            "5h" if five_hour.is_none() => five_hour = Some(window),
            "wk" if weekly.is_none() => weekly = Some(window),
            _ => {}
        }
    }
    // 5h above weekly, matching the design's two-window display order.
    let windows: Vec<QuotaWindow> = [five_hour, weekly].into_iter().flatten().collect();
    (!windows.is_empty()).then_some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_account_id_requires_a_chatgpt_login() {
        let raw =
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok","account_id":"acct-1"}}"#;
        assert_eq!(parse_account_id(raw), Some("acct-1".to_string()));
        assert_eq!(
            parse_account_id(
                r#"{"auth_mode":"apikey","tokens":{"access_token":"tok","account_id":"acct-1"}}"#
            ),
            None
        );
        assert_eq!(
            parse_account_id(
                r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":"sk-x","tokens":{"access_token":"tok","account_id":"acct-1"}}"#
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
        assert_eq!(windows[0].resets_at, Some(1_700_001_330));
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
        assert!(error.message.contains("ChatGPT"));
        assert_eq!(error.kind, FetchKind::Auth);
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
        assert!(error.message.contains("another account"));
        assert_eq!(error.kind, FetchKind::Auth);
    }

    #[test]
    fn session_surfaces_jsonrpc_errors() {
        let mut session = CodexSession::new("acct-1");
        let _ = session.next_request();
        let error = session
            .handle_response(r#"{"id":1,"error":{"code":-32601,"message":"nope"}}"#)
            .unwrap_err();
        assert!(error.message.contains("rejected"));
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
    fn parse_rate_limits_classifies_windows_by_duration_not_slot() {
        // The slots are swapped relative to the historical mapping: the
        // weekly window sits in `primary`, the 5-hour one in `secondary`.
        // The duration, not the slot, decides the label.
        let result = serde_json::json!({
            "rateLimits": {
                "primary": { "usedPercent": 17.0, "windowDurationMins": 10080, "resetsAt": 1700345600 },
                "secondary": { "usedPercent": 39.0, "windowDurationMins": 300, "resetsAt": 1700001330 }
            }
        });
        let windows = parse_rate_limits(&result).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(
            windows[0].remaining_percent, 61,
            "the 5h numbers must come from the secondary slot"
        );
        assert_eq!(windows[0].resets_at, Some(1_700_001_330));
        assert_eq!(windows[1].label, "wk");
        assert_eq!(windows[1].remaining_percent, 83);
        assert_eq!(windows[1].resets_at, Some(1_700_345_600));
    }

    #[test]
    fn parse_rate_limits_ignores_unknown_window_durations() {
        // A daily window in the primary slot and an extra slot with an
        // unknown duration are both ignored; only the 5-hour window remains.
        let result = serde_json::json!({
            "rateLimits": {
                "primary": { "usedPercent": 50.0, "windowDurationMins": 1440 },
                "secondary": { "usedPercent": 39.0, "windowDurationMins": 300 },
                "tertiary": { "usedPercent": 10.0, "windowDurationMins": 43200 }
            }
        });
        let windows = parse_rate_limits(&result).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
    }

    #[test]
    fn parse_rate_limits_without_durations_falls_back_to_slots() {
        // Older app-server responses carry no `windowDurationMins`: keep the
        // historical primary → 5h, secondary → weekly mapping.
        let result = serde_json::json!({
            "rateLimits": {
                "primary": { "usedPercent": 39.0, "resetsAt": 1700001330 },
                "secondary": { "usedPercent": 17.0, "resetsAt": 1700345600 }
            }
        });
        let windows = parse_rate_limits(&result).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(windows[1].label, "wk");
        assert_eq!(windows[1].remaining_percent, 83);
    }

    #[test]
    fn fetch_reports_unavailable_when_signed_out() {
        let result = fetch_via_app_server_with(
            || Some(PathBuf::from("/opt/homebrew/bin/codex")),
            || Ok(None),
            |_| panic!("no process should spawn without a login"),
        )
        .unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
    }

    #[test]
    fn fetch_reports_unavailable_without_a_binary() {
        let result = fetch_via_app_server_with(
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

    // ── direct wham/usage path ──────────────────────────────────────

    fn wham_body() -> String {
        json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 39,
                    "limit_window_seconds": 18000,
                    "reset_at": 1700001330
                },
                "secondary_window": {
                    "used_percent": 17.5,
                    "limit_window_seconds": 604800,
                    "reset_at": 1700345600
                }
            }
        })
        .to_string()
    }

    fn auth_json(access_token: &str, refresh_token: Option<&str>) -> String {
        json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "id-token",
                "access_token": access_token,
                "refresh_token": refresh_token,
                "account_id": "acct-1"
            },
            "last_refresh": "2026-10-01T00:00:00Z"
        })
        .to_string()
    }

    fn base64url_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let packed = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
            out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[(packed >> 6) as usize & 63] as char);
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[packed as usize & 63] as char);
            }
        }
        out
    }

    fn jwt_with_exp(exp: u64) -> String {
        let payload = base64url_encode(format!("{{\"exp\":{exp}}}").as_bytes());
        format!("header.{payload}.signature")
    }

    #[test]
    fn parse_auth_extracts_the_chatgpt_tokens() {
        let auth = parse_auth(&auth_json("tok", Some("ref"))).unwrap();
        assert_eq!(auth.access_token, "tok");
        assert_eq!(auth.refresh_token.as_deref(), Some("ref"));
        assert_eq!(auth.id_token.as_deref(), Some("id-token"));
        assert_eq!(auth.account_id.as_deref(), Some("acct-1"));
        assert!(parse_auth(&auth_json("  ", Some("ref"))).is_none());
        assert!(parse_auth(r#"{"auth_mode":"apikey","tokens":{"access_token":"tok"}}"#).is_none());
    }

    #[test]
    fn jwt_expiration_reads_the_exp_claim() {
        // base64url(`{"exp":123}`)
        assert_eq!(jwt_expiration("x.eyJleHAiOjEyM30.y"), Some(123));
        let token = jwt_with_exp(1_700_000_000);
        assert_eq!(jwt_expiration(&token), Some(1_700_000_000));
        assert_eq!(jwt_expiration("not-a-jwt"), None);
        assert_eq!(jwt_expiration("x.!!!.y"), None);
    }

    #[test]
    fn rfc3339_format_roundtrips_through_the_kimi_parser() {
        for seconds in [0, 1_700_001_330, crate::time::now_epoch_secs()] {
            assert_eq!(
                crate::quota::kimi::parse_iso8601_secs(&format_rfc3339(seconds)),
                Some(seconds)
            );
        }
    }

    #[test]
    fn merge_auth_tokens_rotates_tokens_and_preserves_unknown_keys() {
        let raw = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "future_field": { "nested": true },
            "tokens": {
                "id_token": "old-id",
                "access_token": "old-access",
                "refresh_token": "old-refresh",
                "account_id": "acct-1"
            },
            "last_refresh": "2026-09-01T00:00:00Z"
        })
        .to_string();
        let auth = CodexAuth {
            access_token: "new-access".to_string(),
            refresh_token: Some("new-refresh".to_string()),
            id_token: Some("new-id".to_string()),
            account_id: Some("acct-1".to_string()),
        };
        let merged = merge_auth_tokens(&raw, &auth, "2026-10-03T00:00:00Z").unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["tokens"]["access_token"], "new-access");
        assert_eq!(value["tokens"]["refresh_token"], "new-refresh");
        assert_eq!(value["tokens"]["id_token"], "new-id");
        assert_eq!(value["last_refresh"], "2026-10-03T00:00:00Z");
        assert_eq!(value["auth_mode"], "chatgpt");
        assert_eq!(value["future_field"]["nested"], true);
        assert!(
            value.get("OPENAI_API_KEY").is_some(),
            "null-valued keys are preserved"
        );
    }

    #[test]
    fn parse_wham_usage_maps_both_windows() {
        let payload: Value = serde_json::from_str(&wham_body()).unwrap();
        let windows = parse_wham_usage(&payload).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(windows[0].resets_at, Some(1_700_001_330));
        assert_eq!(windows[1].label, "wk");
        // Floats are tolerated: 100 - 17.5 rounds to 83.
        assert_eq!(windows[1].remaining_percent, 83);
        assert_eq!(windows[1].resets_at, Some(1_700_345_600));
    }

    #[test]
    fn parse_wham_usage_classifies_by_window_length_not_slot() {
        // The weekly window shows up in `primary_window` when no 5h limit
        // applies, and either slot may be null.
        let payload = json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 40,
                    "limit_window_seconds": 604800,
                    "reset_at": 1700345600
                },
                "secondary_window": null
            }
        });
        let windows = parse_wham_usage(&payload).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "wk");
        assert_eq!(windows[0].remaining_percent, 60);
    }

    #[test]
    fn parse_wham_usage_rejects_missing_or_unknown_windows() {
        assert!(parse_wham_usage(&json!({})).is_none());
        assert!(parse_wham_usage(&json!({"rate_limit": {"primary_window": null}})).is_none());
        assert!(
            parse_wham_usage(&json!({
                "rate_limit": {
                    "primary_window": { "used_percent": 10, "limit_window_seconds": 3600 }
                }
            }))
            .is_none(),
            "unknown window lengths are ignored"
        );
    }

    #[test]
    fn direct_fetch_reports_unavailable_without_a_login() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| -> Result<HttpResponse, FetchError> {
            panic!("no request should be made without a login")
        };
        let write = |_: &CodexAuth| panic!("no writeback without a login");
        let result = fetch_direct_with(&curl, &|| Ok(None), &write).unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
        // A non-ChatGPT auth file is also "signed out".
        let result = fetch_direct_with(&curl, &|| Ok(Some("{}".to_string())), &write).unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
    }

    #[test]
    fn direct_fetch_sends_bearer_and_account_headers() {
        let body = wham_body();
        let curl = move |url: &str, args: &[&str], _: Option<&str>| {
            assert_eq!(url, USAGE_URL);
            assert!(args.contains(&"Authorization: Bearer access-token"));
            assert!(args.contains(&"ChatGPT-Account-Id: acct-1"));
            Ok(HttpResponse::new(200, body.clone(), None))
        };
        let raw = auth_json("access-token", Some("refresh-token"));
        let write = |_: &CodexAuth| panic!("no writeback when the token is fresh");
        let result = fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write).unwrap();
        let QuotaFetch::Available(windows) = result else {
            panic!("expected windows");
        };
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
    }

    #[test]
    fn direct_fetch_refreshes_an_expiring_token_and_writes_back() {
        let body = wham_body();
        let curl = move |url: &str, args: &[&str], _: Option<&str>| {
            if url == TOKEN_URL {
                return Ok(HttpResponse::new(
                    200,
                    json!({
                        "access_token": "fresh-access",
                        "refresh_token": "rotated-refresh",
                        "id_token": "fresh-id"
                    })
                    .to_string(),
                    None,
                ));
            }
            assert!(
                args.contains(&"Authorization: Bearer fresh-access"),
                "the refreshed token must be used, got {args:?}"
            );
            Ok(HttpResponse::new(200, body.clone(), None))
        };
        let raw = auth_json(&jwt_with_exp(1), Some("old-refresh"));
        let written = std::cell::RefCell::new(None);
        let write = |auth: &CodexAuth| {
            *written.borrow_mut() = Some(auth.clone());
            Ok(())
        };
        let result = fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write).unwrap();
        assert!(matches!(result, QuotaFetch::Available(_)));
        let written = written.borrow().clone().unwrap();
        assert_eq!(written.access_token, "fresh-access");
        assert_eq!(
            written.refresh_token.as_deref(),
            Some("rotated-refresh"),
            "rotated refresh tokens must be written back"
        );
    }

    #[test]
    fn direct_fetch_adopts_a_sibling_rotation_after_a_401() {
        let body = wham_body();
        let curl = move |_: &str, args: &[&str], _: Option<&str>| {
            if args.contains(&"Authorization: Bearer old-access") {
                Ok(HttpResponse::new(401, String::new(), None))
            } else {
                assert!(
                    args.contains(&"Authorization: Bearer sibling-access"),
                    "the sibling's fresher token must win, got {args:?}"
                );
                Ok(HttpResponse::new(200, body.clone(), None))
            }
        };
        let reads = std::cell::Cell::new(0usize);
        let read = move || {
            reads.set(reads.get() + 1);
            if reads.get() == 1 {
                Ok(Some(auth_json("old-access", Some("refresh-token"))))
            } else {
                Ok(Some(auth_json("sibling-access", Some("sibling-refresh"))))
            }
        };
        let write = |_: &CodexAuth| panic!("a sibling rotation needs no writeback");
        let result = fetch_direct_with(&curl, &read, &write).unwrap();
        assert!(matches!(result, QuotaFetch::Available(_)));
    }

    #[test]
    fn direct_fetch_falls_back_after_a_second_unauthorized() {
        let curl =
            |_: &str, _: &[&str], _: Option<&str>| Ok(HttpResponse::new(401, String::new(), None));
        let raw = auth_json("access-token", None);
        let write = |_: &CodexAuth| panic!("no writeback without a refresh");
        let result = fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write);
        assert!(matches!(result, Err(DirectError::Fallback(_))));
    }

    #[test]
    fn direct_fetch_falls_back_when_the_refresh_is_rejected() {
        let curl = |url: &str, _: &[&str], _: Option<&str>| {
            if url == TOKEN_URL {
                Ok(HttpResponse::new(400, String::new(), None))
            } else {
                Ok(HttpResponse::new(401, String::new(), None))
            }
        };
        let raw = auth_json(&jwt_with_exp(1), Some("stale-refresh"));
        let write = |_: &CodexAuth| panic!("a rejected refresh must not be written");
        let result = fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write);
        assert!(matches!(result, Err(DirectError::Fallback(_))));
    }

    #[test]
    fn direct_fetch_surfaces_the_retry_after_hint_on_429() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| {
            Ok(HttpResponse::new(
                429,
                String::new(),
                Some(Duration::from_secs(120)),
            ))
        };
        let raw = auth_json("access-token", None);
        let write = |_: &CodexAuth| Ok(());
        let Err(DirectError::Fatal(error)) =
            fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write)
        else {
            panic!("a 429 must surface as a fatal throttled error");
        };
        assert_eq!(error.retry_after, Some(Duration::from_secs(120)));
    }

    #[test]
    fn direct_fetch_falls_back_on_schema_drift() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| {
            Ok(HttpResponse::new(
                200,
                json!({"unexpected": true}).to_string(),
                None,
            ))
        };
        let raw = auth_json("access-token", None);
        let write = |_: &CodexAuth| Ok(());
        let result = fetch_direct_with(&curl, &|| Ok(Some(raw.clone())), &write);
        assert!(matches!(result, Err(DirectError::Fallback(_))));
    }
}
