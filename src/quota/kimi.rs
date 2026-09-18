//! Kimi Code quota client.
//!
//! Normative protocol reference: the user's Raycast extension
//! (`agent-usage/src/kimi.ts`). HTTP goes through a `curl` subprocess to match
//! the repository's existing tmux/ps/lsof idiom and avoid adding an HTTP crate.
//! The credentials file has three concurrent consumers (Kimi CLI, Raycast
//! extension, and this sidebar), so the token refresh path is deliberately
//! conservative: re-read the file first and only spend our own refresh token
//! when the on-disk token is still expired.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::{QuotaFetch, QuotaWindow};

const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";
const TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
/// Public client id copied from the Raycast extension (`kimi.ts`).
const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
/// Treat the token as expired this early to avoid racing the server clock.
const EXPIRY_SKEW_SECONDS: i64 = 60;
const CURL_TIMEOUT_SECS: &str = "10";

fn credentials_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".kimi-code")
            .join("credentials")
            .join("kimi-code.json"),
    )
}

/// A `curl` invoker. Production uses [`run_curl`]; tests inject a stub so no
/// test ever performs network I/O or spawns a subprocess.
pub type CurlRunner<'a> = &'a dyn Fn(&str, &[&str], Option<&str>) -> Result<(u16, String), String>;

/// Fetch Kimi quota. `Ok(QuotaFetch::Unavailable)` means there is no
/// credentials file, so the subscription never renders.
pub fn fetch_quota() -> Result<QuotaFetch, String> {
    fetch_quota_with(&run_curl, &read_credentials_file)
}

/// Fetch with an injectable transport and credentials reader.
///
/// `read` returns the raw credentials file contents; `Ok(None)` means the file
/// is absent. `curl` returns `(http_status, body)` and `Err` for transport
/// failures (DNS, timeout, non-zero exit).
pub fn fetch_quota_with(
    curl: CurlRunner<'_>,
    read: &dyn Fn() -> Result<Option<String>, String>,
) -> Result<QuotaFetch, String> {
    let Some(raw) = read()? else {
        return Ok(QuotaFetch::Unavailable);
    };
    let mut credentials = parse_credentials(&raw).ok_or_else(login_message)?;

    if expiring_soon(&credentials) && credentials.refresh_token.is_some() {
        credentials = usable_credentials(curl, read, credentials)?;
    }

    let mut response = request_usages(curl, &credentials)?;
    if response.0 == 401 || response.0 == 403 {
        // The CLI or the Raycast extension may have just refreshed. Re-read
        // the file before spending our own refresh token.
        let latest = read()
            .ok()
            .flatten()
            .and_then(|raw| parse_credentials(&raw));
        credentials = match latest {
            Some(latest)
                if latest.access_token != credentials.access_token && !expiring_soon(&latest) =>
            {
                latest
            }
            other => usable_credentials(curl, read, other.unwrap_or(credentials))?,
        };
        response = request_usages(curl, &credentials)?;
        if response.0 == 401 || response.0 == 403 {
            return Err(login_message());
        }
    }
    if response.0 >= 400 || response.0 == 0 {
        return Err(format!("Kimi quota request failed (HTTP {})", response.0));
    }
    let payload: Value = serde_json::from_str(&response.1)
        .map_err(|_| "Kimi quota response was not valid JSON".to_string())?;
    Ok(QuotaFetch::Available(parse_usage(&payload)?))
}

