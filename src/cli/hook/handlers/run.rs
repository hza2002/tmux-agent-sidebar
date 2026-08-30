use crate::cli::{sanitize_tmux_value, set_attention, set_status};
use crate::desktop_notification;
use crate::desktop_notification::DesktopNotificationKind;
use crate::tmux;

use crate::time::now_epoch_secs;

use super::super::context::{
    AgentContext, clear_run_state, is_system_message, mark_task_reset, set_agent_meta,
};
use super::super::notifications::{
    NotifyLabels, NotifyPayload, notification_run_id, notify_lifecycle, set_notification_run_id,
    stop_body, stop_failure_body, stop_failure_fingerprint, task_completed_body,
    task_completed_fingerprint,
};

const LEGACY_COMPLETION_ID: &str = "legacy";

pub(in crate::cli::hook) fn on_user_prompt_submit(
    pane: &str,
    ctx: &AgentContext<'_>,
    prompt: &str,
    turn_id: Option<&str>,
) -> i32 {
    set_agent_meta(pane, ctx);
    set_attention(pane, "clear");
    set_status(pane, "running");
    set_notification_run_id(pane);
    tmux::unset_pane_option(pane, tmux::PANE_COMPLETED_TURN_ID);
    if let Some(turn_id) = turn_id {
        tmux::set_pane_option(pane, tmux::PANE_TURN_ID, turn_id);
    } else {
        tmux::unset_pane_option(pane, tmux::PANE_TURN_ID);
    }
    if !prompt.is_empty() && !is_system_message(prompt) {
        let p = sanitize_tmux_value(prompt);
        tmux::set_pane_option(pane, tmux::PANE_PROMPT, &p);
        tmux::set_pane_option(pane, tmux::PANE_PROMPT_SOURCE, "user");
    }
    tmux::set_pane_option(pane, tmux::PANE_STARTED_AT, &now_epoch_secs().to_string());
    tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
    0
}

pub(in crate::cli::hook) fn on_stop(
    pane: &str,
    ctx: &AgentContext<'_>,
    last_message: &str,
    response: Option<&str>,
    turn_id: Option<&str>,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    let current_session = tmux::get_pane_option_value(pane, tmux::PANE_SESSION_ID);
    let session_mismatch = ctx.session_id.as_deref().is_some_and(|id| {
        if current_session.is_empty() {
            tmux::get_pane_option_value(pane, tmux::PANE_AGENT).is_empty()
        } else {
            id != current_session
        }
    });
    let current_turn = tmux::get_pane_option_value(pane, tmux::PANE_TURN_ID);
    let completed_turn = tmux::get_pane_option_value(pane, tmux::PANE_COMPLETED_TURN_ID);
    let already_completed = !completed_turn.is_empty();
    let invalid_turn = match turn_id {
        Some(id) => current_turn != id,
        None => !current_turn.is_empty(),
    };
    if session_mismatch || already_completed || invalid_turn {
        if let Some(resp) = response {
            println!("{resp}");
        }
        return 0;
    }
    set_agent_meta(pane, ctx);
    set_attention(pane, "notification");
    if !last_message.is_empty() {
        let msg = sanitize_tmux_value(last_message);
        tmux::set_pane_option(pane, tmux::PANE_PROMPT, &msg);
        tmux::set_pane_option(pane, tmux::PANE_PROMPT_SOURCE, "response");
    }
    // `Stop` is emitted for the parent turn, and Claude Code `Task` subagents
    // are synchronous: once the parent reaches Stop, no child should still be
    // running. Treat any leftover list as stale state from a missed or
    // mismatched SubagentStop and clear it before `mark_task_reset`, whose
    // guard intentionally skips writes while subagents are active.
    tmux::unset_pane_option(pane, tmux::PANE_SUBAGENTS);
    let bg_shell_live = !tmux::get_pane_option_value(pane, tmux::PANE_BG_CMD).is_empty();
    tmux::unset_pane_option(pane, tmux::PANE_STARTED_AT);
    tmux::set_pane_option(
        pane,
        tmux::PANE_WAIT_REASON,
        tmux::WAIT_REASON_RESPONSE_READY,
    );
    mark_task_reset(pane);
    set_status(pane, "waiting");
    tmux::set_pane_option(
        pane,
        tmux::PANE_COMPLETED_TURN_ID,
        turn_id.unwrap_or(LEGACY_COMPLETION_ID),
    );

    if !bg_shell_live {
        let run_id = notification_run_id(pane);
        // Skip the generic Stop notification if an explicit TaskCompleted
        // stamp from the current run has already fired — otherwise Claude
        // Code's `TaskCompleted` → `Stop` sequence produces two desktop
        // notifications for the same logical completion.
        let already_notified = desktop_notification::has_run_scoped_stamp(
            pane,
            DesktopNotificationKind::TaskCompleted,
            run_id,
        );
        if !already_notified {
            let _ = notify_lifecycle(
                pane,
                NotifyLabels::FromCtx(ctx),
                notifications,
                run_id,
                NotifyPayload {
                    kind: DesktopNotificationKind::TaskCompleted,
                    event: desktop_notification::DesktopNotificationEvent::Stop,
                    fingerprint_suffix: "stop",
                    body: &stop_body(last_message),
                },
            );
        }
    }
    if let Some(resp) = response {
        println!("{resp}");
    }
    0
}

