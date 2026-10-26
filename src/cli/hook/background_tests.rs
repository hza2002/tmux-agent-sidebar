use super::handle_event;
use crate::{event::resolve_adapter, tmux};
use serde_json::{Value, json};

fn replay(pane: &str, name: &str, payload: Value) {
    let event = resolve_adapter("claude")
        .unwrap()
        .parse(name, &payload)
        .unwrap();
    assert_eq!(handle_event(pane, "claude", event), 0);
}

fn parent(pane: &str) {
    for (key, value) in [
        (tmux::PANE_AGENT, "claude"),
        (tmux::PANE_SESSION_ID, "parent"),
        (tmux::PANE_CWD, "/repo/parent"),
        (tmux::PANE_PERMISSION_MODE, "default"),
        (tmux::PANE_STATUS, "running"),
    ] {
        tmux::test_mock::set(pane, key, value);
    }
}

fn start_child(pane: &str) {
    replay(
        pane,
        "subagent-start",
        json!({
            "session_id": "parent", "agent_id": "child", "agent_type": "Explore"
        }),
    );
}

fn option(pane: &str, key: &str) -> String {
    tmux::test_mock::get(pane, key).unwrap_or_default()
}

#[test]
fn parent_prompt_and_background_wakeup_work_while_child_is_active() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_PARENT_PROMPT";
    parent(pane);
    start_child(pane);
    tmux::test_mock::set(pane, tmux::PANE_COMPLETED_TURN_ID, "legacy");
    tmux::test_mock::set(
        pane,
        tmux::PANE_WAIT_REASON,
        tmux::WAIT_REASON_RESPONSE_READY,
    );
    replay(
        pane,
        "user-prompt-submit",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "prompt": "continue review"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert_eq!(option(pane, tmux::PANE_PROMPT), "continue review");
    assert!(option(pane, tmux::PANE_COMPLETED_TURN_ID).is_empty());
    replay(
        pane,
        "user-prompt-submit",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "prompt": "<task-notification>result</task-notification>"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_PROMPT), "continue review");
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
}

#[test]
fn child_lifecycle_cannot_replace_parent_reply_or_identity() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_CHILD_LIFECYCLE";
    parent(pane);
    start_child(pane);
    for name in [
        "stop",
        "stop-failure",
        "session-end",
        "user-prompt-submit",
        "cwd-changed",
    ] {
        replay(
            pane,
            name,
            json!({
                "session_id": "parent", "agent_id": "child", "cwd": "/repo/child",
                "permission_mode": "plan", "prompt": "child prompt", "last_assistant_message": "child reply",
                "background_tasks": []
            }),
        );
    }
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert_eq!(option(pane, tmux::PANE_CWD), "/repo/parent");
    assert_eq!(option(pane, tmux::PANE_PERMISSION_MODE), "default");
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert!(option(pane, tmux::PANE_PROMPT).is_empty());
    assert!(option(pane, tmux::PANE_COMPLETED_TURN_ID).is_empty());
}

#[test]
fn child_permission_surfaces_without_overwriting_parent_metadata() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_CHILD_PERMISSION";
    parent(pane);
    start_child(pane);
    tmux::test_mock::set(pane, tmux::PANE_COMPLETED_TURN_ID, "legacy");
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "agent_id": "child", "cwd": "/repo/child",
            "permission_mode": "plan", "notification_type": "permission_prompt"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
    assert!(!option(pane, tmux::PANE_ATTENTION).is_empty());
    assert_eq!(option(pane, tmux::PANE_CWD), "/repo/parent");
    assert_eq!(option(pane, tmux::PANE_PERMISSION_MODE), "default");
}

#[test]
fn parent_stop_keeps_reply_ready_with_child_then_child_stop_only_updates_work() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_STOP_REPLY";
    parent(pane);
    start_child(pane);
    let task =
        json!({"id": "child", "type": "subagent", "agent_type": "Explore", "status": "running"});
    replay(
        pane,
        "stop",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "last_assistant_message": "A reply is available",
            "background_tasks": [task.clone()]
        }),
    );
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
    assert_eq!(option(pane, tmux::PANE_PROMPT), "A reply is available");
    let completed = option(pane, tmux::PANE_COMPLETED_TURN_ID);
    replay(
        pane,
        "subagent-stop",
        json!({
            "session_id": "parent", "agent_id": "child", "agent_type": "Explore",
            "background_tasks": [task], "last_assistant_message": "child reply"
        }),
    );
    assert!(option(pane, tmux::PANE_SUBAGENTS).is_empty());
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
    assert_eq!(option(pane, tmux::PANE_PROMPT), "A reply is available");
    assert_eq!(option(pane, tmux::PANE_COMPLETED_TURN_ID), completed);
}

