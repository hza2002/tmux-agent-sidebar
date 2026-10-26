use crate::tmux;
use serde_json::{Value, json};

pub(in crate::cli::hook) const KEY: &str = tmux::PANE_BACKGROUND_PERMISSION;

/// A child permission temporarily takes attention from the parent's workflow.
/// Keep pending child identities alongside the original parent state.
pub(in crate::cli::hook) fn request(pane: &str, agent_id: &str, reason: &str) {
    request_since(pane, agent_id, reason, "");
}

pub(in crate::cli::hook) fn request_since(pane: &str, agent_id: &str, reason: &str, since: &str) {
    let existing = saved(pane).filter(|state| still_owns_attention(pane, state));
    let mut state = existing.unwrap_or_else(|| {
        json!({
            "status": option(pane, tmux::PANE_STATUS),
            "wait": option(pane, tmux::PANE_WAIT_REASON),
            "attention": option(pane, tmux::PANE_ATTENTION),
            "run": option(pane, tmux::PANE_NOTIFICATION_RUN_ID),
            "since": option(pane, tmux::PANE_STATUS_CHANGED_AT),
            "pending": {},
            "pending_since": {},
        })
    });
    let previous_reason = state["pending"][agent_id].as_str();
    let previous_since = state["pending_since"][agent_id]
        .as_str()
        .unwrap_or_default();
    let since = if !since.is_empty() {
        since.to_owned()
    } else if previous_reason == Some(reason) && !previous_since.is_empty() {
        previous_since.to_owned()
    } else {
        crate::time::now_epoch_millis().to_string()
    };
    if !state["pending_since"].is_object() {
        state["pending_since"] = json!({});
    }
    state["pending"][agent_id] = reason.into();
    state["pending_since"][agent_id] = since.clone().into();
    state["reason"] = reason.into();
    tmux::set_pane_option(pane, KEY, &state.to_string());
    super::on_background_permission(pane, reason);
    restore(pane, tmux::PANE_STATUS_CHANGED_AT, &since);
}

/// Restore parent workflow only after every pending child permission resumes.
/// A newer parent prompt or attention state always wins.
pub(in crate::cli::hook) fn resume(pane: &str, agent_id: &str) {
    let Some(mut state) = saved(pane) else { return };
    if state["pending"].get(agent_id).is_none() {
        return;
    }
    if !still_owns_attention(pane, &state) {
        clear(pane);
        return;
    }
    let pending = state["pending"]
        .as_object_mut()
        .expect("validated pending map");
    pending.remove(agent_id);
    if let Some((id, reason)) = pending
        .iter()
        .next()
        .and_then(|(id, reason)| Some((id.clone(), reason.as_str()?.to_owned())))
    {
        state["reason"] = reason.clone().into();
        tmux::set_pane_option(pane, KEY, &state.to_string());
        tmux::set_pane_option(pane, tmux::PANE_WAIT_REASON, &reason);
        restore(
            pane,
            tmux::PANE_STATUS_CHANGED_AT,
            state["pending_since"][&id].as_str().unwrap_or_default(),
        );
        return;
    }
    clear(pane);
    restore_parent(pane, &state);
}

/// Remove a child overlay before applying a parent event. Comparing the event
/// with its saved parent preserves duplicate parent requests' original age.
pub(in crate::cli::hook) fn rebase(pane: &str) -> Vec<(String, String, String)> {
    let pending = snapshot(pane);
    let state = saved(pane).filter(|state| still_owns_attention(pane, state));
    clear(pane);
    if let Some(state) = state {
        restore_parent(pane, &state);
    }
    pending
}

fn restore_parent(pane: &str, state: &Value) {
    restore(
        pane,
        tmux::PANE_WAIT_REASON,
        state["wait"].as_str().unwrap_or_default(),
    );
    restore(
        pane,
        tmux::PANE_ATTENTION,
        state["attention"].as_str().unwrap_or_default(),
    );
    let status = state["status"].as_str().unwrap_or_default();
    crate::cli::set_status(pane, if status.is_empty() { "clear" } else { status });
    restore(
        pane,
        tmux::PANE_STATUS_CHANGED_AT,
        state["since"].as_str().unwrap_or_default(),
    );
}

#[cfg(test)]
fn owner(pane: &str) -> Vec<(String, String)> {
    snapshot(pane)
        .into_iter()
        .map(|(id, reason, _)| (id, reason))
        .collect()
}

/// Carry individual wait ages across parent workflow rebases.
fn snapshot(pane: &str) -> Vec<(String, String, String)> {
    let Some(state) = saved(pane).filter(|state| still_owns_attention(pane, state)) else {
        return Vec::new();
    };
    state["pending"]
        .as_object()
        .expect("validated pending map")
        .iter()
        .filter_map(|(id, reason)| {
            Some((
                id.clone(),
                reason.as_str()?.to_owned(),
                state["pending_since"][id]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ))
        })
        .collect()
}

pub(in crate::cli::hook) fn clear(pane: &str) {
    tmux::unset_pane_option(pane, KEY);
}

fn option(pane: &str, key: &str) -> String {
    tmux::get_pane_option_value(pane, key)
}

fn saved(pane: &str) -> Option<Value> {
    let state: Value = serde_json::from_str(&option(pane, KEY)).ok()?;
    let strings = ["status", "wait", "attention", "run", "reason"]
        .iter()
        .all(|key| state[*key].is_string());
    let pending = state["pending"].as_object()?;
    (strings && !pending.is_empty() && pending.values().all(Value::is_string)).then_some(state)
}

