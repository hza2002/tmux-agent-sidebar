#[allow(dead_code, unused_imports)]
mod test_helpers;

use test_helpers::*;
use tmux_agent_sidebar::activity::ActivityEntry;
use tmux_agent_sidebar::state::{BottomTab, Focus};
use tmux_agent_sidebar::tmux::{AgentType, PaneStatus, SessionInfo, WindowInfo};

// ─── Bottom Tab Tests ──────────────────────────────────────────────

#[test]
fn test_next_bottom_tab() {
    let mut state = make_state(vec![]);
    assert_eq!(state.bottom_tab, BottomTab::Activity);
    state.next_bottom_tab();
    assert_eq!(state.bottom_tab, BottomTab::GitStatus);
    state.next_bottom_tab();
    assert_eq!(state.bottom_tab, BottomTab::Activity);
}

#[test]
fn test_scroll_bottom_dispatches() {
    let mut state = make_state(vec![]);

    // Set up activity scroll state
    state.activity.entries = vec![
        ActivityEntry {
            timestamp: "10:00".into(),
            tool: "Read".into(),
            label: "a".into(),
        },
        ActivityEntry {
            timestamp: "10:01".into(),
            tool: "Edit".into(),
            label: "b".into(),
        },
    ];
    state.activity.scroll.total_lines = 6;
    state.activity.scroll.visible_height = 4;

    // Set up git scroll state
    state.git.unstaged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "file1.rs".into(),
            additions: 0,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "file2.rs".into(),
            additions: 0,
            deletions: 0,
            path: String::new(),
        },
    ];
    state.git.untracked_files = vec!["file3.rs".into()];
    state.scrolls.git.total_lines = 3;
    state.scrolls.git.visible_height = 1;

    // Activity tab: the block is a cursor list, so scrolling moves the
    // selection (the entry `y` copies) instead of a bare viewport.
    state.bottom_tab = BottomTab::Activity;
    state.scroll_bottom(1);
    assert_eq!(state.activity.selected, 1);
    assert_eq!(state.scrolls.git.offset, 0);

    // Git tab: scroll should affect git
    state.bottom_tab = BottomTab::GitStatus;
    state.scroll_bottom(1);
    assert_eq!(state.scrolls.git.offset, 1);
    assert_eq!(state.activity.selected, 1); // unchanged
}

#[test]
fn snapshot_git_status_tab_ui() {
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
    state.git.branch = "feature/sidebar".into();
    state.git.ahead_behind = Some((2, 1));
    state.git.unstaged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "src/ui/panes.rs".into(),
            additions: 30,
            deletions: 10,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "src/state.rs".into(),
            additions: 12,
            deletions: 5,
            path: String::new(),
        },
    ];
    state.git.untracked_files = vec!["new_file.rs".into()];
    state.git.diff_stat = Some((42, 15));

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │feature/sidebar       ↑2↓1│
    │+42/-15            3 files│
    │──────────────────────────│
    │Unstaged (2)              │
    │M src/ui/panes.rs  +30/-10│
    │M src/state.rs      +12/-5│
    │Untracked (1)             │
    │? new_file.rs             │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_clean_ui() {
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
    // No git changes

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_tab_active_ui() {
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │┃10:32 Edit src/main.rs   │
    ╰──────────────────────────╯
    ");
}

#[test]
fn activity_tab_leaves_one_blank_row_above_entries() {
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "src/main.rs".into(),
    }];

    // The inline snapshot locks in the blank-row spacer: after the `╭ Activity │ Git ╮`
    // title row, the first row must be empty and the timestamp/tool row must appear
    // one row further down.
    insta::assert_snapshot!(render_to_string(&mut state, 28, 24), @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │┃10:32 Edit src/main.rs   │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_long_tool_keeps_one_space_gap() {
    // A long tool name whose width plus the timestamp would fill or
    // overflow the inner width used to collide with the timestamp because
    // the right-align pad saturated to zero. The row must still carry at
    // least one space between them. `PushNotification` is a real name long
    // enough to truncate; MCP names are shortened first, so they no longer
    // exercise this path.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "PushNotification".into(),
        label: "rust".into(),
    }];

    let output = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │┃10:32 PushNotific… rust  │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_mcp_tool_shows_only_its_tool_segment() {
    // An MCP name is `mcp__<server>__<tool>`. The server segment is what makes
    // it long, so the row keeps only the tool part: `mcp__context7__query-docs`
    // reads as `query-docs` instead of `mcp__contex…`.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "mcp__context7__query-docs".into(),
        label: "rust".into(),
    }];

    let output = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │┃10:32 query-docs rust    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_focused_block_unwraps_the_command() {
    // Focused, the block spends rows on the label instead of clipping it to the
    // one row the unfocused block keeps: the timestamp/tool row stays, and the
    // command wraps across up to three rows below it.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Bash".into(),
        label: r#"rg -n "setup guide" src/main.rs | head -20 && cargo test --all-targets"#.into(),
    }];

    let output = render_to_string(&mut state, 34, 14);
    insta::assert_snapshot!(output, @r#"
       1   1   0   0   0     — ▾
    ╭ Activity │ Git ────────────────╮
    │┃10:32 rg -n "setup guide" src/m│
    │┃ain.rs | head -20 && cargo test│
    │┃--all-targets                  │
    ╰────────────────────────────────╯
    "#);
}

