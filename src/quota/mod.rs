//! Subscription quota state, cadence policy, and the background poll loop.
//!
//! Fork-only feature: the sidebar renders remaining quota for the user's
//! ChatGPT (Codex) and Kimi Code subscriptions in the idle rows below the
//! agent list. Fetching lives in the [`kimi`] / [`codex`] leaves and never
//! touches the TUI thread — results arrive through a channel exactly like the
//! session-name and git pollers in `src/app/workers.rs`.
//!
//! Ownership: [`QuotaState`] is local state owned by the singleton sidebar
//! process. Nothing here is written to tmux and no credential value is ever
//! logged, rendered, or snapshotted.

pub mod codex;
pub mod kimi;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::state::AppState;

/// Normal refresh cadence. Quota windows move slowly: the remaining
/// percentage only changes when the user actually consumes quota.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Failure backoff cap. Doubles per consecutive failure so an offline or
/// VPN-less machine is not polled aggressively.
pub const MAX_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Countdown text is recomputed from `resets_at` on every 1s refresh tick,
/// so it stays live without any additional I/O.
pub const QUOTA_SUBSCRIPTION_COUNT: usize = 2;

/// The two subscriptions the fork can display. Claude exposes no equivalent
/// usage API, so it has no variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subscription {
    Codex,
    Kimi,
}

impl Subscription {
    /// Short name used by the compact renderer.
    pub fn label(self) -> &'static str {
        match self {
            Subscription::Codex => "codex",
            Subscription::Kimi => "kimi",
        }
    }
}

/// One quota window (the 5-hour bucket or the weekly bucket).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaWindow {
    /// Normalized label: `"5h"` or `"wk"`.
    pub label: String,
    /// Remaining quota, `0..=100`. The bars represent remaining, matching
    /// the Raycast extension's display semantics.
    pub remaining_percent: u8,
    /// Wall-clock reset time in Unix milliseconds, when the API provides one.
    pub resets_at: Option<u64>,
}

/// Last good snapshot for one subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionQuota {
    pub windows: Vec<QuotaWindow>,
    /// When the snapshot was applied to the state. Drives the stale-age
    /// marker shown after a failed refetch.
    pub fetched_at: Instant,
    /// `true` when the most recent fetch failed. The last good snapshot stays
    /// on screen, dimmed, with an age marker appended.
    pub fetch_failed: bool,
}

impl SubscriptionQuota {
    pub fn new(windows: Vec<QuotaWindow>) -> Self {
        Self {
            windows,
            fetched_at: Instant::now(),
            fetch_failed: false,
        }
    }

    /// `true` when the last refetch failed and the snapshot may be stale.
    pub fn is_stale(&self) -> bool {
        self.fetch_failed
    }

    /// Age of the snapshot as a compact marker (`17m`, `2h`). `None` while
    /// the data is fresh.
    pub fn age_marker(&self) -> Option<String> {
        if !self.fetch_failed {
            return None;
        }
        Some(format_elapsed(self.fetched_at.elapsed()))
    }
}

/// Outcome of one subscription fetch.
///
/// `Unavailable` is distinct from `Err`: a missing credentials file or a
/// missing Codex binary means the subscription never renders, while an error
/// keeps the last good snapshot visible but dimmed.
#[derive(Debug, Clone)]
pub enum QuotaFetch {
    Available(Vec<QuotaWindow>),
    Unavailable,
}

/// Result delivered from the quota worker to the main loop. Per-subscription
/// results keep one failure from masking the other's success.
#[derive(Debug, Clone)]
pub struct QuotaFetchResult {
    pub codex: Result<QuotaFetch, String>,
    pub kimi: Result<QuotaFetch, String>,
    /// Whether this result came from a forced (click) refetch. Exposed so the
    /// renderer tests can drive the same code path as the worker.
    pub forced: bool,
}

/// Local quota state: the last good snapshot per subscription.
#[derive(Debug, Default)]
pub struct QuotaState {
    pub codex: Option<SubscriptionQuota>,
    pub kimi: Option<SubscriptionQuota>,
}

impl QuotaState {
    /// Number of subscriptions that currently have a snapshot to render.
    pub fn subscription_count(&self) -> usize {
        usize::from(self.codex.is_some()) + usize::from(self.kimi.is_some())
    }

