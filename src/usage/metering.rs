//! Query views over the scanner's canonical records; no file I/O in the UI.
use super::{Scanner, Tokens, pricing};
use std::collections::{BTreeMap, HashSet};

pub const WEEK: u64 = 604_800;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EventMeta {
    pub model: String,
    pub tier: Option<String>,
    pub account: Option<String>,
    pub cache_write: u64,
    pub uncertain_identity: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Recent {
    Day,
    #[default]
    SevenDays,
    ThirtyDays,
}
impl Recent {
    pub fn next(self) -> Self {
        match self {
            Self::Day => Self::SevenDays,
            Self::SevenDays => Self::ThirtyDays,
            Self::ThirtyDays => Self::Day,
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Day => 0,
            Self::SevenDays => 1,
            Self::ThirtyDays => 2,
        }
    }
    pub fn seconds(self) -> i64 {
        match self {
            Self::Day => 86400,
            Self::SevenDays => 604800,
            Self::ThirtyDays => 2592000,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "24h",
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Total {
    pub tokens: Tokens,
    pub cost: u128,
    pub unpriced: u64,
    pub unknown_tier: u64,
    pub assumed_standard: u64,
    pub missing_price: u64,
    pub unsupported_cache_write: u64,
    pub assumed_price: u64,
    pub unknown_account: bool,
    pub uncertain_identity: bool,
}
impl Total {
    pub fn amount(&self) -> String {
        if self.unpriced > 0 && self.cost == 0 {
            return "$--?".into();
        }
        format!(
            "{}{}",
            pricing::dollars(self.cost),
            if self.unpriced > 0 { "?" } else { "" }
        )
    }
    fn add(&mut self, meta: &EventMeta, ts: i64, tokens: Tokens) {
        self.tokens.add(tokens);
        self.unknown_account |= meta.account.is_none();
        self.uncertain_identity |= meta.uncertain_identity;
        // Missing tier uses the disclosed Standard estimation baseline. Explicit
        // unsupported tiers remain unpriced; they are not a missing setting.
        let tier = match meta.tier.as_deref() {
            None | Some("default" | "standard") => Some(0),
            Some("priority" | "fast") => Some(1),
            Some("ultrafast") => Some(2),
            _ => None,
        };
        if tier.is_none() {
            self.unknown_tier += 1;
        }
        let version = pricing::version(pricing::BOOK, &meta.model, ts);
        self.missing_price += u64::from(version.is_none());
        self.unsupported_cache_write += u64::from(meta.cache_write > 0);
        match (version, tier) {
            (Some(v), Some(tier)) if meta.cache_write == 0 => {
                self.assumed_standard += u64::from(meta.tier.is_none());
                self.assumed_price += u64::from(v.assumed_before.is_some_and(|end| ts < end));
                self.cost += pricing::cost(v.rates(ts, tokens.uncached + tokens.cached), tokens)
                    * u128::from(v.tier_multipliers[tier]);
            }
            _ => self.unpriced += 1,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub start: i64,
    pub end: i64,
    pub total: Total,
    pub models: BTreeMap<String, Total>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub cycle_end: u64,
    pub cycle: Option<Summary>,
    pub recent: [Summary; 3],
    pub incomplete: bool,
    pub observed_at: i64,
}

pub(super) fn summarize(scanner: &Scanner, now: i64, cycle_end: u64) -> Snapshot {
    let mut result = Snapshot {
        cycle_end,
        observed_at: now,
        incomplete: scanner.incomplete || scanner.files.values().any(|f| f.incomplete),
        ..Snapshot::default()
    };
    if cycle_end > now as u64 && cycle_end.saturating_sub(WEEK) <= now as u64 && cycle_end >= WEEK {
        result.cycle = Some(Summary {
            start: (cycle_end - WEEK) as i64,
            end: now,
            ..Summary::default()
        });
    }
    for w in [Recent::Day, Recent::SevenDays, Recent::ThirtyDays] {
        result.recent[w.index()] = Summary {
            start: now - w.seconds(),
            end: now,
            ..Summary::default()
        };
    }
    let mut seen = HashSet::new();
    // Stable traversal makes conflicting duplicate evidence deterministic.
    let mut records: Vec<_> = scanner
        .files
        .values()
        .flat_map(|f| &f.records)
        .filter(|r| r.codex.is_some())
        .collect();
    records.sort_by(|a, b| (a.ts, &a.codex).cmp(&(b.ts, &b.codex)));
    for r in records {
        if !seen.insert(&r.id) {
            continue;
        }
        let meta = r.codex.as_ref().unwrap();
        for summary in result.cycle.iter_mut().chain(result.recent.iter_mut()) {
            if r.ts >= summary.start && r.ts < summary.end {
                summary.total.add(meta, r.ts, r.tokens);
                summary
                    .models
                    .entry(meta.model.clone())
                    .or_default()
                    .add(meta, r.ts, r.tokens);
            }
        }
    }
    result
}

pub fn format_time(ts: i64) -> String {
    unsafe {
        let instant = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::gmtime_r(&instant, &mut tm).is_null() {
            return ts.to_string();
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_windows_are_elapsed_and_expired_cycles_are_unavailable() {
        let now = 1_800_000_000;
        let s = summarize(&Scanner::default(), now, now as u64 + 100);
        assert_eq!(s.recent[0].start, now - 86400);
        assert_eq!(s.recent[1].start, now - 604800);
        assert_eq!(s.recent[2].start, now - 2592000);
        assert_eq!(s.cycle.unwrap().start, now + 100 - 604800);
        assert!(
            summarize(&Scanner::default(), now, now as u64)
                .cycle
                .is_none()
        );
        assert!(summarize(&Scanner::default(), now, 0).cycle.is_none());
    }
    #[test]
    fn missing_tier_uses_disclosed_standard_but_explicit_tiers_are_respected() {
        let mut total = Total::default();
        let mut meta = EventMeta {
            model: "gpt-6.1-sol".into(),
            tier: None,
            account: None,
            cache_write: 0,
            uncertain_identity: false,
        };
        let tokens = Tokens {
            uncached: 1_000_000,
            cached: 0,
            output: 0,
        };
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        assert_eq!(total.amount(), "$4.00");
        assert_eq!(total.assumed_standard, 1);
        assert_eq!(total.unpriced, 0);
        meta.tier = Some("standard".into());
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        // Long-context rate applies to the entire request.
        assert_eq!(total.amount(), "$8.00");
        meta.tier = Some("fast".into());
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        assert_eq!(total.amount(), "$16.00");
        assert_eq!(total.assumed_standard, 1);
        meta.tier = Some("unrecognized".into());
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        assert_eq!(total.amount(), "$16.00?");
        assert_eq!(total.unknown_tier, 1);
        assert_eq!(total.unpriced, 1);
    }
    #[test]
    fn initial_astra_baseline_prices_history_and_discloses_assumption() {
        let mut total = Total::default();
        let mut meta = EventMeta {
            model: "gpt-6-astra".into(),
            tier: Some("default".into()),
            account: None,
            cache_write: 0,
            uncertain_identity: false,
        };
        let tokens = Tokens {
            uncached: 100_000,
            cached: 100_000,
            output: 10_000,
        };
        total.add(&meta, pricing::ASTRA_RELEASE, tokens);
        assert_eq!(total.amount(), "$1.60");
        assert_eq!(total.assumed_price, 1);
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        assert_eq!(total.assumed_price, 1);
        assert_eq!(total.unpriced, 0);
        total.add(&meta, pricing::ASTRA_RELEASE - 1, tokens);
        assert_eq!(total.missing_price, 1);
        meta.cache_write = 10;
        total.add(&meta, pricing::OPENAI_VERIFIED, tokens);
        assert_eq!(total.unsupported_cache_write, 1);
        assert_eq!(total.unpriced, 2);
        assert_eq!(total.amount(), "$3.20?");
    }
}
