use super::{AppState, RowTarget};
use crate::group::RepoGroup;
use crate::tmux::{PaneInfo, PaneStatus};

/// Only read, quiet sessions may disappear from the list. Attention and stale
/// response markers are kept visible conservatively even on an Idle pane.
pub(crate) fn is_foldable_idle(pane: &PaneInfo) -> bool {
    pane.status == PaneStatus::Idle
        && !pane.attention
        && matches!(
            pane.wait_reason.as_str(),
            "" | "session_resumed" | "session_resumed_compact"
        )
        && !pane.has_background_work()
}

impl AppState {
    pub(crate) fn repo_idle_count(&self, group: &RepoGroup) -> usize {
        group
            .panes
            .iter()
            .filter(|(pane, _)| {
                self.global.status_filter.matches(&pane.status) && is_foldable_idle(pane)
            })
            .count()
    }

    pub(crate) fn repo_idle_collapsed(&self, repo_id: &str) -> bool {
        self.global
            .idle_fold_overrides
            .get(repo_id)
            .copied()
            .unwrap_or(self.global.idle_collapsed)
    }

    pub(crate) fn pane_visible_in_repo(&self, group: &RepoGroup, pane: &PaneInfo) -> bool {
        self.global.status_filter.matches(&pane.status)
            && !(self.repo_idle_collapsed(group.id()) && is_foldable_idle(pane))
    }

    pub(crate) fn visible_row_targets(&self) -> Vec<RowTarget> {
        let mut targets = Vec::new();
        for group in self.ordered_visible_repo_groups() {
            if self.repo_idle_count(group) > 0 && self.repo_idle_collapsed(group.id()) {
                targets.push(RowTarget::Repo {
                    repo_id: group.id().to_string(),
                });
            }
            for (pane, _) in &group.panes {
                if self.pane_visible_in_repo(group, pane) {
                    targets.push(RowTarget::Pane {
                        pane_id: pane.pane_id.clone(),
                    });
                }
            }
        }
        targets
    }

    pub fn toggle_idle_folds(&mut self) {
        self.global.idle_collapsed = !self.global.idle_collapsed;
        self.global.idle_fold_overrides.clear();
        self.global.save_idle_collapsed();
        self.rebuild_row_targets();
    }

    pub(crate) fn toggle_repo_idle(&mut self, repo_id: &str) {
        if !self
            .repo_groups
            .iter()
            .any(|group| group.id() == repo_id && self.repo_idle_count(group) > 0)
        {
            return;
        }
        let collapsed = !self.repo_idle_collapsed(repo_id);
        if collapsed == self.global.idle_collapsed {
            self.global.idle_fold_overrides.remove(repo_id);
        } else {
            self.global
                .idle_fold_overrides
                .insert(repo_id.to_string(), collapsed);
        }
        self.rebuild_row_targets();
    }

