#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::state::Focus;
use tmux_agent_sidebar::tmux::{AgentType, PaneStatus, SessionInfo, WindowInfo};
use tmux_agent_sidebar::ui::colors::ColorTheme;
use tmux_agent_sidebar::ui::icons::StatusIcons;

// ─── Agents: auto-scroll behavior Tests ─────────────────────────────

#[test]
fn test_agents_auto_scroll_keeps_selected_visible() {
    // Create enough agents to overflow a small viewport
    let mut panes = Vec::new();
    for i in 0..10 {
        let mut pane = make_pane(AgentType::Claude, PaneStatus::Idle);
        pane.pane_id = format!("%{}", i);
        panes.push(pane);
    }

    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: panes.clone(),
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", panes)];
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.rebuild_row_targets();

    // Render with a small height. With the single-row header, the first pane
    // still stays visible without needing to scroll.
    let _ = render_to_string(&mut state, 28, 26);
    assert_eq!(state.scrolls.panes.offset, 0, "initially at top");

    // Select last agent and re-render
    state.global.selected_pane_row = 9;
    let _ = render_to_string(&mut state, 28, 26);
    assert!(
        state.scrolls.panes.offset > 0,
        "should scroll down to show selected agent"
    );
}

#[test]
fn test_panes_scroll_offset_tracks_total_and_visible() {
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

    let _ = render_to_string(&mut state, 28, 26);
    // After rendering, panes_scroll.total_lines and panes_scroll.visible_height should be set
    assert!(
        state.scrolls.panes.total_lines > 0,
        "total lines should be populated"
    );
    assert!(
        state.scrolls.panes.visible_height > 0,
        "visible height should be populated"
    );
}

// ─── Agents: Codex agent color ──────────────────────────────────────

#[test]
fn snapshot_codex_agent_styled() {
    let theme = ColorTheme::default();
    assert_eq!(
        theme.agent_color(&AgentType::Codex),
        ratatui::style::Color::Rgb(0x7d, 0xae, 0xa3)
    );
}

// ─── Agents: Unknown agent type ─────────────────────────────────────

#[test]
fn snapshot_unknown_agent_styled() {
    let theme = ColorTheme::default();
    assert_eq!(
        theme.agent_color(&AgentType::Unknown),
        ratatui::style::Color::Rgb(0x92, 0x83, 0x74)
    );
}

// ─── Agents: running icon variants via render ───────────────────────

#[test]
fn test_running_icon() {
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
    state.focus_state.sidebar_focused = false;
    insta::assert_snapshot!(render_to_string(&mut state, 28, 25), @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn test_waiting_icon() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Waiting);
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
    state.focus_state.sidebar_focused = false;

    insta::assert_snapshot!(render_to_string(&mut state, 28, 25), @"
       1   0   0   1    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn test_error_icon() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Error);
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
    state.focus_state.sidebar_focused = false;

    insta::assert_snapshot!(render_to_string(&mut state, 28, 25), @"
       1   0   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn test_unknown_status_icon() {
    let icons = StatusIcons::default();
    assert_eq!(icons.status_icon(&PaneStatus::Unknown), "");
}

// ─── Agents: auto-scroll keeps selected pane visible ───────────────

#[test]
fn test_agents_auto_scroll_shows_last_selected_pane() {
    // When the last agent in a group is selected, the auto-scroll
    // should bring it into view (the selection marker must be visible).
    let mut panes = Vec::new();
    for i in 0..6 {
        let mut pane = make_pane(AgentType::Claude, PaneStatus::Idle);
        pane.pane_id = format!("%{}", i);
        panes.push(pane);
    }

    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: panes.clone(),
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", panes)];
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.rebuild_row_targets();

    // Select the last agent
    state.global.selected_pane_row = 5;
    // Use a tight height so agents area is small (height - 1 margin - 20 bottom)
    let _ = render_to_string(&mut state, 28, 26);

    // Auto-scroll should have moved forward to keep the last-selected pane visible.
    assert!(
        state.scrolls.panes.offset > 0,
        "selecting the last agent should scroll the list"
    );
}

#[test]
fn test_agents_auto_scroll_up_shows_group_header() {
    // After scrolling down, selecting the first agent should scroll
    // back up enough to show the group header.
    let mut panes = Vec::new();
    for i in 0..8 {
        let mut pane = make_pane(AgentType::Claude, PaneStatus::Idle);
        pane.pane_id = format!("%{}", i);
        panes.push(pane);
    }

    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: panes.clone(),
        }],
    }]);
    state.repo_groups = vec![make_repo_group("project", panes)];
    state.focus_state.sidebar_focused = true;
    state.focus_state.focus = Focus::Panes;
    state.rebuild_row_targets();

    // Scroll to bottom
    state.global.selected_pane_row = 7;
    let _ = render_to_string(&mut state, 28, 26);
    assert!(state.scrolls.panes.offset > 0, "should have scrolled down");

    // Now select first agent and re-render
    state.global.selected_pane_row = 0;
    // The snapshot locks in that the `project` repo header is visible after
    // scrolling back up to the first agent.
    insta::assert_snapshot!(render_to_string(&mut state, 28, 26), @"
       8   0   0   0    — ▾
    project
       claude
    ┃  claude
       claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

// ─── Repo popup rendering ───────────────────────────────────────────

#[test]
fn repo_popup_renders_repo_names_when_open() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "frontend".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![
        make_repo_group("frontend", vec![pane.clone()]),
        make_repo_group("backend", vec![pane.clone()]),
    ];
    state.rebuild_row_targets();
    state.popup = tmux_agent_sidebar::state::PopupState::Repo {
        selected: 0,
        query: String::new(),
        area: None,
    };

    // The snapshot locks in that the popup lists the `All` entry plus both
    // repo names when opened.
    insta::assert_snapshot!(render_to_string(&mut state, 40, 30), @"
       2   0   0   0   2   0      — ▾
    frontend                    ┌──────────┐
    ┃  claude                  │/         │
                                │ All      │
    backend                     │ frontend │
    ┃  claude                  │ backend  │
                                └──────────┘
    ╭ Activity │ Git ──────────────────────╮
    │            No activity yet           │
    ╰──────────────────────────────────────╯
    ");
    // The popup area is required for click hit-testing and is non-visual
    // state, so it stays as a direct assertion.
    assert!(
        state.repo_popup_area().is_some(),
        "render should populate repo popup area for hit-testing"
    );
}

#[test]
fn repo_popup_filters_repo_names_from_query() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![]);
    state.repo_groups = vec![
        make_repo_group("sidebar-api", vec![pane.clone()]),
        make_repo_group("tmux-agent-sidebar", vec![pane.clone()]),
        make_repo_group("website", vec![pane]),
    ];
    state.rebuild_row_targets();
    state.popup = tmux_agent_sidebar::state::PopupState::Repo {
        selected: 0,
        query: "sidebar".into(),
        area: None,
    };

    insta::assert_snapshot!(render_to_string(&mut state, 40, 30), @"
       3   0   0   0   3   0      — ▾
    sidebar-api       ┌────────────────────┐
    ┃  claude        │/ sidebar           │
                      │ sidebar-api        │
    tmux-agent-sidebar│ tmux-agent-sidebar │
    ┃  claude        └────────────────────┘
    website
    ┃  claude
    ╭ Activity │ Git ──────────────────────╮
    │            No activity yet           │
    ╰──────────────────────────────────────╯
    ");
}

