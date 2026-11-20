# Admit Codex continuations at the hook router

## Context

In the Codex rust-v0.171.0 release (ccf9be8cc458), the goal extension calls
start_turn_if_idle with a ResponseItem. Core's inspect_pending_input invokes
UserPromptSubmit only for UserInput. Core's new_submission_id explicitly
documents the UUIDv7 timestamp in public turn IDs and uses Uuid::now_v7.
That generator orders IDs within one process. The wire protocol does not
promise ordered IDs across daemon restarts.

Before this change, title routing rejected a different turn before dispatch. Embedded
routing reached the handlers, whose own turn checks rejected it. Stop normally
publishes waiting/response_ready before review can return the pane to idle.

## Decision

Use option A in the common locked Codex dispatch path. A tool, permission or
Stop hook may establish a successor only for its bound session after the
current turn completed. Require two canonical RFC-variant UUIDv7 IDs with
strictly increasing millisecond timestamps. Equal timestamps are ambiguous
across processes. Title-routed events also require the established routing
marker and matching client PID. Child threads have separate session IDs.

Reuse the prompt handler's run initialization, including notification scope,
without synthesizing prompt text. Dispatch the original event with the new
turn bound. The existing handlers own activity, permission, response review
and notification policy. An older or duplicate completion cannot reopen a
turn. A tool-free turn can complete on its first hook. Running is observable
from the first activity hook; there is no hook during initial silent reasoning.

## Alternatives and removal

An app-server subscription (option B) would provide earlier visibility but
requires a persistent subscriber, selection reconciliation and reconnect
handling, including an alternative for embedded clients. Keep the hook path
as the single lifecycle writer. Replace successor inference when Codex
provides an authoritative start hook for automatic turns. No option migration
or hook registration change is required.

## Verification and deployment

Run `./scripts/verify.sh full` for the router unit tests and isolated tmux
lifecycle tests. The regression in `tests/codex_daemon_routing_tests.rs`
exercises activity, permission, stale Stop, completion and duplicate delivery
with a private tmux socket and a notification stand-in. The release build at
the end of verification supplies the binary for subsequent hook invocations.

See the [original observations](../plans/2026-11-20-codex-goal-turn-tracking-reference.md)
and the [maintained routing contract](../codex-daemon-routing.md).
