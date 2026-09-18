//! DeepSeek spend for the quota block's leading row (fork-only).
//!
//! Reads the agents' own session logs — Codex `sessions/**/*.jsonl` and Claude
//! Code `projects/**/*.jsonl` — keeps only DeepSeek-family usage, prices it
//! with DeepSeek's official RMB list, and reports a window total. Nothing here
//! touches tmux, the network, the render path, or a subprocess; callers run
//! [`Scanner::scan`] on a background worker exactly like the quota poller.
//!
//! Rules that are easy to get wrong, each pinned by a test:
//!
//! - Codex `input_tokens` **includes** cached input, so the uncached part is
//!   `input - cached_input_tokens`. Claude's `input_tokens` **excludes** cache;
//!   its cache hits arrive in `cache_read_input_tokens`. The sources therefore
//!   need different splits, and subtracting cache from Claude's input would
//!   underflow.
//! - Pricing follows the *event* timestamp: peak rates apply only inside the
//!   published UTC windows, and a published price change applies from its
//!   effective instant onward instead of being applied to older history.
//! - A day belongs to the local timezone, never to the file's directory or to
//!   the UTC date. A session started yesterday keeps writing into yesterday's
//!   directory after midnight.
//! - Codex repeats the same usage event in resumed or forked sessions, and
//!   Claude writes one entry per streamed content block with the same
//!   `message.id`. Both are deduplicated before they are counted.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_HOUR: i64 = 3_600;

/// Fallback cadence for the background scan. The hook trigger is what makes a
/// finished turn show up promptly; this only covers a missed trigger, and the
/// number can only change while an agent is writing, so 30s is plenty.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

// ─── Pricing ────────────────────────────────────────────────────────────────

/// RMB per **million** tokens, in the order uncached input / cached input /
/// output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub uncached: f64,
    pub cached: f64,
    pub output: f64,
}

/// Pricing family a model id belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Flash,
    Pro,
}

/// One effective pricing period. An event is priced with the last period for
/// its family whose `effective_from` is at or before the event instant, so a
/// future price change is a new entry rather than an edit.
#[derive(Debug, Clone, Copy)]
pub struct RatePeriod {
    /// Unix seconds. DeepSeek announces price changes with a start instant, so
    /// this is compared against the event instant, not against a local day.
    pub effective_from: i64,
    pub family: Family,
    pub off_peak: Rates,
    pub peak: Rates,
}

/// Official RMB price list, 元 / million tokens.
///
/// Source: <https://api-docs.deepseek.com/zh-cn/quick_start/pricing>
/// The English page quotes the same numbers divided by 6.67, so the RMB table
/// is the primary one and no exchange rate belongs anywhere near this code.
///
/// Maintenance: when DeepSeek changes a price or renames a model, append a
/// period or a mapping and add a boundary test. Never edit an existing entry —
/// events are priced by their own timestamp, so history must keep the rates
/// that applied when it happened.
const RATE_PERIODS: &[RatePeriod] = &[
    RatePeriod {
        // 2026-08-16T16:00:00Z = 2026-08-17 00:00 Beijing: the instant the
        // current V4 schedule took effect (the same cutoff ccusage encodes).
        effective_from: 1_786_896_000,
        family: Family::Flash,
        off_peak: Rates {
            uncached: 1.0,
            cached: 0.02,
            output: 4.0,
        },
        peak: Rates {
            uncached: 2.0,
            cached: 0.04,
            output: 8.0,
        },
    },
    RatePeriod {
        effective_from: 1_786_896_000,
        family: Family::Pro,
        off_peak: Rates {
            uncached: 4.5,
            cached: 0.15,
            output: 13.5,
        },
        peak: Rates {
            uncached: 9.0,
            cached: 0.30,
            output: 27.0,
        },
    },
];

/// Map a model id to its pricing family.
///
/// Only the two ids DeepSeek documents today are mapped. Retired ids
/// (`deepseek-v4-flash`, `deepseek-v4-flash-vision-exp`) are deliberately left
/// out: they are no longer selectable, and matching them by prefix would also
/// silently absorb any future id DeepSeek invents.
fn family_for(model: &str) -> Option<Family> {
    match model.trim().to_ascii_lowercase().as_str() {
        "deepseek-flash" => Some(Family::Flash),
        "deepseek-v4-pro" => Some(Family::Pro),
        _ => None,
    }
}

/// Whether a model id claims to be DeepSeek at all. Such ids are counted even
/// when [`family_for`] does not know them, so an unmapped id shows up as an
/// explicit "unpriced" marker instead of silently costing nothing.
fn is_deepseek(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("deepseek")
}

