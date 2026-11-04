//! Ephemeral official quota observations. Account ownership is the daemon's;
//! API results are a baseline, not a prerequisite for local display updates.
use std::time::{Duration, Instant};

use super::{FetchFailure, FetchOutcome, QuotaFetch, QuotaWindow, SubscriptionQuota};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub session: String,
    pub turn: String,
    pub account: String,
    /// Rollout event time, Unix milliseconds (an accepted sampling approximation).
    pub observed_at: u64,
    pub windows: Vec<QuotaWindow>,
}

/// Latest projected source information consumed by the existing quota worker.
#[derive(Debug, Clone, Default)]
pub(super) struct Source {
    pub account: Option<String>,
    pub account_observed_at: u64,
    pub account_changed_at: u64,
    pub windows: Vec<QuotaWindow>,
    /// Oldest local observation among all displayed windows; never publication time.
    pub latest_local: Option<Instant>,
}

#[derive(Debug, Default)]
pub(super) struct State {
    clock: Option<(Instant, u64)>,
    account: Option<String>,
    account_observed_at: u64,
    /// Requests predating an actual account transition cannot restore an old generation.
    account_changed_at: u64,
    api_started_at: u64,
    api: Option<SubscriptionQuota>,
    failure: Option<FetchFailure>,
    observations: Vec<Observation>,
}

impl State {
    fn account(&mut self, account: String, at: u64) -> bool {
        if at < self.account_observed_at {
            return self.account.as_deref() == Some(&account);
        }
        if self.account.as_deref() != Some(&account) {
            // First discovery can happen while the startup request is in flight.
            // Later transitions, including logout/login, delimit account generations.
            if self.account.is_some() || self.account_observed_at != 0 {
                self.account_changed_at = at;
            }
            self.api = None;
            self.failure = None;
            self.api_started_at = 0;
            self.observations.clear();
        }
        self.account = Some(account);
        self.account_observed_at = at;
        true
    }

    pub(super) fn replace(
        &mut self,
        account: Option<&str>,
        observed_at: u64,
        observations: Vec<Observation>,
    ) -> bool {
        // A failed account probe supplies no new identity. Keep the last known
        // display, but do not import candidates from an unidentified runtime.
        let Some(account) = account else {
            // The collector can revoke retained sources using durable metadata
            // even while the live identity probe fails. It admits no new source
            // in this case, and cannot change the account owner.
            if observed_at >= self.account_observed_at && observed_at != 0 {
                self.observations = observations
                    .into_iter()
                    .filter(|o| Some(o.account.as_str()) == self.account.as_deref())
                    .collect();
            }
            return false;
        };
        if observed_at < self.account_observed_at {
            return false;
        }
        self.account(account.to_owned(), observed_at);
        self.observations = observations
            .into_iter()
            .filter(|o| o.account == account)
            .collect();
        true
    }

    /// False means a result for an obsolete account must not update its cycle.
    pub(super) fn apply_api(
        &mut self,
        account: Option<&str>,
        result: &FetchOutcome,
        started_at: u64,
    ) -> bool {
        if started_at < self.account_changed_at {
            return false;
        }
        if result.is_err() {
            if account.is_some_and(|account| self.account.as_deref() != Some(account))
                || (account.is_none() && started_at < self.account_observed_at)
            {
                return false;
            }
        } else if let Some(account) = account {
            if !self.account(account.to_owned(), started_at) {
                return false;
            }
        } else if started_at < self.account_observed_at {
            return false;
        }
        match result {
            Ok(QuotaFetch::Available(windows)) => {
                if account.is_none() {
                    return false;
                }
                self.api_started_at = started_at;
                self.api = Some(SubscriptionQuota::new(windows.clone()));
                self.failure = None;
            }
            Ok(QuotaFetch::Unavailable) => {
                if self.account.is_some() {
                    self.account_changed_at = started_at;
                }
                self.account = None;
                self.account_observed_at = started_at;
                self.api = None;
                self.failure = None;
                self.observations.clear();
            }
            Err(error) => {
                if let Some(api) = &mut self.api {
                    api.fetch_failed = true;
                    api.last_error_kind = Some(error.kind);
                }
                let failure = self.failure.get_or_insert(FetchFailure {
                    first_failed_at: Instant::now(),
                    kind: error.kind,
                });
                failure.kind = error.kind;
            }
        }
        true
    }

