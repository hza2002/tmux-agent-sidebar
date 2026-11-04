//! Fork-owned Codex fallback scheduling. Passive observations are the normal
//! update path; requests repair missing observations and crossed reset boundaries.
use std::time::{Duration, Instant};

use super::{FetchOutcome, Jitter, QuotaFetch, QuotaWindow};

const STARTUP_GRACE: Duration = Duration::from_secs(60);
const ACTIVE_GAP: Duration = Duration::from_secs(5 * 60);
const IDLE_GAP: Duration = Duration::from_secs(15 * 60);
const MAX_UNCHANGED: Duration = Duration::from_secs(4 * 60 * 60);
const RESET_RETRY: Duration = Duration::from_secs(2 * 60);
const RESET_TOLERANCE: u64 = 2;

#[derive(Debug, Clone)]
struct Boundary {
    label: String,
    stamp: u64,
    due: Option<Instant>,
    attempts: u8,
}

/// The caller supplies complete passive-window coverage as `latest_local`.
/// Repeating that same timestamp never extends a deadline. All time is injected
/// except the production jitter seed, so tests never need real sleeps or I/O.
#[derive(Debug, Clone)]
pub(super) struct CodexCadence {
    account: Option<String>,
    account_observed_at: u64,
    account_changed_at: u64,
    started_at: Instant,
    windows: Vec<QuotaWindow>,
    latest_local: Option<Instant>,
    last_local_seen: Option<Instant>,
    active: bool,
    last_fetch: Option<Instant>,
    normal_due: Instant,
    interval: Duration,
    unchanged: bool,
    failure_due: Option<Instant>,
    failures: u32,
    local_since_failure: bool,
    retry_not_before: Option<Instant>,
    boundaries: Vec<Boundary>,
    jitter: Jitter,
}

impl CodexCadence {
    pub(super) fn new(now: Instant) -> Self {
        Self::with_jitter(now, Jitter::from_entropy())
    }

    fn with_jitter(now: Instant, jitter: Jitter) -> Self {
        Self {
            account: None,
            account_observed_at: 0,
            account_changed_at: 0,
            started_at: now,
            windows: Vec::new(),
            latest_local: None,
            last_local_seen: None,
            active: false,
            last_fetch: None,
            normal_due: now + STARTUP_GRACE,
            interval: IDLE_GAP,
            unchanged: false,
            failure_due: None,
            failures: 0,
            local_since_failure: false,
            retry_not_before: None,
            boundaries: Vec::new(),
            jitter,
        }
    }

    /// The transition stamp distinguishes A -> B -> A even if this worker was
    /// blocked in a request while B was observed by the main loop.
    pub(super) fn update(
        &mut self,
        source: &super::passive::Source,
        active: bool,
        now: Instant,
        now_epoch: u64,
    ) {
        if source.account_changed_at < self.account_changed_at {
            return;
        }
        if source.account_changed_at > self.account_changed_at {
            let jitter = self.jitter.clone();
            *self = Self::with_jitter(now, jitter);
            self.account_changed_at = source.account_changed_at;
        }
        self.update_source(
            source.account.as_deref(),
            source.account_observed_at,
            &source.windows,
            source.latest_local,
            active,
            now,
            now_epoch,
        );
    }

    /// Refresh the shared view. Account ownership changes start a fresh grace
    /// period; activity changes alone never restart an accumulated fallback wait.
    #[allow(clippy::too_many_arguments)] // Source metadata plus explicitly injected clocks.
    fn update_source(
        &mut self,
        account: Option<&str>,
        account_observed_at: u64,
        windows: &[QuotaWindow],
        latest_local: Option<Instant>,
        active: bool,
        now: Instant,
        now_epoch: u64,
    ) {
        self.active = active;
        if account_observed_at < self.account_observed_at {
            return;
        }
        self.adopt_account(account, account_observed_at, now);
        self.windows = windows.to_vec();
        let new_local = latest_local
            .filter(|at| *at <= now && self.last_local_seen.is_none_or(|previous| *at > previous));
        // Revoking coverage must not forget the observation watermark: an old
        // candidate returning after an API projection cannot clear error backoff.
        self.latest_local = latest_local.filter(|at| *at <= now);
        if let Some(at) = new_local {
            self.last_local_seen = Some(at);
            self.local_since_failure = self.failure_due.is_some();
            self.unchanged = false;
            self.interval = self.base_interval();
            self.normal_due = at + self.interval;
        } else if self.last_fetch.is_none() && !self.windows.is_empty() && !self.unchanged {
            self.interval = self.base_interval();
            let anchor = self.latest_local.unwrap_or(self.started_at);
            self.normal_due = anchor + self.interval;
        }
        self.update_boundaries(windows, now, now_epoch);
    }

