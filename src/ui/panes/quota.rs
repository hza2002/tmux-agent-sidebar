//! Bottom-anchored quota block for the agents panel.
//!
//! The block renders only in the rows the agent list does not use (`spare`),
//! so the list stays first-class and scrolls exactly as it did before the
//! feature. It collapses full → compact → hidden as space runs out.
//!
//! Display follows the native Codex status line: numbers, not bars. One window
//! group per subscription keeps every subscription on a single line, and the
//! numbers are colored on a battery-style scale so the window that is actually
//! running out is the one that draws the eye.
//!
//! ```text
//!  Quota
//!  codex 5h   3% 2h51m wk  85% 6d21h
//!  kimi  5h 100% 3h2m wk   9% 4d15h
//! ```
//!
//! Width budget for the worst realistic row (35 cells, the default sidebar):
//! indent 1 + name 5 + gap 1 + [`WINDOW_WIDTH`] 2 + gap 1 + [`PERCENT_WIDTH`]
//! 4 + gap 1 + countdown 5 (a 5-hour window cannot exceed `4h59m`) + separator
//! 1 + window 2 + gap 1 + percent 4 + gap 1 + countdown 6 (`23h59m`, the
//! weekly window's last day). A stale row drops both countdowns and appends
//! the age marker with its failure tag instead: the fixed fields take 22
//! cells and the longest marker is ` ·23h59m 限流` (13 cells — the two Han
//! glyphs are double-width), landing on the same 35-cell budget. A
//! first-fetch-failure placeholder has the same shape: `--` in the value
//! columns instead of percentages, same 22 fixed cells, same marker budget.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::quota::{FetchKind, Subscription, SubscriptionQuota, format_countdown};
use crate::state::{AppState, UsageAction, UsageClickTarget};
use crate::ui::text::{display_width, truncate_to_width};
#[cfg(test)]
use crate::usage::Window;
use crate::usage::{UsageRow, format_cny, format_tokens};

/// Leading space keeps the quota block visually aligned with the agent rows
/// (which start with the `┃` marker column).
const INDENT: u16 = 1;
/// Width of the `codex` / `kimi` subscription-name column.
const NAME_WIDTH: usize = 5;
/// Width of the `5h` / `wk` window column.
const WINDOW_WIDTH: usize = 2;
/// Width of the right-aligned percentage field (`100%` or ` 61%`).
const PERCENT_WIDTH: usize = 4;
/// The 5-hour countdown is padded to its widest value so the weekly group
/// starts on the same column in every row. Without this, a `41m` on one row
/// shifts everything after it left and the two rows stop lining up. The weekly
/// countdown ends its row, so padding it would only cost width the block still
/// has to tolerate.
const FIVE_HOUR_COUNTDOWN_WIDTH: usize = 5; // `4h59m`
/// Longest countdown a weekly window can show (`23h59m`), used for the width
/// budget only, which is also why it is test-only.
#[cfg(test)]
const WEEKLY_COUNTDOWN_WIDTH: usize = 6;
/// Separates the 5-hour and weekly window groups on a subscription's row.
/// A single cell is all the width budget allows once both countdowns are on
/// the row; the bold percentages do the visual grouping.
const WINDOW_SEPARATOR: &str = " ";
const TITLE: &str = "Quota";
const USAGE_TITLE: &str = " Usage"; // nf-fa-chart_line
const CACHE_ICON: &str = ""; // nf-fa-database
const PEAK_ICON: &str = ""; // nf-fa-fire, U+F06D
const OFF_PEAK_ICON: &str = ""; // nf-fa-leaf, U+F06C
// Fixed daily USD-equivalent bands, calibrated once, not a live percentile.
const CODEX_USE_LOW_CENTS: u64 = 2_500;
const CODEX_USE_MEDIUM_CENTS: u64 = 7_500;
const CODEX_USE_HIGH_CENTS: u64 = 15_000;
/// Stand-in for a value the first fetch has not delivered yet.
const PENDING_VALUE: &str = "--";

/// Battery-style scale boundaries, from the top down: `>= 80%` healthy,
/// `60–79%` good, `40–59%` warn, `20–39%` low, `< 20%` critical. The numbers
/// stay readable as numbers and only the color carries the urgency.
const HEALTHY_AT: u8 = 80;
const GOOD_AT: u8 = 60;
const WARN_AT: u8 = 40;
const LOW_AT: u8 = 20;

/// Width of the token column on the spend row, right-aligned so the period
/// label lands on the same column for every value.
const USAGE_TOKEN_WIDTH: usize = 6;

/// Daily spend ladder, in cents of a RMB *day*: `¥5` or less healthy, `¥15`
/// warn, `¥30` low, above that critical. The range windows compare their
/// per-day average against the same numbers, so `7 days = ¥35` reads healthy
/// instead of always painting red. Kept next to the battery scale above so the
/// two ladders are visibly different measurements: spend looks bad when it is
/// high, remaining quota looks bad when it is low.
const SPEND_HEALTHY_CENTS: u64 = 500;
const SPEND_WARN_CENTS: u64 = 1_500;
const SPEND_LOW_CENTS: u64 = 3_000;

/// Width of the widest subscription row, derived from the field widths so the
/// budget cannot drift silently. `23h59m` is the longest countdown a weekly
/// window can show; a 5-hour window tops out at `4h59m`.
#[cfg(test)]
fn subscription_row_width() -> usize {
    let group = |countdown: usize| WINDOW_WIDTH + 1 + PERCENT_WIDTH + 1 + countdown;
    INDENT as usize
        + NAME_WIDTH
        + 1
        + group(FIVE_HOUR_COUNTDOWN_WIDTH)
        + display_width(WINDOW_SEPARATOR)
        + group(WEEKLY_COUNTDOWN_WIDTH)
}

/// Widest subscription row the full block can emit — the 35-column budget the
/// block is designed around. See [`usage_row_width`] for the spend row budget.
#[cfg(test)]
pub fn full_row_width() -> usize {
    subscription_row_width()
}

/// Budget for the spend row: indent, name column, amount (`¥1,234.56`),
/// tokens (`999.9M`) and the cache ratio (` 100%`).
/// The one-cell cache icon leaves room even at the largest formatted values.
#[cfg(test)]
pub fn usage_row_width() -> usize {
    INDENT as usize
        + display_width("ds当天")
        + 1
        + 9
        + 1
        + USAGE_TOKEN_WIDTH
        + 1
        + display_width(" 100%")
}

/// Palette slot for one field of a quota row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaSpanKind {
    /// The `Quota` section header.
    Header,
    UsageHeader,
    /// Subscription name, painted in that agent's identity color.
    Name(Subscription),
    /// Static metadata: window label, separator.
    Label,
    /// Remaining percentage, colored on the battery scale.
    Percent(u8),
    /// Placeholder for a value the first fetch has not delivered yet.
    Pending,
    /// Reset countdown, or the staleness marker that replaces it.
    Countdown,
    /// Live quota reset countdown, brighter than stale-snapshot metadata.
    ResetCountdown,
    /// Spend row: the DeepSeek name and explicit time window.
    UsageName,
    /// Spend row: the window's cost, on the daily spend ladder.
    UsageCost {
        cents: u64,
        unpriced: bool,
    },
    /// Spend row: total tokens for the window.
    UsageTokens,
    /// Spend row: share of input tokens that hit DeepSeek's cache.
    UsageCache,
    /// Current DeepSeek price period, independent of the historical window.
    UsagePeriod {
        peak: bool,
    },
    /// Spend row before its first result arrives.
    UsagePending,
    CodexCycle,
    CodexRecent,
    CodexEstimate {
        cents: u64,
        days: i64,
        unpriced: bool,
    },
}

/// One styled field of a quota row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaSpan {
    pub text: String,
    pub kind: QuotaSpanKind,
}

impl QuotaSpan {
    fn label(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: QuotaSpanKind::Label,
        }
    }

    fn name(subscription: Subscription) -> Self {
        Self {
            text: format!("{:<NAME_WIDTH$}", subscription.label()),
            kind: QuotaSpanKind::Name(subscription),
        }
    }

    fn countdown(text: String) -> Self {
        Self {
            text,
            kind: QuotaSpanKind::Countdown,
        }
    }

    /// Placeholder occupying `width` cells, so a pending row lines up with the
    /// rows that replace it.
    fn pending(width: usize) -> Self {
        Self {
            text: format!("{PENDING_VALUE:>width$}"),
            kind: QuotaSpanKind::Pending,
        }
    }
}

/// A rendered quota row. Public so tests can assert the view model without
/// rendering a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaLine {
    pub spans: Vec<QuotaSpan>,
    pub stale: bool,
}

impl QuotaLine {
    /// The row as the terminal shows it, with every field concatenated.
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// Collapse level for the quota block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotaLevel {
    Full,
    Compact,
    CompactCycle,
    Hidden,
    Cycle,
    QuotaOnly,
    QuotaCompact,
}

fn percent_span(remaining_percent: u8) -> QuotaSpan {
    let digits = remaining_percent.to_string();
    QuotaSpan {
        text: format!(
            "{}{digits}%",
            " ".repeat(PERCENT_WIDTH.saturating_sub(digits.len() + 1))
        ),
        kind: QuotaSpanKind::Percent(remaining_percent),
    }
}

/// One row per subscription before the first result arrives, so the block has
/// its final shape from the first frame. The placeholder occupies the value
/// column and, at the full level, the countdown column too.
///
/// `marker` is the `age kind` suffix (`1m 网络`) of a first-fetch failure:
/// the row then drops the countdown placeholders (they would claim data the
/// fetch never delivered) and dims like a stale row. Pending rows pass
/// `None` and keep their plain, undimmed shape.
fn placeholder_row(
    subscription: Subscription,
    with_countdown: bool,
    marker: Option<String>,
) -> QuotaLine {
    let failed = marker.is_some();
    let mut spans = vec![
        QuotaSpan::label(" ".repeat(INDENT as usize)),
        QuotaSpan::name(subscription),
        QuotaSpan::label(" "),
    ];
    let mut first_window = true;
    for window_label in ["5h", "wk"] {
        if !first_window {
            spans.push(QuotaSpan::label(WINDOW_SEPARATOR));
        }
        first_window = false;
        spans.push(QuotaSpan::label(format!("{window_label:>WINDOW_WIDTH$} ")));
        spans.push(QuotaSpan::pending(PERCENT_WIDTH));
        if with_countdown && !failed {
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan::pending(PENDING_VALUE.len()));
        }
    }
    if let Some(marker) = marker {
        spans.push(QuotaSpan::countdown(format!(" ·{marker}")));
    }
    QuotaLine {
        spans,
        stale: failed,
    }
}

