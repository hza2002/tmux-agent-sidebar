//! Local usage for DeepSeek and Codex (fork-only).
//!
//! Reads the agents' own session logs — Codex `sessions/**/*.jsonl` and Claude
//! Code `projects/**/*.jsonl`, normalizes billable usage, and reports calendar,
//! rolling, and quota-cycle totals with historical list prices. Nothing here
//! makes model-service requests or performs I/O on the render path. The
//! background worker reads the local daemon's account over its control socket;
//! callers run [`poll_loop`] exactly like the quota poller.
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

pub(crate) mod attribution;

#[cfg(test)]
use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::fs::File;
#[cfg(test)]
use std::io::{Seek, SeekFrom};
#[cfg(test)]
use std::os::unix::fs::MetadataExt;

pub mod history;
pub mod metering;
pub mod pricing;
#[cfg(test)]
mod source;
#[cfg(test)]
use std::io::{BufRead, BufReader};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
#[cfg(test)]
use std::time::UNIX_EPOCH;
use std::time::{Duration, SystemTime};

#[cfg(test)]
use serde_json::Value;

const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_HOUR: i64 = 3_600;

/// Fallback cadence for the background scan. The hook trigger is what makes a
/// finished turn show up promptly; this only covers a missed trigger, and the
/// number can only change while an agent is writing, so 30s is plenty.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

// ─── Pricing ────────────────────────────────────────────────────────────────

/// Pricing family a model id belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Family {
    Flash,
    Pro,
}

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

pub fn is_peak(ts: i64) -> bool {
    pricing::version(pricing::BOOK, "deepseek-flash", ts)
        .and_then(|v| v.peak)
        .is_some_and(|(schedule, _)| schedule.contains(ts))
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
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > month_days {
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

/// Inverse of [`days_from_civil`].
#[cfg(test)]
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (year + i64::from(month <= 2), month, day)
}

/// Local calendar date of an instant. Codex names its session directories with
/// local dates, so the scanner has to ask the same question the writer did.
#[cfg(test)]
fn local_date(ts: i64) -> (i64, i64, i64) {
    unsafe {
        let instant = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&instant, &mut tm).is_null() {
            return civil_from_days(ts.div_euclid(SECS_PER_DAY));
        }
        (
            tm.tm_year as i64 + 1900,
            tm.tm_mon as i64 + 1,
            tm.tm_mday as i64,
        )
    }
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tokens {
    pub uncached: u64,
    pub cached: u64,
    pub output: u64,
}

impl Tokens {
    pub fn total(self) -> u64 {
        self.uncached
            .saturating_add(self.cached)
            .saturating_add(self.output)
    }

    fn add(&mut self, other: Tokens) {
        self.uncached = self.uncached.saturating_add(other.uncached);
        self.cached = self.cached.saturating_add(other.cached);
        self.output = self.output.saturating_add(other.output);
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
    Last24Hours,
    SevenDays,
    ThirtyDays,
}

impl Window {
    pub const ALL: [Self; 4] = [
        Self::Today,
        Self::SevenDays,
        Self::ThirtyDays,
        Self::Last24Hours,
    ];

    pub fn start(self, now: i64) -> i64 {
        if self == Self::Today {
            local_day_start(now)
        } else {
            now - self.days() * SECS_PER_DAY
        }
    }

    pub fn days(self) -> i64 {
        match self {
            Window::Today | Window::Last24Hours => 1,
            Window::SevenDays => 7,
            Window::ThirtyDays => 30,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Window::Today => Window::Last24Hours,
            Window::Last24Hours => Window::SevenDays,
            Window::SevenDays => Window::ThirtyDays,
            Window::ThirtyDays => Window::Today,
        }
    }

    pub fn recent(self) -> metering::Recent {
        match self {
            Self::Today => metering::Recent::Today,
            Self::Last24Hours => metering::Recent::Day,
            Self::SevenDays => metering::Recent::SevenDays,
            Self::ThirtyDays => metering::Recent::ThirtyDays,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "当天",
            Self::Last24Hours => "24h",
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Today => "ds当天",
            Self::Last24Hours => "ds24h",
            Self::SevenDays => "ds7d",
            Self::ThirtyDays => "ds30d",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Window::Today => 0,
            Window::Last24Hours => 3,
            Window::SevenDays => 1,
            Window::ThirtyDays => 2,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            3 => Window::Last24Hours,
            1 => Window::SevenDays,
            2 => Window::ThirtyDays,
            _ => Window::Today,
        }
    }
}

/// Window selected on a Codex scope row.
///
/// `Cycle` exists only on the signed-in account row: the weekly reset boundary
/// belongs to one account, so the all-accounts row has no cycle of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CodexWindow {
    #[default]
    Cycle,
    Recent(Window),
}

impl CodexWindow {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cycle => "本期",
            Self::Recent(window) => window.label(),
        }
    }

    /// Click order on the signed-in account row.
    pub fn next_account(self) -> Self {
        match self {
            Self::Cycle => Self::Recent(Window::Today),
            Self::Recent(window) => match window {
                Window::Today => Self::Recent(Window::Last24Hours),
                Window::Last24Hours => Self::Recent(Window::SevenDays),
                Window::SevenDays => Self::Recent(Window::ThirtyDays),
                Window::ThirtyDays => Self::Cycle,
            },
        }
    }
}