    /// Merge one fetch result into the state. `Unavailable` clears the
    /// subscription permanently, `Err` keeps the last good snapshot and marks
    /// it stale, `Available` refreshes it.
    pub fn apply(&mut self, subscription: Subscription, fetch: Result<QuotaFetch, String>) {
        let slot = match subscription {
            Subscription::Codex => &mut self.codex,
            Subscription::Kimi => &mut self.kimi,
        };
        match fetch {
            Ok(QuotaFetch::Available(windows)) => {
                *slot = Some(SubscriptionQuota::new(windows));
            }
            Ok(QuotaFetch::Unavailable) => {
                *slot = None;
            }
            Err(_) => {
                // A failure before the first success has nothing to preserve,
                // so the subscription simply stays absent.
                if let Some(quota) = slot {
                    quota.fetch_failed = true;
                }
            }
        }
    }

    /// Snapshot for one subscription. The renderer walks Codex then Kimi so
    /// the two-row-per-subscription block keeps a stable order.
    pub fn get(&self, subscription: Subscription) -> Option<&SubscriptionQuota> {
        match subscription {
            Subscription::Codex => self.codex.as_ref(),
            Subscription::Kimi => self.kimi.as_ref(),
        }
    }

    /// Subscriptions that currently render, in display order.
    pub fn rendered(&self) -> Vec<(Subscription, &SubscriptionQuota)> {
        [Subscription::Codex, Subscription::Kimi]
            .into_iter()
            .filter_map(|subscription| self.get(subscription).map(|quota| (subscription, quota)))
            .collect()
    }
}

impl AppState {
    /// Store a subscription fetch delivered by the quota worker.
    pub fn apply_quota_result(&mut self, result: QuotaFetchResult) {
        self.quota.apply(Subscription::Codex, result.codex);
        self.quota.apply(Subscription::Kimi, result.kimi);
    }

    /// Rows the quota block wants for its current subscriptions.
    pub fn quota_full_height(&self) -> u16 {
        let subscriptions = self.quota.subscription_count() as u16;
        if subscriptions == 0 {
            0
        } else {
            1 + 2 * subscriptions
        }
    }

    /// Rows the compact quota block wants (one line per subscription).
    pub fn quota_compact_height(&self) -> u16 {
        self.quota.subscription_count() as u16
    }
}

/// Interval before the next fetch: double per consecutive failure, capped.
pub fn next_interval(consecutive_failures: u32) -> Duration {
    let mut interval = REFRESH_INTERVAL;
    for _ in 0..consecutive_failures.min(16) {
        interval = interval.saturating_mul(2).min(MAX_REFRESH_INTERVAL);
    }
    interval
}

/// Sleep in one-second slices so a forced refetch takes effect within a
/// second. Returns `true` when the force flag interrupted the sleep.
fn sleep_until_due(interval: Duration, force: &AtomicBool) -> bool {
    let deadline = Instant::now() + interval;
    loop {
        if force.load(Ordering::Relaxed) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_secs(1)));
    }
}

/// Background quota poller. Modeled on `session_poll_loop`: it owns the
/// fetch cadence and reports results through a channel.
///
/// `fetch_codex` / `fetch_kimi` are injected so tests can exercise the loop
/// without network I/O; production passes the closures in
/// `src/app/workers.rs`.
pub fn quota_poll_loop(
    tx: &std::sync::mpsc::Sender<QuotaFetchResult>,
    force: &AtomicBool,
    fetch_codex: impl Fn() -> Result<QuotaFetch, String>,
    fetch_kimi: impl Fn() -> Result<QuotaFetch, String>,
) {
    let mut consecutive_failures: u32 = 0;
    loop {
        let forced = force.swap(false, Ordering::Relaxed);
        let codex = fetch_codex();
        let kimi = fetch_kimi();
        let failed = codex.is_err() || kimi.is_err();
        if tx
            .send(QuotaFetchResult {
                codex,
                kimi,
                forced,
            })
            .is_err()
        {
            return;
        }
        consecutive_failures = if failed {
            consecutive_failures.saturating_add(1)
        } else {
            0
        };
        sleep_until_due(next_interval(consecutive_failures), force);
    }
}

/// `2h13m`, `41m`, `2h`, `4d`. Days replace the hour/minute granularity once
/// the reset is more than a day away so the sidebar stays narrow.
pub fn format_countdown(now_epoch_millis: u64, resets_at: Option<u64>) -> Option<String> {
    let resets_at = resets_at?;
    if resets_at <= now_epoch_millis {
        return Some("now".to_string());
    }
    let minutes = (resets_at - now_epoch_millis).div_ceil(60_000);
    let days = minutes / (60 * 24);
    if days > 0 {
        return Some(format!("{days}d"));
    }
    let hours = minutes / 60;
    let remainder = minutes % 60;
    Some(match (hours, remainder) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h{minutes}m"),
    })
}

