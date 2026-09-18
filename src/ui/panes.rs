mod click_targets;
mod filter_bar;
mod popups;
mod quota;
mod row;
mod row_collector;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::state::{AppState, Focus, PopupState, SpawnField};

pub(super) const SPAWN_BUTTON: &str = "+";

/// Width of the clickable region around the `×` marker. One column of
/// slack on either side makes it comfortable to hit without stealing
/// clicks from adjacent branch text.
pub(super) const REMOVE_MARKER_HIT_WIDTH: u16 = 3;

use super::text::{display_width, truncate_to_width};

/// Compute a popup Rect centered inside `area`, clamped so it never
/// exceeds the parent (a narrow sidebar can't end up with a popup wider
/// than its own pane, which used to crash ratatui).
fn center_popup(area: Rect, desired_width: u16, desired_height: u16) -> Rect {
    let width = desired_width.min(area.width);
    let height = desired_height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

/// Place a popup directly below screen row `anchor_y`, left-aligned to
/// `area`, and shift upward when it would overflow the bottom edge.
fn anchor_below(area: Rect, anchor_y: u16, desired_width: u16, desired_height: u16) -> Rect {
    let width = desired_width.min(area.width);
    let height = desired_height.min(area.height);
    let below = anchor_y.saturating_add(1);
    let bottom = area.y.saturating_add(area.height);
    let y = if below + height <= bottom {
        below
    } else {
        bottom.saturating_sub(height).max(area.y)
    };
    Rect::new(area.x, y, width, height)
}

struct PaneLayout {
    header_area: Rect,
    list_area: Rect,
    /// Collapse level the quota block wants for the current spare rows.
    quota_level: quota::QuotaLevel,
    /// Rows reserved for the quota block.
    quota_height: u16,
    /// Blocks reserved for the tab band, top-first (Activity above Git). Empty
    /// when the band is hidden.
    band_blocks: Vec<(Rect, crate::state::BottomTab)>,
    /// Filler band above the quota block, `None` when the pet is hidden.
    pet_area: Option<Rect>,
}

impl PaneLayout {
    fn compute(area: Rect) -> Self {
        let header_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1.min(area.height),
        };
        let list_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        Self {
            header_area,
            list_area,
            quota_level: quota::QuotaLevel::Hidden,
            quota_height: 0,
            band_blocks: Vec::new(),
            pet_area: None,
        }
    }

    /// Split the agents chunk into the header, the scrollable list, the tab
    /// band, the filler band for the pet, and the bottom-anchored quota block.
    ///
    /// The agent list is first-class: everything below it is funded only by
    /// rows the list does not use. When the list fills the chunk they all
    /// disappear and the list scrolls exactly as it did before this feature.
    ///
    /// Stacked from the bottom up: the quota block (small, always wanted), the
    /// tab band (`band_height` rows, only when the active tab has content),
    /// then the pet.
    fn compute_with_filler(
        area: Rect,
        rendered: usize,
        quota_full: u16,
        quota_compact: u16,
        band_wishes: (u16, u16),
        pet_enabled: bool,
        pet_unavailable: bool,
    ) -> Self {
        let (activity_wish, git_wish) = band_wishes;
        let mut layout = Self::compute(area);
        let rendered = rendered as u16;
        // Space the filler may use is what the agent list does not need. Giving
        // the band the rows of "silent" panes instead was tried and reverted:
        // it shrank the list's viewport, so the list could never be read to the
        // end and keeping the selected row visible pushed the group headers off
        // the top of the panel.
        let spare = layout.list_area.height.saturating_sub(rendered);
        // The quota block is pinned to the bottom of the panel: it shows the
        // subscription windows the user is actually spending, so it keeps its
        // rows even when the agent list has to scroll. Only the width decides
        // whether the full or the compact variant fits.
        layout.quota_level = if quota_full > 0 {
            quota::QuotaLevel::Full
        } else if quota_compact > 0 {
            quota::QuotaLevel::Compact
        } else {
            quota::QuotaLevel::Hidden
        };
        layout.quota_height = match layout.quota_level {
            quota::QuotaLevel::Full => quota_full,
            quota::QuotaLevel::Compact => quota_compact,
            quota::QuotaLevel::Hidden => 0,
        }
        .min(layout.list_area.height);

        // The band sits directly above the quota block and is funded by the
        // rows the list does not need. Each block compresses row by row: it
        // takes as many of the rows it asked for as are free, and only
        // disappears when even its minimum (title bar plus one content row)
        // does not fit. Activity is allocated first so a long Git list cannot
        // starve it; Git then fills what is left.
        let room = spare.saturating_sub(layout.quota_height);
        // Both blocks keep their minimum when there is room for it, then share
        // the leftover row by row between whatever each still wants ("water
        // filling"). Allocating Activity first would let a long activity log
        // push Git out entirely.
        let min = crate::ui::TAB_BAND_MIN_HEIGHT;
        // A wish of zero means the block is not wanted at all.
        let mut activity_h = if activity_wish >= min && room >= min {
            min
        } else {
            0
        };
        let mut git_h = if git_wish >= min && room.saturating_sub(activity_h) >= min {
            min
        } else {
            0
        };
        // Share the leftover evenly; whatever one block cannot use (its wish is
        // already satisfied) falls through to the other.
        let leftover = room.saturating_sub(activity_h + git_h);
        let half = leftover / 2;
        let activity_extra = (leftover - half).min(activity_wish.saturating_sub(activity_h));
        let git_extra = half.min(git_wish.saturating_sub(git_h));
        activity_h += activity_extra;
        git_h += git_extra;
        let mut rest = leftover - activity_extra - git_extra;
        if rest > 0 {
            let extra = rest.min(activity_wish.saturating_sub(activity_h));
            activity_h += extra;
            rest -= extra;
            git_h += rest.min(git_wish.saturating_sub(git_h));
        }
        let mut bottom = layout
            .list_area
            .bottom()
            .saturating_sub(layout.quota_height);
        let mut band_rows = 0;
        let place = |layout: &mut Self,
                     bottom: &mut u16,
                     band_rows: &mut u16,
                     tab: crate::state::BottomTab,
                     height: u16| {
            if height < crate::ui::TAB_BAND_MIN_HEIGHT {
                return;
            }
            *bottom = bottom.saturating_sub(height);
            layout.band_blocks.push((
                Rect {
                    x: layout.list_area.x,
                    y: *bottom,
                    width: layout.list_area.width,
                    height,
                },
                tab,
            ));
            *band_rows += height;
        };
        place(
            &mut layout,
            &mut bottom,
            &mut band_rows,
            crate::state::BottomTab::GitStatus,
            git_h,
        );
        place(
            &mut layout,
            &mut bottom,
            &mut band_rows,
            crate::state::BottomTab::Activity,
            activity_h,
        );
        // Top-first order for rendering and hit testing.
        layout.band_blocks.reverse();

        let pet_rows_needed = crate::ui::PET_SCENE_HEIGHT;
        let pet_band = spare.saturating_sub(layout.quota_height + band_rows);
        if pet_enabled && !pet_unavailable && pet_band >= pet_rows_needed {
            let height = pet_rows_needed.min(pet_band);
            let bottom = layout
                .list_area
                .bottom()
                .saturating_sub(layout.quota_height + band_rows);
            // The band is the *unpainted* tail of the list area: it starts
            // after the rows the panes still need so the scrollable
            // `Paragraph` cannot overdraw the pet.
            let top = layout
                .list_area
                .y
                .saturating_add(rendered)
                .min(bottom.saturating_sub(height));
            layout.pet_area = Some(Rect {
                x: layout.list_area.x,
                y: top,
                width: layout.list_area.width,
                height,
            });
        }
        layout
    }

    /// Rows of the list area the agent list may actually paint. The quota,
    /// band, and pet overdraw only the spare rows a scrollable `Paragraph`
    /// cannot reach, so the scroll clamp uses this reduced height while the
    /// widget itself still renders into `list_area`.
    fn scroll_visible_height(&self) -> u16 {
        let reserved = self.quota_height
            + self
                .band_blocks
                .iter()
                .map(|(area, _)| area.height)
                .sum::<u16>()
            + self.pet_area.map(|area| area.height).unwrap_or(0);
        self.list_area.height.saturating_sub(reserved)
    }
}