#[test]
fn missing_stop_snapshot_preserves_children_and_empty_snapshot_clears_them() {
    let _guard = tmux::test_mock::install();
    for (pane, snapshot) in [
        ("%BACKGROUND_UNKNOWN", None),
        ("%BACKGROUND_EMPTY", Some(json!([]))),
    ] {
        parent(pane);
        start_child(pane);
        let mut payload = json!({"session_id": "parent", "cwd": "/repo/parent"});
        if let Some(snapshot) = snapshot {
            payload["background_tasks"] = snapshot;
        }
        replay(pane, "stop", payload);
        assert_eq!(
            option(pane, tmux::PANE_SUBAGENTS).is_empty(),
            pane == "%BACKGROUND_EMPTY"
        );
    }
}

#[test]
fn stale_session_cannot_replace_background_snapshot_or_append_children() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_STALE";
    parent(pane);
    start_child(pane);
    for name in ["stop", "subagent-stop", "subagent-start"] {
        replay(
            pane,
            name,
            json!({
                "session_id": "old", "agent_id": "child", "agent_type": "Explore", "background_tasks": []
            }),
        );
    }
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert!(option(pane, tmux::PANE_COMPLETED_TURN_ID).is_empty());
}

#[test]
fn duplicate_child_start_and_resume_keep_one_entry() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_DUPLICATE";
    parent(pane);
    start_child(pane);
    start_child(pane);
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    replay(
        pane,
        "subagent-stop",
        json!({"session_id": "parent", "agent_id": "child", "agent_type": "Explore"}),
    );
    start_child(pane);
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
}

#[test]
fn finishing_background_child_parks_without_creating_parent_completion() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_DRAIN";
    parent(pane);
    start_child(pane);
    tmux::test_mock::set(pane, tmux::PANE_STATUS, "background");
    replay(
        pane,
        "subagent-stop",
        json!({"session_id": "parent", "agent_id": "child", "agent_type": "Explore"}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "idle");
    assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
    assert!(option(pane, tmux::PANE_COMPLETED_TURN_ID).is_empty());
    assert!(option(pane, tmux::PANE_ATTENTION).is_empty());
}

#[test]
fn stale_child_permission_cannot_interrupt_new_parent_session() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_STALE_PERMISSION";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "notification",
        json!({
            "session_id": "old", "agent_id": "child", "notification_type": "permission_prompt"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
    assert!(option(pane, tmux::PANE_ATTENTION).is_empty());
}

#[test]
fn parent_session_end_cleans_up_even_with_background_children() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_PARENT_END";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "session-end",
        json!({"session_id": "parent", "end_reason": "logout"}),
    );
    assert!(option(pane, tmux::PANE_SESSION_ID).is_empty());
    assert!(option(pane, tmux::PANE_AGENT).is_empty());
    assert!(option(pane, tmux::PANE_SUBAGENTS).is_empty());
}

#[test]
fn resumed_child_moves_idle_parent_to_background() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_RESUME_IDLE";
    parent(pane);
    tmux::test_mock::set(pane, tmux::PANE_STATUS, "idle");
    start_child(pane);
    assert_eq!(option(pane, tmux::PANE_STATUS), "background");
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
    assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
    assert!(option(pane, tmux::PANE_ATTENTION).is_empty());
}

#[test]
fn approved_child_tool_restores_parent_reply_without_logging_child_activity() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_APPROVE_CHILD";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "stop",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "last_assistant_message": "Review this reply"
        }),
    );
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "agent_id": "child", "notification_type": "permission_prompt"
        }),
    );
    replay(
        pane,
        "activity-log",
        json!({
            "session_id": "parent", "agent_id": "child", "tool_name": "Read", "tool_input": {"file_path": "/repo/child"}
        }),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
    assert_eq!(option(pane, tmux::PANE_PROMPT), "Review this reply");
    assert_eq!(option(pane, tmux::PANE_CWD), "/repo/parent");
    assert!(option(pane, super::handlers::background_permission::KEY).is_empty());
}

