use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::state::{AppState, BottomTab, Focus};
use crate::worktree::RemoveMode;

/// Dispatch a single crossterm [`Event`] into the [`AppState`], returning
/// `true` when a redraw should be scheduled.
///
/// The terminal handle is only borrowed to query its size for mouse
/// coordinate conversion; it is never written to from here.
pub(super) fn handle_event(
    ev: Event,
    state: &mut AppState,
    git_tab_active: &AtomicBool,
    terminal: &Terminal<CrosstermBackend<io::Stdout>>,
) -> bool {
    if matches!(ev, Event::Mouse(_) | Event::FocusLost) {
        state.focus_state.pending_g = None;
    }
    match ev {
        Event::Key(key) => handle_key_event(key, state),
        Event::Mouse(mouse) => {
            let term_height = terminal.size().map(|s| s.height).unwrap_or(0);
            let bottom_h = state.bottom_panel_height;
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let bottom_start = term_height.saturating_sub(bottom_h);
                    // The tab band sits inside the agents panel, so it is
                    // tested before the agents-panel hit targets.
                    if state.handle_band_click(mouse.row, mouse.column) {
                        git_tab_active
                            .store(state.bottom_tab == BottomTab::GitStatus, Ordering::Relaxed);
                    } else if mouse.row < bottom_start {
                        state.handle_mouse_click(mouse.row, mouse.column);
                    } else if mouse.row == bottom_start {
                        state.handle_bottom_tab_click(mouse.column);
                        // Keep the worker flag aligned with mouse-driven tab changes.
                        // Without this, clicking into Git Status leaves polling disabled
                        // until the next refresh tick and the tab renders stale data.
                        git_tab_active
                            .store(state.bottom_tab == BottomTab::GitStatus, Ordering::Relaxed);
                    }
                }
                MouseEventKind::ScrollDown => {
                    state.handle_mouse_scroll(mouse.row, term_height, bottom_h, 3);
                }
                MouseEventKind::ScrollUp => {
                    state.handle_mouse_scroll(mouse.row, term_height, bottom_h, -3);
                }
                _ => {}
            }
            true
        }
        _ => false,
    }
}

