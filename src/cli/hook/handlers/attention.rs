use crate::cli::{set_attention, set_status};
use crate::desktop_notification;
use crate::desktop_notification::DesktopNotificationKind;
use crate::tmux;

use super::super::context::{
    AgentContext, lifecycle_event_allowed, pane_writes_allowed, set_agent_meta,
};
use super::super::notifications::{
    NotifyLabels, NotifyPayload, notification_body, notification_fingerprint, notify_lifecycle,
};
use super::status_priority::resolve_notification_status;
use crate::tmux::is_actionable_wait_reason;

pub(in crate::cli::hook) fn on_notification(
    pane: &str,
    ctx: &AgentContext<'_>,
    wait_reason: &str,
    meta_only: bool,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    if !lifecycle_event_allowed(pane, ctx.session_id.as_deref(), None) {
        return 0;
    }
    set_agent_meta(pane, ctx);
    if meta_only {
        return 0;
    }
    let bg_shell_live = !tmux::get_pane_option_value(pane, tmux::PANE_BG_CMD).is_empty();
    let actionable = !wait_reason.is_empty() && is_actionable_wait_reason(wait_reason);
    if !actionable {
        // Informational notifications must not create a false waiting state.
        // Clear a stale reason/attention marker while preserving the current
        // running/background status until a real lifecycle event changes it.
        tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
        set_attention(pane, "clear");
        return 0;
    }
    // Invalidate any response-review transition before writing the rest of
    // the notification state.
    if wait_reason.is_empty() {
        tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
    } else {
        tmux::set_pane_option(pane, tmux::PANE_WAIT_REASON, wait_reason);
    }
    set_status(
        pane,
        resolve_notification_status(wait_reason, bg_shell_live),
    );
    // Claude emits informational notifications (auth success, resume, rate
    // limits, and agent metadata) through this same hook. They must not look
    // like a prompt or produce an AppleScript alert.
    set_attention(pane, if actionable { "notification" } else { "clear" });
    if actionable {
        let _ = notify_lifecycle(
            pane,
            NotifyLabels::FromCtx(ctx),
            notifications,
            None,
            NotifyPayload {
                kind: DesktopNotificationKind::PermissionRequired,
                event: desktop_notification::DesktopNotificationEvent::Notification,
                fingerprint_suffix: notification_fingerprint(wait_reason),
                body: &notification_body(wait_reason),
            },
        );
    }
    0
}

pub(in crate::cli::hook) fn on_permission_denied(
    pane: &str,
    ctx: &AgentContext<'_>,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    if !lifecycle_event_allowed(pane, ctx.session_id.as_deref(), None) {
        return 0;
    }
    set_agent_meta(pane, ctx);
    tmux::set_pane_option(pane, tmux::PANE_WAIT_REASON, "permission_denied");
    set_status(pane, "waiting");
    set_attention(pane, "notification");
    let _ = notify_lifecycle(
        pane,
        NotifyLabels::FromCtx(ctx),
        notifications,
        None,
        NotifyPayload {
            kind: DesktopNotificationKind::PermissionRequired,
            event: desktop_notification::DesktopNotificationEvent::PermissionDenied,
            fingerprint_suffix: "permission_denied",
            body: "权限被拒绝",
        },
    );
    0
}

/// Mark a Codex permission prompt as requiring user attention. Permission
/// hooks run in separate processes and can arrive late, so reject events from
/// another session or a turn already finalized by Stop.
pub(in crate::cli::hook) fn on_permission_request(
    pane: &str,
    ctx: &AgentContext<'_>,
    turn_id: Option<&str>,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    let current_session = tmux::get_pane_option_value(pane, tmux::PANE_SESSION_ID);
    let session_mismatch = ctx
        .session_id
        .as_deref()
        .is_some_and(|id| !current_session.is_empty() && id != current_session);
    let current_turn = tmux::get_pane_option_value(pane, tmux::PANE_TURN_ID);
    let turn_mismatch = match turn_id {
        Some(id) => !current_turn.is_empty() && id != current_turn,
        None => !current_turn.is_empty(),
    };
    let completed_turn = tmux::get_pane_option_value(pane, tmux::PANE_COMPLETED_TURN_ID);
    let turn_completed =
        turn_id.is_some_and(|id| !completed_turn.is_empty() && id == completed_turn);
    if session_mismatch || turn_mismatch || turn_completed {
        return 0;
    }
    on_notification(pane, ctx, "permission", false, notifications)
}

