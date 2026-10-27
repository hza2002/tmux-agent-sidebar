use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::SPAWN_BUTTON;
use super::row;
use crate::state::{AppState, Focus};
use crate::ui::text::{display_width, truncate_to_width};

#[derive(Debug, Default)]
pub(super) struct CollectedRows {
    pub lines: Vec<Line<'static>>,
    pub line_to_row: Vec<Option<usize>>,
    pub repo_fold_lines: Vec<(usize, String)>,
    pub pending_spawn: Vec<(usize, String, String)>,
    pub pending_remove: Vec<(usize, u16, String)>,
}

pub(super) fn collect(state: &AppState, width: u16) -> CollectedRows {
    let width = width as usize;
    let theme = &state.theme;

    let mut collected = CollectedRows::default();
    let mut row_index: usize = 0;

    for group in state.ordered_visible_repo_groups() {
        let filtered_panes: Vec<_> = group
            .panes
            .iter()
            .filter(|(pane, _)| state.pane_visible_in_repo(group, pane))
            .collect();

        let group_has_focused_pane = state
            .focus_state
            .focused_pane_id
            .as_ref()
            .is_some_and(|fid| group.panes.iter().any(|(p, _)| p.pane_id == *fid));

        // Plain repo header at column 0, with a `[+]` spawn button
        // right-aligned on the same row. Only rendered when the group
        // has a resolved repo_root — panes outside a git repo get a
        // plain title.
        let idle_count = state.repo_idle_count(group);
        let header_selectable = idle_count > 0 && state.repo_idle_collapsed(group.id());
        if idle_count > 0 {
            collected
                .repo_fold_lines
                .push((collected.lines.len(), group.id().to_string()));
        }
        let header_selected = header_selectable
            && state.focus_state.sidebar_focused
            && state.focus_state.focus == Focus::Panes
            && row_index == state.global.selected_pane_row;
        let title = &group.name;
        let title_color = if group_has_focused_pane {
            theme.accent
        } else {
            theme.session_header
        };
        let repo_root = group
            .panes
            .iter()
            .find_map(|(_, git)| git.repo_root.clone());
        let suffix = if header_selectable && state.repo_idle_collapsed(group.id()) {
            format!(
                " ▸ {} {idle_count}",
                state.icons.status_icon(&crate::tmux::PaneStatus::Idle)
            )
        } else {
            String::new()
        };
        let button_width = usize::from(repo_root.is_some()) * SPAWN_BUTTON.len();
        let button_reserve = button_width + usize::from(repo_root.is_some());
        let title_budget = width
            .saturating_sub(button_reserve)
            .saturating_sub(display_width(&suffix));
        let title = truncate_to_width(title, title_budget);
        let title_style = if header_selected {
            Style::default()
                .fg(theme.text_active)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(title_color)
        };
        let mut spans = vec![Span::styled(title.clone(), title_style)];
        // A complete fold count is more useful than a clipped arrow/number.
        if display_width(&suffix) + button_reserve <= width {
            spans.push(Span::styled(suffix.clone(), title_style));
        }
        let used = spans
            .iter()
            .map(|span| display_width(&span.content))
            .sum::<usize>();
        if let Some(ref root) = repo_root {
            collected
                .pending_spawn
                .push((collected.lines.len(), group.name.clone(), root.clone()));
            spans.push(Span::styled(
                " ".repeat(width.saturating_sub(used + button_width)),
                title_style,
            ));
            spans.push(Span::styled(
                SPAWN_BUTTON,
                Style::default().fg(if group_has_focused_pane {
                    theme.accent
                } else {
                    theme.text_active
                }),
            ));
        } else if header_selected {
            spans.push(Span::styled(
                " ".repeat(width.saturating_sub(used)),
                title_style,
            ));
        }
        collected.lines.push(Line::from(spans));
        collected
            .line_to_row
            .push(header_selectable.then_some(row_index));
        if header_selectable {
            row_index += 1;
        }

        for (pane, git_info) in filtered_panes.iter() {
            let is_selected = state.focus_state.sidebar_focused
                && state.focus_state.focus == Focus::Panes
                && row_index == state.global.selected_pane_row;

            let is_active = state.focus_state.focused_pane_id.as_ref() == Some(&pane.pane_id);

            let pane_state = state.pane_state(&pane.pane_id);
            let ports = pane_state.map(|s| s.ports.as_slice());
            let task_progress = pane_state.and_then(|s| s.task_progress.as_ref());
            let status_line_idx = collected.lines.len();
            let pane_lines = row::render_pane_lines_with_ports(
                pane,
                git_info,
                ports,
                task_progress,
                is_selected,
                is_active,
                width,
                &state.icons,
                theme,
                state.now,
            );
            let pane_line_count = pane_lines.len();
            collected.lines.extend(pane_lines);
            for _ in 0..pane_line_count {
                collected.line_to_row.push(Some(row_index));
            }

            // The remove affordance is pinned to the status row's right edge.
            if pane.sidebar_spawned
                && git_info.is_worktree
                && let Some(x) =
                    row::sidebar_remove_marker_col(git_info, true, width.saturating_sub(2))
            {
                collected
                    .pending_remove
                    .push((status_line_idx, x, pane.pane_id.clone()));
            }

            row_index += 1;
        }
    }

    collected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::state::{AppState, StatusFilter};
    use crate::tmux::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorktreeMetadata};

    fn make_pane(id: &str, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/tmp/repo".into(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            status_changed_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
            background_tasks: vec![],
        }
    }

    #[test]
    fn collect_empty_repo_groups_produces_no_lines() {
        let state = AppState::new("%0".into());
        let collected = collect(&state, 40);
        assert!(collected.lines.is_empty());
        assert!(collected.line_to_row.is_empty());
        assert!(collected.pending_spawn.is_empty());
        assert!(collected.pending_remove.is_empty());
    }

    #[test]
    fn collect_skips_group_when_status_filter_excludes_all_panes() {
        let mut state = AppState::new("%0".into());
        // The group has only Running panes, so filter to Waiting to drop them all.
        state.global.status_filter = StatusFilter::Waiting;
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40);
        assert!(collected.lines.is_empty());
        assert!(collected.pending_spawn.is_empty());
    }

    #[test]
    fn collect_records_pending_spawn_when_repo_root_present() {
        let mut state = AppState::new("%0".into());
        let git_info = PaneGitInfo {
            repo_root: Some("/tmp/repo".into()),
            branch: None,
            is_worktree: false,
            worktree_name: None,
        };
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), git_info)],
        }];
        let collected = collect(&state, 40);
        assert_eq!(
            collected.pending_spawn.len(),
            1,
            "groups with a repo_root should emit a spawn target"
        );
        assert_eq!(collected.pending_spawn[0].1, "repo");
        assert_eq!(collected.pending_spawn[0].2, "/tmp/repo");
        // At least the header plus one pane row should have been pushed.
        assert!(!collected.lines.is_empty());
    }

    #[test]
    fn collect_no_pending_spawn_without_repo_root() {
        let mut state = AppState::new("%0".into());
        state.repo_groups = vec![RepoGroup {
            name: "raw-path".into(),
            has_focus: false,
            panes: vec![(make_pane("%1", PaneStatus::Running), PaneGitInfo::default())],
        }];
        let collected = collect(&state, 40);
        assert!(
            collected.pending_spawn.is_empty(),
            "groups without repo_root must not produce spawn targets"
        );
    }

    #[test]
    fn collect_pending_spawn_grows_with_repo_root_bearing_groups() {
        let mut state = AppState::new("%0".into());
        let with_root = |root: &str, name: &str, pane_id: &str| RepoGroup {
            name: name.into(),
            has_focus: false,
            panes: vec![(
                make_pane(pane_id, PaneStatus::Running),
                PaneGitInfo {
                    repo_root: Some(root.into()),
                    branch: None,
                    is_worktree: false,
                    worktree_name: None,
                },
            )],
        };
        state.repo_groups = vec![
            with_root("/repo/a", "a", "%1"),
            with_root("/repo/b", "b", "%2"),
            with_root("/repo/c", "c", "%3"),
        ];
        let collected = collect(&state, 40);
        assert_eq!(collected.pending_spawn.len(), 3);
    }
    #[test]
    fn idle_folds_keep_mixed_work_visible_and_all_idle_repos_on_one_row() {
        let mut state = AppState::new("%0".into());
        let group = |name: &str, panes: Vec<PaneInfo>| RepoGroup {
            name: name.into(),
            has_focus: false,
            panes: panes
                .into_iter()
                .map(|pane| {
                    (
                        pane,
                        PaneGitInfo {
                            repo_root: Some(format!("/repo/{name}")),
                            ..PaneGitInfo::default()
                        },
                    )
                })
                .collect(),
        };
        state.repo_groups = vec![
            group(
                "active-project-with-long-name",
                vec![
                    make_pane("%1", PaneStatus::Running),
                    make_pane("%2", PaneStatus::Idle),
                ],
            ),
            group(
                "archive",
                vec![
                    make_pane("%3", PaneStatus::Idle),
                    make_pane("%4", PaneStatus::Idle),
                ],
            ),
        ];
        state.global.idle_collapsed = true;
        state.focus_state.sidebar_focused = true;
        state.rebuild_row_targets();
        let folded = collect(&state, 28);
        let text = |rows: &CollectedRows| {
            rows.lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        insta::assert_snapshot!(text(&folded), @"
        active-project-with… ▸  1 +
           claude                  
        archive ▸  2              +
        ");
        assert_eq!(folded.line_to_row, vec![Some(0), Some(1), Some(2)]);
        assert_eq!(folded.pending_spawn.len(), 2);
        assert!(
            folded.lines[0]
                .spans
                .iter()
                .any(|span| span.style.bg == Some(state.theme.selection_bg))
        );
        state.toggle_repo_idle("/repo/archive");
        let expanded = collect(&state, 28);
        insta::assert_snapshot!(text(&expanded), @"
        active-project-with… ▸  1 +
           claude                  
        archive                    +
           claude                  
           claude
        ");
        assert_eq!(
            expanded.line_to_row,
            vec![Some(0), Some(1), None, Some(2), Some(3)]
        );
    }
}