/// Dispatch a single [`KeyEvent`]. Split out from [`handle_event`] so that
/// unit tests can drive the keyboard path without constructing a real
/// terminal handle (the [`Terminal`] argument is only needed for mouse
/// coordinate conversion).
pub(super) fn handle_key_event(key: KeyEvent, state: &mut AppState) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    let pending_g = state.focus_state.pending_g.take();
    if state.is_notices_popup_open() {
        if key.code == KeyCode::Esc {
            state.close_notices_popup();
        }
        return true;
    }
    if state.is_spawn_input_open() {
        match key.code {
            KeyCode::Esc => state.close_spawn_input(),
            KeyCode::Enter => state.confirm_spawn_input(),
            KeyCode::Tab | KeyCode::Down => state.spawn_input_next_field(),
            KeyCode::BackTab | KeyCode::Up => state.spawn_input_prev_field(),
            KeyCode::Left => state.spawn_input_cycle(-1),
            KeyCode::Right => state.spawn_input_cycle(1),
            KeyCode::Backspace => state.spawn_input_pop_char(),
            KeyCode::Char(c) => state.spawn_input_push_char(c),
            _ => {}
        }
        return true;
    }
    if state.is_remove_confirm_open() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => state.close_remove_confirm(),
            KeyCode::Char('c') => state.confirm_remove(RemoveMode::WindowOnly),
            KeyCode::Enter | KeyCode::Char('y') => {
                state.confirm_remove(RemoveMode::WindowAndWorktree)
            }
            _ => {}
        }
        return true;
    }
    if state.is_repo_popup_open() {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => state.close_repo_popup(),
            KeyCode::Down => repo_popup_nav_down(state),
            KeyCode::Char('n') if ctrl => repo_popup_nav_down(state),
            KeyCode::Up => repo_popup_nav_up(state),
            KeyCode::Char('p') if ctrl => repo_popup_nav_up(state),
            KeyCode::Enter => state.confirm_repo_popup(),
            KeyCode::Backspace => state.pop_repo_popup_query(),
            KeyCode::Char(c) if !ctrl => state.push_repo_popup_query(c),
            _ => {}
        }
        return true;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
    if plain && key.code == KeyCode::Char('g') {
        if pending_g.is_some_and(|(started, focus)| {
            started.elapsed() <= Duration::from_secs(1) && focus == state.focus_state.focus
        }) {
            jump_boundary(state, false);
        } else {
            state.focus_state.pending_g = Some((Instant::now(), state.focus_state.focus.clone()));
        }
        return true;
    }
    match key.code {
        KeyCode::Char('G') if plain => jump_boundary(state, true),
        KeyCode::Char('u') if ctrl => half_page(state, false),
        KeyCode::Char('d') if ctrl => half_page(state, true),
        KeyCode::Esc => {
            if state.focus_state.focus == Focus::ActivityLog
                || state.focus_state.focus == Focus::Filter
            {
                state.focus_state.focus = Focus::Panes;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => pane_nav_down(state),
        KeyCode::Char('n') if ctrl => pane_nav_down(state),
        KeyCode::Char('k') | KeyCode::Up => pane_nav_up(state),
        KeyCode::Char('p') if ctrl => pane_nav_up(state),
        KeyCode::Char('h') | KeyCode::Left => {
            if state.focus_state.focus == Focus::Filter {
                state.global.status_filter = state.global.status_filter.prev();
                state.global.save_filter();
                state.rebuild_row_targets();
            } else if state.focus_state.focus == Focus::ActivityLog {
                // Move the band's focus to the other block (Activity <-> Git).
                state.switch_band_block();
            } else {
                // From the agent list the footer is one key press away: `Left`
                // lands on the Activity block, `Right` on Git.
                state.focus_footer(BottomTab::Activity);
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if state.focus_state.focus == Focus::Filter {
                state.global.status_filter = state.global.status_filter.next();
                state.global.save_filter();
                state.rebuild_row_targets();
            } else if state.focus_state.focus == Focus::ActivityLog {
                state.switch_band_block();
            } else {
                state.focus_footer(BottomTab::GitStatus);
            }
        }
        KeyCode::Char('r') => {
            if state.focus_state.focus == Focus::Filter {
                state.toggle_repo_popup();
            }
        }
        KeyCode::Char('n') => {
            if state.focus_state.focus == Focus::Panes {
                state.open_spawn_input_from_selection();
            }
        }
        KeyCode::Char('x') => {
            if state.focus_state.focus == Focus::Panes {
                state.open_remove_confirm();
            }
        }
        // Copy the Activity cursor's command. It answers whenever the footer
        // owns the keyboard — not only while the movement keys happen to drive
        // the Activity block — so `y` never silently does nothing. The cursor
        // is visible either way, and the flash names what was copied.
        KeyCode::Char('y') if plain => {
            if state.focus_state.focus == Focus::ActivityLog {
                state.request_activity_copy();
            }
        }
        KeyCode::Enter => {
            if state.focus_state.focus == Focus::Panes {
                state.activate_selected_pane();
            }
        }
        KeyCode::Tab => {
            state.global.status_filter = state.global.status_filter.next();
            state.global.save_filter();
            state.rebuild_row_targets();
        }
        KeyCode::BackTab => {
            state.global.status_filter = state.global.status_filter.prev();
            state.global.save_filter();
            state.rebuild_row_targets();
        }
        KeyCode::Char('/') => state.toggle_repo_popup(),
        _ => {}
    }
    true
}

fn jump_boundary(state: &mut AppState, end: bool) {
    if state.focus_state.focus == Focus::ActivityLog {
        match state.bottom_tab {
            // Activity jumps the cursor, not the viewport: the highlight and
            // the copy target stay the same thing.
            BottomTab::Activity => {
                if end {
                    state.activity.select_last();
                } else {
                    state.activity.select_first();
                }
            }
            BottomTab::GitStatus => {
                let scroll = &mut state.scrolls.git;
                scroll.offset = if end {
                    scroll.total_lines.saturating_sub(scroll.visible_height)
                } else {
                    0
                };
            }
        }
    } else {
        state.focus_state.focus = Focus::Panes;
        state.select_pane_row(if end { usize::MAX } else { 0 });
    }
}

fn half_page(state: &mut AppState, down: bool) {
    if state.focus_state.focus == Focus::ActivityLog {
        let height = match state.bottom_tab {
            BottomTab::Activity => state.activity.scroll.visible_height,
            BottomTab::GitStatus => state.scrolls.git.visible_height,
        };
        let step = (height / 2).max(1) as isize;
        state.scroll_bottom(if down { step } else { -step });
    } else {
        state.focus_state.focus = Focus::Panes;
        state.move_pane_half_page(down);
    }
}

fn pane_nav_down(state: &mut AppState) {
    match state.focus_state.focus {
        Focus::Filter => {
            state.focus_state.focus = Focus::Panes;
        }
        Focus::Panes => {
            if state.move_pane_selection(1) {
                state.global.queue_cursor_save();
            } else {
                state.focus_state.focus = Focus::ActivityLog;
            }
        }
        Focus::ActivityLog => state.scroll_bottom(1),
    }
}

fn pane_nav_up(state: &mut AppState) {
    match state.focus_state.focus {
        Focus::Filter => {}
        Focus::Panes => {
            if state.move_pane_selection(-1) {
                state.global.queue_cursor_save();
            } else {
                state.focus_state.focus = Focus::Filter;
            }
        }
        Focus::ActivityLog => {
            let at_top = match state.bottom_tab {
                BottomTab::Activity => state.activity.selected == 0,
                BottomTab::GitStatus => state.scrolls.git.offset == 0,
            };
            if at_top {
                state.focus_state.focus = Focus::Panes;
            } else {
                state.scroll_bottom(-1);
            }
        }
    }
}

fn repo_popup_nav_down(state: &mut AppState) {
    let count = state.repo_popup_names().len();
    let current = state.repo_popup_selected();
    if current + 1 < count {
        state.set_repo_popup_selected(current + 1);
    }
}

fn repo_popup_nav_up(state: &mut AppState) {
    let current = state.repo_popup_selected();
    if current > 0 {
        state.set_repo_popup_selected(current - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::RepoGroup;
    use crate::state::RowTarget;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// Build an AppState with three navigable pane rows and Panes focus,
    /// which is the precondition the navigation arms operate against.
    fn state_with_three_panes() -> AppState {
        let mut state = AppState::new("%99".into());
        state.layout.pane_row_targets = vec![
            RowTarget {
                pane_id: "%1".into(),
            },
            RowTarget {
                pane_id: "%2".into(),
            },
            RowTarget {
                pane_id: "%3".into(),
            },
        ];
        state.global.selected_pane_row = 0;
        state.focus_state.focus = Focus::Panes;
        state
    }

    fn state_with_repo_popup_open() -> AppState {
        let mut state = AppState::new("%99".into());
        // toggle_repo_popup uses repo_names(), which always includes the
        // "All" sentinel — pad with two named groups so the selection has
        // somewhere to move.
        state.repo_groups = vec![
            RepoGroup {
                name: "repo-a".into(),
                has_focus: false,
                panes: vec![],
            },
            RepoGroup {
                name: "repo-b".into(),
                has_focus: false,
                panes: vec![],
            },
        ];
        state.toggle_repo_popup();
        state.set_repo_popup_selected(0);
        state
    }

    #[test]
    fn vim_boundaries_and_prefix_cancellation() {
        let mut state = state_with_three_panes();
        handle_key_event(
            KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
            &mut state,
        );
        assert_eq!(state.global.selected_pane_row, 2);
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
        assert!(state.focus_state.pending_g.is_none());

        state.global.selected_pane_row = 2;
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        handle_key_event(key(KeyCode::Esc), &mut state);
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
        state.focus_state.pending_g = Some((Instant::now() - Duration::from_secs(2), Focus::Panes));
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
        state.focus_state.focus = Focus::Filter;
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.focus_state.focus, Focus::Filter);
        handle_key_event(key(KeyCode::Char('g')), &mut state);
        assert_eq!(state.focus_state.focus, Focus::Panes);
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn vim_keys_remain_search_text_in_repo_popup() {
        let mut state = state_with_repo_popup_open();
        for c in ['g', 'g', 'G'] {
            handle_key_event(key(KeyCode::Char(c)), &mut state);
        }
        assert_eq!(state.repo_popup_query(), "ggG");
        assert!(state.focus_state.pending_g.is_none());
    }

    #[test]
    fn half_page_uses_rendered_lines_and_clamps_without_leaving_panes() {
        let mut state = state_with_three_panes();
        state.scrolls.panes.visible_height = 10;
        state.layout.line_to_row = vec![None, Some(0), Some(0), Some(1), Some(1), None, Some(2)];
        handle_key_event(ctrl_key('d'), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
        handle_key_event(ctrl_key('d'), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
        assert_eq!(state.focus_state.focus, Focus::Panes);
        handle_key_event(ctrl_key('u'), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
        handle_key_event(ctrl_key('u'), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
        assert_eq!(state.focus_state.focus, Focus::Panes);
    }

    #[test]
    fn y_copies_the_activity_cursor_from_the_footer() {
        let mut state = state_with_three_panes();
        state.activity.entries = vec![
            crate::activity::ActivityEntry {
                timestamp: "10:32".into(),
                tool: "Bash".into(),
                label: "cargo test".into(),
            },
            crate::activity::ActivityEntry {
                timestamp: "10:31".into(),
                tool: "Bash".into(),
                label: "cargo build".into(),
            },
        ];

        // The keyboard is in the agent list: the footer's cursor is not the
        // target of the keys, so `y` stays free.
        handle_key_event(key(KeyCode::Char('y')), &mut state);
        assert!(state.pending_clipboard_copy.is_none());
        assert!(state.pending_osc52_copy.is_none());

        // The footer owns the keyboard while the Git block is the one the
        // movement keys drive: `y` still copies the Activity cursor instead of
        // silently doing nothing.
        state.focus_state.focus = Focus::ActivityLog;
        state.bottom_tab = BottomTab::GitStatus;
        handle_key_event(key(KeyCode::Char('y')), &mut state);
        assert_eq!(state.pending_clipboard_copy.as_deref(), Some("cargo test"));
        assert_eq!(state.pending_osc52_copy.as_deref(), Some("cargo test"));
        assert_eq!(state.take_flash().as_deref(), Some("copied: cargo test"));
        state.pending_clipboard_copy = None;
        state.pending_osc52_copy = None;

        // With the movement keys on Activity, `j` moves the cursor, and `y`
        // copies what the cursor now points at.
        state.bottom_tab = BottomTab::Activity;
        handle_key_event(key(KeyCode::Char('j')), &mut state);
        handle_key_event(key(KeyCode::Char('y')), &mut state);
        // `j` moved the cursor down one entry, so `y` copies that one and not
        // the newest.
        assert_eq!(state.activity.selected, 1);
        assert_eq!(state.pending_clipboard_copy.as_deref(), Some("cargo build"));
        assert_eq!(state.pending_osc52_copy.as_deref(), Some("cargo build"));
        assert_eq!(state.take_flash().as_deref(), Some("copied: cargo build"),);
    }

    #[test]
    fn vim_scrolls_current_bottom_tab() {
        for tab in [BottomTab::Activity, BottomTab::GitStatus] {
            let mut state = state_with_three_panes();
            state.focus_state.focus = Focus::ActivityLog;
            state.bottom_tab = tab;
            let scroll = crate::state::ScrollState {
                offset: 0,
                total_lines: 40,
                visible_height: 10,
            };
            state.activity.scroll = scroll.clone();
            state.scrolls.git = scroll;
            state.activity.entries = (0..40)
                .map(|i| crate::activity::ActivityEntry {
                    timestamp: "10:32".into(),
                    tool: "Bash".into(),
                    label: format!("cargo test {i}"),
                })
                .collect();
            handle_key_event(ctrl_key('d'), &mut state);
            // Activity is a cursor list: the scroll keys move the selection.
            // Git stays a plain scrollable viewport.
            let position = |state: &AppState| match tab {
                BottomTab::Activity => state.activity.selected,
                BottomTab::GitStatus => state.scrolls.git.offset,
            };
            assert_eq!(position(&state), 5);
            handle_key_event(ctrl_key('u'), &mut state);
            assert_eq!(position(&state), 0);
            handle_key_event(key(KeyCode::Char('G')), &mut state);
            assert_eq!(
                position(&state),
                if tab == BottomTab::Activity { 39 } else { 30 }
            );
            for _ in 0..2 {
                handle_key_event(key(KeyCode::Char('g')), &mut state);
            }
            assert_eq!(position(&state), 0);
            assert_eq!(state.focus_state.focus, Focus::ActivityLog);
        }
    }

    #[test]
    fn vim_navigation_handles_empty_list_and_zero_height() {
        let mut state = AppState::new("%99".into());
        for code in [KeyCode::Char('G'), KeyCode::Char('g'), KeyCode::Char('g')] {
            handle_key_event(key(code), &mut state);
        }
        handle_key_event(ctrl_key('d'), &mut state);
        handle_key_event(ctrl_key('u'), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
        let mut state = state_with_three_panes();
        handle_key_event(ctrl_key('d'), &mut state);
        assert_eq!(state.global.selected_pane_row, 1);
    }

    #[test]
    fn ctrl_n_moves_pane_selection_down() {
        let mut state = state_with_three_panes();
        handle_key_event(ctrl_key('n'), &mut state);
        assert_eq!(state.global.selected_pane_row, 1);
        handle_key_event(ctrl_key('n'), &mut state);
        assert_eq!(state.global.selected_pane_row, 2);
    }

    #[test]
    fn ctrl_p_moves_pane_selection_up() {
        let mut state = state_with_three_panes();
        state.global.selected_pane_row = 2;
        handle_key_event(ctrl_key('p'), &mut state);
        assert_eq!(state.global.selected_pane_row, 1);
        handle_key_event(ctrl_key('p'), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn bare_j_and_k_still_navigate_panes() {
        let mut state = state_with_three_panes();
        handle_key_event(key(KeyCode::Char('j')), &mut state);
        assert_eq!(state.global.selected_pane_row, 1);
        handle_key_event(key(KeyCode::Char('k')), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn bare_n_does_not_move_selection() {
        // The bare `n` arm is wired to the spawn input flow, not navigation.
        // We don't assert the popup opens (that requires repo_groups +
        // git metadata, exercised elsewhere) — only that it does NOT
        // shadow the Ctrl-N navigation arm.
        let mut state = state_with_three_panes();
        handle_key_event(key(KeyCode::Char('n')), &mut state);
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn bare_p_is_unbound_in_panes_focus() {
        let mut state = state_with_three_panes();
        state.global.selected_pane_row = 1;
        handle_key_event(key(KeyCode::Char('p')), &mut state);
        assert_eq!(state.global.selected_pane_row, 1);
    }

    #[test]
    fn ctrl_n_navigates_repo_popup_down() {
        let mut state = state_with_repo_popup_open();
        handle_key_event(ctrl_key('n'), &mut state);
        assert_eq!(state.repo_popup_selected(), 1);
        handle_key_event(ctrl_key('n'), &mut state);
        assert_eq!(state.repo_popup_selected(), 2);
        // Past the last entry the popup nav helper is a no-op.
        handle_key_event(ctrl_key('n'), &mut state);
        assert_eq!(state.repo_popup_selected(), 2);
    }

    #[test]
    fn ctrl_p_navigates_repo_popup_up() {
        let mut state = state_with_repo_popup_open();
        state.set_repo_popup_selected(2);
        handle_key_event(ctrl_key('p'), &mut state);
        assert_eq!(state.repo_popup_selected(), 1);
        handle_key_event(ctrl_key('p'), &mut state);
        assert_eq!(state.repo_popup_selected(), 0);
        // Below 0 the popup nav helper is a no-op.
        handle_key_event(ctrl_key('p'), &mut state);
        assert_eq!(state.repo_popup_selected(), 0);
    }

    #[test]
    fn slash_opens_searchable_repo_popup() {
        let mut state = AppState::new("%99".into());

        handle_key_event(key(KeyCode::Char('/')), &mut state);

        assert!(state.is_repo_popup_open());
        assert_eq!(state.repo_popup_query(), "");
    }

    #[test]
    fn repo_popup_accepts_text_and_backspace() {
        let mut state = state_with_repo_popup_open();

        handle_key_event(key(KeyCode::Char('j')), &mut state);
        handle_key_event(key(KeyCode::Char('k')), &mut state);
        assert_eq!(state.repo_popup_query(), "jk");

        handle_key_event(key(KeyCode::Backspace), &mut state);
        assert_eq!(state.repo_popup_query(), "j");
    }

    #[test]
    fn tab_cycles_status_filter_forward() {
        let mut state = AppState::new("%99".into());

        handle_key_event(key(KeyCode::Tab), &mut state);

        assert_eq!(
            state.global.status_filter,
            crate::state::StatusFilter::Running
        );
    }

    #[test]
    fn arrows_switch_the_band_block_when_the_band_has_focus() {
        let mut state = state_with_three_panes();
        state.focus_state.focus = Focus::ActivityLog;
        state.bottom_tab = BottomTab::Activity;

        handle_key_event(key(KeyCode::Right), &mut state);
        assert_eq!(state.bottom_tab, BottomTab::GitStatus, "Right -> Git");
        handle_key_event(key(KeyCode::Left), &mut state);
        assert_eq!(state.bottom_tab, BottomTab::Activity, "Left -> Activity");
        // `h`/`l` mirror the arrows, matching the rest of the keymap.
        handle_key_event(key(KeyCode::Char('l')), &mut state);
        assert_eq!(state.bottom_tab, BottomTab::GitStatus);

        // The filter row keeps cycling the status filter.
        handle_key_event(key(KeyCode::Char('l')), &mut state);
        state.focus_state.focus = Focus::Filter;
        let before = state.global.status_filter;
        handle_key_event(key(KeyCode::Char('l')), &mut state);
        assert_ne!(state.global.status_filter, before);
    }

    #[test]
    fn arrows_enter_the_footer_from_the_agent_list() {
        // The footer is one keystroke away instead of a `j` walk to the last
        // pane: `Left` lands on Activity, `Right` on Git.
        let mut state = state_with_three_panes();
        state.focus_state.focus = Focus::Panes;
        state.bottom_tab = BottomTab::Activity;

        handle_key_event(key(KeyCode::Right), &mut state);
        assert_eq!(state.focus_state.focus, Focus::ActivityLog);
        assert_eq!(state.bottom_tab, BottomTab::GitStatus, "Right -> Git");

        state.focus_state.focus = Focus::Panes;
        handle_key_event(key(KeyCode::Char('h')), &mut state);
        assert_eq!(state.focus_state.focus, Focus::ActivityLog);
        assert_eq!(state.bottom_tab, BottomTab::Activity, "Left -> Activity");

        // The agent list selection is untouched by the jump.
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn back_tab_cycles_status_filter_backward_without_switching_bottom_tab() {
        let mut state = AppState::new("%99".into());

        handle_key_event(key(KeyCode::BackTab), &mut state);

        assert_eq!(
            state.global.status_filter,
            crate::state::StatusFilter::Error
        );
        assert_eq!(state.bottom_tab, BottomTab::Activity);
    }
}
