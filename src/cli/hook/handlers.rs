mod attention;
pub(super) mod background;
pub(super) mod background_permission;
mod run;
mod session;
mod status_priority;
pub(super) mod stopped_subagents;
mod subagent;
mod worktree;

pub(super) use attention::{
    on_background_permission, on_notification, on_permission_denied, on_permission_request,
    on_permission_result, on_teammate_idle,
};
pub(super) use run::{
    on_interrupt, on_stop, on_stop_failure, on_task_completed, on_user_prompt_submit,
};
pub(super) use session::{on_session_end, on_session_start, on_session_title};
pub(super) use subagent::{on_subagent_start, on_subagent_stop};
pub(super) use worktree::on_worktree_remove;