    fn adopt_account(&mut self, account: Option<&str>, observed_at: u64, now: Instant) {
        if self.account.as_deref() != account {
            // Learning the first identity is not an account switch. Preserve the
            // original startup grace and a request that already supplied it.
            if self.account.is_some() || self.account_observed_at != 0 {
                let active = self.active;
                let jitter = self.jitter.clone();
                let account_changed_at = self.account_changed_at;
                *self = Self::with_jitter(now, jitter);
                self.active = active;
                self.account_changed_at = account_changed_at;
            }
            self.account = account.map(str::to_owned);
        }
        self.account_observed_at = observed_at;
    }

    /// Adopt the identity returned by the same request before settling it. This
    /// prevents the next main-loop projection from turning first discovery into
    /// a second startup fetch. Results for an older account are ignored.
    pub(super) fn settle_account(
        &mut self,
        account: Option<&str>,
        started_at: u64,
        result: &FetchOutcome,
        now: Instant,
        now_epoch: u64,
    ) {
        if started_at < self.account_changed_at
            || (self.account.as_deref() != account && started_at < self.account_observed_at)
        {
            return;
        }
        if result.is_ok() {
            self.adopt_account(account, started_at.max(self.account_observed_at), now);
        }
        self.settle(result, now, now_epoch);
    }

    fn base_interval(&self) -> Duration {
        if self.active && !self.windows.iter().any(|w| w.remaining_percent == 0) {
            ACTIVE_GAP
        } else {
            IDLE_GAP
        }
    }

    fn update_boundaries(&mut self, windows: &[QuotaWindow], now: Instant, now_epoch: u64) {
        self.boundaries.retain(|boundary| {
            windows.iter().any(|window| {
                window.label == boundary.label
                    && window
                        .resets_at
                        .is_some_and(|stamp| stamp.abs_diff(boundary.stamp) <= RESET_TOLERANCE)
            })
        });
        for window in windows {
            let Some(stamp) = window.resets_at.filter(|stamp| *stamp > 0) else {
                continue;
            };
            if self.boundaries.iter().any(|b| b.label == window.label) {
                continue;
            }
            let buffer = self
                .jitter
                .between(Duration::from_secs(30), Duration::from_secs(60));
            let Some(due) = now
                .checked_add(Duration::from_secs(stamp.saturating_sub(now_epoch)))
                .and_then(|at| at.checked_add(buffer))
            else {
                continue;
            };
            self.boundaries.push(Boundary {
                label: window.label.clone(),
                stamp,
                due: Some(due),
                attempts: 0,
            });
        }
    }

    fn due(&self) -> Instant {
        let ordinary = self
            .boundaries
            .iter()
            .filter_map(|boundary| boundary.due)
            .fold(self.normal_due, Instant::min);
        // Recovery suppresses unnecessary retries, but only API success clears
        // the failure streak. Reset checks still run after the error minimum.
        let due = self.failure_due.map_or(ordinary, |failure| {
            if self.local_since_failure && self.latest_local.is_some() {
                failure.max(ordinary)
            } else {
                failure
            }
        });
        self.retry_not_before
            .map_or(due, |minimum| due.max(minimum))
    }

    pub(super) fn should_fetch(&self, now: Instant, forced: bool) -> bool {
        self.retry_not_before.is_none_or(|minimum| now >= minimum) && (forced || now >= self.due())
    }

