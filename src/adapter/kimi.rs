use crate::event::{AgentEvent, AgentEventKind, EventAdapter};
use crate::tmux::KIMI_AGENT;
use crate::tool_name::CanonicalTool;
use serde_json::Value;

use super::{
    HookRegistration, alias_keys, canonical_tool_name, json_str, json_value_or_null, optional_str,
    tool_input_value,
};

pub struct KimiAdapter;

/// Kimi names most tools the way Claude Code does, but four of them differ.
/// They are mapped here so the label strategy table, the colour classifier,
/// and the plan-mode badge all see the shared vocabulary.
const TOOL_ALIASES: &[(&str, CanonicalTool)] = &[
    ("FetchURL", CanonicalTool::WebFetch),
    ("ReadMediaFile", CanonicalTool::Read),
    ("TodoList", CanonicalTool::TodoWrite),
    ("AgentSwarm", CanonicalTool::Agent),
];

/// Kimi spells the file argument `path`, where the label extractor expects the
/// `file_path` the other agents use.
fn tool_arg_aliases(tool_name: &str) -> &'static [(&'static str, &'static str)] {
    // Keyed on the parsed variant, not on the name's spelling: a renamed
    // canonical spelling round-trips through `from_name` and still aliases.
    match CanonicalTool::from_name(tool_name) {
        Some(CanonicalTool::Read | CanonicalTool::Write | CanonicalTool::Edit) => {
            &[("path", "file_path")]
        }
        _ => &[],
    }
}

impl KimiAdapter {
    /// Single source of truth for Kimi Code hook wiring. Kimi registers
    /// hooks as `[[hooks]]` entries in `~/.kimi-code/config.toml` with
    /// `event` / `matcher` / `command` / `timeout` fields; the setup
    /// subcommand renders this table as a paste-ready TOML block.
    ///
    /// Caveats:
    /// - Kimi appends hook stdout to the model context on exit 0, so every
    ///   event — including Stop — must keep `response: None`. Allow
    ///   semantics are "exit 0 means allow"; stdout is never required.
    /// - Kimi payloads carry no `permission_mode` field, and its
    ///   long-running `kimi-code` process drops CLI flags from argv after
    ///   startup, so the badge stays unset for Kimi panes.
    /// - Kimi fires `Interrupt` in place of `Stop` when the user aborts a
    ///   turn (Esc). It maps to the dedicated `interrupt` event, not
    ///   `stop`: an abort is not a completion, so no response-ready state
    ///   or completion notification may fire.
    /// - `PostToolUseFailure` maps to the dedicated `tool-failure` event so
    ///   failed calls are visibly marked in the activity log instead of
    ///   being indistinguishable from successful ones.
    /// - Foreground AskUserQuestion waits fire no hook at all (verified
    ///   against Kimi Code 0.41.0 and the official event reference), so a
    ///   question prompt still looks like `running` until it is answered.
    pub const HOOK_REGISTRATIONS: &'static [HookRegistration] = &[
        HookRegistration {
            trigger: "SessionStart",
            matcher: Some("startup|resume"),
            kind: AgentEventKind::SessionStart,
        },
        HookRegistration {
            trigger: "SessionEnd",
            matcher: None,
            kind: AgentEventKind::SessionEnd,
        },
        HookRegistration {
            trigger: "UserPromptSubmit",
            matcher: None,
            kind: AgentEventKind::UserPromptSubmit,
        },
        HookRegistration {
            trigger: "Stop",
            matcher: None,
            kind: AgentEventKind::Stop,
        },
        HookRegistration {
            trigger: "Interrupt",
            matcher: None,
            kind: AgentEventKind::Interrupt,
        },
        HookRegistration {
            trigger: "StopFailure",
            matcher: None,
            kind: AgentEventKind::StopFailure,
        },
        HookRegistration {
            trigger: "PostToolUse",
            matcher: None,
            kind: AgentEventKind::ActivityLog,
        },
        HookRegistration {
            trigger: "PostToolUseFailure",
            matcher: None,
            kind: AgentEventKind::ToolFailure,
        },
        HookRegistration {
            trigger: "Notification",
            matcher: None,
            kind: AgentEventKind::Notification,
        },
        HookRegistration {
            trigger: "SubagentStart",
            matcher: None,
            kind: AgentEventKind::SubagentStart,
        },
        HookRegistration {
            trigger: "SubagentStop",
            matcher: None,
            kind: AgentEventKind::SubagentStop,
        },
        HookRegistration {
            trigger: "PermissionRequest",
            matcher: None,
            kind: AgentEventKind::PermissionRequest,
        },
        HookRegistration {
            trigger: "PermissionResult",
            matcher: None,
            kind: AgentEventKind::PermissionResult,
        },
        HookRegistration {
            trigger: "TaskStarted",
            matcher: None,
            kind: AgentEventKind::TaskCreated,
        },
    ];
}

