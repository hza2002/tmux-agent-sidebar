---
title: Codex
description: What the sidebar shows for Codex panes, and what is not available due to the Codex hook schema.
---

Codex exposes a smaller hook set than Claude Code, so some sidebar features are not available.

## What you get

### Status and prompts

- Live status from `SessionStart` / `UserPromptSubmit` / `PermissionRequest` / `Stop`
- Prompt text from `UserPromptSubmit`
- Response preview (`▷ …`) from `Stop`
- Elapsed time since the last prompt

### Git

- Branch display from the pane's `cwd`
- PR number (needs `gh` CLI)

### Permission badges

- `auto` and `!` — inferred from process arguments
- `plan` / `edit` are **not** available on Codex

### Notifications

- `permission` — fires when Codex's `PermissionRequest` hook reports that it is waiting for approval (surfaced through the sidebar's `notification` event).
- `stop` — available as an opt-in completion alert; it is silent by default.

### Activity log

- Every tool. Codex's `PostToolUse` fires for all of them — the hook matcher is empty and `tool_input` is untyped — so the log shows what the agent calls: `Bash` (Codex rewrites `exec_command` → `Bash`, `cmd` → `command`), `apply_patch`, `webrun`, `request_user_input_async`, and `view_image`. Whether MCP tool calls reach the hook is unverified.
- Codex spells its own tool names and arguments, so the adapter maps them onto the shared vocabulary: `apply_patch` → `Patch` (the row names the first file in the patch document), `webrun` → `WebSearch` (its terms arrive as `search_query: [{q: …}]`), `view_image` → `Read`, `request_user_input_async` → `AskUserQuestion`.
- A tool the sidebar has no mapping for still gets a row: the label falls back to the first describing argument, or to the payload's key list (`{app,x,y}`) when no argument name is recognised.

## What is not available

| Feature                                   | Why                                                                 |
| ----------------------------------------- | ------------------------------------------------------------------- |
| Waiting status + wait reason              | `PermissionRequest` reports Codex approval prompts                  |
| Background shell state                    | Codex's Bash hook payload carries no background flag                 |
| API failure reason                        | Needs `StopFailure` (Claude-only)                                    |
| Task progress counter                     | Codex reports plans through `update_plan`, not `TaskCreate` / `TaskUpdate` |
| Sub-agent tree                            | Needs `SubagentStart` / `SubagentStop`                               |
| Worktree lifecycle tracking               | Needs `WorktreeCreate` / `WorktreeRemove`                            |
| `notification` / `task_completed` / `stop_failure` / `permission_denied` notifications | Those hooks don't exist in Codex                                     |

## Setup

Wire the hooks from inside a Codex pane — see [Codex setup](/tmux-agent-sidebar/getting-started/codex/).
