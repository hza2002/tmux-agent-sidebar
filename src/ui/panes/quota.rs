//! Bottom-anchored quota block for the agents panel.
//!
//! The block renders only in the rows the agent list does not use (`spare`),
//! so the list stays first-class and scrolls exactly as it did before the
//! feature. It collapses full → compact → hidden as space runs out.

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
/// Cells in the remaining-quota bar.
const BAR_CELLS: usize = 10;
/// Width of the ` 61%` percentage field (` ` + up to three digits + `%`).
const PERCENT_WIDTH: usize = 4;
/// Width of the `codex` / `kimi ` subscription-name column.
const NAME_WIDTH: usize = 6;
/// Width of the right-aligned `5h` / `wk` column, including the single cell
/// gap that keeps the bar from hugging the label.
const WINDOW_WIDTH: usize = 4;
const TITLE: &str = "Quota";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaColor {
    Green,
    Yellow,
    Red,
}

/// Color by remaining level. Thresholds are fixed by the design:
/// `> 50%` green, `11–50%` yellow, `<= 10%` red.
fn color_for(remaining_percent: u8) -> QuotaColor {
    if remaining_percent > 50 {
        QuotaColor::Green
    } else if remaining_percent > 10 {
        QuotaColor::Yellow
    } else {
        QuotaColor::Red
    }
}

/// A rendered quota line. Public so tests can assert the view model without
/// rendering a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaLine {
    pub text: String,
    pub remaining_percent: u8,
    pub resets_at: Option<u64>,
    pub stale: bool,
}

/// Collapse level for the quota block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotaLevel {
    Full,
    Compact,
    Hidden,
}

/// A filled/empty block bar representing *remaining* quota.
fn bar(remaining_percent: u8) -> String {
    let filled = ((remaining_percent as usize) * BAR_CELLS).div_ceil(100);
    let filled = filled.min(BAR_CELLS);
    format!("{}{}", "▓".repeat(filled), "░".repeat(BAR_CELLS - filled))
}

fn reset_suffix(now: u64, resets_at: Option<u64>) -> String {
    match format_countdown(now, resets_at) {
        Some(countdown) => format!(" {countdown}"),
        None => String::new(),
    }
}

/// Line width that always fits: indent + name + window + bar + percent.
const BAR_LINE_FIXED_WIDTH: usize =
    INDENT as usize + NAME_WIDTH + WINDOW_WIDTH + BAR_CELLS + PERCENT_WIDTH;

fn full_line(
    name: &str,
    window_label: &str,
    remaining_percent: u8,
    resets_at: Option<u64>,
    now: u64,
) -> QuotaLine {
    let suffix = reset_suffix(now, resets_at);
    let mut text = " ".repeat(INDENT as usize);
    // `codex` / `kimi `, then the right-aligned `5h` / `wk` label, then one
    // gap cell so the bars line up under each other without hugging the text.
    text.push_str(name);
    text.push_str(&" ".repeat(NAME_WIDTH.saturating_sub(display_width(name))));
    text.push_str(&" ".repeat(WINDOW_WIDTH.saturating_sub(display_width(window_label))));
    text.push_str(window_label);
    text.push(' ');
    text.push_str(&format!(
        " {} {:>3}%{}",
        bar(remaining_percent),
        remaining_percent,
        suffix
    ));
    QuotaLine {
        text,
        remaining_percent,
        resets_at,
        stale: false,
    }
}

/// The 5-hour line keeps the `5h` label; the weekly line leaves the label
/// column blank so the two bars line up under each other.
fn subscription_full_lines(
    subscription: Subscription,
    quota: &SubscriptionQuota,
    now: u64,
) -> Vec<(String, u8, Option<u64>, bool)> {
    let mut lines = Vec::new();
    let mut push = |window_label: &str, window: Option<&crate::quota::QuotaWindow>| {
        if let Some(window) = window {
            let line = full_line(
                subscription.label(),
                window_label,
                window.remaining_percent,
                window.resets_at,
                now,
            );
            lines.push((
                line.text,
                line.remaining_percent,
                line.resets_at,
                quota.is_stale(),
            ));
        } else {
            lines.push((String::new(), 0, None, quota.is_stale()));
        }
    };
    push("5h", quota.windows.iter().find(|w| w.label == "5h"));
    push("wk", quota.windows.iter().find(|w| w.label == "wk"));
    lines
}