/// First non-empty string among `keys`, for payload fields whose exact name
/// is not pinned down by the Kimi hook documentation.
fn first_present<'a>(input: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .map(|key| json_str(input, key))
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// Kimi documents no field name for the subagent type on SubagentStart /
/// SubagentStop payloads; accept the Claude-style name first and tolerate
/// the plausible alternatives.
fn subagent_type(input: &Value) -> &str {
    first_present(input, &["agent_type", "subagent_name", "name"])
}

/// Kimi sends the prompt as an array of content parts
/// (`[{"type":"text","text":"..."}]`), unlike Claude/Codex's plain string.
/// Tolerate both shapes.
fn prompt_text(input: &Value) -> String {
    match input.get("prompt") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

impl EventAdapter for KimiAdapter {
    fn parse(&self, event_name: &str, input: &Value) -> Option<AgentEvent> {
        match event_name {
            "session-start" => Some(AgentEvent::SessionStart {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                // Kimi payloads carry no permission_mode (verified against
                // a live 0.41.0 payload), and process argv drops the CLI
                // flags after startup, so there is no mode source at all.
                permission_mode: String::new(),
                source: json_str(input, "source").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "session-end" => Some(AgentEvent::SessionEnd {
                end_reason: first_present(input, &["reason", "source"]).into(),
            }),
            "user-prompt-submit" => Some(AgentEvent::UserPromptSubmit {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                prompt: prompt_text(input),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            "stop" => Some(AgentEvent::Stop {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                last_message: first_present(input, &["last_message", "last_assistant_message"])
                    .into(),
                // Kimi appends hook stdout to the model context on exit 0,
                // so the adapter must never emit a response payload.
                response: None,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            "stop-failure" => Some(AgentEvent::StopFailure {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                error: first_present(input, &["error", "error_type"]).into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "activity-log" => {
                let raw_name = json_str(input, "tool_name");
                if raw_name.is_empty() {
                    return None;
                }
                let tool_name = canonical_tool_name(TOOL_ALIASES, raw_name);
                let tool_input = alias_keys(
                    tool_input_value(input, "tool_input"),
                    tool_arg_aliases(&tool_name),
                );
                Some(AgentEvent::ActivityLog {
                    tool_name,
                    tool_input,
                    tool_response: json_value_or_null(input, "tool_response"),
                    session_id: optional_str(input, "session_id"),
                    turn_id: optional_str(input, "turn_id"),
                })
            }
            "tool-failure" => {
                let raw_name = json_str(input, "tool_name");
                if raw_name.is_empty() {
                    return None;
                }
                let tool_name = canonical_tool_name(TOOL_ALIASES, raw_name);
                let tool_input = alias_keys(
                    tool_input_value(input, "tool_input"),
                    tool_arg_aliases(&tool_name),
                );
                Some(AgentEvent::ToolFailure {
                    tool_name,
                    tool_input,
                    error: first_present(input, &["error", "error_type"]).into(),
                    session_id: optional_str(input, "session_id"),
                    turn_id: optional_str(input, "turn_id"),
                })
            }
            "interrupt" => Some(AgentEvent::Interrupt {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            "notification" => Some(AgentEvent::Notification {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                wait_reason: json_str(input, "wait_reason").into(),
                meta_only: false,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "subagent-start" => {
                let agent_type = subagent_type(input);
                if agent_type.is_empty() {
                    return None;
                }
                Some(AgentEvent::SubagentStart {
                    agent_type: agent_type.into(),
                    agent_id: optional_str(input, "agent_id"),
                })
            }
            "subagent-stop" => {
                let agent_type = subagent_type(input);
                if agent_type.is_empty() {
                    return None;
                }
                Some(AgentEvent::SubagentStop {
                    agent_type: agent_type.into(),
                    agent_id: optional_str(input, "agent_id"),
                    last_message: first_present(input, &["last_message", "last_assistant_message"])
                        .into(),
                    transcript_path: json_str(input, "agent_transcript_path").into(),
                })
            }
            "permission-request" => {
                let tool_name = canonical_tool_name(TOOL_ALIASES, json_str(input, "tool_name"));
                let tool_input = alias_keys(
                    tool_input_value(input, "tool_input"),
                    tool_arg_aliases(&tool_name),
                );
                Some(AgentEvent::PermissionRequest {
                    agent: KIMI_AGENT.into(),
                    cwd: json_str(input, "cwd").into(),
                    permission_mode: String::new(),
                    tool_name,
                    tool_input,
                    agent_id: optional_str(input, "agent_id"),
                    session_id: optional_str(input, "session_id"),
                    turn_id: optional_str(input, "turn_id"),
                })
            }
            "permission-result" => Some(AgentEvent::PermissionResult {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                agent_id: optional_str(input, "agent_id"),
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            // Kimi's TaskStarted payload carries `task_id`, `description`,
            // and `detached`; map it onto the shared task-created shape.
            "task-created" => Some(AgentEvent::TaskCreated {
                task_id: json_str(input, "task_id").into(),
                task_subject: first_present(input, &["description", "subject", "task_subject"])
                    .into(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_registrations_match_parse_arms() {
        super::super::assert_table_drift_free("kimi", KimiAdapter::HOOK_REGISTRATIONS);
    }

    #[test]
    fn every_alias_target_is_in_the_canonical_vocabulary() {
        super::super::assert_aliases_are_canonical("kimi", TOOL_ALIASES);
    }

    #[test]
    fn session_start() {
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": "sess-kimi-1",
            "client_type": "cli",
            "cwd": "/home/user",
            "source": "startup",
            "model": "kimi-k2",
            "profile": "default"
        });
        let event = KimiAdapter.parse("session-start", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: KIMI_AGENT.into(),
                cwd: "/home/user".into(),
                permission_mode: "".into(),
                source: "startup".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-kimi-1".into()),
            }
        );
    }

    #[test]
    fn session_start_missing_fields_default_to_empty() {
        let event = KimiAdapter.parse("session-start", &json!({})).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: KIMI_AGENT.into(),
                cwd: "".into(),
                permission_mode: "".into(),
                source: "".into(),
                worktree: None,
                agent_id: None,
                session_id: None,
            }
        );
    }

    #[test]
    fn session_end() {
        let event = KimiAdapter
            .parse("session-end", &json!({"reason": "exit"}))
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionEnd {
                end_reason: "exit".into()
            }
        );
    }

    #[test]
    fn user_prompt_submit() {
        let input = json!({
            "cwd": "/tmp",
            "prompt": "hello",
            "session_id": "sess-kimi-2",
        });
        let event = KimiAdapter.parse("user-prompt-submit", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::UserPromptSubmit {
                agent: KIMI_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                prompt: "hello".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-kimi-2".into()),
                turn_id: None,
            }
        );
    }

    /// Real payload captured from Kimi Code CLI 0.41.0 (2026-09-15):
    /// `prompt` arrives as an array of content parts, not a string.
    #[test]
    fn user_prompt_submit_real_payload_array_prompt() {
        let input = json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "session_a557b149-c6a3-4ca8-8f8a-e5c0645ecb8c",
            "cwd": "/Users/ghot/repo/archives/tmux-agent-sidebar",
            "client_type": "kimi_code_cli",
            "session_title": "只回复 ok",
            "prompt": [{"type": "text", "text": "只回复 ok"}],
            "is_steer": false
        });
        let event = KimiAdapter.parse("user-prompt-submit", &input).unwrap();
        match event {
            AgentEvent::UserPromptSubmit {
                prompt, session_id, ..
            } => {
                assert_eq!(prompt, "只回复 ok");
                assert_eq!(
                    session_id.as_deref(),
                    Some("session_a557b149-c6a3-4ca8-8f8a-e5c0645ecb8c")
                );
            }
            other => panic!("expected UserPromptSubmit, got {:?}", other),
        }
    }

    /// Real payload captured from Kimi Code CLI 0.41.0 (2026-09-15): Stop
    /// carries no last-message field at all — only `stop_hook_active`.
    #[test]
    fn stop_real_payload_has_no_message_field() {
        let input = json!({
            "hook_event_name": "Stop",
            "session_id": "session_a557b149-c6a3-4ca8-8f8a-e5c0645ecb8c",
            "cwd": "/Users/ghot/repo/archives/tmux-agent-sidebar",
            "client_type": "kimi_code_cli",
            "session_title": "只回复 ok",
            "stop_hook_active": false
        });
        let event = KimiAdapter.parse("stop", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::Stop {
                agent: KIMI_AGENT.into(),
                cwd: "/Users/ghot/repo/archives/tmux-agent-sidebar".into(),
                permission_mode: "".into(),
                last_message: "".into(),
                response: None,
                worktree: None,
                agent_id: None,
                session_id: Some("session_a557b149-c6a3-4ca8-8f8a-e5c0645ecb8c".into()),
                turn_id: None,
            }
        );
    }

    /// Regression guard: Kimi appends hook stdout to the model context on
    /// exit 0, so the Stop event must not echo a response the way Codex
    /// does (`{"continue":true}`). Kimi's allow semantics are exit-code
    /// only.
    #[test]
    fn stop_has_no_response() {
        let input = json!({
            "cwd": "/tmp",
            "last_message": "done",
            "session_id": "sess-kimi-3",
        });
        let event = KimiAdapter.parse("stop", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::Stop {
                agent: KIMI_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                last_message: "done".into(),
                response: None,
                worktree: None,
                agent_id: None,
                session_id: Some("sess-kimi-3".into()),
                turn_id: None,
            }
        );
    }

    #[test]
    fn stop_accepts_claude_style_last_message_key() {
        let event = KimiAdapter
            .parse("stop", &json!({"last_assistant_message": "wrapped up"}))
            .unwrap();
        match event {
            AgentEvent::Stop { last_message, .. } => assert_eq!(last_message, "wrapped up"),
            other => panic!("expected Stop, got {:?}", other),
        }
    }

    #[test]
    fn stop_failure() {
        let input = json!({
            "cwd": "/tmp",
            "error": "boom",
            "session_id": "sess-kimi-4",
        });
        let event = KimiAdapter.parse("stop-failure", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::StopFailure {
                agent: KIMI_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                error: "boom".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-kimi-4".into()),
            }
        );
    }

    #[test]
    fn activity_log() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "ls -la"},
            "tool_response": {"stdout": "file.txt\n"},
            "session_id": "sess-kimi-5",
        });
        let event = KimiAdapter.parse("activity-log", &input).unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                tool_response,
                session_id,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "ls -la");
                assert_eq!(tool_response["stdout"], "file.txt\n");
                assert_eq!(session_id.as_deref(), Some("sess-kimi-5"));
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_empty_tool_name_rejected() {
        assert!(KimiAdapter.parse("activity-log", &json!({})).is_none());
    }

    #[test]
    fn notification() {
        let input = json!({
            "cwd": "/tmp",
            "wait_reason": "permission",
            "session_id": "sess-kimi-6",
        });
        let event = KimiAdapter.parse("notification", &input).unwrap();
        match event {
            AgentEvent::Notification {
                agent,
                wait_reason,
                meta_only,
                session_id,
                ..
            } => {
                assert_eq!(agent, KIMI_AGENT);
                assert_eq!(wait_reason, "permission");
                assert!(!meta_only);
                assert_eq!(session_id.as_deref(), Some("sess-kimi-6"));
            }
            other => panic!("expected Notification, got {:?}", other),
        }
    }

    #[test]
    fn subagent_start() {
        let event = KimiAdapter
            .parse("subagent-start", &json!({"agent_type": "Explore"}))
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SubagentStart {
                agent_type: "Explore".into(),
                agent_id: None,
            }
        );
    }

    #[test]
    fn subagent_events_tolerate_alternate_type_keys() {
        for key in ["subagent_name", "name"] {
            let input = json!({key: "Explore"});
            let event = KimiAdapter.parse("subagent-start", &input).unwrap();
            match event {
                AgentEvent::SubagentStart { agent_type, .. } => {
                    assert_eq!(agent_type, "Explore", "key {key}")
                }
                other => panic!("expected SubagentStart, got {:?}", other),
            }
        }
    }

    #[test]
    fn subagent_start_empty_type_rejected() {
        assert!(KimiAdapter.parse("subagent-start", &json!({})).is_none());
    }

    #[test]
    fn subagent_stop() {
        let input = json!({
            "agent_type": "Plan",
            "agent_id": "sub-7",
            "last_message": "Found it",
        });
        let event = KimiAdapter.parse("subagent-stop", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::SubagentStop {
                agent_type: "Plan".into(),
                agent_id: Some("sub-7".into()),
                last_message: "Found it".into(),
                transcript_path: "".into(),
            }
        );
    }

    #[test]
    fn permission_request() {
        let input = json!({
            "hook_event_name": "PermissionRequest",
            "cwd": "/tmp/project",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf build"},
            "session_id": "sess-kimi-perm",
            "turn_id": "turn-7",
        });
        let event = KimiAdapter.parse("permission-request", &input).unwrap();
        match event {
            AgentEvent::PermissionRequest {
                agent,
                cwd,
                permission_mode,
                tool_name,
                tool_input,
                session_id,
                turn_id,
                ..
            } => {
                assert_eq!(agent, KIMI_AGENT);
                assert_eq!(cwd, "/tmp/project");
                assert_eq!(permission_mode, "");
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "rm -rf build");
                assert_eq!(session_id.as_deref(), Some("sess-kimi-perm"));
                assert_eq!(turn_id.as_deref(), Some("turn-7"));
            }
            other => panic!("expected PermissionRequest, got {:?}", other),
        }
    }

    /// `Interrupt` maps to the dedicated Interrupt event — an abort is not
    /// a completion and must not flow through the stop path.
    #[test]
    fn interrupt() {
        let input = json!({
            "hook_event_name": "Interrupt",
            "session_id": "sess-kimi-int",
            "cwd": "/tmp",
            "reason": "user_escape"
        });
        let event = KimiAdapter.parse("interrupt", &input).unwrap();
        match event {
            AgentEvent::Interrupt {
                agent, session_id, ..
            } => {
                assert_eq!(agent, KIMI_AGENT);
                assert_eq!(session_id.as_deref(), Some("sess-kimi-int"));
            }
            other => panic!("expected Interrupt, got {:?}", other),
        }
    }

    /// PostToolUseFailure maps to the dedicated ToolFailure event, keeping
    /// the error string for the failure marker.
    #[test]
    fn tool_failure() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "make test"},
            "error": "exit code 2",
            "session_id": "sess-kimi-fail",
        });
        let event = KimiAdapter.parse("tool-failure", &input).unwrap();
        match event {
            AgentEvent::ToolFailure {
                tool_name,
                tool_input,
                error,
                session_id,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "make test");
                assert_eq!(error, "exit code 2");
                assert_eq!(session_id.as_deref(), Some("sess-kimi-fail"));
            }
            other => panic!("expected ToolFailure, got {:?}", other),
        }
    }

    #[test]
    fn tool_failure_empty_tool_name_rejected() {
        assert!(KimiAdapter.parse("tool-failure", &json!({})).is_none());
    }

    #[test]
    fn permission_result() {
        let input = json!({
            "hook_event_name": "PermissionResult",
            "cwd": "/tmp/project",
            "session_id": "sess-kimi-pr",
            "turn_id": "turn-9",
        });
        let event = KimiAdapter.parse("permission-result", &input).unwrap();
        match event {
            AgentEvent::PermissionResult {
                agent,
                cwd,
                session_id,
                turn_id,
                ..
            } => {
                assert_eq!(agent, KIMI_AGENT);
                assert_eq!(cwd, "/tmp/project");
                assert_eq!(session_id.as_deref(), Some("sess-kimi-pr"));
                assert_eq!(turn_id.as_deref(), Some("turn-9"));
            }
            other => panic!("expected PermissionResult, got {:?}", other),
        }
    }

    /// Kimi's TaskStarted payload uses `description` for the subject.
    #[test]
    fn task_started_maps_description_to_task_subject() {
        let input = json!({
            "hook_event_name": "TaskStarted",
            "task_id": "task-1",
            "description": "run the test suite",
            "detached": true,
        });
        let event = KimiAdapter.parse("task-created", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::TaskCreated {
                task_id: "task-1".into(),
                task_subject: "run the test suite".into(),
            }
        );
    }

    #[test]
    fn unsupported_events_return_none() {
        for event in [
            "permission-denied",
            "cwd-changed",
            "task-completed",
            "teammate-idle",
            "worktree-create",
            "worktree-remove",
            "something-else",
        ] {
            assert!(
                KimiAdapter.parse(event, &json!({})).is_none(),
                "{event} should not be supported"
            );
        }
    }

    /// Parse an activity-log payload and return the tool name the activity log
    /// records plus the label the block renders.
    fn parsed_label(input: &Value) -> (String, String) {
        let event = KimiAdapter.parse("activity-log", input).unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                tool_response,
                ..
            } => {
                // Production passes the response too; dropping it here would
                // leave every response-reading branch untested.
                let label =
                    crate::cli::label::extract_tool_label(&tool_name, &tool_input, &tool_response);
                (tool_name, label)
            }
            other => panic!("expected ActivityLog, got {other:?}"),
        }
    }

    /// Kimi reports the file path as `path` and names four tools differently
    /// from Claude Code (`FetchURL`, `ReadMediaFile`, `TodoList`,
    /// `AgentSwarm`). Every case here was a blank row in a live Kimi pane.
    #[test]
    fn activity_log_labels_kimi_tool_arguments() {
        let cases = [
            (
                json!({"tool_name": "Read", "tool_input": {"path": "docs/spec/STATUS.md", "line_offset": 1, "n_lines": 40}}),
                "Read",
                "STATUS.md",
            ),
            (
                json!({"tool_name": "Write", "tool_input": {"path": "/repo/src/generated.ts"}}),
                "Write",
                "generated.ts",
            ),
            (
                json!({"tool_name": "Edit", "tool_input": {"path": "/repo/src/main.rs", "old_string": "a", "new_string": "b"}}),
                "Edit",
                "main.rs",
            ),
            (
                json!({"tool_name": "FetchURL", "tool_input": {"url": "https://example.com/docs"}}),
                "WebFetch",
                "example.com/docs",
            ),
            (
                json!({"tool_name": "ReadMediaFile", "tool_input": {"path": "/tmp/chart.png"}}),
                "Read",
                "chart.png",
            ),
            // Kimi's items are `{status, title}` (observed in its session
            // logs); an item shape nobody has seen still yields the count.
            (
                json!({"tool_name": "TodoList", "tool_input": {"todos": [{"status": "in_progress", "title": "Injection-revert experiments"}]}}),
                "TodoWrite",
                "1 task · Injection-revert experiments",
            ),
            (
                json!({"tool_name": "TodoList", "tool_input": {"todos": [{"id": "1"}, {"id": "2"}]}}),
                "TodoWrite",
                "2 tasks",
            ),
            (
                json!({"tool_name": "AgentSwarm", "tool_input": {"description": "Fan out the audit"}}),
                "Agent",
                "Fan out the audit",
            ),
            (
                json!({"tool_name": "EnterPlanMode", "tool_input": {}}),
                "EnterPlanMode",
                "",
            ),
        ];
        for (input, want_tool, want_label) in cases {
            let (tool, label) = parsed_label(&input);
            assert_eq!(tool, want_tool, "for {input}");
            assert_eq!(label, want_label, "for {input}");
        }
    }

    /// Kimi's Bash command keeps its own label, and the background flag drives
    /// `@pane_bg_cmd` — that path depends on the key surviving normalisation.
    #[test]
    fn activity_log_keeps_bash_command_and_background_flag() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "npm run verify", "run_in_background": true},
        });
        let event = KimiAdapter.parse("activity-log", &input).unwrap();
        match event {
            AgentEvent::ActivityLog { tool_input, .. } => {
                assert_eq!(tool_input["command"], "npm run verify");
                assert_eq!(tool_input["run_in_background"], true);
            }
            other => panic!("expected ActivityLog, got {other:?}"),
        }
    }

    #[test]
    fn tool_failure_normalises_the_same_way() {
        let input = json!({
            "tool_name": "Read",
            "tool_input": {"path": "/repo/secret.env"},
            "error": "permission denied",
        });
        let event = KimiAdapter.parse("tool-failure", &input).unwrap();
        match event {
            AgentEvent::ToolFailure {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(tool_input["file_path"], "/repo/secret.env");
            }
            other => panic!("expected ToolFailure, got {other:?}"),
        }
    }
}
