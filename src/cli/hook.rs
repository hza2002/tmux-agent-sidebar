use crate::event::{AgentEvent, resolve_adapter};

use super::{read_stdin_json, tmux_pane};

mod activity;
mod context;
mod handlers;
mod notifications;

use context::sync_pane_location;
use notifications::notification_settings;

struct PaneHookLock(std::fs::File);

impl PaneHookLock {
    fn acquire(pane: &str) -> Result<Self, ()> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};

        let uid = unsafe { libc::geteuid() };
        let lock_dir = std::env::temp_dir().join(format!("tmux-agent-sidebar-{uid}"));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        if let Err(error) = builder.create(&lock_dir)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(());
        }
        let metadata = std::fs::symlink_metadata(&lock_dir).map_err(|_| ())?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(());
        }

        let pane = pane.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let server = tmux_server_lock_id(std::env::var("TMUX").ok().as_deref());
        let path = lock_dir.join(format!("hook-{server}-{pane}.lock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| ())?;
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(());
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(());
        }
        Ok(Self(file))
    }
}

fn tmux_server_lock_id(tmux: Option<&str>) -> String {
    // Stable FNV-1a keeps socket paths out of filenames while separating servers.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in tmux.unwrap_or("unknown").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

impl Drop for PaneHookLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

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

    let pane = tmux_pane();
    if pane.is_empty() {
        return 0;
    }

    let input = read_stdin_json();
    let Some(event) = adapter.parse(event_name, &input) else {
        return 0;
    };

    let Ok(_lock) = PaneHookLock::acquire(&pane) else {
        return 1;
    };
    handle_event(&pane, agent_name, event)
}

// ─── event handler ──────────────────────────────────────────────────────────

fn handle_event(pane: &str, agent_name: &str, event: AgentEvent) -> i32 {
    match event {
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
        AgentEvent::SessionEnd { end_reason } => {
            let notifications = notification_settings();
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
            response,
            worktree,
            session_id,
            turn_id,
            ..
        } => {
            let notifications = notification_settings();
            handlers::on_stop(
                pane,
                &context::make_ctx(&agent, &cwd, &permission_mode, &worktree, &session_id),
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
        } => handlers::on_subagent_start(pane, &agent_type, agent_id.as_deref()),
        AgentEvent::SubagentStop { agent_id, .. } => {
            handlers::on_subagent_stop(pane, agent_id.as_deref())
        }
        AgentEvent::ActivityLog {
            tool_name,
            tool_input,
            tool_response,
        } => activity::handle_activity_log(pane, &tool_name, &tool_input, &tool_response),
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

#[cfg(test)]
mod tests {
    use super::tmux_server_lock_id;

    #[test]
    fn lock_identity_is_stable_and_server_specific() {
        assert_eq!(
            tmux_server_lock_id(Some("/tmp/tmux-501/default,100,0")),
            tmux_server_lock_id(Some("/tmp/tmux-501/default,100,0"))
        );
        assert_ne!(
            tmux_server_lock_id(Some("/tmp/tmux-501/default,100,0")),
            tmux_server_lock_id(Some("/tmp/tmux-501/other,200,0"))
        );
    }
}
