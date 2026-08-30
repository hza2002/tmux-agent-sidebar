---
name: sync-upstream-features
description: Research whether current Claude Code and Codex hook or event capabilities relevant to tmux-agent-sidebar are covered locally. Use for new hooks, events, permission modes, or activity metadata; not for Git branch synchronization. Reports gaps without implementing them.
---

# Sync Upstream Features

Report actionable gaps between current agent capabilities and this repository.
Do not implement changes as part of this skill.

## Context

Read these only as needed:

- `docs/maintainers/fork-strategy.md` for upstream compatibility rules.
- `docs/maintainers/architecture-map.md` for the event pipeline and extension seams.
- `src/adapter/` for registrations and payload parsing.
- `src/event.rs`, `src/event/`, and `src/cli/hook/` for normalized events and handlers.
- `src/tmux/types.rs`, `src/tool_name.rs`, and `src/activity.rs` for display-facing coverage.

Use current official documentation and release notes for external facts. Prefer
OpenAI and Anthropic primary sources, and cite every upstream capability claim.

## Method

1. Derive current coverage from adapter registrations, parser branches, event
   types, and handlers. Do not trust a hardcoded list or an earlier report.
2. Collect upstream changes since the last known local coverage. For Codex,
   distinguish documented capabilities from events the CLI actually emits.
3. Keep only changes that can affect agent status, errors, permissions,
   prompts/responses, worktrees, tasks, subagents, or activity presentation.
4. Trace each candidate through the full local contract:

   ```text
   adapter -> AgentEvent -> handler -> tmux option/log -> query/state -> UI
   ```

5. Classify each item as covered, partial, missing, deferred, or out of scope.
   A parser branch alone is not complete coverage when downstream state is lost.

Exclude capabilities unrelated to monitoring, such as compaction, instruction
loading, generic configuration changes, and file-change notifications, unless
they now affect visible agent state.

## Report

Start with a compact coverage table:

| Capability | Agent/version | Local status | Evidence | Impact |
|---|---|---|---|---|

Then describe only partial or missing items. For each, include:

- the authoritative upstream source;
- the exact local files and missing pipeline stage;
- user-visible impact and priority;
- the narrowest existing extension seam.

Do not propose a parallel event pipeline or unrelated core refactor. If no
relevant gaps remain, say so and list the sources and local surfaces checked.
