use crate::event::{AgentEvent, AgentEventKind, EventAdapter};
use crate::tmux::CODEX_AGENT;
use crate::tool_name::CanonicalTool;
use serde_json::Value;

use super::{
    HookRegistration, alias_keys, canonical_tool_name, json_str, json_value_or_null, optional_str,
    tool_input_value,
};

pub struct CodexAdapter;

/// Codex reports a mix of names: Claude-style ones for the shell
/// (`exec_command` is rewritten to `Bash` with `cmd` → `command` before the
/// hook fires) and its own snake_case names for everything else. The rest are
/// mapped here so the label strategy table and the colour classifier see the
/// same vocabulary they see from every other agent.
const TOOL_ALIASES: &[(&str, CanonicalTool)] = &[
    // Insurance: the rewrite to `Bash` is Codex's, not ours, so keep the raw
    // name working if a call reaches us unrewritten.
    ("exec_command", CanonicalTool::Bash),
    ("apply_patch", CanonicalTool::Patch),
    ("webrun", CanonicalTool::WebSearch),
    ("view_image", CanonicalTool::Read),
    ("request_user_input_async", CanonicalTool::AskUserQuestion),
];

fn tool_arg_aliases(tool_name: &str) -> &'static [(&'static str, &'static str)] {
    // Keyed on the parsed variant, not on the name's spelling: a renamed
    // canonical spelling round-trips through `from_name` and still aliases.
    match CanonicalTool::from_name(tool_name) {
        Some(CanonicalTool::Bash) => &[("cmd", "command")],
        Some(CanonicalTool::Read) => &[("path", "file_path")],
        _ => &[],
    }
}

impl CodexAdapter {
    /// Single source of truth for Codex CLI hook wiring. Verified against
    /// Codex CLI's official hook event enum in
    /// `openai/codex:codex-rs/hooks/src/engine/config.rs`, which currently
    /// defines `PermissionRequest` alongside the lifecycle and tool events.
    ///
    /// Caveats:
    /// - `PostToolUse` fires for every tool: the installed `hooks.json` uses an
    ///   empty matcher and real payloads carry `Bash`, `apply_patch`, `webrun`,
    ///   and `request_user_input_async`. `tool_input` is untyped, so each tool's
    ///   argument spelling is Codex's own — see [`TOOL_ALIASES`].
    /// - `PreToolUse` is supported by Codex but not yet wired.
    pub const HOOK_REGISTRATIONS: &'static [HookRegistration] = &[
        HookRegistration {
            trigger: "SessionStart",
            matcher: Some("startup|resume"),
            kind: AgentEventKind::SessionStart,
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
            trigger: "PermissionRequest",
            matcher: None,
            kind: AgentEventKind::PermissionRequest,
        },
        HookRegistration {
            trigger: "PostToolUse",
            matcher: None,
            kind: AgentEventKind::ActivityLog,
        },
    ];
}

