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

/// Normal refresh cadence while the subscription's own agent is idle.
/// Quota windows do not move on their own: the user only ever consumes
/// quota by running the CLI, so an idle agent means a frozen number.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Fetch floor per subscription while its own agent is actively working
/// (running / background / waiting). The Codex fetch spawns an app-server
/// subprocess while Kimi's is a single curl, so Codex gets the higher floor.
pub const CODEX_ACTIVE_INTERVAL: Duration = Duration::from_secs(90);
pub const KIMI_ACTIVE_INTERVAL: Duration = Duration::from_secs(60);

/// Failure backoff cap, so an offline or VPN-less machine is not polled
/// aggressively. The backoff itself is decorrelated jitter (see
/// [`SubscriptionCadence::settle`]).
pub const MAX_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Floor for a reset-boundary fetch. A window's reset changes the number by
/// more than waiting is worth, but the provider needs a moment to roll it, and
/// the floor keeps a stale or skewed `resets_at` from becoming a tight loop.
pub const MIN_RESET_INTERVAL: Duration = Duration::from_secs(60);

/// EWMA weight on the newest consumption-rate sample: heavy enough that a
/// burst snaps the cadence back to the floor in one step, light enough that a
/// single quantized 1% drop does not pin it there.
const RATE_ALPHA: f64 = 0.4;

/// Rates below this (percent points per minute) are treated as "not
/// consuming". The percent is integer-quantized, so a slow burn legitimately
/// reads as zero for minutes at a time.
const RATE_EPSILON: f64 = 0.01;

/// The active cadence aims to observe about one displayed percent point of
/// consumption per poll.
const TARGET_DELTA_PER_POLL: f64 = 1.0;

/// Random spread on the idle cadence, desyncing this sidebar's timer from
/// sibling pollers (the Raycast extension) that share the same credentials.
const IDLE_JITTER: Duration = Duration::from_secs(30);

/// Random spread on a reset-boundary pull-forward, so the refetch lands
/// after the provider has actually rolled the window rather than racing it.
const BOUNDARY_JITTER: Duration = Duration::from_secs(20);

/// When the boundary fetch lands but the provider has not rolled the window
/// yet (the stamp is already past and no new one arrived), retry this soon,
/// at most `RESET_RETRY_MAX` times, instead of waiting out the base cadence
/// while the UI shows a `now` countdown.
const RESET_RETRY_INTERVAL: Duration = Duration::from_secs(45);
const RESET_RETRY_JITTER: Duration = Duration::from_secs(15);
const RESET_RETRY_MAX: u32 = 4;

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
    /// Remaining quota, `0..=100`. The block shows what is left, matching the
    /// Raycast extension's display semantics.
    pub remaining_percent: u8,
    /// Wall-clock reset time in Unix **seconds**, when the API provides one.
    /// Deliberately the same clock and unit as [`AppState::now`] so countdown
    /// arithmetic never mixes milliseconds with seconds.
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
    /// Why the most recent fetch failed, rendered as a two-character tag
    /// after the age marker. `None` while the data is fresh.
    pub last_error_kind: Option<FetchKind>,
}

impl SubscriptionQuota {
    pub fn new(windows: Vec<QuotaWindow>) -> Self {
        Self {
            windows,
            fetched_at: Instant::now(),
            fetch_failed: false,
            last_error_kind: None,
        }
    }

    /// `true` when the last refetch failed and the snapshot may be stale.
    pub fn is_stale(&self) -> bool {
        self.fetch_failed
    }

    /// Age of the snapshot as a compact marker (`17m`, `2h13m`). `None` while
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

/// Why a fetch failed, in the few buckets the stale row can meaningfully
/// show. Surfaced as a two-character Chinese tag after the age marker so the
/// dimmed row says *why* it is dim, not just *how old*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    /// Provider throttling (HTTP 429, `Retry-After`).
    Throttled,
    /// Login problem: 401/403, expired or unreadable credentials.
    Auth,
    /// Transport problem: curl/subprocess start, non-zero exit, timeout,
    /// early process exit.
    Network,
    /// Response problem: invalid JSON, empty payload, no usable windows,
    /// schema drift, an account changing mid-query.
    Data,
    /// Anything that does not fit the buckets above.
    Other,
}

impl FetchKind {
    /// Two-character tag rendered after the stale row's age marker.
    pub fn label(&self) -> &'static str {
        match self {
            FetchKind::Throttled => "限流",
            FetchKind::Auth => "登录",
            FetchKind::Network => "网络",
            FetchKind::Data => "数据",
            FetchKind::Other => "错误",
        }
    }
}

/// A fetch failure. `retry_after` carries the provider's throttle hint
/// (HTTP 429 `Retry-After`) when one was sent, so the cadence honors it
/// instead of guessing a backoff. `kind` classifies the failure for the
/// stale row's marker; constructors that do not name one default to
/// [`FetchKind::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchError {
    pub message: String,
    pub retry_after: Option<Duration>,
    pub kind: FetchKind,
}

impl FetchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(message, FetchKind::Other)
    }

    pub fn with_kind(message: impl Into<String>, kind: FetchKind) -> Self {
        Self {
            message: message.into(),
            retry_after: None,
            kind,
        }
    }

    pub fn throttled(message: impl Into<String>, retry_after: Option<Duration>) -> Self {
        Self {
            message: message.into(),
            retry_after,
            kind: FetchKind::Throttled,
        }
    }
}

impl From<String> for FetchError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for FetchError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchError {}

/// What a subscription fetch returns.
pub type FetchOutcome = Result<QuotaFetch, FetchError>;

/// One HTTP response from a curl subprocess, with the provider's throttle
/// hint when the response carried a `Retry-After` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<Duration>,
}

impl HttpResponse {
    pub fn new(status: u16, body: String, retry_after: Option<Duration>) -> Self {
        Self {
            status,
            body,
            retry_after,
        }
    }
}

// ── shared curl transport ───────────────────────────────────────────
//
// Both fetchers speak HTTP through a `curl` subprocess, matching the
// repository's tmux/ps/lsof idiom and avoiding an HTTP crate. The runner is
// injectable so no test ever performs network I/O.

/// A `curl` invoker: `(url, extra args, body)` → response; `Err` for
/// transport failures (DNS, timeout, non-zero exit), already tagged
/// [`FetchKind::Network`]. `body` is sent as `--data`.
pub(crate) type CurlRunner<'a> =
    &'a dyn Fn(&str, &[&str], Option<&str>) -> Result<HttpResponse, FetchError>;

/// Flags shared by every curl invocation. `-o -` streams the body to stdout;
/// `--write-out` then appends two trailers: the HTTP status line, and the
/// response headers as a JSON block (`%{header_json}`, curl 7.83+;
/// pretty-printed over many lines) so a `Retry-After` survives the trip.
///
/// The separators are explicit because these endpoints do not all end their
/// bodies with a newline: api.kimi.com answers `{...}` and the status would
/// otherwise be glued onto the JSON.
pub(crate) fn curl_base_args() -> [&'static str; 7] {
    [
        "-sS",
        "-m",
        CURL_TIMEOUT_SECS,
        "--write-out",
        "\n%{http_code}\n%{header_json}",
        "-o",
        "-",
    ]
}