/// Credentials to use once the access token read from disk is expiring.
///
/// The credentials file is shared with the Kimi CLI, the user's status-line
/// script, and the Raycast extension, and Kimi rotates refresh tokens. So this
/// re-reads the file twice: once before spending our own refresh token (a
/// sibling may have just written a fresh one) and once before writing ours back
/// (writing last would strand the sibling's session, because the token it
/// rotated away from is no longer valid server-side).
fn usable_credentials(
    curl: CurlRunner<'_>,
    read: &dyn Fn() -> Result<Option<String>, String>,
    credentials: Credentials,
) -> Result<Credentials, String> {
    let newer = |candidate: Option<Credentials>| match candidate {
        Some(candidate)
            if candidate.access_token != credentials.access_token && !expiring_soon(&candidate) =>
        {
            Some(candidate)
        }
        _ => None,
    };
    if let Some(latest) = newer(
        read()
            .ok()
            .flatten()
            .and_then(|raw| parse_credentials(&raw)),
    ) {
        return Ok(latest);
    }
    if credentials.refresh_token.is_none() {
        // Nothing we can do but try the token we have, exactly as before.
        return Ok(credentials);
    }

    let refreshed = refresh_token(curl, credentials.clone())?;
    if let Some(winner) = newer(
        read()
            .ok()
            .flatten()
            .and_then(|raw| parse_credentials(&raw)),
    ) {
        return Ok(winner);
    }
    save_credentials(&refreshed)?;
    Ok(refreshed)
}

fn login_message() -> String {
    "Kimi credentials are missing or expired; run /usage in Kimi Code to refresh".to_string()
}

// ── protocol parsing (pure) ─────────────────────────────────────────

/// Parse the `GET /coding/v1/usages` payload into the windows the sidebar
/// renders. Follows `kimi.ts`: `limits[]` are the per-window buckets and the
/// top-level `usage` object is the weekly quota.
pub fn parse_usage(payload: &Value) -> Result<Vec<QuotaWindow>, String> {
    let data = payload
        .as_object()
        .ok_or_else(|| "Kimi quota response was not an object".to_string())?;
    if data.is_empty() {
        return Err("Kimi quota response was empty".to_string());
    }

    let mut windows: Vec<QuotaWindow> = Vec::new();
    if let Some(limits) = data.get("limits").and_then(Value::as_array) {
        for item in limits {
            let Some(label) = window_label(item.get("window")) else {
                continue;
            };
            let Some(detail) = item.get("detail") else {
                continue;
            };
            if let Some(window) = detail_window(detail, label.clone())
                && !windows.iter().any(|existing| existing.label == label)
            {
                windows.push(window);
            }
        }
    }
    // The top-level `usage` object is the weekly quota. It is appended after
    // the `limits` buckets so the 5-hour row always renders above the weekly
    // row, and it wins over any weekly entry `limits` happens to include.
    if let Some(weekly) = data
        .get("usage")
        .and_then(|usage| detail_window(usage, "wk".to_string()))
    {
        windows.retain(|window| window.label != "wk");
        windows.push(weekly);
    }
    if windows.is_empty() {
        return Err("Kimi quota response had no usable windows".to_string());
    }
    Ok(windows)
}

/// Normalize `{ duration, timeUnit }` to the sidebar's `5h` / `wk` labels.
/// Every other window is ignored, matching the design's two-window display.
fn window_label(window: Option<&Value>) -> Option<String> {
    let window = window?.as_object()?;
    let duration = json_number(window.get("duration"))?;
    let unit = window.get("timeUnit").and_then(Value::as_str)?;
    match unit {
        "TIME_UNIT_MINUTE" if duration == 300.0 => Some("5h".to_string()),
        "TIME_UNIT_HOUR" if duration == 5.0 => Some("5h".to_string()),
        "TIME_UNIT_DAY" if duration == 7.0 => Some("wk".to_string()),
        "TIME_UNIT_WEEK" if duration == 1.0 => Some("wk".to_string()),
        _ => None,
    }
}

/// Build one window from a `detail`/`usage` object. `used` may be omitted when
/// the full quota remains, in which case it is derived from `limit - remaining`.
fn detail_window(detail: &Value, label: String) -> Option<QuotaWindow> {
    let detail = detail.as_object()?;
    if detail.is_empty() {
        return None;
    }
    let limit = json_number(detail.get("limit"));
    let remaining = json_number(detail.get("remaining"));
    let mut used = json_number(detail.get("used"));
    if used.is_none()
        && let (Some(limit), Some(remaining)) = (limit, remaining)
        && remaining <= limit
    {
        used = Some(limit - remaining);
    }
    let remaining_percent = used_percent(used, limit, remaining);
    let resets_at = detail
        .get("resetTime")
        .and_then(Value::as_str)
        .and_then(parse_iso8601_secs);
    Some(QuotaWindow {
        label,
        remaining_percent: remaining_percent?,
        resets_at,
    })
}