impl EventAdapter for CodexAdapter {
    fn parse(&self, event_name: &str, input: &Value) -> Option<AgentEvent> {
        match event_name {
            "session-start" => Some(AgentEvent::SessionStart {
                agent: CODEX_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: json_str(input, "permission_mode").into(),
                source: json_str(input, "source").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "user-prompt-submit" => Some(AgentEvent::UserPromptSubmit {
                agent: CODEX_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: json_str(input, "permission_mode").into(),
                prompt: json_str(input, "prompt").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            "stop" => Some(AgentEvent::Stop {
                agent: CODEX_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: json_str(input, "permission_mode").into(),
                last_message: json_str(input, "last_assistant_message").into(),
                response: Some("{\"continue\":true}".into()),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: optional_str(input, "turn_id"),
            }),
            "permission-request" => {
                let tool_name = canonical_tool_name(TOOL_ALIASES, json_str(input, "tool_name"));
                let tool_input = alias_keys(
                    tool_input_value(input, "tool_input"),
                    tool_arg_aliases(&tool_name),
                );
                Some(AgentEvent::PermissionRequest {
                    agent: CODEX_AGENT.into(),
                    cwd: json_str(input, "cwd").into(),
                    permission_mode: json_str(input, "permission_mode").into(),
                    tool_name,
                    tool_input,
                    agent_id: optional_str(input, "agent_id"),
                    session_id: optional_str(input, "session_id"),
                    turn_id: optional_str(input, "turn_id"),
                })
            }
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
        super::super::assert_table_drift_free("codex", CodexAdapter::HOOK_REGISTRATIONS);
    }

    #[test]
    fn every_alias_target_is_in_the_canonical_vocabulary() {
        super::super::assert_aliases_are_canonical("codex", TOOL_ALIASES);
    }

    #[test]
    fn session_start() {
        let adapter = CodexAdapter;
        let input = json!({"cwd": "/home/user", "session_id": "sess-codex-1"});
        let event = adapter.parse("session-start", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: CODEX_AGENT.into(),
                cwd: "/home/user".into(),
                permission_mode: "".into(),
                source: "".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-codex-1".into()),
            }
        );
    }

    #[test]
    fn session_end_not_supported() {
        // Codex CLI does not fire SessionEnd (verified against
        // openai/codex:codex-rs/hooks/src/engine/config.rs).
        assert!(CodexAdapter.parse("session-end", &json!({})).is_none());
    }

    #[test]
    fn user_prompt_submit() {
        let adapter = CodexAdapter;
        let input = json!({"cwd": "/tmp", "prompt": "hello", "session_id": "sess-codex-2"});
        let event = adapter.parse("user-prompt-submit", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::UserPromptSubmit {
                agent: CODEX_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                prompt: "hello".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-codex-2".into()),
                turn_id: None,
            }
        );
    }

    #[test]
    fn stop_has_continue_response() {
        let adapter = CodexAdapter;
        let input = json!({
            "cwd": "/tmp",
            "last_assistant_message": "done",
            "session_id": "sess-codex-3",
        });
        let event = adapter.parse("stop", &input).unwrap();
        assert_eq!(
            event,
            AgentEvent::Stop {
                agent: CODEX_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                last_message: "done".into(),
                response: Some("{\"continue\":true}".into()),
                worktree: None,
                agent_id: None,
                session_id: Some("sess-codex-3".into()),
                turn_id: None,
            }
        );
    }

    #[test]
    fn permission_request_extracts_context_and_tool() {
        let adapter = CodexAdapter;
        let input = json!({
            "hook_event_name": "PermissionRequest",
            "cwd": "/tmp/project",
            "permission_mode": "default",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf build"},
            "session_id": "sess-codex-perm",
            "turn_id": "turn-7"
        });
        let event = adapter.parse("permission-request", &input).unwrap();
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
                assert_eq!(agent, CODEX_AGENT);
                assert_eq!(cwd, "/tmp/project");
                assert_eq!(permission_mode, "default");
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "rm -rf build");
                assert_eq!(session_id.as_deref(), Some("sess-codex-perm"));
                assert_eq!(turn_id.as_deref(), Some("turn-7"));
            }
            other => panic!("expected PermissionRequest, got {:?}", other),
        }
    }

    /// Realistic Stop payload matching the upstream Codex hook input schema
    /// (`codex-rs/hooks/schema/generated/stop.command.input.schema.json`),
    /// which declares `session_id` as a required top-level string.
    #[test]
    fn stop_extracts_session_id_from_upstream_schema_payload() {
        let adapter = CodexAdapter;
        let input = json!({
            "hook_event_name": "Stop",
            "cwd": "/home/user/project",
            "session_id": "01HXYZABCDEF0123456789",
            "model": "gpt-5-codex",
            "permission_mode": "default",
            "last_assistant_message": "all tests pass",
            "stop_hook_active": false,
            "transcript_path": null,
            "turn_id": "turn-42",
        });
        let event = adapter.parse("stop", &input).unwrap();
        match event {
            AgentEvent::Stop {
                session_id,
                turn_id,
                permission_mode,
                last_message,
                ..
            } => {
                assert_eq!(session_id.as_deref(), Some("01HXYZABCDEF0123456789"));
                assert_eq!(turn_id.as_deref(), Some("turn-42"));
                assert_eq!(permission_mode, "default");
                assert_eq!(last_message, "all tests pass");
            }
            other => panic!("expected Stop, got {:?}", other),
        }
    }

    #[test]
    fn notification_not_supported() {
        assert!(CodexAdapter.parse("notification", &json!({})).is_none());
    }

    #[test]
    fn stop_failure_not_supported() {
        assert!(CodexAdapter.parse("stop-failure", &json!({})).is_none());
    }

    #[test]
    fn subagent_start_not_supported() {
        assert!(CodexAdapter.parse("subagent-start", &json!({})).is_none());
    }

    #[test]
    fn activity_log_bash_command() {
        let adapter = CodexAdapter;
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "ls -la"},
            "tool_response": {"stdout": "file.txt\n"}
        });
        let event = adapter.parse("activity-log", &input).unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(
                    tool_input.get("command").and_then(|v| v.as_str()),
                    Some("ls -la")
                );
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_empty_tool_name_rejected() {
        assert!(CodexAdapter.parse("activity-log", &json!({})).is_none());
    }

    #[test]
    fn unknown_event_ignored() {
        assert!(CodexAdapter.parse("something-else", &json!({})).is_none());
    }

    /// Defensive path: upstream schema requires `session_id`, but the adapter
    /// must not panic if a malformed payload omits it. Parse returns the event
    /// with `session_id: None` and downstream handlers treat it as missing.
    #[test]
    fn stop_without_session_id_falls_back_to_none() {
        let adapter = CodexAdapter;
        let event = adapter.parse("stop", &json!({})).unwrap();
        assert_eq!(
            event,
            AgentEvent::Stop {
                agent: "codex".into(),
                cwd: "".into(),
                permission_mode: "".into(),
                last_message: "".into(),
                response: Some("{\"continue\":true}".into()),
                worktree: None,
                agent_id: None,
                session_id: None,
                turn_id: None,
            }
        );
    }

    #[test]
    fn subagent_stop_not_supported() {
        assert!(CodexAdapter.parse("subagent-stop", &json!({})).is_none());
    }

    #[test]
    fn permission_denied_not_supported() {
        assert!(
            CodexAdapter
                .parse("permission-denied", &json!({}))
                .is_none()
        );
    }

    #[test]
    fn cwd_changed_not_supported() {
        assert!(CodexAdapter.parse("cwd-changed", &json!({})).is_none());
    }

    #[test]
    fn session_start_has_no_worktree() {
        let event = CodexAdapter
            .parse("session-start", &json!({"cwd": "/tmp"}))
            .unwrap();
        match event {
            AgentEvent::SessionStart {
                worktree, agent_id, ..
            } => {
                assert!(worktree.is_none());
                assert!(agent_id.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn session_start_captures_source() {
        let event = CodexAdapter
            .parse("session-start", &json!({"cwd": "/tmp", "source": "resume"}))
            .unwrap();
        match event {
            AgentEvent::SessionStart { source, .. } => assert_eq!(source, "resume"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn task_created_not_supported() {
        assert!(CodexAdapter.parse("task-created", &json!({})).is_none());
    }

    #[test]
    fn task_completed_not_supported() {
        assert!(CodexAdapter.parse("task-completed", &json!({})).is_none());
    }

    #[test]
    fn teammate_idle_not_supported() {
        assert!(CodexAdapter.parse("teammate-idle", &json!({})).is_none());
    }

    #[test]
    fn worktree_create_not_supported() {
        assert!(CodexAdapter.parse("worktree-create", &json!({})).is_none());
    }

    #[test]
    fn worktree_remove_not_supported() {
        assert!(CodexAdapter.parse("worktree-remove", &json!({})).is_none());
    }

    #[test]
    fn session_start_missing_fields_default_to_empty() {
        let adapter = CodexAdapter;
        let event = adapter.parse("session-start", &json!({})).unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: "codex".into(),
                cwd: "".into(),
                permission_mode: "".into(),
                source: "".into(),
                worktree: None,
                agent_id: None,
                session_id: None,
            }
        );
    }

    /// Parse an activity-log payload and return the tool name the activity log
    /// records plus the label the block renders.
    fn parsed_label(input: &Value) -> (String, String) {
        let event = CodexAdapter.parse("activity-log", input).unwrap();
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

    /// Codex reports its own snake_case tool names and argument spellings.
    /// `apply_patch`, `webrun`, and `request_user_input_async` are the three
    /// whose arguments were watched arriving blank in the live Codex pane (35
    /// of 134 entries). `view_image` and `exec_command` are modelled from the
    /// tool schemas in the installed CLI — neither was observed firing, so
    /// their hook argument spellings stay unconfirmed.
    #[test]
    fn activity_log_labels_codex_tool_arguments() {
        let patch = json!(
            "*** Begin Patch\n*** Update File: /repo/docs/spec.md\n@@\n-old\n+new\n*** End Patch"
        );
        let cases = [
            (
                json!({"tool_name": "apply_patch", "tool_input": patch}),
                "Patch",
                "spec.md",
            ),
            (
                json!({"tool_name": "apply_patch", "tool_input": {"changes": [{"path": "/repo/src/main.rs"}]}}),
                "Patch",
                "main.rs",
            ),
            (
                json!({"tool_name": "webrun", "tool_input": {"query": "rust lsp setup"}}),
                "WebSearch",
                "rust lsp setup",
            ),
            (
                json!({"tool_name": "webrun", "tool_input": {"action": {"queries": ["rust lsp setup"]}}}),
                "WebSearch",
                "rust lsp setup",
            ),
            (
                json!({"tool_name": "request_user_input_async", "tool_input": {"questions": [{"title": "Which database?"}]}}),
                "AskUserQuestion",
                "Which database?",
            ),
            (
                json!({"tool_name": "view_image", "tool_input": {"path": "/tmp/screenshot.png", "detail": "high"}}),
                "Read",
                "screenshot.png",
            ),
            (
                json!({"tool_name": "exec_command", "tool_input": {"cmd": "git status --short"}}),
                "Bash",
                "git status --short",
            ),
            (
                json!({"tool_name": "Bash", "tool_input": {"command": "ls -la"}}),
                "Bash",
                "ls -la",
            ),
        ];
        for (input, want_tool, want_label) in cases {
            let (tool, label) = parsed_label(&input);
            assert_eq!(tool, want_tool, "for {input}");
            assert_eq!(label, want_label, "for {input}");
        }
    }

    /// The shell rewrite Codex performs itself — `exec_command` → `Bash`,
    /// `cmd` → `command` — must survive the adapter's own normalisation.
    #[test]
    fn activity_log_keeps_codex_bash_rewrite() {
        let input = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test", "workdir": "/repo"},
        });
        let event = CodexAdapter.parse("activity-log", &input).unwrap();
        match event {
            AgentEvent::ActivityLog { tool_input, .. } => {
                assert_eq!(tool_input["command"], "cargo test");
            }
            other => panic!("expected ActivityLog, got {other:?}"),
        }
    }

    #[test]
    fn permission_request_normalises_the_tool() {
        let input = json!({
            "tool_name": "apply_patch",
            "tool_input": {"patchText": "*** Add File: /repo/new.md\n+x\n"},
            "session_id": "sess-codex-perm",
        });
        let event = CodexAdapter.parse("permission-request", &input).unwrap();
        match event {
            AgentEvent::PermissionRequest {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Patch");
                assert_eq!(tool_input["patchText"], "*** Add File: /repo/new.md\n+x\n");
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }
}