/// Global interval stop shown by the header control. The account row owns the
/// cycle, so the `Cycle` stop means account = 本期 and both other rows = 当天.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalStop {
    Cycle,
    Rolling(Window),
    Custom,
}

impl IntervalStop {
    fn next(self) -> Self {
        match self {
            Self::Cycle => Self::Rolling(Window::Today),
            Self::Rolling(Window::Today) => Self::Rolling(Window::Last24Hours),
            Self::Rolling(Window::Last24Hours) => Self::Rolling(Window::SevenDays),
            Self::Rolling(Window::SevenDays) => Self::Rolling(Window::ThirtyDays),
            Self::Rolling(Window::ThirtyDays) => Self::Cycle,
            Self::Custom => Self::Cycle,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum RecordId {
    /// Codex repeats a whole usage event when a session is resumed or forked
    /// into another rollout file, and the copy carries the same numbers.
    Codex {
        identity: Box<str>,
        ts: i64,
        uncached: u64,
        cached: u64,
        output: u64,
    },
    /// Claude Code writes the same `message.id` once per streamed content
    /// block; the entry with the largest token total is the real one.
    Claude { message_id: Box<str> },
}

/// One effective, priced-or-countable usage event.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Record {
    codex: Option<metering::EventMeta>,
    ts: i64,
    /// `None` means "DeepSeek model without a price entry".
    family: Option<Family>,
    tokens: Tokens,
    id: RecordId,
}

#[cfg(test)]
#[derive(Debug, Clone, Default)]
struct CachedFile {
    stamp: (u64, u64, i64, i64, u64),
    offset: u64,
    context: source::Context,
    records: Vec<Record>,
    incomplete: bool,
    // Detect ordinary rewrites before continuing an append. Same-size changes
    // always rebuild; fingerprints are not a cryptographic file integrity claim.
    prefix_len: usize,
    prefix_hash: u64,
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
        (Source::Codex, codex.join("archived_sessions")),
        (Source::Claude, home.join(".claude").join("projects")),
    ]
}

/// Legacy scanner retained only for historical regression fixtures.
/// Production collection uses the durable history worker.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct Scanner {
    files: HashMap<PathBuf, CachedFile>,
    incomplete: bool,
}

