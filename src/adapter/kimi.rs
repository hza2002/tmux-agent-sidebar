use crate::event::{AgentEvent, AgentEventKind, EventAdapter};
use crate::tmux::KIMI_AGENT;
use serde_json::Value;

use super::{HookRegistration, json_str, json_value_or_null, optional_str};

pub struct KimiAdapter;

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
                let tool_name = json_str(input, "tool_name");
                if tool_name.is_empty() {
                    return None;
                }
                Some(AgentEvent::ActivityLog {
                    tool_name: tool_name.into(),
                    tool_input: json_value_or_null(input, "tool_input"),
                    tool_response: json_value_or_null(input, "tool_response"),
                    session_id: optional_str(input, "session_id"),
                    turn_id: optional_str(input, "turn_id"),
                })
            }
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
            "permission-request" => Some(AgentEvent::PermissionRequest {
                agent: KIMI_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                tool_name: json_str(input, "tool_name").into(),
                tool_input: json_value_or_null(input, "tool_input"),
                agent_id: optional_str(input, "agent_id"),
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
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

    #[test]
    fn unsupported_events_return_none() {
        for event in [
            "permission-denied",
            "cwd-changed",
            "task-created",
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
}
