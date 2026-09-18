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

## Revision: numeric rows, honest reset times, visible pet

The first render shipped three defects and one design problem, found on the
same day against the live sidebar:

1. **Reset countdowns were garbage (`20692d`).** `AppState::now` is epoch
   *seconds* while the quota payloads had been normalized to *milliseconds*.
   Every reset stamp now carries Unix seconds — the same clock as
   `AppState::now` — and `format_countdown` is documented and tested against
   that contract.
2. **Kimi never rendered.** Two independent transport defects, either of
   which alone was fatal. The usage request sent the header value without its
   name (`Bearer <token>`), so curl emitted a custom `Bearer …` header and the
   API answered `401 Invalid Authentication`; the request now sends
   `Authorization: Bearer <token>`. And `curl --write-out "%{http_code}"`
   appends the status with no separator while `api.kimi.com` returns a body
   with no trailing newline, so the splitter saw `{...}200` as one line,
   failed to parse a status, and reported `HTTP 0` forever. The invoker now
   writes `"\n%{http_code}"` and the splitter also recovers a glued status, so
   a missing separator degrades instead of poisoning the payload.
3. **The bar block cost ten columns per row.** Rendering now follows the
   native Codex status line: numbers, not bars, with one subscription per row:

   ```text
    Quota
    codex 5h   3% 2h51m wk  85% 6d21h
    kimi  5h 100% 3h2m wk   9% 4d15h
   ```

   The row budget is exact. Indent 1 + name 5 + gap 1 + (`5h` 2 + gap 1 +
   percent 4 + gap 1 + countdown 5) + separator 1 + (`wk` 2 + gap 1 +
   percent 4 + gap 1 + countdown 6) = **35 cells** in the worst case, the
   default sidebar width; a typical row is 34. The 5-hour countdown cannot
   exceed `4h59m`, and `23h59m` is the longest value the weekly window can
   produce, so no realistic row is truncated. The test-side
   `ui::panes::quota::full_row_width` helper derives that budget from the field
   constants, and the pane only reserves the full rows when the rows it is
   about to draw actually fit; a narrower pane collapses to the compact rows
   instead of truncating a countdown.

   The collapse ladder is full (header + one row per subscription with
   countdowns) → compact (the same rows without the header and countdowns) →
   hidden, so two subscriptions cost three rows and one subscription two.
4. **Countdowns keep two units.** A weekly window now reads `6d21h` instead of
   rounding to `6d`: the old day-only format was a width compromise, and the
   one-row layout has room for both units.
5. **Colors are a battery scale, not three thresholds.** Each percentage is
   bold and painted with the step it falls into — `>= 80%` healthy,
   `60–79%` good, `40–59%` warn, `20–39%` low, `< 20%` critical — backed by
   five new theme slots (`@sidebar_color_quota_healthy` / `_good` / `_warn` /
   `_low` / `_critical`, defaulting to gruvbox green, aqua, yellow, orange,
   red). Subscription names use the same identity colors the agent list uses
   for Codex and Kimi, window labels stay `text_muted`, countdowns stay
   `text_inactive`, and the `Quota` header uses `section_title`. Each window is
   colored on its own, so an exhausted weekly window no longer dims a healthy
   5-hour one.
6. **The pet stays off by default.** It was briefly flipped to `on` to prove
   the filler-band relocation worked, then flipped back: the upstream sprite is
   a 4×3-cell blob plus a 4×2 desk and a chair, it duplicates the status counts
   the header already shows, and it competes with the quota block for the same
   band. It is opt-in (`@sidebar_pet on`) until a legible replacement is
   designed; see the follow-up question in this record's revision.

### Follow-up: the tab band

The pet's replacement is the fork's own answer to "what belongs in the idle
rows": the **tab band** (`@sidebar_band`, default `on`). While the bottom panel
is hidden (`@sidebar_bottom_height 0`), the agents panel hosts the active bottom
tab — the existing `ui::bottom` renderer, unchanged — in a fixed six rows
directly above the quota block. No new renderer, no new tab, no new data
source: `draw_bottom` takes an arbitrary `Rect`, so the seam is one call site
plus hit-testing.

Rules, deliberately simpler than the quota ladder:

- **All or nothing.** The band reserves its full height or hides; it never
  renders a partial tab, and it never changes its content to fit.
- **Hidden when empty.** An empty activity log, or a focused pane outside a git
  repository, hides the band instead of painting an empty box.
- **The list wins.** The band is funded only by rows the agent list does not
  use; the quota block (three rows) is funded first, the pet last.
- **The bottom panel wins.** With `@sidebar_bottom_height > 0` the tabs stay
  where upstream put them and the band stays hidden.

### Open question: what belongs in the filler band

The pet's only real signal is "at desk with a paper stack = 1–2 agents active",
which the header counts and the per-row status icons already carry. Any
replacement has to beat that duplication. Candidates worth evaluating, in the
order they were worth prototyping:

1. per-pane context/limit usage (Codex exposes it natively through
   `account/rateLimits/read`; Kimi only through the TUI status-line stdin
   snapshot, which is not reachable from a separate process);
2. a session/worktree summary band (branch, worktree, elapsed time of the
   longest-running agent);
3. nothing at all — let the quota block have the whole band.

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