fn rates_for_family(family: Family, ts: i64) -> Option<Rates> {
    let period = RATE_PERIODS
        .iter()
        .rev()
        .find(|period| period.family == family && period.effective_from <= ts)?;
    Some(if is_peak(ts) {
        period.peak
    } else {
        period.off_peak
    })
}

/// Peak windows as published by DeepSeek: **UTC** Monday–Friday 01:00–04:00
/// and 06:00–10:00, half-open, everything else off-peak. Weekends are always
/// off-peak, which falls out of the weekday check.
pub fn is_peak(ts: i64) -> bool {
    let days_since_epoch = ts.div_euclid(SECS_PER_DAY);
    // 1970-01-01 was a Thursday; Sunday-based indexing keeps modulo valid for
    // instants before the epoch too.
    let weekday_from_sunday = (days_since_epoch + 4).rem_euclid(7);
    if !(1..=5).contains(&weekday_from_sunday) {
        return false;
    }
    let hour = ts.rem_euclid(SECS_PER_DAY) / SECS_PER_HOUR;
    (1..4).contains(&hour) || (6..10).contains(&hour)
}

// ─── Time ───────────────────────────────────────────────────────────────────

/// Parse the ISO-8601 UTC stamps both agents write (`2026-09-18T02:43:06.594Z`).
///
/// std has no date parser, and the agents only ever emit `Z`, so any other
/// offset spelling is rejected rather than guessed at.
fn parse_timestamp(raw: &str) -> Option<i64> {
    let bytes = raw.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    // Only UTC stamps are accepted. An offset spelling (`+08:00`) would parse
    // into a different instant, and neither agent writes one.
    match bytes.get(19) {
        Some(b'Z') if bytes.len() == 20 => {}
        Some(b'.') => {
            let tail = &bytes[20..];
            if tail.len() < 2
                || !tail[..tail.len() - 1].iter().all(u8::is_ascii_digit)
                || tail[tail.len() - 1] != b'Z'
            {
                return None;
            }
        }
        _ => return None,
    }
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    let month: i64 = raw.get(5..7)?.parse().ok()?;
    let day: i64 = raw.get(8..10)?.parse().ok()?;
    let hour: i64 = raw.get(11..13)?.parse().ok()?;
    let minute: i64 = raw.get(14..16)?.parse().ok()?;
    let second: i64 = raw.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    // Fractional seconds cannot move an event across a day or a peak window.
    let days = days_from_civil(year, month, day);
    Some(days * SECS_PER_DAY + hour * SECS_PER_HOUR + minute * 60 + second)
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 for a proleptic
/// Gregorian date. Keeps the date math in one tested place.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Unix seconds of the local midnight that starts `ts`'s local day.
///
/// This is the only place the process timezone enters the scan. A failure to
/// resolve the local time falls back to a UTC day rather than dropping the
/// event.
fn local_day_start(ts: i64) -> i64 {
    unsafe {
        let instant = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&instant, &mut tm).is_null() {
            return ts.div_euclid(SECS_PER_DAY) * SECS_PER_DAY;
        }
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        let midnight = libc::mktime(&mut tm);
        if midnight == -1 {
            return ts.div_euclid(SECS_PER_DAY) * SECS_PER_DAY;
        }
        midnight as i64
    }
}

// ─── Totals ─────────────────────────────────────────────────────────────────

/// The three billed token classes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub uncached: u64,
    pub cached: u64,
    pub output: u64,
}

impl Tokens {
    pub fn total(self) -> u64 {
        self.uncached + self.cached + self.output
    }

    fn add(&mut self, other: Tokens) {
        self.uncached += other.uncached;
        self.cached += other.cached;
        self.output += other.output;
    }
}

/// Aggregated DeepSeek usage for one window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spend {
    pub tokens: Tokens,
    pub cost_cny: f64,
    /// Set when a DeepSeek-family model had no price entry. Its tokens are
    /// counted; its cost is not guessed.
    pub unpriced: bool,
}

impl Spend {
    pub fn is_empty(&self) -> bool {
        self.tokens.total() == 0
    }
}

/// Window the row can show; clicking the row cycles through these.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Window {
    #[default]
    Today,
    SevenDays,
    ThirtyDays,
}

impl Window {
    pub fn days(self) -> i64 {
        match self {
            Window::Today => 1,
            Window::SevenDays => 7,
            Window::ThirtyDays => 30,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Window::Today => Window::SevenDays,
            Window::SevenDays => Window::ThirtyDays,
            Window::ThirtyDays => Window::Today,
        }
    }

