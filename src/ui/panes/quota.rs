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
use crate::usage::{UsageRow, format_tokens};

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
/// Shared name/window column: `cx本期` and `ds当天` are the widest labels.
const USAGE_NAME_WIDTH: usize = 6;
/// Cache icon, space, and a three-digit percentage.
const USAGE_CACHE_WIDTH: usize = 6;
/// Right-aligned scope or price-period marker: `账号`, `全部`, `高峰`, `低谷`.
const USAGE_TAIL_WIDTH: usize = 4;
/// Normal monetary budget without thousands separators (`¥9999.99`).
#[cfg(test)]
const USAGE_AMOUNT_MAX: usize = 8;
/// Material Design alpha-t / alpha-a: the `t` and `a` key hints.
const T_ICON: &str = "\u{f0b01}";
const A_ICON: &str = "\u{f0aee}";

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
        + USAGE_NAME_WIDTH
        + 1
        + USAGE_AMOUNT_MAX
        + 1
        + USAGE_TOKEN_WIDTH
        + 1
        + USAGE_CACHE_WIDTH
        + 1
        + USAGE_TAIL_WIDTH
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
    /// Right-aligned scope marker on a Codex row.
    UsageScope,
    /// `!` when the source snapshot is incomplete.
    UsageWarning,
    /// Decorative leader between the Usage title and the interval control.
    UsageLeader,
    /// Shared rolling window control: the `t` glyph and its window label.
    UsageWindow(crate::usage::Window),
    /// The `本期` stop: account cycle plus both other rows on 当天.
    UsageWindowCycle,
    /// Mixed-window interval control; no single shared window label.
    UsageWindowMixed,
    /// Clickable `日均` control while no visible row is in daily mode.
    UsageDailyHint,
    /// Clickable `日均` control while at least one visible row is daily.
    UsageDailyActive,
    /// Spend row before its first result arrives.
    UsagePendingAmount,
    UsagePendingTokens,
    UsagePendingCache,
    /// Signed-in Codex account row (name plus selected window).
    CodexAccount,
    /// All-local-accounts Codex row (name plus selected window).
    CodexAll,
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