/// Minimum agents-panel height the expanded Vercel-style spawn modal
/// needs. Below this the popup falls back to a compact label-less
/// layout to avoid clipping rows (the default 20-row bottom panel can
/// leave only ~10 rows for the agents panel on short terminals).
const SPAWN_MODAL_EXPANDED_MIN_HEIGHT: u16 = 12;

/// Border rows contributed to the total popup height (top + bottom).
const POPUP_BORDER_ROWS: u16 = 2;

// Row offsets inside the inner area of the compact popup.
const COMPACT_TASK_Y: u16 = 0;
const COMPACT_AGENT_Y: u16 = 1;
const COMPACT_MODE_Y: u16 = 2;
const COMPACT_ERROR_Y: u16 = 3;

// Row offsets inside the inner area of the expanded Vercel popup.
// Each section is label → value with a blank spacer between them.
const EXP_TASK_LABEL_Y: u16 = 1;
const EXP_TASK_VALUE_Y: u16 = 2;
const EXP_AGENT_LABEL_Y: u16 = 4;
const EXP_AGENT_VALUE_Y: u16 = 5;
const EXP_MODE_LABEL_Y: u16 = 7;
const EXP_MODE_VALUE_Y: u16 = 8;
const EXP_ERROR_Y: u16 = 10;

