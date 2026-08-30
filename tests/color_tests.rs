#[allow(dead_code, unused_imports)]
mod test_helpers;

use ratatui::style::Color;
use test_helpers::*;
use tmux_agent_sidebar::activity::{ActivityEntry, TaskProgress, TaskStatus, ToolColorClass};
use tmux_agent_sidebar::state::{BottomTab, Focus};
use tmux_agent_sidebar::tmux::{AgentType, PaneStatus, PermissionMode, SessionInfo, WindowInfo};
use tmux_agent_sidebar::ui::colors::ColorTheme;

// ─── ColorTheme Default Values ──────────────────────────────────────

#[test]
fn test_all_color_theme_defaults() {
    let theme = ColorTheme::default();

    // Core UI colors
    assert_eq!(theme.accent, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.border_inactive, Color::Rgb(0x50, 0x49, 0x45));
    assert_eq!(theme.selection_bg, Color::Rgb(0x50, 0x49, 0x45));

    // Status colors
    assert_eq!(theme.status_all, Color::Rgb(0xd3, 0x86, 0x9b));
    assert_eq!(theme.status_running, Color::Rgb(0xb8, 0xbb, 0x26));
    assert_eq!(theme.status_background, Color::Rgb(0x8e, 0xc0, 0x7c));
    assert_eq!(theme.status_waiting, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.status_idle, Color::Rgb(0x83, 0xa5, 0x98));
    assert_eq!(theme.status_error, Color::Rgb(0xfb, 0x49, 0x34));
    assert_eq!(theme.status_unknown, Color::Rgb(0x92, 0x83, 0x74));

    // Agent colors
    assert_eq!(theme.agent_claude, Color::Rgb(0xe7, 0x8a, 0x4e));
    assert_eq!(theme.agent_codex, Color::Rgb(0x7d, 0xae, 0xa3));
    assert_eq!(theme.pet_body, Color::Rgb(0xe7, 0x8a, 0x4e));
    assert_eq!(theme.pet_eye, Color::Rgb(0xb8, 0xbb, 0x26));

    // Text colors
    assert_eq!(theme.text_active, Color::Rgb(0xeb, 0xdb, 0xb2));
    assert_eq!(theme.text_muted, Color::Rgb(0x92, 0x83, 0x74));
    assert_eq!(theme.text_inactive, Color::Rgb(0x7c, 0x6f, 0x64));

    // Header/UI element colors
    assert_eq!(theme.session_header, Color::Rgb(0xbd, 0xae, 0x93));
    assert_eq!(theme.port, Color::Rgb(0x7d, 0xae, 0xa3));
    assert_eq!(theme.wait_reason, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.branch, Color::Rgb(0x8e, 0xc0, 0x7c));

    // New theme fields
    assert_eq!(theme.badge_danger, Color::Rgb(0xfb, 0x49, 0x34));
    assert_eq!(theme.badge_auto, Color::Rgb(0xd8, 0xa6, 0x57));
    assert_eq!(theme.task_progress, Color::Rgb(0xd8, 0xa6, 0x57));
    assert_eq!(theme.subagent, Color::Rgb(0x7d, 0xae, 0xa3));
    assert_eq!(theme.commit_hash, Color::Rgb(0x92, 0x83, 0x74));
    assert_eq!(theme.diff_added, Color::Rgb(0xa9, 0xb6, 0x65));
    assert_eq!(theme.diff_deleted, Color::Rgb(0xea, 0x69, 0x62));
    assert_eq!(theme.file_change, Color::Rgb(0xd8, 0xa6, 0x57));
    assert_eq!(theme.pr_link, Color::Rgb(0x7d, 0xae, 0xa3));
}

// ─── status_color() for all PaneStatus variants ─────────────────────