    /// Row label. The range windows carry their length because the row has no
    /// other field that would say which window it shows.
    pub fn name(self) -> &'static str {
        match self {
            Window::Today => "ds",
            Window::SevenDays => "ds7",
            Window::ThirtyDays => "ds30",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Window::Today => 0,
            Window::SevenDays => 1,
            Window::ThirtyDays => 2,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Window::SevenDays,
            2 => Window::ThirtyDays,
            _ => Window::Today,
        }
    }
}

/// `¥1.91`, `¥12.34`, `¥1,234.56`.
pub fn format_cny(cost: f64) -> String {
    let cents = (cost * 100.0).round().max(0.0) as i64;
    let units = cents / 100;
    let cents = cents % 100;
    let mut digits = units.to_string();
    if digits.len() > 3 {
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (index, ch) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        digits = grouped;
    }
    format!("¥{digits}.{cents:02}")
}

/// `950`, `12.3K`, `14.1M`, `1.2B`.
pub fn format_tokens(total: u64) -> String {
    for (scale, suffix) in [(1_000_000_000u64, 'B'), (1_000_000, 'M'), (1_000, 'K')] {
        if total >= scale {
            return format!("{:.1}{suffix}", total as f64 / scale as f64);
        }
    }
    total.to_string()
}

// ─── Records ────────────────────────────────────────────────────────────────

/// Identity of one usage event, used to drop duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RecordId {
    /// Codex repeats a whole usage event when a session is resumed or forked
    /// into another rollout file, and the copy carries the same numbers.
    Codex {
        ts: i64,
        uncached: u64,
        cached: u64,
        output: u64,
    },
    /// Claude Code writes the same `message.id` once per streamed content
    /// block; the entry with the largest token total is the real one.
    Claude { message_id: Box<str> },
}

/// One priced-or-countable usage event. Kept per file so an unchanged file can
/// be reused from cache without re-parsing.
#[derive(Debug, Clone, PartialEq)]
struct Record {
    ts: i64,
    /// `None` means "DeepSeek model without a price entry".
    family: Option<Family>,
    tokens: Tokens,
    id: RecordId,
}

#[derive(Debug, Clone)]
struct CachedFile {
    mtime: i64,
    size: u64,
    records: Vec<Record>,
}

// ─── Scan ───────────────────────────────────────────────────────────────────

/// Which agent's log tree a root belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Codex,
    Claude,
}

/// Log roots the scanner reads. Codex honours `$CODEX_HOME`; Claude mirrors
/// the `~/.claude` path the rest of the fork already uses.
pub fn roots() -> Vec<(Source, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let codex = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    vec![
        (Source::Codex, codex.join("sessions")),
        (Source::Claude, home.join(".claude").join("projects")),
    ]
}

/// Per-file parse cache plus the scan entry point.
///
/// Owned by one worker thread: it is deliberately not `Sync`, which is what
/// removes every lock from this feature.
#[derive(Debug, Default)]
pub struct Scanner {
    files: HashMap<PathBuf, CachedFile>,
}

impl Scanner {
    /// Aggregate `window` ending at `now`. Returns `None` when `cancel` said
    /// the request was superseded mid-scan; the caller then serves the newest
    /// window. Work already parsed stays cached either way.
    pub fn scan(&mut self, window: Window, now: i64, cancel: &dyn Fn() -> bool) -> Option<Spend> {
        self.scan_roots(&roots(), window, now, cancel)
    }

