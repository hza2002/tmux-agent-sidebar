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
//! weekly window's last day).

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::quota::{Subscription, SubscriptionQuota, format_countdown};
use crate::state::AppState;
use crate::ui::text::{display_width, truncate_to_width};

/// Leading space keeps the quota block visually aligned with the agent rows
/// (which start with the `┃` marker column).
const INDENT: u16 = 1;
/// Width of the `codex` / `kimi` subscription-name column.
const NAME_WIDTH: usize = 5;
/// Width of the `5h` / `wk` window column.
const WINDOW_WIDTH: usize = 2;
/// Width of the right-aligned percentage field (`100%` or ` 61%`).
const PERCENT_WIDTH: usize = 4;
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

/// Width of the widest row the full block can emit, derived from the field
/// widths so the budget cannot drift silently. `23h59m` is the longest
/// countdown a weekly window can show; a 5-hour window tops out at `4h59m`.
#[cfg(test)]
pub fn full_row_width() -> usize {
    let group = |countdown: usize| WINDOW_WIDTH + 1 + PERCENT_WIDTH + 1 + countdown;
    INDENT as usize + NAME_WIDTH + 1 + group(5) + display_width(WINDOW_SEPARATOR) + group(6)
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
            spans.push(QuotaSpan::label(" "));
            spans.push(QuotaSpan::countdown(countdown));
        }
    }
    if let Some(age) = quota.age_marker() {
        spans.push(QuotaSpan::countdown(format!(" ·{age} ago")));
    }
    QuotaLine {
        spans,
        stale: quota.is_stale(),
    }
}

/// Lines for the current quota state at the requested collapse level.
/// Empty when there is nothing to render.
pub fn lines(state: &AppState, level: QuotaLevel) -> Vec<QuotaLine> {
    if level == QuotaLevel::Hidden {
        return Vec::new();
    }
    let rendered = state.quota.rendered();
    if rendered.is_empty() && !state.quota.is_pending() {
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
    if rendered.is_empty() {
        // No snapshot yet: reserve the rows with placeholders so the block has
        // its final shape from the first frame instead of appearing seconds
        // later, once the first fetch comes back.
        lines.extend(
            [Subscription::Codex, Subscription::Kimi]
                .into_iter()
                .map(|subscription| pending_row(subscription, with_countdown)),
        );
        return lines;
    }
    for (subscription, quota) in rendered {
        lines.push(subscription_row(
            subscription,
            quota,
            state.now,
            with_countdown,
        ));
    }
    lines
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
pub fn render(
    frame: &mut Frame,
    state: &AppState,
    list_area: Rect,
    level: QuotaLevel,
) -> Option<(u16, u16)> {
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
    Some((area.y, area.bottom().saturating_sub(1)))
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
        assert_eq!(display_width(&rendered[1].text()), 35);
        assert_eq!(display_width(&rendered[1].text()), full_row_width());
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
        // The weekly window is absent; the row keeps its `5h` label and no
        // trailing separator.
        assert_eq!(rendered[1].text(), " kimi  5h  24% 41m");
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
        // The untrustworthy countdowns are replaced by the snapshot age.
        assert_eq!(rendered[1].text(), " kimi  5h  61% wk  83% ·45m ago");
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
}
