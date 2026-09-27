use serde_json::Value;

use crate::tool_name::CanonicalTool;

/// How a tool's label should be derived from its input/response payload.
/// Keeping this as data (rather than a giant `match` with inline closures)
/// makes adding a new tool a one-line edit in [`STRATEGY_TABLE`] and lets
/// the per-tool extraction logic live as named, individually testable
/// functions for the few cases that need custom code.
enum LabelStrategy {
    /// No row for this tool name — the caller falls back to the payload's own
    /// describing argument or key list.
    Unmapped,
    /// Known tool with nothing worth showing. Distinct from
    /// [`Self::Unmapped`]: a row here keeps the tool deliberately blank instead
    /// of falling through to the payload-shape fallback.
    Empty,
    /// Pull a single string field straight out of `tool_input`.
    Field(&'static str),
    /// Pull a path field out of `tool_input` and reduce to its basename.
    FilePath(&'static str),
    /// Pull a URL field out of `tool_input` and strip the http(s):// prefix.
    UrlStrip(&'static str),
    /// Run a custom extractor that needs both `tool_input` and `tool_response`.
    Custom(fn(&Value, &Value) -> String),
}

/// Tool name → extraction strategy. Order is preserved for readability;
/// dispatch is O(N) but N is ~30 so a linear scan is fine and avoids the
/// overhead/lifetime constraints of a static `HashMap`. Keys are
/// [`CanonicalTool`] so typos become compile errors.
const STRATEGY_TABLE: &[(CanonicalTool, LabelStrategy)] = &[
    (CanonicalTool::Read, LabelStrategy::FilePath("file_path")),
    (CanonicalTool::Edit, LabelStrategy::FilePath("file_path")),
    (CanonicalTool::Write, LabelStrategy::FilePath("file_path")),
    (CanonicalTool::Patch, LabelStrategy::Custom(label_patch)),
    (
        CanonicalTool::NotebookEdit,
        LabelStrategy::FilePath("notebook_path"),
    ),
    (CanonicalTool::Bash, LabelStrategy::Field("command")),
    (CanonicalTool::PowerShell, LabelStrategy::Field("command")),
    (CanonicalTool::Monitor, LabelStrategy::Field("command")),
    (
        CanonicalTool::PushNotification,
        LabelStrategy::Field("message"),
    ),
    (CanonicalTool::Glob, LabelStrategy::Field("pattern")),
    (CanonicalTool::Grep, LabelStrategy::Field("pattern")),
    (CanonicalTool::WebFetch, LabelStrategy::UrlStrip("url")),
    (
        CanonicalTool::WebSearch,
        LabelStrategy::Custom(label_web_search),
    ),
    (CanonicalTool::ToolSearch, LabelStrategy::Field("query")),
    (CanonicalTool::Skill, LabelStrategy::Field("skill")),
    (CanonicalTool::SendMessage, LabelStrategy::Field("to")),
    (CanonicalTool::TeamCreate, LabelStrategy::Field("team_name")),
    (CanonicalTool::TeamDelete, LabelStrategy::Empty),
    (CanonicalTool::Lsp, LabelStrategy::Field("operation")),
    (CanonicalTool::CronCreate, LabelStrategy::Field("cron")),
    (CanonicalTool::CronDelete, LabelStrategy::Field("id")),
    (CanonicalTool::CronList, LabelStrategy::Empty),
    (CanonicalTool::RemoteTrigger, LabelStrategy::Field("action")),
    (
        CanonicalTool::EnterWorktree,
        LabelStrategy::Custom(label_enter_worktree),
    ),
    // ExitWorktree reports what happened to the worktree (`keep` / `remove`),
    // not a name — the worktree's identity is `name` only on the way in.
    (CanonicalTool::ExitWorktree, LabelStrategy::Field("action")),
    (CanonicalTool::Agent, LabelStrategy::Custom(label_agent)),
    // These two label formats are parsed back out of the activity log by
    // `activity::parse_task_progress` — `#1 subject` and `completed #1` drive
    // the task band. Change a format here and that parser changes with it;
    // `test_task_progress_parses_extractor_labels` is the contract test.
    (
        CanonicalTool::TaskCreate,
        LabelStrategy::Custom(label_task_create),
    ),
    (
        CanonicalTool::TaskUpdate,
        LabelStrategy::Custom(label_task_update),
    ),
    (CanonicalTool::TaskGet, LabelStrategy::Custom(label_task_id)),
    (CanonicalTool::TaskList, LabelStrategy::Empty),
    (
        CanonicalTool::TaskStop,
        LabelStrategy::Custom(label_task_id),
    ),
    (
        CanonicalTool::TaskOutput,
        LabelStrategy::Custom(label_task_id),
    ),
    (
        CanonicalTool::AskUserQuestion,
        LabelStrategy::Custom(label_ask_user_question),
    ),
    (CanonicalTool::TodoWrite, LabelStrategy::Custom(label_todos)),
    // Plan-mode transitions carry a plan document, not an identifier, so the
    // row stays deliberately blank rather than printing the payload's shape.
    (CanonicalTool::EnterPlanMode, LabelStrategy::Empty),
    (CanonicalTool::ExitPlanMode, LabelStrategy::Empty),
];

pub(crate) fn extract_tool_label(
    tool_name: &str,
    tool_input: &Value,
    tool_response: &Value,
) -> String {
    let strategy = STRATEGY_TABLE
        .iter()
        .find(|(name, _)| name.as_str() == tool_name)
        .map(|(_, s)| s)
        .unwrap_or(&LabelStrategy::Unmapped);

    // A mapped tool owns its label, including an empty one: `Read` with no path
    // is blank on purpose, not a reason to print the payload's key list. Only a
    // name with no row at all reaches the fallback.
    match strategy {
        LabelStrategy::Unmapped => fallback_label(tool_input),
        LabelStrategy::Empty => String::new(),
        LabelStrategy::Field(key) => field_str(tool_input, key),
        LabelStrategy::FilePath(key) => basename(&field_str(tool_input, key)),
        LabelStrategy::UrlStrip(key) => {
            let url = field_str(tool_input, key);
            url.trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string()
        }
        LabelStrategy::Custom(f) => f(tool_input, tool_response),
    }
}

/// Argument names that describe *what* a call does, in the order we prefer to
/// show them. Shared across agents: every adapter normalises its tool names and
/// key spellings first, so anything reaching here is either an unmapped tool or
/// a mapped one whose argument was missing.
const FALLBACK_KEYS: &[&str] = &[
    "description",
    "summary",
    "title",
    "query",
    "pattern",
    "path",
    "file_path",
    "filePath",
    "command",
    "cmd",
    "prompt",
    "message",
    "objective",
    "explanation",
    "url",
    "name",
    "skill",
    "subject",
    "id",
    "text",
];

/// Fallback labels are one-line hints, not copy targets, so they are capped
/// well below the width a shell command is allowed to use.
const FALLBACK_MAX_CHARS: usize = 160;

/// Last-resort label for a tool the strategy table has no row for: the first
/// scalar value under a [`FALLBACK_KEYS`] name, else the input's own key list.
///
/// The key list is deliberate. It fires only when none of the candidate names
/// is present, and it puts the payload's shape on screen — `{app,x,y}` — which
/// is how an unmapped argument spelling gets identified in the first place. A
/// payload that does carry a describing value shows that value instead.
fn fallback_label(tool_input: &Value) -> String {
    match tool_input {
        Value::String(text) => truncate_label(text),
        Value::Object(map) => {
            for key in FALLBACK_KEYS {
                if let Some(value) = map.get(*key) {
                    let text = scalar_text(value);
                    if !text.is_empty() {
                        return truncate_label(&text);
                    }
                }
            }
            if map.is_empty() {
                String::new()
            } else {
                let keys: Vec<&str> = map.keys().map(String::as_str).collect();
                // Capped like every other fallback: a key name is payload
                // content, so its length is chosen by the agent, not by us.
                truncate_label(&format!("{{{}}}", keys.join(",")))
            }
        }
        _ => String::new(),
    }
}

/// Render a scalar argument as a single line. Arrays contribute their first
/// scalar element (a `queries: ["…"]` shape reads better than nothing) and
/// objects contribute nothing, since their text is not what the caller asked
/// for.
fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(items) => items
            .iter()
            .map(scalar_text)
            .find(|s| !s.is_empty())
            .unwrap_or_default(),
        Value::Object(_) | Value::Null => String::new(),
    }
}

/// Cap a display label. The count is in characters, not display columns, so a
/// label of wide CJK glyphs can render up to twice this width; the renderer
/// wraps and truncates by width, and nothing downstream depends on the exact
/// number, so the cheap count is enough.
fn truncate_label(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= FALLBACK_MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(FALLBACK_MAX_CHARS).collect();
    out.push('…');
    out
}

fn field_str(input: &Value, key: &str) -> String {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Subagent calls: show the short `description` the parent wrote.
///
/// This used to prefer the response text, so the Activity tab would report what
/// came back rather than what was asked. That intent no longer has a payload
/// behind it: Claude Code's Agent response carries no `content` block any more
/// (0 of 154 sampled calls), and Kimi's response is a single `tool_output`
/// string of up to 2000 characters — a blob, not a row. The description is
/// short, present in every adapter, and stable, so it leads; the response text
/// stays as the fallback for a call whose description is missing.
fn label_agent(input: &Value, response: &Value) -> String {
    let description = field_str(input, "description");
    if !description.is_empty() {
        return description;
    }
    response_text(response)
}

/// Extract the text a subagent returned, for the shapes that still carry it
/// inline. Claude's legacy form is `content[].type == "text"`; the other
/// adapters report a plain string under `output` (OpenCode's bridge),
/// `tool_output` (Kimi), or the payload's own `text`/`title`.
fn response_text(response: &Value) -> String {
    let from_blocks = response
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
        })
        .and_then(|block| block.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !from_blocks.trim().is_empty() {
        return truncate_label(from_blocks);
    }
    for key in ["output", "tool_output", "text", "title"] {
        if let Some(text) = response.get(key).and_then(|v| v.as_str())
            && !text.trim().is_empty()
        {
            return truncate_label(text);
        }
    }
    String::new()
}

fn label_task_create(input: &Value, response: &Value) -> String {
    let task_id = response
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subject = field_str(input, "subject");
    if !task_id.is_empty() {
        format!("#{task_id} {subject}")
    } else {
        subject
    }
}

fn label_task_update(input: &Value, _: &Value) -> String {
    let status = field_str(input, "status");
    let task_id = field_str(input, "taskId");
    let mut parts = Vec::new();
    if !status.is_empty() {
        parts.push(status);
    }
    if !task_id.is_empty() {
        parts.push(format!("#{task_id}"));
    }
    parts.join(" ")
}

/// Task tools (Get/Stop/Output) can use either `taskId` or `task_id`.
/// Camel-case wins when both are present, matching the legacy fall-through.
fn label_task_id(input: &Value, _: &Value) -> String {
    let id = field_str(input, "taskId");
    let id = if id.is_empty() {
        field_str(input, "task_id")
    } else {
        id
    };
    if id.is_empty() {
        String::new()
    } else {
        format!("#{id}")
    }
}

/// Web searches report their terms differently per agent: a flat `query` in
/// most, and an array of term objects in Codex's `webrun` (`search_query:
/// [{q: …}]`, the shape its own logs record). Every candidate is tried in order
/// so one spelling cannot shadow another.
fn label_web_search(input: &Value, _: &Value) -> String {
    let action = input.get("action");
    let candidates = [
        input.get("query"),
        input.get("queries"),
        input.get("search_query"),
        action.and_then(|action| action.get("queries")),
        action.and_then(|action| action.get("query")),
        action,
    ];
    for candidate in candidates {
        if let Some(text) = candidate.and_then(search_term) {
            return text;
        }
    }
    String::new()
}

/// One search term out of whatever shape carried it: a bare string, an object
/// with the term under `q`/`query`, or an array of either.
fn search_term(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(text),
        Value::Object(map) => ["q", "query", "text"].iter().find_map(|key| {
            map.get(*key)
                .and_then(|v| v.as_str())
                .and_then(non_empty_text)
        }),
        Value::Array(items) => items.iter().find_map(search_term),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
    }
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Question prompts spell the prompt differently per agent: Claude and
/// OpenCode use `question`, Codex's `request_user_input_async` uses `title`,
/// and an agent that sends a single prompt has no `questions` array at all.
/// The first question is the one the row has space for either way.
fn label_ask_user_question(input: &Value, _: &Value) -> String {
    let first = input
        .get("questions")
        .and_then(|q| q.as_array())
        .and_then(|arr| arr.first());
    let sources = [first, Some(input)];
    for source in sources.into_iter().flatten() {
        for key in ["question", "title", "header"] {
            if let Some(text) = source.get(key).and_then(|v| v.as_str())
                && !text.trim().is_empty()
            {
                return truncate_label(text);
            }
        }
    }
    String::new()
}

/// EnterWorktree names a *new* worktree with `name`, and switches into an
/// existing one with `path` (its own description says so). Either way the row
/// shows the worktree, not a blank.
fn label_enter_worktree(input: &Value, _: &Value) -> String {
    let name = field_str(input, "name");
    if !name.is_empty() {
        return name;
    }
    basename(&field_str(input, "path"))
}

/// Patch documents name the files they touch in their own header lines
/// (`*** Update File: <path>`, `*** Add File: <path>`, `*** Delete File:`).
/// The payload reaches us in one of three shapes depending on the agent:
/// Codex passes the patch text itself, OpenCode passes it under `patchText`,
/// and an agent that reports the result instead passes a `changes[]` array of
/// `{path, …}` entries. All three are read here; the first path names the row
/// and the rest become a count, because a row has space for one filename.
fn label_patch(input: &Value, _: &Value) -> String {
    let mut paths: Vec<String> = Vec::new();

    if let Some(text) = input.as_str() {
        collect_patch_paths(text, &mut paths);
    }
    if let Some(map) = input.as_object() {
        for key in ["patchText", "patch_text", "patch", "input", "text", "diff"] {
            if let Some(text) = map.get(key).and_then(|v| v.as_str()) {
                collect_patch_paths(text, &mut paths);
            }
        }
        for key in ["changes", "files", "edits"] {
            if let Some(items) = map.get(key).and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(path) = item.get("path").and_then(|v| v.as_str()) {
                        paths.push(path.to_string());
                    }
                }
            }
        }
        // Last resort for a spelling nobody has seen yet: any string value that
        // carries the patch markers. Without it an unanticipated key produces a
        // bare `Patch` row, which is the blind spot this mapping exists to close.
        if paths.is_empty() {
            for value in map.values() {
                if let Some(text) = value.as_str()
                    && text.contains("*** ")
                {
                    collect_patch_paths(text, &mut paths);
                }
            }
        }
        if paths.is_empty()
            && let Some(path) = map.get("path").and_then(|v| v.as_str())
        {
            paths.push(path.to_string());
        }
    }

    // Deduplicate by full path, not by basename: `src/a/mod.rs` and
    // `src/b/mod.rs` are two changed files, and counting them as one would make
    // the `+N` understate the patch.
    let mut seen: Vec<String> = Vec::new();
    let mut capped = false;
    for path in paths {
        if path.is_empty() || seen.contains(&path) {
            continue;
        }
        if seen.len() == MAX_PATCH_FILES {
            capped = true;
            break;
        }
        seen.push(path);
    }
    let Some(first) = seen.first() else {
        return String::new();
    };
    let name = truncate_label(&basename(first));
    match seen.len() {
        1 if !capped => name,
        n => format!("{} +{}{}", name, n - 1, if capped { "+" } else { "" }),
    }
}

