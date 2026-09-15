---
title: Kimi Code
description: What the sidebar shows for Kimi Code panes, and how its TOML hooks map to sidebar events.
---

Kimi Code exposes native hooks through `[[hooks]]` entries in `~/.kimi-code/config.toml`, covering the sidebar's core lifecycle — no plugin bridge is needed.

## What you get

### Status and prompts

- Live status from `SessionStart` / `SessionEnd` / `UserPromptSubmit` / `Stop` / `StopFailure`
- Prompt text from `UserPromptSubmit`
- Elapsed time since the last prompt

### Attention cues

- Waiting status + wait reason from `PermissionRequest` and `Notification`
- API failure reason from `StopFailure`
- `notification` / `stop_failure` desktop alerts

### Sub-agents and activity

- Sub-agent display from `SubagentStart` / `SubagentStop`
- Activity log from `PostToolUse` (all tools, not Bash-only)

### Git

- Branch display from the pane's `cwd`

## What is not available

| Feature                    | Why |
| -------------------------- | --- |
| Response preview (`▷ ...`) | Kimi's `Stop` payload carries no message field, so the pane keeps showing the last prompt |
| Permission badge           | Hook payloads carry no permission-mode field, and the long-running `kimi-code` process drops the CLI flags from argv after startup |
| Background shell state     | Kimi does not document a background Bash flag in `PostToolUse` |
| Task progress counter      | No task-lifecycle hooks (`TaskCreated` / `TaskCompleted`) |
| Worktree lifecycle tracking | No `WorktreeCreate` / `WorktreeRemove` hooks |

## Setup

Register the hooks from [Kimi Code setup](/tmux-agent-sidebar/getting-started/kimi/).