#[test]
fn snapshot_activity_cursor_row_wraps_without_the_keyboard() {
    // The keyboard is in the agent list, and the cursor row still wraps: what
    // the cursor marks is what is being read, and reading the log does not wait
    // for the block to be focused.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::Panes;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Bash".into(),
        label: r#"rg -n "setup guide" src/main.rs | head -20 && cargo test --all-targets"#.into(),
    }];

    let output = render_to_string(&mut state, 34, 14);
    insta::assert_snapshot!(output, @r#"
       1   1   0   0   0     — ▾
    ╭ Activity │ Git ────────────────╮
    │┃10:32 rg -n "setup guide" src/m│
    │┃ain.rs | head -20 && cargo test│
    │┃--all-targets                  │
    ╰────────────────────────────────╯
    "#);
}

#[test]
fn snapshot_activity_only_the_cursor_entry_wraps() {
    // The block spends rows on the entry under the cursor and nothing else: a
    // long command is readable in full while the surrounding log stays one row
    // per entry.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![
        ActivityEntry {
            timestamp: "10:35".into(),
            tool: "Bash".into(),
            label: "cargo test".into(),
        },
        ActivityEntry {
            timestamp: "10:34".into(),
            tool: "Bash".into(),
            label: r#"rg -n "quota" src/ui/bottom/activity.rs | head -20 && cargo clippy --all-targets"#.into(),
        },
        ActivityEntry {
            timestamp: "10:33".into(),
            tool: "Edit".into(),
            label: "activity.rs".into(),
        },
        ActivityEntry {
            timestamp: "10:32".into(),
            tool: "Read".into(),
            label: "main.rs".into(),
        },
    ];
    state.activity.move_selection(1);

    let output = render_to_string(&mut state, 34, 16);
    insta::assert_snapshot!(output, @r#"
       1   1   0   0   0     — ▾
    ╭ Activity │ Git ────────────────╮
    │ 10:35 cargo test               │
    │┃10:34 rg -n "quota" src/ui/bott│
    │┃om/activity.rs | head -20 && ca│
    │┃rgo clippy --all-targets       │
    │ 10:33 Edit activity.rs         │
    │ 10:32 Read main.rs             │
    ╰────────────────────────────────╯
    "#);
}