pub(super) fn render_spawn_input_popup(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let PopupState::SpawnInput {
        input,
        agent_idx,
        mode_idx,
        field,
        anchor_y,
        error,
        ..
    } = &state.popup
    else {
        return;
    };
    let input = input.clone();
    let field = *field;
    let anchor_y = *anchor_y;
    let error = error.clone();
    let agent = crate::worktree::AGENTS
        .get(*agent_idx)
        .copied()
        .unwrap_or("");
    let mode = crate::worktree::modes_for(agent)
        .get(*mode_idx)
        .copied()
        .unwrap_or("");
    let theme = &state.theme;

    let popup_width = area.width.min(32).max(area.width.min(14));
    let compact = area.height < SPAWN_MODAL_EXPANDED_MIN_HEIGHT;
    let content_rows: u16 = if compact { 4 } else { 10 };
    let error_rows: u16 = if error.is_some() { 1 } else { 0 };
    let popup_height = content_rows + error_rows + POPUP_BORDER_ROWS;
    let popup_rect = match anchor_y {
        Some(y) => anchor_below(area, y, popup_width, popup_height),
        None => center_popup(area, popup_width, popup_height),
    };
    state.popup.set_spawn_input_area(Some(popup_rect));

    frame.render_widget(Clear, popup_rect);
    let title_trunc = truncate_to_width(
        " Spawn worktree ",
        popup_rect.width.saturating_sub(2) as usize,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            title_trunc,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    // Row 0 is left blank as a top gutter in expanded mode. Content
    // rows get one column of left padding so they don't hug the border.
    let render_at = |frame: &mut Frame, y_offset: u16, spans: Vec<Span<'_>>| {
        if y_offset < inner.height {
            let row = Rect::new(
                inner.x + 1,
                inner.y + y_offset,
                inner.width.saturating_sub(2),
                1,
            );
            frame.render_widget(Paragraph::new(Line::from(spans)), row);
        }
    };

    let label_style = |target: SpawnField| {
        let base = Style::default().add_modifier(Modifier::BOLD);
        if field == target {
            base.fg(theme.accent)
        } else {
            base.fg(theme.text_muted)
        }
    };
    let value_style = |target: SpawnField| {
        if field == target {
            Style::default().fg(theme.text_active)
        } else {
            Style::default().fg(theme.text_muted)
        }
    };

    let content_width = inner.width.saturating_sub(2) as usize;
    let visible_input = tail_fit(&input, content_width.saturating_sub(1));
    let mut task_spans: Vec<Span<'_>> =
        vec![Span::styled(visible_input, value_style(SpawnField::Task))];
    if field == SpawnField::Task {
        task_spans.push(Span::styled("█", Style::default().fg(theme.accent)));
    }
    let agent_value = truncate_to_width(agent, content_width);
    let mode_value = truncate_to_width(mode, content_width);
    let error_spans = error.as_ref().map(|err| {
        vec![Span::styled(
            truncate_to_width(err, content_width),
            Style::default().fg(theme.status_error),
        )]
    });

    if compact {
        render_at(frame, COMPACT_TASK_Y, task_spans);
        render_at(
            frame,
            COMPACT_AGENT_Y,
            vec![Span::styled(agent_value, value_style(SpawnField::Agent))],
        );
        render_at(
            frame,
            COMPACT_MODE_Y,
            vec![Span::styled(mode_value, value_style(SpawnField::Mode))],
        );
        if let Some(err) = error_spans {
            render_at(frame, COMPACT_ERROR_Y, err);
        }
    } else {
        render_at(
            frame,
            EXP_TASK_LABEL_Y,
            vec![Span::styled("NAME", label_style(SpawnField::Task))],
        );
        render_at(frame, EXP_TASK_VALUE_Y, task_spans);
        render_at(
            frame,
            EXP_AGENT_LABEL_Y,
            vec![Span::styled("AGENT", label_style(SpawnField::Agent))],
        );
        render_at(
            frame,
            EXP_AGENT_VALUE_Y,
            vec![Span::styled(agent_value, value_style(SpawnField::Agent))],
        );
        render_at(
            frame,
            EXP_MODE_LABEL_Y,
            vec![Span::styled("MODE", label_style(SpawnField::Mode))],
        );
        render_at(
            frame,
            EXP_MODE_VALUE_Y,
            vec![Span::styled(mode_value, value_style(SpawnField::Mode))],
        );
        if let Some(err) = error_spans {
            render_at(frame, EXP_ERROR_Y, err);
        }
    }
}

/// Keep only the trailing `max_width` display cells of `text` so the
/// cursor at the end stays visible in a narrow input box. Prepends `…`
/// when truncation is applied.
fn tail_fit(text: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if max_width == 0 {
        return String::new();
    }
    if display_width(text) <= max_width {
        return text.to_string();
    }
    let budget = max_width.saturating_sub(1);
    let mut taken = 0usize;
    let mut byte_start = text.len();
    for (i, ch) in text.char_indices().rev() {
        let w = ch.width().unwrap_or(0);
        if taken + w > budget {
            break;
        }
        taken += w;
        byte_start = i;
    }
    let mut out = String::with_capacity(3 + (text.len() - byte_start));
    out.push('…');
    out.push_str(&text[byte_start..]);
    out
}

