# Decision: Adaptive quota fetch cadence driven by agent activity

> Extended by [2026-10-03-quota-cadence-hardening.md](2026-10-03-quota-cadence-hardening.md):
> the activity signal and the idle/active split below still stand, but the
> "double per unchanged snapshot" rule was replaced by an EWMA burn-rate
> interval, the failure path gained jittered backoff and `Retry-After`
> handling, and the Codex fetch moved to a direct HTTP call with the
> app-server as fallback.

## Context

The quota block (`docs/decisions/2026-09-17-quota-block.md`) shipped with a
fixed five-minute cadence per subscription and a deliberate "never reacts to
agent activity" rationale: a Codex fetch spawns an `app-server` subprocess and
a Kimi fetch spends the user's OAuth credentials on the network, so the cadence
was kept independent of the moment the user happens to be running an agent.

In practice the premise cuts the other way. The user only ever runs these
agents from the CLI panes the sidebar already tracks, and quota cannot move
while no matching CLI is active — DeepSeek is consumed through Claude and
covered by the usage row, not the quota block. So the idle five-minute poll
mostly fetches numbers that cannot have changed, while the minutes right after
a turn finishes — when the number actually moved — can wait out the full
interval. The fixed cadence is simultaneously too fast when idle and too slow
when it matters.

## Chosen Seam

Keep the worker/thread/channel architecture untouched and make the cadence
itself adaptive, per subscription:

- The 1s refresh loop already rebuilds the pane inventory
  (`apply_session_snapshot` in `src/state/refresh.rs`). It now publishes two
  flags — "a Codex pane is active" / "a Kimi pane is active" (`is_active()` =
  running / background / waiting) — into a shared `QuotaAgentActivity` next to
  the existing `quota_force_refresh` atomic. No new hook channel, no tmux
  query: the signal rides the poll the sidebar already does.
- `quota_poll_loop` reads the flags each turn. An active agent shortens the
  subscription's cadence to `ACTIVE_INTERVAL` (60s); an idle one keeps the
  five-minute `REFRESH_INTERVAL`. Activity flips wake the sleep within a
  second, so a starting agent retargets the next fetch promptly.
- While active, consecutive fetches whose snapshot did not move double the
  interval back toward `REFRESH_INTERVAL` (60s → 2m → 4m → 5m cap); a snapshot
  that moved snaps back to 60s. Quota consumption arrives in bursts between
  long quiet stretches, so the cadence follows the number, not just the agent.
- Failure backoff (doubling to 30 min) always wins over the active cadence,
  and the reset-boundary pull-forward is unchanged — both guards survive
  intact.

## Alternatives Rejected

- A tmux option for the interval (`@sidebar_quota_active_interval`):
  configuration is the preferred seam, but the policy here is not a
  preference — it follows from when quota can physically change. A constant
  keeps the behavior predictable; an option can be added later if the 60s
  floor ever proves wrong.
- Triggering a fetch on every pane status transition (like the DeepSeek usage
  row does): quota fetches are far more expensive than a cached file stat —
  a subprocess plus network — so a steady 60s floor while active is the right
  granularity, and the click remains the manual escape hatch.
- Keying the fast cadence off any agent (including Claude): Claude has no
  quota subscription, so its activity says nothing about when the Codex or
  Kimi numbers move.

## Upstream Compatibility

The whole feature is fork-only. Changes are confined to the fork-owned
`src/quota/` leaf, one additive `AppState` field, one additive block in
`src/app/workers.rs`, and one method call in `apply_session_snapshot`. Removal
deletes those plus this record and restores the fixed-cadence wording in
`docs/state-management.md`; the removal list in
`docs/decisions/2026-09-17-quota-block.md` covers the rest of the quota
feature.