/// Remaining percentage. Prefers `remaining / limit`; falls back to
/// `100 - used / limit` so a response that only reports `used` still renders.
fn used_percent(used: Option<f64>, limit: Option<f64>, remaining: Option<f64>) -> Option<u8> {
    let limit = limit.filter(|limit| *limit > 0.0)?;
    let percent = match remaining {
        Some(remaining) => (remaining / limit) * 100.0,
        None => 100.0 - (used? / limit) * 100.0,
    };
    Some(percent.clamp(0.0, 100.0).round() as u8)
}

/// Accept numbers and numeric strings, matching `usage.ts`'s `number()`.
fn json_number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let parsed = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
}

/// Minimal ISO-8601 (`YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM]`) parser.
/// Deliberately dependency-free; returns Unix seconds. Fractional seconds are
/// dropped: countdowns are minute-granular, and the quota module works in the
/// same seconds clock as `AppState::now`.
pub fn parse_iso8601_secs(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = value.get(0..4)?.parse().ok()?;
    let month: i64 = value.get(5..7)?.parse().ok()?;
    let day: i64 = value.get(8..10)?.parse().ok()?;
    let hour: i64 = value.get(11..13)?.parse().ok()?;
    let minute: i64 = value.get(14..16)?.parse().ok()?;
    let second: i64 = value.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let mut rest = &value[19..];
    if let Some(stripped) = rest.strip_prefix('.') {
        let digits: String = stripped.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        rest = &stripped[digits.len()..];
    }

    let offset_seconds: i64 = match rest {
        "" | "Z" | "z" => 0,
        _ if rest.len() == 6 => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
        _ => return None,
    };

    let epoch_seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 - offset_seconds;
    Some(epoch_seconds.max(0) as u64)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ── credentials ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Credentials {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
    scope: Option<String>,
    token_type: Option<String>,
}

fn read_credentials_file() -> Result<Option<String>, String> {
    let Some(path) = credentials_path() else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Kimi credentials could not be read: {error}")),
    }
}

fn parse_credentials(raw: &str) -> Option<Credentials> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let object = value.as_object()?;
    let access_token = object.get("access_token")?.as_str()?.trim();
    if access_token.is_empty() {
        return None;
    }
    let number = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .map(|value| value as i64)
    };
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    };
    Some(Credentials {
        access_token: access_token.to_string(),
        refresh_token: text("refresh_token"),
        expires_at: number("expires_at"),
        expires_in: number("expires_in"),
        scope: text("scope"),
        token_type: text("token_type"),
    })
}

fn expiring_soon(credentials: &Credentials) -> bool {
    let Some(expires_at) = credentials.expires_at else {
        return false;
    };
    expires_at - now_epoch_seconds() < EXPIRY_SKEW_SECONDS
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn refresh_token(curl: CurlRunner<'_>, credentials: Credentials) -> Result<Credentials, String> {
    let Some(refresh_token) = credentials.refresh_token.as_deref() else {
        return Err(login_message());
    };
    let body = format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}",
        url_encode(CLIENT_ID),
        url_encode(refresh_token)
    );
    let (status, response) = curl(
        TOKEN_URL,
        &[
            "-X",
            "POST",
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "-H",
            "Accept: application/json",
            "--data",
            &body,
        ],
        None,
    )
    .map_err(|_| "Kimi login refresh failed; check the network and retry".to_string())?;
    if status >= 400 || status == 0 {
        return Err(login_message());
    }
    let payload: Value =
        serde_json::from_str(&response).map_err(|_| login_message().to_string())?;
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(login_message)?;
    let expires_in = json_number(payload.get("expires_in")).unwrap_or(900.0) as i64;
    let text = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    };
    Ok(Credentials {
        access_token: access_token.to_string(),
        refresh_token: text("refresh_token").or(credentials.refresh_token),
        expires_at: Some(now_epoch_seconds() + expires_in),
        expires_in: Some(expires_in),
        scope: text("scope")
            .or(credentials.scope)
            .or(Some("kimi-code".into())),
        token_type: text("token_type").or(Some("Bearer".into())),
    })
}