pub(super) fn render_remove_confirm_popup(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let (branch, error) = match &state.popup {
        PopupState::RemoveConfirm { branch, error, .. } => (branch.clone(), error.clone()),
        _ => return,
    };
    let theme = &state.theme;

    // Narrow-friendly: put the branch in the title, keep option rows
    // short enough to fit in ~16 columns. Reserve an extra row when
    // an inline error is present.
    let popup_height: u16 = if error.is_some() { 7 } else { 6 };
    let popup_rect = center_popup(area, area.width.min(28), popup_height);
    state.popup.set_remove_confirm_area(Some(popup_rect));

    frame.render_widget(Clear, popup_rect);
    let title_text = format!(" {branch} ");
    let title = truncate_to_width(&title_text, popup_rect.width.saturating_sub(2) as usize);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.status_error))
        .title(Span::styled(title, Style::default().fg(theme.status_error)));
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let render_row = |frame: &mut Frame, y_offset: u16, text: &str, style: Style| {
        if y_offset < inner.height {
            let row = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
            let truncated = truncate_to_width(text, row.width as usize);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(truncated, style))),
                row,
            );
        }
    };

    render_row(
        frame,
        0,
        "[y] remove worktree",
        Style::default().fg(theme.status_error),
    );
    render_row(
        frame,
        1,
        "[c] close window only",
        Style::default().fg(theme.text_active),
    );
    render_row(
        frame,
        2,
        "[n] cancel",
        Style::default().fg(theme.text_muted),
    );
    if let Some(err) = error {
        render_row(frame, 4, &err, Style::default().fg(theme.status_error));
    }
}

pub(super) fn render_repo_popup(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let theme = &state.theme;
    let repos = state.repo_popup_names();
    let query = state.repo_popup_query().to_string();

    let max_name_len = repos
        .iter()
        .map(|r| display_width(r))
        .max()
        .unwrap_or_else(|| display_width("No matches"));
    let max_content_width = max_name_len.max(display_width(&query) + 2);
    // Width: padding(1 left + 1 right) + name + borders(2)
    let popup_width = (max_content_width + 4).max(10).min(area.width as usize) as u16;
    // Search row + at least one result/status row + two border rows.
    let result_rows = repos.len().max(1) as u16;
    let popup_height = (result_rows + 3).min(area.height.saturating_sub(1));

    // Right-aligned, below the single header row.
    let popup_x = area.x + area.width.saturating_sub(popup_width);
    let popup_y = area.y + 1;

    let popup_rect = Rect::new(popup_x, popup_y, popup_width, popup_height);
    state.popup.set_repo_area(Some(popup_rect));

    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent));
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    let inner_width = inner.width as usize;
    if inner.height == 0 {
        return;
    }

    let query_prefix = "/ ";
    let shown_query = truncate_to_width(
        &query,
        inner_width.saturating_sub(display_width(query_prefix)),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(query_prefix, Style::default().fg(theme.accent)),
            Span::styled(shown_query, Style::default().fg(theme.text_active)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let visible_results = inner.height.saturating_sub(1) as usize;
    if repos.is_empty() {
        if visible_results > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " No matches",
                    Style::default().fg(theme.text_muted),
                ))),
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
            );
        }
        return;
    }

    let selected = state.repo_popup_selected().min(repos.len() - 1);
    let start = state.repo_popup_result_start(visible_results);
    for (visible_i, (i, name)) in repos.iter().enumerate().skip(start).enumerate() {
        if visible_i >= visible_results {
            break;
        }

        let is_highlighted = i == selected;
        let is_current = state.repo_popup_choice_is_current(i);

        let truncated = truncate_to_width(name, inner_width.saturating_sub(1));
        let text = format!(" {}", truncated);
        let text_dw = display_width(&text);
        let padding = " ".repeat(inner_width.saturating_sub(text_dw));

        let style = if is_highlighted {
            Style::default()
                .fg(theme.text_active)
                .bg(theme.selection_bg)
        } else if is_current {
            Style::default().fg(theme.text_active)
        } else {
            Style::default().fg(theme.text_muted)
        };

        let line_rect = Rect::new(inner.x, inner.y + 1 + visible_i as u16, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("{}{}", text, padding),
                style,
            ))),
            line_rect,
        );
    }
}

fn render_header_into(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let (line, notices_btn_col, repo_btn_col) = filter_bar::render_header(state, area.width);
    state.layout.header_width = area.width;
    state.notices.button_col = notices_btn_col;
    state.layout.repo_button_col = repo_btn_col;
    frame.render_widget(Paragraph::new(vec![line]), area);
}

fn compute_scroll_offset(state: &mut AppState, total_lines: usize, visible_height: u16) -> usize {
    state.scrolls.panes.total_lines = total_lines;
    state.scrolls.panes.visible_height = visible_height as usize;

    // Auto-scroll to keep selected agent visible
    if state.focus_state.sidebar_focused && state.focus_state.focus == Focus::Panes {
        let mut first_line: Option<usize> = None;
        let mut last_line: Option<usize> = None;
        for (i, mapping) in state.layout.line_to_row.iter().enumerate() {
            if *mapping == Some(state.global.selected_pane_row) {
                if first_line.is_none() {
                    first_line = Some(i);
                }
                last_line = Some(i);
            }
        }
        if let (Some(first), Some(last)) = (first_line, last_line) {
            let visible_h = visible_height as usize;
            let offset = state.scrolls.panes.offset;
            if first < offset {
                state.scrolls.panes.offset = first.saturating_sub(1);
            } else if last >= offset + visible_h {
                state.scrolls.panes.offset = (last + 1).saturating_sub(visible_h);
            }
        }
    }

    state.scrolls.panes.offset
}