fn compact_line(subscription: Subscription, quota: &SubscriptionQuota, now: u64) -> QuotaLine {
    let mut text = format!("{}{}", " ".repeat(INDENT as usize), subscription.label());
    let mut worst = 100u8;
    let mut soonest: Option<u64> = None;
    for window in &quota.windows {
        text.push_str(&format!(
            " {} {:>3}%{}",
            window.label,
            window.remaining_percent,
            reset_suffix(now, window.resets_at),
        ));
        worst = worst.min(window.remaining_percent);
        soonest = match (soonest, window.resets_at) {
            (Some(current), Some(candidate)) => Some(current.min(candidate)),
            (None, candidate) => candidate,
            (current, None) => current,
        };
    }
    QuotaLine {
        text,
        remaining_percent: worst,
        resets_at: soonest,
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
    if rendered.is_empty() {
        return Vec::new();
    }
    match level {
        QuotaLevel::Compact => rendered
            .into_iter()
            .map(|(subscription, quota)| compact_line(subscription, quota, state.now))
            .collect(),
        QuotaLevel::Full => {
            let mut lines = vec![QuotaLine {
                text: format!(" {TITLE}"),
                remaining_percent: 100,
                resets_at: None,
                stale: false,
            }];
            for (subscription, quota) in rendered {
                for (text, remaining_percent, resets_at, stale) in
                    subscription_full_lines(subscription, quota, state.now)
                {
                    if text.is_empty() {
                        continue;
                    }
                    lines.push(QuotaLine {
                        text,
                        remaining_percent,
                        resets_at,
                        stale,
                    });
                }
            }
            lines
        }
        // `Hidden` already returned above; the arm keeps the match exhaustive.
        QuotaLevel::Hidden => Vec::new(),
    }
}

fn line_style(state: &AppState, line: &QuotaLine) -> Style {
    let color = match color_for(line.remaining_percent) {
        QuotaColor::Green => state.theme.status_running,
        QuotaColor::Yellow => state.theme.accent,
        QuotaColor::Red => state.theme.status_error,
    };
    let style = Style::default().fg(color);
    if line.stale {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

fn rendered_text(state: &AppState, line: &QuotaLine) -> String {
    if !line.stale {
        return line.text.clone();
    }
    // A stale snapshot trades the reset countdown for the age marker: the
    // countdown is no longer trustworthy, and the age is what tells the user
    // the row is old. This also keeps the marker on screen in narrow sidebars.
    let Some(age) = state
        .quota
        .rendered()
        .into_iter()
        .find_map(|(_, quota)| quota.is_stale().then(|| quota.age_marker()).flatten())
    else {
        return line.text.clone();
    };
    let mut text = line.text.clone();
    if let Some(countdown) = format_countdown(state.now, line.resets_at) {
        text = text.replace(&format!(" {countdown}"), "");
    }
    format!("{text} ·{age}")
}

/// Render the quota block at the bottom of `list_area`. Returns the row of the
/// `Quota` header when the full block rendered, so the caller can record the
/// click target.
pub fn render(
    frame: &mut Frame,
    state: &AppState,
    list_area: Rect,
    level: QuotaLevel,
) -> Option<u16> {
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

    for (index, line) in lines.iter().enumerate() {
        let row = Rect {
            x: area.x,
            y: area.y + index as u16,
            width: area.width,
            height: 1,
        };
        let text = rendered_text(state, line);
        let text = if row.width as usize > BAR_LINE_FIXED_WIDTH {
            text
        } else {
            truncate_to_width(&text, row.width as usize)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, line_style(state, line)))),
            row,
        );
    }
    (level == QuotaLevel::Full).then_some(area.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{QuotaFetch, QuotaWindow};
    use crate::state::AppState;

    fn window(label: &str, remaining_percent: u8, resets_at: Option<u64>) -> QuotaWindow {
        QuotaWindow {
            label: label.into(),
            remaining_percent,
            resets_at,
        }
    }

    fn state_with_kimi() -> AppState {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000_000;
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![
                window("5h", 61, Some(state.now + 133 * 60_000)),
                window("wk", 83, Some(state.now + 4 * 24 * 60 * 60_000)),
            ])),
        );
        state
    }

    #[test]
    fn bar_represents_remaining_quota() {
        assert_eq!(bar(100), "▓▓▓▓▓▓▓▓▓▓");
        assert_eq!(bar(0), "░░░░░░░░░░");
        assert_eq!(bar(61), "▓▓▓▓▓▓▓░░░");
        assert_eq!(bar(1), "▓░░░░░░░░░");
    }

    #[test]
    fn color_thresholds_match_the_design() {
        assert_eq!(color_for(100), QuotaColor::Green);
        assert_eq!(color_for(51), QuotaColor::Green);
        assert_eq!(color_for(50), QuotaColor::Yellow);
        assert_eq!(color_for(11), QuotaColor::Yellow);
        assert_eq!(color_for(10), QuotaColor::Red);
        assert_eq!(color_for(0), QuotaColor::Red);
    }

    #[test]
    fn full_lines_render_header_and_two_rows_per_subscription() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 3);
        assert_eq!(rendered[0].text, " Quota");
        // The weekly row repeats the subscription name so a single-subscription
        // sidebar still identifies which bar belongs to which window.
        assert_eq!(rendered[1].text, " kimi    5h  ▓▓▓▓▓▓▓░░░  61% 2h13m");
        assert_eq!(rendered[2].text, " kimi    wk  ▓▓▓▓▓▓▓▓▓░  83% 4d");
    }

    #[test]
    fn compact_lines_fold_both_windows_into_one_row() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Compact);
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].text, " kimi 5h  61% 2h13m wk  83% 4d");
    }

    #[test]
    fn hidden_level_renders_nothing() {
        let state = state_with_kimi();
        assert!(lines(&state, QuotaLevel::Hidden).is_empty());
    }

    #[test]
    fn no_subscriptions_render_nothing() {
        let state = AppState::new("%0".into());
        assert!(lines(&state, QuotaLevel::Full).is_empty());
        assert!(lines(&state, QuotaLevel::Compact).is_empty());
    }

    #[test]
    fn missing_weekly_window_keeps_the_five_hour_row() {
        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000_000;
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window(
                "5h",
                24,
                Some(state.now + 41 * 60_000),
            )])),
        );
        let rendered = lines(&state, QuotaLevel::Full);
        assert_eq!(rendered.len(), 2);
        assert!(rendered[1].text.contains("5h"));
        // The weekly row is absent; the 5-hour row keeps its `5h` label.
        assert_eq!(rendered[1].text, " kimi    5h  ▓▓▓░░░░░░░  24% 41m");
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
        let text = rendered_text(&state, &rendered[1]);
        assert_eq!(
            text,
            " kimi    5h  \u{2593}\u{2593}\u{2593}\u{2593}\u{2593}\u{2593}\u{2593}\u{2591}\u{2591}\u{2591}  61% ·45m"
        );
        assert!(text.contains('·'), "age marker missing: {text}");
        assert!(
            line_style(&state, &rendered[1])
                .add_modifier
                .contains(Modifier::DIM),
            "stale rows must be dimmed"
        );
    }

    #[test]
    fn colors_come_from_the_theme() {
        let state = state_with_kimi();
        let rendered = lines(&state, QuotaLevel::Full);
        // 61% → green slot, 83% → green slot.
        assert_eq!(
            line_style(&state, &rendered[1]).fg,
            Some(state.theme.status_running)
        );
        let mut red_state = state_with_kimi();
        red_state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![window("5h", 5, None)])),
        );
        let red_lines = lines(&red_state, QuotaLevel::Full);
        assert_eq!(
            line_style(&red_state, &red_lines[1]).fg,
            Some(red_state.theme.status_error)
        );
    }
}