fn still_owns_attention(pane: &str, state: &Value) -> bool {
    option(pane, tmux::PANE_STATUS) == "waiting"
        && state["reason"].as_str() == Some(option(pane, tmux::PANE_WAIT_REASON).as_str())
        && state["run"].as_str() == Some(option(pane, tmux::PANE_NOTIFICATION_RUN_ID).as_str())
}

fn restore(pane: &str, key: &str, value: &str) {
    if value.is_empty() {
        tmux::unset_pane_option(pane, key);
    } else {
        tmux::set_pane_option(pane, key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(pane: &str) {
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "waiting");
        tmux::test_mock::set(
            pane,
            tmux::PANE_WAIT_REASON,
            tmux::WAIT_REASON_RESPONSE_READY,
        );
        tmux::test_mock::set(pane, tmux::PANE_ATTENTION, "1");
        tmux::test_mock::set(pane, tmux::PANE_NOTIFICATION_RUN_ID, "run-1");
    }

    #[test]
    fn repeated_child_request_and_resume_preserve_each_original_age() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_AGES";
        ready(pane);
        tmux::test_mock::set(pane, tmux::PANE_STATUS_CHANGED_AT, "100000");
        request_since(pane, "first", "permission_prompt", "200000");
        request(pane, "first", "permission_prompt");
        assert_eq!(option(pane, tmux::PANE_STATUS_CHANGED_AT), "200000");
        request_since(pane, "second", "elicitation_dialog", "300000");
        resume(pane, "second");
        assert_eq!(option(pane, tmux::PANE_STATUS_CHANGED_AT), "200000");
        resume(pane, "first");
        assert_eq!(option(pane, tmux::PANE_STATUS_CHANGED_AT), "100000");
        assert_eq!(
            option(pane, tmux::PANE_WAIT_REASON),
            tmux::WAIT_REASON_RESPONSE_READY
        );
    }

    #[test]
    fn parent_rebase_preserves_pending_child_wait_age() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_REBASE_AGE";
        ready(pane);
        request_since(pane, "child", "permission_prompt", "200000");
        let pending = snapshot(pane);
        clear(pane);
        crate::cli::set_status_with_reason(pane, "waiting", tmux::WAIT_REASON_RESPONSE_READY);
        tmux::test_mock::set(pane, tmux::PANE_STATUS_CHANGED_AT, "300000");
        for (id, reason, since) in pending {
            request_since(pane, &id, &reason, &since);
        }
        assert_eq!(option(pane, tmux::PANE_STATUS_CHANGED_AT), "200000");
        resume(pane, "child");
        assert_eq!(option(pane, tmux::PANE_STATUS_CHANGED_AT), "300000");
    }

    #[test]
    fn child_resume_restores_original_unread_response() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_RESTORE_READY";
        ready(pane);
        request(pane, "child", "permission_prompt");
        assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
        resume(pane, "child");
        assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
        assert_eq!(
            option(pane, tmux::PANE_WAIT_REASON),
            tmux::WAIT_REASON_RESPONSE_READY
        );
        assert_eq!(option(pane, tmux::PANE_ATTENTION), "1");
        assert!(option(pane, KEY).is_empty());
    }

    #[test]
    fn all_child_prompts_must_resume_before_parent_state_returns() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_OWNER";
        ready(pane);
        request(pane, "first", "permission_prompt");
        request(pane, "second", "permission_prompt");
        resume(pane, "first");
        assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
        assert!(!option(pane, KEY).is_empty());
        resume(pane, "second");
        assert_eq!(
            option(pane, tmux::PANE_WAIT_REASON),
            tmux::WAIT_REASON_RESPONSE_READY
        );
    }

    #[test]
    fn reverse_child_resume_order_keeps_first_permission_pending() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_REVERSE_ORDER";
        ready(pane);
        request(pane, "first", "permission_prompt");
        request(pane, "second", "permission_denied");
        assert_eq!(owner(pane).len(), 2);
        resume(pane, "unrelated");
        assert_eq!(owner(pane).len(), 2);
        resume(pane, "second");
        assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
        assert_eq!(
            owner(pane),
            vec![("first".into(), "permission_prompt".into())]
        );
        resume(pane, "first");
        assert_eq!(
            option(pane, tmux::PANE_WAIT_REASON),
            tmux::WAIT_REASON_RESPONSE_READY
        );
        assert!(option(pane, KEY).is_empty());
    }

    #[test]
    fn child_resume_does_not_overwrite_a_new_parent_run() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_NEW_RUN";
        ready(pane);
        request(pane, "child", "permission_prompt");
        tmux::test_mock::set(pane, tmux::PANE_NOTIFICATION_RUN_ID, "run-2");
        tmux::test_mock::set(pane, tmux::PANE_STATUS, "running");
        tmux::unset_pane_option(pane, tmux::PANE_WAIT_REASON);
        resume(pane, "child");
        assert_eq!(option(pane, tmux::PANE_STATUS), "running");
        assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
        assert!(option(pane, KEY).is_empty());
    }

    #[test]
    fn child_resume_does_not_overwrite_new_parent_attention() {
        let _guard = tmux::test_mock::install();
        let pane = "%PERMISSION_NEW_ATTENTION";
        ready(pane);
        request(pane, "child", "permission_prompt");
        tmux::test_mock::set(pane, tmux::PANE_WAIT_REASON, "ask_user");
        resume(pane, "child");
        assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "ask_user");
    }
}
