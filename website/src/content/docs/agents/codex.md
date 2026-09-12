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

- `Bash` tool calls only. Codex's `PostToolUse` fires only for `Bash` (its `tool_input` is schema-typed as `{ command: string }`), so `Read` / `Edit` / `Write` / `Grep` / `Glob` and every other tool is not reported.

## What is not available

| Feature                                   | Why                                                                 |
| ----------------------------------------- | ------------------------------------------------------------------- |
| Waiting status + wait reason              | `PermissionRequest` reports Codex approval prompts                  |
| Background shell state                    | Codex's Bash hook payload is schema-typed as `{ command: string }` and does not include a background flag |
| API failure reason                        | Needs `StopFailure` (Claude-only)                                    |
| Task progress counter                     | Needs non-Bash `PostToolUse` coverage                                |
| Sub-agent tree                            | Needs `SubagentStart` / `SubagentStop`                               |
| Worktree lifecycle tracking               | Needs `WorktreeCreate` / `WorktreeRemove`                            |
| `notification` / `task_completed` / `stop_failure` / `permission_denied` notifications | Those hooks don't exist in Codex                                     |

## Setup

Wire the hooks from inside a Codex pane — see [Codex setup](/tmux-agent-sidebar/getting-started/codex/).