fn render_pane_rows(
    frame: &mut Frame,
    lines: Vec<Line<'static>>,
    scroll_offset: usize,
    list_area: Rect,
) {
    let paragraph = Paragraph::new(lines).scroll((scroll_offset as u16, 0));
    frame.render_widget(paragraph, list_area);
}

fn render_flash_banner_into(frame: &mut Frame, state: &mut AppState, area: Rect) {
    // Render flash banner (spawn / remove feedback) before popups so
    // popups stay on top.
    if let Some(text) = state.take_flash() {
        let flash_y = area.y + area.height.saturating_sub(1);
        let flash_rect = Rect::new(area.x, flash_y, area.width, 1);
        frame.render_widget(Clear, flash_rect);
        let theme = &state.theme;
        let color = if text.contains("failed") {
            theme.status_error
        } else {
            theme.accent
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color)))),
            flash_rect,
        );
    }
}

/// Rows the full quota block may reserve in a `width`-column pane. The full
/// rows carry two countdowns each, so a pane too narrow for the rows *as they
/// are right now* collapses to the percentage-only rows instead of truncating
/// numbers off the end of every subscription.
fn quota_full_height_for(width: u16, state: &AppState, quota_enabled: bool) -> u16 {
    let rows_fit = quota::lines(state, quota::QuotaLevel::Full)
        .iter()
        .all(|line| display_width(&line.text()) <= width as usize);
    if !quota_enabled || !rows_fit {
        0
    } else {
        state.quota_full_height()
    }
}

/// Rows the tab band may reserve in the agents panel.
///
/// The band hosts the active bottom tab when the bottom panel is hidden. It is
/// all-or-nothing by design: either the tab has room to be readable or it is
/// not drawn at all. `0` means "no band" — the bottom panel is visible, the
/// user turned the band off, or the active tab has nothing to show.
fn band_wishes_for(state: &AppState, width: u16) -> (u16, u16) {
    if !state.band_enabled || state.bottom_panel_height > 0 {
        return (0, 0);
    }
    // Each block asks for its own content height, floored at the minimum so its
    // title bar never disappears. The layout clamps the pair to the free rows.
    let wish =
        |tab| super::bottom::content_height(state, width, tab).max(crate::ui::TAB_BAND_MIN_HEIGHT);
    (
        wish(crate::state::BottomTab::Activity),
        wish(crate::state::BottomTab::GitStatus),
    )
}