#[test]
fn test_status_color_all_variants() {
    let theme = ColorTheme::default();

    assert_eq!(
        theme.status_color(&PaneStatus::Running, false),
        Color::Rgb(0xb8, 0xbb, 0x26)
    );
    assert_eq!(
        theme.status_color(&PaneStatus::Background, false),
        Color::Rgb(0x8e, 0xc0, 0x7c)
    );
    assert_eq!(
        theme.status_color(&PaneStatus::Waiting, false),
        Color::Rgb(0xfa, 0xbd, 0x2f)
    );
    assert_eq!(
        theme.status_color(&PaneStatus::Idle, false),
        Color::Rgb(0x83, 0xa5, 0x98)
    );
    assert_eq!(
        theme.status_color(&PaneStatus::Error, false),
        Color::Rgb(0xfb, 0x49, 0x34)
    );
    assert_eq!(
        theme.status_color(&PaneStatus::Unknown, false),
        Color::Rgb(0x92, 0x83, 0x74)
    );
}

#[test]
fn test_status_color_attention_overrides_all() {
    let theme = ColorTheme::default();

    // attention=true should always return status_waiting regardless of status
    for status in &[
        PaneStatus::Running,
        PaneStatus::Background,
        PaneStatus::Waiting,
        PaneStatus::Idle,
        PaneStatus::Error,
        PaneStatus::Unknown,
    ] {
        assert_eq!(
            theme.status_color(status, true),
            theme.status_waiting,
            "attention=true should override {:?} to waiting color",
            status
        );
    }
}

// ─── agent_color() for all AgentType variants ───────────────────────

#[test]
fn test_agent_color_all_variants() {
    let theme = ColorTheme::default();

    assert_eq!(
        theme.agent_color(&AgentType::Claude),
        Color::Rgb(0xe7, 0x8a, 0x4e)
    );
    assert_eq!(
        theme.agent_color(&AgentType::Codex),
        Color::Rgb(0x7d, 0xae, 0xa3)
    );
    assert_eq!(theme.agent_color(&AgentType::Unknown), theme.status_unknown);
}

// ─── PermissionMode badge colors ────────────────────────────────────