/// ` ds当天 ¥1.91 14.1M 97% 高峰`.
///
/// The price period is row metadata, independent of the selected window.
fn usage_line(state: &AppState) -> QuotaLine {
    let usage = &state.usage;
    let mut spans = vec![
        QuotaSpan::label(" ".repeat(INDENT as usize)),
        QuotaSpan {
            text: usage.selected.name().into(),
            kind: QuotaSpanKind::UsageName,
        },
        QuotaSpan::label(" "),
    ];
    match &usage.snapshot {
        Some(spend) => {
            let amount = usage_amount(state, UsageRow::DeepSeek).unwrap();
            spans.push(QuotaSpan {
                text: amount,
                kind: QuotaSpanKind::UsageCost {
                    cents: (spend.cost_cny * 100.0).round().max(0.0) as u64,
                    unpriced: spend.unpriced,
                },
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: usage_tokens(state, UsageRow::DeepSeek, spend.tokens.total()),
                kind: QuotaSpanKind::UsageTokens,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: cache_ratio(spend.tokens),
                kind: QuotaSpanKind::UsageCache,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(peak_span(state));
        }
        None => {
            spans.push(QuotaSpan {
                text: "¥--".into(),
                kind: QuotaSpanKind::UsagePendingAmount,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: "--".into(),
                kind: QuotaSpanKind::UsagePendingTokens,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: cache_field("--"),
                kind: QuotaSpanKind::UsagePendingCache,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(peak_span(state));
        }
    }
    QuotaLine {
        spans,
        stale: false,
    }
}

fn peak_span(state: &AppState) -> QuotaSpan {
    let peak = crate::usage::is_peak(state.now as i64);
    QuotaSpan {
        text: if peak { "高峰" } else { "低谷" }.into(),
        kind: QuotaSpanKind::UsagePeriod { peak },
    }
}

/// Share of input tokens served from cache, shared by all usage providers.
/// `--` when nothing was billed rather than an invented 0%.
fn cache_ratio(tokens: crate::usage::Tokens) -> String {
    let input = tokens.uncached + tokens.cached;
    if input == 0 {
        return cache_field("--");
    }
    let percent = ((tokens.cached as f64 / input as f64) * 100.0).round() as u64;
    cache_field(&format!("{percent}%"))
}

fn cache_field(value: &str) -> String {
    // Reserve the percentage slot even for missing input or a one-digit ratio.
    format!("{CACHE_ICON} {value:>4}")
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
        let history_marker = if state.usage.history.notice.is_some() {
            " !"
        } else if state.usage.history.importing {
            " 导入中"
        } else {
            ""
        };
        let mut spans = vec![QuotaSpan {
            text: if show_usage {
                format!(" {USAGE_TITLE}{history_marker}")
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
            // The header and keyboard action control all scopes, including a
            // scope temporarily hidden by height pressure.
            let daily_available = state.usage.can_toggle_tokens(UsageRow::DeepSeek, state.now)
                || state
                    .usage
                    .can_toggle_tokens(UsageRow::CodexAccount, state.now)
                || state.usage.can_toggle_tokens(UsageRow::CodexAll, state.now);
            let daily_visible = state.usage.show_daily(UsageRow::DeepSeek, state.now)
                || state.usage.show_daily(UsageRow::CodexAccount, state.now)
                || state.usage.show_daily(UsageRow::CodexAll, state.now);
            spans.push(QuotaSpan {
                text: " ".into(),
                kind: QuotaSpanKind::UsageLeader,
            });
            match state.usage.interval_stop() {
                crate::usage::IntervalStop::Cycle => {
                    spans.push(QuotaSpan {
                        text: T_ICON.into(),
                        kind: QuotaSpanKind::UsageWindowCycle,
                    });
                    spans.push(QuotaSpan {
                        text: "本期".into(),
                        kind: QuotaSpanKind::UsageWindowCycle,
                    });
                }
                crate::usage::IntervalStop::Rolling(window) => {
                    spans.push(QuotaSpan {
                        text: T_ICON.into(),
                        kind: QuotaSpanKind::UsageWindow(window),
                    });
                    spans.push(QuotaSpan {
                        text: window.label().into(),
                        kind: QuotaSpanKind::UsageWindow(window),
                    });
                }
                crate::usage::IntervalStop::Custom => {
                    spans.push(QuotaSpan {
                        text: T_ICON.into(),
                        kind: QuotaSpanKind::UsageWindowMixed,
                    });
                    spans.push(QuotaSpan {
                        text: "混选".into(),
                        kind: QuotaSpanKind::UsageWindowMixed,
                    });
                }
            }
            if daily_available {
                spans.push(QuotaSpan::label(" "));
                spans.push(QuotaSpan {
                    text: format!("{A_ICON}日均"),
                    kind: if daily_visible {
                        QuotaSpanKind::UsageDailyActive
                    } else {
                        QuotaSpanKind::UsageDailyHint
                    },
                });
            }
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
        if state.usage.codex_all.is_some() || state.usage.codex_account.is_some() {
            lines.push(codex_line(state, UsageRow::CodexAccount));
            if matches!(level, QuotaLevel::Full | QuotaLevel::Compact) {
                lines.push(codex_line(state, UsageRow::CodexAll));
            }
        }
        lines.push(usage_line(state));
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
    layout_usage_rows(&mut rows, width);
    if let Some(header) = rows
        .iter_mut()
        .find(|r| r.spans.iter().any(|s| s.kind == QuotaSpanKind::UsageHeader))
    {
        layout_usage_header(header, width);
    }
    rows
}

/// Field positions shared by every usage row. The builder emits them in this
/// order; the layout pass rewrites the texts and separators in place.
struct UsageFields {
    name: usize,
    amount: usize,
    token: usize,
    cache: usize,
    warning: Option<usize>,
    tail: usize,
}

fn usage_fields(row: &QuotaLine) -> Option<UsageFields> {
    let mut fields = UsageFields {
        name: usize::MAX,
        amount: usize::MAX,
        token: usize::MAX,
        cache: usize::MAX,
        warning: None,
        tail: usize::MAX,
    };
    for (index, span) in row.spans.iter().enumerate() {
        match span.kind {
            QuotaSpanKind::UsageName | QuotaSpanKind::CodexAccount | QuotaSpanKind::CodexAll => {
                fields.name = index;
            }
            QuotaSpanKind::UsageCost { .. }
            | QuotaSpanKind::CodexEstimate { .. }
            | QuotaSpanKind::UsagePendingAmount => fields.amount = index,
            QuotaSpanKind::UsageTokens | QuotaSpanKind::UsagePendingTokens => {
                fields.token = index;
            }
            QuotaSpanKind::UsageCache | QuotaSpanKind::UsagePendingCache => fields.cache = index,
            QuotaSpanKind::UsageWarning => fields.warning = Some(index),
            QuotaSpanKind::UsageScope | QuotaSpanKind::UsagePeriod { .. } => fields.tail = index,
            _ => {}
        }
    }
    (fields.name != usize::MAX
        && fields.amount != usize::MAX
        && fields.token != usize::MAX
        && fields.cache != usize::MAX
        && fields.tail != usize::MAX)
        .then_some(fields)
}

fn pad_left(text: &str, width: usize) -> String {
    format!(
        "{}{text}",
        " ".repeat(width.saturating_sub(display_width(text)))
    )
}

fn pad_right(text: &str, width: usize) -> String {
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(display_width(text)))
    )
}

/// Narrowest labels retain the provider and window; `cx本期` becomes `cx周`.
fn short_name(name: &str) -> &str {
    match name {
        "cx本期" => "cx周",
        "ds当天" => "ds天",
        other => other,
    }
}

/// Rebuild one row with fixed columns. Returns `None` when the row cannot fit
/// `width` while keeping a one-cell gap before the tail.
fn build_usage_row(
    row: &QuotaLine,
    fields: &UsageFields,
    width: usize,
    amount_width: usize,
    name_width: usize,
    cache: bool,
    short: bool,
) -> Option<QuotaLine> {
    let name = row.spans[fields.name].text.trim();
    let name = if short { short_name(name) } else { name };
    let name_pad = name_width.max(display_width(name));
    let mut spans = vec![
        QuotaSpan::label(" "),
        QuotaSpan {
            text: pad_right(name, name_pad),
            kind: row.spans[fields.name].kind,
        },
        QuotaSpan::label(" "),
        QuotaSpan {
            text: pad_left(row.spans[fields.amount].text.trim(), amount_width),
            kind: row.spans[fields.amount].kind,
        },
        QuotaSpan::label(" "),
        QuotaSpan {
            text: pad_left(row.spans[fields.token].text.trim(), USAGE_TOKEN_WIDTH),
            kind: row.spans[fields.token].kind,
        },
    ];
    if cache {
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: pad_right(row.spans[fields.cache].text.trim_end(), USAGE_CACHE_WIDTH),
            kind: row.spans[fields.cache].kind,
        });
    }
    if fields
        .warning
        .is_some_and(|index| !row.spans[index].text.trim().is_empty())
    {
        spans.push(QuotaSpan {
            text: " !".into(),
            kind: QuotaSpanKind::UsageWarning,
        });
    }
    let used: usize = spans.iter().map(|span| display_width(&span.text)).sum();
    let gap = width.checked_sub(used + USAGE_TAIL_WIDTH)?;
    if gap == 0 {
        return None;
    }
    spans.push(QuotaSpan::label(" ".repeat(gap)));
    spans.push(QuotaSpan {
        text: pad_left(row.spans[fields.tail].text.trim(), USAGE_TAIL_WIDTH),
        kind: row.spans[fields.tail].kind,
    });
    Some(QuotaLine {
        spans,
        stale: row.stale,
    })
}

/// Place the compact `t区间 a日均` hint at the header's right edge, or drop it.
fn layout_usage_header(header: &mut QuotaLine, width: usize) {
    let title = header.spans[0].text.trim_end().to_string();
    header.spans[0].text = title.clone();
    if header.spans.len() < 4 {
        return;
    }
    let hint: usize = header.spans[2..]
        .iter()
        .map(|span| display_width(&span.text))
        .sum();
    let Some(gap) = width
        .checked_sub(display_width(&title) + hint)
        .filter(|gap| *gap >= 1)
    else {
        header.spans.truncate(1);
        return;
    };
    header.spans[1].text = usage_leader(gap);
}

/// Dim leader between the title and the interval control; keeps one clear cell
/// on either side so the line never touches text.
fn usage_leader(width: usize) -> String {
    match width {
        0 | 1 => " ".into(),
        2 => "  ".into(),
        _ => format!(" {} ", "─".repeat(width - 2)),
    }
}

fn layout_usage_rows(rows: &mut [QuotaLine], width: usize) {
    let mut amount_width = 0;
    for row in rows.iter_mut() {
        let Some(fields) = usage_fields(row) else {
            continue;
        };
        for index in [
            fields.name,
            fields.amount,
            fields.token,
            fields.cache,
            fields.tail,
        ] {
            row.spans[index].text = row.spans[index].text.trim().to_string();
        }
        amount_width = amount_width.max(display_width(&row.spans[fields.amount].text));
    }
    let amount_width = amount_width.max(4);
    for stage in 0..3 {
        let cache = stage < 2;
        let short = stage == 2;
        let name_width = if short {
            rows.iter()
                .filter_map(|row| {
                    usage_fields(row)
                        .map(|fields| display_width(short_name(row.spans[fields.name].text.trim())))
                })
                .max()
                .unwrap_or(USAGE_NAME_WIDTH)
                .max(4)
        } else {
            USAGE_NAME_WIDTH
        };
        let mut candidate = Vec::with_capacity(rows.len());
        let mut fits = true;
        for row in rows.iter() {
            match usage_fields(row) {
                Some(fields) => match build_usage_row(
                    row,
                    &fields,
                    width,
                    amount_width,
                    name_width,
                    cache,
                    short,
                ) {
                    Some(built) => candidate.push(built),
                    None => {
                        fits = false;
                        break;
                    }
                },
                None => candidate.push(row.clone()),
            }
        }
        if fits {
            rows.clone_from_slice(&candidate);
            return;
        }
    }
    // Truncation is the last resort; leave the compact fields unpadded.
}

fn usage_amount(state: &AppState, row: UsageRow) -> Option<String> {
    let daily =
        state.usage.show_daily(row, state.now) && state.usage.can_toggle_amount(row, state.now);
    let seconds = if daily {
        state.usage.daily_seconds(row, state.now)?
    } else {
        86_400
    };
    let amount = if row == UsageRow::DeepSeek {
        let spend = state.usage.snapshot.as_ref()?;
        let amount = format_cny_plain(spend.cost_cny * 86_400.0 / seconds as f64);
        if spend.unpriced {
            format!("{amount}?")
        } else {
            amount
        }
    } else {
        let summary = state.usage.summary(row, state.now)?;
        if daily {
            crate::usage::metering::Total {
                cost: summary.total.cost.saturating_mul(86_400) / seconds as u128,
                ..Default::default()
            }
            .amount()
        } else {
            summary.total.amount()
        }
    };
    Some(amount)
}

fn usage_tokens(state: &AppState, row: UsageRow, total: u64) -> String {
    let total = if state.usage.show_daily(row, state.now) {
        let seconds = state.usage.daily_seconds(row, state.now).unwrap_or(86_400);
        (u128::from(total) * 86_400 / u128::from(seconds)).min(u128::from(u64::MAX)) as u64
    } else {
        total
    };
    format_tokens(total)
}

/// Row amounts omit thousands separators so the column stays eight cells
/// (`¥9999.99`). The details/CLI formatter keeps grouping.
fn format_cny_plain(cost: f64) -> String {
    let cents = (cost * 100.0).round().max(0.0) as i64;
    format!("¥{}.{:02}", cents / 100, cents % 100)
}

fn codex_line(state: &AppState, row: UsageRow) -> QuotaLine {
    let (snapshot, window_label, scope, name_kind) = match row {
        UsageRow::CodexAccount => (
            state.usage.codex_account.as_ref(),
            state.usage.codex_account_window.label(),
            "账号",
            QuotaSpanKind::CodexAccount,
        ),
        UsageRow::CodexAll => (
            state.usage.codex_all.as_ref(),
            state.usage.codex_all_window.label(),
            "全部",
            QuotaSpanKind::CodexAll,
        ),
        UsageRow::DeepSeek => unreachable!("DeepSeek has its own row"),
    };
    let mut spans = vec![
        QuotaSpan::label(" "),
        QuotaSpan {
            text: format!("cx{window_label}"),
            kind: name_kind,
        },
        QuotaSpan::label(" "),
    ];
    if let Some(s) = state.usage.summary(row, state.now) {
        spans.push(QuotaSpan {
            text: usage_amount(state, row).unwrap(),
            kind: QuotaSpanKind::CodexEstimate {
                cents: ((s.total.cost + 5_000_000_000) / 10_000_000_000).min(u64::MAX as u128)
                    as u64,
                days: (s.end.saturating_sub(s.start).max(1) + 86_399) / 86_400,
                unpriced: s.total.unpriced > 0,
            },
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: usage_tokens(state, row, s.total.tokens.total()),
            kind: QuotaSpanKind::UsageTokens,
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: cache_ratio(s.total.tokens),
            kind: QuotaSpanKind::UsageCache,
        });
        if snapshot.is_some_and(|s| s.incomplete) {
            spans.push(QuotaSpan {
                text: " !".into(),
                kind: QuotaSpanKind::UsageWarning,
            });
        }
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: scope.into(),
            kind: QuotaSpanKind::UsageScope,
        });
    } else {
        spans.push(QuotaSpan {
            text: "待刷新".into(),
            kind: QuotaSpanKind::UsagePendingAmount,
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: "--".into(),
            kind: QuotaSpanKind::UsagePendingTokens,
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: cache_field("--"),
            kind: QuotaSpanKind::UsagePendingCache,
        });
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: scope.into(),
            kind: QuotaSpanKind::UsageScope,
        });
    }
    QuotaLine {
        spans,
        stale: snapshot.is_some_and(|s| state.now.saturating_sub(s.observed_at as u64) > 90),
    }
}

