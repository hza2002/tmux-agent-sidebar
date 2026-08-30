---
name: ui-showcase
description: Render an isolated tmux-agent-sidebar showcase for visual inspection after UI changes. Use for UI checks, demos, and verifying all major pane states without mutating the live sidebar.
---

# UI Showcase

Use the existing scenario fixtures to inspect representative running, waiting,
background, error, task, subagent, worktree, and pet states. Do not manufacture
these states in the user's active Agent pane.

## Workflow

1. Read `.agents/skills/regenerate-captures/SKILL.md` for the scenario-to-output
   map and capture workflow.
2. Build the release binary with `cargo build --release`.
3. Render the smallest scenario set that covers the changed surface into a
   private temporary directory. Use `hero` for the general pane/status showcase
   and the relevant focused or pet scenario for specialized UI.
4. Open the generated PNGs and inspect the intended state, narrow edges, icon
   alignment, colors, ordering, and crop boundaries.
5. Leave tracked capture assets unchanged unless the user also requested capture
   regeneration. Remove only the exact temporary directory created in this run.

Never start background servers or subagents in the user's active session for a
showcase.
