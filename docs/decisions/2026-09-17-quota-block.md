# Decision: Subscription quota block in an adaptive filler area

## Context

With few agents running, the lower half of the sidebar is blank background:
the agents chunk is `Constraint::Min(1)` but the collected rows do not fill
it. The user wants that idle space to show remaining quota for their two AI
subscriptions — Codex (ChatGPT account) and Kimi Code — which they already
surface in a Raycast extension
(`~/dotfiles/raycast/extensions/agent-usage/`). The existing pixel pet is
effectively invisible in this fork: it requires both `@sidebar_bottom_height
> 0` and `@sidebar_pet on`, the fork's `Background`/`Waiting` statuses are
not counted as "running" by its state machine, and the user has never
enabled it since forking.

## Chosen Seam

A leaf-module feature behind one new option (`@sidebar_quota`, default on):

- Data arrives through a new background thread + channel in
  `src/app/workers.rs`, modeled exactly on `session_poll_loop` — no new
  tmux query, no blocking on the input/render path.
- Fetching lives in a new `src/quota/` module: Kimi via `curl` subprocess +
  `serde_json` (no new crates); Codex via the `codex app-server` stdio
  JSON-RPC protocol proven by the user's Raycast extension.
- Rendering integrates inside `src/ui/panes.rs` (`PaneLayout` + a new
  `src/ui/panes/quota.rs` leaf renderer), bottom-anchored in the agents
  chunk, funded only by rows the agent list does not use. Space priority:
  agent list first, pet hidden next, quota collapsed (full → compact →
  hidden) last.
- The pet moves from the bottom-panel divider band into the same filler
  area (Phase 3), which removes its double-gating; its state machine starts
  using `PaneStatus::is_active()` so `Background`/`Waiting` count as work.

## Alternatives Rejected

- A third bottom-panel tab: the bottom panel is fixed-height and defaults
  to hidden, so it does not address the idle space at all.
- Reading Claude usage too: there is no equivalent API for Claude
  subscriptions, and the user's subscriptions are Codex + Kimi.
- Adding an HTTP crate (ureq/reqwest): violates the dependency policy; a
  `curl` subprocess matches the repo's existing tmux/ps/lsof idiom.
- Parsing local `~/.claude` JSONL ccusage-style: the user's subscriptions
  expose first-party quota endpoints, which are cheaper and exact.
- Reserving a fixed-height quota section: would shrink the agent list even
  when it needs the rows; the adaptive, spare-rows-only design keeps the
  list first-class as the user required.

## Upstream Compatibility

Everything is additive except one call-site move in Phase 3: the pet stops
rendering in the `src/ui/mod.rs` divider branch and renders from
`src/ui/panes.rs` instead. `src/ui/mod.rs` is a listed skeleton touchpoint;
the edit is confined to the pet branch (removing `draw_pet` from the
divider path and reverting the divider to a 1-row gap) and is explained
here as required. No changes to the query layer, hook handlers, adapters,
or CLI entry points. The feature is fork-only personal policy; nothing here
is expected to be upstreamed.

## Conflict and Removal Strategy

In upstream merges, keep upstream's `src/ui/panes.rs` structure and
reapply the quota reservation inside `PaneLayout::compute`/`draw_agents`
as a focused follow-up; keep upstream's divider logic and reapply the pet
call-site move the same way. Runtime off-switch: `tmux set -g
@sidebar_quota off` + restart. Full removal deletes `src/quota/`,
`src/ui/panes/quota.rs`, the `quota`/`quota_enabled` fields and
`apply_quota_result` in `src/state.rs`, the worker wiring in
`src/app/workers.rs`/`src/app.rs`, the `SIDEBAR_QUOTA` constant and conf
default, the `PaneLayout` reservation, and this record; Phase 3 removal
additionally restores the divider pet call site and the `Running`-only
filter in `src/state/pet.rs`.
