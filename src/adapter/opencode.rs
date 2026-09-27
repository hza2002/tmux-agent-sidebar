use serde_json::Value;

use crate::event::{AgentEvent, EventAdapter};
use crate::tmux::OPENCODE_AGENT;
use crate::tool_name::CanonicalTool;

use super::{
    alias_keys, canonical_tool_name, json_str, json_value_or_null, optional_str, tool_input_value,
};

pub struct OpenCodeAdapter;

/// OpenCode tool IDs are lowercase (`bash`, `read`, …) but the internal
/// vocabulary is Claude-style PascalCase. Normalise here so the activity log,
/// its label strategy table, and the colour classifier share one vocabulary
/// across agents.
const TOOL_ALIASES: &[(&str, CanonicalTool)] = &[
    ("bash", CanonicalTool::Bash),
    ("read", CanonicalTool::Read),
    ("write", CanonicalTool::Write),
    ("edit", CanonicalTool::Edit),
    ("multiedit", CanonicalTool::Edit),
    ("glob", CanonicalTool::Glob),
    ("grep", CanonicalTool::Grep),
    ("webfetch", CanonicalTool::WebFetch),
    ("websearch", CanonicalTool::WebSearch),
    ("task", CanonicalTool::Agent),
    ("skill", CanonicalTool::Skill),
    ("lsp", CanonicalTool::Lsp),
    ("todowrite", CanonicalTool::TodoWrite),
    // `question` is OpenCode's user prompt; `list` was folded into `read`
    // (a directory read); `patch` was renamed `apply_patch`; `plan_exit` is
    // the plan-mode hand-off codex calls `ExitPlanMode`.
    ("question", CanonicalTool::AskUserQuestion),
    ("list", CanonicalTool::Read),
    ("patch", CanonicalTool::Patch),
    ("apply_patch", CanonicalTool::Patch),
    ("plan_exit", CanonicalTool::ExitPlanMode),
];

/// Argument-key aliases, keyed by the canonical tool name. `path` is included
/// next to `filePath` because the directory-listing form of a read reports the
/// directory under `path`, not `filePath`.
fn tool_arg_aliases(tool_name: &str) -> &'static [(&'static str, &'static str)] {
    // Keyed on the parsed variant, not on the name's spelling: a renamed
    // canonical spelling round-trips through `from_name` and still aliases.
    match CanonicalTool::from_name(tool_name) {
        Some(CanonicalTool::Read | CanonicalTool::Write | CanonicalTool::Edit) => {
            &[("filePath", "file_path"), ("path", "file_path")]
        }
        Some(CanonicalTool::Skill) => &[("name", "skill")],
        _ => &[],
    }
}