#[test]
fn snapshot_activity_cursor_marks_the_selected_entry() {
    // The cursor is the entry `y` copies, so it has to be visible: `┃` sits on
    // the selected entry's rows and every other row keeps the gutter blank.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::Panes;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = (0..4)
        .map(|i| ActivityEntry {
            timestamp: format!("10:3{i}"),
            tool: "Bash".into(),
            label: format!("cargo test --package crate{i}"),
        })
        .collect();
    state.activity.move_selection(2);

    let output = render_to_string(&mut state, 34, 14);
    insta::assert_snapshot!(output, @"
       1   1   0   0   0     — ▾
    ╭ Activity │ Git ────────────────╮
    │ 10:30 cargo test --package cra…│
    │ 10:31 cargo test --package cra…│
    │┃10:32 cargo test --package crat│
    │┃e2                             │
    │ 10:33 cargo test --package cra…│
    ╰────────────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_cursor_scrolls_itself_into_view() {
    // A long log whose cursor sits far down: the block scrolls to the cursor
    // instead of showing the top of the log, so what is highlighted is always
    // on screen.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = (0..20)
        .map(|i| ActivityEntry {
            timestamp: format!("10:{i:02}"),
            tool: "Bash".into(),
            label: format!("step {i}"),
        })
        .collect();
    state.activity.move_selection(15);

    let output = render_to_string(&mut state, 30, 12);
    insta::assert_snapshot!(output, @"
       1   1   0   0   0 — ▾
    ╭ Activity │ Git ────────────╮
    │ 10:08 step 8               │
    │ 10:09 step 9               │
    │ 10:10 step 10              │
    │ 10:11 step 11              │
    │ 10:12 step 12              │
    │ 10:13 step 13              │
    │ 10:14 step 14              │
    │┃10:15 step 15              │
    ╰────────────────────────────╯
    ");
}

#[test]
fn snapshot_activity_tall_cursor_entry_shows_its_head() {
    // The cursor entry is taller than the block it lives in: the block aligns
    // its head, so the start of the command is what you read. Aligning the tail
    // (the literal "scroll it into view" rule) would hide the beginning of the
    // entry the cursor is on.
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

    state.bottom_tab = BottomTab::Activity;
    state.focus_state.focus = Focus::ActivityLog;
    state.focus_state.sidebar_focused = true;
    state.activity.entries = vec![
        ActivityEntry {
            timestamp: "10:40".into(),
            tool: "Bash".into(),
            label: r#"git -C /Users/ghot/dotfiles status --short; rg -n "alpha" a.rs; rg -n "beta" b.rs; rg -n "gamma" c.rs; rg -n "delta" d.rs; rg -n "epsilon" e.rs"#.into(),
        },
        ActivityEntry {
            timestamp: "10:39".into(),
            tool: "Bash".into(),
            label: "cargo test".into(),
        },
    ];
    // The cursor sits on the tall entry (the newest one).
    state.activity.select_first();

    // A block of four content rows against a six-row entry: the entry cannot
    // fit, so the head/tail choice is what this snapshot pins.
    let output = render_to_string(&mut state, 30, 8);
    insta::assert_snapshot!(output, @r#"
       1   1   0   0   0 — ▾
    ╭ Activity │ Git ────────────╮
    │┃10:40 git -C /Users/ghot/do│
    │┃tfiles status --short; rg -│
    │┃n "alpha" a.rs; rg -n "beta│
    │┃" b.rs; rg -n "gamma" c.rs;│
    ╰────────────────────────────╯
    "#);
}

#[test]
fn snapshot_tab_bar_renders_both_labels() {
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

    state.activity.entries = vec![ActivityEntry {
        timestamp: "10:32".into(),
        tool: "Edit".into(),
        label: "test".into(),
    }];

    let output = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(output, @"
       1   0   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │┃10:32 Edit test          │
    ╰──────────────────────────╯
    ");
}

// ─── Git Content Tests ──────────────────────────────────────────────

#[test]
fn snapshot_git_full_info_ui() {
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
    state.git.ahead_behind = Some((0, 0));
    state.git.diff_stat = Some((120, 30));
    state.git.unstaged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "src/state.rs".into(),
            additions: 42,
            deletions: 10,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "src/ui/bottom.rs".into(),
            additions: 85,
            deletions: 20,
            path: String::new(),
        },
    ];
    state.git.untracked_files = vec!["new_file.rs".into()];

    // Use plain render since elapsed time varies
    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+120/-30           3 files│
    │──────────────────────────│
    │Unstaged (2)              │
    │M src/state.rs     +42/-10│
    │M src/ui/bottom.rs +85/-20│
    │Untracked (1)             │
    │? new_file.rs             │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_diff_summary_tight_ui() {
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
    state.git.diff_stat = Some((10, 3));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+10/-3             0 files│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_staged_file_diff_right_ui() {
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
    state.git.diff_stat = Some((10, 2));
    state.git.staged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "app.rs".into(),
        additions: 10,
        deletions: 2,
        path: String::new(),
    }];

    let plain = render_to_string(&mut state, 28, 18);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+10/-2             1 files│
    │──────────────────────────│
    │Staged (1)                │
    │M app.rs            +10/-2│
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_unstaged_long_name_diff_right_ui() {
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
    state.git.diff_stat = Some((150, 50));
    state.git.unstaged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "very-long-filename-that-should-be-truncated.rs".into(),
        additions: 150,
        deletions: 50,
        path: String::new(),
    }];

    let plain = render_to_string(&mut state, 28, 18);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+150/-50           1 files│
    │──────────────────────────│
    │Unstaged (1)              │
    │M very-long-file… +150/-50│
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_long_filename_truncated_ui() {
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
    state.git.unstaged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "very-long-filename-that-should-be-truncated.rs".into(),
            additions: 150,
            deletions: 50,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "short.rs".into(),
            additions: 8,
            deletions: 2,
            path: String::new(),
        },
    ];

    // Verify the long filename is truncated (contains ellipsis)
    let plain = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │                   2 files│
    │──────────────────────────│
    │Unstaged (2)              │
    │M very-long-file… +150/-50│
    │M short.rs           +8/-2│
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_more_than_5_files() {
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
    state.git.unstaged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "a.rs".into(),
            additions: 100,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "b.rs".into(),
            additions: 80,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "c.rs".into(),
            additions: 60,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "d.rs".into(),
            additions: 40,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "e.rs".into(),
            additions: 20,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "f.rs".into(),
            additions: 10,
            deletions: 0,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "g.rs".into(),
            additions: 5,
            deletions: 0,
            path: String::new(),
        },
    ];

    // Verify file list rendering (scroll to see overflow)
    let plain = render_to_string(&mut state, 28, 40);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │                   7 files│
    │──────────────────────────│
    │Unstaged (7)              │
    │M a.rs             +100/-0│
    │M b.rs              +80/-0│
    │M c.rs              +60/-0│
    │M d.rs              +40/-0│
    │M e.rs              +20/-0│
    │M f.rs              +10/-0│
    │M g.rs               +5/-0│
    ╰──────────────────────────╯
    ");

    // Setting `offset = 5` when the viewport can show all 8 content
    // rows (no overflow) is clamped back to 0 by `ScrollState::scroll(0)`
    // in `draw_git_content`, so the rendered view still includes the
    // whole file list. The clamp guards against stale over-scroll state
    // when the file list shrinks between frames.
    state.scrolls.git.offset = 5;
    let scrolled = render_to_string(&mut state, 28, 40);
    insta::assert_snapshot!(scrolled, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │                   7 files│
    │──────────────────────────│
    │Unstaged (7)              │
    │M a.rs             +100/-0│
    │M b.rs              +80/-0│
    │M c.rs              +60/-0│
    │M d.rs              +40/-0│
    │M e.rs              +20/-0│
    │M f.rs              +10/-0│
    │M g.rs               +5/-0│
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_branch_only_no_changes() {
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
    state.git.branch = "feature/long-branch-name".into();
    state.git.ahead_behind = Some((5, 0));

    let plain = render_to_string(&mut state, 38, 20);
    insta::assert_snapshot!(plain, @"
       1   1   0   0   0   0    — ▾
    ╭ Activity │ Git ────────────────────╮
    │feature/long-branch-name          ↑5│
    │         Working tree clean         │
    ╰────────────────────────────────────╯
    ");
}

#[test]
fn snapshot_git_pr_number_ui() {
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
    state.git.branch = "feature/fix".into();
    state.git.pr_number = Some("42".into());
    state.git.remote_url = "https://github.com/user/repo".into();
    state.git.diff_stat = Some((10, 3));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │feature/fix            #42│
    │+10/-3             0 files│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
    // Styled snapshot locks in the PR link's underline + pr_link color (fg:117)
    // so future style regressions surface as a diff rather than a missed grep.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 14), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bg:#504945,bold] [fg:#d3869b,bg:#504945,bold]1[fg:#d3869b,bg:#504945,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]

    ╭[fg:#fabd2f] [fg:#fabd2f]A[fg:#928374]c[fg:#928374]t[fg:#928374]i[fg:#928374]v[fg:#928374]i[fg:#928374]t[fg:#928374]y[fg:#928374] [fg:#504945]│[fg:#504945] [fg:#504945]G[fg:#fabd2f]i[fg:#fabd2f]t[fg:#fabd2f] [fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╮[fg:#fabd2f]
    │[fg:#fabd2f]f[fg:#ebdbb2]e[fg:#ebdbb2]a[fg:#ebdbb2]t[fg:#ebdbb2]u[fg:#ebdbb2]r[fg:#ebdbb2]e[fg:#ebdbb2]/[fg:#ebdbb2]f[fg:#ebdbb2]i[fg:#ebdbb2]x[fg:#ebdbb2] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]#[fg:#7daea3,underline]4[fg:#7daea3,underline]2[fg:#7daea3,underline]│[fg:#fabd2f]
    │[fg:#fabd2f]+[fg:#a9b665]1[fg:#a9b665]0[fg:#a9b665]/[fg:#928374]-[fg:#ea6962]3[fg:#ea6962] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]0[fg:#928374] [fg:#928374]f[fg:#928374]i[fg:#928374]l[fg:#928374]e[fg:#928374]s[fg:#928374]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]W[fg:#928374]o[fg:#928374]r[fg:#928374]k[fg:#928374]i[fg:#928374]n[fg:#928374]g[fg:#928374] [fg:#928374]t[fg:#928374]r[fg:#928374]e[fg:#928374]e[fg:#928374] [fg:#928374]c[fg:#928374]l[fg:#928374]e[fg:#928374]a[fg:#928374]n[fg:#928374] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    │[fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f] [fg:#fabd2f]│[fg:#fabd2f]
    ╰[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]─[fg:#fabd2f]╯[fg:#fabd2f]
    ");
}

#[test]
fn test_normalize_git_url() {
    // Test via state: set remote URL and check it's normalized
    let mut state = make_state(vec![]);
    state.git.remote_url = "https://github.com/user/repo".into();
    assert_eq!(state.git.remote_url, "https://github.com/user/repo");
}

#[test]
fn snapshot_git_pr_with_diff_ui() {
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
    state.git.pr_number = Some("123".into());
    state.git.remote_url = "https://github.com/user/repo".into();
    state.git.diff_stat = Some((55, 20));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                  #123│
    │+55/-20            0 files│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_subagents_tree_ui() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.subagents = vec!["Explore #1".into(), "Plan".into(), "Explore #2".into()];

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

    let output = render_to_string(&mut state, 40, 28);
    insta::assert_snapshot!(output, @"
       1   1   0   0   0   0      — ▾
    project
    ┃  claude
    ┃   ├ Explore #1
    ┃   ├ Plan #2
    ┃   └ Explore #2
    ╭ Activity │ Git ──────────────────────╮
    │            No activity yet           │
    ╰──────────────────────────────────────╯
    ");
}

#[test]
fn snapshot_subagent_long_name_truncated_ui() {
    let mut pane = make_pane(AgentType::Claude, PaneStatus::Running);
    pane.subagents = vec![
        "superpowers:code-reviewer".into(),
        "claude-code-guide".into(),
    ];

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

    // Narrow width (28) to force truncation of long subagent names
    let output = render_to_string(&mut state, 28, 27);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ┃   ├ superpowers:code-revi…
    ┃   └ claude-code-guide #2
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");

    assert_right_border_intact(&output);
}

// ─── Empty State Centered Tests ─────────────────────────────────────

#[test]
fn snapshot_activity_empty_centered_ui() {
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
    state.bottom_tab = BottomTab::Activity;
    // No activity entries — should show centered "No activity yet"

    let output = render_to_string(&mut state, 28, 26);
    insta::assert_snapshot!(output, @"
       1   0   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_clean_centered_ui() {
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
    state.bottom_tab = BottomTab::GitStatus;
    // No git info — should show centered "Working tree clean"

    let output = render_to_string(&mut state, 28, 26);
    insta::assert_snapshot!(output, @"
       1   0   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

// ─── Git: "Working tree clean" consistency ──────────────────────────

#[test]
fn snapshot_git_branch_loaded_no_changes_shows_inline_clean() {
    // Bug fix: when git_branch is set but no status/diff/commit data,
    // the early-return "centered clean" path was skipped, falling through
    // to a different "inline clean" layout. Now both paths are consistent.
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
    // Branch loaded, but no changes/commits — should still show "Working tree clean"
    state.git.branch = "main".into();

    let plain = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_no_data_shows_centered_clean() {
    // When no git data is loaded at all, should show centered "Working tree clean"
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
    // No git data at all

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

// ─── Git: ahead/behind rendering ────────────────────────────────────

#[test]
fn test_git_behind_only() {
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
    state.git.ahead_behind = Some((0, 3));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                    ↓3│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn test_git_ahead_and_behind() {
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
    state.git.ahead_behind = Some((2, 3));

    let plain = render_to_string(&mut state, 38, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0   0   0    — ▾
    ╭ Activity │ Git ────────────────────╮
    │main                            ↑2↓3│
    │         Working tree clean         │
    ╰────────────────────────────────────╯
    ");
}

// ─── Git: diff stat with only insertions or only deletions ──────────

#[test]
fn test_git_diff_insertions_only() {
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
    state.git.diff_stat = Some((25, 0));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+25/-0             0 files│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn test_git_diff_deletions_only() {
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
    state.git.diff_stat = Some((0, 15));

    let plain = render_to_string(&mut state, 28, 14);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+0/-15             0 files│
    │    Working tree clean    │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_branch_truncated_ui() {
    let pane = make_pane(AgentType::Claude, PaneStatus::Running);
    let mut state = make_state(vec![SessionInfo {
        session_name: "main".into(),
        windows: vec![WindowInfo {
            window_id: "@0".into(),
            window_name: "project".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![pane.clone()],
        }],
    }]);
    // Use a repo group with a long branch name via PaneGitInfo
    state.repo_groups = vec![tmux_agent_sidebar::group::RepoGroup {
        name: "dotfiles".into(),
        has_focus: true,
        panes: vec![(
            pane,
            tmux_agent_sidebar::group::PaneGitInfo {
                repo_root: Some("/home/user/dotfiles".into()),
                branch: Some("feature/tmux-sidebar-dashboard-refactor".into()),
                is_worktree: false,
                worktree_name: None,
            },
        )],
    }];
    state.rebuild_row_targets();

    let plain = render_to_string(&mut state, 28, 30);
    insta::assert_snapshot!(plain, @"
       1   1   0   0    — ▾
    dotfiles                   +
    ┃  claude     feature/tmux…
    ╭ Activity │ Git ──────────╮
    │      No activity yet     │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_staged_unstaged_untracked_ui() {
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
    state.git.pr_number = Some("5".into());
    state.git.diff_stat = Some((12, 3));
    state.git.staged_files = vec![
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: "app.rs".into(),
            additions: 10,
            deletions: 2,
            path: String::new(),
        },
        tmux_agent_sidebar::git::GitFileEntry {
            status: 'A',
            name: "new.rs".into(),
            additions: 2,
            deletions: 0,
            path: String::new(),
        },
    ];
    state.git.unstaged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "config.toml".into(),
        additions: 0,
        deletions: 1,
        path: String::new(),
    }];
    state.git.untracked_files = vec!["debug.log".into()];

    let output = render_to_string(&mut state, 28, 30);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                    #5│
    │+12/-3             4 files│
    │──────────────────────────│
    │Staged (2)                │
    │M app.rs            +10/-2│
    │A new.rs             +2/-0│
    │Unstaged (1)              │
    │M config.toml        +0/-1│
    │Untracked (1)             │
    │? debug.log               │
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_long_branch_with_pr_ui() {
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
    state.git.branch = "feature/very-long-branch-name".into();
    state.git.pr_number = Some("123".into());
    state.git.diff_stat = Some((5, 2));
    state.git.unstaged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'M',
        name: "main.rs".into(),
        additions: 5,
        deletions: 2,
        path: String::new(),
    }];

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │feature/very-long-br… #123│
    │+5/-2              1 files│
    │──────────────────────────│
    │Unstaged (1)              │
    │M main.rs            +5/-2│
    ╰──────────────────────────╯
    ");
    assert_right_border_intact(&output);
}

#[test]
fn snapshot_git_staged_only_ui() {
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
    state.git.diff_stat = Some((20, 0));
    state.git.staged_files = vec![tmux_agent_sidebar::git::GitFileEntry {
        status: 'A',
        name: "new_feature.rs".into(),
        additions: 20,
        deletions: 0,
        path: String::new(),
    }];

    let output = render_to_string(&mut state, 28, 24);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │main                      │
    │+20/-0             1 files│
    │──────────────────────────│
    │Staged (1)                │
    │A new_feature.rs    +20/-0│
    ╰──────────────────────────╯
    ");
}

#[test]
fn snapshot_git_many_files_more_indicator_ui() {
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
    state.git.branch = "dev".into();
    state.git.unstaged_files = (0..7)
        .map(|i| tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: format!("f{i}.rs"),
            additions: 1,
            deletions: 0,
            path: String::new(),
        })
        .collect();

    let output = render_to_string(&mut state, 28, 30);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │dev                       │
    │                   7 files│
    │──────────────────────────│
    │Unstaged (7)              │
    │M f0.rs              +1/-0│
    │M f1.rs              +1/-0│
    │M f2.rs              +1/-0│
    │M f3.rs              +1/-0│
    │M f4.rs              +1/-0│
    │M f5.rs              +1/-0│
    │M f6.rs              +1/-0│
    ╰──────────────────────────╯
    ");
    assert_right_border_intact(&output);
}

#[test]
fn snapshot_git_more_than_10_files_ui() {
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
    state.git.branch = "dev".into();
    state.git.unstaged_files = (0..12)
        .map(|i| tmux_agent_sidebar::git::GitFileEntry {
            status: 'M',
            name: format!("f{i}.rs"),
            additions: 1,
            deletions: 0,
            path: String::new(),
        })
        .collect();

    let output = render_to_string(&mut state, 28, 30);
    insta::assert_snapshot!(output, @"
       1   1   0   0    — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────╮
    │dev                       │
    │                  12 files│
    │──────────────────────────│
    │Unstaged (12)      +2 more│
    │M f0.rs              +1/-0│
    │M f1.rs              +1/-0│
    │M f2.rs              +1/-0│
    │M f3.rs              +1/-0│
    │M f4.rs              +1/-0│
    │M f5.rs              +1/-0│
    │M f6.rs              +1/-0│
    │M f7.rs              +1/-0│
    │M f8.rs              +1/-0│
    │M f9.rs              +1/-0│
    ╰──────────────────────────╯
    ");
    assert_right_border_intact(&output);
}

#[test]
fn snapshot_focused_group_active_border_styled() {
    // Two repo groups: focused pane in first, second should have inactive border
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
            name: "dotfiles".into(),
            has_focus: true,
            panes: vec![(
                pane1.clone(),
                tmux_agent_sidebar::group::PaneGitInfo::default(),
            )],
        },
        tmux_agent_sidebar::group::RepoGroup {
            name: "my-app".into(),
            has_focus: false,
            panes: vec![(
                pane2.clone(),
                tmux_agent_sidebar::group::PaneGitInfo::default(),
            )],
        },
    ];
    state.focus_state.focused_pane_id = Some("%1".into());
    state.rebuild_row_targets();

    // Styled snapshot locks in the focused group's accent color (fg:153) on
    // the active pane marker and the active bottom-panel border.
    insta::assert_snapshot!(render_to_styled_string(&mut state, 28, 30), @"
    [fg:#fb4934,bold]  [fg:#d3869b,bg:#504945,bold] [fg:#d3869b,bg:#504945,bold]2[fg:#d3869b,bg:#504945,bold]  [fg:#7c6f64] [fg:#7c6f64]1[fg:#ebdbb2]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]  [fg:#7c6f64] [fg:#7c6f64]0[fg:#7c6f64]    —[fg:#928374] ▾[fg:#928374]
    d[fg:#fabd2f]o[fg:#fabd2f]t[fg:#fabd2f]f[fg:#fabd2f]i[fg:#fabd2f]l[fg:#fabd2f]e[fg:#fabd2f]s[fg:#fabd2f]
    ┃[fg:#fabd2f,bg:#504945] [bg:#504945][fg:#b8bb26,bg:#504945] [fg:#e78a4e,bg:#504945]c[fg:#e78a4e,bg:#504945]l[fg:#e78a4e,bg:#504945]a[fg:#e78a4e,bg:#504945]u[fg:#e78a4e,bg:#504945]d[fg:#e78a4e,bg:#504945]e[fg:#e78a4e,bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945] [bg:#504945]
    m[fg:#bdae93]y[fg:#bdae93]-[fg:#bdae93]a[fg:#bdae93]p[fg:#bdae93]p[fg:#bdae93]
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

#[test]
fn test_pet_enabled_preserves_bottom_panel_border() {
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
    state.pet_enabled = true;

    insta::assert_snapshot!(render_to_string(&mut state, 40, 30), @"
       1   0   0   0   1   0      — ▾
    project
    ┃  claude
    ╭ Activity │ Git ──────────────────────╮
    │            No activity yet           │
    ╰──────────────────────────────────────╯
    ");
}