#[cfg(test)]
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
        let start = window.start(now);
        self.incomplete = false;
        let mut live: HashSet<PathBuf> = HashSet::new();
        for (source, root) in roots {
            if !root.is_dir() {
                if matches!(source, Source::Codex)
                    && root.file_name().is_some_and(|n| n == "sessions")
                {
                    self.incomplete = true;
                }
                continue;
            }
            for path in jsonl_files_since(
                root,
                start.min(now - 31 * SECS_PER_DAY),
                &mut self.incomplete,
            ) {
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
        for file in self.files.values_mut() {
            file.records.retain(|r| r.ts >= now - 31 * SECS_PER_DAY);
        }
        let records = self.files.values().flat_map(|file| file.records.iter());
        Some(aggregate(records.filter(|r| r.ts < now), start))
    }

    fn refresh_file(&mut self, source: Source, path: &Path, _start: i64) {
        use std::io::Read;
        let Ok(mut file) = File::open(path) else {
            self.files.remove(path);
            self.incomplete = true;
            return;
        };
        let Ok(metadata) = file.metadata() else {
            self.files.remove(path);
            self.incomplete = true;
            return;
        };
        let stamp = (
            metadata.dev(),
            metadata.ino(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.len(),
        );
        let cached = self.files.entry(path.to_path_buf()).or_default();
        if cached.stamp == stamp {
            return;
        }
        let mut prefix = vec![0; metadata.len().min(512) as usize];
        if file.read_exact(&mut prefix).is_err() {
            cached.incomplete = true;
            return;
        }
        let append = cached.offset > 0
            && stamp.0 == cached.stamp.0
            && stamp.1 == cached.stamp.1
            && stamp.4 > cached.stamp.4
            && prefix.len() >= cached.prefix_len
            && fingerprint(&prefix[..cached.prefix_len]) == cached.prefix_hash;
        if !append {
            *cached = CachedFile::default();
        }
        if file.seek(SeekFrom::Start(cached.offset)).is_err() {
            cached.incomplete = true;
            return;
        }
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(n) if line.ends_with('\n') => {
                    cached.offset += n as u64;
                    source::parse_line(
                        source,
                        &line,
                        path,
                        &mut cached.context,
                        &mut cached.records,
                        &mut cached.incomplete,
                    );
                }
                Ok(_) => break, // reread incomplete tail at next append
                Err(_) => {
                    cached.incomplete = true;
                    break;
                }
            }
        }
        cached.prefix_len = prefix.len();
        cached.prefix_hash = fingerprint(&prefix);
        cached.stamp = stamp;
    }
}

#[cfg(test)]
fn fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Every `*.jsonl` under `root` whose mtime is at or after `start`.
///
/// A file that received an in-window event was necessarily written at or after
/// the window start, so mtime is a safe pre-filter: it can only skip files
/// that cannot contribute. It also means a long-lived session started days ago
/// is still found, and that neither tree needs a date directory layout.
#[cfg(test)]
fn jsonl_files_since(root: &Path, start: i64, incomplete: &mut bool) -> Vec<PathBuf> {
    fn walk(dir: &Path, start: i64, depth: usize, out: &mut Vec<PathBuf>, incomplete: &mut bool) {
        if depth > 16 {
            *incomplete = true;
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            *incomplete = true;
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                *incomplete = true;
                continue;
            };
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                *incomplete = true;
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                walk(&path, start, depth + 1, out, incomplete);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                *incomplete = true;
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
    walk(root, start, 0, &mut out, incomplete);
    out.sort();
    out
}

/// Fold records into one window total, dropping duplicates.
#[cfg(test)]
fn aggregate<'a>(records: impl Iterator<Item = &'a Record>, start: i64) -> Spend {
    let mut spend = Spend::default();
    let mut exact_cost = 0u128;
    let mut codex_seen: HashSet<&RecordId> = HashSet::new();
    let mut claude_best: HashMap<&str, &Record> = HashMap::new();
    for record in records {
        if record.ts < start || record.codex.is_some() {
            continue;
        }
        match &record.id {
            RecordId::Codex { .. } => {
                if !codex_seen.insert(&record.id) {
                    continue;
                }
            }
            RecordId::Claude { message_id } => match claude_best.get(message_id.as_ref()) {
                Some(previous)
                    if (
                        previous.tokens.total(),
                        previous.tokens.uncached,
                        previous.tokens.cached,
                        previous.tokens.output,
                    ) >= (
                        record.tokens.total(),
                        record.tokens.uncached,
                        record.tokens.cached,
                        record.tokens.output,
                    ) =>
                {
                    continue;
                }
                _ => {
                    claude_best.insert(message_id, record);
                    continue;
                }
            },
        }
        exact_cost += add_record(&mut spend, record);
    }
    for record in claude_best.values() {
        exact_cost += add_record(&mut spend, record);
    }
    spend.cost_cny = exact_cost as f64 / 1_000_000_000_000.0;
    spend
}

