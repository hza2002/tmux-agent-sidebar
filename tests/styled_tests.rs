#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::activity::ActivityEntry;
use tmux_agent_sidebar::state::{BottomTab, Focus};
use tmux_agent_sidebar::tmux::{AgentType, PaneStatus, SessionInfo, WindowInfo};

// ─── Styled Snapshot Tests for Selection and Focus ─────────────────

#[test]
fn snapshot_selected_focused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.global.selected_pane_row = 0;
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the selected row's ┃[fg:153,bg:239] marker
    // and the selection background spanning its content cells.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 10), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f,bg:#504945] [bg:#504945][fg:#83a598,bg:#504945] [fg:#e78a4e,bg:#504945]c[fg:#e78a4e,bg:#504945]l[fg:#e78a4e,bg:#504945]a[fg:#e78a4e,bg:#504945]u[fg:#e78a4e,bg:#504945]d[fg:#e78a4e,bg:#504945]e[fg:#e78a4e,bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945]
    ");
}

#[test]
fn snapshot_activity_focused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    // Styled snapshot locks in the focused group header accent (fg:153) and
    // the active-panel border color.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]

    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]1[fg:#7c6f64]0[fg:#7c6f64]:[fg:#7c6f64]3[fg:#7c6f64]2[fg:#7c6f64] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]E[fg:#d8a657]d[fg:#d8a657]i[fg:#d8a657]t[fg:#d8a657]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#928374] [fg:#928374]s[fg:#928374]r[fg:#928374]c[fg:#928374]/[fg:#928374]m[fg:#928374]a[fg:#928374]i[fg:#928374]n[fg:#928374].[fg:#928374]r[fg:#928374]s[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn snapshot_activity_unfocused_styled() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::Panes; // not activity
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    // Styled snapshot locks in the unfocused bottom-panel border
    // (border_inactive fg:240).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]

    ╭[fg:#504945] [fg:#504945]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╮[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945]1[fg:#7c6f64]0[fg:#7c6f64]:[fg:#7c6f64]3[fg:#7c6f64]2[fg:#7c6f64] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]E[fg:#d8a657]d[fg:#d8a657]i[fg:#d8a657]t[fg:#d8a657]│[fg:#504945]
    │[fg:#504945] [fg:#928374] [fg:#928374]s[fg:#928374]r[fg:#928374]c[fg:#928374]/[fg:#928374]m[fg:#928374]a[fg:#928374]i[fg:#928374]n[fg:#928374].[fg:#928374]r[fg:#928374]s[fg:#928374] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    ╰[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╯[fg:#504945]
    ");
}

#[test]
fn bottom_tab_activity_uses_accent_when_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.bottom_tab = BottomTab::Activity;

    // Styled snapshot locks in `A` using accent (fg:153) and `G` remaining
    // muted (fg:252) on the bottom-panel tab title row.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]

    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]N[fg:#928374]o[fg:#928374] [fg:#928374]a[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#928374]y[fg:#928374]e[fg:#928374]t[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn bottom_tab_git_uses_accent_when_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.bottom_tab = BottomTab::GitStatus;

    // Styled snapshot locks in `G` using accent (fg:153) and `A` remaining
    // muted (fg:252) on the bottom-panel tab title row.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]

    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]W[fg:#928374]o[fg:#928374]r[fg:#928374]k[fg:#928374]i[fg:#928374]n[fg:#928374]g[fg:#928374] [fg:#928374]t[fg:#928374]r[fg:#928374]e[fg:#928374]e[fg:#928374] [fg:#928374]c[fg:#928374]l[fg:#928374]e[fg:#928374]a[fg:#928374]n[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

// ─── Selection Background Border Tests ───────────────────────────────

#[test]
fn selection_marker_uses_accent_color_with_selection_bg() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.global.selected_pane_row = 0;

    // Styled snapshot locks in:
    //   1. the selected row begins with `┃[fg:153,bg:239]` (accent + selection bg)
    //   2. the selected row never contains the old frame `│`
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f,bg:#504945] [bg:#504945][fg:#b8bb26,bg:#504945] [fg:#e78a4e,bg:#504945]c[fg:#e78a4e,bg:#504945]l[fg:#e78a4e,bg:#504945]a[fg:#e78a4e,bg:#504945]u[fg:#e78a4e,bg:#504945]d[fg:#e78a4e,bg:#504945]e[fg:#e78a4e,bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945]

    ╭[fg:#504945] [fg:#504945]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╮[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]N[fg:#928374]o[fg:#928374] [fg:#928374]a[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#928374]y[fg:#928374]e[fg:#928374]t[fg:#928374] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    ╰[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╯[fg:#504945]
    ");
}

#[test]
fn selection_bg_covers_inner_padding() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.global.selected_pane_row = 0;

    // Styled snapshot locks in the selection background extending across the
    // inner padding immediately after the `┃` marker (` [bg:239]`).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f,bg:#504945] [bg:#504945][fg:#83a598,bg:#504945] [fg:#e78a4e,bg:#504945]c[fg:#e78a4e,bg:#504945]l[fg:#e78a4e,bg:#504945]a[fg:#e78a4e,bg:#504945]u[fg:#e78a4e,bg:#504945]d[fg:#e78a4e,bg:#504945]e[fg:#e78a4e,bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945]

    ╭[fg:#504945] [fg:#504945]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╮[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]N[fg:#928374]o[fg:#928374] [fg:#928374]a[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#928374]y[fg:#928374]e[fg:#928374]t[fg:#928374] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    ╰[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╯[fg:#504945]
    ");
}

#[test]
fn no_selection_bg_when_not_selected() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = false; // not focused → no selection

    // Styled snapshot locks in the absence of any selection background
    // (bg:239) while the sidebar is not focused.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 24), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]

    ╭[fg:#504945] [fg:#504945]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╮[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]N[fg:#928374]o[fg:#928374] [fg:#928374]a[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#928374]y[fg:#928374]e[fg:#928374]t[fg:#928374] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    ╰[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╯[fg:#504945]
    ");
}

// ─── Custom Theme Tests ─────────────────────────────────────────────

#[test]
fn snapshot_custom_theme_colors() {
    use ratatui::style::Color;
    use tmux_agent_sidebar::ui::colors::ColorTheme;

    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", vec![pane])];
    state.rebuild_row_targets();

    // Override theme with custom colors
    state.theme = ColorTheme {
        accent: Color::Indexed(196),       // red accent
        agent_claude: Color::Indexed(226), // yellow agent
        status_idle: Color::Indexed(46),   // green idle
        port: Color::Indexed(39),          // cyan port
        ..ColorTheme::default()
    };
    // Unfocus sidebar so selected row doesn't use REVERSED (which hides colors)
    state.focus_state.sidebar_focused = false;
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the custom theme colors (accent fg:196,
    // agent_claude fg:226, status_idle fg:46).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 10), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:196]r[fg:196]o[fg:196]j[fg:196]e[fg:196]c[fg:196]t[fg:196]
    ┃[fg:196] [fg:46] [fg:226]c[fg:226]l[fg:226]a[fg:226]u[fg:226]d[fg:226]e[fg:226]
    ");
}

#[test]
fn test_theme_default_matches_gruvbox_truecolor_palette() {
    use ratatui::style::Color;
    use tmux_agent_sidebar::ui::colors::ColorTheme;

    let theme = ColorTheme::default();

    assert_eq!(theme.accent, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.border_inactive, Color::Rgb(0x50, 0x49, 0x45));
    assert_eq!(theme.status_running, Color::Rgb(0xb8, 0xbb, 0x26));
    assert_eq!(theme.status_background, Color::Rgb(0x8e, 0xc0, 0x7c));
    assert_eq!(theme.status_waiting, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.status_idle, Color::Rgb(0x83, 0xa5, 0x98));
    assert_eq!(theme.status_error, Color::Rgb(0xfb, 0x49, 0x34));
    assert_eq!(theme.agent_claude, Color::Rgb(0xe7, 0x8a, 0x4e));
    assert_eq!(theme.agent_codex, Color::Rgb(0x7d, 0xae, 0xa3));
    assert_eq!(theme.text_active, Color::Rgb(0xeb, 0xdb, 0xb2));
    assert_eq!(theme.text_muted, Color::Rgb(0x92, 0x83, 0x74));
    assert_eq!(theme.session_header, Color::Rgb(0xbd, 0xae, 0x93));
    assert_eq!(theme.wait_reason, Color::Rgb(0xfa, 0xbd, 0x2f));
}
