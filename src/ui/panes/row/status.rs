use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::ctx::RowCtx;
use crate::tmux::PaneStatus;
use crate::ui::icons::StatusIcons;
use crate::ui::text::{display_width, elapsed_label, truncate_to_width};

pub(super) fn status_row(
    pane: &crate::tmux::PaneInfo,
    git_info: &crate::group::PaneGitInfo,
    ports: Option<&[u16]>,
    ctx: &RowCtx,
    icons: &StatusIcons,
    now: u64,
) -> Line<'static> {
    use crate::tmux::PermissionMode;
    let theme = ctx.theme;

    let icon = icons.pane_icon(&pane.status, &pane.wait_reason);
    let icon_color = theme.status_color(&pane.status, pane.attention);
    let title_raw: &str = if pane.session_name.is_empty() {
        pane.agent.label()
    } else {
        &pane.session_name
    };
    let badge = pane.permission_mode.badge();
    let elapsed = elapsed_label(pane.started_at, now);
    let branch = crate::ui::text::branch_label(git_info);
    let ports = ports
        .filter(|ports| !ports.is_empty())
        .map(|ports| {
            format!(
                ":{}",
                ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .unwrap_or_default();
    let remove = pane.sidebar_spawned && branch.starts_with("+ ");

    let title_fg = theme.agent_color(&pane.agent);
    let elapsed_fg = if pane.status.is_active() {
        theme.text_active
    } else {
        theme.text_muted
    };

    let badge_extra = if badge.is_empty() { 0 } else { 1 };
    let fixed_width = display_width(icon) + 1 + badge_extra + display_width(badge);
    let remove_width = usize::from(remove) * 2;

    // Keep the title dominant while allowing compact repository context on the
    // same line. Metadata may use at most half the row; elapsed time is added
    // only after branch and ports because it carries the lowest priority.
    let right_budget = ctx
        .inner_width
        .saturating_sub(fixed_width + remove_width + 4)
        .min(ctx.inner_width / 2);
    let mut context = String::new();
    if !branch.is_empty() || !ports.is_empty() {
        if ports.is_empty() {
            context = truncate_to_width(&branch, right_budget);
        } else if branch.is_empty() {
            context = truncate_to_width(&ports, right_budget);
        } else {
            let ports_width = display_width(&ports);
            if ports_width < right_budget {
                let branch_budget = right_budget.saturating_sub(ports_width + 1);
                let shown_branch = truncate_to_width(&branch, branch_budget);
                context = format!("{shown_branch} {ports}");
            } else {
                context = truncate_to_width(&ports, right_budget);
            }
        }
    }
    let context_width = display_width(&context);
    let elapsed_width = display_width(&elapsed);
    let show_elapsed = elapsed_width > 0
        && context_width + usize::from(context_width > 0) + elapsed_width <= right_budget;
    let right_content_width = context_width
        + usize::from(show_elapsed && context_width > 0)
        + usize::from(show_elapsed) * elapsed_width;
    // User-supplied session names (set via `/rename`) can be arbitrarily
    // long; cap the title to the space left after reserving room for the
    // icon, badge, and elapsed label so they stay visible instead of
    // being pushed off-screen.
    let title_budget = ctx.inner_width.saturating_sub(
        fixed_width + remove_width + right_content_width + usize::from(right_content_width > 0),
    );
    let title = truncate_to_width(title_raw, title_budget);

    let left_width = fixed_width + display_width(&title);

    let mut left_spans: Vec<Span<'static>> = Vec::with_capacity(3);
    let icon_style = if pane.status == PaneStatus::Error
        || pane.wait_reason == crate::tmux::WAIT_REASON_RESPONSE_READY
        || (pane.status == PaneStatus::Waiting
            && crate::tmux::is_actionable_wait_reason(&pane.wait_reason)
            && (!pane.wait_reason.is_empty() || pane.attention))
    {
        Style::default().fg(icon_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(icon_color)
    };
    left_spans.push(Span::styled(icon.to_string(), ctx.apply_bg(icon_style)));
    left_spans.push(Span::styled(
        format!(" {}", title),
        ctx.apply_bg(Style::default().fg(title_fg)),
    ));
    if !badge.is_empty() {
        let badge_color = match pane.permission_mode {
            PermissionMode::BypassPermissions => theme.badge_danger,
            PermissionMode::Auto => theme.badge_auto,
            PermissionMode::DontAsk => theme.badge_auto,
            PermissionMode::Plan => theme.badge_plan,
            PermissionMode::AcceptEdits => theme.badge_auto,
            PermissionMode::Defer => theme.badge_auto,
            PermissionMode::Default => theme.text_muted,
        };
        left_spans.push(Span::styled(
            format!(" {}", badge),
            ctx.apply_bg(Style::default().fg(badge_color)),
        ));
    }

    let mut right_spans = Vec::with_capacity(6);
    let mut right_width = right_content_width + remove_width;
    if right_content_width > 0 {
        right_spans.push(Span::styled(" ", ctx.apply_bg(Style::default())));
        right_width += 1;
    }
    if !context.is_empty() {
        right_spans.push(Span::styled(
            context,
            ctx.apply_bg(Style::default().fg(theme.branch)),
        ));
    }
    if show_elapsed {
        if context_width > 0 {
            right_spans.push(Span::styled(" ", ctx.apply_bg(Style::default())));
        }
        right_spans.push(Span::styled(
            elapsed,
            ctx.apply_bg(Style::default().fg(elapsed_fg)),
        ));
    }
    if remove {
        right_spans.push(Span::styled(" ", ctx.apply_bg(Style::default())));
        right_spans.push(Span::styled(
            "×".to_string(),
            ctx.apply_bg(Style::default().fg(theme.status_error)),
        ));
    }

    ctx.row_line_split(left_spans, left_width, right_spans, right_width)
}
