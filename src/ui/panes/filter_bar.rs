use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::state::{AppState, RepoFilter, StatusFilter};
use crate::tmux::PaneStatus;

use crate::ui::text::{display_width, truncate_to_width};
use crate::ui::{FILTER_GROUP_GAP, FILTER_ICON_COUNT_GAP};

/// Render the status filter bar.
pub(super) fn render_filter_bar<'a>(state: &AppState) -> Line<'a> {
    let theme = &state.theme;
    let icons = &state.icons;
    let (all, running, background, waiting, idle, error) = state.status_counts();

    let icon_for = |s: PaneStatus| (icons.status_icon(&s), theme.status_color(&s, false));
    let items: Vec<(StatusFilter, (&str, ratatui::style::Color), usize)> = vec![
        (StatusFilter::All, (icons.all_icon(), theme.status_all), all),
        (
            StatusFilter::Running,
            icon_for(PaneStatus::Running),
            running,
        ),
        (
            StatusFilter::Background,
            icon_for(PaneStatus::Background),
            background,
        ),
        (
            StatusFilter::Waiting,
            icon_for(PaneStatus::Waiting),
            waiting,
        ),
        (StatusFilter::Idle, icon_for(PaneStatus::Idle), idle),
        (StatusFilter::Error, icon_for(PaneStatus::Error), error),
    ];

    let mut spans: Vec<Span<'a>> = Vec::new();

    for (i, (filter, (icon, icon_color), count)) in items.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(FILTER_GROUP_GAP)));
        }

        let is_selected = state.global.status_filter == filter;
        let icon_style = if is_selected {
            Style::default().fg(icon_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.filter_inactive)
        };
        spans.push(Span::styled(
            format!("{icon}{}", " ".repeat(FILTER_ICON_COUNT_GAP)),
            icon_style,
        ));

        let count_str = format!("{count}");
        let count_style = if is_selected {
            Style::default().fg(icon_color).add_modifier(Modifier::BOLD)
        } else if count == 0 {
            Style::default().fg(theme.filter_inactive)
        } else {
            Style::default().fg(theme.text_active)
        };
        spans.push(Span::styled(count_str, count_style));
    }

    Line::from(spans)
}

fn clipped_filter_spans<'a>(line: Line<'a>, max_width: usize) -> (Vec<Span<'a>>, usize) {
    let source = line.spans;
    let mut spans = Vec::new();
    let mut used = 0;
    let mut index = 0;
    while index + 1 < source.len() {
        let separator_width = usize::from(index > 0) * FILTER_GROUP_GAP;
        let item_width = display_width(source[index].content.as_ref())
            + display_width(source[index + 1].content.as_ref());
        if used + separator_width + item_width > max_width {
            break;
        }
        if index > 0 {
            spans.push(source[index - 1].clone());
            used += separator_width;
        }
        spans.push(source[index].clone());
        spans.push(source[index + 1].clone());
        used += item_width;
        index += 3;
    }
    (spans, used)
}

/// Render the single fixed header row: notices, status filters, and repo filter.
pub(super) fn render_header<'a>(
    state: &AppState,
    width: u16,
) -> (Line<'a>, Option<u16>, Option<u16>) {
    let theme = &state.theme;
    let repo_icon = "▾";

    let repo_has_filter = !matches!(state.global.repo_filter, RepoFilter::All);
    let repo_style = if state.is_repo_popup_open() || repo_has_filter {
        Style::default().fg(theme.text_active)
    } else {
        Style::default().fg(theme.text_muted)
    };

    let has_notices_info = crate::ui::notices::has_info(state);
    let notices_button_col = has_notices_info.then_some(0);
    let notices_width = crate::ui::notices::BUTTON_WIDTH;
    let status_line = render_filter_bar(state);

    // Preserve the repo control at the right edge on narrow sidebars.
    // Three columns are the notice slot, one separates status from repo,
    // and two are reserved for the repo label's trailing space + arrow.
    let max_status_width = (width as usize).saturating_sub(notices_width + 4);
    let (status_spans, shown_status_width) = clipped_filter_spans(status_line, max_status_width);
    let max_repo_label_width = (width as usize)
        .saturating_sub(notices_width + shown_status_width + 3)
        .max(1);
    let repo_label = match &state.global.repo_filter {
        RepoFilter::All => "—".to_string(),
        RepoFilter::Repo(id) => {
            let label = state
                .repo_groups
                .iter()
                .find(|group| group.id() == id)
                .map(|group| group.name.as_str())
                .unwrap_or(id);
            truncate_to_width(label, max_repo_label_width)
        }
    };
    let repo_btn_width = display_width(&repo_label) + 2; // label + space + arrow

    let gap = (width as usize).saturating_sub(notices_width + shown_status_width + repo_btn_width);
    let repo_button_col =
        (repo_btn_width <= width as usize).then_some(width.saturating_sub(repo_btn_width as u16));

    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(crate::ui::notices::button_span(state));
    spans.push(Span::raw(" ".repeat(notices_width.saturating_sub(1))));
    spans.extend(status_spans);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(repo_label, repo_style));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(repo_icon, repo_style));

    (Line::from(spans), notices_button_col, repo_button_col)
}