    fn scan_roots(
        &mut self,
        roots: &[(Source, PathBuf)],
        window: Window,
        now: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Option<Spend> {
        let start = local_day_start(now - (window.days() - 1) * SECS_PER_DAY);
        let mut live: HashSet<PathBuf> = HashSet::new();
        for (source, root) in roots {
            if !root.is_dir() {
                continue;
            }
            for path in jsonl_files_since(root, start) {
                if cancel() {
                    return None;
                }
                live.insert(path.clone());
                self.refresh_file(*source, &path, start);
            }
        }
        // A file that left the window (or disappeared) must not keep feeding
        // the totals, or a 7-day view would slowly turn into an all-time view.
        self.files.retain(|path, _| live.contains(path));
        let records = self.files.values().flat_map(|file| file.records.iter());
        Some(aggregate(records, start))
    }

    fn refresh_file(&mut self, source: Source, path: &Path, start: i64) {
        let Ok(metadata) = std::fs::metadata(path) else {
            self.files.remove(path);
            return;
        };
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or(0);
        let size = metadata.len();
        if let Some(cached) = self.files.get(path)
            && cached.mtime == mtime
            && cached.size == size
        {
            return;
        }
        // The fingerprint is captured before the read. If the agent appends
        // while we parse, the next tick sees a different stamp and re-reads the
        // file instead of caching a truncated result.
        let mut records = Vec::new();
        match source {
            Source::Codex => parse_codex_file(path, start, &mut records),
            Source::Claude => parse_claude_file(path, start, &mut records),
        }
        self.files.insert(
            path.to_path_buf(),
            CachedFile {
                mtime,
                size,
                records,
            },
        );
    }
}

/// Every `*.jsonl` under `root` whose mtime is at or after `start`.
///
/// A file that received an in-window event was necessarily written at or after
/// the window start, so mtime is a safe pre-filter: it can only skip files
/// that cannot contribute. It also means a long-lived session started days ago
/// is still found, and that neither tree needs a date directory layout.
fn jsonl_files_since(root: &Path, start: i64) -> Vec<PathBuf> {
    fn walk(dir: &Path, start: i64, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                walk(&path, start, depth + 1, out);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_secs() as i64)
                .unwrap_or(0);
            if modified >= start {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, start, 0, &mut out);
    out.sort();
    out
}

/// Codex: `turn_context` carries the model, `token_count` carries the delta.
fn parse_codex_file(path: &Path, start: i64, out: &mut Vec<Record>) {
    let Ok(file) = File::open(path) else { return };
    let mut model: Option<String> = None;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        // Cheap filter first: these logs reach gigabytes and most lines are
        // tool output that cannot contribute.
        if !line.contains("\"turn_context\"") && !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("turn_context") => {
                if let Some(name) = value.pointer("/payload/model").and_then(Value::as_str) {
                    model = Some(name.to_string());
                }
            }
            Some("event_msg") => {
                if value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count") {
                    continue;
                }
                // A token_count before the first turn_context has no model to
                // attribute it to; skipping beats guessing.
                let Some(model) = model.as_deref() else {
                    continue;
                };
                if !is_deepseek(model) {
                    continue;
                }
                let Some(ts) = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
                else {
                    continue;
                };
                if ts < start {
                    continue;
                }
                let Some(usage) = value.pointer("/payload/info/last_token_usage") else {
                    continue;
                };
                let input = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cached = usage
                    .get("cached_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let tokens = Tokens {
                    // `input_tokens` includes the cached part.
                    uncached: input.saturating_sub(cached),
                    cached,
                    output,
                };
                if tokens.total() == 0 {
                    continue;
                }
                out.push(Record {
                    ts,
                    family: family_for(model),
                    tokens,
                    id: RecordId::Codex {
                        ts,
                        uncached: tokens.uncached,
                        cached: tokens.cached,
                        output: tokens.output,
                    },
                });
            }
            _ => {}
        }
    }
}

/// Claude Code: assistant messages carry `message.model` and `message.usage`.
fn parse_claude_file(path: &Path, start: i64, out: &mut Vec<Record>) {
    let Ok(file) = File::open(path) else { return };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(model) = value.pointer("/message/model").and_then(Value::as_str) else {
            continue;
        };
        if !is_deepseek(model) {
            continue;
        }
        let Some(ts) = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp)
        else {
            continue;
        };
        if ts < start {
            continue;
        }
        let Some(usage) = value.pointer("/message/usage") else {
            continue;
        };
        let input = usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cached = usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // DeepSeek publishes no separate cache-write price and its Anthropic
        // endpoint has reported zero so far; bill any future writes as
        // uncached input rather than dropping them.
        let cache_write = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let tokens = Tokens {
            // Anthropic-style usage excludes cache from `input_tokens`.
            uncached: input + cache_write,
            cached,
            output,
        };
        if tokens.total() == 0 {
            continue;
        }
        let message_id = value
            .pointer("/message/id")
            .and_then(Value::as_str)
            .map(Box::from)
            .unwrap_or_else(|| {
                // No id: fall back to the payload itself so the duplicate
                // collapse below still works.
                format!(
                    "{ts}:{}:{}:{}",
                    tokens.uncached, tokens.cached, tokens.output
                )
                .into()
            });
        out.push(Record {
            ts,
            family: family_for(model),
            tokens,
            id: RecordId::Claude { message_id },
        });
    }
}