fn add_record(spend: &mut Spend, record: &Record) -> u128 {
    spend.tokens.add(record.tokens);
    let model = record.family.map(|f| match f {
        Family::Flash => "deepseek-flash",
        Family::Pro => "deepseek-v4-pro",
    });
    match model.and_then(|model| pricing::version(pricing::BOOK, model, record.ts)) {
        Some(v) => pricing::cost(
            v.rates(record.ts, record.tokens.uncached + record.tokens.cached),
            record.tokens,
        ),
        None => {
            spend.unpriced = true;
            0
        }
    }
}

// ─── Worker ─────────────────────────────────────────────────────────────────

/// All provider windows computed from one scan and one timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub history: history::Health,
    pub spends: [Spend; 4],
    /// Every local Codex record, regardless of account.
    pub codex_all: metering::Snapshot,
    /// Only records attributed to the observed daemon account.
    pub codex_account: metering::Snapshot,
    pub codex_account_id: Option<String>,
}

/// Rows with independent total/daily display preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageRow {
    DeepSeek,
    CodexAccount,
    CodexAll,
}

/// Local UI state for the spend row plus the mailbox shared with the scanner
/// thread. Mirrors [`crate::quota::QuotaState`]: nothing here is written to
/// tmux, and no credential or prompt text passes through it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuotaCycle {
    pub account: Option<String>,
    pub end: u64,
}

