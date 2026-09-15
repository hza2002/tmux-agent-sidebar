---
title: Keybindings
description: Every shortcut in the sidebar, the worktree spawn modal, and the close-pane modal.
---

## Sidebar

| Key            | Action                                                        |
| -------------- | ------------------------------------------------------------- |
| `prefix + A`   | Open/summon the singleton sidebar; close it when pressed inside the sidebar |
| `prefix + M-A` | Close the singleton sidebar from any pane                     |
| `j` / `Down`   | Move selection down                                           |
| `k` / `Up`     | Move selection up                                             |
| `h` / `Left`   | Previous status filter (when the header has focus)                                        |
| `l` / `Right`  | Next status filter (when the header has focus)                                            |
| `/`            | Open repo filter popup                                        |
| `gg`           | Jump to the first pane, or the top of the focused bottom panel |
| `G` (`Shift+g`) | Jump to the last pane, or the end of the focused bottom panel |
| `Ctrl+u` / `Ctrl+d` | Move up/down half the visible panel height |
| `Enter`        | Jump to the selected pane                                     |
| `Tab`          | Cycle status filter                                           |
| `Shift+Tab`    | Cycle status filter backward                      |
| `Esc`          | Return focus or close the popup                               |

The selected status filter has a background highlight. `r` also opens the repo
filter when the header has focus. Existing `Ctrl+n` / `Ctrl+p` aliases move
down/up. `j` / `k` cross between the header, pane list, and bottom panel at
boundaries; fast navigation stays within the target panel. `gg` requires two
consecutive presses within one second. In text inputs, `g` and `G` remain text.

`A` and `M-A` are defaults, not fixed keys. Set `@sidebar_key` and
`@sidebar_close_key` before loading the plugin to change or disable them.

## Repo filter popup

Opened with `r`, `/`, or by clicking the repo filter button in the sidebar header.

| Key                 | Action                                 |
| ------------------- | -------------------------------------- |
| Type                | Search repository names                |
| `Down` / `Ctrl+n`   | Move selection down                    |
| `Up` / `Ctrl+p`     | Move selection up                      |
| `Backspace`         | Delete the last search character       |
| `Enter`             | Confirm — filter the list to that repo |
| `Esc`               | Cancel                                 |

## Notices popup

Opened by clicking the colored status indicator shown when hooks, plugin setup, or an update needs attention.

| Key   | Action          |
| ----- | --------------- |
| `Esc` | Close the popup |

## Worktree

| Key | Action                                 |
| --- | -------------------------------------- |
| `n` | Spawn a new worktree + agent           |
| `x` | Remove the selected spawn-created pane |

## Spawn worktree modal

Opened with `n` on a repo.

| Key                                | Action                                                                                           |
| ---------------------------------- | ------------------------------------------------------------------------------------------------ |
| Text keys                          | Type the name (used as the branch slug and tmux window name)                                     |
| `↑` / `↓` / `Tab` / `Shift+Tab`    | Move focus between `NAME` / `AGENT` / `MODE` fields                                              |
| `←` / `→`                          | Cycle the value when the agent or mode field has focus                                           |
| `Enter`                            | Create the worktree + window and launch the agent                                                |
| `Esc`                              | Cancel                                                                                           |

## Close pane modal

Opened with `x` on a spawn-created pane.

| Key             | Action                                                                                                    |
| --------------- | --------------------------------------------------------------------------------------------------------- |
| `y` / `Enter`   | Close the tmux window, remove the git worktree (`--force`), and delete the branch (`git branch -D`)       |
| `c`             | Close the tmux window only, keep the worktree and branch on disk                                          |
| `n` / `Esc`     | Cancel                                                                                                    |