#[test]
fn repo_popup_renders_no_matches_for_empty_result() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![]);
    state.repo_groups = vec![make_repo_group("frontend", vec![pane])];
    state.rebuild_row_targets();
    state.popup = tmux_agent_sidebar::state::PopupState::Repo {
        selected: 0,
        query: "missing".into(),
        area: None,
    };

    insta::assert_snapshot!(render_to_string(&mut state, 40, 30), @"
       1   0   0   0   1   0      — ▾
    frontend                  ┌────────────┐
    ┃  claude                │/ missing   │
                              │ No matches │
                              └────────────┘
    ╭ Activity │ Git ──────────────────────╮
    │            No activity yet           │
    ╰──────────────────────────────────────╯
    ");
}

#[test]
fn repo_popup_highlights_selected_entry_with_background() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "frontend".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    state.repo_groups = vec![
        make_repo_group("frontend", vec![pane.clone()]),
        make_repo_group("backend", vec![pane.clone()]),
    ];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = false; // surface raw colors instead of REVERSED
    state.popup = tmux_agent_sidebar::state::PopupState::Repo {
        selected: 2, // "backend" (0=All, 1=frontend, 2=backend)
        query: String::new(),
        area: None,
    };

    // Styled snapshot locks in that the `backend` row carries the selection
    // background (bg:239) on each cell of the entry.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 40, 30), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]2[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]2[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]      —[fg:#ebdbb2] ▾[fg:#ebdbb2]
    f[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]n[fg:#fabd2f]t[fg:#fabd2f]e[fg:#fabd2f]n[fg:#fabd2f]d[fg:#fabd2f]                    ┌[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]┐[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#83a598] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]                  │[fg:#fabd2f]/[fg:#fabd2f] [fg:#fabd2f]        │[fg:#fabd2f]
                                │[fg:#fabd2f] [fg:#ebdbb2]A[fg:#ebdbb2]l[fg:#ebdbb2]l[fg:#ebdbb2] [fg:#ebdbb2] [fg:#ebdbb2] [fg:#ebdbb2] [fg:#ebdbb2] [fg:#ebdbb2] [fg:#ebdbb2]│[fg:#fabd2f]
    b[fg:#fabd2f]a[fg:#fabd2f]c[fg:#fabd2f]k[fg:#fabd2f]e[fg:#fabd2f]n[fg:#fabd2f]d[fg:#fabd2f]                     │[fg:#fabd2f] [fg:#928374]f[fg:#928374]r[fg:#928374]o[fg:#928374]n[fg:#928374]t[fg:#928374]e[fg:#928374]n[fg:#928374]d[fg:#928374] [fg:#928374]│[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#83a598] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]                  │[fg:#fabd2f] [fg:#ebdbb2,bg:#504945]b[fg:#ebdbb2,bg:#504945]a[fg:#ebdbb2,bg:#504945]c[fg:#ebdbb2,bg:#504945]k[fg:#ebdbb2,bg:#504945]e[fg:#ebdbb2,bg:#504945]n[fg:#ebdbb2,bg:#504945]d[fg:#ebdbb2,bg:#504945] [fg:#ebdbb2,bg:#504945] [fg:#ebdbb2,bg:#504945]│[fg:#fabd2f]
                                └[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]┘[fg:#fabd2f]



    ╭[fg:#504945] [fg:#504945]A[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]i[fg:#fabd2f]v[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f]y[fg:#fabd2f] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#928374]i[fg:#928374]t[fg:#928374] [fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╮[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]N[fg:#928374]o[fg:#928374] [fg:#928374]a[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#928374]y[fg:#928374]e[fg:#928374]t[fg:#928374] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    │[fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945] [fg:#504945]│[fg:#504945]
    ╰[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]╯[fg:#504945]
    ");
}