/// Fold records into one window total, dropping duplicates.
fn aggregate<'a>(records: impl Iterator<Item = &'a Record>, start: i64) -> Spend {
    let mut spend = Spend::default();
    let mut codex_seen: HashSet<&RecordId> = HashSet::new();
    let mut claude_best: HashMap<&str, &Record> = HashMap::new();
    for record in records {
        if record.ts < start {
            continue;
        }
        match &record.id {
            RecordId::Codex { .. } => {
                if !codex_seen.insert(&record.id) {
                    continue;
                }
            }
            RecordId::Claude { message_id } => match claude_best.get(message_id.as_ref()) {
                Some(previous) if previous.tokens.total() >= record.tokens.total() => continue,
                _ => {
                    claude_best.insert(message_id, record);
                    continue;
                }
            },
        }
        add_record(&mut spend, record);
    }
    for record in claude_best.values() {
        add_record(&mut spend, record);
    }
    spend
}

fn add_record(spend: &mut Spend, record: &Record) {
    spend.tokens.add(record.tokens);
    match record
        .family
        .and_then(|family| rates_for_family(family, record.ts))
    {
        Some(rates) => {
            let million = 1_000_000.0;
            spend.cost_cny += (record.tokens.uncached as f64 * rates.uncached
                + record.tokens.cached as f64 * rates.cached
                + record.tokens.output as f64 * rates.output)
                / million;
        }
        None => spend.unpriced = true,
    }
}

// ─── Worker ─────────────────────────────────────────────────────────────────

/// One scan result, tagged with the window it belongs to. The renderer only
/// paints a result whose window is the one currently selected, so a result
/// that arrives after the user cycled away can never be shown under the wrong
/// label.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub window: Window,
    pub spend: Spend,
}

/// Local UI state for the spend row plus the mailbox shared with the scanner
/// thread. Mirrors [`crate::quota::QuotaState`]: nothing here is written to
/// tmux, and no credential or prompt text passes through it.
#[derive(Debug)]
pub struct UsageState {
    /// Window the click currently selects.
    pub selected: Window,
    /// Last result whose window matches `selected`.
    pub snapshot: Option<Spend>,
    /// Whether the scanner has reported at least once. The row stays hidden
    /// until then so the shared snapshot fixtures keep their current shape.
    pub received: bool,
    /// Newest agent status-change stamp already turned into a scan request.
    pub last_status_stamp: u64,
    /// Single-value window mailbox: the click overwrites it and the scanner
    /// always reads the newest value.
    pub window: std::sync::Arc<AtomicU8>,
    /// Re-check-now request, set by the hook trigger and by clicks.
    pub force: std::sync::Arc<AtomicBool>,
}

impl Default for UsageState {
    fn default() -> Self {
        Self {
            selected: Window::Today,
            snapshot: None,
            received: false,
            last_status_stamp: 0,
            window: std::sync::Arc::new(AtomicU8::new(Window::Today.code())),
            force: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }
}

impl UsageState {
    /// Apply a scanner result. A result for a window the user already cycled
    /// away from is dropped: the scanner re-scans the newest request and sends
    /// another update, so the row never shows one window's number under
    /// another window's label.
    pub fn apply(&mut self, update: Update) {
        self.received = true;
        if update.window == self.selected {
            self.snapshot = Some(update.spend);
        }
    }