pub(in crate::cli::hook) fn on_stop_failure(
    pane: &str,
    ctx: &AgentContext<'_>,
    error: &str,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    set_agent_meta(pane, ctx);
    set_attention(pane, "clear");
    clear_run_state(pane);
    mark_task_reset(pane);
    if !error.is_empty() {
        tmux::set_pane_option(pane, tmux::PANE_WAIT_REASON, error);
    }
    set_status(pane, "error");
    let _ = notify_lifecycle(
        pane,
        NotifyLabels::FromCtx(ctx),
        notifications,
        None,
        NotifyPayload {
            kind: DesktopNotificationKind::TaskFailed,
            event: desktop_notification::DesktopNotificationEvent::StopFailure,
            fingerprint_suffix: stop_failure_fingerprint(error),
            body: &stop_failure_body(error),
        },
    );
    0
}

pub(in crate::cli::hook) fn on_task_completed(
    pane: &str,
    agent_name: &str,
    task_id: &str,
    task_subject: &str,
    notifications: &desktop_notification::DesktopNotificationSettings,
) -> i32 {
    let _ = notify_lifecycle(
        pane,
        NotifyLabels::FromPane { agent: agent_name },
        notifications,
        None,
        NotifyPayload {
            kind: DesktopNotificationKind::TaskCompleted,
            event: desktop_notification::DesktopNotificationEvent::TaskCompleted,
            fingerprint_suffix: task_completed_fingerprint(task_id, task_subject),
            body: &task_completed_body(task_subject),
        },
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_user_prompt_submit_sets_running_and_stores_prompt() {
        let _guard = tmux::test_mock::install();
        let pane = "%PROMPT";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let exit = on_user_prompt_submit(pane, &ctx, "fix the bug", None);
        assert_eq!(exit, 0);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("fix the bug")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT_SOURCE).as_deref(),
            Some("user")
        );
        assert!(tmux::test_mock::contains(pane, tmux::PANE_STARTED_AT));
    }

    #[test]
    fn on_user_prompt_submit_ignores_system_messages() {
        let _guard = tmux::test_mock::install();
        let pane = "%SYS_PROMPT";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        on_user_prompt_submit(
            pane,
            &ctx,
            "<system-reminder>ignore me</system-reminder>",
            None,
        );
        assert!(
            !tmux::test_mock::contains(pane, tmux::PANE_PROMPT),
            "system messages should not be stored as user prompt"
        );
        // But status should still advance to running.
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
    }

    #[test]
    fn on_user_prompt_submit_clears_stale_wait_reason_but_preserves_bg_cmd() {
        let _guard = tmux::test_mock::install();
        let pane = "%PROMPT_CLEAR_WAIT";
        tmux::test_mock::set(pane, tmux::PANE_WAIT_REASON, "permission");
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "npm run dev");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        on_user_prompt_submit(pane, &ctx, "new prompt", None);
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_WAIT_REASON));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_BG_CMD).as_deref(),
            Some("npm run dev"),
            "bg command must survive a new user turn — the shell is still running",
        );
    }

    #[test]
    fn on_stop_with_background_shell_sets_ready_status() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOP_BG";
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "npm run dev");
        tmux::test_mock::set(pane, tmux::PANE_STARTED_AT, "123");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };

        let exit = on_stop(
            pane,
            &ctx,
            "",
            None,
            None,
            &desktop_notification::DesktopNotificationSettings {
                enabled: false,
                events: Default::default(),
            },
        );

        assert_eq!(exit, 0);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STARTED_AT));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some(tmux::WAIT_REASON_RESPONSE_READY)
        );
    }

    #[test]
    fn on_stop_without_background_shell_sets_ready_status() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOP_IDLE";
        tmux::test_mock::set(pane, tmux::PANE_STARTED_AT, "123");
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };

        on_stop(
            pane,
            &ctx,
            "",
            None,
            None,
            &desktop_notification::DesktopNotificationSettings {
                enabled: false,
                events: Default::default(),
            },
        );

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STARTED_AT));
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_ATTENTION).as_deref(),
            Some("notification")
        );
    }

    #[test]
    fn on_stop_clears_stale_subagents() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOP_STALE_SUBAGENTS";
        tmux::test_mock::set(
            pane,
            tmux::PANE_SUBAGENTS,
            "general-purpose:sub-1,general-purpose:sub-2",
        );
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };

        on_stop(
            pane,
            &ctx,
            "",
            None,
            None,
            &desktop_notification::DesktopNotificationSettings {
                enabled: false,
                events: Default::default(),
            },
        );

        assert!(
            !tmux::test_mock::contains(pane, tmux::PANE_SUBAGENTS),
            "parent Stop must clear stale subagent list"
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("waiting")
        );
    }

    #[test]
    fn delayed_codex_stop_cannot_overwrite_newer_turn() {
        let _guard = tmux::test_mock::install();
        let pane = "%CODEX_TURN_ORDER";
        let session = Some("session-a".to_string());
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };

        on_user_prompt_submit(pane, &ctx, "turn A", Some("turn-a"));
        on_stop(pane, &ctx, "answer A", None, Some("turn-a"), &notifications);
        on_user_prompt_submit(pane, &ctx, "turn B", Some("turn-b"));
        on_stop(pane, &ctx, "late A", None, Some("turn-a"), &notifications);

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("turn B")
        );
        assert!(!tmux::test_mock::contains(
            pane,
            tmux::PANE_COMPLETED_TURN_ID
        ));
    }

    #[test]
    fn duplicate_stop_for_same_turn_is_idempotent() {
        let _guard = tmux::test_mock::install();
        let pane = "%CODEX_DUP_STOP";
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };

        on_user_prompt_submit(pane, &ctx, "prompt", Some("turn-a"));
        on_stop(pane, &ctx, "first", None, Some("turn-a"), &notifications);
        on_stop(
            pane,
            &ctx,
            "duplicate",
            None,
            Some("turn-a"),
            &notifications,
        );

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("first")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_COMPLETED_TURN_ID).as_deref(),
            Some("turn-a")
        );
    }

    #[test]
    fn duplicate_legacy_stop_does_not_reopen_parked_response() {
        let _guard = tmux::test_mock::install();
        let pane = "%LEGACY_DUP_STOP";
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

        on_user_prompt_submit(pane, &ctx, "prompt", None);
        on_stop(pane, &ctx, "first", None, None, &notifications);
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "idle");
        tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
        on_stop(pane, &ctx, "duplicate", None, None, &notifications);

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("idle")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("first")
        );
    }

    #[test]
    fn prompt_without_turn_id_invalidates_previous_turn_id() {
        let _guard = tmux::test_mock::install();
        let pane = "%MIXED_TURN_IDS";
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };

        on_user_prompt_submit(pane, &ctx, "turn A", Some("turn-a"));
        on_user_prompt_submit(pane, &ctx, "turn B", None);
        on_stop(pane, &ctx, "late A", None, Some("turn-a"), &notifications);

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("turn B")
        );
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_TURN_ID));
    }

    #[test]
    fn stop_without_turn_id_cannot_overwrite_identified_turn() {
        let _guard = tmux::test_mock::install();
        let pane = "%MISSING_STOP_TURN_ID";
        let ctx = AgentContext {
            agent: "codex",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };

        on_user_prompt_submit(pane, &ctx, "current turn", Some("turn-b"));
        on_stop(pane, &ctx, "stale response", None, None, &notifications);

        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("running")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_PROMPT).as_deref(),
            Some("current turn")
        );
    }

    #[test]
    fn stop_cannot_revive_session_after_teardown() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOP_AFTER_SESSION_END";
        let session = Some("session-a".to_string());
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &session,
        };
        let notifications = desktop_notification::DesktopNotificationSettings {
            enabled: false,
            events: Default::default(),
        };

        on_user_prompt_submit(pane, &ctx, "prompt", None);
        for key in [
            tmux::PANE_AGENT,
            tmux::PANE_SESSION_ID,
            tmux::PANE_STATUS,
            tmux::PANE_TURN_ID,
            tmux::PANE_COMPLETED_TURN_ID,
        ] {
            tmux::unset_pane_option(pane, key);
        }
        on_stop(pane, &ctx, "late response", None, None, &notifications);

        assert!(!tmux::test_mock::contains(pane, tmux::PANE_AGENT));
        assert!(!tmux::test_mock::contains(pane, tmux::PANE_STATUS));
    }

    #[test]
    fn on_stop_failure_records_error_wait_reason_and_error_status() {
        let _guard = tmux::test_mock::install();
        let pane = "%STOP_FAIL";
        let ctx = AgentContext {
            agent: "claude",
            cwd: "/repo",
            permission_mode: "default",
            worktree: &None,
            session_id: &None,
        };
        let exit = on_stop_failure(
            pane,
            &ctx,
            "rate_limit",
            &desktop_notification::DesktopNotificationSettings {
                enabled: false,
                events: Default::default(),
            },
        );
        assert_eq!(exit, 0);
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_STATUS).as_deref(),
            Some("error")
        );
        assert_eq!(
            tmux::test_mock::get(pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("rate_limit")
        );
    }
}