    /// Toggle only the repository that owns the selected row. A collapsed
    /// repository header owns its own repository, so the same action expands
    /// it. Unlike `toggle_idle_folds`, this keeps the global mode and every
    /// other repository exception untouched.
    pub(crate) fn toggle_selected_repo_idle(&mut self) {
        let Some(target) = self
            .layout
            .pane_row_targets
            .get(self.global.selected_pane_row)
        else {
            return;
        };
        let repo_id = match target {
            RowTarget::Repo { repo_id } => repo_id.clone(),
            RowTarget::Pane { pane_id } => {
                match self
                    .repo_groups
                    .iter()
                    .find(|group| group.panes.iter().any(|(pane, _)| pane.pane_id == *pane_id))
                {
                    Some(group) => group.id().to_string(),
                    None => return,
                }
            }
        };
        self.toggle_repo_idle(&repo_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::PaneGitInfo;
    use crate::tmux::{AgentType, PermissionMode, WorktreeMetadata};

    fn pane(id: &str, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/repo".into(),
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

    fn state(panes: Vec<PaneInfo>) -> AppState {
        let mut state = AppState::new("%sidebar".into());
        state.repo_groups = vec![RepoGroup {
            name: "repo".into(),
            has_focus: false,
            panes: panes
                .into_iter()
                .map(|pane| {
                    (
                        pane,
                        PaneGitInfo {
                            repo_root: Some("/repo".into()),
                            ..PaneGitInfo::default()
                        },
                    )
                })
                .collect(),
        }];
        state.rebuild_row_targets();
        state
    }

    fn ids(state: &AppState) -> Vec<&str> {
        state
            .layout
            .pane_row_targets
            .iter()
            .filter_map(RowTarget::pane_id)
            .collect()
    }

    #[test]
    fn expanded_headers_are_not_keyboard_stops() {
        let mut state = state(vec![pane("%1", PaneStatus::Idle)]);
        assert_eq!(state.layout.pane_row_targets.len(), 1);
        assert_eq!(state.layout.pane_row_targets[0].pane_id(), Some("%1"));
        state.toggle_idle_folds();
        assert!(matches!(
            state.layout.pane_row_targets[0],
            RowTarget::Repo { .. }
        ));
        state.activate_selected_pane();
        assert_eq!(state.layout.pane_row_targets[0].pane_id(), Some("%1"));
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn folds_only_read_idle_without_attention_or_background_work() {
        let idle = pane("%idle", PaneStatus::Idle);
        let mut unread = pane("%unread", PaneStatus::Idle);
        unread.wait_reason = crate::tmux::WAIT_REASON_RESPONSE_READY.into();
        let mut attention = pane("%attention", PaneStatus::Idle);
        attention.attention = true;
        let mut shell = pane("%shell", PaneStatus::Idle);
        shell.bg_shell_cmd = Some("sleep 60".into());
        let mut state = state(vec![
            idle,
            unread,
            attention,
            shell,
            pane("%running", PaneStatus::Running),
            pane("%waiting", PaneStatus::Waiting),
            pane("%error", PaneStatus::Error),
            pane("%background", PaneStatus::Background),
        ]);
        state.toggle_idle_folds();
        assert_eq!(
            ids(&state),
            vec![
                "%unread",
                "%attention",
                "%shell",
                "%running",
                "%waiting",
                "%error",
                "%background"
            ]
        );
        assert_eq!(
            state.layout.pane_row_targets[0],
            RowTarget::Repo {
                repo_id: "/repo".into()
            }
        );
    }

    #[test]
    fn collapse_selected_idle_moves_to_header_and_enter_selects_first_pane() {
        let mut state = state(vec![
            pane("%1", PaneStatus::Idle),
            pane("%2", PaneStatus::Idle),
        ]);
        state.global.selected_pane_row = 1;
        state.toggle_idle_folds();
        assert_eq!(state.layout.pane_row_targets.len(), 1);
        assert_eq!(state.global.selected_pane_row, 0);
        state.activate_selected_pane();
        assert_eq!(ids(&state), vec!["%1", "%2"]);
        assert_eq!(state.global.selected_pane_row, 0);
        assert!(state.global.idle_collapsed);
        // Local expansion cannot invert the next global action.
        state.toggle_idle_folds();
        assert!(!state.global.idle_collapsed);
        assert!(!state.repo_idle_collapsed("/repo"));
        state.toggle_idle_folds();
        assert_eq!(state.layout.pane_row_targets.len(), 1);
    }

    #[test]
    fn new_activity_is_visible_and_focus_does_not_expand_idle() {
        let mut state = state(vec![pane("%1", PaneStatus::Idle)]);
        state.toggle_idle_folds();
        state.focus_state.focused_pane_id = Some("%1".into());
        state.rebuild_row_targets();
        assert!(ids(&state).is_empty());
        state.repo_groups[0].panes[0].0.status = PaneStatus::Running;
        state.rebuild_row_targets();
        assert_eq!(ids(&state), vec!["%1"]);
        assert_eq!(state.global.selected_pane_row, 0);
    }

    #[test]
    fn global_fold_mode_round_trips_options_without_persisting_local_exceptions() {
        let mut state = state(vec![pane("%1", PaneStatus::Idle)]);
        let opts = std::collections::HashMap::from([(
            crate::tmux::SIDEBAR_IDLE_COLLAPSED.to_string(),
            "on".to_string(),
        )]);
        state.global.apply_all(&opts);
        state.rebuild_row_targets();
        assert!(ids(&state).is_empty());
        state.toggle_repo_idle("/repo");
        assert_eq!(ids(&state), vec!["%1"]);
        // Repeated focus synchronization must preserve a local exception.
        state.global.apply_all(&opts);
        state.rebuild_row_targets();
        assert_eq!(ids(&state), vec!["%1"]);
        state.global.apply_all(&std::collections::HashMap::new());
        assert!(!state.global.idle_collapsed);
        assert!(state.global.idle_fold_overrides.is_empty());
    }

    #[test]
    fn mouse_header_toggles_same_fold_without_activating_a_pane() {
        let mut state = state(vec![pane("%1", PaneStatus::Idle)]);
        state.layout.line_to_row = vec![None, Some(0)];
        state.layout.repo_fold_lines = vec![(0, "/repo".into())];
        state.handle_mouse_click(1, 0);
        assert!(state.repo_idle_collapsed("/repo"));
        assert_eq!(state.layout.pane_row_targets.len(), 1);
        state.layout.repo_fold_lines = vec![(0, "/repo".into())];
        state.handle_mouse_click(1, 0);
        assert!(!state.repo_idle_collapsed("/repo"));
        assert_eq!(state.layout.pane_row_targets[0].pane_id(), Some("%1"));
    }
    #[test]
    fn folding_invalidates_old_screen_rows_before_queued_clicks() {
        let mut state = state(vec![
            pane("%idle", PaneStatus::Idle),
            pane("%active", PaneStatus::Running),
        ]);
        state.layout.line_to_row = vec![None, Some(0), Some(1)];
        state.layout.repo_fold_lines = vec![(0, "/repo".into())];
        state.global.selected_pane_row = 0;
        state.toggle_idle_folds();
        assert!(state.layout.line_to_row.is_empty());
        assert!(state.layout.repo_fold_lines.is_empty());
        assert_eq!(state.global.selected_pane_row, 0);
        state.handle_mouse_click(2, 0);
        assert_eq!(state.global.selected_pane_row, 0);
        assert!(matches!(
            state.layout.pane_row_targets[0],
            RowTarget::Repo { .. }
        ));
    }

    fn pane_in(id: &str, repo_root: &str, status: PaneStatus) -> (PaneInfo, PaneGitInfo) {
        (
            pane(id, status),
            PaneGitInfo {
                repo_root: Some(repo_root.into()),
                ..PaneGitInfo::default()
            },
        )
    }

    fn state_with_repos() -> AppState {
        let mut state = AppState::new("%sidebar".into());
        state.repo_groups = vec![
            RepoGroup {
                name: "alpha".into(),
                has_focus: false,
                panes: vec![
                    pane_in("%a-idle", "/repo-a", PaneStatus::Idle),
                    pane_in("%a-running", "/repo-a", PaneStatus::Running),
                ],
            },
            RepoGroup {
                name: "beta".into(),
                has_focus: false,
                panes: vec![pane_in("%b-idle", "/repo-b", PaneStatus::Idle)],
            },
            RepoGroup {
                name: "gamma".into(),
                has_focus: false,
                panes: vec![pane_in("%c-running", "/repo-c", PaneStatus::Running)],
            },
        ];
        state.rebuild_row_targets();
        state
    }

    fn select_pane(state: &mut AppState, pane_id: &str) {
        state.global.selected_pane_row = state
            .layout
            .pane_row_targets
            .iter()
            .position(|target| target.pane_id() == Some(pane_id))
            .expect("pane row target");
    }

    #[test]
    fn selected_repo_toggle_folds_only_that_repo_and_round_trips_from_its_header() {
        let mut state = state_with_repos();
        select_pane(&mut state, "%a-idle");
        state.toggle_selected_repo_idle();
        assert!(state.repo_idle_collapsed("/repo-a"));
        assert!(!state.repo_idle_collapsed("/repo-b"));
        assert!(!state.global.idle_collapsed);
        assert!(ids(&state).contains(&"%b-idle"));
        assert!(ids(&state).contains(&"%a-running"));
        assert!(!ids(&state).contains(&"%a-idle"));
        // Collapsing moves selection to the repository header; the next toggle
        // expands that same repository without touching the global mode.
        assert_eq!(
            state.layout.pane_row_targets[state.global.selected_pane_row],
            RowTarget::Repo {
                repo_id: "/repo-a".into()
            }
        );
        state.toggle_selected_repo_idle();
        assert!(!state.repo_idle_collapsed("/repo-a"));
        assert!(ids(&state).contains(&"%a-idle"));
    }

    #[test]
    fn selecting_a_running_pane_still_folds_its_repo_but_never_a_repo_without_idle() {
        let mut state = state_with_repos();
        select_pane(&mut state, "%a-running");
        state.toggle_selected_repo_idle();
        assert!(state.repo_idle_collapsed("/repo-a"));
        assert!(!ids(&state).contains(&"%a-idle"));

        let before = state.layout.pane_row_targets.clone();
        select_pane(&mut state, "%c-running");
        state.toggle_selected_repo_idle();
        assert!(!state.repo_idle_collapsed("/repo-c"));
        // The foldability guard leaves the navigation targets untouched.
        assert_eq!(state.layout.pane_row_targets, before);
    }

    #[test]
    fn known_resumed_idle_sessions_fold_but_unknown_wait_markers_stay_visible() {
        for reason in ["session_resumed", "session_resumed_compact"] {
            let mut resumed = pane("%1", PaneStatus::Idle);
            resumed.wait_reason = reason.into();
            assert!(is_foldable_idle(&resumed));
            resumed.wait_reason = "future_unknown_reason".into();
            assert!(!is_foldable_idle(&resumed));
        }
    }
}