/// Kimi-only counterpart to `on_permission_request`: the user answered the
/// permission prompt and the turn resumed, so drop the waiting state
/// immediately instead of holding it until the next lifecycle event. Only a
/// pane still waiting on a permission prompt is touched — a racing Stop or
/// prompt submit that already moved the pane elsewhere wins.
pub(in crate::cli::hook) fn on_permission_result(
    pane: &str,
    ctx: &AgentContext<'_>,
    turn_id: Option<&str>,
) -> i32 {
    if !lifecycle_event_allowed(pane, ctx.session_id.as_deref(), turn_id) {
        return 0;
    }
    set_agent_meta(pane, ctx);
    let waiting_on_permission = tmux::get_pane_option_value(pane, tmux::PANE_STATUS) == "waiting"
        && tmux::get_pane_option_value(pane, tmux::PANE_WAIT_REASON) == "permission";
    if waiting_on_permission {
        tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
        set_attention(pane, "clear");
        set_status(pane, "running");
    }
    0
}

pub(in crate::cli::hook) fn on_teammate_idle(
    pane: &str,
    teammate_name: &str,
    idle_reason: &str,
) -> i32 {
    if !pane_writes_allowed(pane) {
        return 0;
    }
    let reason = if idle_reason.is_empty() {
        format!("teammate_idle:{teammate_name}")
    } else {
        format!("teammate_idle:{teammate_name}:{idle_reason}")
    };
    tmux::set_pane_option(pane, tmux::PANE_WAIT_REASON, &reason);
    set_attention(pane, "notification");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_teammate_idle_sets_attention_and_reason() {
        let _guard = tmux::test_mock::install();
        let pane = "%TEAM";
        let exit = on_teammate_idle(pane, "alice", "");
        assert_eq!(exit, 0);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_ATTENTION).as_deref(),
            Some("notification")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("teammate_idle:alice")
        );
    }

    #[test]
    fn on_teammate_idle_includes_idle_reason_when_present() {
        let _guard = tmux::test_mock::install();
        let pane = "%TEAM_REASON";
        on_teammate_idle(pane, "alice", "tokens_exhausted");
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("teammate_idle:alice:tokens_exhausted")
        );
    }

    #[test]
    fn on_notification_meta_only_skips_status_and_attention() {
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_META";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "permission",
            /* meta_only */ true,
            &notifications,
        );
        // meta_only=true must short-circuit before status/attention/wait_reason writes.
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_ATTENTION));
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_WAIT_REASON));
        // Agent meta should still be applied so the sidebar can render the pane.
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_AGENT).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn permission_request_ignores_completed_turn() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERM_LATE";
        tmux::test_mock::set(pane, tmux::PANE_SESSION_ID, "sess");
        tmux::test_mock::set(pane, tmux::PANE_COMPLETED_TURN_ID, "turn-1");
        let session_id = Some("sess".to_string());
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session_id,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_permission_request(pane, &ctx, Some("turn-1"), &notifications);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
    }

    #[test]
    fn permission_request_ignores_other_session() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERM_SESSION";
        tmux::test_mock::set(pane, tmux::PANE_SESSION_ID, "current");
        let session_id = Some("stale".to_string());
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session_id,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_permission_request(pane, &ctx, Some("turn-2"), &notifications);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
    }

    #[test]
    fn permission_request_sets_waiting_status() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERM_ACTIVE";
        tmux::test_mock::set(pane, tmux::PANE_SESSION_ID, "sess");
        tmux::test_mock::set(pane, tmux::PANE_TURN_ID, "turn-2");
        let session_id = Some("sess".to_string());
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session_id,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_permission_request(pane, &ctx, Some("turn-2"), &notifications);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("permission")
        );
    }

    #[test]
    fn permission_request_ignores_other_turn() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERM_TURN";
        tmux::test_mock::set(pane, tmux::PANE_TURN_ID, "current");
        let session_id = Some("sess".to_string());
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session_id,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_permission_request(pane, &ctx, Some("stale"), &notifications);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
    }

    #[test]
    fn on_notification_sets_waiting_status_and_reason() {
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_WAIT";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "permission",
            /* meta_only */ false,
            &notifications,
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_ATTENTION).as_deref(),
            Some("notification")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("permission")
        );
    }

    #[test]
    fn on_notification_keeps_background_when_softer_reason_and_bg_shell_live() {
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_BG_PREEMPT";
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "cargo test");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "auth_success",
            /* meta_only */ false,
            &notifications,
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_ATTENTION).as_deref(),
            Some("")
        );
    }

    #[test]
    fn on_notification_permission_reason_preempts_background() {
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_PERM_OVER_BG";
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "cargo test");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "permission_prompt",
            /* meta_only */ false,
            &notifications,
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting"),
        );
    }

    #[test]
    fn on_notification_plain_permission_preempts_background() {
        // Claude's real `notification_type: "permission"` payload must
        // stay in `waiting` even with a live bg shell — the user has to
        // act on the prompt regardless.
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_PERM_PLAIN_OVER_BG";
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "cargo test");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "permission",
            /* meta_only */ false,
            &notifications,
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting"),
        );
    }

    #[test]
    fn on_notification_soft_reason_without_bg_preserves_status() {
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_SOFT_NO_BG";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(
            pane,
            &ctx,
            "auth_success",
            /* meta_only */ false,
            &notifications,
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
    }

    #[test]
    fn on_notification_empty_wait_reason_clears_stale_value() {
        // Regression: an empty wait_reason used to be a no-op, which
        // left the previously-written reason on the pane. A later
        // notification that genuinely has no reason must drop the
        // stale one so the sidebar does not keep rendering the wrong
        // cause.
        let _guard = tmux::test_mock::install();
        let pane = "%NOTIF_STALE";
        tmux::test_mock::set(pane, tmux::PANE_WAIT_REASON, "permission");

        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(pane, &ctx, "", /* meta_only */ false, &notifications);

        assert!(
            !tmux::test_mock::contains(pane, tmux::PANE_WAIT_REASON),
            "empty wait_reason must clear a prior value"
        );
    }

    #[test]
    fn on_permission_denied_records_permission_denied_wait_reason() {
        let _guard = tmux::test_mock::install();
        let pane = "%PD";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_permission_denied(pane, &ctx, &notifications);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("permission_denied")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
    }

    #[test]
    fn permission_result_resumes_pane_waiting_on_permission() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMRES";
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "waiting");
        tmux::test_mock::set(pane, tmux::PANE_WAIT_REASON, "permission");
        let ctx = AgentContext {
            agent: "kimi",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        on_permission_result(pane, &ctx, None);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_WAIT_REASON));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_ATTENTION).as_deref(),
            Some("")
        );
    }

    #[test]
    fn permission_result_leaves_response_ready_waiting_untouched() {
        // A late PermissionResult racing a Stop must not reopen a turn that
        // already finished as response-ready.
        let _guard = tmux::test_mock::install();
        let pane = "%PERMRES_RACE";
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "waiting");
        tmux::test_mock::set(
            pane,
            tmux::PANE_WAIT_REASON,
            tmux::WAIT_REASON_RESPONSE_READY,
        );
        let ctx = AgentContext {
            agent: "kimi",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        on_permission_result(pane, &ctx, None);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some(tmux::WAIT_REASON_RESPONSE_READY)
        );
    }

    #[test]
    fn permission_result_ignores_completed_turn() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMRES_LATE";
        tmux::test_mock::set(pane, tmux::PANE_COMPLETED_TURN_ID, "legacy");
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "waiting");
        tmux::test_mock::set(pane, tmux::PANE_WAIT_REASON, "permission");
        let ctx = AgentContext {
            agent: "kimi",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        on_permission_result(pane, &ctx, None);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
    }

    #[test]
    fn attention_events_are_ignored_while_subagent_owns_pane() {
        let _guard = tmux::test_mock::install();
        let pane = "%ATTENTION_CHILD";
        tmux::test_mock::set(pane, tmux::PANE_SUBAGENTS, "Explore:child");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };
        on_notification(pane, &ctx, "permission", false, &notifications);
        on_permission_denied(pane, &ctx, &notifications);
        on_teammate_idle(pane, "child", "");
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_ATTENTION));
    }
}