/// `codex 5h   3% 2h51m wk  85% 6d21h`.
///
/// One subscription per row: the name column, then each window it actually has
/// as a `label percentage countdown` group. A window the API omitted is skipped
/// instead of leaving a hole, so a weekly-only subscription still renders and
/// stays inside the width budget.
///
/// `with_countdown` is `false` at the compact level, where the row keeps both
/// percentages and drops the reset times to stay narrow.
fn subscription_row(
    subscription: Subscription,
    quota: &SubscriptionQuota,
    now: u64,
    with_countdown: bool,
) -> QuotaLine {
    let mut spans = vec![
        QuotaSpan::label(" ".repeat(INDENT as usize)),
        QuotaSpan::name(subscription),
        QuotaSpan::label(" "),
    ];
    // A stale snapshot keeps the last good percentages but drops the reset
    // countdowns: they are no longer trustworthy, and the snapshot age below
    // is what tells the user the row is old.
    let with_countdown = with_countdown && !quota.is_stale();
    let mut first_window = true;
    for window_label in ["5h", "wk"] {
        let Some(window) = quota.windows.iter().find(|w| w.label == window_label) else {
            continue;
        };
        if !first_window {
            spans.push(QuotaSpan::label(WINDOW_SEPARATOR));
        }
        first_window = false;
        // Left pad so `5h` and `wk` end on the same column.
        spans.push(QuotaSpan::label(format!("{window_label:>WINDOW_WIDTH$} ")));
        spans.push(percent_span(window.remaining_percent));
        if with_countdown && let Some(countdown) = format_countdown(now, window.resets_at) {
            let text = if window_label == "5h" {
                format!("{countdown:>FIVE_HOUR_COUNTDOWN_WIDTH$}")
            } else {
                countdown
            };
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text,
                kind: QuotaSpanKind::ResetCountdown,
            });
        }
    }
    if let Some(age) = quota.age_marker() {
        // The failure reason rides the age marker as a two-character tag
        // (`·45m 限流`), so the dimmed row says why it is stale, not just how
        // old. A snapshot that predates kind tracking renders the generic
        // `错误`.
        let kind = quota.last_error_kind.unwrap_or(FetchKind::Other);
        spans.push(QuotaSpan::countdown(format!(" ·{age} {}", kind.label())));
    }
    QuotaLine {
        spans,
        stale: quota.is_stale(),
    }
}

/// ` ds当天 ¥1.91 14.1M`.
///
/// Full views place the current period in the header. Title-less compact views
/// keep the signal on this row regardless of the selected historical window.
fn usage_line(state: &AppState, level: QuotaLevel) -> QuotaLine {
    let usage = &state.usage;
    let mut spans = vec![
        QuotaSpan::label(" ".repeat(INDENT as usize)),
        QuotaSpan {
            text: format!(
                "{}{}",
                usage.selected.name(),
                " ".repeat(
                    usage_name_width(state).saturating_sub(display_width(usage.selected.name()))
                )
            ),
            kind: QuotaSpanKind::UsageName,
        },
        QuotaSpan::label(" "),
    ];
    match &usage.snapshot {
        Some(spend) => {
            let amount = usage_amount(state, UsageRow::DeepSeek).unwrap();
            spans.push(QuotaSpan {
                text: pad_amount(state, &amount),
                kind: QuotaSpanKind::UsageCost {
                    cents: (spend.cost_cny * 100.0).round().max(0.0) as u64,
                    unpriced: spend.unpriced,
                },
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: format!(
                    "{:>USAGE_TOKEN_WIDTH$}",
                    format_tokens(spend.tokens.total())
                ),
                kind: QuotaSpanKind::UsageTokens,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: cache_ratio(spend.tokens),
                kind: QuotaSpanKind::UsageCache,
            });
        }
        None => {
            spans.push(QuotaSpan {
                text: pad_amount(state, "¥--"),
                kind: QuotaSpanKind::UsagePending,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: format!("{:>USAGE_TOKEN_WIDTH$}", "--"),
                kind: QuotaSpanKind::UsagePending,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: format!("{CACHE_ICON} --"),
                kind: QuotaSpanKind::UsagePending,
            });
        }
    }
    if matches!(level, QuotaLevel::Compact | QuotaLevel::CompactCycle) {
        spans.push(QuotaSpan::label(" "));
        spans.push(current_period(state, false));
    }
    QuotaLine {
        spans,
        stale: false,
    }
}

/// Current price period is independent of the selected historical window.
fn current_period(state: &AppState, provider: bool) -> QuotaSpan {
    let peak = crate::usage::is_peak(state.now as i64);
    QuotaSpan {
        text: format!(
            "{}{}{}",
            if provider { "ds" } else { "" },
            if peak { PEAK_ICON } else { OFF_PEAK_ICON },
            if peak { "高峰" } else { "低谷" }
        ),
        kind: QuotaSpanKind::UsagePeriod { peak },
    }
}

/// Share of input tokens served from cache, shared by all usage providers.
/// `--` when nothing was billed rather than an invented 0%.
fn cache_ratio(tokens: crate::usage::Tokens) -> String {
    let input = tokens.uncached + tokens.cached;
    if input == 0 {
        return format!("{CACHE_ICON} --");
    }
    let percent = ((tokens.cached as f64 / input as f64) * 100.0).round() as u64;
    format!("{CACHE_ICON} {percent}%")
}

/// Spend ladder step for an average daily cost in cents.
fn spend_cents(state: &AppState, cents: u64, days: i64) -> (u64, ratatui::style::Color) {
    let per_day = cents / days.max(1) as u64;
    let color = match per_day {
        c if c <= SPEND_HEALTHY_CENTS => state.theme.quota_healthy,
        c if c <= SPEND_WARN_CENTS => state.theme.quota_warn,
        c if c <= SPEND_LOW_CENTS => state.theme.quota_low,
        _ => state.theme.quota_critical,
    };
    (per_day, color)
}

/// More API-equivalent work means greater subscription utilization, not spend risk.
fn codex_utilization_color(state: &AppState, cents: u64, days: i64) -> ratatui::style::Color {
    match cents / days.max(1) as u64 {
        n if n <= CODEX_USE_LOW_CENTS => state.theme.text_muted,
        n if n <= CODEX_USE_MEDIUM_CENTS => state.theme.agent_codex,
        n if n <= CODEX_USE_HIGH_CENTS => state.theme.quota_good,
        _ => state.theme.quota_healthy,
    }
}

/// Lines for the current quota state at the requested collapse level.
/// Empty when there is nothing to render.
pub fn lines(state: &AppState, level: QuotaLevel) -> Vec<QuotaLine> {
    if level == QuotaLevel::Hidden {
        return Vec::new();
    }
    let rendered = state.quota.rendered();
    let show_usage =
        state.usage.received && !matches!(level, QuotaLevel::QuotaOnly | QuotaLevel::QuotaCompact);
    let has_failure_rows = state.quota.failure_count() > 0;
    if rendered.is_empty() && !has_failure_rows && !state.quota.is_pending() && !show_usage {
        return Vec::new();
    }
    let with_countdown = matches!(
        level,
        QuotaLevel::Full | QuotaLevel::Cycle | QuotaLevel::QuotaOnly
    );
    let mut lines = Vec::with_capacity(rendered.len() + 1);
    if with_countdown && (state.quota.expected_count() > 0 || show_usage) {
        let mut spans = vec![QuotaSpan {
            text: if show_usage {
                state.usage.shared_window().map_or_else(
                    || format!(" {USAGE_TITLE}"),
                    |window| format!(" {USAGE_TITLE} {}", window.recent().label()),
                )
            } else {
                format!(" {TITLE}")
            },
            kind: if show_usage {
                QuotaSpanKind::UsageHeader
            } else {
                QuotaSpanKind::Header
            },
        }];
        if show_usage {
            spans.push(QuotaSpan::label(" "));
            spans.push(current_period(state, true));
        }
        lines.push(QuotaLine {
            spans,
            stale: false,
        });
    }
    if rendered.is_empty() && state.quota.is_pending() {
        // No snapshot yet: reserve the rows with placeholders so the block has
        // its final shape from the first frame instead of appearing seconds
        // later, once the first fetch comes back.
        lines.extend(
            [Subscription::Codex, Subscription::Kimi]
                .into_iter()
                .filter(|&subscription| state.quota.enabled(subscription))
                .map(|subscription| placeholder_row(subscription, with_countdown, None)),
        );
    } else {
        for subscription in [Subscription::Codex, Subscription::Kimi] {
            if let Some(quota) = state.quota.get(subscription) {
                lines.push(subscription_row(
                    subscription,
                    quota,
                    state.now,
                    with_countdown,
                ));
            } else if let Some(failure) = state.quota.failure(subscription) {
                // A subscription whose first fetch failed must not vanish:
                // it gets a dimmed placeholder with the failure's age and
                // kind, at both the full and the compact level.
                lines.push(placeholder_row(
                    subscription,
                    with_countdown,
                    Some(failure.marker()),
                ));
            }
        }
    }
    if show_usage {
        if state.usage.codex.is_some() {
            lines.push(codex_line(state, true));
            if matches!(level, QuotaLevel::Full | QuotaLevel::Compact) {
                lines.push(codex_line(state, false));
            }
        }
        lines.push(usage_line(state, level));
    }
    lines
}

