# Reference: Claude Code Background Work at Stop

> Historical research, archived at closeout on 2026-10-27. The original
> discussion below is preserved. Background-work handling shipped in
> `v0.16.0-hza.1`; the [accepted decision](../decisions/2026-10-26-claude-background-work.md) describes the implemented
> scope. Unselected alternatives are not outstanding release requirements.

> Status: draft for independent architecture review, 2026-10-26.
>
> This document is not a decision record. It separates verified facts,
> assumptions, options, and a provisional position so a reviewer can challenge
> any layer independently. The reviewer is asked to find missing options,
> invalid assumptions, and weaker-than-claimed guarantees before a decision
> record is written.

## Reviewer Brief

Review the sidebar's `Stop` transition for Claude Code panes. The current
handler assumes that a parent `Stop` means no child or background work remains.
Claude Code's `Stop` and `SubagentStop` hook input now includes
`background_tasks` and `session_crons`, which describe work still in flight when
the parent turn ends. The question is whether the sidebar should consume those
fields, and if so, what the smallest correct state machine is.

Please pay particular attention to:

1. Whether a `Stop` with running `background_tasks` is a real state for the
   Claude Code versions and plans in use, or only a theoretical one.
2. Whether the product state while background work runs should be `running`,
   `background`, or `waiting` with a badge.
3. Whether the existing `@pane_bg_cmd`, `@pane_subagents`, completion stamp,
   task-reset marker, notification stamps, and deferred worktree teardown
   already form a state machine that can absorb the new information, or whether
   a parallel model is needed.
4. Which completion signals actually exist for each task type
   (`subagent`, `shell`, `workflow`, `teammate`, `cloud session`, `MCP task`,
   `monitor`), and which types can leave a row stuck if their drain event never
   arrives.
5. Whether the proposed tests can be derived from captured payloads without
   connecting to the developer's live tmux server.

## Contents