pub fn draw_agents(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let quota_enabled = state.quota_enabled;
    let quota_full = quota_full_height_for(area.width, state, quota_enabled);
    let quota_compact = if quota_enabled {
        state.quota_compact_height()
    } else {
        0
    };
    let (activity_wish, git_wish) = band_wishes_for(state, area.width);
    let quota_empty = state.quota.subscription_count() == 0;

    let row_collector::CollectedRows {
        lines,
        line_to_row,
        pending_spawn,
        pending_remove,
    } = row_collector::collect(state, area.width);

    let layout = PaneLayout::compute_with_filler(
        area,
        lines.len(),
        quota_full,
        quota_compact,
        (activity_wish, git_wish),
        state.pet_enabled,
        quota_empty,
    );

    render_header_into(frame, state, layout.header_area);
    state.layout.line_to_row = line_to_row;
    let visible_height = layout.scroll_visible_height();
    let scroll_offset = compute_scroll_offset(state, lines.len(), visible_height);
    click_targets::materialize(
        state,
        pending_spawn,
        pending_remove,
        scroll_offset,
        layout.list_area,
    );
    render_pane_rows(frame, lines, scroll_offset, layout.list_area);

    state.layout.quota_block_rows =
        quota::render(frame, state, layout.list_area, layout.quota_level);
    for (rect, tab) in &layout.band_blocks {
        // Same reason as the quota block: a block can sit on rows the list
        // painted, so clear them before drawing the frame and its content.
        frame.render_widget(Clear, *rect);
        super::bottom::draw_band_block(frame, state, *rect, *tab);
    }
    state.layout.band_blocks = layout.band_blocks.clone();
    if let Some(pet_area) = layout.pet_area {
        let running_count = state.running_count();
        crate::ui::pet::draw_pet(frame, state, pet_area, running_count);
    }

    render_flash_banner_into(frame, state, area);
    popups::render_if_open(frame, state, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_layout_splits_area_into_header_and_list() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let layout = PaneLayout::compute(area);
        assert_eq!(layout.header_area.x, 0);
        assert_eq!(layout.header_area.y, 0);
        assert_eq!(layout.header_area.width, 40);
        assert_eq!(layout.header_area.height, 1);
        assert_eq!(layout.list_area.y, 1);
        assert_eq!(layout.list_area.height, 19);
        assert_eq!(layout.list_area.width, 40);
    }

    #[test]
    fn pane_layout_handles_tiny_area() {
        // Only 1 row available: the header gets it and the list collapses to 0.
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 1,
        };
        let layout = PaneLayout::compute(area);
        assert_eq!(layout.header_area.height, 1);
        assert_eq!(layout.list_area.height, 0);
    }

    #[test]
    fn pane_layout_handles_zero_height() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 0,
        };
        let layout = PaneLayout::compute(area);
        assert_eq!(layout.header_area.height, 0);
        assert_eq!(layout.list_area.height, 0);
    }

    #[test]
    fn pane_layout_respects_non_zero_origin() {
        let area = Rect {
            x: 5,
            y: 10,
            width: 30,
            height: 15,
        };
        let layout = PaneLayout::compute(area);
        assert_eq!(layout.header_area.x, 5);
        assert_eq!(layout.header_area.y, 10);
        assert_eq!(layout.list_area.x, 5);
        assert_eq!(layout.list_area.y, 11);
        assert_eq!(layout.list_area.height, 14);
    }

    /// Area wide enough for the filler tests; only the height matters.
    fn filler_area(height: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 34,
            height,
        }
    }

    #[test]
    fn quota_reserves_full_rows_only_when_the_rows_fit() {
        use crate::quota::{QuotaFetch, QuotaWindow, Subscription};

        let mut state = AppState::new("%0".into());
        state.now = 1_700_000_000;
        assert_eq!(
            quota_full_height_for(quota::full_row_width() as u16, &state, true),
            3,
            "before the first fetch the block reserves placeholder rows"
        );
        // Once a result has landed the reservations follow the real data.
        state.quota.received_first_result = true;
        assert_eq!(
            quota_full_height_for(quota::full_row_width() as u16, &state, true),
            0,
            "a resolved block with no subscriptions reserves nothing"
        );
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![
                QuotaWindow {
                    label: "5h".into(),
                    remaining_percent: 61,
                    resets_at: Some(state.now + 2 * 60 * 60 + 13 * 60),
                },
                QuotaWindow {
                    label: "wk".into(),
                    remaining_percent: 41,
                    resets_at: Some(state.now + 4 * 24 * 60 * 60),
                },
            ])),
        );

        // " kimi  5h  61% 2h13m wk  41% 4d" is 31 cells.
        let row: Vec<_> = quota::lines(&state, quota::QuotaLevel::Full);
        let row_width = display_width(&row[1].text()) as u16;
        assert_eq!(row_width, 31);
        assert_eq!(
            quota_full_height_for(row_width, &state, true),
            state.quota_full_height()
        );
        assert_eq!(
            quota_full_height_for(row_width - 1, &state, true),
            0,
            "a pane narrower than the rows collapses to compact rows"
        );

        // The collapse follows the rows on screen, not a worst-case constant:
        // a weekly countdown of `23h59m` is six cells and needs the full
        // default width.
        state.quota.apply(
            Subscription::Kimi,
            Ok(QuotaFetch::Available(vec![
                QuotaWindow {
                    label: "5h".into(),
                    remaining_percent: 61,
                    resets_at: Some(state.now + 2 * 60 * 60 + 13 * 60),
                },
                QuotaWindow {
                    label: "wk".into(),
                    remaining_percent: 41,
                    resets_at: Some(state.now + 23 * 60 * 60 + 59 * 60),
                },
            ])),
        );
        let rows = quota::lines(&state, quota::QuotaLevel::Full);
        let longest = display_width(&rows[1].text()) as u16;
        assert_eq!(longest, quota::full_row_width() as u16);
        assert_eq!(
            quota_full_height_for(longest, &state, true),
            state.quota_full_height(),
            "the widest realistic row still fits the default 35-column sidebar"
        );
        assert_eq!(
            quota_full_height_for(longest, &state, false),
            0,
            "the @sidebar_quota off switch still wins"
        );
    }

    #[test]
    fn filler_picks_full_when_all_rows_fit() {
        // 20 rows − 1 header = 19 list rows; 2 rendered leaves 17 spare.
        let layout =
            PaneLayout::compute_with_filler(filler_area(20), 2, 5, 2, (0, 0), false, false);
        assert_eq!(layout.quota_level, quota::QuotaLevel::Full);
        assert_eq!(layout.quota_height, 5);
        assert_eq!(layout.scroll_visible_height(), 19 - 5);
    }

    #[test]
    fn tab_band_sits_above_the_quota_block() {
        // 20 rows − 1 header = 19 list rows; 2 rendered leaves 17 spare.
        let layout =
            PaneLayout::compute_with_filler(filler_area(20), 2, 5, 2, (6, 6), false, false);
        assert_eq!(layout.band_blocks.len(), 2, "both blocks fit");
        let (activity, activity_tab) = layout.band_blocks[0];
        let (git, git_tab) = layout.band_blocks[1];
        assert_eq!(activity.height, 6);
        assert_eq!(git.height, 6);
        assert_eq!(activity_tab, crate::state::BottomTab::Activity);
        assert_eq!(git_tab, crate::state::BottomTab::GitStatus);
        assert!(activity.y < git.y, "Activity is stacked above Git");
        assert_eq!(
            git.y + git.height,
            layout.list_area.bottom() - layout.quota_height,
            "the bottom block ends where the quota block starts"
        );
        // The list may still paint only the rows nothing else reserved.
        assert_eq!(
            layout.scroll_visible_height(),
            19 - layout.quota_height - (activity.height + git.height)
        );
    }

    #[test]
    fn tab_band_shrinks_to_the_free_rows_instead_of_hiding() {
        // 9 rows − 1 header = 8 list rows; 2 rendered leaves 6 spare. The quota
        // block takes 2, so a 12-row wish is clamped to the 4 that are free and
        // the tab renders whatever fits.
        let layout =
            PaneLayout::compute_with_filler(filler_area(9), 2, 0, 2, (12, 12), false, false);
        assert_eq!(layout.quota_height, 2);
        assert_eq!(
            layout.band_blocks.first().expect("clamped band").0.height,
            4
        );

        // 6 rows − 1 header = 5 list rows; 2 rendered leaves 3 spare. The quota
        // block takes 2, leaving 1 — below the minimum, so the band hides.
        let layout =
            PaneLayout::compute_with_filler(filler_area(6), 2, 0, 2, (12, 12), false, false);
        assert_eq!(layout.quota_height, 2);
        assert!(layout.band_blocks.is_empty());
    }

    #[test]
    fn both_blocks_keep_their_minimum_when_the_space_is_shared() {
        // 12 rows − 1 header = 11 list rows; 2 rendered leaves 9 spare, no
        // quota. A 40-row Activity wish and a 4-row Git wish share the 9 rows:
        // both keep the 3-row minimum, and the leftover 3 rows go to whichever
        // still wants more (the water-filling step).
        let layout =
            PaneLayout::compute_with_filler(filler_area(12), 2, 0, 0, (40, 4), false, false);
        let heights: Vec<u16> = layout.band_blocks.iter().map(|(r, _)| r.height).collect();
        assert_eq!(heights.iter().sum::<u16>(), 9);
        assert!(
            layout.band_blocks.iter().all(|(r, _)| r.height >= 3),
            "no block is squeezed out: {:?}",
            layout.band_blocks
        );
        assert_eq!(heights, vec![5, 4], "Activity above, Git below");
    }

    #[test]
    fn tab_band_yields_to_the_agent_list_first() {
        // 6 rows − 1 header = 5 list rows; 2 protected rows leave 3 spare, and
        // the band takes exactly those 3: the protected rows stay paintable
        // (the list scrolls instead of losing them).
        let layout = PaneLayout::compute_with_filler(filler_area(6), 2, 0, 0, (6, 6), false, false);
        assert_eq!(
            layout
                .band_blocks
                .iter()
                .map(|(r, _)| r.height)
                .sum::<u16>(),
            3
        );
        assert_eq!(layout.scroll_visible_height(), 2);
    }

    #[test]
    fn pet_band_sits_above_the_tab_band() {
        let layout = PaneLayout::compute_with_filler(filler_area(30), 2, 5, 2, (6, 6), true, false);
        let band = layout.band_blocks.first().expect("band should fit").0;
        let pet = layout.pet_area.expect("pet should fit above the band");
        assert!(
            pet.y + pet.height <= band.y,
            "pet {pet:?} must not overlap the band {band:?}"
        );
    }

    #[test]
    fn band_height_follows_content_and_options() {
        use crate::state::BottomTab;

        let mut state = AppState::new("%0".into());
        state.bottom_panel_height = 0;
        // An empty tab keeps the minimum band: the title bar has to stay
        // reachable, so clicking the other tab is always possible.
        assert_eq!(
            band_wishes_for(&state, 40).0,
            crate::ui::TAB_BAND_MIN_HEIGHT
        );

        state.activity.entries = vec![crate::activity::ActivityEntry {
            timestamp: "10:32".into(),
            tool: "Edit".into(),
            label: "src/main.rs".into(),
        }];
        // Timestamp/tool row + wrapped label, plus two borders. No leading
        // spacer: a separator must never outlive the content it introduces.
        assert_eq!(band_wishes_for(&state, 40).0, 4);

        // More entries make the band grow upward instead of scrolling.
        state.activity.entries = (0..4)
            .map(|i| crate::activity::ActivityEntry {
                timestamp: format!("10:3{i}"),
                tool: "Bash".into(),
                label: "cargo test".into(),
            })
            .collect();
        assert_eq!(
            band_wishes_for(&state, 40).0,
            2 + 4 * 2,
            "two rows per entry"
        );

        state.bottom_panel_height = 12;
        assert_eq!(
            band_wishes_for(&state, 40).0,
            0,
            "the bottom panel already hosts the tabs"
        );

        state.bottom_panel_height = 0;
        state.band_enabled = false;
        assert_eq!(band_wishes_for(&state, 40).0, 0, "@sidebar_band off wins");

        // The git tab reports its header plus file sections once the focused
        // pane is inside a repository, and the empty state before that.
        state.band_enabled = true;
        state.bottom_tab = BottomTab::GitStatus;
        state.activity.entries.clear();
        assert_eq!(
            band_wishes_for(&state, 40).0,
            crate::ui::TAB_BAND_MIN_HEIGHT,
            "no repository yet: empty-state band"
        );
        state.git.branch = "main".into();
        assert!(
            band_wishes_for(&state, 40).1 > crate::ui::TAB_BAND_MIN_HEIGHT,
            "a repository with a branch reports header lines"
        );
    }

    #[test]
    fn quota_keeps_its_rows_when_the_caller_offers_the_full_variant() {
        // The caller zeroes `quota_full` when the wide row cannot fit the pane
        // width; only then does the block fall back to compact rows.
        let full = PaneLayout::compute_with_filler(filler_area(7), 2, 5, 2, (0, 0), false, false);
        assert_eq!(full.quota_level, quota::QuotaLevel::Full);
        assert_eq!(full.quota_height, 5);

        let compact =
            PaneLayout::compute_with_filler(filler_area(7), 2, 0, 2, (0, 0), false, false);
        assert_eq!(compact.quota_level, quota::QuotaLevel::Compact);
        assert_eq!(compact.quota_height, 2);

        let hidden = PaneLayout::compute_with_filler(filler_area(7), 2, 0, 0, (0, 0), false, false);
        assert_eq!(hidden.quota_level, quota::QuotaLevel::Hidden);
        assert_eq!(hidden.quota_height, 0);
    }

    #[test]
    fn quota_stays_pinned_when_the_list_fills_the_pane() {
        // 6 rows − 1 header = 5 list rows for 5 rendered rows: zero spare. The
        // block keeps its rows and the list scrolls instead.
        let layout = PaneLayout::compute_with_filler(filler_area(6), 5, 5, 2, (0, 0), true, false);
        assert_eq!(layout.quota_level, quota::QuotaLevel::Full);
        assert_eq!(layout.quota_height, 5);
        assert!(layout.pet_area.is_none());
        assert_eq!(layout.scroll_visible_height(), 0);
    }

    #[test]
    fn filler_reserves_the_pet_band_above_the_quota_block() {
        // 20 rows − 1 header = 19 list rows; 2 rendered leaves 17 spare.
        let layout = PaneLayout::compute_with_filler(filler_area(20), 2, 5, 2, (0, 0), true, false);
        assert_eq!(layout.quota_level, quota::QuotaLevel::Full);
        let pet = layout.pet_area.expect("pet band should fit");
        assert_eq!(pet.height, crate::ui::PET_SCENE_HEIGHT);
        // The pet band starts immediately after the last rendered agent row
        // and ends above the bottom-anchored quota block.
        assert_eq!(pet.y, layout.list_area.y + 2);
        assert!(pet.y + pet.height <= layout.list_area.bottom() - layout.quota_height);
        assert_eq!(
            layout.scroll_visible_height(),
            19 - layout.quota_height - pet.height
        );
    }

    #[test]
    fn filler_hides_the_pet_when_only_the_quota_fits() {
        // 8 rows − 1 header = 7 list rows; 2 rendered leaves 5 spare, exactly
        // the full quota block, leaving no room for the pet band.
        let layout = PaneLayout::compute_with_filler(filler_area(8), 2, 5, 2, (0, 0), true, false);
        assert_eq!(layout.quota_level, quota::QuotaLevel::Full);
        assert!(layout.pet_area.is_none());
    }

    #[test]
    fn filler_keeps_the_pet_visible_when_subscriptions_are_absent() {
        // 20 rows − 1 header = 19 list rows; 1 rendered leaves 18 spare and no
        // quota block, so the pet renders in the filler area.
        let layout = PaneLayout::compute_with_filler(filler_area(20), 1, 0, 0, (0, 0), true, false);
        assert_eq!(layout.quota_level, quota::QuotaLevel::Hidden);
        let pet = layout.pet_area.expect("pet band should fit");
        assert_eq!(pet.y, layout.list_area.y + 1);
    }

    #[test]
    fn filler_disables_the_pet_when_quota_is_unavailable_and_pet_is_off() {
        let layout =
            PaneLayout::compute_with_filler(filler_area(20), 1, 0, 0, (0, 0), false, false);
        assert!(layout.pet_area.is_none());
    }

    #[test]
    fn filler_handles_tiny_and_zero_height_areas() {
        for height in [0, 1, 2] {
            let layout =
                PaneLayout::compute_with_filler(filler_area(height), 0, 5, 2, (0, 0), true, false);
            // The block reserves what the panel can hold; the renderer skips
            // painting when even that is not its full height.
            assert_eq!(layout.quota_height, layout.list_area.height);
            assert!(layout.pet_area.is_none());
            assert_eq!(
                layout.scroll_visible_height(),
                layout.list_area.height - layout.quota_height
            );
        }
    }
}