    pub(super) fn project(
        &mut self,
        now: Instant,
        now_epoch_ms: u64,
    ) -> (Option<SubscriptionQuota>, Option<FetchFailure>, Source) {
        let (anchor, epoch) = *self.clock.get_or_insert((now, now_epoch_ms));
        let mut quota = self.api.clone();
        let mut local_times = Vec::new();
        for label in ["5h", "wk"] {
            let candidate = self
                .observations
                .iter()
                .filter(|o| {
                    Some(o.account.as_str()) == self.account.as_deref()
                        && o.observed_at > self.api_started_at
                        && o.observed_at <= now_epoch_ms
                })
                .filter_map(|o| o.windows.iter().find(|w| w.label == label).map(|w| (o, w)))
                .max_by_key(|(o, _)| o.observed_at);
            let Some((observation, window)) = candidate else {
                continue;
            };
            // Two disagreeing observations at the same timestamp have no
            // reliable order. Leave this window on the API baseline.
            if self.observations.iter().any(|o| {
                o.observed_at == observation.observed_at
                    && o.account == observation.account
                    && o.windows.iter().any(|w| w.label == label && w != window)
            }) {
                continue;
            }
            let at = if observation.observed_at >= epoch {
                anchor + Duration::from_millis(observation.observed_at - epoch)
            } else {
                anchor
                    .checked_sub(Duration::from_millis(epoch - observation.observed_at))
                    .unwrap_or(anchor)
            };
            let q = quota.get_or_insert_with(|| SubscriptionQuota::new(Vec::new()));
            let mut window = window.clone();
            if let Some(previous) = q.windows.iter_mut().find(|w| w.label == label) {
                window.resets_at = window.resets_at.or(previous.resets_at);
                *previous = window;
            } else {
                q.windows.push(window);
            }
            local_times.push(at);
        }
        let mut source = Source {
            account: self.account.clone(),
            account_observed_at: self.account_observed_at.max(self.api_started_at),
            account_changed_at: self.account_changed_at,
            ..Source::default()
        };
        if let Some(q) = &mut quota {
            q.windows
                .sort_by_key(|w| if w.label == "5h" { 0 } else { 1 });
            source.windows = q.windows.clone();
            if !q.windows.is_empty() && local_times.len() == q.windows.len() {
                source.latest_local = local_times.into_iter().min();
                if let Some(at) = source.latest_local {
                    q.fetched_at = at;
                    if now.saturating_duration_since(at) < Duration::from_secs(5 * 60) {
                        q.fetch_failed = false;
                        q.last_error_kind = None;
                    } else if let Some(failure) = self.failure {
                        q.fetch_failed = true;
                        q.last_error_kind = Some(failure.kind);
                    }
                }
            } else if let Some(failure) = self.failure {
                q.fetch_failed = true;
                q.last_error_kind = Some(failure.kind);
            }
        }
        let failure = quota.is_none().then_some(self.failure).flatten();
        (quota, failure, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{FetchError, FetchKind};

    fn windows(percent: u8) -> Vec<QuotaWindow> {
        vec![QuotaWindow {
            label: "wk".into(),
            remaining_percent: percent,
            resets_at: Some(9999),
        }]
    }
    fn observation(account: &str, at: u64, percent: u8) -> Observation {
        Observation {
            session: "session".into(),
            turn: "turn".into(),
            account: account.into(),
            observed_at: at,
            windows: windows(percent),
        }
    }
    #[test]
    fn queued_usage_for_old_account_cannot_replace_current_account() {
        let mut app = crate::state::AppState::new("%99".into());
        let update = |account: &str, at| crate::usage::Update {
            history: Default::default(),
            spends: Default::default(),
            codex_all: Default::default(),
            codex_account: Default::default(),
            codex_account_id: Some(account.into()),
            codex_observed_at: at,
            codex_quota: vec![observation(account, at, 70)],
        };
        app.apply_usage_update(update("b", 1000));
        app.apply_quota_result(crate::quota::QuotaFetchResult {
            codex: Some(Ok(QuotaFetch::Available(windows(65)))),
            codex_started_at: 1100,
            codex_account: Some("b".into()),
            kimi: None,
            forced: false,
        });
        // This snapshot was sampled before the B request, then queued on the
        // independent usage channel while that request completed.
        app.apply_usage_update(update("a", 1050));
        assert_eq!(app.usage.codex_account_id.as_deref(), Some("b"));
        assert_eq!(app.quota.codex.as_ref().unwrap().windows, windows(65));
        assert_eq!(app.quota.passive.account.as_deref(), Some("b"));
    }

    #[test]
    fn account_round_trip_rejects_old_generation_api_success_and_error() {
        let mut state = State::default();
        state.replace(Some("a"), 90, vec![]);
        state.replace(Some("b"), 110, vec![]);
        state.replace(Some("a"), 120, vec![observation("a", 120, 70)]);
        assert_eq!(state.project(Instant::now(), 130).2.account_changed_at, 120);
        assert!(!state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(90))), 100));
        assert!(!state.apply_api(
            Some("a"),
            &Err(FetchError::with_kind("timeout", FetchKind::Timeout)),
            100,
        ));
        assert!(state.api.is_none());
        assert!(state.failure.is_none());
        assert_eq!(
            state.project(Instant::now(), 130).0.unwrap().windows,
            windows(70)
        );
    }

    #[test]
    fn logout_and_login_reject_requests_from_the_previous_generation() {
        let mut state = State::default();
        state.replace(Some("a"), 90, vec![]);
        assert!(state.apply_api(None, &Ok(QuotaFetch::Unavailable), 110));
        assert_eq!(state.project(Instant::now(), 115).2.account_changed_at, 110);
        state.replace(Some("a"), 120, vec![]);
        assert_eq!(state.account_changed_at, 120);
        assert!(!state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(90))), 100));
        assert!(!state.apply_api(None, &Ok(QuotaFetch::Unavailable), 115));
        assert_eq!(state.account.as_deref(), Some("a"));
    }

    #[test]
    fn slow_api_within_current_generation_remains_a_revocation_baseline() {
        let mut state = State::default();
        // Initial identity discovery does not invalidate the startup request.
        state.replace(Some("a"), 110, vec![observation("a", 120, 70)]);
        assert_eq!(state.account_changed_at, 0);
        assert!(state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(90))), 100));
        state.replace(Some("b"), 130, vec![]);
        state.replace(Some("b"), 150, vec![observation("b", 150, 60)]);
        assert!(state.apply_api(Some("b"), &Ok(QuotaFetch::Available(windows(80))), 140));
        assert_eq!(
            state.project(Instant::now(), 160).0.unwrap().windows,
            windows(60)
        );
        state.replace(Some("b"), 160, vec![]);
        assert_eq!(
            state.project(Instant::now(), 160).0.unwrap().windows,
            windows(80)
        );
    }

    #[test]
    fn cached_error_identity_cannot_transfer_owner_or_drop_local_data() {
        let mut state = State::default();
        state.replace(Some("b"), 900, vec![observation("b", 950, 70)]);
        let error = Err(FetchError::with_kind("timeout", FetchKind::Timeout));
        // A request launched from a cached A source after the B account was
        // sampled, but failed without any authoritative account result.
        assert!(!state.apply_api(Some("a"), &error, 1000));
        let (quota, failure, source) = state.project(Instant::now(), 1100);
        assert_eq!(source.account.as_deref(), Some("b"));
        assert_eq!(quota.unwrap().windows, windows(70));
        assert!(failure.is_none());
        assert_eq!(state.account_observed_at, 900);
    }

    #[test]
    fn healthy_usage_without_daemon_account_still_updates_global_totals() {
        let mut app = crate::state::AppState::new("%99".into());
        let mut spends: [crate::usage::Spend; 4] = Default::default();
        spends[0].cost_cny = 12.5;
        app.apply_usage_update(crate::usage::Update {
            history: Default::default(),
            spends,
            codex_all: Default::default(),
            codex_account: Default::default(),
            codex_account_id: None,
            codex_observed_at: 1000,
            codex_quota: Vec::new(),
        });
        assert!(app.usage.received);
        assert!(app.usage.codex_all.is_some());
        assert_eq!(app.usage.spends.as_ref().unwrap()[0].cost_cny, 12.5);
        assert!(app.usage.codex_account_id.is_none());
    }

    #[test]
    fn passive_bootstraps_survives_api_error_and_revokes_conflict() {
        let mut state = State::default();
        state.replace(Some("a"), 1000, vec![observation("a", 1000, 80)]);
        assert_eq!(
            state.project(Instant::now(), 1100).0.unwrap().windows,
            windows(80)
        );
        state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(90))), 900);
        state.apply_api(
            None,
            &Err(FetchError::with_kind("timeout", FetchKind::Timeout)),
            1050,
        );
        let q = state.project(Instant::now(), 1100).0.unwrap();
        assert_eq!(q.windows, windows(80));
        assert!(!q.fetch_failed);
        state.replace(Some("a"), 1200, vec![]);
        let q = state.project(Instant::now(), 1200).0.unwrap();
        assert_eq!(q.windows, windows(90));
        assert!(q.fetch_failed);
    }
    #[test]
    fn api_ordering_and_account_switch_reject_stale_deliveries() {
        let mut state = State::default();
        state.replace(Some("a"), 1000, vec![observation("a", 1100, 70)]);
        state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(80))), 1050);
        assert_eq!(
            state.project(Instant::now(), 1200).0.unwrap().windows,
            windows(70)
        );
        state.replace(Some("b"), 1300, vec![observation("b", 1300, 50)]);
        assert!(!state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(80))), 1200));
        state.replace(Some("a"), 1200, vec![observation("a", 1200, 90)]);
        assert_eq!(
            state.project(Instant::now(), 1400).0.unwrap().windows,
            windows(50)
        );
    }
    #[test]
    fn repeats_do_not_renew_freshness_and_old_logs_do_not_beat_api() {
        let mut state = State::default();
        state.replace(Some("a"), 1000, vec![observation("a", 1000, 70)]);
        let now = Instant::now();
        let at = state.project(now, 2000).2.latest_local.unwrap();
        state.replace(Some("a"), 2000, vec![observation("a", 1000, 70)]);
        assert_eq!(state.project(now, 2000).2.latest_local, Some(at));
        state.apply_api(Some("a"), &Ok(QuotaFetch::Available(windows(60))), 2100);
        let (q, _, source) = state.project(now, 2200);
        assert_eq!(q.unwrap().windows, windows(60));
        assert!(source.latest_local.is_none());
    }
    #[test]
    fn failed_identity_probe_still_revokes_conflicted_overlay_through_app_update() {
        let mut app = crate::state::AppState::new("%99".into());
        app.now = crate::time::now_epoch_secs();
        let mut baseline = windows(90);
        baseline[0].resets_at = Some(app.now + 3600);
        app.apply_quota_result(crate::quota::QuotaFetchResult {
            codex: Some(Ok(QuotaFetch::Available(baseline.clone()))),
            codex_started_at: 1000,
            codex_account: Some("a".into()),
            kimi: None,
            forced: false,
        });
        app.apply_usage_update(crate::usage::Update {
            history: Default::default(),
            spends: Default::default(),
            codex_all: Default::default(),
            codex_account: Default::default(),
            codex_account_id: Some("a".into()),
            codex_observed_at: 2000,
            codex_quota: vec![observation("a", 2000, 70)],
        });
        assert_eq!(app.quota.codex.as_ref().unwrap().windows, windows(70));

        // The collector discovered sticky conflict in metadata while the live
        // account probe failed. Empty replacement revokes the overlay without
        // signing out or discarding the known-account API baseline.
        app.apply_usage_update(crate::usage::Update {
            history: Default::default(),
            spends: Default::default(),
            codex_all: Default::default(),
            codex_account: Default::default(),
            codex_account_id: None,
            codex_observed_at: 3000,
            codex_quota: Vec::new(),
        });
        assert_eq!(app.quota.codex.as_ref().unwrap().windows, baseline);
        assert_eq!(app.usage.codex_account_id.as_deref(), Some("a"));
        assert_eq!(app.quota.passive.account.as_deref(), Some("a"));
        assert!(app.quota.passive.observations.is_empty());
        assert_eq!(
            app.usage.cycle.lock().unwrap().account.as_deref(),
            Some("a")
        );
    }

    #[test]
    fn explicit_daemon_logout_clears_quota_usage_account_and_cycle() {
        let end = crate::time::now_epoch_secs() + 3600;
        let mut available = windows(65);
        available[0].resets_at = Some(end);
        let mut app = crate::state::AppState::new("%99".into());
        app.now = crate::time::now_epoch_secs();
        app.apply_quota_result(crate::quota::QuotaFetchResult {
            codex: Some(Ok(QuotaFetch::Available(available))),
            codex_started_at: 1000,
            codex_account: Some("a".into()),
            kimi: None,
            forced: false,
        });
        app.usage.codex_account = Some(Default::default());
        assert_eq!(app.usage.codex_account_id.as_deref(), Some("a"));
        assert_eq!(app.usage.cycle.lock().unwrap().end, end);

        // Unavailable is a successful signed-out/nonofficial response, unlike
        // an unknown identity caused by a failed probe.
        app.apply_quota_result(crate::quota::QuotaFetchResult {
            codex: Some(Ok(QuotaFetch::Unavailable)),
            codex_started_at: 2000,
            codex_account: None,
            kimi: None,
            forced: false,
        });
        assert!(app.quota.codex.is_none());
        assert!(app.quota.passive.account.is_none());
        assert!(app.usage.codex_account_id.is_none());
        assert!(app.usage.codex_account.is_none());
        assert_eq!(
            *app.usage.cycle.lock().unwrap(),
            crate::usage::QuotaCycle::default()
        );
    }
}