#[test]
fn child_resume_cannot_restore_old_reply_after_parent_prompt() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_RESUME_AFTER_PROMPT";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "agent_id": "child", "notification_type": "permission_prompt"
        }),
    );
    replay(
        pane,
        "user-prompt-submit",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "prompt": "New direction"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    replay(
        pane,
        "activity-log",
        json!({
            "session_id": "parent", "agent_id": "child", "tool_name": "Read"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert_eq!(option(pane, tmux::PANE_PROMPT), "New direction");
    assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
    assert!(option(pane, super::handlers::background_permission::KEY).is_empty());
}

#[test]
fn parent_reply_during_child_permission_is_restored_after_child_resumes() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_REPLY_DURING_PERMISSION";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "agent_id": "child", "notification_type": "permission_prompt"
        }),
    );
    replay(
        pane,
        "stop",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "last_assistant_message": "New reply while child waits"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    assert_eq!(
        option(pane, tmux::PANE_PROMPT),
        "New reply while child waits"
    );
    replay(
        pane,
        "activity-log",
        json!({
            "session_id": "parent", "agent_id": "child", "tool_name": "Read"
        }),
    );
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
    assert_eq!(
        option(pane, tmux::PANE_PROMPT),
        "New reply while child waits"
    );
}

#[test]
fn informational_notifications_preserve_unread_reply_and_child_permission() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_INFORMATIONAL";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "stop",
        json!({
            "session_id": "parent", "cwd": "/repo/parent", "last_assistant_message": "Unread reply"
        }),
    );
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "notification_type": "auth_success"
        }),
    );
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
    assert!(!option(pane, tmux::PANE_ATTENTION).is_empty());
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "agent_id": "child", "notification_type": "permission_prompt"
        }),
    );
    replay(
        pane,
        "notification",
        json!({
            "session_id": "parent", "notification_type": "auth_success"
        }),
    );
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    assert!(!option(pane, tmux::PANE_ATTENTION).is_empty());
    assert!(!option(pane, super::handlers::background_permission::KEY).is_empty());
}

#[test]
fn out_of_order_snapshots_cannot_resurrect_stopped_children_but_resume_can() {
    let _guard = tmux::test_mock::install();
    let pane = "%BACKGROUND_SNAPSHOT_RACE";
    parent(pane);
    let tasks = json!([
        {"id": "A", "type": "subagent", "agent_type": "Explore", "status": "running"},
        {"id": "B", "type": "subagent", "agent_type": "Plan", "status": "running"}
    ]);
    for (id, kind) in [("A", "Explore"), ("B", "Plan")] {
        replay(
            pane,
            "subagent-start",
            json!({"session_id": "parent", "agent_id": id, "agent_type": kind}),
        );
    }
    for (id, kind) in [("A", "Explore"), ("B", "Plan")] {
        replay(
            pane,
            "subagent-stop",
            json!({
                "session_id": "parent", "agent_id": id, "agent_type": kind, "background_tasks": tasks.clone()
            }),
        );
    }
    assert!(option(pane, tmux::PANE_SUBAGENTS).is_empty());
    replay(
        pane,
        "stop",
        json!({"session_id": "parent", "background_tasks": tasks.clone()}),
    );
    assert!(option(pane, tmux::PANE_SUBAGENTS).is_empty());
    replay(
        pane,
        "subagent-start",
        json!({"session_id": "parent", "agent_id": "B", "agent_type": "Plan"}),
    );
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Plan:B");
    replay(
        pane,
        "user-prompt-submit",
        json!({"session_id": "parent", "prompt": "Continue"}),
    );
    replay(
        pane,
        "stop",
        json!({"session_id": "parent", "background_tasks": tasks}),
    );
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Plan:B");
}

#[test]
fn child_continuation_after_blocked_stop_reestablishes_liveness() {
    let _guard = tmux::test_mock::install();
    let pane = "%CHILD_STOP_CONTINUED";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "subagent-stop",
        json!({"session_id":"parent", "agent_id":"child", "agent_type":"Explore"}),
    );
    replay(
        pane,
        "activity-log",
        json!({"session_id":"parent", "agent_id":"child", "tool_name":"Read", "tool_input":{"file_path":"src/lib.rs"}}),
    );
    replay(
        pane,
        "stop",
        json!({"session_id":"parent", "background_tasks":[{"id":"child", "type":"subagent", "agent_type":"Explore", "status":"running"}]}),
    );
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
}

#[test]
fn unidentified_child_stop_cannot_clear_registry() {
    let _guard = tmux::test_mock::install();
    let pane = "%UNKNOWN_CHILD_STOP";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "subagent-stop",
        json!({"session_id":"parent", "agent_type":"Explore", "background_tasks":[]}),
    );
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
}

