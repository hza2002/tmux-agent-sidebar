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
//! glyphs are double-width), landing on the same 35-cell budget.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::quota::{FetchKind, Subscription, SubscriptionQuota, format_countdown};
use crate::state::AppState;
use crate::ui::text::{display_width, truncate_to_width};
use crate::usage::{Window, format_cny, format_tokens};

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
/// block is designed around. The spend row has its own, slightly larger budget
/// in the extreme case; see [`usage_row_width`].
#[cfg(test)]
pub fn full_row_width() -> usize {
    subscription_row_width()
}

/// Budget for the spend row: indent, name column, amount (`¥1,234.56`),
/// tokens (`999.9M`), the cache ratio (`缓存100%`) and the period label
/// (`空闲`). One column past the subscription rows' budget in the extreme; a
/// day that expensive collapses the block to compact, which drops the period
/// label and fits again.
#[cfg(test)]
pub fn usage_row_width() -> usize {
    INDENT as usize
        + NAME_WIDTH
        + 1
        + 9
        + 1
        + USAGE_TOKEN_WIDTH
        + 1
        + display_width("缓存100%")
        + 1
        + display_width("空闲")
}

/// Palette slot for one field of a quota row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaSpanKind {
    /// The `Quota` section header.
    Header,
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
    /// Spend row: the DeepSeek name (`ds`, `ds7`, `ds30`).
    UsageName,
    /// Spend row: the window's cost, on the daily spend ladder.
    UsageCost { cents: u64, unpriced: bool },
    /// Spend row: total tokens for the window.
    UsageTokens,
    /// Spend row: share of input tokens that hit DeepSeek's cache.
    UsageCache,
    /// Spend row: current DeepSeek period (`空闲` / `高峰`).
    UsagePeriod { peak: bool },
    /// Spend row before its first result arrives.
    UsagePending,
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
    Hidden,
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
fn pending_row(subscription: Subscription, with_countdown: bool) -> QuotaLine {
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
        if with_countdown {
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan::pending(PENDING_VALUE.len()));
        }
    }
    QuotaLine {
        spans,
        stale: false,
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
            spans.push(QuotaSpan::countdown(text));
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

/// ` ds    ¥1.91 14.1M 空闲`.
///
/// One row for the whole fork-owned spend feature. The compact level drops the
/// period label — the token total and the amount are what the user acts on,
/// and a range window mixes both periods anyway.
fn usage_line(state: &AppState, level: QuotaLevel) -> QuotaLine {
    let usage = &state.usage;
    let mut spans = vec![
        QuotaSpan::label(" ".repeat(INDENT as usize)),
        QuotaSpan {
            text: format!("{:<NAME_WIDTH$}", usage.selected.name()),
            kind: QuotaSpanKind::UsageName,
        },
        QuotaSpan::label(" "),
    ];
    match &usage.snapshot {
        Some(spend) => {
            let amount = format_cny(spend.cost_cny);
            let amount = if spend.unpriced {
                // An unmapped DeepSeek model id: the tokens are real, the cost
                // is a lower bound, and saying so beats a silent zero.
                format!("{amount}?")
            } else {
                amount
            };
            spans.push(QuotaSpan {
                text: amount,
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
                text: "¥--".to_string(),
                kind: QuotaSpanKind::UsagePending,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: format!("{:>USAGE_TOKEN_WIDTH$}", "--"),
                kind: QuotaSpanKind::UsagePending,
            });
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan {
                text: "缓存--".to_string(),
                kind: QuotaSpanKind::UsagePending,
            });
        }
    }
    if level == QuotaLevel::Full && usage.selected == Window::Today {
        let now = state.now as i64;
        let peak = crate::usage::is_peak(now);
        spans.push(QuotaSpan::label(" "));
        spans.push(QuotaSpan {
            text: if peak { "高峰" } else { "空闲" }.to_string(),
            kind: QuotaSpanKind::UsagePeriod { peak },
        });
    }
    QuotaLine {
        spans,
        stale: false,
    }
}

