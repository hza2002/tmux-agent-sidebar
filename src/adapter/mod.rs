pub mod claude;
pub mod codex;
pub mod kimi;
pub mod opencode;

use crate::event::AgentEventKind;
use crate::tool_name::CanonicalTool;

pub(crate) fn json_str<'a>(val: &'a serde_json::Value, key: &str) -> &'a str {
    val.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

pub(crate) fn optional_str(val: &serde_json::Value, key: &str) -> Option<String> {
    let s = json_str(val, key);
    if s.is_empty() { None } else { Some(s.into()) }
}

pub(crate) fn json_value_or_null(val: &serde_json::Value, key: &str) -> serde_json::Value {
    val.get(key).cloned().unwrap_or(serde_json::Value::Null)
}

/// Translate an agent-native tool name into the canonical vocabulary the label
/// strategy table, the colour classifier, and the plan-mode badge all key off.
/// Unknown names pass through unchanged so the activity log still shows what
/// the agent actually called instead of a silently empty tool column.
///
/// Adapters call this at the boundary; nothing downstream re-maps tool names.
pub(crate) fn canonical_tool_name(aliases: &[(&str, CanonicalTool)], raw: &str) -> String {
    aliases
        .iter()
        .find(|(native, _)| *native == raw)
        .map(|(_, canonical)| canonical.as_str().to_string())
        .unwrap_or_else(|| raw.to_string())
}

/// Add `(from, to)` argument-key aliases to a tool-input object. Originals are
/// kept alongside the added keys so a consumer that wants the raw payload still
/// sees it. A destination that already holds a non-empty string wins; an empty
/// or null one does not block the alias, so a payload carrying both spellings
/// still produces a label.
pub(crate) fn alias_keys(input: serde_json::Value, pairs: &[(&str, &str)]) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = input else {
        return input;
    };
    for (src, dst) in pairs {
        let taken = map
            .get(*dst)
            .and_then(|value| value.as_str())
            .is_some_and(|text| !text.is_empty());
        if taken {
            continue;
        }
        if let Some(value) = map.get(*src).cloned() {
            map.insert((*dst).to_string(), value);
        }
    }
    serde_json::Value::Object(map)
}

/// Read a tool-payload field that may arrive as an object or as a stringified
/// JSON object. Claude Code sends both shapes; the other adapters pass whatever
/// their agent sends. Only a string that *looks* like an object literal is
/// parsed, so a raw command (`true`, `123`, `ls -la`) or a patch document is
/// returned untouched — those are read as text by the label extractors.
pub(crate) fn tool_input_value(input: &serde_json::Value, field: &str) -> serde_json::Value {
    match input.get(field) {
        Some(serde_json::Value::String(text)) if text.trim_start().starts_with('{') => {
            serde_json::from_str(text)
                .ok()
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| serde_json::Value::String(text.clone()))
        }
        Some(value) => value.clone(),
        None => serde_json::Value::Null,
    }
}

/// Binding between an upstream agent-side hook trigger (as it appears in the
/// agent's `settings.json`) and the internal `AgentEventKind` the sidebar
/// produces once the hook fires.
///
/// Each adapter exposes its full `HOOK_REGISTRATIONS` table so install
/// wizards, README snippets, setup commands, and docs can all be generated
/// from a single source of truth. The `kind` field is a compile-time enum,
/// not a string, so typos cannot creep in. Drift between the table and the
/// adapter's `parse()` match arms is caught by the tests in `claude.rs` /
/// `codex.rs` via [`assert_table_drift_free`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookRegistration {
    /// Trigger name in the agent's settings.json (e.g. `"SessionStart"`,
    /// `"PostToolUse"`).
    pub trigger: &'static str,
    /// Optional matcher value. `None` means "register with an empty matcher"
    /// (catches all). `Some("startup|resume")` etc. captures a specific filter.
    pub matcher: Option<&'static str>,
    /// Internal event this registration produces.
    pub kind: AgentEventKind,
}

/// Every alias target must be a tool the vocabulary can parse back. A variant
/// missing from `CanonicalTool::ALL` never reaches the strategy table or the
/// colour classifier, so the alias would silently produce a gray, unlabelled
/// row; this turns that into a test failure in the adapter that declares it.
#[cfg(test)]
pub(crate) fn assert_aliases_are_canonical(agent: &str, aliases: &[(&str, CanonicalTool)]) {
    for (native, canonical) in aliases {
        assert_eq!(
            CanonicalTool::from_name(canonical.as_str()),
            Some(*canonical),
            "{agent}: alias {native} → {canonical:?} is not reachable through \
             CanonicalTool::from_name — add it to CanonicalTool::ALL"
        );
    }
}

#[cfg(test)]
pub(crate) fn minimal_payload(kind: AgentEventKind) -> serde_json::Value {
    use serde_json::json;
    match kind {
        AgentEventKind::ActivityLog => json!({"tool_name": "Read"}),
        AgentEventKind::ToolFailure => json!({"tool_name": "Bash", "error": "boom"}),
        AgentEventKind::PermissionRequest => json!({
            "cwd": "/tmp",
            "permission_mode": "default",
            "tool_name": "Bash",
            "tool_input": {"command": "echo test"},
            "session_id": "session-test",
            "turn_id": "turn-test"
        }),
        AgentEventKind::SubagentStart | AgentEventKind::SubagentStop => {
            json!({"agent_type": "Explore"})
        }
        _ => json!({}),
    }
}