/// Atomic writeback: temp file in the same directory, 0600, then rename.
fn save_credentials(credentials: &Credentials) -> Result<(), String> {
    let Some(path) = credentials_path() else {
        return Err("Kimi credentials path is unavailable".to_string());
    };
    let mut object = serde_json::Map::new();
    object.insert("access_token".into(), json!(credentials.access_token));
    if let Some(refresh_token) = &credentials.refresh_token {
        object.insert("refresh_token".into(), json!(refresh_token));
    }
    if let Some(expires_at) = credentials.expires_at {
        object.insert("expires_at".into(), json!(expires_at));
    }
    if let Some(expires_in) = credentials.expires_in {
        object.insert("expires_in".into(), json!(expires_in));
    }
    if let Some(scope) = &credentials.scope {
        object.insert("scope".into(), json!(scope));
    }
    if let Some(token_type) = &credentials.token_type {
        object.insert("token_type".into(), json!(token_type));
    }
    write_atomic(
        &path,
        &serde_json::to_string(&Value::Object(object)).map_err(|error| error.to_string())?,
    )
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_file_name(format!(
        "{}.tmp{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "kimi-code.json".to_string()),
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        set_owner_only(&file)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|error| format!("Kimi credentials could not be written: {error}"))
}

#[cfg(unix)]
fn set_owner_only(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

fn url_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

// ── transport ───────────────────────────────────────────────────────

/// Flags shared by every curl invocation. `-o -` streams the body to stdout;
/// `--write-out` then appends the HTTP status after a separator newline.
///
/// The separator is explicit because these endpoints do not all end their
/// bodies with a newline: api.kimi.com answers `{...}` and the status would
/// otherwise be glued onto the JSON.
fn curl_base_args() -> [&'static str; 7] {
    [
        "-sS",
        "-m",
        CURL_TIMEOUT_SECS,
        "--write-out",
        "\n%{http_code}",
        "-o",
        "-",
    ]
}

/// Runner used in production: `curl -sS -m 10 ...`. It appends
/// `--write-out` so the HTTP status is machine-readable (same idea as
/// `src/port.rs` relying on `lsof`'s `-F` output).
fn run_curl(url: &str, args: &[&str], _body: Option<&str>) -> Result<(u16, String), String> {
    let output = Command::new("curl")
        .args(curl_base_args())
        .args(args)
        .arg(url)
        .output()
        .map_err(|error| format!("curl could not be started: {error}"))?;
    if !output.status.success() {
        return Err(format!("curl exited with {}", output.status));
    }
    parse_curl_output(&String::from_utf8_lossy(&output.stdout))
}

/// Split the `--write-out` status from the response body produced by
/// [`run_curl`]. Pure so the parsing can be unit-tested without a subprocess.
fn parse_curl_output(body: &str) -> Result<(u16, String), String> {
    // `--write-out` appends the status right after the body, so split on the
    // last newline. When a body arrives without that separator (a curl invoker
    // that forgot the `--write-out` newline), fall back to the trailing digits
    // so the status is still recovered instead of poisoning the payload.
    let (payload, status_line) = match body.rfind('\n') {
        Some(index) => (&body[..index], &body[index + 1..]),
        None => match trailing_digits(body) {
            Some(index) => (&body[..index], &body[index..]),
            None => ("", body),
        },
    };
    let status = status_line.trim().parse::<u16>().unwrap_or(0);
    let payload = payload.strip_suffix('\r').unwrap_or(payload).to_string();
    Ok((status, payload))
}

/// Byte index where the trailing run of ASCII digits starts, if any.
fn trailing_digits(value: &str) -> Option<usize> {
    let index = value.len() - value.chars().rev().take_while(char::is_ascii_digit).count();
    (index < value.len()).then_some(index)
}

fn request_usages(
    curl: CurlRunner<'_>,
    credentials: &Credentials,
) -> Result<(u16, String), String> {
    // The header name is part of the argument: `Bearer <token>` on its own is
    // sent as a custom header called `Bearer …`, which the API rejects with
    // `401 Invalid Authentication`.
    let authorization = format!("Authorization: Bearer {}", credentials.access_token);
    curl(
        USAGE_URL,
        &["-H", &authorization, "-H", "Accept: application/json"],
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> Value {
        json!({
            "limits": [
                {
                    "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                    "detail": {
                        "used": 39,
                        "limit": 100,
                        "remaining": 61,
                        "resetTime": "2026-09-17T14:21:00Z"
                    }
                },
                {
                    "window": { "duration": 1, "timeUnit": "TIME_UNIT_WEEK" },
                    "detail": { "used": 10, "limit": 100, "remaining": 90 }
                },
                {
                    "window": { "duration": 1, "timeUnit": "TIME_UNIT_DAY" },
                    "detail": { "used": 1, "limit": 10, "remaining": 9 }
                }
            ],
            "usage": {
                "used": 17,
                "limit": 100,
                "remaining": 83,
                "resetTime": "2026-09-21T00:00:00Z"
            }
        })
    }

    #[test]
    fn parse_usage_maps_5h_and_weekly_windows() {
        let windows = parse_usage(&sample_payload()).unwrap();
        assert_eq!(windows.len(), 2, "unknown day windows are ignored");
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(
            windows[0].resets_at,
            parse_iso8601_secs("2026-09-17T14:21:00Z")
        );
        assert_eq!(windows[1].label, "wk");
        // The top-level `usage` object wins over the limits entry for the
        // weekly window, matching the Raycast extension.
        assert_eq!(windows[1].remaining_percent, 83);
        assert_eq!(
            windows[1].resets_at,
            parse_iso8601_secs("2026-09-21T00:00:00Z")
        );
    }

    #[test]
    fn parse_usage_derives_used_when_omitted() {
        let payload = json!({
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": { "limit": 200, "remaining": 50 }
            }]
        });
        let windows = parse_usage(&payload).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].remaining_percent, 25);
    }

    #[test]
    fn parse_usage_accepts_numeric_strings_and_day_windows() {
        let payload = json!({
            "limits": [{
                "window": { "duration": "420", "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": { "used": "0", "limit": "100", "remaining": "100" }
            }],
            "usage": { "used": 0, "limit": 100, "remaining": 100 }
        });
        let windows = parse_usage(&payload).unwrap();
        // 420 minutes is not the 5-hour bucket, so only the weekly remains.
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "wk");
    }

    #[test]
    fn parse_usage_rejects_malformed_payload() {
        assert!(parse_usage(&json!("nope")).is_err());
        assert!(parse_usage(&json!({})).is_err());
        assert!(parse_usage(&json!({ "limits": "nope" })).is_err());
    }

    #[test]
    fn parse_usage_clamps_out_of_range_remaining() {
        let payload = json!({
            "usage": { "used": 0, "limit": 100, "remaining": 250 }
        });
        assert_eq!(parse_usage(&payload).unwrap()[0].remaining_percent, 100);
    }

    #[test]
    fn iso8601_parser_handles_offsets_fractions_and_invalid_input() {
        assert_eq!(parse_iso8601_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso8601_secs("2026-09-17T14:21:00+08:00"),
            parse_iso8601_secs("2026-09-17T06:21:00Z")
        );
        // Fractional seconds are dropped rather than rounded up.
        assert_eq!(
            parse_iso8601_secs("2026-09-17T14:21:00.250Z"),
            parse_iso8601_secs("2026-09-17T14:21:00Z")
        );
        assert_eq!(parse_iso8601_secs("not a timestamp"), None);
        assert_eq!(parse_iso8601_secs("2026-13-17T14:21:00Z"), None);
        assert_eq!(parse_iso8601_secs("2026-09-17T14:21:00.Z"), None);
    }

    fn credentials_json(access_token: &str, expires_at: i64) -> String {
        json!({
            "access_token": access_token,
            "refresh_token": "refresh-token",
            "expires_at": expires_at,
            "expires_in": 900,
            "scope": "kimi-code",
            "token_type": "Bearer"
        })
        .to_string()
    }

    #[test]
    fn fetch_reports_unavailable_without_credentials_file() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| {
            panic!("no request should be made without credentials")
        };
        let result = fetch_quota_with(&curl, &|| Ok(None)).unwrap();
        assert!(matches!(result, QuotaFetch::Unavailable));
    }

    #[test]
    fn fetch_uses_stored_token_and_returns_windows() {
        let body = sample_payload().to_string();
        let curl = move |url: &str, args: &[&str], _: Option<&str>| {
            assert_eq!(url, USAGE_URL);
            assert!(
                args.contains(&"Authorization: Bearer fresh-token"),
                "usage request must send an Authorization header, got {args:?}"
            );
            Ok((200, body.clone()))
        };
        let raw = credentials_json("fresh-token", now_epoch_seconds() + 3600);
        let result = fetch_quota_with(&curl, &|| Ok(Some(raw.clone()))).unwrap();
        let QuotaFetch::Available(windows) = result else {
            panic!("expected windows");
        };
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "5h");
        assert_eq!(windows[0].remaining_percent, 61);
        assert_eq!(windows[1].label, "wk");
        assert_eq!(windows[1].remaining_percent, 83);
    }

    #[test]
    fn fetch_retries_once_with_a_newer_on_disk_token() {
        let body = sample_payload().to_string();
        let calls = std::cell::Cell::new(0usize);
        let curl = |_: &str, args: &[&str], _: Option<&str>| {
            calls.set(calls.get() + 1);
            if args.contains(&"Authorization: Bearer old-token") {
                Ok((401, String::new()))
            } else {
                Ok((200, body.clone()))
            }
        };
        let reads = std::cell::Cell::new(0usize);
        let expires = now_epoch_seconds() + 3600;
        let read = || {
            reads.set(reads.get() + 1);
            // First read: the stored token is still valid on its face.
            // Second read (after the 401): another client refreshed it.
            if reads.get() == 1 {
                Ok(Some(credentials_json("old-token", expires)))
            } else {
                Ok(Some(credentials_json("new-token", expires)))
            }
        };
        let result = fetch_quota_with(&curl, &read).unwrap();
        assert!(matches!(result, QuotaFetch::Available(_)));
        assert_eq!(calls.get(), 2, "the 401 path retries exactly once");
    }

    #[test]
    fn fetch_returns_error_after_second_unauthorized() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| Ok((401, String::new()));
        let raw = credentials_json("old-token", now_epoch_seconds() + 3600);
        let result = fetch_quota_with(&curl, &|| Ok(Some(raw.clone())));
        assert!(result.is_err());
    }

    #[test]
    fn fetch_returns_error_when_transport_fails() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| Err("offline".to_string());
        let raw = credentials_json("token", now_epoch_seconds() + 3600);
        assert!(fetch_quota_with(&curl, &|| Ok(Some(raw.clone()))).is_err());
    }

    #[test]
    fn expired_token_adopts_a_sibling_refresh_instead_of_spending_ours() {
        let body = sample_payload().to_string();
        let curl = move |_: &str, args: &[&str], _: Option<&str>| {
            assert!(
                args.contains(&"Authorization: Bearer sibling-token"),
                "the sibling's fresher token must win before we refresh, got {args:?}"
            );
            Ok((200, body.clone()))
        };
        let reads = std::cell::Cell::new(0usize);
        let expires = now_epoch_seconds() + 3600;
        let read = || {
            reads.set(reads.get() + 1);
            // First read: our token is about to expire. Second read (before we
            // spend the refresh token): a sibling already rotated it.
            if reads.get() == 1 {
                Ok(Some(credentials_json(
                    "ours-expiring",
                    now_epoch_seconds() - 1,
                )))
            } else {
                Ok(Some(credentials_json("sibling-token", expires)))
            }
        };
        let result = fetch_quota_with(&curl, &read).unwrap();
        assert!(matches!(result, QuotaFetch::Available(_)));
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn refresh_does_not_clobber_a_token_written_while_ours_was_in_flight() {
        let body = sample_payload().to_string();
        let curl = move |url: &str, args: &[&str], _: Option<&str>| {
            if url == TOKEN_URL {
                // Our refresh succeeds...
                return Ok((
                    200,
                    json!({ "access_token": "ours-fresh", "expires_in": 900 }).to_string(),
                ));
            }
            assert!(
                args.contains(&"Authorization: Bearer sibling-token"),
                "the sibling's token must be kept, got {args:?}"
            );
            Ok((200, body.clone()))
        };
        let reads = std::cell::Cell::new(0usize);
        let read = || {
            reads.set(reads.get() + 1);
            match reads.get() {
                // Our token is expiring, and the pre-refresh re-read still
                // shows nobody else has moved.
                1 | 2 => Ok(Some(credentials_json(
                    "ours-expiring",
                    now_epoch_seconds() - 1,
                ))),
                // ... but the file moved on before we could write ours back.
                _ => Ok(Some(credentials_json(
                    "sibling-token",
                    now_epoch_seconds() + 3600,
                ))),
            }
        };
        let result = fetch_quota_with(&curl, &read).unwrap();
        assert!(matches!(result, QuotaFetch::Available(_)));
        assert_eq!(
            reads.get(),
            3,
            "initial read, pre-refresh re-read, writeback check"
        );
    }

    #[test]
    fn malformed_credentials_are_rejected_without_a_request() {
        let curl = |_: &str, _: &[&str], _: Option<&str>| -> Result<(u16, String), String> {
            panic!("no request should be made for malformed credentials")
        };
        let result = fetch_quota_with(&curl, &|| Ok(Some("{}".to_string())));
        assert!(result.is_err());
    }

    #[test]
    fn credentials_parse_ignores_blank_optional_fields() {
        let raw = r#"{"access_token":"tok","refresh_token":"  ","expires_at":"nope"}"#;
        let parsed = parse_credentials(raw).unwrap();
        assert_eq!(parsed.access_token, "tok");
        assert_eq!(parsed.refresh_token, None);
        assert_eq!(parsed.expires_at, None);
        assert!(!expiring_soon(&parsed));
    }

    #[test]
    fn credentials_without_access_token_are_rejected() {
        assert!(parse_credentials(r#"{"access_token":""}"#).is_none());
        assert!(parse_credentials("not json").is_none());
    }

    #[test]
    fn url_encode_escapes_reserved_characters() {
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn curl_output_splits_status_from_body() {
        // api.kimi.com: no trailing newline in the body, status glued on by
        // the `--write-out` separator.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n200").unwrap(),
            (200, "{\"a\":1}".to_string())
        );
        // A body that already ends in a newline keeps it, and JSON serde
        // tolerates the extra whitespace.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n\n200").unwrap(),
            (200, "{\"a\":1}\n".to_string())
        );
        assert_eq!(
            parse_curl_output("{\"a\":1}\r\n200").unwrap(),
            (200, "{\"a\":1}".to_string())
        );
        // Defensive fallback for a curl invoker without the separator: take
        // the trailing digits as the status instead of the whole payload.
        assert_eq!(
            parse_curl_output("{\"a\":1}200").unwrap(),
            (200, "{\"a\":1}".to_string())
        );
        assert_eq!(parse_curl_output("200").unwrap(), (200, String::new()));
        assert_eq!(
            parse_curl_output("").unwrap(),
            (0, String::new()),
            "an empty response has no status and no body"
        );
        // A status that fails to parse is reported as 0 so callers treat it as
        // a transport problem rather than a valid response.
        assert_eq!(parse_curl_output("body\n").unwrap().0, 0);
    }

    #[test]
    fn curl_write_out_carries_its_own_separator() {
        // Real responses arrive without a trailing newline, so the separator
        // that keeps the status out of the JSON body has to come from
        // `--write-out` itself.
        assert_eq!(curl_base_args()[3], "--write-out");
        assert_eq!(curl_base_args()[4], "\n%{http_code}");
    }
}