impl QuotaCycle {
    pub(crate) fn for_account(&self, account: Option<&str>) -> u64 {
        if account.is_some() && account == self.account.as_deref() {
            self.end
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub struct UsageState {
    pub history: history::Health,
    /// DeepSeek's window; the DS row cycles independently.
    pub selected: Window,
    pub codex_all: Option<metering::Snapshot>,
    pub codex_account: Option<metering::Snapshot>,
    /// Observed daemon account used to select the cached account snapshot.
    pub codex_account_id: Option<String>,
    /// Signed-in account row: cycle plus rolling windows.
    pub codex_account_window: CodexWindow,
    /// All-accounts row: rolling windows only.
    pub codex_all_window: Window,
    /// Row-level total/daily display mode, shared by amount and tokens.
    daily: [bool; 3],
    pub cycle: std::sync::Arc<std::sync::Mutex<QuotaCycle>>,
    /// Last result whose window matches `selected`.
    pub snapshot: Option<Spend>,
    /// Whether the scanner has reported at least once. The row stays hidden
    /// until then so the shared snapshot fixtures keep their current shape.
    pub received: bool,
    /// Newest agent status-change stamp already turned into a scan request.
    pub last_status_stamp: u64,
    /// All windows from the same background scan; switching requires no I/O.
    pub spends: Option<[Spend; 4]>,
    /// Re-check-now request, set by activity and quota-cycle changes.
    pub force: std::sync::Arc<AtomicBool>,
}

impl Default for UsageState {
    fn default() -> Self {
        Self {
            history: history::Health::default(),
            selected: Window::Today,
            codex_all: None,
            codex_account: None,
            codex_account_id: None,
            codex_account_window: CodexWindow::Cycle,
            codex_all_window: Window::Today,
            daily: [false; 3],
            cycle: Default::default(),
            snapshot: None,
            received: false,
            last_status_stamp: 0,
            spends: None,
            force: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }
}

impl UsageState {
    /// Signed-in account's weekly cycle, when the live boundary matches.
    pub fn cycle_summary(&self, now: u64) -> Option<&metering::Summary> {
        let snapshot = self.codex_account.as_ref()?;
        let cycle = self.cycle.lock().ok()?;
        (self.codex_account_id == cycle.account
            && snapshot.cycle_end == cycle.end
            && now < cycle.end)
            .then_some(())?;
        snapshot.cycle.as_ref()
    }

    pub fn summary(&self, row: UsageRow, now: u64) -> Option<&metering::Summary> {
        match row {
            UsageRow::DeepSeek => None,
            UsageRow::CodexAccount => match self.codex_account_window {
                CodexWindow::Cycle => self.cycle_summary(now),
                CodexWindow::Recent(window) => self
                    .codex_account
                    .as_ref()
                    .map(|s| &s.recent[window.recent().index()]),
            },
            UsageRow::CodexAll => self
                .codex_all
                .as_ref()
                .map(|s| &s.recent[self.codex_all_window.recent().index()]),
        }
    }

    /// Daily mode is meaningful only for multi-day windows. Today is a partial
    /// day; 24h would equal the total itself. The cycle uses its elapsed
    /// seven-day span.
    fn allows_daily(&self, row: UsageRow) -> bool {
        match row {
            UsageRow::DeepSeek => {
                matches!(self.selected, Window::SevenDays | Window::ThirtyDays)
            }
            UsageRow::CodexAccount => match self.codex_account_window {
                CodexWindow::Cycle => true,
                CodexWindow::Recent(window) => {
                    matches!(window, Window::SevenDays | Window::ThirtyDays)
                }
            },
            UsageRow::CodexAll => {
                matches!(
                    self.codex_all_window,
                    Window::SevenDays | Window::ThirtyDays
                )
            }
        }
    }

    /// Use the observation's interval, not render time: stale snapshots must
    /// retain the same rate until fresh usage arrives.
    pub fn daily_seconds(&self, row: UsageRow, now: u64) -> Option<u64> {
        let seconds = match row {
            UsageRow::DeepSeek => {
                self.snapshot.as_ref()?;
                // Both Codex snapshots share one observation timestamp.
                let observed = self
                    .codex_all
                    .as_ref()
                    .map_or(now as i64, |s| s.observed_at);
                observed.checked_sub(self.selected.start(observed))?
            }
            _ => {
                let summary = self.summary(row, now)?;
                summary.end.checked_sub(summary.start)?
            }
        };
        u64::try_from(seconds).ok().filter(|&seconds| seconds > 0)
    }

    pub fn can_toggle_amount(&self, row: UsageRow, now: u64) -> bool {
        self.can_toggle_tokens(row, now)
            && match row {
                UsageRow::DeepSeek => self.snapshot.as_ref().is_some_and(|s| !s.unpriced),
                _ => self
                    .summary(row, now)
                    .is_some_and(|s| s.total.unpriced == 0),
            }
    }

    pub fn can_toggle_tokens(&self, row: UsageRow, now: u64) -> bool {
        self.allows_daily(row) && self.daily_seconds(row, now).is_some()
    }

    /// Row-level display mode: amount and tokens switch together.
    pub fn show_daily(&self, row: UsageRow, now: u64) -> bool {
        self.daily[row as usize] && self.can_toggle_tokens(row, now)
    }

    pub fn toggle_daily(&mut self, row: UsageRow, now: u64) {
        if self.can_toggle_tokens(row, now) {
            self.daily[row as usize] = !self.daily[row as usize];
        }
    }

    /// Flip every eligible row to the same target state.
    pub fn toggle_daily_all(&mut self, now: u64) {
        let rows = [
            UsageRow::DeepSeek,
            UsageRow::CodexAccount,
            UsageRow::CodexAll,
        ];
        let target = !rows.iter().any(|&row| self.show_daily(row, now));
        for row in rows {
            if self.can_toggle_tokens(row, now) {
                self.daily[row as usize] = target;
            }
        }
    }

    /// Publish the complete scan using the currently selected window.
    pub fn apply(&mut self, update: Update) {
        self.history = update.history;
        if !self.history.available {
            return;
        }
        self.received = true;
        self.codex_all = Some(update.codex_all);
        self.codex_account = Some(update.codex_account);
        self.codex_account_id = update.codex_account_id;
        self.snapshot = Some(update.spends[self.selected.code() as usize].clone());
        self.spends = Some(update.spends);
    }

    /// Common rolling window of the visible usage rows. The account cycle and
    /// mixed selections have no shared suffix.
    /// The canonical global stop, or `Custom` after independent row clicks.
    pub fn interval_stop(&self) -> IntervalStop {
        let codex_visible = self.codex_all.is_some() || self.codex_account.is_some();
        if !codex_visible {
            return IntervalStop::Rolling(self.selected);
        }
        if self.codex_account_window == CodexWindow::Cycle
            && self.codex_all_window == Window::Today
            && self.selected == Window::Today
        {
            return IntervalStop::Cycle;
        }
        if let CodexWindow::Recent(account) = self.codex_account_window
            && account == self.codex_all_window
            && account == self.selected
        {
            return IntervalStop::Rolling(account);
        }
        IntervalStop::Custom
    }

    pub fn shared_window(&self) -> Option<Window> {
        match self.interval_stop() {
            IntervalStop::Rolling(window) => Some(window),
            IntervalStop::Cycle | IntervalStop::Custom => None,
        }
    }

    fn select_deepseek(&mut self, window: Window) {
        self.selected = window;
        self.snapshot = self
            .spends
            .as_ref()
            .map(|s| s[window.code() as usize].clone());
    }

    /// Switch only DeepSeek using the last complete scan.
    pub fn cycle(&mut self) {
        self.select_deepseek(self.selected.next());
    }

    pub fn cycle_codex_account(&mut self) {
        self.codex_account_window = self.codex_account_window.next_account();
    }

    pub fn cycle_codex_all(&mut self) {
        self.codex_all_window = self.codex_all_window.next();
    }

    fn apply_interval(&mut self, stop: IntervalStop) {
        match stop {
            IntervalStop::Cycle => {
                self.codex_account_window = CodexWindow::Cycle;
                self.codex_all_window = Window::Today;
                self.select_deepseek(Window::Today);
            }
            IntervalStop::Rolling(window) => {
                self.codex_account_window = CodexWindow::Recent(window);
                self.codex_all_window = window;
                self.select_deepseek(window);
            }
            IntervalStop::Custom => {}
        }
    }

    /// Cycle through 本期 (account cycle + both other rows today), 当天, 24h,
    /// 7d, 30d, and back. A custom row combination returns to the 本期 stop.
    pub fn cycle_all(&mut self) {
        let next = self.interval_stop().next();
        self.apply_interval(next);
    }
}

/// Refresh all windows in the existing background worker. Clicks only select cached results.
pub fn poll_loop(tx: &Sender<Update>, force: &AtomicBool, cycle: &std::sync::Mutex<QuotaCycle>) {
    history::run(tx, force, cycle);
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
            codex: None,
            ts,
            family: family_for(model),
            tokens,
            id: RecordId::Codex {
                identity: "fixture".into(),
                ts,
                uncached: tokens.uncached,
                cached: tokens.cached,
                output: tokens.output,
            },
        }
    }

    fn claude_record(ts: i64, model: &str, message_id: &str, tokens: Tokens) -> Record {
        Record {
            codex: None,
            ts,
            family: family_for(model),
            tokens,
            id: RecordId::Claude {
                message_id: message_id.into(),
            },
        }
    }

    #[test]
    fn cycle_snapshot_requires_matching_account_even_with_identical_reset() {
        let now = 100;
        let end = 200;
        let mut state = UsageState::default();
        let update = |account: &str| Update {
            history: history::Health::default(),
            spends: Default::default(),
            codex_all: Default::default(),
            codex_account: metering::Snapshot {
                cycle_end: end,
                cycle: Some(metering::Summary::default()),
                ..Default::default()
            },
            codex_account_id: Some(account.into()),
        };
        *state.cycle.lock().unwrap() = QuotaCycle {
            account: Some("a".into()),
            end,
        };
        state.apply(update("a"));
        assert!(state.cycle_summary(now).is_some());
        // B may share A's reset timestamp. Neither retained A data nor a late A
        // worker result may be shown as B's current cycle.
        state.cycle.lock().unwrap().account = Some("b".into());
        assert!(state.cycle_summary(now).is_none());
        let mut failed = update("b");
        failed.history.available = false;
        state.apply(failed);
        assert!(state.cycle_summary(now).is_none());
        state.apply(update("b"));
        assert!(state.cycle_summary(now).is_some());
        state.apply(update("a"));
        assert!(state.cycle_summary(now).is_none());
        // Last-good rolling results remain cached for their normal display.
        assert!(state.codex_account.is_some());
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
        assert_eq!(parse_timestamp("2026-02-29T00:00:00Z"), None);
        assert_eq!(parse_timestamp("2026-04-31T00:00:00Z"), None);
        assert_eq!(parse_timestamp("1900-02-29T00:00:00Z"), None);
        assert!(parse_timestamp("2000-02-29T00:00:00Z").is_some());
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
    fn today_and_rolling_windows_stay_distinct_across_dst() {
        // May also be run under TZ=America/New_York; never mutate process TZ.
        for stamp in ["2026-03-10T12:00:00Z", "2026-11-03T12:00:00Z"] {
            let now = parse_timestamp(stamp).unwrap();
            let start = Window::Today.start(now);
            assert_eq!(local_date(start), local_date(now));
            assert_eq!(Window::Today.start(start), start);
            assert_eq!(Window::Last24Hours.start(now), now - 86_400);
            assert_eq!(Window::SevenDays.start(now), now - 604_800);
        }
    }

    #[test]
    fn price_periods_apply_from_their_effective_instant() {
        let before = pricing::version(pricing::BOOK, "deepseek-flash", CUTOFF - 1);
        assert!(
            before.is_none(),
            "no period covers instants before the table"
        );
        let peak = pricing::version(pricing::BOOK, "deepseek-flash", FRI_0100)
            .unwrap()
            .rates(FRI_0100, 0);
        assert_eq!(peak.input, 2_000_000);
        let off_peak = pricing::version(pricing::BOOK, "deepseek-v4-pro", FRI_1000)
            .unwrap()
            .rates(FRI_1000, 0);
        assert_eq!(off_peak.input, 4_500_000);
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
    fn provider_windows_share_exact_bounds_and_exclude_the_end() {
        let now = FRI_0600;
        let records = [
            codex_record(now - SECS_PER_DAY - 1, "deepseek-flash", tokens(1, 0, 0)),
            codex_record(now - SECS_PER_DAY, "deepseek-flash", tokens(2, 0, 0)),
            codex_record(now - 1, "deepseek-flash", tokens(4, 0, 0)),
            codex_record(now, "deepseek-flash", tokens(8, 0, 0)),
        ];
        assert_eq!(
            aggregate(
                records.iter().filter(|r| r.ts < now),
                Window::Last24Hours.start(now)
            )
            .tokens
            .total(),
            6
        );
        assert_eq!(Window::Today.start(now), local_day_start(now));
        assert_eq!(Window::SevenDays.start(now), now - 7 * SECS_PER_DAY);
        assert_eq!(Window::ThirtyDays.start(now), now - 30 * SECS_PER_DAY);
        let codex = metering::summarize(&Scanner::default(), now, 0);
        for (ds, cx) in [
            (Window::Today, metering::Recent::Today),
            (Window::Last24Hours, metering::Recent::Day),
            (Window::SevenDays, metering::Recent::SevenDays),
            (Window::ThirtyDays, metering::Recent::ThirtyDays),
        ] {
            assert_eq!(ds.start(now), codex.recent[cx.index()].start);
            assert_eq!(codex.recent[cx.index()].end, now);
        }
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
        assert_eq!(Window::Today.next(), Window::Last24Hours);
        assert_eq!(Window::Last24Hours.next(), Window::SevenDays);
        assert_eq!(Window::SevenDays.next(), Window::ThirtyDays);
        assert_eq!(Window::ThirtyDays.next(), Window::Today);
        assert_eq!(
            Window::from_code(Window::ThirtyDays.code()),
            Window::ThirtyDays
        );
        assert_eq!(Window::from_code(9), Window::Today);
        assert_eq!(Window::Today.name(), "ds当天");
        assert_eq!(Window::SevenDays.name(), "ds7d");
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

    /// Codex fixtures have to sit in the local-date directory the scanner
    /// walks, built the same way the writer names it — which also keeps these
    /// tests independent of the machine's timezone.
    fn dated_codex_path(name: &str, ts: i64) -> String {
        let (year, month, day) = local_date(ts);
        format!("{year:04}/{month:02}/{day:02}/{name}")
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
            &dated_codex_path("a.jsonl", FRI_0600),
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
    fn touched_up_old_session_files_stay_out_of_the_window() {
        // Regression: sync and backup tools rewrite old session files, so their
        // mtime is fresh. An mtime-only filter parsed ~950MB of May–August logs
        // for a "today" window; the directory date is what bounds it.
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "2026/07/21/rollout.jsonl",
            &codex_fixture_lines("2026-07-21T02:43:06.594Z", "deepseek-flash"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        let spend = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert!(spend.is_empty(), "old events are excluded by event time");
        assert_eq!(
            scanner.files.len(),
            1,
            "old directories must remain discoverable"
        );
    }

    #[test]
    fn a_session_started_days_ago_still_counts_when_it_writes_today() {
        // A rollout lives in the directory of the day it started, so the scan
        // reaches a few days back and lets the event timestamp decide.
        let dir = tempfile::tempdir().unwrap();
        let started = FRI_0600 - 3 * SECS_PER_DAY;
        write(
            dir.path(),
            &dated_codex_path("rollout.jsonl", started),
            &codex_fixture_lines("2026-09-18T02:43:06.594Z", "deepseek-flash"),
        );
        let mut scanner = Scanner::default();
        let roots = vec![(Source::Codex, dir.path().to_path_buf())];
        let spend = scanner
            .scan_roots(&roots, Window::Today, FRI_0600, &|| false)
            .unwrap();
        assert_eq!(spend.tokens, tokens(200, 800, 100));
    }

    #[test]
    fn scanner_reuses_cached_files_and_drops_stale_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            &dated_codex_path("a.jsonl", FRI_0600),
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
            &dated_codex_path("a.jsonl", FRI_0600),
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
    #[test]
    #[ignore = "explicit read-only benchmark; requires USAGE_BENCH_ROOT"]
    fn usage_corpus_benchmark() {
        let root = std::env::var_os("USAGE_BENCH_ROOT").expect("explicit corpus root required");
        let mut scanner = Scanner::default();
        let now = crate::time::now_epoch_secs() as i64;
        let roots = [(Source::Codex, PathBuf::from(root))];
        let started = std::time::Instant::now();
        scanner
            .scan_roots(&roots, Window::ThirtyDays, now, &|| false)
            .unwrap();
        let initial = started.elapsed();
        let files = scanner.files.len();
        let records: usize = scanner.files.values().map(|f| f.records.len()).sum();
        let started = std::time::Instant::now();
        scanner
            .scan_roots(&roots, Window::ThirtyDays, now, &|| false)
            .unwrap();
        let snapshot = metering::summarize(&scanner, now, 0);
        let total = &snapshot.recent[2].total;
        eprintln!(
            "30d coverage: unpriced={}, unknown_tier={}, missing_price={}, cache_write={}, assumed_price={}, assumed_standard={}",
            total.unpriced,
            total.unknown_tier,
            total.missing_price,
            total.unsupported_cache_write,
            total.assumed_price,
            total.assumed_standard
        );
        eprintln!(
            "usage benchmark: files={files}, records={records}, initial={initial:?}, cached_and_query={:?}",
            started.elapsed()
        );
    }
}