/// Spend spare columns on reset times, then clearer names, without clipping numbers.
pub fn lines_for_width(state: &AppState, level: QuotaLevel, width: u16) -> Vec<QuotaLine> {
    let mut rows = lines(state, level);
    let width = width as usize;
    for row in &mut rows {
        // Preserve the failure reason before its age on narrow sidebars.
        if row.stale && display_width(&row.text()) > width {
            for span in &mut row.spans {
                if span.kind == QuotaSpanKind::Countdown
                    && let Some(reason) = span.text.split_whitespace().last()
                {
                    span.text = format!(" ·{reason}");
                }
            }
        }
        let subscription = row.spans.iter().find_map(|span| match span.kind {
            QuotaSpanKind::Name(subscription) => Some(subscription),
            _ => None,
        });
        if let Some(subscription) = subscription
            && let Some(quota) = state.quota.get(subscription).filter(|q| !q.is_stale())
        {
            let mut richer = subscription_row(subscription, quota, state.now, true);
            // Keep the short-window reset first when both countdowns do not fit.
            while display_width(&richer.text()) > width {
                let Some(index) = richer
                    .spans
                    .iter()
                    .rposition(|s| s.kind == QuotaSpanKind::ResetCountdown)
                else {
                    break;
                };
                if index == 0 || richer.spans[index - 1].text != " " {
                    break;
                }
                richer.spans.drain(index - 1..=index);
            }
            if display_width(&richer.text()) <= width {
                *row = richer;
            }
        }
    }
    let is_usage_name = |kind| {
        matches!(
            kind,
            QuotaSpanKind::UsageName | QuotaSpanKind::CodexCycle | QuotaSpanKind::CodexRecent
        )
    };
    let can_expand = rows
        .iter()
        .any(|r| r.spans.iter().any(|s| s.kind == QuotaSpanKind::CodexCycle))
        && rows
            .iter()
            .filter(|r| r.spans.iter().any(|s| is_usage_name(s.kind)))
            .all(|r| display_width(&r.text()) < width);
    if can_expand {
        for row in &mut rows {
            for span in row.spans.iter_mut().filter(|s| is_usage_name(s.kind)) {
                let name = if span.kind == QuotaSpanKind::CodexCycle {
                    "cx本期"
                } else {
                    span.text.trim_end()
                };
                span.text = format!(
                    "{name}{}",
                    " ".repeat(
                        (usage_name_width(state).max(NAME_WIDTH + 1))
                            .saturating_sub(display_width(name))
                    )
                );
            }
        }
    }
    // Keep explicit window labels and complete numbers ahead of optional metadata.
    for row in &mut rows {
        if !row.spans.iter().any(|s| is_usage_name(s.kind)) {
            continue;
        }
        if display_width(&row.text()) > width {
            if let Some(period) = row
                .spans
                .iter_mut()
                .find(|s| matches!(s.kind, QuotaSpanKind::UsagePeriod { .. }))
            {
                period.text = period
                    .text
                    .trim_end_matches(['高', '峰', '低', '谷'])
                    .to_string();
            }
            if display_width(&row.text()) > width
                && row.spans.first().is_some_and(|s| s.text == " ")
            {
                row.spans.remove(0);
            }
            if display_width(&row.text()) > width
                && let Some(cache) = row
                    .spans
                    .iter_mut()
                    .find(|s| s.kind == QuotaSpanKind::UsageCache)
            {
                cache.text = cache.text.replace(' ', "");
            }
            if display_width(&row.text()) > width
                && let Some(index) = row
                    .spans
                    .iter()
                    .position(|s| s.kind == QuotaSpanKind::UsageCache)
            {
                row.spans.drain(index - 1..=index);
            }
        }
    }
    // The current DS period is right-aligned metadata, outside the historical window.
    if let Some(header) = rows
        .iter_mut()
        .find(|r| r.spans.iter().any(|s| s.kind == QuotaSpanKind::UsageHeader))
    {
        if display_width(&header.text()) > width
            && let Some(period) = header.spans.last_mut()
        {
            period.text = period
                .text
                .trim_end_matches(['高', '峰', '低', '谷'])
                .to_string();
        }
        let gap = width.saturating_sub(display_width(&header.text()));
        if let Some(separator) = header.spans.get_mut(1) {
            separator.text.push_str(&" ".repeat(gap));
        }
    }
    rows
}

fn usage_amount(state: &AppState, row: UsageRow) -> Option<String> {
    let daily = state.usage.show_daily_amount(row, state.now);
    let days = if daily {
        state.usage.amount_days(row, state.now)
    } else {
        1
    };
    let amount = if row == UsageRow::DeepSeek {
        let spend = state.usage.snapshot.as_ref()?;
        let amount = format_cny(spend.cost_cny / days as f64);
        if spend.unpriced {
            format!("{amount}?")
        } else {
            amount
        }
    } else {
        let summary = state.usage.summary(row, state.now)?;
        if daily {
            crate::usage::metering::Total {
                cost: summary.total.cost / days as u128,
                ..Default::default()
            }
            .amount()
        } else {
            summary.total.amount()
        }
    };
    Some(if daily { format!("{amount}/d") } else { amount })
}

// Both amount modes share alignment; display mode never changes token totals or colors.
fn pad_amount(state: &AppState, amount: &str) -> String {
    let width = [
        UsageRow::DeepSeek,
        UsageRow::CodexCycle,
        UsageRow::CodexRecent,
    ]
    .into_iter()
    .filter_map(|row| usage_amount(state, row))
    .map(|s| display_width(&s))
    .max()
    .unwrap_or(0);
    format!(
        "{amount}{}",
        " ".repeat(width.saturating_sub(display_width(amount)))
    )
}

fn usage_name_width(state: &AppState) -> usize {
    NAME_WIDTH
        .max(display_width(state.usage.selected.name()))
        .max(
            if state.usage.recent == crate::usage::metering::Recent::Today
                && state.usage.codex.is_some()
            {
                6
            } else {
                0
            },
        )
}

fn codex_line(state: &AppState, cycle: bool) -> QuotaLine {
    let snapshot = state.usage.codex.as_ref().unwrap();
    let summary = if cycle {
        state.usage.cycle_summary(state.now)
    } else {
        Some(&snapshot.recent[state.usage.recent.index()])
    };
    // Compact names keep the quota cycle distinct from elapsed rolling windows.
    let name = if cycle {
        "cx周".to_string()
    } else {
        format!("cx{}", state.usage.recent.label())
    };
    let mut spans = vec![
        QuotaSpan::label(" "),
        QuotaSpan {
            text: format!(
                "{name}{}",
                " ".repeat(usage_name_width(state).saturating_sub(display_width(&name)))
            ),
            kind: if cycle {
                QuotaSpanKind::CodexCycle
            } else {
                QuotaSpanKind::CodexRecent
            },
        },
        QuotaSpan::label(" "),
    ];
    if let Some(s) = summary {
        spans.push(QuotaSpan {
            text: pad_amount(
                state,
                &usage_amount(
                    state,
                    if cycle {
                        UsageRow::CodexCycle
                    } else {
                        UsageRow::CodexRecent
                    },
                )
                .unwrap(),
            ),
            kind: QuotaSpanKind::CodexEstimate {
                cents: ((s.total.cost + 5_000_000_000) / 10_000_000_000).min(u64::MAX as u128)
                    as u64,
                days: (s.end.saturating_sub(s.start).max(1) + 86_399) / 86_400,
                unpriced: s.total.unpriced > 0,
            },
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: format!(
                "{:>USAGE_TOKEN_WIDTH$}",
                format_tokens(s.total.tokens.total())
            ),
            kind: QuotaSpanKind::UsageTokens,
        });
        spans.push(QuotaSpan::label(" "));
        let cache = cache_ratio(s.total.tokens);
        spans.push(QuotaSpan {
            text: cache,
            kind: QuotaSpanKind::UsageCache,
        });
        if snapshot.incomplete {
            spans.push(QuotaSpan::label(" !"));
        }
    } else {
        spans.push(QuotaSpan {
            text: "本期待刷新".into(),
            kind: QuotaSpanKind::UsagePending,
        });
    }
    QuotaLine {
        spans,
        stale: state.now.saturating_sub(snapshot.observed_at as u64) > 90,
    }
}

/// Style one field. Percentages carry the signal; names carry the
/// subscription's identity so the two blocks are told apart at a glance, and
/// everything else stays quiet.
fn span_style(state: &AppState, kind: QuotaSpanKind) -> Style {
    match kind {
        QuotaSpanKind::CodexCycle | QuotaSpanKind::CodexRecent => {
            Style::default().fg(state.theme.agent_codex)
        }
        QuotaSpanKind::CodexEstimate {
            cents,
            days,
            unpriced,
        } => Style::default()
            .fg(if unpriced {
                state.theme.text_muted
            } else {
                codex_utilization_color(state, cents, days)
            })
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::Header | QuotaSpanKind::UsageHeader => {
            Style::default().fg(state.theme.section_title)
        }
        QuotaSpanKind::Name(Subscription::Codex) => Style::default().fg(state.theme.agent_codex),
        QuotaSpanKind::Name(Subscription::Kimi) => Style::default().fg(state.theme.agent_kimi),
        QuotaSpanKind::Label => Style::default().fg(state.theme.text_muted),
        QuotaSpanKind::Percent(remaining) => Style::default()
            .fg(quota_color(state, remaining))
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::Pending => Style::default().fg(state.theme.text_inactive),
        QuotaSpanKind::Countdown => Style::default().fg(state.theme.text_inactive),
        QuotaSpanKind::ResetCountdown => Style::default().fg(state.theme.text_active),
        QuotaSpanKind::UsageName => Style::default().fg(state.theme.agent_deepseek),
        QuotaSpanKind::UsageCost { cents, unpriced } => {
            if unpriced {
                // Painting a colour would claim a certainty the number does not
                // have: the `?` in the text already says "lower bound".
                return Style::default().fg(state.theme.text_muted);
            }
            let (_, color) = spend_cents(state, cents, state.usage.selected.days());
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        }
        // Every field owns a hue, but only the amount's colour changes with
        // its value: the other three are fixed identities, so a shifting
        // colour on the row always means "money", never "volume" or "price
        // window". All four are `@sidebar_color_quota_spend_*` overridable.
        QuotaSpanKind::UsageTokens => Style::default().fg(state.theme.quota_spend_tokens),
        QuotaSpanKind::UsageCache => Style::default().fg(state.theme.quota_spend_cache),
        // Peak also gets weight: colour alone would tie with the amount's
        // ladder, and the period is the one field whose meaning flips.
        QuotaSpanKind::UsagePeriod { peak } => Style::default()
            .fg(if peak {
                state.theme.quota_spend_peak
            } else {
                state.theme.quota_spend_off_peak
            })
            .add_modifier(if peak {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        QuotaSpanKind::UsagePending => Style::default().fg(state.theme.text_inactive),
    }
}

/// Battery-style remaining-quota color: green while there is plenty, then
/// yellow, orange, and red as the window empties out.
fn quota_color(state: &AppState, remaining: u8) -> ratatui::style::Color {
    match remaining {
        remaining if remaining >= HEALTHY_AT => state.theme.quota_healthy,
        remaining if remaining >= GOOD_AT => state.theme.quota_good,
        remaining if remaining >= WARN_AT => state.theme.quota_warn,
        remaining if remaining >= LOW_AT => state.theme.quota_low,
        _ => state.theme.quota_critical,
    }
}

fn dim(style: Style, stale: bool) -> Style {
    if stale {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

/// Styled row. A row too narrow for every field falls back to one plain style
/// over truncated text, so a wide glyph can never be split in half.
fn styled_line<'a>(state: &AppState, line: &'a QuotaLine, width: u16) -> Line<'a> {
    let text = line.text();
    if display_width(&text) > width as usize {
        return Line::from(Span::styled(
            truncate_to_width(&text, width as usize),
            dim(Style::default().fg(state.theme.text_muted), line.stale),
        ));
    }
    Line::from(
        line.spans
            .iter()
            .map(|span| {
                Span::styled(
                    span.text.as_str(),
                    dim(span_style(state, span.kind), line.stale),
                )
            })
            .collect::<Vec<_>>(),
    )
}

/// Final footer rows and field targets, replaced on every frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaRows {
    pub block: (u16, u16),
    pub usage_rows: Vec<u16>,
    pub targets: Vec<UsageClickTarget>,
}

fn field_targets(state: &AppState, line: &QuotaLine, area: Rect) -> Vec<UsageClickTarget> {
    // styled_line uses a plain truncated fallback in this case; expose no invisible control.
    if display_width(&line.text()) > area.width as usize {
        return Vec::new();
    }
    let row = line.spans.iter().find_map(|s| match s.kind {
        QuotaSpanKind::UsageName => Some(UsageRow::DeepSeek),
        QuotaSpanKind::CodexRecent => Some(UsageRow::CodexRecent),
        QuotaSpanKind::CodexCycle => Some(UsageRow::CodexCycle),
        _ => None,
    });
    let mut x = area.x;
    let mut targets = Vec::new();
    for span in &line.spans {
        let action = match span.kind {
            QuotaSpanKind::UsageHeader => Some(UsageAction::CycleAll),
            QuotaSpanKind::UsageName => Some(UsageAction::CycleDeepSeek),
            QuotaSpanKind::CodexRecent => Some(UsageAction::CycleCodexRecent),
            QuotaSpanKind::CodexCycle => Some(UsageAction::OpenCycleDetails),
            QuotaSpanKind::UsageCost { .. } | QuotaSpanKind::CodexEstimate { .. } => row
                .filter(|&row| state.usage.can_toggle_amount(row, state.now))
                .map(UsageAction::ToggleAmount),
            _ => None,
        };
        if let Some(action) = action {
            let leading = &span.text[..span.text.len() - span.text.trim_start().len()];
            let width = display_width(span.text.trim()) as u16;
            if width > 0 {
                targets.push(UsageClickTarget {
                    rect: Rect::new(x + display_width(leading) as u16, area.y, width, 1),
                    action,
                });
            }
        }
        x += display_width(&span.text) as u16;
    }
    targets
}

pub fn render(
    frame: &mut Frame,
    state: &AppState,
    list_area: Rect,
    level: QuotaLevel,
) -> Option<QuotaRows> {
    if level == QuotaLevel::Hidden {
        return None;
    }
    let lines = lines_for_width(state, level, list_area.width);
    if lines.is_empty() {
        return None;
    }
    let height = lines.len() as u16;
    if height > list_area.height {
        return None;
    }
    let area = Rect {
        x: list_area.x,
        y: list_area.bottom().saturating_sub(height),
        width: list_area.width,
        height,
    };
    // The agent list paints into its whole rect, not just the rows it "needs",
    // so the block clears its own rows before drawing: otherwise rows the list
    // painted under a stolen row would show through the padding.
    frame.render_widget(ratatui::widgets::Clear, area);

    let mut targets = Vec::new();
    let mut usage_rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let row = Rect {
            x: area.x,
            y: area.y + index as u16,
            width: area.width,
            height: 1,
        };
        if line.spans.iter().any(|s| {
            matches!(
                s.kind,
                QuotaSpanKind::UsageHeader
                    | QuotaSpanKind::UsageName
                    | QuotaSpanKind::CodexCycle
                    | QuotaSpanKind::CodexRecent
            )
        }) {
            usage_rows.push(row.y);
        }
        targets.extend(field_targets(state, line, row));
        frame.render_widget(Paragraph::new(styled_line(state, line, row.width)), row);
    }
    Some(QuotaRows {
        block: (area.y, area.bottom().saturating_sub(1)),
        usage_rows,
        targets,
    })
}