- [Problem](#problem)
- [Goals](#goals)
- [Non-Goals](#non-goals)
- [Repository Constraints](#repository-constraints)
- [Verified Facts](#verified-facts)
- [Assumptions and Unverified Claims](#assumptions-and-unverified-claims)
- [Scenario Matrix](#scenario-matrix)
- [Options](#options)
- [Comparison Matrix](#comparison-matrix)
- [Selection Guide](#selection-guide)
- [Provisional Position](#provisional-position)
- [Open Questions for the Reviewer](#open-questions-for-the-reviewer)
- [Validation Plan](#validation-plan)
- [Sources](#sources)
- [Glossary](#glossary)

## Problem

`on_stop` in `src/cli/hook/handlers/run.rs:49` is the single completion
transition for Claude Code turns. It currently does the following:

1. Rejects mismatched session ids, duplicate completions, and turn ids that do
   not match the active turn (`src/cli/hook/handlers/run.rs:58-79`).
2. Writes prompt, attention, and agent metadata.
3. Clears `@pane_subagents` on the stated assumption that Claude Code `Task`
   subagents are synchronous (`src/cli/hook/handlers/run.rs:85-90`).
4. Records whether a background shell is live from `@pane_bg_cmd`.
5. Sets `waiting` with `response_ready`, writes the task-reset marker, and
   publishes the completion stamp (`src/cli/hook/handlers/run.rs:91-107`).
6. Fires a `TaskCompleted` desktop notification unless a background shell is
   live or a `TaskCompleted` stamp already fired for this run
   (`src/cli/hook/handlers/run.rs:109-136`).

The Claude adapter registers `Stop` and `SubagentStop`
(`src/adapter/claude/mod.rs:73`, `:98`) but parses neither `background_tasks`
nor `session_crons` (`src/adapter/claude/mod.rs:167`). Claude Code's hook
reference says those arrays exist precisely to distinguish "session is done"
from "session is paused waiting for background work to wake it back up". If
that state occurs in practice, the current handler reports a finished turn and
notifies the user while work is still running.

The problem statement is deliberately narrow: this is about the visible state,
the completion notification, and any teardown that keyed off completion. It is
not a request to model every Claude Code task type in the sidebar.

## Goals

- The visible row matches the actual session state after a parent `Stop`:
  working while work still runs, ready-for-review when it drains.
- Completion notifications fire when the user actually needs to look, not when
  the parent turn merely paused.
- Task progress, the subagent tree, and deferred worktree teardown stay coherent
  across the pause.
- The behavior degrades to today's behavior when the fields are absent, empty,
  malformed, or belong to an older Claude Code version.
- The change stays on the existing seam: adapter -> `AgentEvent` -> handler ->
  tmux pane option -> query -> UI.
- The logic is testable with captured payloads and the tmux test mock; no test
  touches the developer's live tmux server.

## Non-Goals

- Rendering every task type as a distinct UI element.
- Replacing the background-shell model (`@pane_bg_cmd`) or the subagent list
  (`@pane_subagents`) with a general task registry.
- Changing Codex, OpenCode, or Kimi semantics.
- Reconstructing Claude Code's task registry from transcripts when the hook
  payload is missing.
- Solving notification aggregation or sound policy.

## Repository Constraints

These constraints come from the repository instructions and current
architecture, not from a preference:

- Normalization belongs in `src/adapter/claude/`; handlers consume
  `AgentEvent`, not raw agent JSON (`docs/maintainers/architecture-map.md`).
- Pane metadata is a contract across `src/tmux/options.rs`, the hook writer and
  cleanup, `src/tmux/query.rs`, `docs/state-management.md`, and snapshots. A new
  option must move all of those together.
- Hook writes are serialized per pane by `src/pane_lock.rs`, but hook processes
  still run concurrently and may finish out of order.
- Rendering reads the tmux snapshot; it must not spawn processes or perform I/O.
- Unit tests must not connect to a live tmux server. Tests that render a frame
  use inline `insta` snapshots.
- Files under `docs/` are written in English.
- A change that alters core data flow or three or more hotspots needs a
  decision record before implementation.

## Verified Facts

### Claude Code hook contract

Accessed from <https://code.claude.com/docs/en/hooks> on 2026-10-26.

| Fact | Evidence |
|---|---|
| `Stop` input includes `stop_hook_active`, `last_assistant_message`, `background_tasks`, and `session_crons`, in addition to the common input fields. | Hooks reference, "Stop input". |
| `SubagentStop` receives the same `background_tasks` and `session_crons` arrays, scoped to the parent session, not to the subagent. | Hooks reference, "SubagentStop input". |
| Both arrays are present when the task registry is reachable and are empty when nothing is in flight or scheduled. | Hooks reference, "Stop input". |
| The documented purpose is to distinguish "session is done" from "session is paused waiting for background work to wake it back up". | Hooks reference, "Stop input". |
| `background_tasks[]` fields are `id`, `type`, `status`, `description`, `command` (shell only), `agent_type` (subagent only), `server` and `tool` (monitor and MCP task only), and `name` (workflow only). | Hooks reference, `background_tasks[]` entries table. |
| `type` labels are `shell`, `subagent`, `monitor`, `workflow`, `teammate`, `cloud session`, and `MCP task`; an unrecognized type falls back to the raw discriminant. | Same table. |
| `status` is documented only as "Current task status". The table does not enumerate values; the example shows `"running"`. | Same table and example payload. |
| `session_crons[]` fields are `id`, `schedule`, `recurring`, and `prompt`. They are sourced from `CronCreate`, `ScheduleWakeup`, and `/loop`. | Hooks reference, `session_crons[]` entries. |
| Example `Stop` input includes one running `shell` task and one recurring `session_crons` entry. | Hooks reference, "Exact example shape". |
| `Stop` does not run when the user interrupts; API errors fire `StopFailure` instead. | Hooks reference, "Stop". |

The reference does not state which status values other than `running` are
possible, whether completed tasks can remain in the array transiently, or how
often a `Stop` occurs while a `subagent` task is still running. Those are
empirical questions for the validation plan.

### Harp's independent implementation

Harp is an unrelated Warp fork that solved a similar problem for its own agent
sidebar. It is evidence that the state exists, not an authority on the
contract.

| Fact | Evidence |
|---|---|
| Harp ships an optional Claude Code hook because its agent plugin's `background_task_count` is unreliable for this purpose. The hook sends the real running count from `background_tasks`. | `integrations/claude-code/README.md` and `warp-background-tasks.sh` in <https://github.com/dominikmartn/harp>. |
| The hook keeps only entries whose `status` is `running`, excludes `type == "monitor"`, and scans the transcript for `Monitor` tool results because some Claude Code versions label monitors as `shell`. | `integrations/claude-code/warp-background-tasks.sh`. |
| The hook sends `started_ms`; the app drops a report older than the newest one it applied. | Same script and README protocol section. |
| A `Stop` that arrives before the background report waits up to 4 seconds, then counts as done. A count above zero keeps the row "working"; the done notification fires once the count reaches zero. | `integrations/claude-code/README.md`. |
| A finished turn with background work still running is displayed as working. | `app/src/terminal/cli_agent_sessions/mod.rs`, `CLIAgentSessionStatus::displayed`. |

Harp's implementation adds a second hook because its plugin's own aggregate
count was lossy. The sidebar already receives `Stop` and `SubagentStop`
directly, so it may not need a second hook to read the same arrays. That is a
design question, not a fact.

### Current sidebar behavior

| Fact | Evidence |
|---|---|
| `Stop` and `SubagentStop` are registered for Claude. | `src/adapter/claude/mod.rs:73`, `:98`. |
| The Claude adapter parses `stop` into `AgentEvent::Stop` without reading `background_tasks` or `session_crons`. | `src/adapter/claude/mod.rs:167`. |
| `on_stop` clears `@pane_subagents`, sets `waiting`/`response_ready`, writes the task-reset marker, publishes the completion stamp, and conditionally notifies. | `src/cli/hook/handlers/run.rs:49-136`. |
| `on_subagent_start` appends `type:id` to `@pane_subagents`; `on_subagent_stop` removes one id and drains pending teardowns only when a non-empty list becomes empty. | `src/cli/hook/handlers/subagent.rs:5-41`. |
| `remove_subagent` returns `None` for an empty list or an unknown id, in which case no teardown is drained. | `src/cli/hook/context/subagents.rs:23-26`. |
| `mark_task_reset` skips while `@pane_subagents` is non-empty. `on_stop` clears the list before calling it, so the guard does not protect a background subagent. | `src/cli/hook/context/meta.rs:115-120`; `src/cli/hook/handlers/run.rs:90-98`. |
| `@pane_bg_cmd` tracks the latest background Bash command and is cleared by a `ps`-based liveness sweep. After review, a pane with a live background shell parks as `background`. | `docs/state-management.md:51`; `src/state/refresh.rs:545-594`. |
| A pending worktree removal is replayed by `on_subagent_stop` only when the subagent list drains to empty. | `src/cli/hook/context/pending.rs:29-45`; `src/cli/hook/handlers/subagent.rs:38-41`. |
| The current test suite pins the synchronous assumption with `on_stop_clears_stale_subagents`. | `src/cli/hook/handlers/run.rs:416`. |

## Assumptions and Unverified Claims

These are not facts. They should be resolved before a decision.

1. The user's Claude Code versions actually emit a `Stop` while a `subagent`
   task is still running. The docs allow it; local usage may never trigger it.
2. `background_tasks` contains only in-flight tasks. The docs say it is empty
   when nothing is in flight, but a finished entry could remain transiently.
3. `status == "running"` is the only active value. Unknown values should be
   treated conservatively rather than as done.
4. A running background subagent always produces a later `SubagentStop`.
   Workflows, teammates, cloud sessions, and MCP tasks may not produce any hook
   the sidebar already consumes.
5. `type` is reliable. Harp's monitor-as-shell workaround suggests it is not
   always.
6. The parent session resumes automatically when background work completes. If
   it does not, `waiting` might be an acceptable display.
7. A pending `session_crons` entry should not keep the row working; it is a
   future wakeup, not active work.
8. Late `SubagentStop` / `TaskCompleted` / `TeammateIdle` events are not rejected
   by the completion stamp or turn-id checks.
9. Task progress should not reset while background work runs; the user may want
   to see the background task's progress.
10. A pending worktree removal should wait for every background task type, not
    only subagents.
11. A lost drain event needs a timeout or refresh fallback; no such fallback
    exists today.
12. Hook processes may interleave; per-pane locking serializes writes but does
    not guarantee ordering between `Stop` and `SubagentStop`.

## Scenario Matrix

"Current result" is what the code does today, assuming `@pane_bg_cmd` is set
only by the activity hook for a background Bash command.

| # | Stop input | Current result | Likely correct? |
|---|---|---|---|
| 1 | Both arrays empty | `waiting` / `response_ready`, notify | Yes |
| 2 | One running `shell` task, `@pane_bg_cmd` live | `waiting` / `response_ready`, no notify; after review -> `background` | Likely yes; the shell model already covers it |
| 3 | One running `subagent` task | Subagent list cleared, `waiting` / `response_ready`, notify, task reset | Likely no |
| 4 | One running `workflow`, `teammate`, `cloud session`, or `MCP task` | Same as 3 | Likely no; drain signal unknown |
| 5 | One running `monitor` only | `waiting` / `response_ready`, notify | Yes if monitors are excluded; wrong if counted as work |
| 6 | Only `session_crons` entries | `waiting` / `response_ready`, notify | Yes if crons are excluded |
| 7 | Fields absent (older Claude Code) | Today's behavior | Yes; backward compatibility |
| 8 | Malformed or unknown entries | Today's behavior, possibly counted if parsing is naive | Must remain conservative |

Scenario 3 also has secondary effects: the next `SubagentStop` sees an empty
list and cannot drain a pending worktree teardown, and the task-reset marker
has already been written.

## Options

### Option 0: No change, document the limitation

Keep the current synchronous assumption. Add a note to the state documentation
that background subagents after a parent `Stop` are displayed as ready.

Strengths: no new state, no migration, no test churn. Weaknesses: knowingly
misreports a state the vendor documents; the notification and task-reset side
effects remain.

### Option 1: Stop-aware one-shot

Parse `background_tasks` and `session_crons` in the Claude adapter. Add a
compact summary to `AgentEvent::Stop` (for example, a running count and a
`has_non_monitor_work` flag). In `on_stop`, when running non-monitor work
exists, keep `running`, do not clear `@pane_subagents`, do not write the
task-reset marker, and do not notify.

Strengths: smallest behavioral delta; uses the existing adapter/handler seam;
no new hook. Weaknesses: no drain transition. If no later event arrives, the
row stays `running` forever. It also suppresses the completion until some
future turn, which may be later than the user expects.

### Option 2: Deferred completion in pane options

Option 1 plus a durable pending-completion record. On `Stop` with running work,
store `@pane_stop_pending` (turn id, completion stamp, a short last-message
fingerprint, start time) and keep `running`. When the last tracked background
task drains (`SubagentStop`, `TaskCompleted`, or `TeammateIdle`), apply the
deferred transition: set `waiting` / `response_ready`, write the task-reset
marker, publish the completion stamp, and notify. A refresh timer expires the
pending record if no drain arrives.

Strengths: correct visible state and notification timing; the pending record
survives hook-process restarts; reuses the existing completion transition.
Weaknesses: new pane option and lifecycle; ordering and duplicate guards are
needed; the timeout value is a product decision; shell tasks already have their
own path and should not be double-counted.

### Option 3: Generic background-task option

Add `@pane_bg_tasks` with a compact encoding of count, kinds, and start time.
Update it from `Stop`, `SubagentStop`, `TaskCompleted`, and `TeammateIdle`;
expire monitors and crons; use a refresh fallback for lost drain events. The UI
may optionally show a count or a dot.

Strengths: handles all task types uniformly; one source of truth; observable.
Weaknesses: larger contract change across options, query, state, UI, and
snapshots; risks becoming a parallel model next to `@pane_bg_cmd` and
`@pane_subagents`; type-specific completion signals are still needed.

### Option 4: Harp-style separate hook report

Add a user hook that emits a count and `started_ms` over OSC 777 or a local
file, and have `Stop` wait briefly for it.

Strengths: independent of the built-in payload; can report a count the sidebar
would otherwise have to infer. Weaknesses: adds a second hook registration and
deployment surface; the arrays are already in the input the sidebar receives;
requires passthrough or a file channel; duplicates the vendor's data.

### Option 5: Process-liveness inference

Infer background work from the pane's process tree and treat any live child as
running.

Strengths: no contract change. Weaknesses: approximate; misses in-process
workflows, teammates, and MCP tasks; may count unrelated children; cannot
distinguish a monitor from a one-shot shell.

## Comparison Matrix

`Strong` / `Adequate` / `Weak` / `Unknown` are qualitative, not scores.

| Criterion | 0 No change | 1 One-shot | 2 Deferred | 3 Generic | 4 Extra hook | 5 Process |
|---|---|---|---|---|---|---|
| Correct state for background subagents | Weak | Weak (may stay running) | Strong | Strong | Strong | Adequate |
| Correct state for other task types | Weak | Weak | Adequate | Strong | Adequate | Weak |
| Notification timing | Weak | Adequate | Strong | Strong | Strong | Adequate |
| Task-reset safety | Weak | Strong (skip) | Strong | Strong | Strong | Unknown |
| Worktree teardown safety | Weak | Adequate | Adequate | Strong | Adequate | Unknown |
| Backward compatibility | Strong | Strong | Adequate | Adequate | Adequate | Strong |
| State and contract complexity | Strong | Adequate | Adequate | Weak | Weak | Adequate |
| Configuration surface | Strong | Strong | Strong | Strong | Weak | Strong |
| Testability with fixtures | Strong | Strong | Strong | Adequate | Adequate | Weak |
| Risk of a parallel model | Strong | Adequate | Adequate | Weak | Weak | Weak |

## Selection Guide

| Dominant priority | Lean toward | Why |
|---|---|---|
| Smallest change, accept a stuck `running` row | 1 | No new durable state |
| Correct visible state for plain background subagents | 2 (subagent-only) | Deferred completion with a fallback |
| Correct state for every task type | 3 | One model for all types |
| No new hook or config surface | 1 or 2 | Uses the existing `Stop` payload |
| Lowest risk until evidence exists | 0 | Document the limitation first |
| Vendor payload proves unreliable | 4 or 5 | Fallback signal rather than inference |

## Provisional Position

No implementation should start until the validation plan captures at least one
real `Stop` with a running `subagent` task. If that state is real and common,
Option 2 with a subagent-only scope is the smallest design that fixes the
visible state, the notification, the task reset, and the deferred teardown
without creating a general task registry. Option 3 is the right target only if
the user actually runs workflows, teammates, cloud sessions, or MCP tasks and
expects those to be represented.

The current clearing of `@pane_subagents` on `Stop` should be treated as an
assumption, not an invariant. The comment in `on_stop` is accurate for
synchronous subagents and inaccurate for any task the vendor keeps running
after the parent turn. The reviewer should decide whether the assumption is
still true for the versions in use before any code changes.

Conditions that would change this position:

- If captures show only background shells, Option 0 is sufficient; the existing
  `@pane_bg_cmd` path already handles them.
- If background subagents are common but every one produces a reliable
  `SubagentStop`, Option 2 can be kept small.
- If workflows or MCP tasks can outlive the parent without any hook the
  sidebar consumes, Option 3 or a timeout-driven fallback is necessary.
- If the built-in arrays prove unreliable across versions, the harp-style
  report in Option 4 becomes a justified second source.

## Open Questions for the Reviewer

1. Which product state is correct while background work runs after `Stop`:
   `running`, `background`, or `waiting` with a badge?
2. Does the user actually use background subagents, workflows, teammates,
   monitors, or cloud/MCP tasks? Which of them matter for this fork?
3. What are all possible `background_tasks[].status` values, and can finished
   entries remain in the array?
4. Do workflows, teammates, cloud sessions, and MCP tasks emit a hook the
   sidebar already consumes when they finish? If not, what is the drain signal?
5. Should a monitor ever affect the row status? Harp says no. Confirm for the
   Claude Code versions in use.
6. Should a pending `session_crons` entry keep the row idle? Confirm.
7. Where should the pending-completion record live: a tmux pane option, an
   activity-log marker, or in-memory state? It must survive hook-process
   restarts and be writable from concurrent hooks.
8. How should duplicate or out-of-order `SubagentStop` / `TaskCompleted` /
   `TeammateIdle` events be handled without double-completing?
9. What timeout or refresh fallback prevents a permanent `running` row when a
   drain event is lost?
10. Should the task-reset marker be deferred with the completion, or written at
    `Stop` even while background work runs?
11. Should a pending worktree removal wait for every task type? How should it
    drain after `Stop` clears the subagent list?
12. How should absent, empty, and malformed arrays differ? Is absent "unknown"
    and empty "nothing running"?
13. Should the completion notification fire at `Stop` or at drain? What body
    should it carry if the last assistant message is older than the drain?
14. Does the status filter, workflow priority, or grouping need a new tier for
    "running background work"?
15. Is a decision record required? This change touches the adapter, handler,
    pane-option contract, state documentation, and tests, so the repository
    rule likely applies.

## Validation Plan

Run these experiments before writing a decision record.

1. **Payload capture.** Register a temporary `Stop` and `SubagentStop` hook
   that appends `{received_at, payload}` to a scratch file, with prompt text
   redacted. Capture at least: a plain turn; a synchronous `Task` subagent; a
   background subagent; a background Bash command; a workflow; a teammate; a
   monitor; and a pending cron. Do not leave the hook installed.
2. **Ordering capture.** Record the arrival order of `Stop`, `SubagentStop`,
   `TaskCompleted`, and `TeammateIdle` for the same session, including delays.
3. **Drain signals.** For each task type, determine which hook or refresh
   signal arrives when it finishes. Record any type with no signal.
4. **Status values.** Record every `status` value observed and whether finished
   entries remain in the array.
5. **Type reliability.** Check whether monitors ever appear as `shell`, as
   Harp reports.
6. **Fixture replay.** Convert the captures into adapter/handler fixtures.
   Assert the status, `@pane_subagents`, task-reset behavior, notification
   stamps, and pending teardown state.
7. **Failure-path tests.** Cover absent, empty, malformed, unknown-type,
   unknown-status, duplicate, and out-of-order inputs.
8. **Restart test.** Stop the sidebar between `Stop` and drain; verify the
   pending state survives and completes correctly.
9. **Lost-drain test.** Drop the drain event and verify the timeout/fallback
   does not leave a permanent `running` row.
10. **No live tmux.** All unit tests use the test mock or an isolated server.

## Sources

Local:

- `src/adapter/claude/mod.rs` — hook registrations and the `stop` parser.
- `src/cli/hook/handlers/run.rs` — `on_stop`, `on_stop_failure`, completion
  notifications.
- `src/cli/hook/handlers/subagent.rs` — subagent start/stop and teardown drain.
- `src/cli/hook/context/meta.rs` — `mark_task_reset`.
- `src/cli/hook/context/subagents.rs` — list encoding and empty-list behavior.
- `src/cli/hook/context/pending.rs` — deferred teardown replay.
- `src/state/refresh.rs` — background-shell liveness sweep and reviewed-response
  parking.
- `docs/state-management.md` — pane options, update cadence, and status scope.
- `docs/maintainers/architecture-map.md` — seam and hotspot rules.
- `docs/maintainers/fork-strategy.md` — fork constraints and decision-record
  threshold.

Upstream and external:

- Claude Code hooks reference, Stop and SubagentStop input:
  <https://code.claude.com/docs/en/hooks#stop-input>
- Claude Code status line reference (context on what hooks do not expose):
  <https://code.claude.com/docs/en/statusline>
- Harp Claude Code hook README:
  <https://github.com/dominikmartn/harp/blob/main/integrations/claude-code/README.md>
- Harp background-tasks hook:
  <https://github.com/dominikmartn/harp/blob/main/integrations/claude-code/warp-background-tasks.sh>
- Harp CLI agent session state machine:
  <https://github.com/dominikmartn/harp/blob/main/app/src/terminal/cli_agent_sessions/mod.rs>

## Glossary

- **Background task**: an entry in Claude Code's `background_tasks` array that
  is still in flight when the parent turn reaches `Stop`.
- **Drain**: the event that removes the last running background task and allows
  the deferred completion transition to run.
- **Monitor**: a long-lived watcher started by Claude Code or a plugin. It
  should not keep a row working indefinitely.
- **Pending completion**: a durable record of a `Stop` transition that is
  waiting for background work to drain.
- **Completion stamp**: `@pane_completed_turn_id`, used to reject late events
  from a turn whose completion was already applied.
- **Synchronous subagent**: a `Task` child that finishes before the parent
  `Stop`, which is the current handler's assumption.