const CURL_TIMEOUT_SECS: &str = "10";

/// Runner used in production: `curl -sS -m 10 ...`.
pub(crate) fn run_curl(
    url: &str,
    args: &[&str],
    body: Option<&str>,
) -> Result<HttpResponse, FetchError> {
    let mut command = std::process::Command::new("curl");
    command.args(curl_base_args()).args(args);
    if let Some(body) = body {
        command.arg("--data").arg(body);
    }
    let output = command.arg(url).output().map_err(|error| {
        FetchError::with_kind(
            format!("curl could not be started: {error}"),
            FetchKind::Network,
        )
    })?;
    if !output.status.success() {
        return Err(FetchError::with_kind(
            format!("curl exited with {}", output.status),
            FetchKind::Network,
        ));
    }
    parse_curl_output(&String::from_utf8_lossy(&output.stdout)).map_err(FetchError::new)
}

/// Split the `--write-out` trailers (status line, then header JSON) from the
/// response body produced by [`run_curl`]. Pure so the parsing can be
/// unit-tested without a subprocess.
///
/// Layout: `body \n status \n header-json`. curl pretty-prints the header
/// JSON one header per line, so the status is located by scanning from the
/// end for the all-digits line; assuming fixed trailer line counts breaks
/// against real curl output.
pub(crate) fn parse_curl_output(output: &str) -> Result<HttpResponse, String> {
    let mut status_line = None;
    let mut search = output.len();
    while let Some(index) = output[..search].rfind('\n') {
        let line = output[index + 1..search].trim();
        if !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_digit()) {
            status_line = Some((index, search));
            break;
        }
        search = index;
    }
    let (payload, status, trailer) = match status_line {
        Some((start, end)) => (&output[..start], &output[start + 1..end], &output[end..]),
        // Defensive fallback for a curl invoker without the separator: take
        // the trailing digits as the status instead of the whole payload.
        None => match trailing_digits(output) {
            Some(index) => (&output[..index], &output[index..], ""),
            None => ("", output, ""),
        },
    };
    let retry_after = serde_json::from_str::<serde_json::Value>(trailer)
        .ok()
        .filter(|headers| headers.is_object())
        .and_then(|headers| retry_after_header(&headers));
    let status = status.trim().parse::<u16>().unwrap_or(0);
    let payload = payload.strip_suffix('\r').unwrap_or(payload).to_string();
    Ok(HttpResponse::new(status, payload, retry_after))
}

/// Byte index where the trailing run of ASCII digits starts, if any.
fn trailing_digits(value: &str) -> Option<usize> {
    let index = value.len() - value.chars().rev().take_while(char::is_ascii_digit).count();
    (index < value.len()).then_some(index)
}

/// Extract `Retry-After` (seconds form) from a curl `%{header_json}` object.
/// Header names arrive lowercase from curl but are matched case-insensitively;
/// the HTTP-date form is ignored — the cadence's backoff covers it.
fn retry_after_header(headers: &serde_json::Value) -> Option<Duration> {
    let object = headers.as_object()?;
    let value = object
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))?
        .1;
    let raw = value.as_array()?.first()?.as_str()?;
    Some(Duration::from_secs(raw.trim().parse().ok()?))
}

/// Atomic credentials writeback: temp file in the same directory, 0600, then
/// rename. Shared by both fetchers' token-refresh paths.
pub(crate) fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_file_name(format!(
        "{}.tmp{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "credentials.json".to_string()),
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut file = create_owner_only(&tmp)?;
        set_owner_only(&file)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|error| format!("credentials could not be written: {error}"))
}