impl EventAdapter for OpenCodeAdapter {
    fn parse(&self, event_name: &str, input: &Value) -> Option<AgentEvent> {
        match event_name {
            "session-start" => Some(AgentEvent::SessionStart {
                agent: OPENCODE_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                source: json_str(input, "source").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "user-prompt-submit" => Some(AgentEvent::UserPromptSubmit {
                agent: OPENCODE_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                prompt: json_str(input, "prompt").into(),
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: None,
            }),
            "notification" => Some(AgentEvent::Notification {
                agent: OPENCODE_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                wait_reason: json_str(input, "wait_reason").into(),
                meta_only: false,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
            }),
            "stop" => Some(AgentEvent::Stop {
                agent: OPENCODE_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                last_message: json_str(input, "last_message").into(),
                response: None,
                worktree: None,
                agent_id: None,
                session_id: optional_str(input, "session_id"),
                turn_id: None,
            }),
            "stop-failure" => Some(AgentEvent::StopFailure {
                agent: OPENCODE_AGENT.into(),
                cwd: json_str(input, "cwd").into(),
                permission_mode: String::new(),
                error: json_str(input, "error").into(),
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
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_start() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "session-start",
                &json!({"cwd": "/tmp", "session_id": "ses-1", "source": "startup"}),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::SessionStart {
                agent: OPENCODE_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                source: "startup".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("ses-1".into()),
            }
        );
    }

    #[test]
    fn user_prompt_submit() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "user-prompt-submit",
                &json!({"cwd": "/tmp", "prompt": "hello"}),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::UserPromptSubmit {
                agent: OPENCODE_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                prompt: "hello".into(),
                worktree: None,
                agent_id: None,
                session_id: None,
                turn_id: None,
            }
        );
    }

    #[test]
    fn activity_log_normalizes_lowercase_bash() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "bash",
                    "tool_input": {"command": "ls"},
                    "tool_response": {"stdout": "file.txt"}
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                tool_response,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "ls");
                assert_eq!(tool_response["stdout"], "file.txt");
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_preserves_bash_background_flag() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "bash",
                    "tool_input": {
                        "command": "npm run dev",
                        "runInBackground": true
                    }
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_input["command"], "npm run dev");
                assert_eq!(tool_input["runInBackground"], true);
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_normalizes_read_filepath_key() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "read",
                    "tool_input": {"filePath": "/home/user/src/main.rs"}
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(tool_input["file_path"], "/home/user/src/main.rs");
                assert_eq!(tool_input["filePath"], "/home/user/src/main.rs");
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_unknown_tool_passes_through() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "custom-mcp-tool",
                    "tool_input": {"foo": "bar"}
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog { tool_name, .. } => {
                assert_eq!(tool_name, "custom-mcp-tool");
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn activity_log_multiedit_maps_to_edit() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "activity-log",
                &json!({
                    "tool_name": "multiedit",
                    "tool_input": {"filePath": "/a/b.rs"}
                }),
            )
            .unwrap();
        match event {
            AgentEvent::ActivityLog {
                tool_name,
                tool_input,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(tool_input["file_path"], "/a/b.rs");
            }
            other => panic!("expected ActivityLog, got {:?}", other),
        }
    }

    #[test]
    fn stop_failure() {
        let adapter = OpenCodeAdapter;
        let event = adapter
            .parse(
                "stop-failure",
                &json!({"cwd": "/tmp", "error": "boom", "session_id": "ses-1"}),
            )
            .unwrap();
        assert_eq!(
            event,
            AgentEvent::StopFailure {
                agent: OPENCODE_AGENT.into(),
                cwd: "/tmp".into(),
                permission_mode: "".into(),
                error: "boom".into(),
                worktree: None,
                agent_id: None,
                session_id: Some("ses-1".into()),
            }
        );
    }

    /// Parse an activity-log payload and return the tool name the activity log
    /// records plus the label the block renders.
    fn parsed_label(input: &Value) -> (String, String) {
        let event = OpenCodeAdapter.parse("activity-log", input).unwrap();
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

    /// The bridge passes OpenCode's lowercase ids and raw `args` through, so
    /// every mapping lives in this adapter. Each case below was unmapped or
    /// mis-keyed before.
    #[test]
    fn activity_log_labels_opencode_tool_arguments() {
        let cases = [
            (
                json!({"tool_name": "question", "tool_input": {"questions": [{"question": "Which database?"}]}}),
                "AskUserQuestion",
                "Which database?",
            ),
            (
                json!({"tool_name": "skill", "tool_input": {"name": "commit"}}),
                "Skill",
                "commit",
            ),
            (
                json!({"tool_name": "patch", "tool_input": {"patchText": "*** Update File: src/ui.rs\n@@\n"}}),
                "Patch",
                "ui.rs",
            ),
            (
                json!({"tool_name": "apply_patch", "tool_input": {"patchText": "*** Add File: docs/new.md\n+x\n"}}),
                "Patch",
                "new.md",
            ),
            (
                json!({"tool_name": "list", "tool_input": {"path": "/repo/src", "ignore": []}}),
                "Read",
                "src",
            ),
            (
                json!({"tool_name": "todowrite", "tool_input": {"todos": [{"content": "Ship it", "status": "in_progress"}]}}),
                "TodoWrite",
                "1 task · Ship it",
            ),
            (
                json!({"tool_name": "plan_exit", "tool_input": {}}),
                "ExitPlanMode",
                "",
            ),
        ];
        for (input, want_tool, want_label) in cases {
            let (tool, label) = parsed_label(&input);
            assert_eq!(tool, want_tool, "for {input}");
            assert_eq!(label, want_label, "for {input}");
        }
    }

    #[test]
    fn every_alias_target_is_in_the_canonical_vocabulary() {
        super::super::assert_aliases_are_canonical("opencode", TOOL_ALIASES);
    }

    /// `task` is the subagent call: its `description` is the label. The
    /// legacy `content[].text` case is the discriminating one — a
    /// response-first label would show the subagent's report instead.
    #[test]
    fn activity_log_labels_task_from_its_description() {
        let envelope = json!({
            "tool_name": "task",
            "tool_input": {"description": "Audit the adapter", "prompt": "long prompt"},
            "tool_response": {"title": "Audited", "output": "the full answer", "metadata": null},
        });
        let (tool, label) = parsed_label(&envelope);
        assert_eq!(tool, "Agent");
        assert_eq!(label, "Audit the adapter");

        let legacy = json!({
            "tool_name": "task",
            "tool_input": {"description": "Audit the adapter", "prompt": "long prompt"},
            "tool_response": {"content": [{"type": "text", "text": "the subagent's report"}]},
        });
        assert_eq!(parsed_label(&legacy).1, "Audit the adapter");
    }
}
