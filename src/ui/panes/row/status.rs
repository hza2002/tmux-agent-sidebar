use ratatui::{
    style::{Color, Modifier, Style},
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

    let icon = icons.pane_icon_at(&pane.status, &pane.wait_reason, now);
    let base_icon_color = theme.status_color(&pane.status, pane.attention);
    let icon_color = status_signal_color(
        &pane.status,
        &pane.wait_reason,
        base_icon_color,
        theme.status_error,
        theme.text_active,
        now,
    );
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

fn status_signal_color(
    status: &PaneStatus,
    wait_reason: &str,
    base: Color,
    error: Color,
    peak: Color,
    now: u64,
) -> Color {
    if now == 0 || wait_reason == crate::tmux::WAIT_REASON_RESPONSE_REVIEWING {
        return base;
    }

    if *status == PaneStatus::Error {
        return if now.is_multiple_of(2) {
            dim_color(error, 65)
        } else {
            error
        };
    }

    if wait_reason == crate::tmux::WAIT_REASON_RESPONSE_READY {
        return if (now / 2).is_multiple_of(2) {
            base
        } else {
            blend_color(base, peak, 35)
        };
    }

    if *status == PaneStatus::Waiting
        && crate::tmux::is_actionable_wait_reason(wait_reason)
        && now % 4 == 2
    {
        return blend_color(base, peak, 42);
    }

    base
}

fn blend_color(base: Color, peak: Color, peak_weight: u16) -> Color {
    let (base, peak) = match (base, peak) {
        (Color::Rgb(br, bg, bb), Color::Rgb(pr, pg, pb)) => ((br, bg, bb), (pr, pg, pb)),
        _ => return base,
    };

    let blend = |a: u8, b: u8, peak_weight: u16| {
        (((a as u16 * (100 - peak_weight)) + (b as u16 * peak_weight)) / 100) as u8
    };
    Color::Rgb(
        blend(base.0, peak.0, peak_weight),
        blend(base.1, peak.1, peak_weight),
        blend(base.2, peak.2, peak_weight),
    )
}

fn dim_color(color: Color, percent: u16) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as u16 * percent / 100) as u8,
            (g as u16 * percent / 100) as u8,
            (b as u16 * percent / 100) as u8,
        ),
        _ => color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_stays_at_its_base_color() {
        let base = Color::Rgb(100, 150, 200);
        assert_eq!(
            status_signal_color(
                &PaneStatus::Running,
                "",
                base,
                Color::Rgb(255, 0, 0),
                Color::Rgb(200, 200, 200),
                2,
            ),
            base
        );
    }

    #[test]
    fn review_ready_uses_a_slow_four_second_breath() {
        let base = Color::Rgb(100, 150, 200);
        let peak = Color::Rgb(200, 200, 200);
        let expected_peak = Color::Rgb(135, 167, 200);

        for now in [4, 5] {
            assert_eq!(
                status_signal_color(
                    &PaneStatus::Waiting,
                    crate::tmux::WAIT_REASON_RESPONSE_READY,
                    base,
                    Color::Rgb(255, 0, 0),
                    peak,
                    now,
                ),
                base
            );
        }
        for now in [6, 7] {
            assert_eq!(
                status_signal_color(
                    &PaneStatus::Waiting,
                    crate::tmux::WAIT_REASON_RESPONSE_READY,
                    base,
                    Color::Rgb(255, 0, 0),
                    peak,
                    now,
                ),
                expected_peak
            );
        }
    }

    #[test]
    fn actionable_waiting_has_one_bright_pulse_per_four_seconds() {
        let base = Color::Rgb(100, 150, 200);
        let peak = Color::Rgb(200, 200, 200);
        for now in [4, 5, 7] {
            assert_eq!(
                status_signal_color(
                    &PaneStatus::Waiting,
                    "permission_prompt",
                    base,
                    Color::Rgb(255, 0, 0),
                    peak,
                    now,
                ),
                base
            );
        }
        assert_eq!(
            status_signal_color(
                &PaneStatus::Waiting,
                "permission_prompt",
                base,
                Color::Rgb(255, 0, 0),
                peak,
                6,
            ),
            Color::Rgb(142, 171, 200)
        );
    }

    #[test]
    fn error_alternates_dark_and_bright_red_each_second() {
        let error = Color::Rgb(200, 100, 50);
        assert_eq!(
            status_signal_color(
                &PaneStatus::Error,
                "",
                Color::Rgb(1, 2, 3),
                error,
                Color::Rgb(255, 255, 255),
                4,
            ),
            Color::Rgb(130, 65, 32)
        );
        assert_eq!(
            status_signal_color(
                &PaneStatus::Error,
                "",
                Color::Rgb(1, 2, 3),
                error,
                Color::Rgb(255, 255, 255),
                5,
            ),
            error
        );
    }

    #[test]
    fn reviewing_and_non_rgb_colors_remain_static() {
        let base = Color::Rgb(100, 150, 200);
        assert_eq!(
            status_signal_color(
                &PaneStatus::Waiting,
                crate::tmux::WAIT_REASON_RESPONSE_REVIEWING,
                base,
                Color::Rgb(255, 0, 0),
                Color::Rgb(200, 200, 200),
                6,
            ),
            base
        );
        assert_eq!(
            status_signal_color(
                &PaneStatus::Waiting,
                crate::tmux::WAIT_REASON_RESPONSE_READY,
                Color::Indexed(10),
                Color::Indexed(1),
                Color::Indexed(15),
                6,
            ),
            Color::Indexed(10)
        );
    }
}