/// Create the temp file owner-only from the start: `File::create` would use
/// the umask (typically 0644), leaving rotated OAuth tokens group-readable
/// until the chmod below lands.
#[cfg(unix)]
fn create_owner_only(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_owner_only(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(path)
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

/// Result delivered from the quota worker to the main loop. A subscription is
/// `None` when its own cadence says it is not due yet. Per-subscription results
/// keep one failure from masking the other's success *or* its refresh cadence.
#[derive(Debug, Clone)]
pub struct QuotaFetchResult {
    pub codex: Option<FetchOutcome>,
    pub kimi: Option<FetchOutcome>,
    /// Whether this result came from a forced (click) refetch. Exposed so the
    /// renderer tests can drive the same code path as the worker.
    pub forced: bool,
}

/// Local quota state: the last good snapshot per subscription.
#[derive(Debug, Default)]
pub struct QuotaState {
    pub codex: Option<SubscriptionQuota>,
    pub kimi: Option<SubscriptionQuota>,
    /// Whether the worker has reported at least once. Before that the block
    /// paints placeholder rows for both subscriptions instead of staying
    /// invisible for the seconds the first fetch takes.
    pub received_first_result: bool,
}

impl QuotaState {
    /// Number of subscriptions that currently have a snapshot to render.
    pub fn subscription_count(&self) -> usize {
        usize::from(self.codex.is_some()) + usize::from(self.kimi.is_some())
    }

    /// Subscriptions the block reserves rows for. Before the first result the
    /// fork assumes both known subscriptions so placeholders can render; a
    /// subscription that later reports `Unavailable` (no credentials) drops
    /// out for good.
    pub fn expected_count(&self) -> usize {
        if self.received_first_result {
            self.subscription_count()
        } else {
            QUOTA_SUBSCRIPTION_COUNT
        }
    }

    /// Whether the block is still waiting for its first fetch.
    pub fn is_pending(&self) -> bool {
        !self.received_first_result
    }

    /// Merge one fetch result into the state. `Unavailable` clears the
    /// subscription permanently, `Err` keeps the last good snapshot and marks
    /// it stale (recording the failure kind for the age marker's tag),
    /// `Available` refreshes it.
    pub fn apply(&mut self, subscription: Subscription, fetch: FetchOutcome) {
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
            Err(error) => {
                // A failure before the first success has nothing to preserve,
                // so the subscription simply stays absent.
                if let Some(quota) = slot {
                    quota.fetch_failed = true;
                    quota.last_error_kind = Some(error.kind);
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
        self.quota.received_first_result = true;
        if let Some(codex) = result.codex {
            self.quota.apply(Subscription::Codex, codex);
        }
        if let Some(kimi) = result.kimi {
            self.quota.apply(Subscription::Kimi, kimi);
        }
    }

    /// Rows the full quota block wants: the `Quota` header plus one row per
    /// subscription, both windows inline, plus the DeepSeek spend row once the
    /// scanner has reported.
    pub fn quota_full_height(&self) -> u16 {
        let rows = self.quota.expected_count() as u16 + u16::from(self.usage.received);
        if rows == 0 { 0 } else { 1 + rows }
    }

    /// Rows the compact quota block wants: one row per subscription without the
    /// header and without the reset countdowns, plus the spend row.
    pub fn quota_compact_height(&self) -> u16 {
        self.quota.expected_count() as u16 + u16::from(self.usage.received)
    }
}

/// Tiny splitmix64 PRNG for cadence jitter. Deterministic given a seed so
/// tests can pin the sequence; production seeds from the wall clock and pid.
/// No `rand` dependency, matching the repository's dependency policy (the
/// pet animation uses the same kind of LCG).
#[derive(Debug, Clone)]
pub struct Jitter(u64);

impl Jitter {
    pub fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    pub fn from_entropy() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos() as u64)
            .unwrap_or(0);
        Self(nanos ^ (std::process::id() as u64).rotate_left(32))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform duration in `[lo, hi]`. `hi <= lo` collapses to `lo`.
    pub fn between(&mut self, lo: Duration, hi: Duration) -> Duration {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo).as_millis() as u64 + 1;
        lo + Duration::from_millis(self.next_u64() % span)
    }
}

/// Whether each subscription's own agent is actively working, published by
/// the refresh loop from the pane inventory every second and consumed by
/// [`quota_poll_loop`] to pick its cadence. Shared via `Arc`, exactly like
/// the force-refetch flag.
#[derive(Debug, Default)]
pub struct QuotaAgentActivity {
    pub codex: AtomicBool,
    pub kimi: AtomicBool,
}

impl QuotaAgentActivity {
    pub fn codex_active(&self) -> bool {
        self.codex.load(Ordering::Relaxed)
    }

    pub fn kimi_active(&self) -> bool {
        self.kimi.load(Ordering::Relaxed)
    }
}

/// Time until the earliest reset that is still ahead of `now_epoch`. `None`
/// when no window carries a usable stamp: a reset the provider has already
/// passed says nothing about when the value will next move.
///
/// Both stamps are Unix seconds, the unit the payloads are normalized to.
fn earliest_reset_in(windows: &[QuotaWindow], now_epoch: u64) -> Option<Duration> {
    windows
        .iter()
        .filter_map(|window| window.resets_at)
        .filter(|resets_at| *resets_at > now_epoch)
        .min()
        .map(|resets_at| Duration::from_secs(resets_at - now_epoch))
}

/// Sleep in one-second slices so a forced refetch takes effect within a
/// second. Also wakes early when either activity flag flips, so an agent
/// starting (or going idle) retargets the cadence on the next loop turn
/// instead of waiting out the old deadline. Returns `true` when the sleep
/// was interrupted.
fn sleep_until_due(
    interval: Duration,
    force: &AtomicBool,
    activity: &QuotaAgentActivity,
    baseline: (bool, bool),
) -> bool {
    let deadline = Instant::now() + interval;
    loop {
        if force.load(Ordering::Relaxed) {
            return true;
        }
        if (activity.codex_active(), activity.kimi_active()) != baseline {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_secs(1)));
    }
}

/// Per-subscription fetch cadence. A subscription that keeps failing backs off
/// on its own, so one broken subscription neither slows the other down nor gets
/// hammered at the healthy subscription's cadence.
#[derive(Debug, Clone)]
struct SubscriptionCadence {
    consecutive_failures: u32,
    /// Previous failure delay. The next one is decorrelated jitter —
    /// `random(REFRESH_INTERVAL, prev * 3)` capped — so retry rounds of
    /// independent pollers (this sidebar, the Raycast extension) do not
    /// phase-lock onto the provider.
    prev_backoff: Duration,
    /// Last reported `(label, remaining_percent)` pairs. `resets_at` is
    /// deliberately excluded: a drifting reset stamp is not consumption and
    /// must not count as movement, or the active cadence would pin at its
    /// floor forever.
    last_percents: Option<Vec<(String, u8)>>,
    /// When the displayed percent last moved. Drives the rate estimate.
    last_change_at: Option<Instant>,
    /// EWMA of observed consumption, percent points per minute. Persists
    /// across active/idle transitions — consumption physics do not reset
    /// when the pane goes idle — restarts on a window refill, and decays by
    /// `(1 - RATE_ALPHA)` per unmoved active poll so a stale burst estimate
    /// relaxes instead of pinning the floor.
    rate_per_min: f64,
    /// Consecutive active-mode fetches whose snapshot did not move. Only
    /// consulted while the rate estimate is zero (a slow burn below display
    /// resolution): the cadence probes at `active_floor * 2^n` then, so a
    /// long quiet stretch relaxes toward the idle cadence.
    unchanged_fetches: u32,
    /// Activity state the current `next_due` was scheduled under; a flip
    /// retargets the deadline via [`SubscriptionCadence::retarget`].
    scheduled_active: Option<bool>,
    last_fetch_at: Option<Instant>,
    /// Anti-flap: a pane bouncing in/out of the process snapshot must not
    /// re-pull the deadline on every re-activation.
    last_retarget: Option<Instant>,
    /// Consecutive retries after a reset boundary the provider missed.
    reset_retries: u32,
    /// `None` means "due now", which is how the loop starts.
    next_due: Option<Instant>,
    /// Per-subscription floor while active (the Codex fetch is heavier).
    active_floor: Duration,
    jitter: Jitter,
}

impl SubscriptionCadence {
    fn new(active_floor: Duration, jitter: Jitter) -> Self {
        Self {
            consecutive_failures: 0,
            prev_backoff: REFRESH_INTERVAL,
            last_percents: None,
            last_change_at: None,
            rate_per_min: 0.0,
            unchanged_fetches: 0,
            scheduled_active: None,
            last_fetch_at: None,
            last_retarget: None,
            reset_retries: 0,
            next_due: None,
            active_floor,
            jitter,
        }
    }

    /// Whether this subscription's next fetch is due.
    fn is_due(&self, now: Instant) -> bool {
        self.next_due.is_none_or(|due| now >= due)
    }

    /// React to an agent-activity flip between fetches.
    ///
    /// A newly active agent pulls the next attempt forward to one active
    /// floor, so the block starts tracking consumption promptly instead of
    /// waiting out a deadline that was scheduled under the idle cadence. An
    /// agent going idle earns one trailing fetch on the same floor: the
    /// final turns of a session often bill after the pane has already parked.
    /// Both pulls are skipped while the subscription is backing off (it is
    /// already being retried as fast as its errors allow) and rate-limited to
    /// one per floor interval, so a flapping pane cannot hold the cadence at
    /// the floor.
    fn retarget(&mut self, active: bool, now: Instant) {
        if self.scheduled_active == Some(active) {
            return;
        }
        self.scheduled_active = Some(active);
        if self.consecutive_failures > 0 {
            return;
        }
        let pulled_recently = self
            .last_retarget
            .is_some_and(|at| now.duration_since(at) < self.active_floor);
        let trailing_fetch_useful = !active
            && self
                .last_fetch_at
                .is_some_and(|at| now.duration_since(at) >= self.active_floor);
        if (active && pulled_recently) || (!active && !trailing_fetch_useful) {
            return;
        }
        let due = self.next_due.unwrap_or(now);
        let pulled = due.min(now + self.active_floor);
        // A pull that does not actually shorten the wait is not recorded:
        // counting it would let a no-op trailing pull suppress the next
        // genuine activation.
        if pulled < due {
            self.next_due = Some(pulled);
            self.last_retarget = Some(now);
        }
    }

    /// Fold one fetched snapshot into the movement/rate estimate. Returns
    /// whether the displayed percent moved.
    ///
    /// A window whose percent *rose* is a provider refill, not consumption:
    /// the rate estimate restarts from zero instead of learning a nonsense
    /// negative rate, and the refill itself still counts as movement.
    fn observe(&mut self, windows: &[QuotaWindow], now: Instant) -> bool {
        let percents: Vec<(String, u8)> = windows
            .iter()
            .map(|window| (window.label.clone(), window.remaining_percent))
            .collect();
        let Some(previous) = self.last_percents.take() else {
            self.last_percents = Some(percents);
            self.last_change_at = Some(now);
            return true;
        };
        if previous == percents {
            self.last_percents = Some(percents);
            return false;
        }
        let percent_of = |windows: &[(String, u8)], label: &str| {
            windows.iter().find(|(l, _)| l == label).map(|(_, p)| *p)
        };
        let rose = percents
            .iter()
            .any(|(label, pct)| percent_of(&previous, label).is_some_and(|before| pct > &before));
        let drop: f64 = percents
            .iter()
            .filter_map(|(label, pct)| {
                percent_of(&previous, label).map(|before| before.saturating_sub(*pct) as f64)
            })
            .sum();
        if rose {
            self.rate_per_min = 0.0;
        } else if drop > 0.0
            && let Some(changed_at) = self.last_change_at
        {
            let minutes = now.duration_since(changed_at).as_secs_f64() / 60.0;
            if minutes > 0.0 {
                let sample = drop / minutes;
                self.rate_per_min = if self.rate_per_min > 0.0 {
                    RATE_ALPHA * sample + (1.0 - RATE_ALPHA) * self.rate_per_min
                } else {
                    sample
                };
            }
        }
        self.last_percents = Some(percents);
        self.last_change_at = Some(now);
        true
    }

    /// The active-mode interval. With a measured consumption rate the cadence
    /// aims to observe [`TARGET_DELTA_PER_POLL`] percent per poll, clamped to
    /// `[active_floor, REFRESH_INTERVAL]` — a fast burn sits at the floor, a
    /// slow burn stretches by measurement rather than by luck. While the rate
    /// is zero (a quiet pane, or a burn below display resolution) it probes
    /// at `active_floor * 2^unchanged`, capped at the idle cadence.
    fn active_delay(&self) -> Duration {
        if self.rate_per_min > RATE_EPSILON {
            let minutes = TARGET_DELTA_PER_POLL / self.rate_per_min;
            return Duration::from_secs_f64(minutes * 60.0)
                .clamp(self.active_floor, REFRESH_INTERVAL);
        }
        let mut interval = self.active_floor;
        for _ in 0..self.unchanged_fetches.min(8) {
            interval = interval.saturating_mul(2).min(REFRESH_INTERVAL);
        }
        interval
    }

    /// Record the outcome of a fetch and schedule the next attempt.
    ///
    /// Failures back off with decorrelated jitter and always win over every
    /// other consideration; a `Retry-After` throttle hint raises the delay
    /// further. Successes pick the idle cadence (lightly jittered) or the
    /// rate-adaptive active cadence, then clamp to the earliest window reset
    /// — a rolling window changes the number far more than any amount of
    /// polling is worth, and without the clamp the block keeps showing the
    /// expired window's percentage, next to a `now` countdown, for up to a
    /// full interval. A boundary fetch that finds the provider has not rolled
    /// the window yet retries on a short leash instead of falling back to the
    /// base cadence.
    fn settle(&mut self, result: &FetchOutcome, now: Instant, now_epoch: u64, active: bool) {
        self.scheduled_active = Some(active);
        self.last_fetch_at = Some(now);
        if let Err(error) = result {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            let ceiling = self
                .prev_backoff
                .saturating_mul(3)
                .max(REFRESH_INTERVAL)
                .min(MAX_REFRESH_INTERVAL);
            let mut delay = self.jitter.between(REFRESH_INTERVAL, ceiling);
            if let Some(retry_after) = error.retry_after {
                delay = delay.max(retry_after);
            }
            let delay = delay.min(MAX_REFRESH_INTERVAL);
            self.prev_backoff = delay;
            self.next_due = Some(now + delay);
            return;
        }
        self.consecutive_failures = 0;
        self.prev_backoff = REFRESH_INTERVAL;

        let windows = match result {
            Ok(QuotaFetch::Available(windows)) => Some(windows.as_slice()),
            Ok(QuotaFetch::Unavailable) => None,
            Err(_) => unreachable!("handled above"),
        };

        let moved = windows.is_some_and(|windows| self.observe(windows, now));
        if !active {
            self.unchanged_fetches = 0;
        } else {
            self.unchanged_fetches = if moved {
                0
            } else {
                // An unmoved active poll is a zero-rate sample over that
                // interval: decay the estimate so a rate learned from a past
                // burst relaxes toward the idle cadence instead of pinning
                // the floor for the rest of a quiet-but-active pane.
                self.rate_per_min *= 1.0 - RATE_ALPHA;
                self.unchanged_fetches.saturating_add(1)
            };
        }

        let mut delay = if !active {
            self.jitter
                .between(REFRESH_INTERVAL, REFRESH_INTERVAL + IDLE_JITTER)
        } else {
            self.active_delay()
        };

        match windows {
            Some(windows) => {
                // A stamp in the past means the window should have rolled but
                // no new stamp arrived: the provider is late. Retry shortly
                // instead of waiting out the base cadence next to a `now`
                // countdown. This is checked per window, not via the earliest
                // *future* stamp — a weekly stamp hours away must not shadow
                // a 5h window that just missed its roll.
                let missed = windows
                    .iter()
                    .any(|window| window.resets_at.is_some_and(|stamp| stamp <= now_epoch));
                if missed && self.reset_retries < RESET_RETRY_MAX {
                    self.reset_retries += 1;
                    delay = delay.min(
                        RESET_RETRY_INTERVAL
                            + self.jitter.between(Duration::ZERO, RESET_RETRY_JITTER),
                    );
                } else {
                    if !missed {
                        self.reset_retries = 0;
                    }
                    // After the leash gives up, the next future boundary (the
                    // weekly window) still clamps the wait.
                    if let Some(reset_in) = earliest_reset_in(windows, now_epoch) {
                        let boundary = reset_in.max(MIN_RESET_INTERVAL)
                            + self.jitter.between(Duration::ZERO, BOUNDARY_JITTER);
                        delay = delay.min(boundary);
                    }
                }
            }
            None => self.reset_retries = 0,
        }
        self.next_due = Some(now + delay);
    }

    /// Time left until this subscription's next fetch.
    fn wait(&self, now: Instant) -> Duration {
        match self.next_due {
            Some(due) => due.saturating_duration_since(now),
            None => Duration::ZERO,
        }
    }
}

/// Background quota poller. Modeled on `session_poll_loop`: it owns the
/// fetch cadence and reports results through a channel.
///
/// `fetch_codex` / `fetch_kimi` are injected so tests can exercise the loop
/// without network I/O; production passes the closures in
/// `src/app/workers.rs`.
///
/// Each subscription keeps its own cadence state: a healthy subscription
/// stays on its schedule while the other one backs off, and a backing-off
/// subscription is not retried by the healthy one's ticks.
///
/// The cadence reacts to the pane inventory through `activity`: quota only
/// moves while the user is running the matching CLI, so an active agent
/// (running / background / waiting) earns the rate-adaptive cadence (see
/// [`SubscriptionCadence::settle`]) while an idle one keeps the plain
/// five-minute cadence. Failure backoff always wins, a successful fetch is
/// still pulled forward to the next window reset, and a click remains the
/// manual escape hatch.
pub fn quota_poll_loop(
    tx: &std::sync::mpsc::Sender<QuotaFetchResult>,
    force: &AtomicBool,
    activity: &QuotaAgentActivity,
    fetch_codex: impl Fn() -> FetchOutcome,
    fetch_kimi: impl Fn() -> FetchOutcome,
) {
    let mut codex_cadence = SubscriptionCadence::new(CODEX_ACTIVE_INTERVAL, Jitter::from_entropy());
    let mut kimi_cadence = SubscriptionCadence::new(KIMI_ACTIVE_INTERVAL, Jitter::from_entropy());
    loop {
        let forced = force.swap(false, Ordering::Relaxed);
        let now = Instant::now();
        let now_epoch = crate::time::now_epoch_secs();
        let activity_flags = (activity.codex_active(), activity.kimi_active());
        codex_cadence.retarget(activity_flags.0, now);
        kimi_cadence.retarget(activity_flags.1, now);
        // A click refetches both subscriptions regardless of their cadence.
        let codex = (forced || codex_cadence.is_due(now)).then(&fetch_codex);
        let kimi = (forced || kimi_cadence.is_due(now)).then(&fetch_kimi);
        if let Some(result) = &codex {
            codex_cadence.settle(result, now, now_epoch, activity_flags.0);
        }
        if let Some(result) = &kimi {
            kimi_cadence.settle(result, now, now_epoch, activity_flags.1);
        }
        // A wake with nothing due (an activity flip) has nothing to report;
        // skipping the send also skips a pointless redraw.
        if (codex.is_some() || kimi.is_some())
            && tx
                .send(QuotaFetchResult {
                    codex,
                    kimi,
                    forced,
                })
                .is_err()
        {
            return;
        }
        // Measure the wait from a fresh clock: `settle` anchored `next_due`
        // to the pre-fetch `now`, and the fetches themselves take seconds
        // (the Codex path alone has a 10s timeout).
        let after = Instant::now();
        let wait = codex_cadence.wait(after).min(kimi_cadence.wait(after));
        sleep_until_due(wait, force, activity, activity_flags);
    }
}

/// Two-unit countdowns: `41m`, `2h13m`, `6d21h`, and the bare `2h` / `4d` when
/// the smaller unit is zero. A reset more than a day away keeps its hours
/// rather than collapsing to whole days, so a weekly window still reads as
/// "most of a day left" instead of a flat `4d`.
///
/// Both stamps are Unix seconds — the unit the quota payloads are normalized
/// to and the unit [`AppState::now`] carries.
pub fn format_countdown(now_epoch_secs: u64, resets_at: Option<u64>) -> Option<String> {
    let resets_at = resets_at?;
    if resets_at <= now_epoch_secs {
        return Some("now".to_string());
    }
    let minutes = (resets_at - now_epoch_secs).div_ceil(60);
    if minutes < 60 {
        return Some(format!("{minutes}m"));
    }
    let hours = minutes / 60;
    let remainder = minutes % 60;
    if hours < 24 {
        return Some(match remainder {
            0 => format!("{hours}h"),
            remainder => format!("{hours}h{remainder}m"),
        });
    }
    let days = hours / 24;
    let hours = hours % 24;
    Some(match hours {
        0 => format!("{days}d"),
        hours => format!("{days}d{hours}h"),
    })
}

/// Compact age marker for a stale snapshot, in the same two-unit style as
/// [`format_countdown`]: `45m`, `2h13m`, `6d21h`, and the bare `2h` / `4d`
/// when the smaller unit is zero. The two-unit cap keeps the marker at six
/// columns (`23h59m`) so a stale row cannot outgrow the sidebar's width
/// budget no matter how long fetches keep failing.
pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        return "now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let remainder = minutes % 60;
    if hours < 24 {
        return match remainder {
            0 => format!("{hours}h"),
            remainder => format!("{hours}h{remainder}m"),
        };
    }
    let days = hours / 24;
    let hours = hours % 24;
    match hours {
        0 => format!("{days}d"),
        hours => format!("{days}d{hours}h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_output_splits_status_headers_and_body() {
        // api.kimi.com: no trailing newline in the body; the `--write-out`
        // trailers (status, then header JSON) carry their own separators.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n200\n{\"retry-after\":[\"30\"]}").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), Some(Duration::from_secs(30)))
        );
        // Real curl pretty-prints `%{header_json}` one header per line; the
        // status must still be found and the hint recovered.
        assert_eq!(
            parse_curl_output(
                "{\"a\":1}\n200\n{\n\"content-type\":[\"application/json\"],\n\"retry-after\":[\"30\"],\n\"server-timing\":[\"inner; dur=12\"]\n}"
            )
            .unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), Some(Duration::from_secs(30)))
        );
        // A pretty-printed header block without Retry-After yields no hint.
        assert_eq!(
            parse_curl_output(
                "{\"a\":1}\n200\n{\n\"content-type\":[\"application/json\"],\n\"server-timing\":[\"inner; dur=12\"]\n}"
            )
            .unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), None)
        );
        // Header names are matched case-insensitively.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n200\n{\"Retry-After\":[\"5\"]}").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), Some(Duration::from_secs(5)))
        );
        // No Retry-After header, no hint.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n200\n{\"content-type\":[\"application/json\"]}").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), None)
        );
        // Legacy invoker without the header trailer: status-only parsing
        // still works and the body is not poisoned.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n200").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), None)
        );
        // A body that already ends in a newline keeps it, and JSON serde
        // tolerates the extra whitespace.
        assert_eq!(
            parse_curl_output("{\"a\":1}\n\n200").unwrap(),
            HttpResponse::new(200, "{\"a\":1}\n".to_string(), None)
        );
        assert_eq!(
            parse_curl_output("{\"a\":1}\r\n200").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), None)
        );
        // Defensive fallback for a curl invoker without the separator: take
        // the trailing digits as the status instead of the whole payload.
        assert_eq!(
            parse_curl_output("{\"a\":1}200").unwrap(),
            HttpResponse::new(200, "{\"a\":1}".to_string(), None)
        );
        assert_eq!(
            parse_curl_output("200").unwrap(),
            HttpResponse::new(200, String::new(), None)
        );
        assert_eq!(
            parse_curl_output("").unwrap(),
            HttpResponse::new(0, String::new(), None),
            "an empty response has no status and no body"
        );
        // A status that fails to parse is reported as 0 so callers treat it as
        // a transport problem rather than a valid response.
        assert_eq!(parse_curl_output("body\n").unwrap().status, 0);
    }

    #[test]
    fn curl_write_out_carries_its_own_separator() {
        // Real responses arrive without a trailing newline, so the separator
        // that keeps the status out of the JSON body has to come from
        // `--write-out` itself.
        assert_eq!(curl_base_args()[3], "--write-out");
        assert_eq!(curl_base_args()[4], "\n%{http_code}\n%{header_json}");
    }

    fn window(label: &str, remaining_percent: u8) -> QuotaWindow {
        QuotaWindow {
            label: label.into(),
            remaining_percent,
            resets_at: None,
        }
    }

    fn window_at(label: &str, remaining_percent: u8, resets_at: u64) -> QuotaWindow {
        QuotaWindow {
            label: label.into(),
            remaining_percent,
            resets_at: Some(resets_at),
        }
    }

    #[test]
    fn countdown_formats_minutes_hours_and_days() {
        let now = 1_700_000_000u64;
        assert_eq!(
            format_countdown(now, Some(now + 41 * 60)),
            Some("41m".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + (2 * 60 + 13) * 60)),
            Some("2h13m".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + 120 * 60)),
            Some("2h".to_string())
        );
        // Day-scale resets keep their remaining hours instead of flattening to
        // whole days.
        assert_eq!(
            format_countdown(now, Some(now + (4 * 24 + 15) * 60 * 60)),
            Some("4d15h".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + 4 * 24 * 60 * 60)),
            Some("4d".to_string())
        );
        assert_eq!(
            format_countdown(now, Some(now + (23 * 60 + 59) * 60)),
            Some("23h59m".to_string())
        );
        assert_eq!(format_countdown(now, None), None);
        assert_eq!(format_countdown(now, Some(now - 1)), Some("now".into()));
    }

    #[test]
    fn countdown_rounds_up_partial_minutes() {
        let now = 1_700_000_000u64;
        // 30 seconds away still reads as one minute rather than 0m.
        assert_eq!(
            format_countdown(now, Some(now + 30)),
            Some("1m".to_string())
        );
    }

    #[test]
    fn countdown_uses_the_same_clock_as_app_state_now() {
        // Regression guard: `AppState::now` is epoch *seconds*, so a reset
        // stamp read as milliseconds used to render as ~20000 days.
        let mut state = AppState::new("%0".into());
        state.refresh_now();
        let resets_at = state.now + 3 * 60 * 60;
        assert_eq!(
            format_countdown(state.now, Some(resets_at)),
            Some("3h".to_string())
        );
    }

    fn new_cadence() -> SubscriptionCadence {
        SubscriptionCadence::new(KIMI_ACTIVE_INTERVAL, Jitter::seeded(42))
    }

    fn available(percent: u8) -> FetchOutcome {
        Ok(QuotaFetch::Available(vec![window("5h", percent)]))
    }

    #[test]
    fn jitter_stays_in_range_and_collapses_empty_ranges() {
        let mut jitter = Jitter::seeded(7);
        for _ in 0..100 {
            let value = jitter.between(Duration::from_secs(10), Duration::from_secs(20));
            assert!((10..=20).contains(&value.as_secs()));
        }
        assert_eq!(
            jitter.between(Duration::from_secs(5), Duration::from_secs(5)),
            Duration::from_secs(5)
        );
        assert_eq!(
            jitter.between(Duration::from_secs(9), Duration::from_secs(5)),
            Duration::from_secs(9)
        );
    }

    #[test]
    fn failure_backoff_is_jittered_grows_and_caps() {
        let start = Instant::now();
        let mut cadence = new_cadence();

        // First failure: uniform in [REFRESH, REFRESH*3].
        cadence.settle(&Err("boom".into()), start, 0, false);
        let first = cadence.wait(start);
        assert!(first >= REFRESH_INTERVAL && first <= REFRESH_INTERVAL * 3);

        // Second failure: the ceiling triples the previous draw, capped.
        cadence.settle(&Err("boom".into()), start, 0, false);
        let second = cadence.wait(start);
        assert!(second >= REFRESH_INTERVAL && second <= MAX_REFRESH_INTERVAL);

        // A long failure run never exceeds the cap...
        for _ in 0..20 {
            cadence.settle(&Err("boom".into()), start, 0, false);
            assert!(cadence.wait(start) <= MAX_REFRESH_INTERVAL);
        }

        // ...and a success resets the backoff to the base range.
        cadence.settle(&Ok(QuotaFetch::Unavailable), start, 0, false);
        cadence.settle(&Err("boom".into()), start, 0, false);
        assert!(cadence.wait(start) <= REFRESH_INTERVAL * 3);
    }

    #[test]
    fn a_throttle_hint_raises_and_outlives_the_jittered_backoff() {
        let start = Instant::now();
        let mut cadence = new_cadence();
        cadence.settle(
            &Err(FetchError::throttled(
                "429",
                Some(Duration::from_secs(20 * 60)),
            )),
            start,
            0,
            true,
        );
        // The jittered backoff tops out at REFRESH*3 = 15min here, so the
        // provider's 20-minute hint wins.
        assert_eq!(cadence.wait(start), Duration::from_secs(20 * 60));

        // A ludicrous hint is still capped.
        cadence.settle(
            &Err(FetchError::throttled(
                "429",
                Some(Duration::from_secs(6 * 60 * 60)),
            )),
            start,
            0,
            true,
        );
        assert_eq!(cadence.wait(start), MAX_REFRESH_INTERVAL);
    }

    #[test]
    fn each_subscription_keeps_its_own_cadence() {
        let start = Instant::now();
        let mut failing = new_cadence();
        let mut healthy = new_cadence();
        assert!(failing.is_due(start) && healthy.is_due(start));

        // First tick: the failing subscription backs off (at least one idle
        // interval), the healthy one stays on the idle cadence.
        failing.settle(&Err("boom".into()), start, 0, false);
        healthy.settle(&Ok(QuotaFetch::Unavailable), start, 0, false);
        assert!(failing.wait(start) >= REFRESH_INTERVAL);
        let healthy_wait = healthy.wait(start);
        assert!(healthy_wait >= REFRESH_INTERVAL);
        assert!(healthy_wait <= REFRESH_INTERVAL + IDLE_JITTER);

        // Just before the failing subscription's earliest possible retry it
        // is not due, so the healthy subscription's tick does not drag it
        // along.
        assert!(!failing.is_due(start + REFRESH_INTERVAL - Duration::from_secs(1)));
        assert!(healthy.is_due(start + healthy_wait));

        // A later success clears the backoff.
        let recovery = start + failing.wait(start);
        failing.settle(&Ok(QuotaFetch::Unavailable), recovery, 0, false);
        let wait = failing.wait(recovery);
        assert!(wait >= REFRESH_INTERVAL && wait <= REFRESH_INTERVAL + IDLE_JITTER);
    }

    #[test]
    fn an_active_agent_starts_at_the_floor_and_probes_back_when_quiet() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();

        // The baseline-setting fetch counts as movement: the fast cadence
        // holds while the rate is still unknown.
        cadence.settle(&available(61), start, now_epoch, true);
        assert_eq!(cadence.wait(start), KIMI_ACTIVE_INTERVAL);

        // Identical snapshots probe at floor * 2^n, capped at the idle
        // cadence.
        let tick = start + KIMI_ACTIVE_INTERVAL;
        cadence.settle(&available(61), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL * 2);
        cadence.settle(&available(61), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL * 4);
        cadence.settle(&available(61), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), REFRESH_INTERVAL);
        cadence.settle(&available(61), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), REFRESH_INTERVAL);
    }

    #[test]
    fn the_active_cadence_follows_the_measured_consumption_rate() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();

        cadence.settle(&available(61), start, now_epoch, true);

        // 3 points in one minute: 3%/min wants a 20s poll, clamped to the
        // 60s floor.
        let tick = start + Duration::from_secs(60);
        cadence.settle(&available(58), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL);

        // A slow burn — 1 point in 2 minutes — stretches to a measured 2min
        // instead of relying on the unchanged-probe doubling.
        let mut slow = new_cadence();
        slow.settle(&available(61), start, now_epoch, true);
        let tick = start + Duration::from_secs(120);
        slow.settle(&available(60), tick, now_epoch, true);
        assert_eq!(slow.wait(tick), Duration::from_secs(120));

        // Slower than one point per idle interval saturates at the idle
        // cadence.
        let mut drip = new_cadence();
        drip.settle(&available(61), start, now_epoch, true);
        let tick = start + REFRESH_INTERVAL;
        drip.settle(&available(60), tick, now_epoch, true);
        assert_eq!(drip.wait(tick), REFRESH_INTERVAL);
    }

    #[test]
    fn a_reset_drifting_stamp_does_not_count_as_movement() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        cadence.settle(
            &Ok(QuotaFetch::Available(vec![window_at(
                "5h",
                61,
                now_epoch + 5000,
            )])),
            start,
            now_epoch,
            true,
        );
        // Same percent, reset stamp shifted: not consumption. The quiet-pane
        // probe backoff must engage.
        let tick = start + KIMI_ACTIVE_INTERVAL;
        cadence.settle(
            &Ok(QuotaFetch::Available(vec![window_at(
                "5h",
                61,
                now_epoch + 4999,
            )])),
            tick,
            now_epoch,
            true,
        );
        // min(probe 120s, boundary 4999s + jitter) — the probe wins.
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL * 2);
    }

    #[test]
    fn a_refill_restarts_the_rate_estimate() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        cadence.settle(&available(61), start, now_epoch, true);
        let tick = start + Duration::from_secs(60);
        cadence.settle(&available(58), tick, now_epoch, true);
        assert!(cadence.rate_per_min > 1.0);

        // The window refilled (percent rose): that is a reset, not negative
        // consumption. The rate restarts from zero and the cadence falls back
        // to the unknown-rate probe.
        let tick = tick + Duration::from_secs(60);
        cadence.settle(&available(100), tick, now_epoch, true);
        assert_eq!(cadence.rate_per_min, 0.0);
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL);
    }

    #[test]
    fn an_idle_agent_uses_the_jittered_idle_cadence() {
        let start = Instant::now();
        let mut cadence = new_cadence();
        cadence.settle(&available(61), start, 1_700_000_000, false);
        let wait = cadence.wait(start);
        assert!(wait >= REFRESH_INTERVAL && wait <= REFRESH_INTERVAL + IDLE_JITTER);
    }

    #[test]
    fn failure_backoff_wins_over_the_active_cadence() {
        let start = Instant::now();
        let mut cadence = new_cadence();
        cadence.settle(&Err("offline".into()), start, 1_700_000_000, true);
        assert!(cadence.wait(start) >= REFRESH_INTERVAL);

        // A retarget while backing off must not pull the deadline forward:
        // the subscription is already being retried as fast as its errors
        // allow.
        cadence.retarget(true, start);
        assert!(cadence.wait(start) >= REFRESH_INTERVAL);
    }

    #[test]
    fn a_newly_active_agent_pulls_the_next_fetch_forward_once_per_floor() {
        let start = Instant::now();
        let mut cadence = new_cadence();
        cadence.settle(&Ok(QuotaFetch::Unavailable), start, 0, false);
        assert!(cadence.wait(start) >= REFRESH_INTERVAL);

        cadence.retarget(true, start);
        assert_eq!(cadence.wait(start), KIMI_ACTIVE_INTERVAL);

        // No flip, no retarget.
        cadence.retarget(true, start);
        assert_eq!(cadence.wait(start), KIMI_ACTIVE_INTERVAL);

        // A flap inside one floor interval does not re-pull: the pane
        // bouncing in and out of the process snapshot must not pin the
        // cadence at the floor.
        cadence.retarget(false, start);
        cadence.retarget(true, start + Duration::from_secs(10));
        assert_eq!(
            cadence.wait(start + Duration::from_secs(10)),
            Duration::from_secs(50)
        );

        // After a full floor interval a genuine re-activation pulls again.
        let later = start + KIMI_ACTIVE_INTERVAL + Duration::from_secs(10);
        cadence.retarget(false, later);
        cadence.settle(&Ok(QuotaFetch::Unavailable), later, 0, false);
        assert!(cadence.wait(later) >= REFRESH_INTERVAL);
        cadence.retarget(true, later);
        assert_eq!(cadence.wait(later), KIMI_ACTIVE_INTERVAL);
    }

    #[test]
    fn going_idle_earns_one_trailing_fetch_unless_one_just_ran() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;

        // The fetch a minute out already covers the trailing consumption, so
        // parking right after a fetch moves nothing.
        let mut fresh = new_cadence();
        fresh.settle(&Ok(QuotaFetch::Unavailable), start, now_epoch, false);
        fresh.retarget(true, start + Duration::from_secs(10));
        let tick = start + Duration::from_secs(10) + KIMI_ACTIVE_INTERVAL;
        fresh.settle(&available(61), tick, now_epoch, true);
        assert_eq!(fresh.wait(tick), KIMI_ACTIVE_INTERVAL);
        fresh.retarget(false, tick + Duration::from_secs(5));
        assert_eq!(
            fresh.wait(tick + Duration::from_secs(5)),
            KIMI_ACTIVE_INTERVAL - Duration::from_secs(5)
        );

        // A subscription deep into the quiet-probe backoff whose agent parks
        // pulls one trailing fetch onto the floor instead of waiting out the
        // probe interval.
        let mut quiet = new_cadence();
        quiet.settle(&available(61), start, now_epoch, true);
        let t1 = start + KIMI_ACTIVE_INTERVAL;
        quiet.settle(&available(61), t1, now_epoch, true);
        let t2 = t1 + KIMI_ACTIVE_INTERVAL * 2;
        quiet.settle(&available(61), t2, now_epoch, true);
        // Two unchanged fetches in: the probe interval is now four floors.
        assert_eq!(quiet.wait(t2), KIMI_ACTIVE_INTERVAL * 4);
        let park = t2 + KIMI_ACTIVE_INTERVAL;
        quiet.retarget(false, park);
        assert_eq!(quiet.wait(park), KIMI_ACTIVE_INTERVAL);
    }

    #[test]
    fn a_near_reset_pulls_the_next_fetch_forward() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        // The 5h window rolls first; the weekly stamp must not hold the next
        // attempt back to its own, much later, boundary. The boundary pull is
        // jittered by up to BOUNDARY_JITTER.
        cadence.settle(
            &Ok(QuotaFetch::Available(vec![
                window_at("5h", 61, now_epoch + 90),
                window_at("wk", 12, now_epoch + 4 * 24 * 60 * 60),
            ])),
            start,
            now_epoch,
            false,
        );
        let wait = cadence.wait(start);
        assert!(wait >= Duration::from_secs(90));
        assert!(wait <= Duration::from_secs(90) + BOUNDARY_JITTER);
    }

    #[test]
    fn a_missed_reset_retries_on_a_short_leash_then_gives_up() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        let expired = || {
            Ok(QuotaFetch::Available(vec![window_at(
                "5h",
                0,
                now_epoch - 1,
            )]))
        };

        // The stamp says the window should have rolled, but the provider has
        // not rolled it: retry shortly, at most RESET_RETRY_MAX times.
        for attempt in 0..RESET_RETRY_MAX {
            cadence.settle(&expired(), start, now_epoch, false);
            let wait = cadence.wait(start);
            assert!(
                wait >= RESET_RETRY_INTERVAL && wait <= RESET_RETRY_INTERVAL + RESET_RETRY_JITTER,
                "attempt {attempt} should stay on the retry leash, got {wait:?}"
            );
        }
        // Budget exhausted: back to the idle cadence.
        cadence.settle(&expired(), start, now_epoch, false);
        assert!(cadence.wait(start) >= REFRESH_INTERVAL);
    }

    #[test]
    fn a_missed_5h_reset_is_not_shadowed_by_a_future_weekly_stamp() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        let expired_with_weekly = || {
            Ok(QuotaFetch::Available(vec![
                window_at("5h", 0, now_epoch - 1),
                window_at("wk", 80, now_epoch + 400_000),
            ]))
        };

        // The weekly window's far-future stamp must not hide the 5h window's
        // missed roll: the retry leash still applies.
        cadence.settle(&expired_with_weekly(), start, now_epoch, false);
        let wait = cadence.wait(start);
        assert!(
            wait >= RESET_RETRY_INTERVAL && wait <= RESET_RETRY_INTERVAL + RESET_RETRY_JITTER,
            "the weekly stamp must not shadow the missed 5h reset, got {wait:?}"
        );

        // Once the leash is exhausted, the weekly boundary takes over the
        // clamp and the wait relaxes to the idle cadence.
        for _ in 0..RESET_RETRY_MAX {
            cadence.settle(&expired_with_weekly(), start, now_epoch, false);
        }
        assert!(cadence.wait(start) >= REFRESH_INTERVAL);
    }

    #[test]
    fn a_stale_rate_decays_while_an_active_pane_stays_quiet() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        cadence.settle(&available(61), start, now_epoch, true);
        // Learn a fast burn: 3 points in one minute pins the floor.
        let tick = start + Duration::from_secs(60);
        cadence.settle(&available(58), tick, now_epoch, true);
        assert_eq!(cadence.wait(tick), KIMI_ACTIVE_INTERVAL);

        // Quiet active polls are zero-rate samples: the estimate decays and
        // the interval stretches back toward the idle cadence.
        for _ in 0..3 {
            cadence.settle(&available(58), tick, now_epoch, true);
        }
        let wait = cadence.wait(tick);
        assert!(
            wait > KIMI_ACTIVE_INTERVAL,
            "a decaying rate must relax the floor, got {wait:?}"
        );
        for _ in 0..10 {
            cadence.settle(&available(58), tick, now_epoch, true);
        }
        assert_eq!(cadence.wait(tick), REFRESH_INTERVAL);
    }

    #[test]
    fn an_imminent_reset_keeps_the_floor() {
        let start = Instant::now();
        let now_epoch = 1_700_000_000u64;
        let mut cadence = new_cadence();
        cadence.settle(
            &Ok(QuotaFetch::Available(vec![window_at(
                "5h",
                0,
                now_epoch + 5,
            )])),
            start,
            now_epoch,
            false,
        );
        let wait = cadence.wait(start);
        assert!(wait >= MIN_RESET_INTERVAL);
        assert!(wait <= MIN_RESET_INTERVAL + BOUNDARY_JITTER);
    }

    #[test]
    fn unavailable_clears_but_failure_keeps_last_snapshot() {
        let mut state = QuotaState::default();
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 61)])),
        );
        assert_eq!(state.subscription_count(), 1);

        // A failure with a snapshot preserves it, marks it stale, and records
        // the failure kind for the age marker's tag.
        state.apply(Subscription::Kimi, Err("boom".into()));
        let quota = state.get(Subscription::Kimi).unwrap();
        assert!(quota.is_stale());
        assert_eq!(quota.windows, vec![window("5h", 61)]);
        assert_eq!(quota.last_error_kind, Some(FetchKind::Other));

        // A later failure overwrites the kind with the newest one.
        state.apply(
            Subscription::Kimi,
            Err(FetchError::throttled("429", Some(Duration::from_secs(30)))),
        );
        assert_eq!(
            state.get(Subscription::Kimi).unwrap().last_error_kind,
            Some(FetchKind::Throttled)
        );

        // A later success clears the stale flag and the recorded kind.
        state.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 42)])),
        );
        let quota = state.get(Subscription::Kimi).unwrap();
        assert!(!quota.is_stale());
        assert_eq!(quota.last_error_kind, None);

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
    fn elapsed_marker_uses_two_units_and_stays_bounded() {
        assert_eq!(format_elapsed(Duration::from_secs(30)), "now");
        assert_eq!(format_elapsed(Duration::from_secs(45 * 60)), "45m");
        assert_eq!(format_elapsed(Duration::from_secs(2 * 3600)), "2h");
        assert_eq!(
            format_elapsed(Duration::from_secs(2 * 3600 + 13 * 60)),
            "2h13m"
        );
        // The longest form the marker can take: six columns.
        assert_eq!(
            format_elapsed(Duration::from_secs(23 * 3600 + 59 * 60)),
            "23h59m"
        );
        assert_eq!(
            format_elapsed(Duration::from_secs(2 * 24 * 3600 + 5 * 3600)),
            "2d5h"
        );
        assert_eq!(format_elapsed(Duration::from_secs(4 * 24 * 3600)), "4d");
    }

    #[test]
    fn poll_loop_reports_both_subscriptions_on_the_first_tick() {
        let (tx, rx) = std::sync::mpsc::channel::<QuotaFetchResult>();
        // `force` starts `true`, so the loop sends its first result before any
        // cadence wait.
        let force = AtomicBool::new(true);
        let activity = QuotaAgentActivity::default();
        let handle = std::thread::spawn(move || {
            quota_poll_loop(
                &tx,
                &force,
                &activity,
                || Ok(QuotaFetch::Unavailable),
                || Ok(QuotaFetch::Available(vec![window("5h", 24)])),
            );
        });
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("quota poll loop should report immediately");
        assert!(result.forced);
        assert!(matches!(result.codex, Some(Ok(QuotaFetch::Unavailable))));
        assert!(matches!(result.kimi, Some(Ok(QuotaFetch::Available(_)))));

        // Let the loop fall into its cadence wait, then drop the receiver so
        // the next send fails and the thread exits on its own. The detached
        // thread is harmless: `force` stays false, so its only other action is
        // a sleep before the failed send.
        drop(rx);
        drop(handle);
    }
}
