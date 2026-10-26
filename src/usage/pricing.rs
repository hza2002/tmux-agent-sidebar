//! Historical list prices. Append versions; never apply today's schedule to old usage.
//! Rates are micro-currency units per million tokens. Products are accumulated
//! exactly and divided by 1e6 only when formatting the aggregate.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeakSchedule {
    pub utc_offset_seconds: i64,
    /// Sunday = 0; bit N enables weekday N.
    pub weekdays: u8,
    pub hours: &'static [(u8, u8)],
}

impl PeakSchedule {
    pub fn contains(self, ts: i64) -> bool {
        let local = ts + self.utc_offset_seconds;
        let weekday = (local.div_euclid(86_400) + 4).rem_euclid(7);
        let hour = local.rem_euclid(86_400) / 3600;
        self.weekdays & (1 << weekday) != 0
            && self
                .hours
                .iter()
                .any(|&(start, end)| (i64::from(start)..i64::from(end)).contains(&hour))
    }
}

pub const DEEPSEEK_PEAK: PeakSchedule = PeakSchedule {
    utc_offset_seconds: 0,
    weekdays: 0b0111110,
    hours: &[(1, 4), (6, 10)],
};

#[derive(Debug, Clone, Copy)]
pub struct PriceVersion {
    pub model: &'static str,
    pub from: i64,
    pub until: Option<i64>,
    pub rates: Rates,
    pub peak: Option<(PeakSchedule, Rates)>,
    pub long_context: Option<(u64, Rates)>,
    /// Standard, Fast, Ultrafast factors belong to this historical version.
    pub tier_multipliers: [u64; 3],
    /// Earlier events use an explicitly assumed initial price baseline.
    pub assumed_before: Option<i64>,
}

pub const REVISION: &str = "2026-10-26.2";
// Verification time is provenance, not the price effective date.
pub const OPENAI_VERIFIED: i64 = 1_792_972_800; // 2026-10-26T00:00Z
pub const ASTRA_RELEASE: i64 = 1788393600; // 2026-09-03T00:00Z
pub const SOL_RELEASE: i64 = 1_790_640_000; // 2026-09-29T00:00Z
const fn rates(input: u64, cached: u64, output: u64) -> Rates {
    Rates {
        input,
        cached,
        output,
    }
}
pub const BOOK: &[PriceVersion] = &[
    PriceVersion {
        model: "deepseek-flash",
        from: 1_786_896_000,
        until: None,
        rates: rates(1_000_000, 20_000, 4_000_000),
        peak: Some((DEEPSEEK_PEAK, rates(2_000_000, 40_000, 8_000_000))),
        long_context: None,
        tier_multipliers: [1, 2, 6],
        assumed_before: None,
    },
    PriceVersion {
        model: "deepseek-v4-pro",
        from: 1_786_896_000,
        until: None,
        rates: rates(4_500_000, 150_000, 13_500_000),
        peak: Some((DEEPSEEK_PEAK, rates(9_000_000, 300_000, 27_000_000))),
        long_context: None,
        tier_multipliers: [1, 2, 6],
        assumed_before: None,
    },
    PriceVersion {
        model: "gpt-6-astra",
        from: ASTRA_RELEASE,
        until: None,
        rates: rates(10_000_000, 1_000_000, 50_000_000),
        peak: None,
        long_context: Some((272_000, rates(20_000_000, 2_000_000, 75_000_000))),
        tier_multipliers: [1, 2, 6],
        assumed_before: Some(OPENAI_VERIFIED),
    },
    PriceVersion {
        model: "gpt-6.1-sol",
        from: SOL_RELEASE,
        until: None,
        rates: rates(2_000_000, 100_000, 10_000_000),
        peak: None,
        long_context: Some((272_000, rates(4_000_000, 200_000, 15_000_000))),
        tier_multipliers: [1, 2, 6],
        assumed_before: None,
    },
];

pub fn version<'a>(book: &'a [PriceVersion], model: &str, ts: i64) -> Option<&'a PriceVersion> {
    let mut matches = book
        .iter()
        .filter(|v| v.model == model && ts >= v.from && v.until.is_none_or(|end| ts < end));
    let found = matches.next()?;
    matches.next().is_none().then_some(found)
}

impl PriceVersion {
    pub fn rates(self, ts: i64, input: u64) -> Rates {
        if let Some((threshold, rates)) = self.long_context
            && input > threshold
        {
            return rates;
        }
        if let Some((schedule, rates)) = self.peak
            && schedule.contains(ts)
        {
            return rates;
        }
        self.rates
    }
}

pub fn cost(rates: Rates, tokens: super::Tokens) -> u128 {
    u128::from(rates.input) * u128::from(tokens.uncached)
        + u128::from(rates.cached) * u128::from(tokens.cached)
        + u128::from(rates.output) * u128::from(tokens.output)
}

pub fn dollars(numerator: u128) -> String {
    let cents = (numerator + 5_000_000_000) / 10_000_000_000;
    format!("${}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versioned_schedule_and_rates_do_not_reprice_history() {
        let old = PriceVersion {
            model: "test",
            from: 0,
            until: Some(86400),
            rates: rates(1, 2, 3),
            peak: Some((
                PeakSchedule {
                    utc_offset_seconds: 0,
                    weekdays: 127,
                    hours: &[(1, 2)],
                },
                rates(4, 5, 6),
            )),
            long_context: None,
            tier_multipliers: [1, 2, 6],
            assumed_before: None,
        };
        let new = PriceVersion {
            from: 86400,
            until: Some(172800),
            rates: rates(7, 8, 9),
            tier_multipliers: [1, 3, 8],
            peak: Some((
                PeakSchedule {
                    utc_offset_seconds: 3600,
                    weekdays: 127,
                    hours: &[(1, 3)],
                },
                rates(10, 11, 12),
            )),
            ..old
        };
        let book = [old, new];
        assert_eq!(version(&book, "test", 3600).unwrap().tier_multipliers[1], 2);
        assert_eq!(
            version(&book, "test", 86400).unwrap().tier_multipliers[1],
            3
        );
        assert_eq!(
            version(&book, "test", 3600).unwrap().rates(3600, 0).input,
            4
        );
        assert_eq!(
            version(&book, "test", 86400).unwrap().rates(86400, 0).input,
            10
        );
        assert_eq!(
            version(&book, "test", 86400 + 7200)
                .unwrap()
                .rates(86400 + 7200, 0)
                .input,
            7
        );
        assert!(version(&book, "test", 172800).is_none());
        assert!(version(&[old, old], "test", 1).is_none());
    }
    #[test]
    fn book_intervals_never_overlap_and_money_rounds_after_sum() {
        for (i, a) in BOOK.iter().enumerate() {
            for b in &BOOK[i + 1..] {
                assert!(
                    a.model != b.model
                        || a.until.is_some_and(|t| t <= b.from)
                        || b.until.is_some_and(|t| t <= a.from)
                );
            }
        }
        assert_eq!(dollars(6_000_000_000), "$0.01");
        assert_eq!(dollars(3_000_000_000), "$0.00");
        let v = version(BOOK, "gpt-6.1-sol", SOL_RELEASE).unwrap();
        assert_eq!(v.rates(SOL_RELEASE, 272001).output, 15_000_000);
        assert!(version(BOOK, "gpt-6-astra", ASTRA_RELEASE - 1).is_none());
        let historical = version(BOOK, "gpt-6-astra", ASTRA_RELEASE).unwrap();
        assert_eq!(historical.rates.input, 10_000_000);
        assert_eq!(historical.assumed_before, Some(OPENAI_VERIFIED));
    }
}
