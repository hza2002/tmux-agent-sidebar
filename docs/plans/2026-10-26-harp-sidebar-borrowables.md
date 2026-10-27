# Reference: Harp Sidebar Features and Fork Candidates

> Historical research, archived at closeout on 2026-10-27. The original
> discussion below is preserved. Selected workflow timers and idle folds shipped in
> `v0.16.0-hza.1`; the [accepted decision](../decisions/2026-10-26-workflow-timers-and-idle-folds.md) describes the implemented
> scope. Unselected alternatives are not outstanding release requirements.

> Status: draft portfolio for independent review, 2026-10-26.
>
> This document is not a decision record. It is a backlog of independent
> candidates, not one design. The reviewer is asked to verify the facts,
> challenge the value claims, and decide whether any candidate deserves its own
> design document. Each candidate can be accepted, deferred, or rejected on its
> own.

## Reviewer Brief

The source is [`dominikmartn/harp`](https://github.com/dominikmartn/harp), an
unofficial macOS fork of Warp whose vertical tab bar is built for coding agents.
It is not a tmux plugin and not a sibling implementation of this repository, so
its UI and persistence choices do not transfer directly. The useful material is
its data sources and a small number of state-model details.

The repository's current sidebar already covers most of what Harp advertises:
panes across sessions and windows, live metadata, worktrees, notifications,
quota, an activity panel, a Git panel, and a tmux-native configuration model.
This document therefore focuses on gaps, not parity.

Evaluate each candidate against:

1. User value: does it change a decision the user makes while working?
2. Evidence: is the behavior observed, documented, or only inferred?
3. Seam: does it fit an existing module and contract, or create a parallel
   model?
4. Merge cost: does it add fork-only surface that must be reapplied on every
   upstream sync?
5. Test cost: can it be covered with fixtures and snapshots without a live
   tmux server?
6. Risk: can it misreport state, lose data, or rewrite user configuration?

Status vocabulary used below:

- **Candidate**: worth a design pass when the user wants it.
- **Deferred**: a real feature, but a prerequisite is missing.
- **Needs evidence**: value depends on observed usage.
- **Rejected**: not a fit for this repository, with a revisit condition.

## Contents

- [What Harp Is](#what-harp-is)
- [Feature Inventory](#feature-inventory)
- [Comparison With This Sidebar](#comparison-with-this-sidebar)
- [Decision Inputs](#decision-inputs)
- [Candidate Backlog](#candidate-backlog)
- [H1: Claude Subscription Quota](#h1-claude-subscription-quota)
- [H2: Codex Local-Log Quota Fallback](#h2-codex-local-log-quota-fallback)
- [H3: Time-Since-Status-Change Label](#h3-time-since-status-change-label)
- [H4: Hot Reload of Tmux Options](#h4-hot-reload-of-tmux-options)
- [H5: Collapsible Repository Groups](#h5-collapsible-repository-groups)
- [H6: Additional Agent Coverage](#h6-additional-agent-coverage)
- [H7: Background Work at Stop](#h7-background-work-at-stop)
- [Rejected Candidates](#rejected-candidates)
- [Patterns Worth Adopting](#patterns-worth-adopting)
- [Decision Checklist](#decision-checklist)
- [Sources](#sources)
- [License](#license)

## What Harp Is

| Fact | Evidence |
|---|---|
| A macOS application derived from Warp's codebase; its sidebar replaces the tab bar and targets Claude Code and Codex sessions. | Repository README, 2026-10-26. |
| Written in Rust with Cargo; the fork retains Warp crates such as `app`, `warp_core`, `warpui`, and `warpui_core`. | Repository tree and README. |
| AGPL-3.0, except the `warpui` and `warpui_core` crates, which are MIT. | Repository README and license files. |
| The reviewed checkout was `bad92b3`, described as "Harp 0.1.3". The repository was created 2026-10-20 and had about 618 stars on 2026-10-26. | `git log`; GitHub API metadata. |
| Local-first integrations: limit and tmux features read local files and invoke tmux directly; the project adds no servers of its own. | README, "What it adds" and "Settings". |
| Agent support is first-class for Claude Code and Codex, with a Gemini extension and an OpenCode npm plugin. Warp upstream recognizes many more CLI agents by process name. | `app/src/terminal/cli_agent.rs`; `app/src/terminal/cli_agent_sessions/plugin_manager/`. |

The important difference for this repository: Harp is a GUI terminal whose
sidebar is a list of app tabs. This repository is a TUI that lives inside one
tmux pane and observes every tmux pane. Harp's tab groups, tab order, adoption,
and ring graphics are solutions to GUI-tab problems. Its data sources and state
rules are the portable part.

## Feature Inventory

| Area | Feature | Implementation note |
|---|---|---|
| Row state | Working, needs you, done, idle, shell, pending; unread dot; "moon" for a set-aside tab; background-shell dot; animated working arc; `Cmd+R` rename. | Highest-priority pane in a split wins: needs you > working > done. |
| Groups | Named groups, pinning, drag reorder, collapse, needs-you count in the collapsed header. | Group membership and order are stored on tmux sessions so multiple clients agree. |
| tmux sync | Tab name, group, order, inactive, and marked-unread flags follow the tmux session. A session started elsewhere can request adoption with `@warp_adopt 1`. | Polls every 10 s; layout uses a revision counter to merge writes. |
| Limit rings | Two rings per provider: outer weekly, inner 5-hour. Hover shows numbers and reset times. | Claude reads the status line input; Codex reads session logs; an external JSON file can add providers. No credentials and no network for the rings. |
| Plugin management | Detect install state and minimum version; auto-install/update through the agent's own CLI for Claude, Codex, and Gemini; manual version-pin instructions for OpenCode. | `plugin_manager/` per agent; checks installed version and, for Gemini, verifies the update took effect. |
| Background work | Optional Claude hook reports the count of running background tasks so a finished turn stays "working" until they drain. | Excludes monitors; uses `started_ms` to drop stale reports; waits up to 4 s after Stop. |
| Configuration | `~/.harp/settings.toml`, reloaded when the file changes. | Limit rings, tmux sync, appearance, and a toolbar flag. |
| Distribution | Signed DMG from GitHub Releases; update check every 10 minutes. | Not relevant to a source-built fork. |
| Deliberate subtraction | Search field, view options, and tab-config buttons removed from the framed sidebar. | Reduces UI surface; not a feature to copy. |

## Comparison With This Sidebar

| Capability | This sidebar | Harp | Relevance |
|---|---|---|---|
| Scope | Every tmux pane in every session and window. | Tabs in one app window, with an optional tmux bridge. | Our scope is larger; do not adopt tab-centric models. |
| Grouping | Automatic by git repository, then workflow priority (urgent, ready, working, parked). | User-named groups with manual order and pinning. | Different models; parallel grouping would conflict with priority tiers. |
| Worktrees | Spawn and tear down a worktree, branch, and window from the sidebar. | Not a feature. | We are ahead. |
| Activity and Git | Activity log with copy, Git status tab, PR number. | File tree and editor surfaces inherited from Warp. | Different product; not comparable. |
| Quota | Codex and Kimi subscription windows, plus a DeepSeek spend row. | Codex and Claude rings, plus an external provider file. | H1 and H2 are the relevant gaps. |
| Notifications | Native notifications with per-event settings and duplicate suppression. | Native alerts with similar transition rules. | No clear gap. |
| Hook/plugin setup | Copy-paste setup per agent; notices for missing hooks and Claude plugin cache drift. | One-click install/update where the agent CLI supports it. | Our drift detection is stronger; auto-rewriting config is risky. |
| Background work | Background shell marker with a `ps` liveness sweep; subagent list. | Background task count from Claude's Stop payload. | H7 is a possible correctness gap. |
| Configuration | tmux global and pane options, read at startup. | A settings file that reloads on change. | H4 is the transferable part. |
| UI | Ratatui TUI in a narrow tmux pane. | Native macOS GUI. | Ring graphics and hover cards do not transfer. |

## Decision Inputs

These are current-context facts, not permanent requirements. They should be
rechecked when a candidate is picked up.

- At review time the user has no Claude Pro/Max subscription. Claude's
  `rate_limits` object is absent for API-key usage, so H1 would render no row.
- The Kimi quota row is disabled by `@sidebar_quota_kimi off` after the
  subscription ended in 2026-10; the row is available if that changes.
- The DeepSeek spend row exists and is computed from local session logs by
  `src/usage.rs`; it is not a subscription quota.
- The fork budget favors a configuration option, an existing extension seam, or
  a leaf module over a new subsystem.

## Candidate Backlog

| ID | Candidate | Status | Value | Effort | Risk | Trigger |
|---|---|---|---|---|---|---|
| H1 | Claude subscription quota via status line | Deferred | Medium now, high with a subscription | Medium | Medium | A Claude Pro/Max plan |
| H2 | Codex local-log quota fallback | Candidate | Low-medium | Low | Low | Frequent API failures or offline use |
| H3 | Time-since-status-change label | Candidate | Low-medium | Low | Low | The current time label proves unhelpful |
| H4 | Hot reload of tmux options | Candidate | Medium | Medium | Medium | Frequent option changes without restart |
| H5 | Collapsible repository groups | Needs evidence | Low-medium | Medium | Medium | Many repositories or panes |
| H6 | Additional agent coverage | Trigger-based | Depends on the agent | Medium | Low | The user starts using that agent |
| H7 | Background work at Stop | Separate review | Medium-high | Medium | Medium-high | See the companion reference |

## H1: Claude Subscription Quota

**Status: Deferred.** The prerequisite is a Claude Pro/Max subscription, or a
gateway that supplies a spend limit. Without it, Claude Code does not put
`rate_limits` in the status line input, so there is no data to display.

### What Harp does

- It reads Claude Code's status line input, not a network API. The input
  contains `rate_limits.five_hour` and `rate_limits.seven_day`, each with
  `used_percentage` and `resets_at`.
- Its status line command stores that input in
  `~/.harp/claude-statusline.json` and prints nothing. The write uses a
  per-process temporary file and `mv`, so concurrent Claude sessions cannot
  expose a half-written file.
- On first run it adds a `statusLine` entry to `~/.claude/settings.json` only
  when the file parses, has no `statusLine`, and `~/.claude` exists. It marks
  the install so a status line the user later removes stays removed. It inserts
  the entry as text so the rest of the file keeps its order and formatting.
- The rings treat a reading as fresh until its window resets. The README notes
  that used shares only grow within a window, so an old number can be low, never
  high.

### Why this is not "the Codex quota with another endpoint"

| Dimension | Codex (current implementation) | Claude (Harp pattern) |
|---|---|---|
| Data source | `GET https://chatgpt.com/backend-api/wham/usage`, with `codex app-server` as a fallback. | A local JSON file written by Claude Code's status line command. |
| Credentials | Reads `~/.codex/auth.json`; refreshes the OAuth token and writes rotated tokens back. | None. |
| Availability | Pollable whenever the credentials are valid, even if Codex is not running. | Only after a Claude response; only for claude.ai Pro/Max, or a gateway with a spend limit. |
| Windows | 5-hour and weekly, classified by `limit_window_seconds`. | `five_hour` and `seven_day`, each independently absent. |
| Value | `used_percent`, converted to remaining percentage. | `used_percentage`, converted to remaining percentage. |
| Freshness | Adaptive polling: 90 s while Codex is active, 5 min idle, pulled forward at a reset boundary. | File mtime; as fresh as the last Claude response. |
| Failure states | Auth, throttling, network, schema drift, missing credentials. | No file, no limits for API-key plans, stale file, malformed JSON. |
| Configuration | None; credentials imply the subscription. | A user-level `statusLine` entry or a wrapper around an existing one. |
| Value today | Works. | None without a subscription. |

Claude Code does not document an equivalent usage endpoint that the sidebar
could call. The status line is the only place it exposes plan limits. That makes
H1 a different integration with different guarantees, not a copy of the Codex
fetcher.

A gateway spend limit is a third case: `rate_limits.spend_limit` carries
`used_percentage`, `resets_at`, and optionally `used_usd`, `limit_usd`, and
`period`. That is a spend metric, not the 5-hour/weekly subscription windows,
and would need its own row semantics.

### Design sketch if activated

1. Add `integrations/claude-statusline.sh` (or a subcommand of the existing
   binary) that reads stdin, writes atomically to
   `${XDG_CACHE_HOME:-$HOME/.cache}/tmux-agent-sidebar/claude-statusline.json`,
   and prints nothing. It must not block Claude's rendered status line.
2. Add `src/quota/claude.rs` that reads the file, maps
   `100 - used_percentage` to `remaining_percent`, ignores windows whose
   `resets_at` is in the past, and reports no data when `rate_limits` is absent.
3. Add `Subscription::Claude` to `src/quota/mod.rs`, an enabled flag from
   `@sidebar_quota_claude`, and rendering in `src/ui/panes/quota.rs`. The Kimi
   row's existing slot and style are the closest precedent.
4. Decide freshness explicitly: use the file mtime for the age marker; a
   reading inside an unexpired window is usable, but the UI should show its age
   because it only updates when Claude runs.
5. Prefer an explicit `setup` command or a documented snippet over silently
   editing `~/.claude/settings.json`. Claude Code plugins cannot ship a
   `statusLine` setting; the manifest's `settings` field only applies `agent`
   and `subagentStatusLine`. A user who already has a status line needs a
   wrapper such as `tee file | existing-command`.

### Risks and constraints

- The status line is user configuration. Silent changes are hard to audit and
  may surprise the user.
- An existing status line must be preserved. Replacing it would break another
  workflow.
- `rate_limits` appears only after the first response and is absent for API-key
  plans and for windows the provider has dropped.
- `used_percentage` is a vendor snapshot, not a billed amount. It should not
  share a scale with the DeepSeek spend row.
- Multiple Claude sessions write the same file; the atomic temp-and-rename
  pattern is required.
- The data is only as fresh as the last Claude response. A row that shows
  remaining quota must also show its age.

### Decision trigger

A Claude Pro/Max subscription. At that point, open a design document that
includes the status-line install UX, the stale rules, and a snapshot of the row
at full and compact widths.

## H2: Codex Local-Log Quota Fallback

**Status: Candidate.** Low effort, low risk, useful only when the existing
fetchers fail.

### What Harp does

- It looks at the newest three rollout files under
  `$CODEX_HOME/sessions/YYYY/MM/DD/`.
- It keeps lines containing `rate_limits`, parses each line, and classifies a
  window by `window_minutes`: at most 360 minutes is the 5-hour window, at least
  1440 minutes is the weekly window. The last value per window wins.
- It also copies `plan_type` into a display tier.

### Fit with the current Codex fetcher

`src/quota/codex.rs` already fetches the vendor endpoint and falls back to
`codex app-server`; it classifies windows by `limit_window_seconds`, which is
the same rule in different units. A log fallback would be a third path, used
only when both existing paths fail. It reads local files, touches no
credentials, and can display a stale reading instead of a `--` placeholder.

The existing `src/usage.rs` already walks Codex rollout files for the DeepSeek
row, but that scanner is optimized for token accounting and filters by model.
The quota fallback should stay inside `src/quota/codex.rs` rather than couple
the two features.

### Risks and constraints

- The log only contains readings from responses; it may lag the vendor and may
  omit a window that was not active.
- The rollout format is internal and can change. The parser needs a drift
  signal and a test fixture.
- Only the newest reading should be used; resumed or forked sessions repeat
  history.
- A stale reading is still a reading. The UI must show its age and must not
  present it as a fresh fetch.

### Decision trigger

Repeated vendor-endpoint failures, offline use, or a desire to avoid spawning
`codex app-server`. If none of these occurs, the existing two paths are enough.

## H3: Time-Since-Status-Change Label

**Status: Candidate.** Small, local, and easy to reverse.

The current row shows elapsed time from `pane.started_at`
(`src/ui/panes/row/status.rs:39`). `started_at` is written on prompt submit and
cleared on Stop. Harp shows time since the last status change, formatted as
`now`, `5m`, `3h`, or `2d`.

| Option | Behavior | Risk |
|---|---|---|
| A | Keep time since start for every row. | Answers "how long has this turn run", not "how long has it waited". |
| B | Running rows keep time since start; waiting, error, and done rows use `status_changed_at`. | Two meanings in one column; needs a clear header or color cue. |
| C | Add a config option to choose the meaning. | More surface for a small feature. |

`status_changed_at` already exists and is used for repository ordering. The
change is local to the row renderer and its snapshots. The main product
question is whether the user reads the number as turn duration or as waiting
duration.

### Decision trigger

The user reports that the time label is not the number they want while triaging
panes.

## H4: Hot Reload of Tmux Options

**Status: Candidate.** Medium effort, but a quality-of-life improvement that
fits the tmux-native configuration model.

The sidebar reads several options once at startup: theme, icons, band, quota,
Kimi quota, and bottom-panel height (`src/ui/mod.rs:45-106`). Changing
`@sidebar_quota`, `@sidebar_band`, or `@sidebar_pet` therefore requires
restarting the sidebar. Harp reloads `settings.toml` when the file changes.

| Option | Mechanism | Trade-off |
|---|---|---|
| A | Re-read a small option subset every 5–10 s in the refresh loop. | One extra tmux call; needs layout reconciliation when a block appears or disappears. |
| B | Re-read on `SIGUSR1`, alongside the existing global-state reload. | No polling, but the user must send a signal. |
| C | Full configuration reload, including theme and icons. | Largest surface; more snapshot and test churn. |

The difficult part is not reading the option; it is recomputing the filler
layout when quota, band, or pet visibility changes mid-session. Starting with
one option, such as the quota toggle, would contain the risk.

### Decision trigger

Frequent option changes, or a user complaint that a `set -g @sidebar_*` change
did nothing until restart.

## H5: Collapsible Repository Groups

**Status: Needs evidence.** Harp's collapsed group header keeps a needs-you
count, which is a useful pattern for a narrow sidebar.

This sidebar renders a plain repository header with a `[+]` spawn button
(`src/ui/panes/row_collector.rs:43-66`) and no collapse state. A collapsed group
would hide member rows but keep the header and a count.

| Concern | Consideration |
|---|---|
| Hidden urgency | A collapsed group must still show urgent, ready, and needs-you counts. |
| State persistence | Collapse state should live in a tmux global option keyed by the stable repository identity, not the display name. |
| Input | A key binding and click target are needed; the header already has a spawn button. |
| Layout | Row collection, scrolling, click targets, and snapshots all change. |
| Value | Depends on the number of repositories and panes the user actually keeps open. |

### Decision trigger

The agent list regularly exceeds the pane height because of parked
repositories, and filtering by status or repository is not sufficient.

## H6: Additional Agent Coverage

**Status: Trigger-based.** Harp supports Gemini through a CLI extension and
inherits Warp's broad process-name detection. This repository has adapters for
Claude, Codex, OpenCode, and Kimi; Kimi is currently phased out.

Adding an agent is not a copy of a Harp feature. It needs a hook or plugin
declaration, an adapter, setup output, drift tests, and documentation. The
existing extension seam makes it tractable, but there is no value without a
user of that agent.

### Decision trigger

The user starts using an agent that the sidebar does not track.

## H7: Background Work at Stop

**Status: Separate review.** See
[`2026-10-26-claude-background-tasks-reference.md`](2026-10-26-claude-background-tasks-reference.md).
That document covers the `background_tasks` and `session_crons` question, the
current synchronous-subagent assumption, and the options for a deferred
completion transition.

## Rejected Candidates

| Candidate | Why rejected | Revisit if |
|---|---|---|
| Limit-ring graphics | A TUI quota block already collapses full -> compact -> hidden; rings are a GUI affordance. | The quota block moves to a graphical client. |
| Named tab groups, manual order, layout revision protocol | Repository grouping and workflow priority are derived, not user-managed. A second grouping model would conflict with the tiers and the one-directional state flow. | The user wants manual grouping that cannot be expressed by repository or status. |
| `@warp_adopt` session adoption | The sidebar already sees every tmux pane; there is no separate app-tab set to adopt. | The sidebar stops observing all panes. |
| One-click plugin install/update and automatic config rewrite | The current setup is copy-paste and the drift notices are stronger (file-content comparison). Rewriting user config adds risk and a new policy surface. | A safe merge/rollback design exists and the user asks for it. |
| GitHub-release updater | The fork is built from source and resolves the live release binary. | The distribution model changes. |
| External quota provider JSON file | The quota block has a fixed, small set of subscriptions. A generic provider file adds abstraction without a demonstrated third provider. | A third non-Codex, non-Claude provider becomes important. |
| `settings.toml` instead of tmux options | tmux options are the native configuration surface and already participate in the global-state sync. | Configuration needs structure that tmux options cannot express. |
| Search field and view controls | Harp removed them on purpose; this sidebar already has status and repository filters. | The pane list grows beyond what filters can narrow. |
| Phone-friendly tmux tab flags | This sidebar is already inside tmux, so every tmux client sees the same pane list. | The sidebar becomes a separate GUI client. |

## Patterns Worth Adopting

These are small engineering patterns, independent of any single feature.

- **Local-first data sources.** Prefer a file or log the agent already writes
  over a new credential or network call. The Claude status line and the Codex
  rollout fallback are examples.
- **Atomic file writes.** Write to a per-process temporary file and rename, so
  concurrent hook or agent processes never expose a partial file.
- **Absent, stale, failed, and unknown are distinct.** A missing reading is not
  a zero, and an old reading is not a fresh one. The existing quota state
  already models this distinction; new sources should reuse it.
- **Exclude long-lived monitors.** Work that is designed to run forever should
  not keep a row "working" indefinitely. Harp learned this the hard way.
- **Preserve user configuration text.** If a future setup flow edits
  `settings.json`, insert the entry as text, keep the rest of the file intact,
  and record that the install happened so a user's removal stays removed.
- **One failure mode per test.** Harp's limit-source module lists its known
  failure modes in a doc comment and pins each with a named test. This
  repository already follows the same style; keep it.

## Decision Checklist

Use this template when a candidate is picked up.

```text
Candidate:
User value:
Evidence needed:
Chosen seam:
Files and contracts touched:
Fork/merge cost:
Test cost:
Rollback:
Risk:
Decision: accept | defer | reject
Date:
```

For H1 specifically, the first line of the decision should be whether a Claude
Pro/Max subscription exists. Without it, the candidate stays deferred regardless
of implementation quality.

## Sources

Harp:

- Repository and README:
  <https://github.com/dominikmartn/harp>
- Limit-rings file format:
  <https://github.com/dominikmartn/harp/blob/main/docs/limit-rings.md>
- Claude Code background-tasks hook:
  <https://github.com/dominikmartn/harp/blob/main/integrations/claude-code/README.md>
- Limit sources implementation:
  <https://github.com/dominikmartn/harp/blob/main/app/src/workspace/view/vertical_tabs.rs>
- Plugin managers:
  <https://github.com/dominikmartn/harp/tree/main/app/src/terminal/cli_agent_sessions/plugin_manager>
- CLI agent session state machine:
  <https://github.com/dominikmartn/harp/blob/main/app/src/terminal/cli_agent_sessions/mod.rs>

Claude Code:

- Status line input fields:
  <https://code.claude.com/docs/en/statusline>
- Plugin manifest and `settings` limits:
  <https://code.claude.com/docs/en/plugins-reference>
- Hooks reference:
  <https://code.claude.com/docs/en/hooks>

Local:

- `src/quota/mod.rs`, `src/quota/codex.rs`, `src/quota/kimi.rs` — current quota
  state, fetchers, cadence, and failure handling.
- `src/usage.rs` — DeepSeek spend row and local session-log scanning.
- `src/ui/panes/quota.rs` — quota rendering and collapse levels.
- `src/ui/mod.rs` — startup option reads.
- `src/ui/panes/row/status.rs` — elapsed-time label.
- `src/ui/panes/row_collector.rs` — repository headers and row collection.
- `docs/state-management.md` — state scope, options, and cadence.
- `docs/maintainers/fork-strategy.md` — seam and fork-budget rules.

## License

Harp is AGPL-3.0, with the `warpui` and `warpui_core` crates under MIT. This
repository is MIT. Do not copy Harp source into this repository without a
license review. The candidates above are descriptions of behavior and design;
they should be reimplemented against this repository's own modules and tests.