/// Style one field. Percentages carry the signal; names carry the
/// subscription's identity so the two blocks are told apart at a glance, and
/// everything else stays quiet.
fn span_style(state: &AppState, kind: QuotaSpanKind) -> Style {
    match kind {
        QuotaSpanKind::CodexAccount | QuotaSpanKind::CodexAll => {
            Style::default().fg(state.theme.agent_codex)
        }
        QuotaSpanKind::UsageScope => Style::default().fg(state.theme.text_muted),
        QuotaSpanKind::UsageWarning => Style::default().fg(state.theme.quota_low),
        QuotaSpanKind::UsageLeader => Style::default().fg(state.theme.border_inactive),
        QuotaSpanKind::UsageWindow(window) => Style::default()
            .fg(usage_window_color(state, window))
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::UsageWindowCycle => Style::default()
            .fg(state.theme.agent_deepseek)
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::UsageWindowMixed => Style::default()
            .fg(state.theme.text_muted)
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::UsageDailyHint => Style::default().fg(state.theme.text_inactive),
        QuotaSpanKind::UsageDailyActive => Style::default()
            .fg(state.theme.text_active)
            .add_modifier(Modifier::BOLD),
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
        QuotaSpanKind::UsagePendingAmount
        | QuotaSpanKind::UsagePendingTokens
        | QuotaSpanKind::UsagePendingCache => Style::default().fg(state.theme.text_inactive),
    }
}