#[cfg(test)]
use crate::group::PaneGitInfo;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn snapshot_fixed_header_omits_version_banner_when_notice_present() {
        // Version notices change the status indicator in the header but
        // must not leak the "new release vX.Y.Z" banner into the row —
        // the banner lives in the popup, not the header. A snapshot here
        // catches any regression that would put banner text back on the
        // row, including subtle width or spacing drift.
        let mut state = crate::state::AppState::new(String::new());
        state.version_notice = Some(crate::version::UpdateNotice {
            local_version: "0.2.6".into(),
            latest_version: "0.2.7".into(),
        });

        let text = line_text(&render_header(&state, 30).0);
        insta::assert_snapshot!(text, @"   0   0   0   0   0 — ▾");
    }

    #[test]
    fn render_header_keeps_repo_position_with_or_without_notices_info() {
        let mut with_info = AppState::new(String::new());
        with_info.version_notice = Some(crate::version::UpdateNotice {
            local_version: "0.2.6".into(),
            latest_version: "0.2.7".into(),
        });
        with_info.notices.missing_hook_groups = vec![crate::state::NoticesMissingHookGroup {
            agent: "claude".into(),
            hooks: vec!["SessionStart".into()],
        }];

        let without_info = AppState::new(String::new());

        let (_, _, with_repo_col) = render_header(&with_info, 30);
        let (_, _, without_repo_col) = render_header(&without_info, 30);

        assert_eq!(with_repo_col, without_repo_col);
        assert_eq!(with_repo_col, Some(27));
    }

    #[test]
    fn snapshot_fixed_header_without_notices_info() {
        let state = AppState::new(String::new());
        let text = line_text(&render_header(&state, 30).0);
        insta::assert_snapshot!(text, @"   0   0   0   0   0 — ▾");
    }

    #[test]
    fn snapshot_fixed_header_with_version_only() {
        let mut state = AppState::new(String::new());
        state.version_notice = Some(crate::version::UpdateNotice {
            local_version: "0.2.6".into(),
            latest_version: "0.2.7".into(),
        });
        let text = line_text(&render_header(&state, 30).0);
        insta::assert_snapshot!(text, @"   0   0   0   0   0 — ▾");
    }

    #[test]
    fn snapshot_fixed_header_with_hooks_only() {
        let mut state = AppState::new(String::new());
        state.notices.missing_hook_groups = vec![crate::state::NoticesMissingHookGroup {
            agent: "claude".into(),
            hooks: vec!["SessionStart".into()],
        }];
        let text = line_text(&render_header(&state, 30).0);
        insta::assert_snapshot!(text, @"   0   0   0   0   0 — ▾");
    }

    #[test]
    fn snapshot_fixed_header_with_version_and_hooks() {
        let mut state = AppState::new(String::new());
        state.version_notice = Some(crate::version::UpdateNotice {
            local_version: "0.2.6".into(),
            latest_version: "0.2.7".into(),
        });
        state.notices.missing_hook_groups = vec![crate::state::NoticesMissingHookGroup {
            agent: "claude".into(),
            hooks: vec!["SessionStart".into()],
        }];
        let text = line_text(&render_header(&state, 30).0);
        insta::assert_snapshot!(text, @"   0   0   0   0   0 — ▾");
    }

    // ─── render_filter_bar tests ──────────────────────────────

    fn make_state_with_groups(groups: Vec<crate::group::RepoGroup>) -> AppState {
        let mut state = AppState::new("%99".into());
        state.repo_groups = groups;
        state.rebuild_row_targets();
        state
    }

    fn filter_bar_text(state: &AppState) -> String {
        let line = render_filter_bar(state);
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn render_filter_bar_is_status_only() {
        let state = make_state_with_groups(vec![]);
        let text = filter_bar_text(&state);
        assert!(
            !text.contains("▾"),
            "status filter bar should not contain repo button"
        );
    }

    #[test]
    fn render_filter_bar_uses_selected_and_inactive_icon_colors() {
        let pane1 = crate::tmux::PaneInfo {
            pane_id: "%2".into(),
            pane_active: true,
            status: PaneStatus::Running,
            attention: false,
            agent: crate::tmux::AgentType::Claude,
            path: String::new(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            status_changed_at: None,
            wait_reason: String::new(),
            permission_mode: crate::tmux::PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: crate::tmux::WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        };
        let pane2 = crate::tmux::PaneInfo {
            pane_id: "%3".into(),
            pane_active: false,
            status: PaneStatus::Idle,
            attention: false,
            agent: crate::tmux::AgentType::Codex,
            path: String::new(),
            current_command: String::new(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            status_changed_at: None,
            wait_reason: String::new(),
            permission_mode: crate::tmux::PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: crate::tmux::WorktreeMetadata::default(),
            session_id: None,
            session_name: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        };
        let mut state = make_state_with_groups(vec![crate::group::RepoGroup {
            name: "project".into(),
            has_focus: true,
            panes: vec![
                (pane1, PaneGitInfo::default()),
                (pane2, PaneGitInfo::default()),
            ],
        }]);
        state.global.status_filter = StatusFilter::Running;
        let theme = &state.theme;

        let line = render_filter_bar(&state);
        let cells: Vec<_> = line
            .spans
            .iter()
            .filter(|span| !span.content.as_ref().trim().is_empty())
            .collect();

        assert_eq!(cells.len(), 12);

        assert_eq!(cells[0].content.as_ref(), " ");
        assert_eq!(cells[0].style.fg, Some(theme.filter_inactive));
        assert!(!cells[0].style.add_modifier.contains(Modifier::DIM));

        assert_eq!(cells[1].content.as_ref(), "2");
        assert_eq!(cells[1].style.fg, Some(theme.text_active));

        assert_eq!(cells[2].content.as_ref(), " ");
        assert_eq!(cells[2].style.fg, Some(theme.status_running));
        assert!(cells[2].style.add_modifier.contains(Modifier::BOLD));

        assert_eq!(cells[3].content.as_ref(), "1");
        assert_eq!(cells[3].style.fg, Some(theme.status_running));
        assert!(cells[3].style.add_modifier.contains(Modifier::BOLD));

        assert_eq!(cells[4].content.as_ref(), " ");
        assert_eq!(cells[4].style.fg, Some(theme.filter_inactive));
        assert!(!cells[4].style.add_modifier.contains(Modifier::DIM));

        assert_eq!(cells[5].content.as_ref(), "0");
        assert_eq!(cells[5].style.fg, Some(theme.filter_inactive));

        assert_eq!(cells[6].content.as_ref(), " ");
        assert_eq!(cells[6].style.fg, Some(theme.filter_inactive));

        assert_eq!(cells[7].content.as_ref(), "0");
        assert_eq!(cells[7].style.fg, Some(theme.filter_inactive));

        assert_eq!(cells[8].content.as_ref(), " ");
        assert_eq!(cells[8].style.fg, Some(theme.filter_inactive));

        assert_eq!(cells[9].content.as_ref(), "1");
        assert_eq!(cells[9].style.fg, Some(theme.text_active));

        assert_eq!(cells[10].content.as_ref(), " ");
        assert_eq!(cells[10].style.fg, Some(theme.filter_inactive));

        assert_eq!(cells[11].content.as_ref(), "0");
        assert_eq!(cells[11].style.fg, Some(theme.filter_inactive));
    }

    #[test]
    fn render_header_repo_button_col_returned() {
        let state = make_state_with_groups(vec![]);
        let (_, _, col) = render_header(&state, 28);
        assert_eq!(col, Some(25), "repo button should be right-aligned");
    }

    #[test]
    fn snapshot_fixed_header_narrow_keeps_complete_status_items_and_repo_button() {
        let state = make_state_with_groups(vec![]);
        let text = line_text(&render_header(&state, 18).0);
        insta::assert_snapshot!(text, @"   0   0    — ▾");
    }

    #[test]
    fn snapshot_fixed_header_shows_notices_indicator_when_missing_hooks_exist() {
        // Visual regression check: the indicator MUST sit at column 0
        // and the repo filter MUST stay pinned to the right edge when
        // missing hooks are present. A snapshot catches any drift in
        // spacing, glyph, or column alignment that a `starts_with` /
        // `contains` probe would silently miss.
        let mut state = make_state_with_groups(vec![]);
        state.notices.missing_hook_groups = vec![crate::state::NoticesMissingHookGroup {
            agent: "claude".into(),
            hooks: vec!["SessionStart".into(), "Stop".into()],
        }];

        let (line, notices_col, repo_col) = render_header(&state, 28);
        let text = line_text(&line);
        insta::assert_snapshot!(text, @"   0   0   0   0    — ▾");
        // Click-target columns are layout state, not visible characters,
        // so they stay as direct equality checks alongside the snapshot.
        assert_eq!(notices_col, Some(0));
        assert_eq!(repo_col, Some(25));
    }

    #[test]
    fn render_header_shows_repo_name_when_filtered() {
        let mut state = make_state_with_groups(vec![crate::group::RepoGroup {
            name: "my-app".into(),
            has_focus: true,
            panes: vec![],
        }]);
        state.global.repo_filter = RepoFilter::Repo("my-app".into());
        let text = line_text(&render_header(&state, 40).0);
        assert!(
            text.contains("my-app"),
            "fixed header should show filtered repo name, got: {text}"
        );
        assert!(
            text.find("my-app").unwrap() < text.find("▾").unwrap(),
            "repo name should come before the arrow"
        );
        let (line, _, _) = render_header(&state, 40);
        let repo_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("my-app"))
            .unwrap();
        assert!(
            !repo_span.style.add_modifier.contains(Modifier::BOLD),
            "filtered repo label should not be bold"
        );
    }

    #[test]
    fn render_header_truncates_long_repo_name() {
        let mut state = make_state_with_groups(vec![crate::group::RepoGroup {
            name: "very-long-repository-name-that-exceeds-width".into(),
            has_focus: true,
            panes: vec![],
        }]);
        state.global.repo_filter =
            RepoFilter::Repo("very-long-repository-name-that-exceeds-width".into());
        let text = line_text(&render_header(&state, 28).0);
        assert!(
            text.contains('…'),
            "repo name should be truncated with an ellipsis"
        );
        assert!(text.contains("▾"));
        assert!(
            !text.contains("very-long-repository-name-that-exceeds-width"),
            "repo name should not fit in full at this width"
        );
        assert!(
            text.find('…').unwrap() < text.find("▾").unwrap(),
            "repo name should come before the arrow"
        );
    }

    #[test]
    fn render_header_popup_open_styling() {
        let mut state = make_state_with_groups(vec![]);
        state.popup = crate::state::PopupState::Repo {
            selected: 0,
            query: String::new(),
            area: None,
        };
        let (line, _, _) = render_header(&state, 28);
        let last_span = line.spans.last().unwrap();
        assert!(
            !last_span.style.add_modifier.contains(Modifier::UNDERLINED),
            "repo button should not be underlined when popup is open"
        );
        assert!(
            !last_span.style.add_modifier.contains(Modifier::BOLD),
            "repo button should not be bold when popup is open"
        );
    }
}
