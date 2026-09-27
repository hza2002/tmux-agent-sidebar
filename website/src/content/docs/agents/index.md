---
title: Agent support overview
description: What the sidebar shows for Claude Code, Codex, OpenCode, and Kimi Code, side by side.
---

Claude Code, Codex, OpenCode, and Kimi Code work with the sidebar, but they expose different sets of hooks — so the sidebar's surface area is narrower for Codex and OpenCode than it is for Claude Code and Kimi Code.

## Feature support by agent

| Feature                                  | Claude Code | Codex        | OpenCode     | Kimi Code    | Notes                                                                                                                           |
| ---------------------------------------- | ----------- | ------------ | ------------ | ------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| Base status tracking                    | ✓           | ✓            | ✓            | ✓            | Covers `running`, `idle`, and `error`; `waiting` and `background` depend on agent-specific hooks                                |
| Prompt text display                      | ✓           | ✓            | ✓            | ✓            | Saved from `UserPromptSubmit`                                                                                                   |
| Response text display (`▷ ...`)          | ✓           | ✓            | ✓            | —            | Populated from the `Stop` payload; Kimi's `Stop` payload carries no message field, so the pane keeps showing the last prompt        |
| Background shell state                   | ✓           | —            | —            | —            | Claude Bash tools can report `run_in_background`; the other agents do not currently document a background Bash flag               |
| Waiting status + wait reason             | ✓           | ✓ (approval) | ✓            | ✓            | Codex and Kimi map `PermissionRequest` to waiting; OpenCode maps permission prompts; Claude also has `Notification`, `PermissionDenied`, and `TeammateIdle` |
| API failure reason display               | ✓           | —            | ✓            | ✓            | `StopFailure` is wired for Claude, OpenCode, and Kimi                                                                           |
| Permission badge                         | ✓ (`plan` / `edit` / `auto` / `!`) | ✓ (`auto` / `!` only) | — | — | Codex badges are inferred from process arguments; Kimi's long-running process drops its CLI flags from argv, and OpenCode does not expose permission modes |
| Git branch display                       | ✓           | ✓            | ✓            | ✓            | Uses the pane `cwd`; Claude updates dynamically via `CwdChanged`                                                                |
| Elapsed time                             | ✓           | ✓            | ✓            | ✓            | Since the last prompt                                                                                                            |
| Task progress                            | ✓           | —            | —            | —            | Requires `PostToolUse` plus task lifecycle events; Codex reports plans through `update_plan`, not `TaskCreate` / `TaskUpdate`     |
| Task lifecycle notifications             | ✓           | ✓ (`Stop` only) | ✓         | ✓            | `Stop` desktop notifications fire for all four. `Notification`, `TaskCompleted`, `StopFailure`, and `PermissionDenied` vary.     |
| Sub-agent display                        | ✓           | —            | —            | ✓            | Requires `SubagentStart` / `SubagentStop`                                                                                        |
| Activity log                             | ✓           | ✓            | ✓            | ✓            | Codex fires `PostToolUse` for every tool; the sidebar maps its own tool names and argument spellings. OpenCode records the tool events the plugin bridge receives |
| Worktree lifecycle tracking              | ✓           | —            | —            | —            | Requires `WorktreeCreate` / `WorktreeRemove`                                                                                     |