/// Distinct header colors for the shared rolling window. These identify the
/// interval only; they do not imply quota health or spend.
fn usage_window_color(state: &AppState, window: crate::usage::Window) -> ratatui::style::Color {
    match window {
        crate::usage::Window::Today => state.theme.quota_good,
        crate::usage::Window::Last24Hours => state.theme.agent_codex,
        crate::usage::Window::SevenDays => state.theme.quota_warn,
        crate::usage::Window::ThirtyDays => state.theme.quota_low,
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
        QuotaSpanKind::CodexAccount => Some(UsageRow::CodexAccount),
        QuotaSpanKind::CodexAll => Some(UsageRow::CodexAll),
        _ => None,
    });
    let mut x = area.x;
    let mut targets = Vec::new();
    for span in &line.spans {
        let action = match span.kind {
            QuotaSpanKind::UsageWindow(_)
            | QuotaSpanKind::UsageWindowCycle
            | QuotaSpanKind::UsageWindowMixed => Some(UsageAction::CycleAll),
            QuotaSpanKind::UsageDailyHint | QuotaSpanKind::UsageDailyActive => {
                Some(UsageAction::ToggleDailyAll)
            }
            QuotaSpanKind::UsageName => Some(UsageAction::CycleDeepSeek),
            QuotaSpanKind::CodexAccount => Some(UsageAction::CycleCodexAccount),
            QuotaSpanKind::CodexAll => Some(UsageAction::CycleCodexAll),
            QuotaSpanKind::UsageCost { .. } | QuotaSpanKind::CodexEstimate { .. } => row
                .filter(|&row| state.usage.can_toggle_amount(row, state.now))
                .map(UsageAction::ToggleDaily),
            QuotaSpanKind::UsageTokens => row
                .filter(|&row| state.usage.can_toggle_tokens(row, state.now))
                .map(UsageAction::ToggleDaily),
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
                    | QuotaSpanKind::CodexAccount
                    | QuotaSpanKind::CodexAll
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
        assert_eq!(rendered[1].text(), " ds当天 ¥1.47 12.3M   98% 低谷");
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
        assert_eq!(rendered[1].text(), " ds当天 ¥1234.56 999.9M  100% 低谷");
        assert_eq!(display_width(&rendered[1].text()), usage_row_width());
        let bounded = lines_for_width(&state, QuotaLevel::Full, full_row_width() as u16);
        assert!(display_width(&bounded[1].text()) <= full_row_width());
        assert!(
            bounded[1]
                .spans
                .iter()
                .any(|s| matches!(s.kind, QuotaSpanKind::UsagePeriod { .. }))
        );
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds当天 ¥1234.56 999.9M  100% 低谷");
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
        assert_eq!(full[1].text(), " ds7d ¥9.87 71.3M   98% 低谷");
        // The period describes the current instant, never the historical window.
        assert!(
            full[1]
                .spans
                .iter()
                .any(|span| matches!(span.kind, QuotaSpanKind::UsagePeriod { .. }))
        );
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds7d ¥9.87 71.3M   98% 低谷");
    }

    #[test]
    fn spend_row_pending_and_unpriced_states_stay_honest() {
        let mut pending = AppState::new("%0".into());
        pending.now = 1_700_000_000;
        pending.usage.received = true;
        pending.quota.received_first_result = true;
        let rendered = lines(&pending, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " ds当天 ¥-- --    -- 低谷");

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
    #[test]
    fn usage_rows_are_account_all_then_deepseek_with_scope_tails() {
        let state = metering_state();
        let usage: Vec<_> = lines(&state, QuotaLevel::Full)
            .into_iter()
            .filter(|line| usage_fields(line).is_some())
            .collect();
        assert_eq!(usage.len(), 3);
        assert!(
            usage[0]
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::CodexAccount)
        );
        assert!(
            usage[1]
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::CodexAll)
        );
        assert!(
            usage[2]
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::UsageName)
        );
        assert_eq!(usage[0].spans.last().unwrap().text.trim(), "账号");
        assert_eq!(usage[1].spans.last().unwrap().text.trim(), "全部");
        assert_eq!(usage[2].spans.last().unwrap().text.trim(), "高峰");
    }

    #[test]
    fn daily_hint_lights_up_without_a_row_icon() {
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        fn header(state: &AppState) -> QuotaLine {
            lines_for_width(state, QuotaLevel::Full, 35)
                .into_iter()
                .find(|line| {
                    line.spans
                        .iter()
                        .any(|span| span.kind == QuotaSpanKind::UsageHeader)
                })
                .unwrap()
        }
        assert!(
            !header(&state)
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::UsageDailyActive)
        );
        state.usage.toggle_daily(UsageRow::DeepSeek, state.now);
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(
            header(&state)
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::UsageDailyActive)
        );
        // The row itself is unchanged; the header owns the daily indicator.
        let ds = usage_line(&state);
        assert!(matches!(
            ds.spans[3].kind,
            QuotaSpanKind::UsageCost { .. } | QuotaSpanKind::UsagePendingAmount
        ));
        assert_ne!(
            usage_tokens(&state, UsageRow::DeepSeek, 12_300_000),
            "12.3M"
        );
    }

    #[test]
    fn daily_mode_is_limited_to_multi_day_windows() {
        let mut state = metering_state();
        for window in [Window::Today, Window::Last24Hours] {
            state.usage.selected = window;
            state.usage.codex_all_window = window;
            state.usage.codex_account_window = crate::usage::CodexWindow::Recent(window);
            for row in [
                UsageRow::DeepSeek,
                UsageRow::CodexAccount,
                UsageRow::CodexAll,
            ] {
                assert!(!state.usage.can_toggle_tokens(row, state.now));
                state.usage.toggle_daily(row, state.now);
                assert!(!state.usage.show_daily(row, state.now));
            }
        }
        for window in [Window::SevenDays, Window::ThirtyDays] {
            state.usage.selected = window;
            state.usage.codex_all_window = window;
            state.usage.codex_account_window = crate::usage::CodexWindow::Recent(window);
            for row in [
                UsageRow::DeepSeek,
                UsageRow::CodexAccount,
                UsageRow::CodexAll,
            ] {
                assert!(state.usage.can_toggle_tokens(row, state.now));
                state.usage.toggle_daily(row, state.now);
                assert!(state.usage.show_daily(row, state.now));
                state.usage.toggle_daily(row, state.now);
                assert!(!state.usage.show_daily(row, state.now));
            }
        }
        // The cycle uses its elapsed seven-day span and is eligible.
        state.usage.codex_account_window = crate::usage::CodexWindow::Cycle;
        assert!(
            state
                .usage
                .can_toggle_tokens(UsageRow::CodexAccount, state.now)
        );
    }

    #[test]
    fn global_daily_hint_includes_height_hidden_rows() {
        let mut state = metering_state();
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.toggle_daily(UsageRow::CodexAll, state.now);
        let header = lines_for_width(&state, QuotaLevel::Cycle, 35)
            .into_iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.kind == QuotaSpanKind::UsageHeader)
            })
            .unwrap();
        assert!(
            header
                .spans
                .iter()
                .any(|span| span.kind == QuotaSpanKind::UsageDailyActive)
        );
        let target = field_targets(&state, &header, Rect::new(0, 20, 35, 1))
            .into_iter()
            .find(|target| target.action == UsageAction::ToggleDailyAll)
            .unwrap();
        state.layout.usage_rows = vec![20];
        state.layout.usage_targets = vec![target];
        assert!(state.handle_quota_click(20, target.rect.x));
        assert!(!state.usage.show_daily(UsageRow::CodexAll, state.now));
        assert!(!state.usage.show_daily(UsageRow::CodexAccount, state.now));
    }

    #[test]
    fn header_controls_cycle_interval_and_toggle_daily() {
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        fn header(state: &AppState) -> (QuotaLine, Vec<UsageClickTarget>) {
            let line = lines_for_width(state, QuotaLevel::Full, 35)
                .into_iter()
                .find(|line| {
                    line.spans
                        .iter()
                        .any(|span| span.kind == QuotaSpanKind::UsageHeader)
                })
                .unwrap();
            let targets = field_targets(state, &line, Rect::new(0, 20, 35, 1));
            (line, targets)
        }
        let (first, targets) = header(&state);
        let title_width = display_width(&first.spans[0].text) as u16;
        assert!(
            !targets.iter().any(|target| target.rect.x < title_width),
            "the Usage title itself is not clickable"
        );
        let daily = targets
            .iter()
            .find(|target| target.action == UsageAction::ToggleDailyAll)
            .unwrap()
            .rect;
        state.layout.usage_rows = vec![20];
        state.layout.usage_targets = targets;
        assert!(state.handle_quota_click(20, daily.x));
        for row in [
            UsageRow::DeepSeek,
            UsageRow::CodexAccount,
            UsageRow::CodexAll,
        ] {
            assert!(state.usage.show_daily(row, state.now));
        }

        // The interval control's hit box moves when the window label changes,
        // so rebuild the frame targets before clicking it.
        let (_, targets) = header(&state);
        let interval = targets
            .iter()
            .find(|target| target.action == UsageAction::CycleAll)
            .unwrap()
            .rect;
        state.layout.usage_targets = targets;
        assert!(state.handle_quota_click(20, interval.x));
        assert_eq!(state.usage.codex_all_window, Window::ThirtyDays);
        assert_eq!(
            state.usage.codex_account_window,
            crate::usage::CodexWindow::Recent(Window::ThirtyDays)
        );
    }

    fn metering_state() -> AppState {
        use crate::usage::metering::{Snapshot, Summary, Total};
        let mut state =
            state_with_spend(Window::Today, 1.47, (200_000, 12_000_000, 100_000), false);
        state.now = 1_793_000_000;
        let end = state.now + 3 * 86400;
        state.usage.cycle.lock().unwrap().end = end;
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
        let snapshot = Snapshot {
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
        };
        state.usage.codex_all = Some(snapshot.clone());
        state.usage.codex_account = Some(snapshot);
        state
    }

    fn frame_text(
        state: &AppState,
        level: QuotaLevel,
        width: u16,
        height: u16,
        _details: bool,
    ) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                let _ = render(f, state, f.area(), level);
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
                    codex_account: None,
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
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        35 超时

         codex 5h  62% wk  35% ·now 超 时
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        35 证书

         codex 5h  62% wk  35% ·now 证 书
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        28 DNS

         codex 5h  62% wk  35% ·DNS
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
        28 超时

         codex 5h  62% wk  35% ·超 时
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
        28 证书

         codex 5h  62% wk  35% ·证 书
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
        ");
    }

    #[test]
    fn spare_columns_preserve_countdown_then_expand_name() {
        let mut state = metering_state();
        let mut output = frame_text(&state, QuotaLevel::Compact, 28, 5, false);
        // A larger amount takes precedence over the expanded cycle name.
        state
            .usage
            .codex_all
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
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
        Large amount

         codex 5h  62% 2h10m wk  35%
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
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
        cx周    $128.40  18.2M  账 号
        cx当 天  $128.40  18.2M  全 部
        ds天      ¥1.47  12.3M  高 峰

        codex 5h  62% 2h10m wk  35%
        cx当 天  $128.40  18.2M  账 号
        cx当 天  $128.40  18.2M  全 部
        ds天      ¥1.47  12.3M  高 峰

        codex 5h  62% 2h10m wk  35%
        cx24h $128.40  18.2M   账 号
        cx24h $128.40  18.2M   全 部
        ds24h   ¥1.47  12.3M   高 峰

        codex 5h  62% 2h10m wk  35%
        cx7d $128.40  18.2M    账 号
        cx7d $128.40  18.2M    全 部
        ds7d   ¥1.47  12.3M    高 峰
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
        state.usage.codex_all_window = state.usage.codex_all_window.next();
        state.usage.codex_account_window =
            crate::usage::CodexWindow::Recent(state.usage.codex_all_window);
        output.push_str(&frame_text(&state, QuotaLevel::Full, 35, 7, false));
        insta::assert_snapshot!(output, @"


         Usage ────────────── 󰬁本 期  󰫮日 均
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  $128.40  18.2M   99%  账 号
        cx当 天  $128.40  18.2M   99%  全 部
        ds当 天    ¥1.47  12.3M   98%  高 峰


         Usage ──────────────────── 󰬁当 天
        codex 5h  62% 2h10m wk  35% 3d
        cx当 天  $128.40  18.2M   99%  账 号
        cx当 天  $128.40  18.2M   99%  全 部
        ds当 天    ¥1.47  12.3M   98%  高 峰


         Usage ───────────────────── 󰬁24h
        codex 5h  62% 2h10m wk  35% 3d
        cx24h  $128.40  18.2M   99%  账 号
        cx24h  $128.40  18.2M   99%  全 部
        ds24h    ¥1.47  12.3M   98%  高 峰


         Usage ──────────────── 󰬁7d 󰫮日 均
        codex 5h  62% 2h10m wk  35% 3d
        cx7d   $128.40  18.2M   99%  账 号
        cx7d   $128.40  18.2M   99%  全 部
        ds7d     ¥1.47  12.3M   98%  高 峰


         Usage ────────────── 󰬁混 选  󰫮日 均
        codex 5h  62% 2h10m wk  35% 3d
        cx30d  $128.40  18.2M   99%  账 号
        cx30d  $128.40  18.2M   99%  全 部
        ds当 天    ¥1.47  12.3M   98%  高 峰


         Usage ──────────────────── 󰬁当 天
        codex 5h  62% 2h10m wk  35% 3d
        cx当 天  $128.40  18.2M   99%  账 号
        cx当 天  $128.40  18.2M   99%  全 部
        ds当 天    ¥1.47  12.3M   98%  高 峰
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
                state.usage.codex_all_window = window;
                state.usage.codex_account_window = crate::usage::CodexWindow::Recent(window);
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


         Usage ───────────────────── 󰬁24h
        codex 5h  62% 4h44m wk  35% 3d2h
        cx24h  $128.40  18.2M   99%  账 号
        cx24h  $128.40  18.2M   99%  全 部
        ds24h    ¥1.47  12.3M   98%  低 谷

        codex 5h  62% 4h44m wk  35%
        cx24h $128.40  18.2M   账 号
        cx24h $128.40  18.2M   全 部
        ds24h   ¥1.47  12.3M   低 谷


         Usage ───────────────────── 󰬁24h
        codex 5h  62% 2h10m wk  35% 3d
        cx24h  $128.40  18.2M   99%  账 号
        cx24h  $128.40  18.2M   99%  全 部
        ds24h    ¥1.47  12.3M   98%  高 峰

        codex 5h  62% 2h10m wk  35%
        cx24h $128.40  18.2M   账 号
        cx24h $128.40  18.2M   全 部
        ds24h   ¥1.47  12.3M   高 峰
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
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        let mut output = String::new();
        for daily in [false, true] {
            if daily {
                for row in [
                    UsageRow::DeepSeek,
                    UsageRow::CodexAll,
                    UsageRow::CodexAccount,
                ] {
                    state.usage.toggle_daily(row, state.now);
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


          Usage ───────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35%
         cx7d $128.40  18.2M    账 号
         cx7d $128.40  18.2M    全 部
         ds7d   ¥1.47  12.3M    高 峰
        Width 35


          Usage ──────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $128.40  18.2M   99%  账 号
         cx7d   $128.40  18.2M   99%  全 部
         ds7d     ¥1.47  12.3M   98%  高 峰
        Width 45


          Usage ────────────────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $128.40  18.2M   99%            账 号
         cx7d   $128.40  18.2M   99%            全 部
         ds7d     ¥1.47  12.3M   98%            高 峰
        Width 60


          Usage ───────────────────────────────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $128.40  18.2M   99%                           账 号
         cx7d   $128.40  18.2M   99%                           全 部
         ds7d     ¥1.47  12.3M   98%                           高 峰
        Daily true
        Width 28


          Usage ───────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35%
         cx7d $18.34   2.6M     账 号
         cx7d $18.34   2.6M     全 部
         ds7d  ¥0.21   1.8M     高 峰
        Width 35


          Usage ──────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $18.34   2.6M   99%   账 号
         cx7d   $18.34   2.6M   99%   全 部
         ds7d    ¥0.21   1.8M   98%   高 峰
        Width 45


          Usage ────────────────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $18.34   2.6M   99%             账 号
         cx7d   $18.34   2.6M   99%             全 部
         ds7d    ¥0.21   1.8M   98%             高 峰
        Width 60


          Usage ───────────────────────────────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $18.34   2.6M   99%                            账 号
         cx7d   $18.34   2.6M   99%                            全 部
         ds7d    ¥0.21   1.8M   98%                            高 峰
        ");
    }

    #[test]
    fn daily_mode_is_row_level_and_survives_window_changes() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        // Ineligible windows stay total even when the stored preference is on.
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        state.usage.toggle_daily(UsageRow::DeepSeek, state.now);
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        for window in [Window::Today, Window::Last24Hours] {
            state.usage.selected = window;
            assert!(!state.usage.show_daily(UsageRow::DeepSeek, state.now));
        }
        state.usage.selected = Window::SevenDays;
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));

        // Each eligible row keeps its own preference.
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(!state.usage.show_daily(UsageRow::CodexAll, state.now));
        assert!(!state.usage.show_daily(UsageRow::CodexAccount, state.now));
        state.usage.toggle_daily(UsageRow::CodexAll, state.now);
        assert!(state.usage.show_daily(UsageRow::CodexAll, state.now));
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(!state.usage.show_daily(UsageRow::CodexAccount, state.now));
        state.usage.toggle_daily(UsageRow::CodexAccount, state.now);
        assert!(state.usage.show_daily(UsageRow::CodexAccount, state.now));

        // An unpriced amount stays total while its tokens follow the row mode.
        state.usage.snapshot.as_mut().unwrap().unpriced = true;
        assert!(!state.usage.can_toggle_amount(UsageRow::DeepSeek, state.now));
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert_eq!(usage_amount(&state, UsageRow::DeepSeek).unwrap(), "¥1.47?");
    }

    #[test]
    fn final_span_targets_exclude_status_padding_and_noninteractive_fields() {
        let mut state = metering_state();
        state.usage.spends = Some(std::array::from_fn(|_| {
            state.usage.snapshot.clone().unwrap()
        }));
        state.usage.cycle_all();
        state.usage.cycle_all();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        for width in [28, 35, 45, 60] {
            for level in [QuotaLevel::Full, QuotaLevel::Compact] {
                let lines = lines_for_width(&state, level, width);
                for (i, line) in lines.iter().enumerate() {
                    let area = Rect::new(7, 20 + i as u16, width, 1);
                    let targets = field_targets(&state, line, area);
                    let mut x = area.x;
                    for span in &line.spans {
                        let cycle = line
                            .spans
                            .iter()
                            .any(|s| s.kind == QuotaSpanKind::CodexAccount)
                            && state.usage.codex_account_window == crate::usage::CodexWindow::Cycle;
                        let active = matches!(
                            span.kind,
                            QuotaSpanKind::UsageWindow(_)
                                | QuotaSpanKind::UsageWindowCycle
                                | QuotaSpanKind::UsageWindowMixed
                                | QuotaSpanKind::UsageDailyHint
                                | QuotaSpanKind::UsageDailyActive
                                | QuotaSpanKind::UsageName
                                | QuotaSpanKind::CodexAccount
                                | QuotaSpanKind::CodexAll
                        ) || (!cycle
                            && matches!(
                                span.kind,
                                QuotaSpanKind::UsageCost { .. }
                                    | QuotaSpanKind::CodexEstimate { .. }
                                    | QuotaSpanKind::UsageTokens
                            ));
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
        let ds = usage_line(&state);
        // The amount cannot switch to a daily rate while unpriced; the tokens can.
        assert_eq!(
            field_targets(&state, &ds, Rect::new(0, 0, 60, 1))
                .iter()
                .filter(|t| matches!(t.action, UsageAction::ToggleDaily(_)))
                .count(),
            1
        );
        assert!(field_targets(&state, &ds, Rect::new(0, 0, 8, 1)).is_empty());
    }

    #[test]
    fn multi_day_windows_use_observed_elapsed_time_for_daily_rates() {
        let mut state = metering_state();
        state.now = (Window::Today.start(state.now as i64) + 12 * 3600) as u64;
        let end = state.now + 604_800 - 8 * 3600;
        state.usage.cycle.lock().unwrap().end = end;
        let snapshot = state.usage.codex_all.as_mut().unwrap();
        snapshot.observed_at = state.now as i64;
        snapshot.cycle_end = end;
        let cycle = snapshot.cycle.as_mut().unwrap();
        cycle.start = state.now as i64 - 8 * 3600;
        cycle.end = state.now as i64;
        state.usage.codex_account = state.usage.codex_all.clone();

        // Today is a partial day and 24h equals the total itself.
        for window in [Window::Today, Window::Last24Hours] {
            state.usage.selected = window;
            assert!(!state.usage.can_toggle_tokens(UsageRow::DeepSeek, state.now));
        }

        // 7d divides by its fixed seven days.
        state.usage.selected = Window::SevenDays;
        assert!(state.usage.can_toggle_tokens(UsageRow::DeepSeek, state.now));
        state.usage.toggle_daily(UsageRow::DeepSeek, state.now);
        assert_eq!(usage_amount(&state, UsageRow::DeepSeek).unwrap(), "¥0.21");
        assert_eq!(usage_tokens(&state, UsageRow::DeepSeek, 12_300_000), "1.8M");

        // The cycle uses its elapsed span: eight hours here, so three times.
        assert!(
            state
                .usage
                .can_toggle_tokens(UsageRow::CodexAccount, state.now)
        );
        state.usage.toggle_daily(UsageRow::CodexAccount, state.now);
        assert_eq!(
            usage_amount(&state, UsageRow::CodexAccount).unwrap(),
            "$385.20"
        );
        assert_eq!(
            usage_tokens(&state, UsageRow::CodexAccount, 18_200_000),
            "54.6M"
        );
        state.now += 3600;
        assert_eq!(
            usage_amount(&state, UsageRow::CodexAccount).unwrap(),
            "$385.20"
        );

        let cycle = state
            .usage
            .codex_account
            .as_mut()
            .unwrap()
            .cycle
            .as_mut()
            .unwrap();
        cycle.start = cycle.end;
        assert!(
            !state
                .usage
                .can_toggle_tokens(UsageRow::CodexAccount, state.now)
        );
    }

    #[test]
    fn row_daily_mode_does_not_require_prices() {
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        for row in [UsageRow::DeepSeek, UsageRow::CodexAll] {
            state.usage.toggle_daily(row, state.now);
            assert!(state.usage.show_daily(row, state.now));
        }
        state.usage.snapshot.as_mut().unwrap().unpriced = true;
        assert!(!state.usage.can_toggle_amount(UsageRow::DeepSeek, state.now));
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        let update = crate::usage::Update {
            history: Default::default(),
            spends: std::array::from_fn(|_| state.usage.snapshot.clone().unwrap()),
            codex_all: state.usage.codex_all.clone().unwrap(),
            codex_account: state.usage.codex_all.clone().unwrap(),
            codex_account_id: None,
        };
        state.usage.apply(update);
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(state.usage.show_daily(UsageRow::CodexAll, state.now));
        state.usage.toggle_daily(UsageRow::DeepSeek, state.now);
        assert!(!state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(state.usage.show_daily(UsageRow::CodexAll, state.now));
    }

    #[test]
    fn pending_fields_share_token_units_and_cache_icon_columns() {
        let mut state = metering_state();
        state.usage.snapshot = None;
        let recent = &mut state.usage.codex_all.as_mut().unwrap().recent
            [state.usage.codex_all_window.recent().index()];
        recent.total.tokens = crate::usage::Tokens {
            uncached: 0,
            cached: 2_000_000_000,
            output: 0,
        };
        let mut output = String::new();
        for width in [28, 35, 45] {
            let mut cache_columns = Vec::new();
            let mut unit_columns = Vec::new();
            for row in lines_for_width(&state, QuotaLevel::Full, width) {
                let Some(token) = row.spans.iter().position(|s| {
                    matches!(
                        s.kind,
                        QuotaSpanKind::UsageTokens | QuotaSpanKind::UsagePendingTokens
                    )
                }) else {
                    continue;
                };
                let token_text = &row.spans[token].text;
                let start: usize = row.spans[..token]
                    .iter()
                    .map(|s| display_width(&s.text))
                    .sum();
                unit_columns.push(start + display_width(token_text));
                let Some(cache) = row.spans.iter().position(|s| s.text.contains(CACHE_ICON)) else {
                    continue;
                };
                cache_columns.push(
                    row.spans[..cache]
                        .iter()
                        .map(|s| display_width(&s.text))
                        .sum::<usize>(),
                );
            }
            let targets: Vec<_> = lines_for_width(&state, QuotaLevel::Full, width)
                .iter()
                .flat_map(|line| field_targets(&state, line, Rect::new(0, 0, width, 1)))
                .collect();
            assert!(
                !targets
                    .iter()
                    .any(|t| matches!(t.action, UsageAction::ToggleDaily(UsageRow::DeepSeek)))
            );
            assert_eq!(
                unit_columns.len(),
                3,
                "pending row must participate in layout"
            );
            assert!(unit_columns.windows(2).all(|v| v[0] == v[1]));
            assert!(cache_columns.windows(2).all(|v| v[0] == v[1]));
            output.push_str(&format!(
                "Pending width {width}\n{}\n",
                frame_text(&state, QuotaLevel::Full, width, 7, false)
            ));
        }
        state.usage.codex_all.as_mut().unwrap().incomplete = true;
        let positions: Vec<_> = lines_for_width(&state, QuotaLevel::Full, 45)
            .iter()
            .filter_map(|row| {
                let cache = row.spans.iter().position(|s| s.text.contains(CACHE_ICON))?;
                Some(
                    row.spans[..cache]
                        .iter()
                        .map(|s| display_width(&s.text))
                        .sum::<usize>(),
                )
            })
            .collect();
        assert_eq!(positions.len(), 3);
        assert!(positions.windows(2).all(|v| v[0] == v[1]));
        output.push_str(&format!(
            "Incomplete\n{}\n",
            frame_text(&state, QuotaLevel::Full, 35, 7, false)
        ));
        state.usage.codex_all.as_mut().unwrap().incomplete = false;
        state.usage.snapshot = Some(Default::default());
        output.push_str(&format!(
            "Zero usage\n{}\n",
            frame_text(&state, QuotaLevel::Full, 35, 7, false)
        ));
        state.usage.cycle.lock().unwrap().end = 0;
        output.push_str(&format!(
            "Cycle unavailable\n{}\n",
            frame_text(&state, QuotaLevel::Full, 35, 7, false)
        ));
        insta::assert_snapshot!(output, @"
        Pending width 28


          Usage ─────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35%
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40   2.0B  全 部
         ds天        ¥--     --  高 峰
        Pending width 35


          Usage ────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40   2.0B  100%  全 部
         ds当 天      ¥--     --    --  高 峰
        Pending width 45


          Usage ──────────────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%            账 号
         cx当 天  $128.40   2.0B  100%            全 部
         ds当 天      ¥--     --    --            高 峰
        Incomplete


          Usage ────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx周    $128.40  18.2M         账 号
         cx当 天  $128.40   2.0B !       全 部
         ds天        ¥--     --         高 峰
        Zero usage


          Usage ────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40   2.0B  100%  全 部
         ds当 天    ¥0.00      0    --  高 峰
        Cycle unavailable


          Usage ──────────────────── 󰬁本 期
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期   待 刷 新      --    --  账 号
         cx当 天  $128.40   2.0B  100%  全 部
         ds当 天    ¥0.00      0    --  高 峰
        ");
    }

    #[test]
    fn usage_columns_align_and_tail_uses_the_right_edge_of_available_width() {
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        state.usage.toggle_daily(UsageRow::DeepSeek, state.now);
        state.usage.toggle_daily(UsageRow::CodexAll, state.now);
        for width in [35, 45, 60] {
            let mut numeric_edges = Vec::new();
            for row in lines_for_width(&state, QuotaLevel::Full, width) {
                let Some(fields) = usage_fields(&row) else {
                    continue;
                };
                let edge = |index| {
                    row.spans[..=index]
                        .iter()
                        .map(|s| display_width(&s.text))
                        .sum::<usize>()
                };
                numeric_edges.push((edge(fields.amount), edge(fields.token), edge(fields.tail)));
                assert_eq!(display_width(&row.text()), width as usize);
            }
            assert_eq!(numeric_edges.len(), 3);
            assert!(numeric_edges.windows(2).all(|pair| pair[0] == pair[1]));
        }
        let before: Vec<_> = lines_for_width(&state, QuotaLevel::Cycle, 35)
            .iter()
            .map(QuotaLine::text)
            .collect();
        state.usage.codex_all.as_mut().unwrap().recent
            [state.usage.codex_all_window.recent().index()]
        .total
        .cost *= 1_000_000;
        let after: Vec<_> = lines_for_width(&state, QuotaLevel::Cycle, 35)
            .iter()
            .map(QuotaLine::text)
            .collect();
        assert_eq!(
            before, after,
            "hidden recent values must not widen visible columns"
        );
    }

    #[test]
    fn usage_clicks_follow_resized_columns_and_cycle_supports_daily() {
        let mut state = metering_state();
        state.bottom_panel_height = 0;
        state.band_enabled = false;
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        let mut output = String::new();
        for width in [35, 45, 28, 35] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 8)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut state)).unwrap();
            for action in [
                UsageAction::ToggleDaily(UsageRow::DeepSeek),
                UsageAction::ToggleDaily(UsageRow::CodexAll),
            ] {
                let target = state
                    .layout
                    .usage_targets
                    .iter()
                    .find(|t| t.action == action)
                    .unwrap()
                    .rect;
                state.handle_mouse_click(target.y, target.x + target.width / 2);
                terminal.draw(|f| crate::ui::draw(f, &mut state)).unwrap();
            }
            state.usage.codex_account_window = crate::usage::CodexWindow::Cycle;
            terminal.draw(|f| crate::ui::draw(f, &mut state)).unwrap();
            assert!(
                state
                    .layout
                    .usage_targets
                    .iter()
                    .any(|t| matches!(t.action, UsageAction::ToggleDaily(UsageRow::CodexAccount)))
            );
            assert!(
                state
                    .layout
                    .usage_targets
                    .iter()
                    .any(|t| t.action == UsageAction::CycleCodexAccount)
            );
            state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
            output.push_str(&format!("Width {width}\n"));
            for row in terminal.backend().buffer().content.chunks(width as usize) {
                let mut text = String::new();
                let mut x = 0;
                while x < row.len() {
                    let symbol = row[x].symbol();
                    text.push_str(symbol);
                    x += display_width(symbol).max(1);
                }
                output.push_str(text.trim_end());
                output.push('\n');
            }
        }
        insta::assert_snapshot!(output, @"
        Width 35
           0   0   0   0   0   0 — ▾


          Usage ────────────── 󰬁混选 󰫮日均
         codex 5h  62% 2h10m wk  35% 3d
         cx本期 $128.40  18.2M   99%  账号
         cx7d    $18.34   2.6M   99%  全部
         ds7d     ¥0.21   1.8M   98%  高峰
        Width 45
           0   0   0   0   0   0           — ▾


          Usage ──────────────────────── 󰬁混选 󰫮日均
         codex 5h  62% 2h10m wk  35% 3d
         cx本期 $128.40  18.2M   99%            账号
         cx7d   $128.40  18.2M   99%            全部
         ds7d     ¥1.47  12.3M   98%            高峰
        Width 28
           0   0   0   0    — ▾


          Usage ─────── 󰬁混选 󰫮日均
         codex 5h  62% 2h10m wk  35%
         cx周 $128.40  18.2M    账号
         cx7d  $18.34   2.6M    全部
         ds7d   ¥0.21   1.8M    高峰
        Width 35
           0   0   0   0   0   0 — ▾


          Usage ────────────── 󰬁混选 󰫮日均
         codex 5h  62% 2h10m wk  35% 3d
         cx本期 $128.40  18.2M   99%  账号
         cx7d   $128.40  18.2M   99%  全部
         ds7d     ¥1.47  12.3M   98%  高峰
        ");
    }

    #[test]
    fn row_daily_mode_fits_narrow_layout_and_keeps_click_targets() {
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        for row in [
            UsageRow::DeepSeek,
            UsageRow::CodexAll,
            UsageRow::CodexAccount,
        ] {
            state.usage.toggle_daily(row, state.now);
            assert!(state.usage.show_daily(row, state.now));
        }
        let mut output = String::new();
        for width in [35, 45] {
            output.push_str(&format!(
                "Width {width}\n{}\n",
                frame_text(&state, QuotaLevel::Full, width, 7, false)
            ));
            let targets: Vec<_> = lines_for_width(&state, QuotaLevel::Full, width)
                .iter()
                .flat_map(|line| field_targets(&state, line, Rect::new(0, 0, width, 1)))
                .collect();
            for row in [
                UsageRow::DeepSeek,
                UsageRow::CodexAll,
                UsageRow::CodexAccount,
            ] {
                assert!(
                    targets
                        .iter()
                        .any(|t| t.action == UsageAction::ToggleDaily(row))
                );
            }
            for line in lines_for_width(&state, QuotaLevel::Full, width) {
                if usage_fields(&line).is_some() {
                    assert_eq!(display_width(&line.text()), width as usize);
                }
            }
        }
        // Height pressure drops the all-accounts row before the account row.
        let cycle_rows = lines(&state, QuotaLevel::Cycle);
        assert_eq!(
            cycle_rows
                .iter()
                .filter(|line| usage_fields(line).is_some())
                .count(),
            2
        );
        insta::assert_snapshot!(output, @"
        Width 35


          Usage ──────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $18.34   2.6M   99%   账 号
         cx7d   $18.34   2.6M   99%   全 部
         ds7d    ¥0.21   1.8M   98%   高 峰
        Width 45


          Usage ────────────────────────── 󰬁7d 󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx7d   $18.34   2.6M   99%             账 号
         cx7d   $18.34   2.6M   99%             全 部
         ds7d    ¥0.21   1.8M   98%             高 峰
        ");
    }

    #[test]
    fn rendered_amount_action_changes_only_its_row() {
        use std::sync::atomic::Ordering;
        let mut state = metering_state();
        state.usage.selected = Window::SevenDays;
        state.usage.codex_all_window = Window::SevenDays;
        state.usage.codex_account_window = crate::usage::CodexWindow::Recent(Window::SevenDays);
        let line = usage_line(&state);
        let targets = field_targets(&state, &line, Rect::new(7, 20, 45, 1));
        let amount = targets
            .iter()
            .find(|t| t.action == UsageAction::ToggleDaily(UsageRow::DeepSeek))
            .unwrap()
            .rect;
        state.layout.usage_rows = vec![20];
        state.layout.usage_targets = targets;
        let tokens = state.usage.snapshot.as_ref().unwrap().tokens;
        for col in [amount.x, amount.x + amount.width / 2, amount.right() - 1] {
            assert!(state.handle_quota_click(20, col));
        }
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
        assert!(!state.usage.show_daily(UsageRow::CodexAll, state.now));
        assert_eq!(state.usage.selected, Window::SevenDays);
        assert_eq!(state.usage.snapshot.as_ref().unwrap().tokens, tokens);
        assert!(state.handle_quota_click(20, amount.right()));
        assert!(state.usage.show_daily(UsageRow::DeepSeek, state.now));
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



          Usage ────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        Cycle




          Usage ────────────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         ds当 天    ¥1.47  12.3M   98%  高 峰
        Compact




         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        CompactCycle





         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         ds当 天    ¥1.47  12.3M   98%  高 峰
        QuotaOnly






         Quota
         codex 5h  62% 2h10m wk  35% 3d
        ");
    }

    #[test]
    fn quota_failure_keeps_local_cycle_fresh() {
        let mut state = metering_state();
        let before = codex_line(&state, UsageRow::CodexAccount);
        assert!(!before.stale);
        state.apply_quota_result(crate::quota::QuotaFetchResult {
            codex_account: None,
            codex: Some(Err(crate::quota::FetchError::with_kind(
                "timeout",
                FetchKind::Network,
            ))),
            kimi: None,
            forced: false,
        });
        let after = codex_line(&state, UsageRow::CodexAccount);
        assert!(!after.stale);
        assert_eq!(after.spans, before.spans);
        assert!(state.quota.codex.as_ref().unwrap().is_stale());
    }

    #[test]
    fn metering_pending_and_unknown_price_snapshot() {
        let mut state = metering_state();
        state.usage.cycle.lock().unwrap().end = state.now - 1;
        let snapshot = state.usage.codex_all.as_mut().unwrap();
        snapshot.incomplete = true;
        snapshot.recent[state.usage.codex_all_window.recent().index()]
            .total
            .cost = 0;
        snapshot.recent[state.usage.codex_all_window.recent().index()]
            .total
            .unpriced = 3;
        insta::assert_snapshot!(frame_text(&state,QuotaLevel::Full,35,8,false), @"



         Usage ──────────────────── 󰬁本 期
        codex 5h  62% 2h10m wk  35% 3d
        cx本 期  待 刷 新      --    --   账 号
        cx当 天    $--?  18.2M   99% ! 全 部
        ds当 天   ¥1.47  12.3M   98%   高 峰
        ");
    }

    #[test]
    fn history_health_marks_existing_header_without_adding_rows() {
        let mut state = metering_state();
        let rows = lines(&state, QuotaLevel::Full).len();
        state.usage.history.importing = true;
        let mut output = frame_text(&state, QuotaLevel::Full, 35, 8, false);
        assert_eq!(lines(&state, QuotaLevel::Full).len(), rows);
        state.usage.history.notice = Some("History write failed".into());
        output.push_str("\nNarrow error\n");
        output.push_str(&frame_text(&state, QuotaLevel::Full, 28, 8, false));
        assert_eq!(lines(&state, QuotaLevel::Full).len(), rows);
        insta::assert_snapshot!(output, @"



          Usage 导 入 中  ─────── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35% 3d
         cx本 期  $128.40  18.2M   99%  账 号
         cx当 天  $128.40  18.2M   99%  全 部
         ds当 天    ¥1.47  12.3M   98%  高 峰
        Narrow error



          Usage ! ───── 󰬁本 期  󰫮日 均
         codex 5h  62% 2h10m wk  35%
         cx周    $128.40  18.2M  账 号
         cx当 天  $128.40  18.2M  全 部
         ds天      ¥1.47  12.3M  高 峰
        ");
    }

    #[test]
    fn codex_amounts_use_daily_color_steps() {
        let state = metering_state();
        for kind in [QuotaSpanKind::CodexAccount, QuotaSpanKind::CodexAll] {
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
        let row = codex_line(&state, UsageRow::CodexAccount);
        assert!(row.spans.iter().any(|s| matches!(
            s.kind,
            QuotaSpanKind::UsageCache | QuotaSpanKind::UsagePendingCache
        ) && s.text == "  99%"));
        assert_eq!(
            span_style(&state, QuotaSpanKind::UsageCache).fg,
            Some(state.theme.quota_spend_cache)
        );
        assert_eq!(
            span_style(&state, QuotaSpanKind::UsageTokens).fg,
            Some(state.theme.quota_spend_tokens)
        );
    }

    #[test]
    fn metering_clicks_and_expired_cycle_never_relabel_old_usage() {
        let mut state = metering_state();
        state.layout.usage_rows = vec![3, 4];
        state.layout.usage_targets = [
            (3, UsageAction::CycleCodexAccount),
            (4, UsageAction::CycleCodexAll),
        ]
        .map(|(row, action)| UsageClickTarget {
            rect: Rect::new(1, row, 6, 1),
            action,
        })
        .into();
        assert!(state.handle_quota_click(4, 1));
        assert_eq!(state.usage.codex_all_window, Window::Last24Hours);
        assert!(state.handle_quota_click(3, 1));
        assert_eq!(
            state.usage.codex_account_window,
            crate::usage::CodexWindow::Recent(Window::Today)
        );
        state.now += 604800;
        assert!(state.usage.cycle_summary(state.now).is_none());
        assert_eq!(
            state.usage.codex_all.as_ref().unwrap().recent[1]
                .total
                .tokens
                .total(),
            18_200_000
        );
    }
}