/// A patch can list an unbounded number of files; only the first name and the
/// count are shown, and the count is a one-line hint, so collection stops here.
const MAX_PATCH_FILES: usize = 64;

/// Pull the file headers out of a patch document. Only the `***` markers are
/// recognised: they are what both Codex's and OpenCode's patch tools emit, and
/// guessing at other diff dialects would put a wrong filename on the row.
fn collect_patch_paths(text: &str, out: &mut Vec<String>) {
    for line in text.lines() {
        let line = line.trim();
        for marker in ["*** Update File:", "*** Add File:", "*** Delete File:"] {
            if let Some(path) = line.strip_prefix(marker) {
                let path = path.trim();
                if !path.is_empty() {
                    out.push(path.to_string());
                }
            }
        }
    }
}

/// Todo lists are arrays of items, so the row shows the count plus the item
/// still open — the same thing the reader would scan the list for. Item keys
/// differ per agent (`content`/`activeForm` on Claude, `text` on others).
fn label_todos(input: &Value, _: &Value) -> String {
    let Some(items) = input.get("todos").and_then(|v| v.as_array()) else {
        return String::new();
    };
    if items.is_empty() {
        return String::new();
    }
    let active = items.iter().find(|item| {
        matches!(
            item.get("status").and_then(|v| v.as_str()),
            Some("in_progress") | Some("in-progress")
        )
    });
    let text = active
        .or_else(|| items.first())
        .map(|item| {
            ["activeForm", "content", "text", "subject", "title", "label"]
                .iter()
                .find_map(|key| item.get(*key).and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    if text.is_empty() {
        count_tasks(items.len())
    } else {
        format!("{} · {}", count_tasks(items.len()), truncate_label(&text))
    }
}

fn count_tasks(count: usize) -> String {
    if count == 1 {
        "1 task".to_string()
    } else {
        format!("{count} tasks")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every tool in the vocabulary has a deliberate label decision. Without
    /// this, a new variant silently falls into the payload-shape fallback and
    /// shows `{file_path}` instead of a filename.
    #[test]
    fn every_canonical_tool_has_a_strategy_row() {
        for tool in CanonicalTool::ALL {
            assert!(
                STRATEGY_TABLE.iter().any(|(name, _)| name == tool),
                "{tool:?} has no STRATEGY_TABLE row — add one (LabelStrategy::Empty if it carries nothing worth showing)"
            );
        }
    }

    #[test]
    fn label_read_extracts_basename() {
        let input = json!({"file_path": "/home/user/project/src/main.rs"});
        assert_eq!(extract_tool_label("Read", &input, &json!(null)), "main.rs");
    }

    #[test]
    fn label_edit_extracts_basename() {
        let input = json!({"file_path": "/tmp/foo.txt"});
        assert_eq!(extract_tool_label("Edit", &input, &json!(null)), "foo.txt");
    }

    #[test]
    fn label_write_extracts_basename() {
        let input = json!({"file_path": "/a/b/c.json"});
        assert_eq!(extract_tool_label("Write", &input, &json!(null)), "c.json");
    }

    #[test]
    fn label_file_missing_path() {
        assert_eq!(extract_tool_label("Read", &json!({}), &json!(null)), "");
    }

    #[test]
    fn label_file_bare_filename() {
        let input = json!({"file_path": "README.md"});
        assert_eq!(
            extract_tool_label("Read", &input, &json!(null)),
            "README.md"
        );
    }

    #[test]
    fn label_bash_extracts_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(
            extract_tool_label("Bash", &input, &json!(null)),
            "cargo build"
        );
    }

    #[test]
    fn label_bash_preserves_long_command() {
        let cmd = "npm run test -- --watch --coverage --verbose --maxWorkers=4";
        let input = json!({"command": cmd});
        assert_eq!(extract_tool_label("Bash", &input, &json!(null)), cmd);
    }

    #[test]
    fn label_glob_extracts_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_label("Glob", &input, &json!(null)), "**/*.rs");
    }

    #[test]
    fn label_grep_extracts_pattern() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_label("Grep", &input, &json!(null)), "fn main");
    }

    #[test]
    fn label_agent_prefers_description_over_response_text() {
        // Claude Code's Agent response carries no `content` block (0 of 154
        // sampled calls), and Kimi's response is a 2000-character blob, so the
        // short description the parent wrote is what the row shows.
        let input = json!({"description": "Search codebase"});
        let response = json!({
            "content": [
                {"type": "text", "text": "Found the bug at main.rs:42"}
            ],
            "status": "completed"
        });
        assert_eq!(
            extract_tool_label("Agent", &input, &response),
            "Search codebase"
        );
    }

    #[test]
    fn label_agent_uses_response_text_when_description_missing() {
        let response = json!({
            "content": [
                {"type": "text", "text": "Found the bug at main.rs:42"}
            ]
        });
        assert_eq!(
            extract_tool_label("Agent", &json!({}), &response),
            "Found the bug at main.rs:42"
        );
    }

    #[test]
    fn label_agent_falls_back_to_description_when_response_has_no_text_block() {
        // tool_response exists but lacks a text content block.
        let input = json!({"description": "Deploy to staging"});
        let response = json!({"status": "completed"});
        assert_eq!(
            extract_tool_label("Agent", &input, &response),
            "Deploy to staging"
        );
    }

    #[test]
    fn label_agent_falls_back_to_description_when_text_is_empty() {
        let input = json!({"description": "Explore repo"});
        let response = json!({
            "content": [
                {"type": "text", "text": "   "}
            ]
        });
        assert_eq!(
            extract_tool_label("Agent", &input, &response),
            "Explore repo"
        );
    }

    #[test]
    fn label_agent_picks_text_block_among_mixed_types() {
        // Response can contain multiple content blocks; pick the first text one.
        let response = json!({
            "content": [
                {"type": "tool_use", "id": "x"},
                {"type": "text", "text": "No drift detected"}
            ]
        });
        assert_eq!(
            extract_tool_label("Agent", &json!({}), &response),
            "No drift detected"
        );
    }

    #[test]
    fn label_agent_preserves_multiline_response() {
        // Multi-line responses are kept as-is; sanitization happens later in
        // hook.rs::write_activity_entry.
        let response = json!({
            "content": [
                {"type": "text", "text": "main.rs\nlib.rs\ntmux.rs"}
            ]
        });
        assert_eq!(
            extract_tool_label("Agent", &json!({}), &response),
            "main.rs\nlib.rs\ntmux.rs"
        );
    }

    #[test]
    fn label_webfetch_strips_https() {
        let input = json!({"url": "https://example.com/docs"});
        assert_eq!(
            extract_tool_label("WebFetch", &input, &json!(null)),
            "example.com/docs"
        );
    }

    #[test]
    fn label_webfetch_strips_http() {
        let input = json!({"url": "http://example.com"});
        assert_eq!(
            extract_tool_label("WebFetch", &input, &json!(null)),
            "example.com"
        );
    }

    #[test]
    fn label_webfetch_no_protocol_unchanged() {
        let input = json!({"url": "example.com/path"});
        assert_eq!(
            extract_tool_label("WebFetch", &input, &json!(null)),
            "example.com/path"
        );
    }

    #[test]
    fn label_websearch_extracts_query() {
        let input = json!({"query": "rust tutorial"});
        assert_eq!(
            extract_tool_label("WebSearch", &input, &json!(null)),
            "rust tutorial"
        );
    }

    #[test]
    fn label_skill_extracts_skill() {
        let input = json!({"skill": "commit"});
        assert_eq!(extract_tool_label("Skill", &input, &json!(null)), "commit");
    }

    #[test]
    fn label_toolsearch_extracts_query() {
        let input = json!({"query": "select:Read"});
        assert_eq!(
            extract_tool_label("ToolSearch", &input, &json!(null)),
            "select:Read"
        );
    }

    #[test]
    fn label_task_create_with_id() {
        let input = json!({"subject": "Add feature"});
        let response = json!({"task": {"id": "1"}});
        assert_eq!(
            extract_tool_label("TaskCreate", &input, &response),
            "#1 Add feature"
        );
    }

    #[test]
    fn label_task_create_without_id() {
        let input = json!({"subject": "Add feature"});
        assert_eq!(
            extract_tool_label("TaskCreate", &input, &json!(null)),
            "Add feature"
        );
    }

    #[test]
    fn label_task_create_empty_subject_with_id() {
        let input = json!({});
        let response = json!({"task": {"id": "5"}});
        assert_eq!(extract_tool_label("TaskCreate", &input, &response), "#5 ");
    }

    #[test]
    fn label_task_update_status_and_id() {
        let input = json!({"status": "completed", "taskId": "3"});
        assert_eq!(
            extract_tool_label("TaskUpdate", &input, &json!(null)),
            "completed #3"
        );
    }

    #[test]
    fn label_task_update_status_only() {
        let input = json!({"status": "in_progress"});
        assert_eq!(
            extract_tool_label("TaskUpdate", &input, &json!(null)),
            "in_progress"
        );
    }

    #[test]
    fn label_task_update_id_only() {
        let input = json!({"taskId": "7"});
        assert_eq!(extract_tool_label("TaskUpdate", &input, &json!(null)), "#7");
    }

    #[test]
    fn label_task_update_empty() {
        assert_eq!(
            extract_tool_label("TaskUpdate", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_task_get_with_task_id() {
        let input = json!({"taskId": "5"});
        assert_eq!(extract_tool_label("TaskGet", &input, &json!(null)), "#5");
    }

    #[test]
    fn label_task_stop_with_task_id() {
        let input = json!({"task_id": "7"});
        assert_eq!(extract_tool_label("TaskStop", &input, &json!(null)), "#7");
    }

    #[test]
    fn label_task_get_prefers_task_id_camel_case() {
        let input = json!({"taskId": "1", "task_id": "2"});
        assert_eq!(extract_tool_label("TaskGet", &input, &json!(null)), "#1");
    }

    #[test]
    fn label_task_output_empty() {
        assert_eq!(
            extract_tool_label("TaskOutput", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_send_message() {
        let input = json!({"to": "agent-1"});
        assert_eq!(
            extract_tool_label("SendMessage", &input, &json!(null)),
            "agent-1"
        );
    }

    #[test]
    fn label_team_create() {
        let input = json!({"team_name": "reviewers"});
        assert_eq!(
            extract_tool_label("TeamCreate", &input, &json!(null)),
            "reviewers"
        );
    }

    #[test]
    fn label_notebook_edit() {
        let input = json!({"notebook_path": "/home/user/analysis.ipynb"});
        assert_eq!(
            extract_tool_label("NotebookEdit", &input, &json!(null)),
            "analysis.ipynb"
        );
    }

    #[test]
    fn label_lsp() {
        let input = json!({"operation": "hover"});
        assert_eq!(extract_tool_label("LSP", &input, &json!(null)), "hover");
    }

    #[test]
    fn label_ask_user_question() {
        let input = json!({"questions": [{"question": "Which option?"}]});
        assert_eq!(
            extract_tool_label("AskUserQuestion", &input, &json!(null)),
            "Which option?"
        );
    }

    #[test]
    fn label_ask_user_question_empty_array() {
        assert_eq!(
            extract_tool_label("AskUserQuestion", &json!({"questions": []}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_ask_user_question_no_questions_key() {
        assert_eq!(
            extract_tool_label("AskUserQuestion", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_cron_create() {
        let input = json!({"cron": "*/5 * * * *"});
        assert_eq!(
            extract_tool_label("CronCreate", &input, &json!(null)),
            "*/5 * * * *"
        );
    }

    #[test]
    fn label_cron_delete() {
        let input = json!({"id": "abc123"});
        assert_eq!(
            extract_tool_label("CronDelete", &input, &json!(null)),
            "abc123"
        );
    }

    #[test]
    fn label_enter_worktree() {
        let input = json!({"name": "feature-branch"});
        assert_eq!(
            extract_tool_label("EnterWorktree", &input, &json!(null)),
            "feature-branch"
        );
    }

    #[test]
    fn label_powershell_extracts_command() {
        let input = json!({"command": "Get-Process"});
        assert_eq!(
            extract_tool_label("PowerShell", &input, &json!(null)),
            "Get-Process"
        );
    }

    #[test]
    fn label_exit_worktree_ignores_a_stale_name_key() {
        // ExitWorktree used to be mapped to `name`, which the tool never sends:
        // the label was always empty. A name-only payload stays blank rather
        // than inventing one.
        let input = json!({"name": "feature-branch"});
        assert_eq!(extract_tool_label("ExitWorktree", &input, &json!(null)), "");
    }

    #[test]
    fn label_powershell_empty_command() {
        assert_eq!(
            extract_tool_label("PowerShell", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_monitor_extracts_command() {
        let input = json!({"command": "tail -f /var/log/server.log"});
        assert_eq!(
            extract_tool_label("Monitor", &input, &json!(null)),
            "tail -f /var/log/server.log"
        );
    }

    #[test]
    fn label_push_notification_extracts_message() {
        let input = json!({"message": "Deploy finished"});
        assert_eq!(
            extract_tool_label("PushNotification", &input, &json!(null)),
            "Deploy finished"
        );
    }

    #[test]
    fn label_exit_worktree_empty_name() {
        assert_eq!(
            extract_tool_label("ExitWorktree", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_unknown_tool_reports_its_payload_shape() {
        assert_eq!(
            extract_tool_label("UnknownTool", &json!({"anything": "value"}), &json!(null)),
            "{anything}"
        );
    }

    #[test]
    fn label_null_inputs() {
        assert_eq!(extract_tool_label("Read", &json!(null), &json!(null)), "");
        assert_eq!(
            extract_tool_label("TaskCreate", &json!(null), &json!(null)),
            ""
        );
        assert_eq!(extract_tool_label("Bash", &json!(null), &json!(null)), "");
        assert_eq!(
            extract_tool_label("WebFetch", &json!(null), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_restored_tools_keep_their_own_strategies() {
        // CronList carries nothing to show, so its row stays bare even when a
        // payload arrives with a describing key.
        assert_eq!(
            extract_tool_label("CronList", &json!({"description": "jobs"}), &json!(null)),
            ""
        );
        assert_eq!(
            extract_tool_label("RemoteTrigger", &json!({"action": "fire"}), &json!(null)),
            "fire"
        );
        assert_eq!(
            extract_tool_label("TaskList", &json!({"active_only": true}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_team_delete_is_blank() {
        assert_eq!(
            extract_tool_label("TeamDelete", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_exit_worktree_reports_the_action() {
        // ExitWorktree has no `name`; it reports what happened to the worktree.
        let input = json!({"action": "remove", "discard_changes": true});
        assert_eq!(
            extract_tool_label("ExitWorktree", &input, &json!(null)),
            "remove"
        );
    }

    #[test]
    fn label_patch_reads_the_patch_text_itself() {
        // Codex passes the patch document as the tool argument (a string).
        let input = json!(
            "*** Begin Patch\n*** Update File: /repo/src/activity.rs\n@@\n-old\n+new\n*** End Patch"
        );
        assert_eq!(
            extract_tool_label("Patch", &input, &json!(null)),
            "activity.rs"
        );
    }

    #[test]
    fn label_patch_counts_the_remaining_files() {
        let input = json!(
            "*** Begin Patch\n*** Add File: /repo/docs/a.md\n+body\n*** Delete File: /repo/docs/b.md\n*** End Patch"
        );
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "a.md +1");
    }

    #[test]
    fn label_patch_reads_the_patch_text_key() {
        // OpenCode reports the same document under `patchText`.
        let input = json!({"patchText": "*** Update File: src/main.rs\n@@\n"});
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "main.rs");
    }

    #[test]
    fn label_patch_reads_a_changes_array() {
        // An agent that reports the applied result instead of the document.
        let input = json!({"changes": [{"path": "/repo/src/lib.rs", "kind": "update"}]});
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "lib.rs");
    }

    #[test]
    fn label_patch_dedupes_repeated_files() {
        let input = json!(
            "*** Begin Patch\n*** Update File: /a/x.rs\n@@\n*** Update File: /a/x.rs\n@@\n*** End Patch"
        );
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "x.rs");
    }

    #[test]
    fn label_todos_counts_and_names_the_active_item() {
        let input = json!({"todos": [
            {"content": "Wire the adapter", "status": "completed"},
            {"content": "Add a test", "activeForm": "Adding a test", "status": "in_progress"}
        ]});
        assert_eq!(
            extract_tool_label("TodoWrite", &input, &json!(null)),
            "2 tasks · Adding a test"
        );
    }

    #[test]
    fn label_todos_falls_back_to_the_first_item() {
        let input = json!({"todos": [{"content": "Only one", "status": "pending"}]});
        assert_eq!(
            extract_tool_label("TodoWrite", &input, &json!(null)),
            "1 task · Only one"
        );
    }

    #[test]
    fn label_todos_handles_an_unknown_item_shape() {
        // Kimi's TodoList items are not the Claude shape; the count still shows.
        let input = json!({"todos": [{"id": "1"}, {"id": "2"}]});
        assert_eq!(
            extract_tool_label("TodoWrite", &input, &json!(null)),
            "2 tasks"
        );
    }

    #[test]
    fn label_todos_empty_list_is_blank() {
        assert_eq!(
            extract_tool_label("TodoWrite", &json!({"todos": []}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_web_search_reads_codex_search_query_objects() {
        // The shape Codex's own logs record for `web__run`.
        let input =
            json!({"search_query": [{"q": "site:clash-verge-rev.github.io tun"}, {"q": "second"}]});
        assert_eq!(
            extract_tool_label("WebSearch", &input, &json!(null)),
            "site:clash-verge-rev.github.io tun"
        );
        let flat_objects = json!({"queries": [{"query": "object term"}]});
        assert_eq!(
            extract_tool_label("WebSearch", &flat_objects, &json!(null)),
            "object term"
        );
    }

    #[test]
    fn label_enter_worktree_reads_a_name_or_a_path() {
        assert_eq!(
            extract_tool_label("EnterWorktree", &json!({"name": "feature-x"}), &json!(null)),
            "feature-x"
        );
        assert_eq!(
            extract_tool_label(
                "EnterWorktree",
                &json!({"path": "/repo/.claude/worktrees/feature-branch"}),
                &json!(null)
            ),
            "feature-branch"
        );
        assert_eq!(
            extract_tool_label("EnterWorktree", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_web_search_reads_the_nested_codex_shape() {
        // Codex's `webrun` model-facing form nests the terms; its hook spelling
        // is unconfirmed, so both shapes are read.
        let input = json!({"action": {"queries": ["rust lsp setup"]}});
        assert_eq!(
            extract_tool_label("WebSearch", &input, &json!(null)),
            "rust lsp setup"
        );
    }

    #[test]
    fn label_web_search_prefers_the_flat_query() {
        let input = json!({"query": "rust lsp", "action": {"queries": ["other"]}});
        assert_eq!(
            extract_tool_label("WebSearch", &input, &json!(null)),
            "rust lsp"
        );
    }

    #[test]
    fn label_plan_mode_transitions_stay_blank() {
        // The plan document is not an identifier; the row keeps its tool name.
        let input = json!({"plan": "# Plan\n- step", "planFilePath": "/tmp/plan.md"});
        assert_eq!(extract_tool_label("ExitPlanMode", &input, &json!(null)), "");
        assert_eq!(
            extract_tool_label("EnterPlanMode", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_ask_user_question_reads_codex_title() {
        // Codex's request_user_input_async spells the prompt `title`.
        let input = json!({"questions": [{"title": "Which database?", "options": []}]});
        assert_eq!(
            extract_tool_label("AskUserQuestion", &input, &json!(null)),
            "Which database?"
        );
    }

    #[test]
    fn fallback_uses_the_first_describing_argument() {
        // An unmapped tool — a new agent tool or an MCP call.
        let input = json!({"anything": 1, "description": "Take a screenshot"});
        assert_eq!(
            extract_tool_label("mcp__plugin-x__capture", &input, &json!(null)),
            "Take a screenshot"
        );
    }

    #[test]
    fn fallback_reports_the_payload_shape_when_no_key_is_known() {
        // No candidate key: the row carries the argument names instead of
        // nothing, which is how an unmapped payload gets identified.
        let input = json!({"app": "Safari", "count": 2});
        let label = extract_tool_label("mcp__plugin-kimi-cu_mac__click", &input, &json!(null));
        assert!(
            label.starts_with('{') && label.ends_with('}'),
            "got {label}"
        );
        assert!(
            label.contains("app") && label.contains("count"),
            "got {label}"
        );
    }

    #[test]
    fn fallback_reads_a_string_payload() {
        let input = json!("raw tool argument");
        assert_eq!(
            extract_tool_label("some_new_tool", &input, &json!(null)),
            "raw tool argument"
        );
    }

    #[test]
    fn fallback_truncates_a_long_value() {
        let input = json!({"description": "x".repeat(400)});
        let label = extract_tool_label("some_new_tool", &input, &json!(null));
        assert_eq!(label.chars().count(), FALLBACK_MAX_CHARS + 1);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn fallback_truncates_a_long_key_list() {
        // Key names are payload content, so the shape branch is capped too.
        let long_key = "k".repeat(400);
        let input = json!({ long_key: 1 });
        let label = extract_tool_label("some_new_tool", &input, &json!(null));
        assert_eq!(label.chars().count(), FALLBACK_MAX_CHARS + 1);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn fallback_reads_scalars_and_arrays() {
        assert_eq!(
            extract_tool_label("some_new_tool", &json!({"id": 42}), &json!(null)),
            "42"
        );
        assert_eq!(
            extract_tool_label(
                "some_new_tool",
                &json!({"query": ["first", "second"]}),
                &json!(null)
            ),
            "first"
        );
    }

    #[test]
    fn label_patch_reads_the_alternate_key_spellings() {
        // A spelling nobody has observed yet still yields a row.
        for key in ["patch", "patch_text", "input", "diff"] {
            let input = json!({ key: "*** Update File: /repo/src/alt.rs\n@@\n" });
            assert_eq!(
                extract_tool_label("Patch", &input, &json!(null)),
                "alt.rs",
                "key {key}"
            );
        }
        let files = json!({"files": [{"path": "/repo/src/one.ts"}]});
        assert_eq!(extract_tool_label("Patch", &files, &json!(null)), "one.ts");
        let bare = json!({"path": "/repo/src/two.ts"});
        assert_eq!(extract_tool_label("Patch", &bare, &json!(null)), "two.ts");
    }

    #[test]
    fn label_patch_scans_an_unknown_string_value_for_markers() {
        // The key name is unknown, but the value is recognisably a patch.
        let input = json!({"operation": "*** Add File: /repo/docs/new.md\n+x\n"});
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "new.md");
    }

    #[test]
    fn label_patch_counts_files_by_path_not_basename() {
        let input = json!(
            "*** Begin Patch\n*** Update File: src/a/mod.rs\n@@\n*** Update File: src/b/mod.rs\n@@\n*** End Patch"
        );
        assert_eq!(
            extract_tool_label("Patch", &input, &json!(null)),
            "mod.rs +1"
        );
    }

    #[test]
    fn label_patch_without_markers_is_blank() {
        let input = json!({"patchText": "no markers here"});
        assert_eq!(extract_tool_label("Patch", &input, &json!(null)), "");
    }

    #[test]
    fn label_todos_reads_every_documented_item_key() {
        for key in ["text", "subject", "title", "label"] {
            let input = json!({"todos": [{ key: "Do the thing", "status": "in-progress" }]});
            assert_eq!(
                extract_tool_label("TodoWrite", &input, &json!(null)),
                "1 task · Do the thing",
                "key {key}"
            );
        }
    }

    #[test]
    fn label_todos_without_a_list_is_blank() {
        assert_eq!(
            extract_tool_label("TodoWrite", &json!({"items": []}), &json!(null)),
            ""
        );
        assert_eq!(
            extract_tool_label("TodoWrite", &json!({"todos": "not a list"}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_web_search_tries_every_spelling() {
        // A non-array `action.queries` must not shadow the flat spelling.
        let shadowed = json!({"action": {"queries": "nested"}, "queries": ["flat"]});
        assert_eq!(
            extract_tool_label("WebSearch", &shadowed, &json!(null)),
            "flat"
        );
        // Codex's model-facing form nests a single term under `action.query`.
        let nested = json!({"action": {"query": "nested term"}});
        assert_eq!(
            extract_tool_label("WebSearch", &nested, &json!(null)),
            "nested term"
        );
        let action_string = json!({"action": "bare"});
        assert_eq!(
            extract_tool_label("WebSearch", &action_string, &json!(null)),
            "bare"
        );
        assert_eq!(
            extract_tool_label("WebSearch", &json!({}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_ask_user_question_reads_a_flat_prompt() {
        // An agent that sends a single prompt has no `questions` array.
        let flat = json!({"title": "Which database?"});
        assert_eq!(
            extract_tool_label("AskUserQuestion", &flat, &json!(null)),
            "Which database?"
        );
        assert_eq!(
            extract_tool_label("AskUserQuestion", &json!({"questions": []}), &json!(null)),
            ""
        );
    }

    #[test]
    fn label_agent_reads_a_string_response_of_any_agent() {
        // OpenCode's bridge and Kimi both report a plain string, not content blocks.
        for key in ["output", "tool_output", "text", "title"] {
            let response = json!({ key: "the subagent's answer" });
            assert_eq!(
                extract_tool_label("Agent", &json!({}), &response),
                "the subagent's answer",
                "key {key}"
            );
        }
    }

    #[test]
    fn fallback_leaves_an_empty_payload_blank() {
        assert_eq!(
            extract_tool_label("some_new_tool", &json!({}), &json!(null)),
            ""
        );
        assert_eq!(
            extract_tool_label("some_new_tool", &json!(null), &json!(null)),
            ""
        );
    }

    #[test]
    fn fallback_does_not_override_a_mapped_label() {
        // A mapped tool owns its label, including an empty one. `Read` with no
        // path and a describing key stays blank — an implementation that fell
        // back whenever the label came out empty would print "why".
        let input = json!({"description": "why"});
        assert_eq!(extract_tool_label("Read", &input, &json!(null)), "");
        // With its path present it shows the file, not the description.
        let input = json!({"file_path": "/a/b.rs", "description": "why"});
        assert_eq!(extract_tool_label("Read", &input, &json!(null)), "b.rs");
    }
}
