# Codex Goal Turns and Sidebar State Tracking

The original investigation observed a Codex 0.171.0 goal running while the
sidebar showed idle. The fix landed in `6d0d696`. The
[decision record](../decisions/2026-11-20-codex-goal-turns.md) defines turn
admission and its limits. This page preserves the evidence from before the fix.

## Codex behavior

- A thread goal is persisted in `$CODEX_HOME/goals_1.sqlite`
  (`thread_goals.status`, `tokens_used`, `time_used_seconds`).
- When the thread is idle and the goal is active, Codex starts a continuation
  turn. The rollout records `task_started` with `turn_trigger: "goal"` and a
  user-role message wrapped as
  `<codex_internal_context source="goal">`.
- `codex-rs/ext/goal/src/steering.rs::continuation_request` wraps the
  continuation prompt as `TurnInput::ResponseItem(...)`.
- `codex-rs/core/src/hook_runtime.rs::inspect_pending_input` runs
  `UserPromptSubmit` only for `TurnInput::UserInput`. `ResponseItem`,
  `FunctionCallOutput`, and `InterAgentCommunication` do not run it.
- `PostToolUse` hooks do run for goal turns and carry the goal turn id.

The installed Codex version and the upstream source were both checked. Re-check
both before implementing if either has moved.

## Failure in the original sidebar

At `7bfd92a`, a completed user turn left its ID in both `@pane_turn_id`
and `@pane_completed_turn_id`. The first goal hook carried the same session
ID and a new turn ID. Daemon title routing rejected the mismatch before
dispatch. Embedded routing reached handlers that rejected the mismatched turn.
Neither path admitted the first goal activity, permission request or Stop.

The old turn's Stop initially published `waiting/response_ready`. The idle
state sampled below was the state observed later, during the continuation.

## Runtime evidence

Observed on a live Codex 0.171.0 session; thread and turn ids are intentionally
omitted here.

| Observation | Result |
| --- | --- |
| Goal status | `active`; `tokens_used` and `time_used_seconds` increased for 25+ minutes |
| Rollout goal turn | `turn_trigger: "goal"`, internal-context message, one long turn |
| Pane status | `idle` at every observation during the goal |
| Pane turn options | `@pane_turn_id == @pane_completed_turn_id == T_old` |
| Captured `activity-log` payload | session matched the pane; `turn_id == T_goal` |
| Hook events in the capture window | `activity-log` only; no `user-prompt-submit` |
| Activity log | last entry before the goal started; no goal-turn entries |

The hook capture used a temporary wrapper around the installed `hook.sh` that
teed stdin. It was restored immediately after capture; it is a diagnostic, not
a production change. A read-only alternative is to sample
`ps` for `tmux-agent-sidebar hook codex activity-log` while a goal runs.

## Source evidence

The release source shows how [goal input is built](https://github.com/openai/codex/blob/ccf9be8cc458/codex-rs/ext/goal/src/steering.rs),
[how continuation turns start](https://github.com/openai/codex/blob/ccf9be8cc458/codex-rs/ext/goal/src/runtime.rs)
and [which inputs invoke UserPromptSubmit](https://github.com/openai/codex/blob/ccf9be8cc458/codex-rs/core/src/hook_runtime.rs).
