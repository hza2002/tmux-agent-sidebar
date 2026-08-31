---
title: tmux options
description: Every @sidebar_* / @agent-sidebar-* option the plugin reads.
---

Most options must be set **before** loading the plugin in your `tmux.conf`. Color values can be xterm 256-color numbers (`0`-`255`) or six-digit hex colors (`#RRGGBB` or `RRGGBB`); icons can be any Unicode glyph.

## Sidebar behavior

| Option                           | Default | Description                                                                             |
| -------------------------------- | ------- | --------------------------------------------------------------------------------------- |
| `@sidebar_key`                   | `A`     | Prefix-triggered keybinding to toggle the sidebar in the current window; `off` disables it |
| `@sidebar_key_all`               | `off`   | Prefix-triggered keybinding to toggle the sidebar in all windows; `off` disables it       |
| `@sidebar_close_key`             | `M-A`   | Prefix-triggered keybinding that closes the current window's sidebar from any pane; empty or `off` disables it |
| `@sidebar_width`                 | `35`    | Width in columns or as a percentage                                                     |
| `@sidebar_position`              | `left`  | Sidebar placement (`left` or `right`)                                                   |
| `@sidebar_bottom_height`         | `0`     | Bottom panel height in lines (`0` hides it)                                              |
| `@sidebar_auto_create`           | `off`   | Auto-create the sidebar on new windows (set `on` to enable)                             |
| `@sidebar_auto_create_delay`     | `0`     | Seconds to defer auto-create after a window opens, so a declaratively-built window (e.g. tmuxinator's `select-layout`) finishes before the sidebar pane is injected; accepts fractional seconds. `0` keeps the create synchronous |
| `@sidebar_notifications`         | `on`    | Master switch for desktop notifications                                                 |
| `@sidebar_notifications_events`  | unset   | Restrict events — see [Notifications](/tmux-agent-sidebar/features/notifications/)       |
| `@sidebar_pet`                   | `off`   | Show the animated pet in a 5-row band above the bottom panel                            |
| `@sidebar_hook_check_agents`     | `codex` | Comma-separated agents whose hook setup is checked; add `claude` when that integration is used |

## Worktree spawn defaults

| Option                            | Default     | Description                                               |
| --------------------------------- | ----------- | --------------------------------------------------------- |
| `@agent-sidebar-default-agent`    | `codex`     | Agent launched by `n`&nbsp;(also accepts `claude`)        |
| `@agent-sidebar-branch-prefix`    | `agent/`    | Branch prefix for new worktrees                           |
| `@agent-sidebar-worktree-dir`     | `.worktrees` | Repo-relative directory for sidebar-spawned worktrees; absolute paths and `..` are rejected |

## Status and filter colors

| Option                            | Default         | What it paints                                                    |
| --------------------------------- | --------------- | ----------------------------------------------------------------- |
| `@sidebar_color_all`              | `#d3869b` | Selected "all" filter icon                                      |
| `@sidebar_color_running`          | `#b8bb26` | Running filter icon and pane status                              |
| `@sidebar_color_background`       | `#8ec07c` | Background-shell filter icon and pane status                     |
| `@sidebar_color_waiting`          | `#fabd2f` | Selected waiting filter icon, waiting pane status, version banner |
| `@sidebar_color_idle`             | `#83a598` | Idle filter icon and pane status                                 |
| `@sidebar_color_error`            | `#fb4934` | Selected error filter icon and error pane status                 |
| `@sidebar_color_filter_inactive`  | `#7c6f64` | Unselected icons and zero counts                                 |

## Structural colors

| Option                     | Default              | What it paints                                                                                          |
| -------------------------- | -------------------- | ------------------------------------------------------------------------------------------------------- |
| `@sidebar_color_border`    | `#504945` | Unfocused panel borders and tab separators                                                              |
| `@sidebar_color_accent`    | `#fabd2f` | Active pane marker, focused repo header, focused bottom panel border, repo popup border, and focused repo `+` |
| `@sidebar_color_session`   | `#bdae93` | Unfocused repository and session headers                                                                |
| `@sidebar_color_selection` | `#504945` | Selected row background                                                                                 |

## Agent colors

| Option                          | Default            | What it paints       |
| ------------------------------- | ------------------ | -------------------- |
| `@sidebar_color_agent_claude`   | `#e78a4e` | Claude brand color   |
| `@sidebar_color_agent_codex`    | `#7daea3` | Codex brand color    |
| `@sidebar_color_agent_opencode` | `#89b482` | OpenCode brand color |

## Text colors

| Option                         | Default          | What it paints                                                                                   |
| ------------------------------ | ---------------- | ------------------------------------------------------------------------------------------------ |
| `@sidebar_color_text_active`   | `#ebdbb2` | Primary text, active rows, nonzero counts, filtered repo label                                    |
| `@sidebar_color_text_muted`    | `#928374` | Secondary text, tree branches, empty-state messages, inactive bottom tabs, activity log labels   |
| `@sidebar_color_text_inactive` | `#7c6f64` | Body text of unfocused pane rows, prompt / response, idle hint                                    |
| `@sidebar_color_port`          | `#7daea3` | Port numbers                                                                                     |
| `@sidebar_color_wait_reason`   | `#fabd2f` | Wait reason text                                                                                 |
| `@sidebar_color_response_arrow`| `#89b482` | Response arrow                                                                                   |

## Task and sub-agent colors

| Option                          | Default           | What it paints        |
| ------------------------------- | ----------------- | --------------------- |
| `@sidebar_color_task_progress`  | `#d8a657` | Task progress summary |
| `@sidebar_color_subagent`       | `#7daea3` | Sub-agent tree        |

## Git tab colors

| Option                          | Default            | What it paints      |
| ------------------------------- | ------------------ | ------------------- |
| `@sidebar_color_branch`         | `#8ec07c` | Git branch name     |
| `@sidebar_color_commit_hash`    | `#928374` | Commit hash         |
| `@sidebar_color_diff_added`     | `#a9b665` | Added diff lines    |
| `@sidebar_color_diff_deleted`   | `#ea6962` | Deleted diff lines  |
| `@sidebar_color_file_change`    | `#d8a657` | File change stats   |
| `@sidebar_color_pr_link`        | `#7daea3` | PR link / number    |

## Section titles and timestamps

| Option                                | Default      | What it paints      |
| ------------------------------------- | ------------ | ------------------- |
| `@sidebar_color_section_title`        | `#bdae93` | Section titles      |
| `@sidebar_color_activity_timestamp`   | `#7c6f64` | Activity timestamps |

## Status icons

Any Unicode glyph works. The defaults use Nerd Font glyphs.

| Option                     | Default | Meaning                       |
| -------------------------- | ------- | ----------------------------- |
| `@sidebar_icon_all`        | ``      | Status filter bar "all" icon  |
| `@sidebar_icon_running`    | ``      | Running status icon           |
| `@sidebar_icon_background` | ``      | Background shell status icon  |
| `@sidebar_icon_waiting`    | ``      | Waiting status icon           |
| `@sidebar_icon_idle`       | ``      | Idle status icon              |
| `@sidebar_icon_ready`      | ``      | Completed response to review  |
| `@sidebar_icon_reviewing`  | ``      | Response currently reviewing  |
| `@sidebar_icon_error`      | ``      | Error status icon             |
| `@sidebar_icon_unknown`    | ``      | Unknown status icon           |

## Example config

```bash
# Behavior
set -g @sidebar_key T
set -g @sidebar_close_key M-A
set -g @sidebar_width 32
set -g @sidebar_position right
set -g @sidebar_bottom_height 25
set -g @sidebar_notifications_events "stop,notification"
set -g @sidebar_hook_check_agents "codex,claude" # opt into Claude checks when used
set -g @agent-sidebar-default-agent codex

# Colors
set -g @sidebar_color_accent 117
set -g @sidebar_color_agent_claude "#d97757"
set -g @sidebar_color_agent_opencode 39

# Icons
set -g @sidebar_icon_running '▶'
set -g @sidebar_icon_error   '⚠'

run-shell ~/.config/tmux/plugins/tmux-agent-sidebar/tmux-agent-sidebar.tmux
```
