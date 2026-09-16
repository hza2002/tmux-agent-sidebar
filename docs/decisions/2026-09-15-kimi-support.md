# Kimi Code agent support

## Context

The sidebar ships adapters for Claude Code, Codex, and OpenCode. Kimi Code
CLI exposes a native hook mechanism (`[[hooks]]` entries in
`~/.kimi-code/config.toml`, TOML with `event`/`matcher`/`command`/`timeout`
fields, stdin JSON payloads, exit code 0/2 semantics) whose event set covers
the sidebar's core lifecycle: SessionStart/SessionEnd, UserPromptSubmit,
Stop/StopFailure, PostToolUse, PermissionRequest, Notification,
SubagentStart/SubagentStop. This makes Kimi a first-class adapter candidate
through the existing seam, with no plugin bridge (unlike OpenCode).

## Chosen Seam

New leaf adapter `src/adapter/kimi.rs` registered through the existing
`EventAdapter` / `resolve_adapter` / `HOOK_REGISTRATIONS` seam, plus one new
`AgentType::Kimi` variant and the standard agent color triple. No new event
kinds; the 17-variant `AgentEventKind` model covers every registered trigger.

Three deliberate deviations, each localized:

1. **Silent stdout.** Kimi appends hook stdout to the model context on exit
   0, so the Kimi adapter sets `response: None` on every event — including
   Stop, where Codex echoes `{"continue":true}`. Kimi's allow semantics are
   "exit 0 means allow"; stdout output is never required.
2. **Setup emits TOML, not JSON.** The existing setup/notices machinery
   assumes the Claude/Codex nested JSON shape (`trigger → [{matcher,
   hooks:[...]}]`). Kimi's flat `[[hooks]]` array cannot reuse that diff
   logic, so setup prints a paste-ready TOML block generated from
   `HOOK_REGISTRATIONS`. Kimi is excluded from the missing-hooks notice
   whitelist in v1 (detecting drift in TOML would need a TOML parser, which
   the dependency policy does not yet justify).
3. **Permission mode: unavailable, by design.** Kimi hook payloads carry no
   `permission_mode` field (confirmed from a live 0.41.0 payload), and the
   process-probing fallback used for Codex does not transfer: the
   long-running `kimi-code` process drops the CLI flags (`--yolo`/`--auto`/
   `--plan`) from argv after startup, so probing always reads nothing. The
   badge therefore stays unset for Kimi panes; Kimi is kept out of the
   probing lists rather than carrying dead probing code. One process-related
   adaptation *is* required: the pane-liveness sweep matches process names
   against the agent label, so `process_matches_agent` aliases `kimi` to the
   real binary name `kimi-code` — without this, the TUI's dead-pane sweep
   wipes live Kimi pane metadata within one 10s refresh cycle.

## Alternatives Rejected

An OpenCode-style plugin bridge: Kimi has native hooks, so a bridge adds a
moving part with no payoff. Merging TOML snippets into `config.toml`
programmatically: requires a TOML writer for a one-time install step, and a
bad merge corrupts the user's whole CLI config.

## Follow-up: Interrupt, PostToolUseFailure, PermissionResult, TaskStarted

The v1 record deferred Kimi-only events because no sidebar surface consumed
them. Runtime evidence changed that: Kimi fires `Interrupt` **in place of**
`Stop` on Esc, so an interrupted turn left the pane stuck in `running`
forever. Four events are now wired. Three of them carry semantics the older
agents do not have, so they get **dedicated event kinds instead of being
forced onto the closest existing one** — reuse was tried first and rejected
in review: `Interrupt`→`stop` rendered a user abort as "response ready"
plus a completion notification, and `PostToolUseFailure`→`activity-log`
produced log lines indistinguishable from successful calls.

- `Interrupt` → new `AgentEventKind::Interrupt`; `on_interrupt` lands the
  pane in `idle` with run state cleared and the completion stamp set (so
  late events dedup), but no attention, no wait reason, no notification;
- `PostToolUseFailure` → new `AgentEventKind::ToolFailure`;
  `handle_tool_failure` appends a `×`-marked activity entry without
  touching pane status (`StopFailure` owns turn-level errors);
- `PermissionResult` → new `AgentEventKind::PermissionResult`; the handler
  returns a permission-waiting pane to `running` the moment the prompt is
  answered, instead of holding `waiting` until the next lifecycle event;
- `TaskStarted` → shared `task-created` kind (`description` maps to
  `task_subject`) — a genuine semantic match, so no new kind.

Still unwired by choice: `PreToolUse` (would double-log activity),
`TurnStarted` / `UserPromptQueued` / `SessionHeartbeat` (no consuming
surface), `PreCompact` / `PostCompact` (not monitoring state). Kimi fires
**no** hook while a foreground AskUserQuestion waits for an answer, so that
wait still renders as `running`; fixing it needs an upstream Kimi event.

## Upstream Compatibility

All changes are additive match arms, one new enum variant, and one leaf
module; Claude/Codex/OpenCode paths are untouched. The event model and data
flow are unchanged. This feature is agent-support infrastructure, not a
personal preference, and is upstreamable in principle.

## Conflict and Removal Strategy

In upstream merges, keep upstream's adapter registry, `AgentType`, and color
tables, then reapply the Kimi arm as a focused follow-up. Removal deletes
`src/adapter/kimi.rs`, the `kimi` match arms in `adapter/mod.rs`,
`event/adapter.rs`, `tmux/types.rs`, `ui/colors.rs`, `tmux/options.rs`,
`desktop_notification.rs`, `cli/setup`, the TOML snippet tests, and this
record, restoring prior behavior.