#[test]
fn parent_permission_after_child_permission_survives_child_resume() {
    let _guard = tmux::test_mock::install();
    let pane = "%PARENT_PERMISSION_AFTER_CHILD";
    parent(pane);
    start_child(pane);
    tmux::test_mock::set(pane, tmux::PANE_STATUS, "waiting");
    tmux::test_mock::set(
        pane,
        tmux::PANE_WAIT_REASON,
        tmux::WAIT_REASON_RESPONSE_READY,
    );
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "agent_id":"child", "notification_type":"permission_prompt"}),
    );
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "notification_type":"permission_prompt"}),
    );
    replay(
        pane,
        "activity-log",
        json!({"session_id":"parent", "agent_id":"child", "tool_name":"Read", "tool_input":{}}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
}

#[test]
fn compact_preserves_live_background_work_and_pending_child_permission() {
    let _guard = tmux::test_mock::install();
    let pane = "%COMPACT_BACKGROUND_CHILD";
    parent(pane);
    start_child(pane);
    tmux::test_mock::set(pane, tmux::PANE_STATUS, "background");
    replay(
        pane,
        "session-start",
        json!({"session_id":"parent", "source":"compact", "cwd":"/repo/parent"}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "background");
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "agent_id":"child", "notification_type":"permission_prompt"}),
    );
    replay(
        pane,
        "session-start",
        json!({"session_id":"parent", "source":"compact", "cwd":"/repo/parent"}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    replay(
        pane,
        "activity-log",
        json!({"session_id":"parent", "agent_id":"child", "tool_name":"Read", "tool_input":{}}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "background");
    assert_eq!(option(pane, tmux::PANE_SUBAGENTS), "Explore:child");
}

#[test]
fn parent_activity_preserves_pending_child_permission() {
    let _guard = tmux::test_mock::install();
    let pane = "%PARENT_ACTIVITY_CHILD_PERMISSION";
    parent(pane);
    start_child(pane);
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "agent_id":"child", "notification_type":"permission_prompt"}),
    );
    replay(
        pane,
        "activity-log",
        json!({"session_id":"parent", "tool_name":"Read", "tool_input":{}}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    replay(
        pane,
        "activity-log",
        json!({"session_id":"parent", "agent_id":"child", "tool_name":"Read", "tool_input":{}}),
    );
    assert_eq!(option(pane, tmux::PANE_STATUS), "running");
    assert!(option(pane, tmux::PANE_WAIT_REASON).is_empty());
}

#[test]
fn parent_stop_reconciles_child_permissions_with_known_registry() {
    let _guard = tmux::test_mock::install();
    for (pane, snapshot, expected) in [
        (
            "%STOP_PERMISSION_EMPTY",
            Some(json!([])),
            tmux::WAIT_REASON_RESPONSE_READY,
        ),
        ("%STOP_PERMISSION_UNKNOWN", None, "permission_prompt"),
    ] {
        parent(pane);
        start_child(pane);
        replay(
            pane,
            "notification",
            json!({"session_id":"parent", "agent_id":"child", "notification_type":"permission_prompt"}),
        );
        let mut payload = json!({"session_id":"parent", "last_assistant_message":"New reply"});
        if let Some(snapshot) = snapshot {
            payload["background_tasks"] = snapshot;
        }
        replay(pane, "stop", payload);
        assert_eq!(option(pane, tmux::PANE_WAIT_REASON), expected);
        if expected == tmux::WAIT_REASON_RESPONSE_READY {
            assert!(option(pane, tmux::PANE_BACKGROUND_PERMISSION).is_empty());
        }
    }
}

#[test]
fn late_parent_permission_after_completed_turn_without_work_is_ignored() {
    let _guard = tmux::test_mock::install();
    let pane = "%LATE_PARENT_PERMISSION_NO_WORK";
    parent(pane);
    replay(
        pane,
        "stop",
        json!({"session_id":"parent", "last_assistant_message":"Reply ready", "background_tasks":[]}),
    );
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "notification_type":"permission_prompt"}),
    );
    assert_eq!(
        option(pane, tmux::PANE_WAIT_REASON),
        tmux::WAIT_REASON_RESPONSE_READY
    );
}

#[test]
fn parent_permission_after_completed_turn_with_background_work_is_actionable() {
    let _guard = tmux::test_mock::install();
    let pane = "%PARENT_PERMISSION_BACKGROUND_WORK";
    parent(pane);
    replay(
        pane,
        "stop",
        json!({"session_id":"parent", "last_assistant_message":"Reply ready", "background_tasks":[{"id":"shell-1", "type":"shell", "status":"running", "command":"cargo test"}]}),
    );
    replay(
        pane,
        "notification",
        json!({"session_id":"parent", "notification_type":"permission_prompt"}),
    );
    assert_eq!(option(pane, tmux::PANE_WAIT_REASON), "permission_prompt");
    assert_eq!(option(pane, tmux::PANE_STATUS), "waiting");
}