/// Compact age marker for a stale snapshot.
pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        return "now".to_string();
    }
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(label: &str, remaining_percent: u8) -> QuotaWindow {
        QuotaWindow {
            label: label.into(),
            remaining_percent,
            resets_at: None,
        }
    }

    #[test]
    fn countdown_formats_minutes_hours_and_days() {
        let now = 1_700_000_000_000u64;
        assert_eq!(
            format_countdown(now, Some(now + 41 * 60_000)),
            Some("41m".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + (2 * 60 + 13) * 60_000)),
            Some("2h13m".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + 120 * 60_000)),
            Some("2h".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + 4 * 24 * 60 * 60_000)),
            Some("4d".to_string())
        );
        assert_eq!(format_countdown(now, None), None);
        assert_eq!(format_countdown(now, Some(now - 1)), Some("now".into()));
    }

    #[test]
    fn countdown_rounds_up_partial_minutes() {
        let now = 1_700_000_000_000u64;
        // 30 seconds away still reads as one minute rather than 0m.
        assert_eq!(
            format_countdown(now, Some(now + 30_000)),
            Some("1m".to_string())
        );
    }

    #[test]
    fn interval_doubles_per_failure_and_caps() {
        assert_eq!(next_interval(0), REFRESH_INTERVAL);
        assert_eq!(next_interval(1), REFRESH_INTERVAL * 2);
        assert_eq!(next_interval(2), REFRESH_INTERVAL * 4);
        assert_eq!(next_interval(3), MAX_REFRESH_INTERVAL);
        assert_eq!(next_interval(50), MAX_REFRESH_INTERVAL);
    }

    #[test]
    fn unavailable_clears_but_failure_keeps_last_snapshot() {
        let mut state = QuotaState::default();
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 61)])),
        );
        assert_eq!(state.subscription_count(), 1);

        // A failure with a snapshot preserves it and marks it stale.
        state.apply(Subscription::Kimi, Err("boom".into()));
        let quota = state.get(Subscription::Kimi).unwrap();
        assert!(quota.is_stale());
        assert_eq!(quota.windows, vec![window("5h", 61)]);

        // A later success clears the stale flag.
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 42)])),
        );
        assert!(!state.get(Subscription::Kimi).unwrap().is_stale());

        // Unavailable (no credentials) removes the row entirely.
        state.apply(Subscription::Kimi, Ok(QuotaFetch::Unavailable));
        assert!(state.get(Subscription::Kimi).is_none());
        assert_eq!(state.subscription_count(), 0);
    }

    #[test]
    fn failure_without_snapshot_stays_absent() {
        let mut state = QuotaState::default();
        state.apply(Subscription::Codex, Err("boom".into()));
        assert!(state.get(Subscription::Codex).is_none());
    }

    #[test]
    fn rendered_keeps_codex_before_kimi() {
        let mut state = QuotaState::default();
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("wk", 37)])),
        );
        state.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![window("wk", 83)])),
        );
        let order: Vec<_> = state
            .rendered()
            .into_iter()
            .map(|(subscription, _)| subscription)
            .collect();
        assert_eq!(order, vec![Subscription::Codex, Subscription::Kimi]);
    }

    #[test]
    fn age_marker_only_when_stale() {
        let mut state = QuotaState::default();
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 24)])),
        );
        assert_eq!(state.get(Subscription::Kimi).unwrap().age_marker(), None);
        state.apply(Subscription::Kimi, Err("offline".into()));
        assert!(
            state
                .get(Subscription::Kimi)
                .unwrap()
                .age_marker()
                .is_some()
        );
    }

    #[test]
    fn poll_loop_reports_both_subscriptions_on_the_first_tick() {
        let (tx, rx) = std::sync::mpsc::channel::<QuotaFetchResult>();
        // `force` starts `true`, so the loop sends its first result before any
        // cadence wait.
        let force = AtomicBool::new(true);
        let handle = std::thread::spawn(move || {
            quota_poll_loop(
                &tx,
                &force,
                || Ok(QuotaFetch::Unavailable),
                || Ok(QuotaFetch::Available(vec![window("5h", 24)])),
            );
        });
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("quota poll loop should report immediately");
        assert!(result.forced);
        assert!(matches!(result.codex, Ok(QuotaFetch::Unavailable)));
        assert!(matches!(result.kimi, Ok(QuotaFetch::Available(_))));

        // Let the loop fall into its cadence wait, then drop the receiver so
        // the next send fails and the thread exits on its own. The detached
        // thread is harmless: `force` stays false, so its only other action is
        // a sleep before the failed send.
        drop(rx);
        drop(handle);
    }
}