#[cfg(test)]
pub(crate) fn assert_table_drift_free(agent: &str, table: &[HookRegistration]) {
    use crate::event::resolve_adapter;
    let adapter = resolve_adapter(agent).expect("adapter should exist");

    // Table → parse: every registration must be accepted by `parse()` and
    // produce an `AgentEvent` whose kind matches the registration.
    for reg in table {
        let event_name = reg.kind.external_name();
        let payload = minimal_payload(reg.kind);
        let produced = adapter.parse(event_name, &payload).unwrap_or_else(|| {
            panic!(
                "{agent}: HOOK_REGISTRATIONS lists {:?} but parse() returned None — parse arm missing",
                reg.kind
            )
        });
        assert_eq!(
            produced.kind(),
            reg.kind,
            "{agent}: table declares {:?} but parse() produced {:?}",
            reg.kind,
            produced.kind()
        );
    }

    // Parse → table: every kind `parse()` accepts must appear in the table.
    // Catches "added parse arm, forgot to update HOOK_REGISTRATIONS".
    for kind in AgentEventKind::ALL {
        let accepted = adapter
            .parse(kind.external_name(), &minimal_payload(*kind))
            .is_some();
        let in_table = table.iter().any(|r| r.kind == *kind);
        assert!(
            !accepted || in_table,
            "{agent}: parse() accepts {:?} but HOOK_REGISTRATIONS does not list it — add it to the table",
            kind
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_name::CanonicalTool;
    use serde_json::json;

    #[test]
    fn canonical_tool_name_maps_aliases_and_passes_others_through() {
        let aliases: &[(&str, CanonicalTool)] = &[
            ("read", CanonicalTool::Read),
            ("apply_patch", CanonicalTool::Patch),
        ];
        assert_eq!(canonical_tool_name(aliases, "read"), "Read");
        assert_eq!(canonical_tool_name(aliases, "apply_patch"), "Patch");
        assert_eq!(canonical_tool_name(aliases, "Bash"), "Bash");
        assert_eq!(
            canonical_tool_name(aliases, "mcp__context7__query-docs"),
            "mcp__context7__query-docs"
        );
    }

    #[test]
    fn alias_keys_adds_without_removing_the_original() {
        let input = json!({"filePath": "/a/b.rs"});
        let out = alias_keys(input, &[("filePath", "file_path")]);
        assert_eq!(out["file_path"], "/a/b.rs");
        assert_eq!(out["filePath"], "/a/b.rs");
    }

    #[test]
    fn alias_keys_keeps_an_existing_destination() {
        let input = json!({"filePath": "/new.rs", "file_path": "/original.rs"});
        let out = alias_keys(input, &[("filePath", "file_path")]);
        assert_eq!(out["file_path"], "/original.rs");
    }

    #[test]
    fn alias_keys_replaces_an_empty_destination() {
        // A present-but-empty canonical key must not block the alias, or a
        // payload carrying both spellings renders a blank row.
        for empty in [json!(""), json!(null)] {
            let input = json!({"path": "/repo/a.rs", "file_path": empty});
            let out = alias_keys(input, &[("path", "file_path")]);
            assert_eq!(out["file_path"], "/repo/a.rs");
        }
    }

    #[test]
    fn alias_keys_leaves_non_objects_alone() {
        let input = json!("*** Begin Patch\n*** Update File: a.rs\n");
        assert_eq!(alias_keys(input.clone(), &[("a", "b")]), input);
    }

    #[test]
    fn tool_input_value_parses_a_stringified_object() {
        let input = json!({"tool_input": "{\"filePath\":\"/a/b.rs\"}"});
        let parsed = tool_input_value(&input, "tool_input");
        assert_eq!(parsed["filePath"], "/a/b.rs");
    }

    #[test]
    fn tool_input_value_keeps_raw_text_and_scalars() {
        // A patch document, a bare word, and a JSON scalar are all text, not
        // objects: parsing them would destroy the value the extractor reads.
        let patch = json!({"tool_input": "*** Begin Patch\n*** Update File: a.rs\n"});
        assert_eq!(
            tool_input_value(&patch, "tool_input"),
            json!("*** Begin Patch\n*** Update File: a.rs\n")
        );
        for raw in [json!("ls -la"), json!("true"), json!("123")] {
            assert_eq!(
                tool_input_value(&json!({"tool_input": raw}), "tool_input"),
                raw
            );
        }
    }

    #[test]
    fn tool_input_value_passes_objects_and_missing_fields_through() {
        let input = json!({"tool_input": {"command": "ls"}});
        assert_eq!(
            tool_input_value(&input, "tool_input"),
            json!({"command": "ls"})
        );
        assert_eq!(
            tool_input_value(&json!({}), "tool_input"),
            serde_json::Value::Null
        );
    }
}
