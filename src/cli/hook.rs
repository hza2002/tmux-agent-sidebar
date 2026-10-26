use crate::event::{AgentEvent, resolve_adapter};

use super::{read_stdin_json, tmux_pane};

mod activity;
#[cfg(test)]
mod background_tests;
mod context;
mod handlers;
mod notifications;

use context::sync_pane_location;
use notifications::notification_settings;

use crate::pane_lock::PaneHookLock;

mod codex_routing;

// ─── hook subcommand ────────────────────────────────────────────────────────

pub(crate) fn cmd_hook(args: &[String]) -> i32 {
    let agent_name = args.first().map(|s| s.as_str()).unwrap_or("");
    let event_name = args.get(1).map(|s| s.as_str()).unwrap_or("");

    if agent_name.is_empty() || event_name.is_empty() {
        return 0;
    }

    let Some(adapter) = resolve_adapter(agent_name) else {
        return 0;
    };

    if agent_name == "codex" {
        let input = read_stdin_json();
        if let Some(event) = adapter.parse(event_name, &input) {
            return codex_routing::dispatch(event);
        }
        return 0;
    }

    let pane = tmux_pane();
    if pane.is_empty() {
        return 0;
    }

    let input = read_stdin_json();
    let Some(event) = adapter.parse(event_name, &input) else {
        return 0;
    };

    let Ok(_lock) = PaneHookLock::acquire(&pane, None) else {
        return 1;
    };
    handle_event(&pane, agent_name, event)
}

// ─── event handler ──────────────────────────────────────────────────────────

fn handle_event(pane: &str, agent_name: &str, event: AgentEvent) -> i32 {
    if agent_name == "claude" && !claude_parent_event_allowed(pane, &event) {
        return 0;
    }
    match event {
        AgentEvent::SessionTitle { session_id, title } => {
            handlers::on_session_title(pane, &session_id, &title)
        }
        AgentEvent::SessionStart {
            agent,
            cwd,
            permission_mode,
            source,
            worktree,
            session_id,
            ..
        } => handlers::on_session_start(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
            &source,
        ),
        AgentEvent::SessionEnd {
            end_reason,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            if agent_name == "claude" && context::bound_session_matches(pane, session_id.as_deref())
            {
                // Identity proves this is the parent. Permit the normal teardown
                // (including exceptional-exit notifications) despite live children.
                crate::tmux::unset_pane_option(pane, crate::tmux::PANE_SUBAGENTS);
            }
            handlers::on_session_end(pane, agent_name, &end_reason, &notifications)
        }
        AgentEvent::UserPromptSubmit {
            agent,
            cwd,
            permission_mode,
            prompt,
            worktree,
            session_id,
            turn_id,
            ..
        } => handlers::on_user_prompt_submit(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
            &prompt,
            turn_id.as_deref(),
        ),
        AgentEvent::Notification {
            agent,
            cwd,
            permission_mode,
            wait_reason,
            meta_only,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_notification(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &wait_reason,
                meta_only,
                &notifications,
            )
        }
        AgentEvent::Stop {
            agent,
            cwd,
            permission_mode,
            last_message,
            background_tasks,
            response,
            worktree,
            session_id,
            turn_id,
            ..
        } => {
            let notifications = notification_settings();
            let mut ctx = context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id);
            ctx.background_tasks = background_tasks.as_deref();
            handlers::on_stop(
                pane,
                &ctx,
                &last_message,
                response.as_deref(),
                turn_id.as_deref(),
                &notifications,
            )
        }
        AgentEvent::StopFailure {
            agent,
            cwd,
            permission_mode,
            error,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_stop_failure(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &error,
                &notifications,
            )
        }
        AgentEvent::SubagentStart {
            agent_type,
            agent_id,
            ..
        } => handlers::on_subagent_start(pane, &agent_type, agent_id.as_deref()),
        AgentEvent::SubagentStop {
            agent_id,
            background_tasks,
            ..
        } => {
            let Some(id) = agent_id.as_deref().filter(|id| !id.is_empty()) else {
                return 0;
            };
            handlers::background_permission::resume(pane, id);
            handlers::stopped_subagents::record(pane, id);
            handlers::background::reconcile(pane, background_tasks.as_deref(), Some(id));
            handlers::on_subagent_stop(pane, Some(id))
        }
        AgentEvent::ActivityLog {
            tool_name,
            tool_input,
            tool_response,
            session_id,
            turn_id,
            ..
        } => activity::handle_activity_log_with_context(
            pane,
            &tool_name,
            &tool_input,
            &tool_response,
            session_id.as_deref(),
            turn_id.as_deref(),
        ),
        AgentEvent::PermissionDenied {
            agent,
            cwd,
            permission_mode,
            worktree,
            session_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_permission_denied(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
                &notifications,
            )
        }
        AgentEvent::PermissionRequest {
            agent,
            cwd,
            permission_mode,
            session_id,
            turn_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_permission_request(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &None, &session_id),
                turn_id.as_deref(),
                &notifications,
            )
        }
        AgentEvent::PermissionResult {
            agent,
            cwd,
            permission_mode,
            session_id,
            turn_id,
            ..
        } => handlers::on_permission_result(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &None, &session_id),
            turn_id.as_deref(),
        ),
        AgentEvent::Interrupt {
            agent,
            cwd,
            permission_mode,
            worktree,
            session_id,
            turn_id,
            ..
        } => handlers::on_interrupt(
            pane,
            &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
            turn_id.as_deref(),
        ),
        AgentEvent::ToolFailure {
            tool_name,
            tool_input,
            error,
            session_id,
            turn_id,
        } => activity::handle_tool_failure(
            pane,
            &tool_name,
            &tool_input,
            &error,
            session_id.as_deref(),
            turn_id.as_deref(),
        ),
        AgentEvent::CwdChanged {
            cwd,
            worktree,
            session_id,
            ..
        } => {
            sync_pane_location(pane, &cwd, &worktree, &session_id);
            0
        }
        AgentEvent::TaskCreated { .. } => 0,
        AgentEvent::TaskCompleted {
            task_id,
            task_subject,
        } => {
            super::set_attention(pane, "notification");
            let notifications = notification_settings();
            handlers::on_task_completed(pane, agent_name, &task_id, &task_subject, &notifications)
        }
        AgentEvent::TeammateIdle {
            teammate_name,
            idle_reason,
            ..
        } => handlers::on_teammate_idle(pane, &teammate_name, &idle_reason),
        AgentEvent::WorktreeCreate => 0,
        AgentEvent::WorktreeRemove { .. } => handlers::on_worktree_remove(pane),
    }
}