    /// Settle one request after it completes. A failed attempt never counts as an
    /// unchanged snapshot. Manual requests share this same settlement path.
    pub(super) fn settle(&mut self, result: &FetchOutcome, now: Instant, now_epoch: u64) {
        self.last_fetch = Some(now);
        if let Err(error) = result {
            self.local_since_failure = false;
            self.failures = self.failures.saturating_add(1);
            let minutes = match self.failures {
                1 => 5,
                2 => 10,
                3 => 20,
                _ => 30,
            };
            let delay = Duration::from_secs(minutes * 60)
                + self.jitter.between(Duration::ZERO, Duration::from_secs(30));
            self.failure_due = Some(now + delay);
            if let Some(wait) = error.retry_after.and_then(|wait| now.checked_add(wait)) {
                self.retry_not_before =
                    Some(self.retry_not_before.map_or(wait, |old| old.max(wait)));
            }
            return;
        }
        self.failures = 0;
        self.failure_due = None;
        self.local_since_failure = false;
        self.retry_not_before = None;
        let windows = match result {
            Ok(QuotaFetch::Available(windows)) => windows.as_slice(),
            Ok(QuotaFetch::Unavailable) => &[],
            Err(_) => unreachable!("handled above"),
        };
        let unchanged = same_windows(&self.windows, windows);
        if unchanged {
            self.interval = self
                .interval
                .max(self.base_interval())
                .saturating_mul(2)
                .min(MAX_UNCHANGED);
        } else {
            self.windows = windows.to_vec();
            self.interval = self.base_interval();
        }
        self.unchanged = unchanged;
        self.normal_due = now + self.interval;

        // One ordinary request after a boundary counts as its first check, even
        // if it was manually requested or the ordinary fallback was also due.
        for boundary in &mut self.boundaries {
            if boundary.stamp <= now_epoch {
                boundary.attempts = boundary.attempts.saturating_add(1);
                boundary.due = (boundary.attempts < 2).then_some(now + RESET_RETRY);
            }
        }
        self.windows = windows.to_vec();
        self.update_boundaries(windows, now, now_epoch);
    }

    pub(super) fn wait(&self, now: Instant) -> Duration {
        self.due().saturating_duration_since(now)
    }
}

