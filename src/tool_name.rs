/// Canonical tool-name vocabulary used across agents. Claude and Codex emit
/// these PascalCase names natively; OpenCode's lowercase IDs are normalised to
/// this vocabulary in `src/adapter/opencode.rs`. Keeping the list as an enum
/// means typos in adapters or the strategy table become compile errors rather
/// than silently unmatched tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalTool {
    Bash,
    Read,
    Edit,
    Write,
    /// Patch application by agents that send a whole patch document instead of
    /// an edit pair (Codex `apply_patch`, OpenCode `patch`/`apply_patch`).
    Patch,
    NotebookEdit,
    PowerShell,
    Monitor,
    PushNotification,
    Glob,
    Grep,
    WebFetch,
    WebSearch,
    ToolSearch,
    Skill,
    SendMessage,
    TeamCreate,
    /// Team teardown. Carries no label; listed so its colour stays off the
    /// gray fallback.
    TeamDelete,
    Lsp,
    CronCreate,
    CronDelete,
    CronList,
    /// Claude Code's remote-trigger tool.
    RemoteTrigger,
    EnterWorktree,
    ExitWorktree,
    /// Plan-mode transitions. They carry no label; they are canonical so the
    /// pane's permission badge keys off the vocabulary instead of raw agent
    /// strings (Kimi uses these names natively, OpenCode's `plan_exit` is
    /// aliased to `ExitPlanMode`).
    EnterPlanMode,
    ExitPlanMode,
    Agent,
    TaskCreate,
    TaskUpdate,
    TaskGet,
    TaskList,
    TaskStop,
    TaskOutput,
    AskUserQuestion,
    TodoWrite,
}

impl CanonicalTool {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "Bash",
            Self::Read => "Read",
            Self::Edit => "Edit",
            Self::Write => "Write",
            Self::Patch => "Patch",
            Self::NotebookEdit => "NotebookEdit",
            Self::PowerShell => "PowerShell",
            Self::Monitor => "Monitor",
            Self::PushNotification => "PushNotification",
            Self::Glob => "Glob",
            Self::Grep => "Grep",
            Self::WebFetch => "WebFetch",
            Self::WebSearch => "WebSearch",
            Self::ToolSearch => "ToolSearch",
            Self::Skill => "Skill",
            Self::SendMessage => "SendMessage",
            Self::TeamCreate => "TeamCreate",
            Self::TeamDelete => "TeamDelete",
            Self::Lsp => "LSP",
            Self::CronCreate => "CronCreate",
            Self::CronDelete => "CronDelete",
            Self::CronList => "CronList",
            Self::RemoteTrigger => "RemoteTrigger",
            Self::EnterWorktree => "EnterWorktree",
            Self::ExitWorktree => "ExitWorktree",
            Self::EnterPlanMode => "EnterPlanMode",
            Self::ExitPlanMode => "ExitPlanMode",
            Self::Agent => "Agent",
            Self::TaskCreate => "TaskCreate",
            Self::TaskUpdate => "TaskUpdate",
            Self::TaskGet => "TaskGet",
            Self::TaskList => "TaskList",
            Self::TaskStop => "TaskStop",
            Self::TaskOutput => "TaskOutput",
            Self::AskUserQuestion => "AskUserQuestion",
            Self::TodoWrite => "TodoWrite",
        }
    }

    /// Every variant, so a test can sweep the vocabulary and the display side
    /// can match on the parsed enum instead of on string literals that silently
    /// drift. Adding a variant means adding it here.
    pub const ALL: &'static [Self] = &[
        Self::Bash,
        Self::Read,
        Self::Edit,
        Self::Write,
        Self::Patch,
        Self::NotebookEdit,
        Self::PowerShell,
        Self::Monitor,
        Self::PushNotification,
        Self::Glob,
        Self::Grep,
        Self::WebFetch,
        Self::WebSearch,
        Self::ToolSearch,
        Self::Skill,
        Self::SendMessage,
        Self::TeamCreate,
        Self::TeamDelete,
        Self::Lsp,
        Self::CronCreate,
        Self::CronDelete,
        Self::CronList,
        Self::RemoteTrigger,
        Self::EnterWorktree,
        Self::ExitWorktree,
        Self::EnterPlanMode,
        Self::ExitPlanMode,
        Self::Agent,
        Self::TaskCreate,
        Self::TaskUpdate,
        Self::TaskGet,
        Self::TaskList,
        Self::TaskStop,
        Self::TaskOutput,
        Self::AskUserQuestion,
        Self::TodoWrite,
    ];

    /// Parse an activity-log tool name back into the vocabulary. `None` is the
    /// expected answer for everything that is not ours: MCP tools
    /// (`mcp__server__tool`), the task-reset sentinel, and agent tools nobody
    /// has mapped yet.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tool| tool.as_str() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_name() {
        for tool in CanonicalTool::ALL {
            assert_eq!(
                CanonicalTool::from_name(tool.as_str()),
                Some(*tool),
                "{:?} does not parse back from its own name",
                tool
            );
        }
    }

    #[test]
    fn names_are_unique() {
        for (index, tool) in CanonicalTool::ALL.iter().enumerate() {
            for other in &CanonicalTool::ALL[index + 1..] {
                assert_ne!(tool.as_str(), other.as_str(), "duplicate tool name");
            }
        }
    }

    #[test]
    fn foreign_names_do_not_parse() {
        for name in [
            "mcp__context7__query-docs",
            "__task_reset__",
            "apply_patch",
            "webrun",
            "",
        ] {
            assert_eq!(CanonicalTool::from_name(name), None, "{name}");
        }
    }
}