/// Child lifecycle hooks share a pane with their parent. Identity, not the
/// existence of any child, decides which events may update parent workflow.
fn claude_parent_event_allowed(pane: &str, event: &AgentEvent) -> bool {
    let (agent_id, session_id) = match event {
        AgentEvent::SessionStart {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::SessionEnd {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::UserPromptSubmit {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::Notification {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::Stop {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::StopFailure {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::PermissionDenied {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::CwdChanged {
            agent_id,
            session_id,
            ..
        }
        | AgentEvent::ActivityLog {
            agent_id,
            session_id,
            ..
        } => (agent_id, session_id),
        AgentEvent::SubagentStart { session_id, .. }
        | AgentEvent::SubagentStop { session_id, .. } => {
            return session_id.as_deref().is_none_or(|id| {
                let bound = crate::tmux::get_pane_option_value(pane, crate::tmux::PANE_SESSION_ID);
                bound.is_empty() || bound == id
            });
        }
        _ => return true,
    };
    if let Some(id) = agent_id.as_deref().filter(|id| !id.is_empty()) {
        let children = crate::tmux::get_pane_option_value(pane, crate::tmux::PANE_SUBAGENTS);
        let tracked = children
            .split(',')
            .any(|entry| entry.rsplit_once(':').is_some_and(|(_, child)| child == id));
        let matching_session = session_id
            .as_deref()
            .is_none_or(|sid| context::bound_session_matches(pane, Some(sid)));
        if matching_session
            && (tracked || context::bound_session_matches(pane, session_id.as_deref()))
        {
            match event {
                AgentEvent::Notification { wait_reason, .. }
                    if crate::tmux::is_actionable_wait_reason(wait_reason) =>
                {
                    handlers::background_permission::request(pane, id, wait_reason);
                }
                AgentEvent::PermissionDenied { .. } => {
                    handlers::background_permission::request(pane, id, "permission_denied");
                }
                AgentEvent::ActivityLog { .. } => {
                    if handlers::stopped_subagents::stopped(pane, id) {
                        handlers::on_subagent_start(pane, "Agent", Some(id));
                    }
                    handlers::background_permission::resume(pane, id);
                }
                _ => {}
            }
        }
        return false;
    }
    if matches!(event, AgentEvent::SessionStart { .. }) {
        return true;
    }
    session_id.as_deref().is_none_or(|id| {
        let bound = crate::tmux::get_pane_option_value(pane, crate::tmux::PANE_SESSION_ID);
        bound.is_empty() || bound == id
    })
}