/// Reset clocks occasionally move by one second. Treat that as the same window
/// so it cannot defeat the unchanged backoff or schedule fresh boundary retries.
fn same_windows(previous: &[QuotaWindow], next: &[QuotaWindow]) -> bool {
    previous.len() == next.len()
        && previous.iter().all(|before| {
            next.iter().any(|after| {
                before.label == after.label
                    && before.remaining_percent == after.remaining_percent
                    && match (before.resets_at, after.resets_at) {
                        (Some(a), Some(b)) => a.abs_diff(b) <= RESET_TOLERANCE,
                        (None, None) => true,
                        _ => false,
                    }
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{FetchError, FetchKind};

    fn window(remaining: u8, reset: Option<u64>) -> Vec<QuotaWindow> {
        vec![QuotaWindow {
            label: "wk".into(),
            remaining_percent: remaining,
            resets_at: reset,
        }]
    }

    fn cadence(now: Instant) -> CodexCadence {
        CodexCadence::with_jitter(now, Jitter::seeded(7))
    }

    #[test]
    fn unrepresentable_reset_keeps_ordinary_fallback_and_valid_boundaries() {
        let now = Instant::now();
        let mut state = cadence(now);
        let mut windows = window(80, Some(u64::MAX));
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        assert!(state.boundaries.is_empty());
        assert_eq!(state.wait(now), ACTIVE_GAP);

        windows.push(QuotaWindow {
            label: "5h".into(),
            remaining_percent: 90,
            resets_at: Some(1010),
        });
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        assert_eq!(state.boundaries.len(), 1);
        assert_eq!(state.boundaries[0].label, "5h");
        assert!((Duration::from_secs(40)..=Duration::from_secs(70)).contains(&state.wait(now)));
    }

    #[test]
    fn unseen_account_roundtrip_restarts_grace_and_rejects_old_same_account_results() {
        let start = Instant::now();
        let mut state = cadence(start);
        let initial = super::super::passive::Source {
            account: Some("a".into()),
            account_observed_at: 1000,
            ..Default::default()
        };
        state.update(&initial, true, start, 1);
        let completed = start + Duration::from_secs(30);
        // The worker never saw B. Only the transition stamp proves this is a
        // different A generation, despite an identical account name.
        let current = super::super::passive::Source {
            account_observed_at: 3000,
            account_changed_at: 2500,
            ..initial.clone()
        };
        state.update(&current, true, completed, 3);
        assert_eq!(state.wait(completed), STARTUP_GRACE);
        for result in [
            Ok(QuotaFetch::Available(window(80, None))),
            Err(FetchError::with_kind("timeout", FetchKind::Timeout)),
        ] {
            state.settle_account(Some("a"), 1500, &result, completed, 3);
            assert_eq!(state.wait(completed), STARTUP_GRACE);
            assert!(state.last_fetch.is_none());
            assert_eq!(state.failures, 0);
        }
        state.update(&initial, true, completed, 3);
        assert_eq!(state.account_changed_at, 2500);
    }

    #[test]
    fn first_identity_and_later_same_generation_samples_allow_inflight_success() {
        let start = Instant::now();
        let mut state = cadence(start);
        let result = Ok(QuotaFetch::Available(window(80, None)));
        let source = super::super::passive::Source {
            account: Some("a".into()),
            account_observed_at: 3000,
            ..Default::default()
        };
        let completed = start + Duration::from_secs(30);
        state.update(&source, true, completed, 3);
        assert_eq!(state.wait(completed), Duration::from_secs(30));
        state.settle_account(Some("a"), 1000, &result, completed, 3);
        assert_eq!(state.last_fetch, Some(completed));
        assert_eq!(state.windows, window(80, None));
        assert_eq!(state.wait(completed), ACTIVE_GAP);
    }

    #[test]
    fn startup_waits_and_repeated_local_records_do_not_extend_the_gap() {
        let now = Instant::now();
        let mut state = cadence(now);
        assert!(!state.should_fetch(now + Duration::from_secs(59), false));
        assert!(state.should_fetch(now + STARTUP_GRACE, false));
        let windows = window(80, None);
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        state.update_source(
            Some("a"),
            1000,
            &windows,
            Some(now),
            true,
            now + Duration::from_secs(299),
            1299,
        );
        assert!(!state.should_fetch(now + Duration::from_secs(299), false));
        assert!(state.should_fetch(now + ACTIVE_GAP, false));
        state.update_source(
            Some("a"),
            1000,
            &windows,
            Some(now + ACTIVE_GAP),
            true,
            now + ACTIVE_GAP,
            1300,
        );
        assert!(!state.should_fetch(now + ACTIVE_GAP, false));
    }

    #[test]
    fn unchanged_backoff_exceeds_an_hour_and_survives_activity_flips() {
        let start = Instant::now();
        let mut state = cadence(start);
        let windows = window(80, None);
        state.update_source(Some("a"), 1000, &windows, Some(start), true, start, 1000);
        let mut now = start + ACTIVE_GAP;
        for expected_minutes in [10, 20, 40, 80, 160, 240, 240] {
            state.settle(&Ok(QuotaFetch::Available(windows.clone())), now, 1000);
            assert_eq!(state.wait(now), Duration::from_secs(expected_minutes * 60));
            state.update_source(Some("a"), 1000, &windows, Some(start), false, now, 1000);
            state.update_source(Some("a"), 1000, &windows, Some(start), true, now, 1000);
            assert_eq!(state.wait(now), Duration::from_secs(expected_minutes * 60));
            now += state.wait(now);
        }
        state.settle(&Ok(QuotaFetch::Available(window(70, None))), now, 1000);
        assert_eq!(state.wait(now), ACTIVE_GAP);
    }

    #[test]
    fn idle_and_exhausted_sources_use_long_initial_wait() {
        let now = Instant::now();
        for (active, remaining) in [(false, 80), (true, 0)] {
            let mut state = cadence(now);
            state.update_source(
                Some("a"),
                1000,
                &window(remaining, None),
                Some(now),
                active,
                now,
                1000,
            );
            assert_eq!(state.wait(now), IDLE_GAP);
        }
    }

    #[test]
    fn account_change_drops_old_backoff_and_reset_deadlines() {
        let now = Instant::now();
        let mut state = cadence(now);
        state.update_source(
            Some("a"),
            1000,
            &window(80, Some(1001)),
            Some(now),
            true,
            now,
            1000,
        );
        state.settle(
            &Ok(QuotaFetch::Available(window(80, Some(1001)))),
            now,
            1000,
        );
        state.update_source(Some("b"), 1001, &[], None, true, now, 1000);
        assert_eq!(state.wait(now), STARTUP_GRACE);
    }

    #[test]
    fn local_recovery_preserves_error_retry_and_provider_minimum() {
        let now = Instant::now();
        let mut state = cadence(now);
        let windows = window(80, None);
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        state.settle(
            &Err(FetchError::throttled(
                "wait",
                Some(Duration::from_secs(3600)),
            )),
            now,
            1000,
        );
        assert!(!state.should_fetch(now, true));
        let recovered = now + Duration::from_secs(10);
        state.update_source(
            Some("a"),
            1000,
            &windows,
            Some(recovered),
            true,
            recovered,
            1010,
        );
        assert!(state.failure_due.is_some());
        assert_eq!(state.failures, 1);
        assert!(!state.should_fetch(recovered + ACTIVE_GAP, true));
        assert!(state.should_fetch(now + Duration::from_secs(3600), true));
    }

    #[test]
    fn local_recovery_preserves_failure_streak_and_waits_for_both_deadlines() {
        let start = Instant::now();
        let mut state = cadence(start);
        let windows = window(80, None);
        let error = Err(FetchError::with_kind("timeout", FetchKind::Timeout));
        state.update_source(Some("a"), 1000, &windows, Some(start), true, start, 1000);
        state.settle(&error, start, 1000);
        let second = start + state.wait(start);
        state.settle(&error, second, 1300);
        let retry = state.failure_due.unwrap();
        let recovered = second + Duration::from_secs(1);
        state.update_source(
            Some("a"),
            1000,
            &windows,
            Some(recovered),
            true,
            recovered,
            1301,
        );
        assert_eq!(state.failures, 2);
        assert!(!state.should_fetch(recovered + ACTIVE_GAP, false));
        assert!(state.should_fetch(retry, false));

        // Continuing records suppress an overdue retry without erasing it.
        state.update_source(Some("a"), 1000, &windows, Some(retry), true, retry, 1900);
        assert!(!state.should_fetch(retry, false));
        assert!(!state.should_fetch(retry + ACTIVE_GAP - Duration::from_secs(1), false));
        assert!(state.should_fetch(retry + ACTIVE_GAP, false));
        state.settle(&error, retry + ACTIVE_GAP, 2200);
        assert_eq!(state.failures, 3);
        assert!(state.wait(retry + ACTIVE_GAP) >= Duration::from_secs(20 * 60));
        state.settle(
            &Ok(QuotaFetch::Available(windows)),
            retry + ACTIVE_GAP,
            2200,
        );
        assert_eq!(state.failures, 0);
        assert!(state.failure_due.is_none());
    }

    #[test]
    fn reset_checks_survive_local_recovery_but_respect_failure_backoff() {
        let start = Instant::now();
        let mut state = cadence(start);
        let windows = window(80, Some(1010));
        let error = Err(FetchError::with_kind("timeout", FetchKind::Timeout));
        state.update_source(Some("a"), 1000, &windows, Some(start), true, start, 1000);
        state.settle(&error, start, 1000);
        let retry = state.failure_due.unwrap();
        let recovered = start + Duration::from_secs(1);
        state.update_source(
            Some("a"),
            1000,
            &windows,
            Some(recovered),
            true,
            recovered,
            1001,
        );
        assert!(!state.should_fetch(start + Duration::from_secs(70), false));
        // Ordinary local fallback is later, but the reset check may run once
        // the independent error deadline allows it.
        assert!(state.should_fetch(retry, false));
        state.settle(&Ok(QuotaFetch::Available(windows.clone())), retry, 1400);
        assert_eq!(state.boundaries[0].attempts, 1);
        assert_eq!(state.wait(retry), RESET_RETRY);
        let second = retry + RESET_RETRY;
        state.settle(&Ok(QuotaFetch::Available(windows)), second, 1520);
        assert_eq!(state.boundaries[0].attempts, 2);
        assert!(state.boundaries[0].due.is_none());
    }

    #[test]
    fn errors_back_off_without_becoming_unchanged_samples() {
        let mut now = Instant::now();
        let mut state = cadence(now);
        let error = Err(FetchError::with_kind("timeout", FetchKind::Timeout));
        for minutes in [5, 10, 20, 30, 30] {
            state.settle(&error, now, 1000);
            assert!(state.wait(now) >= Duration::from_secs(minutes * 60));
            assert!(state.wait(now) <= Duration::from_secs(minutes * 60 + 30));
            assert!(!state.unchanged);
            now += state.wait(now);
        }
    }

    #[test]
    fn reset_checks_are_bounded_and_one_second_drift_does_not_rearm_them() {
        let now = Instant::now();
        let mut state = cadence(now);
        let windows = window(0, Some(1010));
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        let first_wait = state.wait(now);
        assert!((Duration::from_secs(40)..=Duration::from_secs(70)).contains(&first_wait));
        let first = now + first_wait;
        state.settle(
            &Ok(QuotaFetch::Available(window(0, Some(1011)))),
            first,
            1070,
        );
        assert_eq!(state.wait(first), RESET_RETRY);
        let second = first + RESET_RETRY;
        state.settle(&Ok(QuotaFetch::Available(windows.clone())), second, 1190);
        assert!(state.wait(second) >= Duration::from_secs(3600));
        state.update_source(Some("a"), 1000, &windows, Some(now), true, second, 1190);
        assert!(state.wait(second) >= Duration::from_secs(3600));
        state.update_source(
            Some("a"),
            1000,
            &window(100, Some(2000)),
            Some(second),
            true,
            second,
            1190,
        );
        assert!(state.boundaries[0].attempts == 0);
        assert!(state.boundaries[0].stamp == 2000);
    }
    #[test]
    fn first_identity_discovery_preserves_startup_and_completed_request_schedule() {
        let now = Instant::now();
        let mut state = cadence(now);
        state.update_source(
            Some("a"),
            1030,
            &[],
            None,
            true,
            now + Duration::from_secs(30),
            1030,
        );
        assert_eq!(
            state.wait(now + Duration::from_secs(30)),
            Duration::from_secs(30)
        );

        let mut state = cadence(now);
        let result = Ok(QuotaFetch::Available(window(80, None)));
        let completed = now + STARTUP_GRACE;
        state.settle_account(Some("a"), 1060, &result, completed, 1060);
        let due = state.wait(completed);
        // A lagging shared projection must not erase the just-settled identity.
        state.update_source(None, 0, &[], None, true, completed, 1060);
        assert_eq!(state.account.as_deref(), Some("a"));
        assert_eq!(state.wait(completed), due);
        state.update_source(
            Some("a"),
            1060,
            &window(80, None),
            None,
            true,
            completed,
            1060,
        );
        assert_eq!(state.wait(completed), due);
    }

    #[test]
    fn stale_results_and_sources_cannot_transfer_backoff_to_another_account() {
        let now = Instant::now();
        let mut state = cadence(now);
        state.update_source(
            Some("b"),
            2000,
            &window(70, None),
            Some(now),
            true,
            now,
            2000,
        );
        state.settle_account(
            Some("a"),
            1999,
            &Err(FetchError::with_kind("timeout", FetchKind::Timeout)),
            now,
            2000,
        );
        assert!(state.failure_due.is_none());
        state.settle_account(
            Some("a"),
            1999,
            &Ok(QuotaFetch::Available(window(80, None))),
            now,
            2000,
        );
        assert_eq!(state.account.as_deref(), Some("b"));
        state.update_source(Some("a"), 1999, &window(80, None), None, true, now, 2000);
        assert_eq!(state.account.as_deref(), Some("b"));
        assert_eq!(state.windows, window(70, None));
    }

    #[test]
    fn revoking_and_repeating_local_coverage_does_not_clear_failure_backoff() {
        let now = Instant::now();
        let mut state = cadence(now);
        let windows = window(80, None);
        state.update_source(Some("a"), 1000, &windows, Some(now), true, now, 1000);
        let later = now + Duration::from_secs(5);
        state.settle_account(
            Some("a"),
            1001,
            &Err(FetchError::with_kind("timeout", FetchKind::Timeout)),
            later,
            1005,
        );
        let wait = state.wait(later);
        state.update_source(Some("a"), 1005, &windows, None, true, later, 1005);
        assert_eq!(state.wait(later), wait);
        state.update_source(Some("a"), 1005, &windows, Some(now), true, later, 1005);
        assert_eq!(state.wait(later), wait);
        assert_eq!(state.failures, 1);
    }
}
