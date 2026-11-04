//! Codex quota from the existing running server. No credentials or child processes.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{FetchError, FetchKind, FetchOutcome, QuotaFetch, QuotaWindow};
use crate::adapter::codex::control::{self, ControlError};
use serde_json::Value;

/// One budget for connect, initialization, account checks and the backend query.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn control_socket_path() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))?;
    Some(home.join("app-server-control/app-server-control.sock"))
}

pub fn fetch_quota() -> FetchOutcome {
    fetch_scoped_quota().1
}

/// Quota owner is the running daemon account, never a later disk login.
pub fn fetch_scoped_quota() -> (Option<String>, FetchOutcome) {
    let Some(path) = control_socket_path() else {
        return (
            None,
            Err(FetchError::with_kind(
                "Codex control socket is unavailable",
                FetchKind::Connection,
            )),
        );
    };
    fetch_at(&path, Instant::now() + FETCH_TIMEOUT)
}

fn fetch_at(path: &Path, deadline: Instant) -> (Option<String>, FetchOutcome) {
    match control::scoped_rate_limits(path, deadline) {
        Ok(Some((account, result))) => match parse_rate_limits(&result) {
            Some(windows) => (Some(account), Ok(QuotaFetch::Available(windows))),
            None => (
                None,
                Err(FetchError::with_kind(
                    "Codex returned no usable quota windows",
                    FetchKind::Data,
                )),
            ),
        },
        Ok(None) => (None, Ok(QuotaFetch::Unavailable)),
        Err(error) => {
            let kind = match &error {
                ControlError::Timeout => FetchKind::Timeout,
                ControlError::Transport => FetchKind::Connection,
                ControlError::Data | ControlError::AccountChanged => FetchKind::Data,
                ControlError::Rpc { .. } => FetchKind::Server,
            };
            (
                None,
                Err(FetchError {
                    message: error.to_string(),
                    kind,
                    // The server wraps upstream errors in generic RPC errors;
                    // HTTP status and Retry-After are not exposed by this method.
                    retry_after: None,
                }),
            )
        }
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
}