    /// Advance to the next window and ask the scanner to serve it.
    pub fn cycle(&mut self) {
        self.selected = self.selected.next();
        // The previous window's number must not wear the new label while the
        // scanner is still working on it.
        self.snapshot = None;
        self.window.store(self.selected.code(), Ordering::Relaxed);
        self.force.store(true, Ordering::Relaxed);
    }
}

/// Background scan loop, modeled on `quota_poll_loop`.
///
/// `window` is a single-value mailbox: a click overwrites it, and the loop
/// always reads the newest value, so rapid clicking can never queue work. The
/// scanner's per-file cache means an abandoned scan still contributes.
pub fn poll_loop(tx: &Sender<Update>, window: &AtomicU8, force: &AtomicBool) {
    let mut scanner = Scanner::default();
    loop {
        force.swap(false, Ordering::Relaxed);
        let code = window.load(Ordering::Relaxed);
        let requested = Window::from_code(code);
        let now = crate::time::now_epoch_secs() as i64;
        let cancel = || window.load(Ordering::Relaxed) != code;
        let Some(spend) = scanner.scan(requested, now, &cancel) else {
            // Superseded mid-scan: serve the newest window immediately. The
            // parsed files stay cached, so this is not wasted work.
            continue;
        };
        if tx
            .send(Update {
                window: requested,
                spend,
            })
            .is_err()
        {
            return;
        }
        sleep_until_due(REFRESH_INTERVAL, force);
    }
}

/// Sleep in one-second slices so the hook trigger takes effect within a
/// second, exactly like the quota poller.
fn sleep_until_due(interval: Duration, force: &AtomicBool) {
    let deadline = SystemTime::now() + interval;
    loop {
        if force.load(Ordering::Relaxed) {
            return;
        }
        let remaining = deadline
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return;
        }
        std::thread::sleep(remaining.min(Duration::from_secs(1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 2026-09-18 is a Friday. These are the published window boundaries in
    /// UTC, which is also what the sidebar's Asia/Taipei users hit at
    /// 09:00/12:00/14:00/18:00 local time.
    const FRI_0059: i64 = 1_789_693_199;
    const FRI_0100: i64 = 1_789_693_200;
    const FRI_0359: i64 = 1_789_703_999;
    const FRI_0400: i64 = 1_789_704_000;
    const FRI_0559: i64 = 1_789_711_199;
    const FRI_0600: i64 = 1_789_711_200;
    const FRI_0959: i64 = 1_789_725_599;
    const FRI_1000: i64 = 1_789_725_600;
    /// Saturday 03:00Z: inside the weekday window's hours, but off-peak.
    const SAT_0300: i64 = 1_789_786_800;
    const CUTOFF: i64 = 1_786_896_000;

    fn tokens(uncached: u64, cached: u64, output: u64) -> Tokens {
        Tokens {
            uncached,
            cached,
            output,
        }
    }

    fn codex_record(ts: i64, model: &str, tokens: Tokens) -> Record {
        Record {
            ts,
            family: family_for(model),
            tokens,
            id: RecordId::Codex {
                ts,
                uncached: tokens.uncached,
                cached: tokens.cached,
                output: tokens.output,
            },
        }
    }

    fn claude_record(ts: i64, model: &str, message_id: &str, tokens: Tokens) -> Record {
        Record {
            ts,
            family: family_for(model),
            tokens,
            id: RecordId::Claude {
                message_id: message_id.into(),
            },
        }
    }

    #[test]
    fn peak_windows_are_utc_weekday_and_half_open() {
        assert!(!is_peak(FRI_0059), "01:00Z is the first peak instant");
        assert!(is_peak(FRI_0100));
        assert!(is_peak(FRI_0359));
        assert!(!is_peak(FRI_0400), "04:00Z ends the first window");
        assert!(!is_peak(FRI_0559));
        assert!(is_peak(FRI_0600));
        assert!(is_peak(FRI_0959));
        assert!(!is_peak(FRI_1000));
        assert!(!is_peak(SAT_0300), "weekends stay off-peak all day");
    }

    #[test]
    fn peak_is_the_same_for_every_weekday_and_never_on_weekends() {
        // Saturday and Sunday at 01:00Z are off-peak; Monday is peak.
        let day = SECS_PER_DAY;
        assert!(!is_peak(FRI_0100 + day));
        assert!(!is_peak(FRI_0100 + 2 * day));
        assert!(is_peak(FRI_0100 + 3 * day));
    }

    #[test]
    fn timestamps_parse_without_a_date_library() {
        // 02:43:06Z is 6186 seconds after the 01:00Z peak boundary.
        assert_eq!(
            parse_timestamp("2026-09-18T02:43:06.594Z"),
            Some(FRI_0100 + 6186)
        );
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2026-09-18T02:43:06+08:00"), None);
        assert_eq!(parse_timestamp("not-a-timestamp"), None);
        assert_eq!(parse_timestamp("2026-13-18T02:43:06Z"), None);
    }

    #[test]
    fn local_day_start_is_the_start_of_the_local_day() {
        // Timezone independent properties: the result is a fixed point and
        // contains the instant it came from.
        for ts in [FRI_0100, SAT_0300, CUTOFF] {
            let start = local_day_start(ts);
            assert_eq!(local_day_start(start), start);
            assert!(start <= ts && ts < start + SECS_PER_DAY);
            assert_eq!(start % 60, 0, "midnight lands on a minute boundary");
        }
    }

    #[test]
    fn price_periods_apply_from_their_effective_instant() {
        let before = rates_for_family(Family::Flash, CUTOFF - 1);
        assert!(
            before.is_none(),
            "no period covers instants before the table"
        );
        let peak = rates_for_family(Family::Flash, FRI_0100).unwrap();
        assert_eq!(peak, RATE_PERIODS[0].peak);
        let off_peak = rates_for_family(Family::Pro, FRI_1000).unwrap();
        assert_eq!(off_peak, RATE_PERIODS[1].off_peak);
        assert_eq!(off_peak.uncached, 4.5);
    }

    #[test]
    fn only_the_two_current_model_ids_are_priced() {
        assert_eq!(family_for("deepseek-flash"), Some(Family::Flash));
        assert_eq!(family_for("DeepSeek-V4-Pro"), Some(Family::Pro));
        assert_eq!(family_for("deepseek-v4-flash"), None);
        assert_eq!(family_for("gpt-6-astra"), None);
        assert!(is_deepseek("deepseek-something-new"));
        assert!(!is_deepseek("gpt-6-astra"));
    }

    #[test]
    fn codex_input_includes_cached_input() {
        // 1000 in / 800 cached must bill 200 uncached, not 1000.
        let record = codex_record(FRI_0600, "deepseek-flash", tokens(200, 800, 0));
        let spend = aggregate([&record].into_iter(), 0);
        assert_eq!(spend.tokens, tokens(200, 800, 0));
        // Peak: 200 * 2 + 800 * 0.04 = 432 / 1e6 = ¥0.000432.
        assert!((spend.cost_cny - 0.000432).abs() < 1e-9);
    }

    #[test]
    fn claude_input_excludes_cache_and_bills_both_halves() {
        // Anthropic-style: 575 uncached, 22912 cache hits.
        let record = claude_record(
            FRI_1000,
            "deepseek-v4-pro",
            "m1",
            tokens(575, 22_912, 5_927),
        );
        let spend = aggregate([&record].into_iter(), 0);
        assert_eq!(spend.tokens, tokens(575, 22_912, 5_927));
        // Off-peak Pro: 575 * 4.5 + 22912 * 0.15 + 5927 * 13.5 = 86038.8 / 1e6.
        assert!((spend.cost_cny - 0.0860388).abs() < 1e-9);
        assert!(!spend.unpriced);
    }

    #[test]
    fn mixed_peak_and_off_peak_events_are_priced_separately() {
        let peak = codex_record(FRI_0100, "deepseek-flash", tokens(1_000_000, 0, 0));
        let off_peak = codex_record(FRI_1000, "deepseek-flash", tokens(1_000_000, 0, 0));
        let spend = aggregate([&peak, &off_peak].into_iter(), 0);
        assert_eq!(spend.tokens.uncached, 2_000_000);
        // ¥2.00 peak + ¥1.00 off-peak.
        assert!((spend.cost_cny - 3.0).abs() < 1e-9);
    }

    #[test]
    fn resumed_codex_sessions_do_not_double_count() {
        let copy_a = codex_record(FRI_0600, "deepseek-flash", tokens(100, 10, 50));
        let copy_b = copy_a.clone();
        let other = codex_record(FRI_0600 + 1, "deepseek-flash", tokens(200, 20, 75));
        let spend = aggregate([&copy_a, &copy_b, &other].into_iter(), 0);
        assert_eq!(spend.tokens, tokens(300, 30, 125));
    }

    #[test]
    fn claude_streaming_duplicates_keep_the_largest_entry() {
        let partial = claude_record(FRI_0600, "deepseek-flash", "m1", tokens(10, 0, 0));
        let full = claude_record(FRI_0600, "deepseek-flash", "m1", tokens(100, 5, 50));
        let later = claude_record(FRI_0600 + 1, "deepseek-flash", "m2", tokens(7, 0, 1));
        let spend = aggregate([&partial, &full, &later].into_iter(), 0);
        assert_eq!(spend.tokens, tokens(107, 5, 51));
    }

    #[test]
    fn unknown_deepseek_ids_count_tokens_and_flag_unpriced() {
        let record = codex_record(FRI_0600, "deepseek-v4-flash", tokens(1_000_000, 0, 0));
        let spend = aggregate([&record].into_iter(), 0);
        assert_eq!(spend.tokens.uncached, 1_000_000);
        assert!(spend.unpriced);
        assert_eq!(spend.cost_cny, 0.0);
    }

    #[test]
    fn window_start_filters_older_events() {
        let yesterday = codex_record(FRI_0600 - SECS_PER_DAY, "deepseek-flash", tokens(1, 0, 0));
        let today = codex_record(FRI_0600, "deepseek-flash", tokens(2, 0, 0));
        let spend = aggregate([&yesterday, &today].into_iter(), FRI_0600);
        assert_eq!(spend.tokens.uncached, 2);
    }

    #[test]
    fn cny_formatting_uses_two_decimals_and_thousands() {
        assert_eq!(format_cny(0.0), "¥0.00");
        assert_eq!(format_cny(1.905), "¥1.91");
        assert_eq!(format_cny(9.999), "¥10.00");
        assert_eq!(format_cny(1234.5), "¥1,234.50");
        assert_eq!(format_cny(1_234_567.891), "¥1,234,567.89");
    }

    #[test]
    fn token_formatting_keeps_rows_within_budget() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(12_345), "12.3K");
        assert_eq!(format_tokens(14_100_000), "14.1M");
        assert_eq!(format_tokens(1_200_000_000), "1.2B");
    }

    #[test]
    fn windows_cycle_through_the_three_views() {
        assert_eq!(Window::default(), Window::Today);
        assert_eq!(Window::Today.next(), Window::SevenDays);
        assert_eq!(Window::SevenDays.next(), Window::ThirtyDays);
        assert_eq!(Window::ThirtyDays.next(), Window::Today);
        assert_eq!(
            Window::from_code(Window::ThirtyDays.code()),
            Window::ThirtyDays
        );
        assert_eq!(Window::from_code(9), Window::Today);
        assert_eq!(Window::Today.name(), "ds");
        assert_eq!(Window::SevenDays.name(), "ds7");
    }

    // ─── File level ─────────────────────────────────────────────────────────

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    fn codex_fixture_lines(ts: &str, model: &str) -> String {
        format!(
            "{}\n{}\n",
            format_args!(
                r#"{{"timestamp":"{ts}","type":"turn_context","payload":{{"model":"{model}"}}}}"#
            ),
            format_args!(
                r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":100}}}}}}}}"#
            ),
        )
    }

    #[test]
    fn scanner_reads_codex_fixture_and_reports_the_three_token_classes() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "2026/09/18/rollout.jsonl",
            &codex_fixture_lines("2026-09-18T02:43:06.594Z", "deepseek-flash"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        let spend = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert_eq!(spend.tokens, tokens(200, 800, 100));
        // Peak (02:43Z) Flash: 200*2 + 800*0.04 + 100*8 = 1232 / 1e6.
        assert!((spend.cost_cny - 0.001232).abs() < 1e-9);
        assert!(!spend.unpriced);
    }

    #[test]
    fn scanner_skips_non_deepseek_and_events_without_a_model() {
        let dir = tempfile::tempdir().unwrap();
        let orphan = r#"{"timestamp":"2026-09-18T02:43:06.594Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":100}}}}"#;
        write(
            dir.path(),
            "a.jsonl",
            &format!(
                "{}\n{}\n",
                orphan,
                codex_fixture_lines("2026-09-18T02:43:06.594Z", "gpt-6-astra")
            ),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        let spend = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert!(spend.is_empty(), "no deepseek usage may be counted");
    }

    #[test]
    fn scanner_reads_claude_fixture_and_dedupes_message_ids() {
        let dir = tempfile::tempdir().unwrap();
        let line = r#"{"timestamp":"2026-09-18T02:43:06.594Z","type":"assistant","message":{"id":"msg-1","model":"deepseek-v4-pro","usage":{"input_tokens":575,"cache_creation_input_tokens":0,"cache_read_input_tokens":22912,"output_tokens":5927}}}"#;
        write(
            dir.path(),
            "proj/session.jsonl",
            &format!("{line}\n{line}\n"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Claude, dir.path().to_path_buf())];
        let spend = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert_eq!(spend.tokens, tokens(575, 22_912, 5_927));
    }

    #[test]
    fn scanner_reuses_cached_files_and_drops_stale_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "a.jsonl",
            &codex_fixture_lines("2026-09-18T02:43:06.594Z", "deepseek-flash"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        let first = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert_eq!(first.tokens, tokens(200, 800, 100));
        // Same content and mtime: the cached records are reused, and a scan
        // with no files at all prunes the entry instead of keeping it forever.
        let second = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert_eq!(second, first);
        fs::remove_file(&path).unwrap();
        let third = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert!(third.is_empty());
        assert!(scanner.files.is_empty());
    }

    #[test]
    fn scan_reports_cancellation_without_losing_cached_work() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "a.jsonl",
            &codex_fixture_lines("2026-09-18T02:43:06.594Z", "deepseek-flash"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        assert!(
            scanner
                .scan_roots(&roots, Window::Today, FRI_0600, &|| true)
                .is_none()
        );
        assert!(scanner.files.is_empty());
    }
}