pub(super) fn render_usage_details(frame: &mut Frame, state: &AppState, area: Rect) {
    use crate::state::PopupState;
    use ratatui::widgets::{Block, Borders, Clear};
    let PopupState::Usage { recent, scroll } = state.popup else {
        return;
    };
    let summary = if recent {
        state
            .usage
            .codex
            .as_ref()
            .map(|s| &s.recent[state.usage.recent.index()])
    } else {
        state.usage.cycle_summary(state.now)
    };
    let mut text = vec![
        "本机记录 · API 等价估值".to_string(),
        "不代表账号全部用量或订阅账单".to_string(),
        format!("价目表 {}", crate::usage::pricing::REVISION),
    ];
    if let Some(s) = summary {
        text.push(format!(
            "{} →",
            crate::usage::metering::format_time(s.start)
        ));
        text.push(format!(
            "{} UTC",
            crate::usage::metering::format_time(s.end)
        ));
        text.push(format!(
            "总计 {} {}",
            format_tokens(s.total.tokens.total()),
            s.total.amount()
        ));
        if s.total.assumed_standard > 0 {
            text.push(format!(
                "{} 条未记录档位，按 Standard 估算",
                s.total.assumed_standard
            ));
        }
        if s.total.unknown_tier > 0 {
            text.push("? 未知服务档位，未套用标准价".into());
        }
        if s.total.unpriced > 0 {
            text.push(format!("? {} 条记录未定价", s.total.unpriced));
        }
        if s.total.missing_price > 0 {
            text.push(format!("? {} 条缺少模型历史价格", s.total.missing_price));
        }
        if s.total.unsupported_cache_write > 0 {
            text.push(format!(
                "? {} 条缓存写入计费不明",
                s.total.unsupported_cache_write
            ));
        }
        if s.total.assumed_price > 0 {
            text.push("历史段按已核实官方价格回填".into());
            text.push("假设核实日前价格未变".into());
        }
        if s.total.unknown_account {
            text.push("部分记录无账号归属".into());
        }
        if s.total.uncertain_identity {
            text.push("部分记录缺少请求去重依据".into());
        }
        for (model, t) in &s.models {
            text.push(model.clone());
            text.push(format!(
                "  {} {}",
                format_tokens(t.tokens.total()),
                t.amount()
            ));
            text.push(format!("  输入 {}", format_tokens(t.tokens.uncached)));
            text.push(format!("  缓存 {}", format_tokens(t.tokens.cached)));
            text.push(format!("  输出 {}", format_tokens(t.tokens.output)));
        }
    } else {
        text.push("周期边界未确认，等待额度刷新".into());
    }
    if state.usage.codex.as_ref().is_some_and(|s| s.incomplete) {
        text.push("! 部分日志无法解析，统计不完整".into());
    }
    text.push("仅含日志可见用量，图片费用未覆盖".into());
    text.push("Tab 本期/近期 · ↑↓ 滚动 · Esc 关闭".into());
    let width = area.width.min(52);
    let mut wrapped = Vec::new();
    for line in text {
        let mut part = String::new();
        let mut cells = 0;
        for ch in line.chars() {
            let size = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if cells + size > width.saturating_sub(2) as usize && !part.is_empty() {
                wrapped.push(std::mem::take(&mut part));
                cells = 0;
            }
            part.push(ch);
            cells += size;
        }
        wrapped.push(part);
    }
    let text = wrapped;
    let height = area.height.min(text.len() as u16 + 2);
    if width < 12 || height < 3 {
        return;
    }
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let title = if recent {
        format!(" Usage {} ", state.usage.recent.label())
    } else {
        " Usage 本期 ".into()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(state.theme.border_inactive));
    let inner = block.inner(rect);
    let scroll = scroll.min((text.len() as u16).saturating_sub(inner.height));
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let rows: Vec<Line> = text
        .iter()
        .skip(scroll as usize)
        .take(inner.height as usize)
        .map(|s| Line::from(truncate_to_width(s, inner.width as usize)))
        .collect();
    frame.render_widget(
        Paragraph::new(rows).style(Style::default().fg(state.theme.text_muted)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{QuotaFetch, QuotaWindow};
    use crate::state::AppState;
    use ratatui::style::Color;

    fn window(label: &str, remaining_percent: u8, resets_at: Option<u64>) -> QuotaWindow {
        QuotaWindow {
            label: label.into(),
            remaining_percent,
            resets_at,
        }
    }

    fn state_with_kimi() -> AppState {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![
                window("5h", 61, Some(state.now + 133 * 60)),
                window("wk", 83, Some(state.now + 4 * 24 * 60 * 60)),
            ])),
        );
        state
    }

    #[test]
    fn percentages_are_right_aligned_within_the_field() {
        assert_eq!(percent_span(3).text, "  3%");
        assert_eq!(percent_span(61).text, " 61%");
        assert_eq!(percent_span(100).text, "100%");
        assert_eq!(display_width(&percent_span(100).text), PERCENT_WIDTH);
    }

    #[test]
    fn percentages_follow_a_battery_scale() {
        let state = state_with_kimi();
        let color = |remaining| quota_color(&state, remaining);
        assert_eq!(color(100), state.theme.quota_healthy);
        assert_eq!(color(80), state.theme.quota_healthy);
        assert_eq!(color(79), state.theme.quota_good);
        assert_eq!(color(60), state.theme.quota_good);
        assert_eq!(color(59), state.theme.quota_warn);
        assert_eq!(color(40), state.theme.quota_warn);
        assert_eq!(color(39), state.theme.quota_low);
        assert_eq!(color(20), state.theme.quota_low);
        assert_eq!(color(19), state.theme.quota_critical);
        assert_eq!(color(0), state.theme.quota_critical);

        // The steps are the gruvbox green → yellow → orange → red ramp, and
        // each step is its own color rather than a repeat.
        assert_eq!(state.theme.quota_healthy, Color::Rgb(0xb8, 0xbb, 0x26));
        assert_eq!(state.theme.quota_good, Color::Rgb(0x8e, 0xc0, 0x7c));
        assert_eq!(state.theme.quota_warn, Color::Rgb(0xfa, 0xbd, 0x2f));
        assert_eq!(state.theme.quota_low, Color::Rgb(0xe7, 0x8a, 0x4e));
        assert_eq!(state.theme.quota_critical, Color::Rgb(0xfb, 0x49, 0x34));

        // The number itself is what stands out from the row's other fields.
        assert!(
            span_style(&state, QuotaSpanKind::Percent(5))
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn field_slots_come_from_the_theme() {
        let state = state_with_kimi();
        assert_eq!(
            span_style(&state, QuotaSpanKind::Header).fg,
            Some(state.theme.section_title)
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::Label).fg,
            Some(state.theme.text_muted)
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::Countdown).fg,
            Some(state.theme.text_inactive)
        );
        // Subscriptions are told apart by the same identity colors the agent
        // list uses for them.
        assert_eq!(
            span_style(&state, QuotaSpanKind::Name(Subscription::Codex)).fg,
            Some(state.theme.agent_codex)
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::Name(Subscription::Kimi)).fg,
            Some(state.theme.agent_kimi)
        );
    }

    #[test]
    fn full_lines_render_the_header_and_one_row_per_subscription() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].text(), " Quota");
        // One subscription per row: both windows, each with its percentage and
        // reset countdown, inside the default 35-column sidebar.
        assert_eq!(rendered[1].text(), " kimi  5h  61% 2h13m wk  83% 4d");
        assert!(
            display_width(&rendered[1].text()) <= 35,
            "a full row has to fit the default sidebar width: {}",
            rendered[1].text()
        );
    }

    #[test]
    fn worst_case_row_still_fits_the_default_sidebar() {
        // The 5-hour window can never be more than five hours away, and the
        // weekly window's `23h59m` is the longest countdown either window can
        // produce, so this is the widest row the renderer can emit.
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![
                window("5h", 100, Some(state.now + 4 * 3600 + 59 * 60)),
                window("wk", 100, Some(state.now + 23 * 3600 + 59 * 60)),
            ])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " codex 5h 100% 4h59m wk 100% 23h59m");
        assert_eq!(display_width(&rendered[1].text()), subscription_row_width());
        assert_eq!(display_width(&rendered[1].text()), 35);
    }

    #[test]
    fn worst_case_stale_row_still_fits_the_default_sidebar() {
        // Both windows at 100%, the age marker in its longest form (`23h59m`,
        // see `format_elapsed`), and a two-character Han tag: the widest
        // stale row the renderer can emit.
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![
                window("5h", 100, Some(state.now + 60)),
                window("wk", 100, Some(state.now + 60)),
            ])),
        );
        state.quota.apply(
            Subscription::Codex,
            Err(crate::quota::FetchError::throttled("429", None)),
        );
        if let Some(quota) = state.quota.codex.as_mut() {
            quota.fetched_at =
                std::time::Instant::now() - std::time::Duration::from_secs(23 * 3600 + 59 * 60);
        }
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " codex 5h 100% wk 100% ·23h59m 限流");
        assert!(
            display_width(&rendered[1].text()) <= 35,
            "a stale row has to fit the default sidebar width: {}",
            rendered[1].text()
        );
    }

    #[test]
    fn windows_keep_their_own_color_on_a_shared_row() {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![
                window("5h", 80, Some(state.now + 60)),
                window("wk", 4, Some(state.now + 60)),
            ])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        let percents: Vec<_> = rendered[1]
            .spans
            .iter()
            .filter(|span| matches!(span.kind, QuotaSpanKind::Percent(_)))
            .map(|span| span_style(&state, span.kind).fg)
            .collect();
        assert_eq!(
            percents,
            vec![
                Some(state.theme.quota_healthy),
                Some(state.theme.quota_critical)
            ],
            "a healthy 5h window must not dim the exhausted weekly one"
        );
    }

    #[test]
    fn compact_lines_drop_the_header_reset_times() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Compact);
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].text(), " kimi  5h  61% wk  83%");
    }

    #[test]
    fn hidden_level_renders_nothing() {
        let state = state_with_kimi();
        assert!(lines(&state, QuotaLevel::Hidden).is_empty());
    }

    #[test]
    fn pending_state_renders_placeholders_for_both_subscriptions() {
        let state = AppState::new("%0".into());
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 3, "header plus one row per subscription");
        assert_eq!(rendered[0].text(), " Quota");
        // Full keeps the countdown column as a placeholder so the row has the
        // same shape as the data that replaces it.
        assert_eq!(rendered[1].text(), " codex 5h   -- -- wk   -- --");
        assert_eq!(rendered[2].text(), " kimi  5h   -- -- wk   -- --");
        assert!(
            rendered[1]
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::Pending)
        );
        // Compact keeps both rows and drops the header, like a resolved block.
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact.len(), 2);
        assert_eq!(compact[0].text(), " codex 5h   -- wk   --");
    }

    #[test]
    fn disabled_kimi_has_no_pending_placeholder() {
        let mut state = AppState::new("%0".into());
        state.quota.kimi_enabled = false;
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2, "header plus the Codex placeholder only");
        assert_eq!(rendered[1].text(), " codex 5h   -- -- wk   -- --");
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact.len(), 1);
        assert_eq!(compact[0].text(), " codex 5h   -- wk   --");
    }

    #[test]
    fn resolved_state_without_subscriptions_renders_nothing() {
        let mut state = AppState::new("%0".into());
        state.quota.received_first_result = true;
        assert!(lines(&state, QuotaLevel::Full).is_empty());
        assert!(lines(&state, QuotaLevel::Compact).is_empty());
    }

    #[test]
    fn missing_weekly_window_keeps_the_five_hour_row() {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window(
                "5h",
                24,
                Some(state.now + 41 * 60),
            )])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2);
        // The weekly window is absent; the row keeps its `5h` label, its
        // countdown stays in the shared column, and no trailing separator is
        // left behind.
        assert_eq!(rendered[1].text(), " kimi  5h  24%   41m");
    }

    #[test]
    fn stale_lines_carry_an_age_marker_and_dim() {
        let mut state = state_with_kimi();
        state.quota.apply(Subscription::Kimi, Err("offline".into()));
        if let Some(quota) = state.quota.kimi.as_mut() {
            quota.fetched_at = std::time::Instant::now() - std::time::Duration::from_secs(45 * 60);
        }
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(
            state.quota.kimi.as_ref().unwrap().age_marker(),
            Some("45m".to_string())
        );
        assert!(rendered[1].stale);
        // The untrustworthy countdowns are replaced by the snapshot age plus
        // the failure tag (`错误` for an untagged error).
        assert_eq!(rendered[1].text(), " kimi  5h  61% wk  83% ·45m 错误");
        let styled = styled_line(&state, &rendered[1], 40);
        assert_eq!(styled.spans[0].style.add_modifier, Modifier::DIM);
        assert!(
            styled
                .spans
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::DIM)),
            "stale rows must be dimmed"
        );
    }

    fn state_with_failed_kimi() -> AppState {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.received_first_result = true;
        state.quota.apply(
            Subscription::Kimi,
            Err(crate::quota::FetchError::with_kind(
                "offline",
                crate::quota::FetchKind::Network,
            )),
        );
        if let Some(failure) = state.quota.kimi_failure.as_mut() {
            failure.first_failed_at =
                std::time::Instant::now() - std::time::Duration::from_secs(60);
        }
        state
    }

    #[test]
    fn first_fetch_failure_renders_a_dimmed_placeholder_with_the_failure_kind() {
        let state = state_with_failed_kimi();
        let rendered = lines(&state, QuotaLevel::Full);
        // Header plus the placeholder row: a failed first fetch must not make
        // the subscription vanish.
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[1].text(), " kimi  5h   -- wk   -- ·1m 网络");
        assert!(rendered[1].stale);
        assert!(
            rendered[1]
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::Pending)
        );
        let styled = styled_line(&state, &rendered[1], 40);
        assert!(
            styled
                .spans
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::DIM)),
            "failure placeholders dim like stale rows"
        );
        // The height the pane reserves matches what the renderer paints.
        assert_eq!(state.quota_full_height(), rendered.len() as u16);
    }

    #[test]
    fn failure_placeholder_keeps_its_marker_at_the_compact_level() {
        let state = state_with_failed_kimi();
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact.len(), 1);
        assert_eq!(compact[0].text(), " kimi  5h   -- wk   -- ·1m 网络");
        assert_eq!(state.quota_compact_height(), compact.len() as u16);
    }

    #[test]
    fn failure_placeholder_keeps_display_order_next_to_live_rows() {
        let mut state = state_with_failed_kimi();
        state.quota.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![
                window("5h", 61, Some(state.now + 133 * 60)),
                window("wk", 83, Some(state.now + 4 * 24 * 60 * 60)),
            ])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 3);
        assert_eq!(rendered[1].text(), " codex 5h  61% 2h13m wk  83% 4d");
        assert_eq!(rendered[2].text(), " kimi  5h   -- wk   -- ·1m 网络");
    }

    #[test]
    fn a_success_replaces_the_failure_placeholder() {
        let mut state = state_with_failed_kimi();
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![
                window("5h", 61, Some(state.now + 133 * 60)),
                window("wk", 83, Some(state.now + 4 * 24 * 60 * 60)),
            ])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[1].text(), " kimi  5h  61% 2h13m wk  83% 4d");
        assert!(!rendered[1].stale);
    }

    #[test]
    fn unavailable_never_renders_a_placeholder() {
        let mut state = state_with_failed_kimi();
        // Credentials gone: the subscription does not exist, so the
        // placeholder leaves with the failure record.
        state
            .quota
            .apply(Subscription::Kimi, Ok(QuotaFetch::Unavailable));
        assert!(lines(&state, QuotaLevel::Full).is_empty());

        // Unavailable from the start never renders anything either.
        let mut state = AppState::new("%0".into());
        state.quota.received_first_result = true;
        state
            .quota
            .apply(Subscription::Codex, Ok(QuotaFetch::Unavailable));
        assert!(lines(&state, QuotaLevel::Full).is_empty());
    }

    #[test]
    fn worst_case_failure_placeholder_fits_the_default_sidebar() {
        // Longest age (`23h59m`) plus a two-character Han tag on top of the
        // 22 fixed cells of the placeholder: the same 35-cell budget as a
        // stale row.
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        state.quota.received_first_result = true;
        state.quota.apply(
            Subscription::Codex,
            Err(crate::quota::FetchError::with_kind(
                "offline",
                crate::quota::FetchKind::Network,
            )),
        );
        if let Some(failure) = state.quota.codex_failure.as_mut() {
            failure.first_failed_at =
                std::time::Instant::now() - std::time::Duration::from_secs(23 * 3600 + 59 * 60);
        }
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " codex 5h   -- wk   -- ·23h59m 网络");
        assert!(
            display_width(&rendered[1].text()) <= 35,
            "a failure placeholder has to fit the default sidebar width: {}",
            rendered[1].text()
        );
    }

    #[test]
    fn narrow_rows_fall_back_to_truncated_plain_text() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Full);
        let styled = styled_line(&state, &rendered[1], 12);
        assert_eq!(styled.spans.len(), 1);
        assert_eq!(styled.spans[0].content, " kimi  5h  …");
        assert_eq!(styled.spans[0].style.fg, Some(state.theme.text_muted));
        // A row that fits keeps its per-field styles.
        let styled = styled_line(&state, &rendered[1], 40);
        assert!(styled.spans.len() > 1);
    }

    // ─── DeepSeek spend row ─────────────────────────────────────────────────

    fn state_with_spend(
        window: crate::usage::Window,
        cost_cny: f64,
        tokens: (u64, u64, u64),
        unpriced: bool,
    ) -> AppState {
        let mut state = AppState::new("%0".into());
        // 2023-11-14T22:13:20Z: a weekday outside every peak window, so the
        // period label is deterministic.
        state.now = 1_700_000_000;
        state.usage.received = true;
        state.usage.selected = window;
        // Skip the subscription placeholders: this fixture isolates the spend
        // row, and the shared quota fixture keeps them out of snapshots too.
        state.quota.received_first_result = true;
        state.usage.snapshot = Some(crate::usage::Spend {
            tokens: crate::usage::Tokens {
                uncached: tokens.0,
                cached: tokens.1,
                output: tokens.2,
            },
            cost_cny,
            unpriced,
        });
        state
    }

    #[test]
    fn spend_row_shows_amount_tokens_and_the_current_period() {
        let state = state_with_spend(
            crate::usage::Window::Today,
            1.47,
            (200_000, 12_000_000, 100_000),
            false,
        );
        let rendered = lines(&state, QuotaLevel::Full);
        // Header, then the spend row (no subscriptions in this fixture).
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[1].text(), " ds当天 ¥1.47  12.3M  98%");
        assert!(
            display_width(&rendered[1].text()) <= subscription_row_width(),
            "a typical spend row has to fit the same width as the rows around it"
        );
    }

    #[test]
    fn spend_row_worst_case_fits_its_own_budget() {
        // The row only reaches its budget on a day past ¥1000, and only with
        // three-digit thousands, a `999.9M` token total and a full cache hit.
        let state = state_with_spend(
            crate::usage::Window::Today,
            1_234.56,
            (1_000, 999_900_000, 1_000),
            false,
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " ds当天 ¥1,234.56 999.9M  100%");
        assert_eq!(display_width(&rendered[1].text()), usage_row_width());
        let bounded = lines_for_width(&state, QuotaLevel::Full, full_row_width() as u16);
        assert!(display_width(&bounded[1].text()) <= full_row_width());
        assert!(
            !bounded[1]
                .spans
                .iter()
                .any(|s| matches!(s.kind, QuotaSpanKind::UsagePeriod { .. }))
        );
        // Compact keeps the current period and drops cache detail only if needed.
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds当天 ¥1,234.56 999.9M  100% 低谷");
        let narrow = lines_for_width(&state, QuotaLevel::Compact, 28);
        assert!(display_width(&narrow[0].text()) <= 28);
        assert!(
            narrow[0]
                .spans
                .iter()
                .any(|s| matches!(s.kind, QuotaSpanKind::UsagePeriod { peak: false }))
        );
    }

    #[test]
    fn spend_row_carries_the_window_label_for_range_views() {
        let state = state_with_spend(
            crate::usage::Window::SevenDays,
            9.87,
            (1_300_000, 70_000_000, 0),
            false,
        );
        let full = lines(&state, QuotaLevel::Full);
        assert_eq!(full[1].text(), " ds7d  ¥9.87  71.3M  98%");
        // A range mixes both periods, so it never claims to be peak or off-peak.
        assert!(
            !full[1]
                .spans
                .iter()
                .any(|span| matches!(span.kind, QuotaSpanKind::UsagePeriod { .. })),
            "only the today view labels the current period"
        );
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds7d  ¥9.87  71.3M  98% 低谷");
    }

    #[test]
    fn spend_row_pending_and_unpriced_states_stay_honest() {
        let mut pending = AppState::new("%0".into());
        pending.now = 1_700_000_000;
        pending.usage.received = true;
        pending.quota.received_first_result = true;
        let rendered = lines(&pending, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " ds当天 ¥--     --  --");

        let unpriced = state_with_spend(crate::usage::Window::Today, 0.0, (1_000_000, 0, 0), true);
        let rendered = lines(&unpriced, QuotaLevel::Full);
        assert!(
            rendered[1].text().contains("¥0.00?"),
            "an unmapped model id must be visible, not a silent zero: {}",
            rendered[1].text()
        );
        assert!(
            rendered[1]
                .spans
                .iter()
                .any(|span| matches!(span.kind, QuotaSpanKind::UsageCost { unpriced: true, .. }))
        );
    }

    #[test]
    fn spend_row_colors_follow_the_daily_ladder() {
        let color = |cost_cny: f64, window: crate::usage::Window| {
            let state = state_with_spend(window, cost_cny, (1, 0, 0), false);
            let rendered = lines(&state, QuotaLevel::Full);
            match rendered[1]
                .spans
                .iter()
                .find(|span| matches!(span.kind, QuotaSpanKind::UsageCost { .. }))
                .map(|span| span.kind)
            {
                Some(kind @ QuotaSpanKind::UsageCost { .. }) => span_style(&state, kind).fg,
                _ => None,
            }
        };
        let today = crate::usage::Window::Today;
        assert_eq!(
            color(5.0, today),
            Some(
                state_with_spend(today, 5.0, (1, 0, 0), false)
                    .theme
                    .quota_healthy
            )
        );
        assert_eq!(
            color(5.01, today),
            Some(
                state_with_spend(today, 5.01, (1, 0, 0), false)
                    .theme
                    .quota_warn
            )
        );
        assert_eq!(
            color(15.01, today),
            Some(
                state_with_spend(today, 15.01, (1, 0, 0), false)
                    .theme
                    .quota_low
            )
        );
        assert_eq!(
            color(30.01, today),
            Some(
                state_with_spend(today, 30.01, (1, 0, 0), false)
                    .theme
                    .quota_critical
            )
        );
        // The 7-day view spreads the same budget over the window.
        assert_eq!(
            color(34.0, crate::usage::Window::SevenDays),
            Some(
                state_with_spend(crate::usage::Window::SevenDays, 34.0, (1, 0, 0), false)
                    .theme
                    .quota_healthy
            )
        );
    }

    #[test]
    fn spend_row_keeps_one_alarm_channel() {
        let state = state_with_spend(
            crate::usage::Window::Today,
            1.47,
            (200_000, 12_000_000, 0),
            false,
        );
        let style_of = |kind: QuotaSpanKind| span_style(&state, kind);
        assert_eq!(
            style_of(QuotaSpanKind::UsageName).fg,
            Some(state.theme.agent_deepseek)
        );
        // Each field owns its own hue, so no two numbers on the row share a
        // colour identity.
        assert_eq!(
            style_of(QuotaSpanKind::UsageTokens).fg,
            Some(state.theme.quota_spend_tokens)
        );
        assert_eq!(
            style_of(QuotaSpanKind::UsageCache).fg,
            Some(state.theme.quota_spend_cache)
        );
        assert_eq!(
            style_of(QuotaSpanKind::UsagePeriod { peak: false }).fg,
            Some(state.theme.quota_spend_off_peak)
        );
        // Peak marks itself with its own colour *and* weight, because it is the
        // one field whose meaning flips while the row is on screen.
        let peak = style_of(QuotaSpanKind::UsagePeriod { peak: true });
        assert_eq!(peak.fg, Some(state.theme.quota_spend_peak));
        assert!(peak.add_modifier.contains(Modifier::BOLD));
        assert!(
            !style_of(QuotaSpanKind::UsagePeriod { peak: false })
                .add_modifier
                .contains(Modifier::BOLD)
        );
        // An unpriced amount is muted and unpainted: a ladder colour would
        // claim a certainty that the `?` in the text says we do not have.
        let unpriced = style_of(QuotaSpanKind::UsageCost {
            cents: 100,
            unpriced: true,
        });
        assert_eq!(unpriced.fg, Some(state.theme.text_muted));
        assert!(!unpriced.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn spend_row_is_hidden_until_the_scanner_reports() {
        let mut state = state_with_kimi();
        state.quota.received_first_result = true;
        assert!(!state.usage.received);
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2, "subscription rows only");
        // The height the pane reserves matches what the renderer paints.
        assert_eq!(state.quota_full_height(), rendered.len() as u16);
        // Once the scanner reports, the block grows by exactly one row, and
        // the compact level grows with it.
        state.usage.received = true;
        let full = lines(&state, QuotaLevel::Full);
        assert_eq!(full.len(), 3);
        assert_eq!(state.quota_full_height(), 3);
        assert_eq!(state.quota_compact_height(), 2);
    }
    fn metering_state() -> AppState {
        use crate::usage::metering::{Snapshot, Summary, Total};
        let mut state =
            state_with_spend(Window::Today, 1.47, (200_000, 12_000_000, 100_000), false);
        state.now = 1_793_000_000;
        let end = state.now + 3 * 86400;
        state
            .usage
            .cycle_end
            .store(end, std::sync::atomic::Ordering::Relaxed);
        state.quota.apply(
            Subscription::Codex,
            Ok(QuotaFetch::Available(vec![
                window("5h", 62, Some(state.now + 7800)),
                window("wk", 35, Some(end)),
            ])),
        );
        let total = Total {
            tokens: crate::usage::Tokens {
                uncached: 200_000,
                cached: 17_000_000,
                output: 1_000_000,
            },
            cost: 128_400_000_000_000,
            ..Total::default()
        };
        let summary = Summary {
            start: (end - 604800) as i64,
            end: state.now as i64,
            total: total.clone(),
            models: [("gpt-6.1-sol".into(), total)].into(),
        };
        state.usage.codex = Some(Snapshot {
            cycle_end: end,
            cycle: Some(summary.clone()),
            recent: std::array::from_fn(|i| crate::usage::metering::Summary {
                start: if i == 3 {
                    Window::Today.start(state.now as i64)
                } else {
                    state.now as i64 - [86_400, 604_800, 2_592_000][i]
                },
                ..summary.clone()
            }),
            observed_at: state.now as i64,
            incomplete: false,
        });
        state
    }

    fn frame_text(
        state: &AppState,
        level: QuotaLevel,
        width: u16,
        height: u16,
        details: bool,
    ) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                if details {
                    render_usage_details(f, state, f.area());
                } else {
                    render(f, state, f.area(), level);
                }
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .chunks(width as usize)
            .map(|row| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn short_error_labels_fit_default_and_narrow_rows_snapshot() {
        let mut output = String::new();
        for width in [35, 28] {
            for kind in [FetchKind::Dns, FetchKind::Timeout, FetchKind::Certificate] {
                let mut state = metering_state();
                state.apply_quota_result(crate::quota::QuotaFetchResult {
                    codex: Some(Err(crate::quota::FetchError::with_kind("fixture", kind))),
                    kimi: None,
                    forced: false,
                });
                let rows = lines_for_width(&state, QuotaLevel::Compact, width);
                let quota = rows.iter().find(|row| row.stale).unwrap();
                assert!(display_width(&quota.text()) <= width as usize);
                assert!(
                    quota
                        .spans
                        .iter()
                        .any(|span| span.kind == QuotaSpanKind::Countdown
                            && span.text.ends_with(kind.label()))
                );
                output.push_str(&format!("{width} {}\n", kind.label()));
                output.push_str(&frame_text(&state, QuotaLevel::Compact, width, 5, false));
                output.push('\n');
            }
        }
        insta::assert_snapshot!(output, @"
        35 DNS

         codex 5h  62% wk  35% ·now DNS
         cx本 期  $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98% 高 峰
        35 超时

         codex 5h  62% wk  35% ·now 超 时
         cx本 期  $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98% 高 峰
        35 证书

         codex 5h  62% wk  35% ·now 证 书
         cx本 期  $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98% 高 峰
        28 DNS

         codex 5h  62% wk  35% ·DNS
         cx周    $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M 98% 
        28 超时

         codex 5h  62% wk  35% ·超 时
         cx周    $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M 98% 
        28 证书

         codex 5h  62% wk  35% ·证 书
         cx周    $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M 98% 
        ");
    }

    #[test]
    fn spare_columns_preserve_countdown_then_expand_name() {
        let mut state = metering_state();
        let mut output = frame_text(&state, QuotaLevel::Compact, 28, 5, false);
        // A larger amount takes precedence over the expanded cycle name.
        state
            .usage
            .codex
            .as_mut()
            .unwrap()
            .cycle
            .as_mut()
            .unwrap()
            .total
            .cost = 1_234_560_000_000_000;
        output.push_str("\nLarge amount\n");
        output.push_str(&frame_text(&state, QuotaLevel::Compact, 28, 5, false));
        insta::assert_snapshot!(output, @"

         codex 5h  62% 2h10m wk  35%
         cx周    $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M 98% 
        Large amount

         codex 5h  62% 2h10m wk  35%
        cx周    $1234.56  18.2M  99%
        cx当 天  $128.40   18.2M  99%
        ds当 天  ¥1.47     12.3M 
        ");
    }

    #[test]
    fn shared_usage_window_labels_snapshot() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        let mut output = String::new();
        for _ in 0..4 {
            output.push_str(&frame_text(&state, QuotaLevel::Compact, 28, 5, false));
            output.push('\n');
            state.usage.cycle_all();
        }
        insta::assert_snapshot!(output, @"

         codex 5h  62% 2h10m wk  35%
         cx周    $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M 98% 

         codex 5h  62% 2h10m wk  35%
         cx周   $128.40  18.2M  99%
         cx24h $128.40  18.2M  99%
        ds24h ¥1.47    12.3M  98% 

         codex 5h  62% 2h10m wk  35%
         cx周   $128.40  18.2M  99%
         cx7d  $128.40  18.2M  99%
        ds7d  ¥1.47    12.3M  98% 

         codex 5h  62% 2h10m wk  35%
         cx周   $128.40  18.2M  99%
         cx30d $128.40  18.2M  99%
        ds30d ¥1.47    12.3M  98% 
        ");
    }

    #[test]
    fn usage_header_tracks_shared_and_independent_windows_snapshot() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        let mut output = String::new();
        for _ in 0..4 {
            output.push_str(&frame_text(&state, QuotaLevel::Full, 35, 7, false));
            output.push('\n');
            state.usage.cycle_all();
        }
        state.usage.cycle();
        output.push_str(&frame_text(&state, QuotaLevel::Full, 35, 7, false));
        output.push('\n');
        state.usage.recent = state.usage.recent.next();
        output.push_str(&frame_text(&state, QuotaLevel::Full, 35, 7, false));
        insta::assert_snapshot!(output, @"


         Usage 当 天                ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx当 天  $128.40  18.2M  99%
        ds当 天  ¥1.47    12.3M  98%


         Usage 24h                ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx24h  $128.40  18.2M  99%
        ds24h  ¥1.47    12.3M  98%


         Usage 7d                 ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx7d   $128.40  18.2M  99%
        ds7d   ¥1.47    12.3M  98%


         Usage 30d                ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx30d  $128.40  18.2M  99%
        ds30d  ¥1.47    12.3M  98%


         Usage                    ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx当 天  $128.40  18.2M  99%
        ds24h  ¥1.47    12.3M  98%


         Usage 24h                ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M  99%
        cx24h  $128.40  18.2M  99%
        ds24h  ¥1.47    12.3M  98%
        ");
    }

    #[test]
    fn current_period_stays_visible_for_every_window_and_compact_width() {
        let mut state = metering_state();
        let mut output = String::new();
        for (now, peak) in [(1_792_990_800, false), (1_793_000_000, true)] {
            state.now = now;
            for window in Window::ALL {
                state.usage.selected = window;
                state.usage.recent = window.recent();
                for level in [QuotaLevel::Full, QuotaLevel::Compact] {
                    let rows = lines_for_width(&state, level, 28);
                    assert!(rows.iter().any(|r| {
                        r.spans
                            .iter()
                            .any(|s| s.kind == QuotaSpanKind::UsagePeriod { peak })
                    }));
                }
            }
            output.push_str(&frame_text(&state, QuotaLevel::Full, 35, 7, false));
            output.push('\n');
            output.push_str(&frame_text(&state, QuotaLevel::Compact, 28, 5, false));
            output.push('\n');
        }
        assert_ne!(
            span_style(&state, QuotaSpanKind::UsagePeriod { peak: true }).fg,
            span_style(&state, QuotaSpanKind::UsagePeriod { peak: false }).fg
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::ResetCountdown).fg,
            Some(state.theme.text_active)
        );
        assert_ne!(
            span_style(&state, QuotaSpanKind::ResetCountdown).fg,
            span_style(&state, QuotaSpanKind::Countdown).fg
        );
        insta::assert_snapshot!(output, @"


          Usage 24h                ds低 谷
         codex 5h  62% 4h44m wk  35% 3d2h
         cx本 期  $128.40  18.2M  99%
         cx24h  $128.40  18.2M  99%
         ds24h  ¥1.47    12.3M  98%

         codex 5h  62% 4h44m wk  35%
         cx周   $128.40  18.2M  99%
         cx24h $128.40  18.2M  99%
        ds24h ¥1.47    12.3M  98% 


          Usage 24h                ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx24h  $128.40  18.2M  99%
         ds24h  ¥1.47    12.3M  98%

         codex 5h  62% 2h10m wk  35%
         cx周   $128.40  18.2M  99%
         cx24h $128.40  18.2M  99%
        ds24h ¥1.47    12.3M  98% 
        ");
    }

    #[test]
    fn amount_modes_preserve_tokens_at_supported_widths_snapshot() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        state.usage.cycle_all();
        state.usage.cycle_all();
        let mut output = String::new();
        for daily in [false, true] {
            if daily {
                for row in [
                    UsageRow::DeepSeek,
                    UsageRow::CodexRecent,
                    UsageRow::CodexCycle,
                ] {
                    state.usage.toggle_amount(row, state.now);
                }
            }
            output.push_str(&format!("Daily {daily}\n"));
            for width in [28, 35, 45, 60] {
                output.push_str(&format!("Width {width}\n"));
                output.push_str(&frame_text(&state, QuotaLevel::Full, width, 7, false));
                output.push('\n');
            }
        }
        insta::assert_snapshot!(output, @"
        Daily false
        Width 28


          Usage 7d          ds高 峰
         codex 5h  62% 2h10m wk  35%
         cx本 期  $128.40  18.2M  99%
         cx7d   $128.40  18.2M  99%
         ds7d   ¥1.47    12.3M  98%
        Width 35


          Usage 7d                 ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx7d   $128.40  18.2M  99%
         ds7d   ¥1.47    12.3M  98%
        Width 45


          Usage 7d                           ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx7d   $128.40  18.2M  99%
         ds7d   ¥1.47    12.3M  98%
        Width 60


          Usage 7d                                          ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx7d   $128.40  18.2M  99%
         ds7d   ¥1.47    12.3M  98%
        Daily true
        Width 28


          Usage 7d          ds高 峰
         codex 5h  62% 2h10m wk  35%
         cx周   $32.10/d  18.2M  99%
         cx7d  $18.34/d  18.2M  99%
         ds7d  ¥0.21/d   12.3M  98%
        Width 35


          Usage 7d                 ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $32.10/d  18.2M  99%
         cx7d   $18.34/d  18.2M  99%
         ds7d   ¥0.21/d   12.3M  98%
        Width 45


          Usage 7d                           ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $32.10/d  18.2M  99%
         cx7d   $18.34/d  18.2M  99%
         ds7d   ¥0.21/d   12.3M  98%
        Width 60


          Usage 7d                                          ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $32.10/d  18.2M  99%
         cx7d   $18.34/d  18.2M  99%
         ds7d   ¥0.21/d   12.3M  98%
        ");
    }

    #[test]
    fn amount_preferences_are_independent_and_survive_ineligible_windows() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        for row in [UsageRow::DeepSeek, UsageRow::CodexRecent] {
            state.usage.toggle_amount(row, state.now);
            assert!(!state.usage.show_daily_amount(row, state.now));
        }
        state.usage.cycle_all();
        state.usage.cycle_all();
        let tokens = state.usage.snapshot.as_ref().unwrap().tokens;
        state.usage.toggle_amount(UsageRow::DeepSeek, state.now);
        assert!(state.usage.show_daily_amount(UsageRow::DeepSeek, state.now));
        assert!(
            !state
                .usage
                .show_daily_amount(UsageRow::CodexRecent, state.now)
        );
        state.usage.toggle_amount(UsageRow::CodexRecent, state.now);
        assert!(
            !state
                .usage
                .show_daily_amount(UsageRow::CodexCycle, state.now)
        );
        state.usage.toggle_amount(UsageRow::CodexCycle, state.now);
        assert!(
            state
                .usage
                .show_daily_amount(UsageRow::CodexCycle, state.now)
        );
        assert_eq!(state.usage.snapshot.as_ref().unwrap().tokens, tokens);
        for window in [Window::Today, Window::Last24Hours, Window::SevenDays] {
            state.usage.selected = window;
            state.usage.recent = window.recent();
            for row in [UsageRow::DeepSeek, UsageRow::CodexRecent] {
                assert_eq!(
                    state.usage.show_daily_amount(row, state.now),
                    window == Window::SevenDays
                );
            }
        }
        state.usage.snapshot.as_mut().unwrap().unpriced = true;
        assert!(!state.usage.show_daily_amount(UsageRow::DeepSeek, state.now));
        state.usage.toggle_amount(UsageRow::DeepSeek, state.now);
        state.usage.snapshot.as_mut().unwrap().unpriced = false;
        assert!(state.usage.show_daily_amount(UsageRow::DeepSeek, state.now));
        state.usage.codex.as_mut().unwrap().recent[state.usage.recent.index()]
            .total
            .unpriced = 1;
        assert!(
            !state
                .usage
                .can_toggle_amount(UsageRow::CodexRecent, state.now)
        );
        state.now += 604_800;
        assert!(
            !state
                .usage
                .can_toggle_amount(UsageRow::CodexCycle, state.now)
        );
    }

    #[test]
    fn final_span_targets_exclude_status_padding_and_noninteractive_fields() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        state.usage.cycle_all();
        state.usage.cycle_all();
        for width in [28, 35, 45, 60] {
            for level in [QuotaLevel::Full, QuotaLevel::Compact] {
                let lines = lines_for_width(&state, level, width);
                for (i, line) in lines.iter().enumerate() {
                    let area = Rect::new(7, 20 + i as u16, width, 1);
                    let targets = field_targets(&state, line, area);
                    let mut x = area.x;
                    for span in &line.spans {
                        let active = matches!(
                            span.kind,
                            QuotaSpanKind::UsageHeader
                                | QuotaSpanKind::UsageName
                                | QuotaSpanKind::CodexCycle
                                | QuotaSpanKind::CodexRecent
                                | QuotaSpanKind::UsageCost { .. }
                                | QuotaSpanKind::CodexEstimate { .. }
                        );
                        let trimmed = span.text.trim();
                        let leading = display_width(
                            &span.text[..span.text.len() - span.text.trim_start().len()],
                        ) as u16;
                        let text_width = display_width(trimmed) as u16;
                        for col in x..x + display_width(&span.text) as u16 {
                            let hits = targets
                                .iter()
                                .filter(|t| {
                                    t.rect
                                        .contains(ratatui::layout::Position { x: col, y: area.y })
                                })
                                .count();
                            assert_eq!(
                                hits,
                                usize::from(
                                    active && col >= x + leading && col < x + leading + text_width
                                ),
                                "{span:?} at {col}, width {width}"
                            );
                        }
                        x += display_width(&span.text) as u16;
                    }
                    assert!(
                        targets
                            .iter()
                            .all(|t| t.rect.x >= area.x && t.rect.right() <= area.right())
                    );
                }
            }
        }
        state.usage.snapshot.as_mut().unwrap().unpriced = true;
        let ds = usage_line(&state, QuotaLevel::Full);
        assert!(
            !field_targets(&state, &ds, Rect::new(0, 0, 60, 1))
                .iter()
                .any(|t| matches!(t.action, UsageAction::ToggleAmount(_)))
        );
        assert!(field_targets(&state, &ds, Rect::new(0, 0, 8, 1)).is_empty());
    }

    #[test]
    fn rendered_amount_action_changes_only_its_row() {
        use std::sync::atomic::Ordering;
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        state.usage.cycle_all();
        state.usage.cycle_all();
        let line = usage_line(&state, QuotaLevel::Full);
        let targets = field_targets(&state, &line, Rect::new(7, 20, 45, 1));
        let amount = targets
            .iter()
            .find(|t| t.action == UsageAction::ToggleAmount(UsageRow::DeepSeek))
            .unwrap()
            .rect;
        state.layout.usage_rows = vec![20];
        state.layout.usage_targets = targets;
        let tokens = state.usage.snapshot.as_ref().unwrap().tokens;
        for col in [amount.x, amount.x + amount.width / 2, amount.right() - 1] {
            assert!(state.handle_quota_click(20, col));
        }
        assert!(state.usage.show_daily_amount(UsageRow::DeepSeek, state.now));
        assert!(
            !state
                .usage
                .show_daily_amount(UsageRow::CodexRecent, state.now)
        );
        assert_eq!(state.usage.selected, Window::SevenDays);
        assert_eq!(state.usage.snapshot.as_ref().unwrap().tokens, tokens);
        assert!(state.handle_quota_click(20, amount.right()));
        assert!(state.usage.show_daily_amount(UsageRow::DeepSeek, state.now));
        assert!(!state.quota_force_refresh.load(Ordering::Relaxed));
        assert!(!state.usage.force.load(Ordering::Relaxed));
    }

    #[test]
    fn metering_sections_and_collapse_snapshot() {
        let state = metering_state();
        let mut result = String::new();
        for level in [
            QuotaLevel::Full,
            QuotaLevel::Cycle,
            QuotaLevel::Compact,
            QuotaLevel::CompactCycle,
            QuotaLevel::QuotaOnly,
        ] {
            result.push_str(&format!(
                "{level:?}\n{}\n",
                frame_text(&state, level, 35, 8, false)
            ));
        }
        insta::assert_snapshot!(result, @"
        Full



          Usage 当 天                ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98%
        Cycle




          Usage 当 天                ds高 峰
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98%
        Compact




         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         cx当 天  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98% 高 峰
        CompactCycle





         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M  99%
         ds当 天  ¥1.47    12.3M  98% 高 峰
        QuotaOnly






         Quota
         codex 5h  62% 2h10m wk  35% 3d
        ");
    }

    #[test]
    fn quota_failure_keeps_local_cycle_fresh() {
        let mut state = metering_state();
        let before = codex_line(&state, true);
        assert!(!before.stale);
        state.apply_quota_result(crate::quota::QuotaFetchResult {
            codex: Some(Err(crate::quota::FetchError::with_kind(
                "timeout",
                FetchKind::Network,
            ))),
            kimi: None,
            forced: false,
        });
        let after = codex_line(&state, true);
        assert!(!after.stale);
        assert_eq!(after.spans, before.spans);
        assert!(state.quota.codex.as_ref().unwrap().is_stale());
    }

    #[test]
    fn metering_pending_and_unknown_price_snapshot() {
        let mut state = metering_state();
        state
            .usage
            .cycle_end
            .store(state.now - 1, std::sync::atomic::Ordering::Relaxed);
        let snapshot = state.usage.codex.as_mut().unwrap();
        snapshot.incomplete = true;
        snapshot.recent[state.usage.recent.index()].total.cost = 0;
        snapshot.recent[state.usage.recent.index()].total.unpriced = 3;
        insta::assert_snapshot!(frame_text(&state,QuotaLevel::Full,35,8,false), @"



         Usage 当 天                ds高 峰
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  本 期 待 刷 新
        cx当 天  $--?   18.2M  99% !
        ds当 天  ¥1.47  12.3M  98%
        ");
    }

    #[test]
    fn metering_details_snapshot() {
        let mut state = metering_state();
        state.popup = crate::state::PopupState::Usage {
            recent: false,
            scroll: 0,
        };
        insta::assert_snapshot!(frame_text(&state,QuotaLevel::Full,40,20,true), @"


        ┌ Usage 本 期  ──────────────────────────┐
        │本 机 记 录  · API 等 价 估 值                │
        │不 代 表 账 号 全 部 用 量 或 订 阅 账 单           │
        │价 目 表  2026-10-26.2                   │
        │2026-10-22 07:33 →                    │
        │2026-10-26 07:33 UTC                  │
        │总 计  18.2M $128.40                    │
        │gpt-6.1-sol                           │
        │  18.2M $128.40                       │
        │  输 入  200.0K                         │
        │  缓 存  17.0M                          │
        │  输 出  1.0M                           │
        │仅 含 日 志 可 见 用 量 ， 图 片 费 用 未 覆 盖       │
        │Tab 本 期 /近 期  · ↑↓ 滚 动  · Esc 关 闭     │
        └──────────────────────────────────────┘
        ");
    }

    #[test]
    fn metering_price_explanations_snapshot() {
        let mut state = metering_state();
        let total = &mut state
            .usage
            .codex
            .as_mut()
            .unwrap()
            .cycle
            .as_mut()
            .unwrap()
            .total;
        total.assumed_price = 12;
        total.unpriced = 3;
        total.unknown_tier = 1;
        total.assumed_standard = 4;
        total.missing_price = 1;
        total.unsupported_cache_write = 1;
        state.popup = crate::state::PopupState::Usage {
            recent: false,
            scroll: 0,
        };
        insta::assert_snapshot!(frame_text(&state, QuotaLevel::Full,40,32,true), @"





        ┌ Usage 本 期  ──────────────────────────┐
        │本 机 记 录  · API 等 价 估 值                │
        │不 代 表 账 号 全 部 用 量 或 订 阅 账 单           │
        │价 目 表  2026-10-26.2                   │
        │2026-10-22 07:33 →                    │
        │2026-10-26 07:33 UTC                  │
        │总 计  18.2M $128.40?                   │
        │4 条 未 记 录 档 位 ， 按  Standard 估 算       │
        │? 未 知 服 务 档 位 ， 未 套 用 标 准 价           │
        │? 3 条 记 录 未 定 价                       │
        │? 1 条 缺 少 模 型 历 史 价 格                 │
        │? 1 条 缓 存 写 入 计 费 不 明                 │
        │历 史 段 按 已 核 实 官 方 价 格 回 填             │
        │假 设 核 实 日 前 价 格 未 变                   │
        │gpt-6.1-sol                           │
        │  18.2M $128.40                       │
        │  输 入  200.0K                         │
        │  缓 存  17.0M                          │
        │  输 出  1.0M                           │
        │仅 含 日 志 可 见 用 量 ， 图 片 费 用 未 覆 盖       │
        │Tab 本 期 /近 期  · ↑↓ 滚 动  · Esc 关 闭     │
        └──────────────────────────────────────┘
        ");
    }

    #[test]
    fn codex_amounts_use_daily_color_steps_and_details_scroll() {
        let mut state = metering_state();
        for kind in [QuotaSpanKind::CodexCycle, QuotaSpanKind::CodexRecent] {
            let style = span_style(&state, kind);
            assert_eq!(style.fg, Some(state.theme.agent_codex));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }
        for (cents, expected) in [
            (0, state.theme.text_muted),
            (2500, state.theme.text_muted),
            (2501, state.theme.agent_codex),
            (7500, state.theme.agent_codex),
            (7501, state.theme.quota_good),
            (15000, state.theme.quota_good),
            (15001, state.theme.quota_healthy),
        ] {
            for days in [1, 7, 30] {
                let style = span_style(
                    &state,
                    QuotaSpanKind::CodexEstimate {
                        cents: cents * days as u64,
                        days,
                        unpriced: false,
                    },
                );
                assert_eq!(style.fg, Some(expected));
                assert!(style.add_modifier.contains(Modifier::BOLD));
            }
        }
        assert_eq!(
            span_style(
                &state,
                QuotaSpanKind::CodexEstimate {
                    cents: 99999,
                    days: 1,
                    unpriced: true
                }
            )
            .fg,
            Some(state.theme.text_muted)
        );
        let row = codex_line(&state, true);
        assert!(
            row.spans
                .iter()
                .any(|s| s.kind == QuotaSpanKind::UsageCache && s.text == " 99%")
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::UsageCache).fg,
            Some(state.theme.quota_spend_cache)
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::UsageTokens).fg,
            Some(state.theme.quota_spend_tokens)
        );
        state.popup = crate::state::PopupState::Usage {
            recent: false,
            scroll: 4,
        };
        insta::assert_snapshot!(frame_text(&state,QuotaLevel::Full,28,10,true), @"
        ┌ Usage 本 期  ──────────────┐
        │2026-10-22 07:33 →        │
        │2026-10-26 07:33 UTC      │
        │总 计  18.2M $128.40        │
        │gpt-6.1-sol               │
        │  18.2M $128.40           │
        │  输 入  200.0K             │
        │  缓 存  17.0M              │
        │  输 出  1.0M               │
        └──────────────────────────┘
        ");
    }

    #[test]
    fn metering_clicks_and_expired_cycle_never_relabel_old_usage() {
        let mut state = metering_state();
        state.layout.usage_rows = vec![3, 4];
        state.layout.usage_targets = [
            (3, UsageAction::OpenCycleDetails),
            (4, UsageAction::CycleCodexRecent),
        ]
        .map(|(row, action)| UsageClickTarget {
            rect: Rect::new(1, row, 6, 1),
            action,
        })
        .into();
        assert!(state.handle_quota_click(4, 1));
        assert_eq!(state.usage.recent, crate::usage::metering::Recent::Day);
        assert!(state.handle_quota_click(3, 1));
        assert!(matches!(
            state.popup,
            crate::state::PopupState::Usage { recent: false, .. }
        ));
        state.now += 604800;
        assert!(state.usage.cycle_summary(state.now).is_none());
        assert_eq!(
            state.usage.codex.as_ref().unwrap().recent[1]
                .total
                .tokens
                .total(),
            18_200_000
        );
    }
}
