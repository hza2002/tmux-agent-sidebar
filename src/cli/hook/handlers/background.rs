use crate::{event::BackgroundTask, tmux};

use super::super::context::{append_subagent, drain_pending_teardowns};

/// Reconcile a registry observation, keeping absence distinct from emptiness.
/// A SubagentStop snapshot can still contain the agent whose stop triggered it.
pub(in crate::cli::hook) fn reconcile(
    pane: &str,
    tasks: Option<&[BackgroundTask]>,
    stopping: Option<&str>,
) {
    let Some(tasks) = tasks else { return };
    let mut children = if stopping.is_some() {
        tmux::get_pane_option_value(pane, tmux::PANE_SUBAGENTS)
    } else {
        String::new()
    };
    if let Some(id) = stopping {
        children = super::super::context::remove_subagent(&children, id).unwrap_or(children);
    }
    let mut other = Vec::new();
    for task in tasks {
        if stopping == Some(task.id.as_str()) {
            continue;
        }
        let label = crate::cli::sanitize_tmux_value(&task.label);
        match task.kind.as_str() {
            "subagent" => {
                if super::stopped_subagents::stopped(pane, &task.id) {
                    continue;
                }
                // The existing child option uses comma/colon separators.
                let label = label.replace([',', ':'], " ");
                let id = task.id.replace([',', ':', '\n', '\r'], "_");
                children = append_subagent(&children, &label, &id);
            }
            _ => other.push(BackgroundTask {
                label,
                ..task.clone()
            }),
        }
    }
    set_or_clear(pane, tmux::PANE_SUBAGENTS, &children);
    // Child hooks can acquire the lock after a newer parent Stop. Their
    // snapshots may still describe already-ended shells or workflows, and
    // those tasks have no child stop ID with which to reject stale entries.
    // Only the parent reconciles non-subagent work; child completion still
    // removes its own ID and lets the normal parent wakeup publish an update.
    if stopping.is_none() {
        tmux::unset_pane_option(pane, tmux::PANE_BG_CMD);
        if other.is_empty() {
            tmux::unset_pane_option(pane, tmux::PANE_BACKGROUND_TASKS);
        } else {
            tmux::set_pane_option(
                pane,
                tmux::PANE_BACKGROUND_TASKS,
                &BackgroundTask::encode_list(&other),
            );
        }
    }
    if children.is_empty() {
        drain_pending_teardowns(pane);
    }
    settle_background(pane);
}

fn set_or_clear(pane: &str, key: &str, value: &str) {
    if value.is_empty() {
        tmux::unset_pane_option(pane, key);
    } else {
        tmux::set_pane_option(pane, key, value);
    }
}

/// Ending background work never consumes an unread reply or a permission prompt.
pub(super) fn settle_background(pane: &str) {
    if tmux::get_pane_option_value(pane, tmux::PANE_STATUS) == "background"
        && [
            tmux::PANE_SUBAGENTS,
            tmux::PANE_BG_CMD,
            tmux::PANE_BACKGROUND_TASKS,
        ]
        .iter()
        .all(|key| tmux::get_pane_option_value(pane, key).is_empty())
    {
        crate::cli::set_status(pane, "idle");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(id: &str) -> BackgroundTask {
        BackgroundTask {
            id: id.into(),
            kind: "workflow".into(),
            label: "Integration checks".into(),
        }
    }

    #[test]
    fn parent_stop_registry_cannot_be_resurrected_by_child_snapshot() {
        let _guard = tmux::test_mock::install();
        let pane = "%STALE_CHILD_REGISTRY";
        let old = vec![workflow("old-work")];
        reconcile(pane, Some(&old), None);
        assert!(!tmux::get_pane_option_value(pane, tmux::PANE_BACKGROUND_TASKS).is_empty());

        // The parent observed all work finished before an older child hook
        // acquired the pane lock. That child's snapshot must not revive it.
        reconcile(pane, Some(&[]), None);
        reconcile(pane, Some(&old), Some("child"));

        assert!(tmux::get_pane_option_value(pane, tmux::PANE_BACKGROUND_TASKS).is_empty());
    }

    #[test]
    fn parent_stop_registry_replaces_non_subagent_tasks() {
        let _guard = tmux::test_mock::install();
        let pane = "%PARENT_REGISTRY_UPDATE";
        let old = vec![workflow("old-work")];
        let new = vec![workflow("new-work")];
        reconcile(pane, Some(&old), None);
        reconcile(pane, Some(&new), None);

        assert_eq!(
            BackgroundTask::decode_list(&tmux::get_pane_option_value(
                pane,
                tmux::PANE_BACKGROUND_TASKS,
            )),
            Some(new),
        );
    }

    #[test]
    fn child_snapshot_does_not_clear_parent_registry_or_legacy_shell() {
        let _guard = tmux::test_mock::install();
        let pane = "%PARENT_REGISTRY_PRESERVED";
        let tasks = vec![workflow("current-work")];
        reconcile(pane, Some(&tasks), None);
        tmux::test_mock::set(pane, tmux::PANE_BG_CMD, "cargo watch");

        reconcile(pane, Some(&[]), Some("child"));

        assert_eq!(
            BackgroundTask::decode_list(&tmux::get_pane_option_value(
                pane,
                tmux::PANE_BACKGROUND_TASKS,
            )),
            Some(tasks),
        );
        assert_eq!(
            tmux::get_pane_option_value(pane, tmux::PANE_BG_CMD),
            "cargo watch",
        );
    }
}