/// Share of input tokens served from DeepSeek's cache. Cache hits bill at 2% of
/// the miss price, so this number explains most of why a day cost what it did.
/// `--` when nothing was billed rather than an invented 0%.
fn cache_ratio(tokens: crate::usage::Tokens) -> String {
    let input = tokens.uncached + tokens.cached;
    if input == 0 {
        return "缓存--".to_string();
    }
    let percent = ((tokens.cached as f64 / input as f64) * 100.0).round() as u64;
    format!("缓存{percent}%")
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

/// Lines for the current quota state at the requested collapse level.
/// Empty when there is nothing to render.
pub fn lines(state: &AppState, level: QuotaLevel) -> Vec<QuotaLine> {
    if level == QuotaLevel::Hidden {
        return Vec::new();
    }
    let rendered = state.quota.rendered();
    let show_usage = state.usage.received;
    if rendered.is_empty() && !state.quota.is_pending() && !show_usage {
        return Vec::new();
    }
    let with_countdown = level == QuotaLevel::Full;
    let mut lines = Vec::with_capacity(rendered.len() + 1);
    if with_countdown {
        lines.push(QuotaLine {
            spans: vec![QuotaSpan {
                text: format!(" {TITLE}"),
                kind: QuotaSpanKind::Header,
            }],
            stale: false,
        });
    }
    // The user's own spend leads the block: it is the number they act on, and
    // the subscription rows below it are reference.
    if show_usage {
        lines.push(usage_line(state, level));
    }
    if rendered.is_empty() && state.quota.is_pending() {
        // No snapshot yet: reserve the rows with placeholders so the block has
        // its final shape from the first frame instead of appearing seconds
        // later, once the first fetch comes back.
        lines.extend(
            [Subscription::Codex, Subscription::Kimi]
                .into_iter()
                .map(|subscription| pending_row(subscription, with_countdown)),
        );
    } else {
        for (subscription, quota) in rendered {
            lines.push(subscription_row(
                subscription,
                quota,
                state.now,
                with_countdown,
            ));
        }
    }
    lines
}

/// Line index of the spend row inside the block. It sits directly under the
/// header at the full level and at the top when the header is gone, which is
/// what makes its click target computable without a second pass over `lines`.
fn spend_row_index(level: QuotaLevel) -> u16 {
    u16::from(level == QuotaLevel::Full)
}

/// Style one field. Percentages carry the signal; names carry the
/// subscription's identity so the two blocks are told apart at a glance, and
/// everything else stays quiet.
fn span_style(state: &AppState, kind: QuotaSpanKind) -> Style {
    match kind {
        QuotaSpanKind::Header => Style::default().fg(state.theme.section_title),
        QuotaSpanKind::Name(Subscription::Codex) => Style::default().fg(state.theme.agent_codex),
        QuotaSpanKind::Name(Subscription::Kimi) => Style::default().fg(state.theme.agent_kimi),
        QuotaSpanKind::Label => Style::default().fg(state.theme.text_muted),
        QuotaSpanKind::Percent(remaining) => Style::default()
            .fg(quota_color(state, remaining))
            .add_modifier(Modifier::BOLD),
        QuotaSpanKind::Pending => Style::default().fg(state.theme.text_inactive),
        QuotaSpanKind::Countdown => Style::default().fg(state.theme.text_inactive),
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

/// Render the quota block at the bottom of `list_area`. Returns the inclusive
/// row range the block painted, so the caller can record it as a click target:
/// any row in the block forces a refetch, because the header only exists at the
/// full level.
/// Screen rows the block painted. `spend` is the DeepSeek line, whose click
/// cycles the window instead of forcing a refetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaRows {
    pub block: (u16, u16),
    pub spend: Option<u16>,
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
    let lines = lines(state, level);
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

    for (index, line) in lines.iter().enumerate() {
        let row = Rect {
            x: area.x,
            y: area.y + index as u16,
            width: area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(styled_line(state, line, row.width)), row);
    }
    Some(QuotaRows {
        block: (area.y, area.bottom().saturating_sub(1)),
        spend: state
            .usage
            .received
            .then(|| area.y + spend_row_index(level)),
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
        assert_eq!(rendered[1].text(), " ds    ¥1.47  12.3M 缓存98% 空闲");
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
        assert_eq!(rendered[1].text(), " ds    ¥1,234.56 999.9M 缓存100% 空闲");
        assert_eq!(display_width(&rendered[1].text()), usage_row_width());
        assert!(usage_row_width() > full_row_width());
        // Compact drops the period label, which is what lets the block survive
        // a pane too narrow for the worst row.
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds    ¥1,234.56 999.9M 缓存100%");
        assert!(display_width(&compact[0].text()) < usage_row_width());
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
        assert_eq!(full[1].text(), " ds7   ¥9.87  71.3M 缓存98%");
        // A range mixes both periods, so it never claims to be peak or off-peak.
        assert!(
            !full[1]
                .spans
                .iter()
                .any(|span| matches!(span.kind, QuotaSpanKind::UsagePeriod { .. })),
            "only the today view labels the current period"
        );
        let compact = lines(&state, QuotaLevel::Compact);
        assert_eq!(compact[0].text(), " ds7   ¥9.87  71.3M 缓存98%");
    }

    #[test]
    fn spend_row_pending_and_unpriced_states_stay_honest() {
        let mut pending = AppState::new("%0".into());
        pending.now = 1_700_000_000;
        pending.usage.received = true;
        pending.quota.received_first_result = true;
        let rendered = lines(&pending, QuotaLevel::Full);
        assert_eq!(rendered[1].text(), " ds    ¥--     -- 缓存-- 空闲");

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
}
