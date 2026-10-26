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
| `h` / `Left`   | Previous status filter (header focus), the Activity block (from the agent list), or the other tab block (tabs focused) |
| `l` / `Right`  | Next status filter (header focus), the Git block (from the agent list), or the other tab block (tabs focused)          |
| `u`            | Cycle all local Usage windows; align mixed windows to today (also works without a header) |
| `y`            | Copy the selected command from the focused Activity tab                       |
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

The Activity tab keeps one row per entry, and the entry under the cursor wraps
as far as it needs to so a long command can be read in full — whether or not the
tabs have the focus. Shell commands are highlighted by the bash grammar.
`j` / `k`, `Ctrl+d` / `Ctrl+u`, `gg` / `G`, and the wheel move the cursor —
marked with `┃` on the left — and `y` copies the cursor's command to the OS
clipboard, the tmux paste buffer, and the terminal (OSC 52). `y` works whenever
the footer has the keyboard, not only while the movement keys are on Activity.

`A` and `M-A` are defaults, not fixed keys. Set `@sidebar_key` and
`@sidebar_close_key` before loading the plugin to change or disable them.

## Local Usage

Click `Usage` to cycle all local usage rows through today, 24h, 7d, and 30d.
Click only the `ds…` or Codex recent `cx…` window label to change that provider. The
header shows the common window when rows agree, and only `Usage` when they
differ; clicking a mixed header returns both to today. The Codex quota-cycle
label remains independent and opens details. Clicking a subscription quota row
requests a quota refresh. Plain `u` does not override text input or `Ctrl+u`.
Click an eligible monetary field to toggle only its row between total and daily
average (`/d`). Today/24h, unpriced amounts and unavailable cycles do not toggle.
Tokens, cache rate, field padding, and the right-hand DS fire/leaf price signal
are informational and do not switch windows or request a refresh.

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
