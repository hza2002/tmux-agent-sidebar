mod activity;
mod git;
mod syntax;

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::state::{AppState, BottomTab, Focus};

use super::text::display_width;

/// Height one tab wants at `width`, including the two border rows. The band
/// uses this per block to size the stacked Activity/Git pair; an empty tab
/// still reports a single content row so its title bar stays reachable.
pub(super) fn content_height(state: &AppState, width: u16, tab: BottomTab) -> u16 {
    // `Block::inner` removes one column of border on each side.
    let inner_w = width.saturating_sub(2) as usize;
    let lines = match tab {
        BottomTab::Activity => activity::content_height(state, inner_w),
        BottomTab::GitStatus => git::content_height(state, inner_w),
    };
    lines.saturating_add(2)
}

/// Render one tab as its own bordered block in the agents panel. The focused
/// block (the one `bottom_tab` names) takes the accent border; the other stays
/// muted. Blocks paint whatever rows they were given — the tab renderers clip
/// or scroll — which is what lets the stacked pair compress instead of
/// disappearing.
pub(super) fn draw_band_block(frame: &mut Frame, state: &mut AppState, area: Rect, tab: BottomTab) {
    let focused = state.bottom_tab == tab;
    let border_color = if focused {
        state.theme.accent
    } else {
        state.theme.border_inactive
    };
    let title = match tab {
        BottomTab::Activity => " Activity ",
        BottomTab::GitStatus => " Git ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        // Match the bottom panel's rounded chrome.
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    match tab {
        BottomTab::Activity => activity::draw_activity_content(frame, state, inner),
        BottomTab::GitStatus => git::draw_git_content(frame, state, inner),
    }
}

fn render_centered(frame: &mut Frame, area: Rect, text: &str, color: Color) {
    // Vertically center: pad with empty lines above
    let top_pad = area.height.saturating_sub(1) / 2;
    let mut lines: Vec<Line<'_>> = Vec::new();
    for _ in 0..top_pad {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(text, Style::default().fg(color))));
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

pub fn draw_bottom(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let theme = &state.theme;
    let border_color = if state.focus_state.focus == Focus::ActivityLog {
        theme.accent
    } else {
        theme.border_inactive
    };

    let tab_title = build_tab_title(state);

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let title_spans = tab_title.spans;
    let title_dw = title_spans
        .iter()
        .map(|span| display_width(&span.content))
        .sum::<usize>();
    let top_fill_len = (area.width as usize).saturating_sub(title_dw + 4);
    let mut top_line_spans = vec![Span::styled("╭ ", Style::default().fg(border_color))];
    top_line_spans.extend(title_spans);
    top_line_spans.push(Span::styled(
        format!(" {}╮", "─".repeat(top_fill_len)),
        Style::default().fg(border_color),
    ));
    let top_line = Line::from(top_line_spans);
    let top_rect = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(Paragraph::new(top_line), top_rect);

    let bottom_line = Line::from(Span::styled(
        format!("╰{}╯", "─".repeat((area.width as usize).saturating_sub(2))),
        Style::default().fg(border_color),
    ));
    let bottom_rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    frame.render_widget(Paragraph::new(bottom_line), bottom_rect);

    match state.bottom_tab {
        BottomTab::Activity => activity::draw_activity_content(frame, state, inner),
        BottomTab::GitStatus => git::draw_git_content(frame, state, inner),
    }
}

fn build_tab_title(state: &AppState) -> Line<'static> {
    let theme = &state.theme;
    let activity_style = if state.bottom_tab == BottomTab::Activity {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.text_muted)
    };

    let git_style = if state.bottom_tab == BottomTab::GitStatus {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.text_muted)
    };

    let sep_style = Style::default().fg(theme.border_inactive);

    Line::from(vec![
        Span::styled("Activity", activity_style),
        Span::styled(" \u{2502} ", sep_style),
        Span::styled("Git", git_style),
    ])
}

#[cfg(test)]
mod tests {
    use crate::ui::text::truncate_to_width;

    #[test]
    fn truncate_to_width_short() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn truncate_to_width_truncated() {
        let result = truncate_to_width("hello world", 8);
        assert!(result.ends_with('…'));
        assert!(result.len() <= 10); // 7 chars + ellipsis in bytes
    }
}