#[test]
fn test_permission_mode_bypass_all_renders_danger_color() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.permission_mode = PermissionMode::BypassPermissions;

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

    // Styled snapshot locks in the BypassAll `!` badge rendered with
    // badge_danger (fg:167).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 26), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e] [fg:#fb4934]![fg:#fb4934]



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
fn test_permission_mode_full_auto_renders_auto_color() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.permission_mode = PermissionMode::Auto;

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

    // Styled snapshot locks in the Auto `auto` badge rendered with
    // badge_auto (fg:221).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 26), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e] [fg:#d8a657]a[fg:#d8a657]u[fg:#d8a657]t[fg:#d8a657]o[fg:#d8a657]



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
fn test_permission_mode_normal_no_badge() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    // permission_mode is Normal by default in make_pane

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

    // Snapshot locks in that the agent row shows no badge in Normal mode.
    insta::assert_snapshot!(render_to_string(&mut state, 28, 25), @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

// ─── Activity semantic color classes ────────────────────────────────

#[test]
fn test_tool_color_all_branches() {
    let cases = vec![
        ("Edit", ToolColorClass::Edit),
        ("Write", ToolColorClass::Edit),
        ("Bash", ToolColorClass::Command),
        ("Read", ToolColorClass::Read),
        ("Glob", ToolColorClass::Read),
        ("Grep", ToolColorClass::Read),
        ("Agent", ToolColorClass::Agent),
        ("UnknownTool", ToolColorClass::Unknown),
        ("", ToolColorClass::Unknown),
    ];
    for (tool, expected) in cases {
        let entry = ActivityEntry {
            timestamp: "10:00".into(),
            tool: tool.into(),
            label: "test".into(),
        };
        assert_eq!(
            entry.tool_color_class(),
            expected,
            "tool_color_class for '{}'",
            tool
        );
    }
}

// ─── Git status summary colors ──────────────────────────────────────

#[test]
fn test_git_summary_modified_uses_badge_auto_color() {
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

    state.bottom_tab = BottomTab::GitStatus;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.git.branch = "main".into();
    state.git.unstaged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "src/lib.rs".into(),
        additions: 5,
        deletions: 2,
        path: String::new(),
    }];

    // Styled snapshot locks in the Modified file badge color
    // (badge_auto fg:221).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]


    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]m[fg:#ebdbb2]a[fg:#ebdbb2]i[fg:#ebdbb2]n[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]1[fg:#928374] [fg:#928374]f[fg:#928374]i[fg:#928374]l[fg:#928374]e[fg:#928374]s[fg:#928374]│[fg:#fabd2f]
    │[fg:#fabd2f]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]│[fg:#fabd2f]
    │[fg:#fabd2f]U[fg:#bdae93]n[fg:#bdae93]s[fg:#bdae93]t[fg:#bdae93]a[fg:#bdae93]g[fg:#bdae93]e[fg:#bdae93]d[fg:#bdae93] [fg:#bdae93]([fg:#bdae93]1[fg:#bdae93])[fg:#bdae93] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]M[fg:#d8a657] [fg:#fabd2f]s[fg:#928374]r[fg:#928374]c[fg:#928374]/[fg:#928374]l[fg:#928374]i[fg:#928374]b[fg:#928374].[fg:#928374]r[fg:#928374]s[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]+[fg:#a9b665]5[fg:#a9b665]/[fg:#928374]-[fg:#ea6962]2[fg:#ea6962]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
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

// ─── Render Tests: verify correct colors in styled output ───────────

#[test]
fn test_task_progress_line_uses_task_progress_color() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.pane_id = "%1".into();

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

    // Set task progress for pane %1
    let progress = TaskProgress {
        tasks: vec![
            ("Task A".into(), TaskStatus::Completed),
            ("Task B".into(), TaskStatus::InProgress),
            ("Task C".into(), TaskStatus::Pending),
        ],
    };
    state.set_pane_task_progress("%1", Some(progress));

    // Styled snapshot locks in both the task_progress color (fg:223) and
    // the progress glyphs (✔/◼/◻) with the "1/3" count.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 40, 40), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]      —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
       [fg:#d8a657] [fg:#d8a657]✔[fg:#d8a657]◼[fg:#d8a657]◻[fg:#d8a657] [fg:#d8a657]1[fg:#d8a657]/[fg:#d8a657]3[fg:#d8a657]
















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

#[test]
fn test_subagent_line_uses_subagent_color() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.subagents = vec!["Explore #1".into()];

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

    // Styled snapshot locks in the subagent line color (fg:73) plus the
    // rendered "Explore #1" label.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 40, 27), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]      —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
       [fg:#928374] [fg:#928374]└[fg:#928374] [fg:#928374]E[fg:#7daea3]x[fg:#7daea3]p[fg:#7daea3]l[fg:#7daea3]o[fg:#7daea3]r[fg:#7daea3]e[fg:#7daea3] [fg:#7daea3]#[fg:#7daea3]1[fg:#7daea3]



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

#[test]
fn test_response_arrow_uses_response_arrow_color() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Idle);
    pane.pane_active = false;
    pane.prompt = "Task completed successfully".into();
    pane.prompt_is_response = true;

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

    // Styled snapshot locks in:
    //   • response_arrow color (fg:81) + bold on the ▷ glyph
    //   • text_active color (fg:255) on the focused response text
    insta::assert_snapshot!(render_to_styled_string(&mut state, 40, 27), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]      —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#83a598] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
      ▷[fg:#89b482,bold] [fg:#89b482,bold]T[fg:#ebdbb2]a[fg:#ebdbb2]s[fg:#ebdbb2]k[fg:#ebdbb2] [fg:#ebdbb2]c[fg:#ebdbb2]o[fg:#ebdbb2]m[fg:#ebdbb2]p[fg:#ebdbb2]l[fg:#ebdbb2]e[fg:#ebdbb2]t[fg:#ebdbb2]e[fg:#ebdbb2]d[fg:#ebdbb2] [fg:#ebdbb2]s[fg:#ebdbb2]u[fg:#ebdbb2]c[fg:#ebdbb2]c[fg:#ebdbb2]e[fg:#ebdbb2]s[fg:#ebdbb2]s[fg:#ebdbb2]f[fg:#ebdbb2]u[fg:#ebdbb2]l[fg:#ebdbb2]l[fg:#ebdbb2]y[fg:#ebdbb2]



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

// test_commit_hash_uses_commit_hash_color was removed because
// git_last_commit and commit hash rendering no longer exist.

#[test]
fn test_pr_link_uses_pr_link_color() {
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

    state.bottom_tab = BottomTab::GitStatus;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.git.branch = "feature/test".into();
    state.git.pr_number = Some("99".into());
    state.git.remote_url = "https://github.com/user/repo".into();

    // Styled snapshot locks in the PR link: pr_link color (fg:117) plus
    // the underline modifier on the `#99` glyphs.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 40), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]

















    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]f[fg:#ebdbb2]e[fg:#ebdbb2]a[fg:#ebdbb2]t[fg:#ebdbb2]u[fg:#ebdbb2]r[fg:#ebdbb2]e[fg:#ebdbb2]/[fg:#ebdbb2]t[fg:#ebdbb2]e[fg:#ebdbb2]s[fg:#ebdbb2]t[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]#[fg:#7daea3,underline]9[fg:#7daea3,underline]9[fg:#7daea3,underline]│[fg:#fabd2f]
    │[fg:#fabd2f]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
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
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn test_diff_stat_added_uses_diff_added_color() {
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

    state.bottom_tab = BottomTab::GitStatus;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.git.branch = "main".into();
    state.git.diff_stat = Some((42, 10));

    // Styled snapshot locks in the `+42` additions rendered with
    // diff_added color (fg:114).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 40), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]

















    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]m[fg:#ebdbb2]a[fg:#ebdbb2]i[fg:#ebdbb2]n[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]+[fg:#a9b665]4[fg:#a9b665]2[fg:#a9b665]/[fg:#928374]-[fg:#ea6962]1[fg:#ea6962]0[fg:#ea6962] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]0[fg:#928374] [fg:#928374]f[fg:#928374]i[fg:#928374]l[fg:#928374]e[fg:#928374]s[fg:#928374]│[fg:#fabd2f]
    │[fg:#fabd2f]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
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
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn test_diff_stat_deleted_uses_diff_deleted_color() {
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

    state.bottom_tab = BottomTab::GitStatus;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.git.branch = "main".into();
    state.git.diff_stat = Some((0, 25));

    // Styled snapshot locks in the `-25` deletions rendered with
    // diff_deleted color (fg:174).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]


    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]m[fg:#ebdbb2]a[fg:#ebdbb2]i[fg:#ebdbb2]n[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]+[fg:#a9b665]0[fg:#a9b665]/[fg:#928374]-[fg:#ea6962]2[fg:#ea6962]5[fg:#ea6962] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]0[fg:#928374] [fg:#928374]f[fg:#928374]i[fg:#928374]l[fg:#928374]e[fg:#928374]s[fg:#928374]│[fg:#fabd2f]
    │[fg:#fabd2f]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
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
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn test_file_change_stat_uses_file_change_color() {
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

    state.bottom_tab = BottomTab::GitStatus;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.git.branch = "main".into();
    state.git.unstaged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "lib.rs".into(),
        additions: 40,
        deletions: 10,
        path: String::new(),
    }];

    // Styled snapshot locks in the `M lib.rs` row rendered with
    // badge_auto (fg:221) on the status glyph.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]


    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]m[fg:#ebdbb2]a[fg:#ebdbb2]i[fg:#ebdbb2]n[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]1[fg:#928374] [fg:#928374]f[fg:#928374]i[fg:#928374]l[fg:#928374]e[fg:#928374]s[fg:#928374]│[fg:#fabd2f]
    │[fg:#fabd2f]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]─[fg:#504945]│[fg:#fabd2f]
    │[fg:#fabd2f]U[fg:#bdae93]n[fg:#bdae93]s[fg:#bdae93]t[fg:#bdae93]a[fg:#bdae93]g[fg:#bdae93]e[fg:#bdae93]d[fg:#bdae93] [fg:#bdae93]([fg:#bdae93]1[fg:#bdae93])[fg:#bdae93] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f]M[fg:#d8a657] [fg:#fabd2f]l[fg:#928374]i[fg:#928374]b[fg:#928374].[fg:#928374]r[fg:#928374]s[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]+[fg:#a9b665]4[fg:#a9b665]0[fg:#a9b665]/[fg:#928374]-[fg:#ea6962]1[fg:#ea6962]0[fg:#ea6962]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
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

// ─── Custom theme overrides for new fields ──────────────────────────

#[test]
fn test_custom_theme_new_fields_override() {
    let theme = ColorTheme {
        badge_danger: Color::Indexed(196),
        badge_auto: Color::Indexed(226),
        task_progress: Color::Indexed(200),
        subagent: Color::Indexed(100),
        commit_hash: Color::Indexed(150),
        diff_added: Color::Indexed(46),
        diff_deleted: Color::Indexed(160),
        file_change: Color::Indexed(208),
        pr_link: Color::Indexed(33),
        port: Color::Indexed(82),
        ..ColorTheme::default()
    };

    assert_eq!(theme.badge_danger, Color::Indexed(196));
    assert_eq!(theme.badge_auto, Color::Indexed(226));
    assert_eq!(theme.task_progress, Color::Indexed(200));
    assert_eq!(theme.subagent, Color::Indexed(100));
    assert_eq!(theme.commit_hash, Color::Indexed(150));
    assert_eq!(theme.diff_added, Color::Indexed(46));
    assert_eq!(theme.diff_deleted, Color::Indexed(160));
    assert_eq!(theme.file_change, Color::Indexed(208));
    assert_eq!(theme.pr_link, Color::Indexed(33));
    assert_eq!(theme.port, Color::Indexed(82));

    // Original fields should still be default
    assert_eq!(theme.accent, Color::Rgb(0xfa, 0xbd, 0x2f));
    assert_eq!(theme.agent_claude, Color::Rgb(0xe7, 0x8a, 0x4e));
    assert_eq!(theme.pet_body, Color::Rgb(0xe7, 0x8a, 0x4e));
    assert_eq!(theme.pet_eye, Color::Rgb(0xb8, 0xbb, 0x26));
}

// ─── Branch color in styled output ──────────────────────────────────

#[test]
fn test_branch_color_in_agent_panel() {
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
    state.repo_groups = vec![tmux_agent_sidebar::group::RepoGroup {
        name: "project".into(),
        has_focus: true,
        panes: vec![(
            pane,
            tmux_agent_sidebar::group::PaneGitInfo {
                repo_root: Some("/home/user/project".into()),
                branch: Some("feature/cool-feature".into()),
                is_worktree: false,
                worktree_name: None,
            },
        )],
    }];
    state.rebuild_row_targets();
    state.focus_state.sidebar_focused = false;
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the branch name rendered with branch color
    // (fg:109).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 40, 26), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]      —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]                                +[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]           f[fg:#8ec07c]e[fg:#8ec07c]a[fg:#8ec07c]t[fg:#8ec07c]u[fg:#8ec07c]r[fg:#8ec07c]e[fg:#8ec07c]/[fg:#8ec07c]c[fg:#8ec07c]o[fg:#8ec07c]o[fg:#8ec07c]l[fg:#8ec07c]-[fg:#8ec07c]f[fg:#8ec07c]e[fg:#8ec07c]a[fg:#8ec07c]t[fg:#8ec07c]u[fg:#8ec07c]…[fg:#8ec07c]
    ");
}

// ─── Selection background color ─────────────────────────────────────

#[test]
fn test_selection_bg_color_applied() {
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

    // Styled snapshot locks in the selected agent row's selection
    // background (bg:239).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
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

// ─── Accent (focused) vs border_inactive colors ─────────────────────

#[test]
fn test_accent_vs_border_inactive_colors() {
    let mut pane1 = make_pane(AgentType::Claude, PaneStatus::Running);
    pane1.pane_id = "%1".into();
    let mut pane2 = make_pane(AgentType::Codex, PaneStatus::Idle);
    pane2.pane_id = "%2".into();

    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@0".into(),
            window_name: "fish".into(),
            window_active: true,
            auto_rename: true,
            panes: vec![pane1.clone(), pane2.clone()],
        }],
    }]);
    state.repo_groups = vec![
        tmux_agent_sidebar::group::RepoGroup {
            name: "focused-repo".into(),
            has_focus: true,
            panes: vec![(pane1, tmux_agent_sidebar::group::PaneGitInfo::default())],
        },
        tmux_agent_sidebar::group::RepoGroup {
            name: "unfocused-repo".into(),
            has_focus: false,
            panes: vec![(pane2, tmux_agent_sidebar::group::PaneGitInfo::default())],
        },
    ];
    state.focus_state.focused_pane_id = Some("%1".into());
    state.rebuild_row_targets();

    // Styled snapshot locks in:
    //   • focused group header rendered with accent (fg:153)
    //   • unfocused group header rendered with border_inactive (fg:240)
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 30), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]2[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    f[fg:#fabd2f]o[fg:#fabd2f]c[fg:#fabd2f]u[fg:#fabd2f]s[fg:#fabd2f]e[fg:#fabd2f]d[fg:#fabd2f]-[fg:#fabd2f]r[fg:#fabd2f]e[fg:#fabd2f]p[fg:#fabd2f]o[fg:#fabd2f]
    ┃[fg:#fabd2f,bg:#504945] [bg:#504945][fg:#b8bb26,bg:#504945] [fg:#e78a4e,bg:#504945]c[fg:#e78a4e,bg:#504945]l[fg:#e78a4e,bg:#504945]a[fg:#e78a4e,bg:#504945]u[fg:#e78a4e,bg:#504945]d[fg:#e78a4e,bg:#504945]e[fg:#e78a4e,bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945]
    u[fg:#bdae93]n[fg:#bdae93]f[fg:#bdae93]o[fg:#bdae93]c[fg:#bdae93]u[fg:#bdae93]s[fg:#bdae93]e[fg:#bdae93]d[fg:#bdae93]-[fg:#bdae93]r[fg:#bdae93]e[fg:#bdae93]p[fg:#bdae93]o[fg:#bdae93]
      [fg:#83a598] [fg:#7daea3]c[fg:#7daea3]o[fg:#7daea3]d[fg:#7daea3]e[fg:#7daea3]x[fg:#7daea3]





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

// ─── Status color rendering for each PaneStatus ─────────────────────

#[test]
fn test_running_status_color_in_output() {
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
    state.bottom_panel_height = 0;

    // Styled snapshot locks the running glyph to status_running.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#b8bb26] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
    ");
}

#[test]
fn test_waiting_status_color_in_output() {
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
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the waiting status using status_waiting
    // color (fg:221).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#fabd2f] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
    ");
}

#[test]
fn test_error_status_color_in_output() {
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
    state.bottom_panel_height = 0;

    // Styled snapshot locks in the error status using status_error
    // color (fg:167).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#fb4934,bold] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]
    ");
}

#[test]
fn test_idle_status_color_in_output() {
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
    state.focus_state.sidebar_focused = false;

    // Styled snapshot locks in the idle status using status_idle
    // color (fg:110).
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 25), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bold] [fg:#d3869b,bold]1[fg:#d3869b,bold]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    p[fg:#fabd2f]r[fg:#fabd2f]o[fg:#fabd2f]j[fg:#fabd2f]e[fg:#fabd2f]c[fg:#fabd2f]t[fg:#fabd2f]
    ┃[fg:#fabd2f] [fg:#83a598] [fg:#e78a4e]c[fg:#e78a4e]l[fg:#e78a4e]a[fg:#e78a4e]u[fg:#e78a4e]d[fg:#e78a4e]e[fg:#e78a4e]


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
fn test_unknown_status_color_in_output() {
    let theme = tmux_agent_sidebar::ui::colors::ColorTheme::default();
    assert_eq!(
        theme.status_color(&PaneStatus::Unknown, false),
        ratatui::style::Color::Rgb(0x92, 0x83, 0x74)
    );
}

#[test]
fn default_theme_contains_no_indexed_colors() {
    let theme = ColorTheme::default();
    assert!(!format!("{theme:?}").contains("Indexed"));
}
