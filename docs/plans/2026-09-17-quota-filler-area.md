# Implementation Plan: Subscription Quota Block + Adaptive Filler Area

Date: 2026-09-17
Status: approved design, ready for implementation
Companion decision record: `docs/decisions/2026-09-17-quota-block.md`

This plan is written for an executing agent with zero prior context. Follow it
end to end; every step names concrete files, constants, and tests. If reality
drifts from a specific claim here (a line moved, a function renamed), prefer
the current code and note the drift in the commit message.

## 1. Goal and Non-Goals

### Goal

Use the idle vertical space below the agent list (when few agents are running,
the lower half of the sidebar is blank background) for:

1. **Quota block** — remaining quota for the user's two AI subscriptions,
   **Codex (ChatGPT account)** and **Kimi Code**, anchored to the bottom of
   the agents panel.
2. **Pet relocation (Phase 3)** — the existing pixel pet moves from the
   bottom-panel divider band into the free space above the quota block.

Priority order when vertical space runs out (user decision, non-negotiable):

1. Agent list is first-class. It never loses rows to quota or pet.
2. Pet is hidden first.
3. Quota is hidden second (via a three-level collapse, see §4).

### Non-Goals

- No Claude quota (Claude exposes no equivalent usage API; the user's two
  subscriptions are Codex and Kimi — confirmed by reading their Raycast
  extension, see §5).
- No burn-rate or cost estimation (the Raycast extension does not have them;
  keep parity).
- No tamagotchi mechanics (XP, hats, evolution) — Phase 3 is relocation plus
  a semantic fix, not Pet 2.0.
- No new third-party crates. HTTP goes through a `curl` subprocess; JSON
  through the already-present `serde_json`.
- No changes to `src/ui/mod.rs` top-level layout composition (skeleton
  touchpoint). All rendering integration stays inside `src/ui/panes.rs`.

## 2. Repository Constraints (from AGENTS.md — binding)

- Tests must never touch the developer's live tmux server. All tests here are
  pure/unit tests with fixtures; no tmux needed at all.
- Any test that renders a Ratatui frame must use an inline
  `insta::assert_snapshot!`. Never hand-edit snapshots.
- Run `cargo fmt` before every commit; `./scripts/verify.sh quick` (fmt check
  + clippy) must pass; run `./scripts/verify.sh full` before handoff.
- New `@sidebar_*` options live as constants in `src/tmux/options.rs` and get
  defaults in `agent-sidebar.conf`.
- Update `docs/state-management.md` (state scope + cadence tables) when adding
  state fields — this plan adds local state (§6).
- The data-flow direction `tmux query → AppState → ui::draw` must not be
  reversed. Quota data arrives via a background thread + channel, exactly like
  `session_poll_loop` in `src/app/workers.rs:45-53`.

## 3. Prior Art: the User's Raycast Extension (normative protocol reference)

The user already built this feature as a Raycast extension. Its source is the
normative reference for both protocols:

- `~/dotfiles/raycast/extensions/agent-usage/src/kimi.ts` — Kimi fetch,
  OAuth refresh, atomic credential writeback, 401 re-read strategy.
- `~/dotfiles/raycast/extensions/agent-usage/src/usage.ts` — Codex
  app-server stdio JSON-RPC state machine, view model
  (`PlanView { title, subtitle, rows: [{ title, remaining%, resetsAt }] }`).

Read both files before writing any fetch code. If the files are unavailable,
the essential protocol facts in §7 and §8 below are sufficient to proceed.

Key facts established during design (do not re-litigate):

- The extension keeps results in memory only; there is no cache file to read.
  The sidebar must fetch for itself.
- Display semantics are **remaining percentage + reset countdown** per window
  (5-hour and weekly). Match this exactly so the sidebar and Raycast tell the
  same story.
- Both subscriptions load independently; one failing must not affect the
  other. Copy this error isolation.

## 4. Layout and Degradation Spec

All work happens inside the agents chunk (`chunks[0]` in
`src/ui/mod.rs:85`, passed to `panes::draw_agents`). Do not touch
`src/ui/mod.rs`.

Current split inside `draw_agents` (`src/ui/panes.rs:508-531`):
`PaneLayout::compute` → 1 header row + `list_area` (rest); rows render as a
scrollable `Paragraph` in `list_area`.

New split:

```
┌ agents chunk ────────────────────┐
│ header (1 row, unchanged)        │
│ list rows (scrollable)           │  ← first-class; scrolls exactly as today
│ ┄ pet scene (Phase 3) ┄          │  ← only when leftover rows ≥ PET_SCENE_HEIGHT
│ quota block (bottom-anchored)    │  ← collapses  full → compact → hidden
└──────────────────────────────────┘
```

Definitions:

- `rendered = lines.len()` (the collected agent rows).
- `spare = list_area.height.saturating_sub(rendered)`. Quota and pet are
  funded **only** from `spare`. When the list fills the chunk, both vanish and
  the list scrolls as it does today — this is what "agent list is
  first-class" means.
- Quota heights: `full = 1 + 2 × subscription_count` rows (a `Quota` header
  row, then a 5-hour row and a weekly row per subscription), `compact =
  subscription_count` rows (one line per subscription), `hidden = 0`.
- Selection rule: if `spare >= full` → full; else if `spare >= compact` →
  compact; else hidden. (Pet, Phase 3: rendered in the rows between the list
  and the quota block only when `spare - quota_height >= PET_SCENE_HEIGHT`.)
- The quota block renders at the **bottom** of `list_area` (`y =
  list_area.bottom() - quota_height`), leaving the gap between the last list
  row and the quota block as dead space (Phase 1/2) or the pet scene
  (Phase 3). The scrollable `Paragraph` keeps `list_area` as its render rect;
  quota/pet overdraw only rows the `Paragraph` cannot reach (spare rows), so
  no `Clear` is needed. Verify this in snapshot tests.

Mockup (30 cols, 2 subscriptions, full mode, Gruvbox colors at runtime):

```
 ✓   2   1   0   1   — ▾
dotfiles
┃ ▶ claude · 3m
api
┃ ⚠ codex
┃   permission required

 Quota
 codex 5h ▓▓▓▓▓▓░░░░ 61% 2h13m
       wk ▓▓▓▓▓▓▓░ 83% 4d
 kimi  5h ▓▓░░░░░░░░ 24% 41m
       wk ▓▓▓░░░░░░░ 37% 2d
```

Compact mode: `codex 5h 61%·2h13m wk 83%` / `kimi 5h 24%·41m wk 37%`.

Stale data (last fetch failed): keep the last good snapshot, dim the whole
row, append an age marker like `·17m`. Missing credentials for a subscription
→ that subscription never renders; both missing → the block never renders
(indistinguishable from disabled).

Bars use block characters (`▓` filled / `░` empty, 10 cells) and **represent
remaining quota**. Color by remaining level using existing theme slots:
`theme.status_running`-adjacent green when high, accent/yellow when low,
`theme.status_error` red when nearly exhausted (≤10%). Exact thresholds:
>50% green, 11–50% yellow, ≤10% red. Colors must come from `state.theme`,
never hardcoded (AGENTS.md UI invariant).

## 5. New Configuration

One option, default on:

- `@sidebar_quota` — `on`/`off` (accept `true/false/1/0` like
  `pet_enabled_from_options` in `src/ui/mod.rs:47-52`). Default `on`.
- Constant: `pub const SIDEBAR_QUOTA: &str = "@sidebar_quota";` in
  `src/tmux/options.rs` next to `SIDEBAR_PET`; re-export via `src/tmux.rs`
  if the neighboring constants are re-exported there.
- Default in `agent-sidebar.conf` (next to the `@sidebar_pet` line):
  `set -g @sidebar_quota "on"`.
- Parse helper `quota_enabled_from_options(&HashMap<String,String>) -> bool`
  (default `true`) + unit tests, following the
  `bottom_panel_height_from_options` pattern in `src/ui/mod.rs:32-43`.
- Read once at startup in `src/app/setup.rs:init_state` into
  `state.quota_enabled` (restart-to-apply, same as `pet_enabled`).

No other knobs. Refresh cadence is fixed (§6); credentials paths are fixed
(§7/§8).

## 6. State and Cadence

New local state (owned by the singleton sidebar process; nothing is written
to tmux):

```rust
// src/state.rs (or src/state/quota.rs — prefer a submodule if AppState
// already groups sub-structs; follow the FocusState/ActivityState precedent)
struct QuotaState {
    kimi: Option<SubscriptionQuota>,
    codex: Option<SubscriptionQuota>,
}

struct SubscriptionQuota {
    windows: Vec<QuotaWindow>,   // parsed from the API; typically 5h + week
    fetched_at: Instant,         // for the stale-age marker
    fetch_failed: bool,          // last fetch failed → dim + age marker
    // credentials_absent is represented by `kimi/codex: None` forever.
}

struct QuotaWindow {
    label: String,               // "5h" / "wk" — normalize at parse time
    remaining_percent: u8,       // 0..=100
    resets_at: Option<SystemTime>,
}
```

Cadence (user explicitly asked for careful thought here — do not deviate
without noting why):

| Action | Frequency | Rationale |
|---|---|---|
| Network fetch (both subscriptions, independent) | every 5 min | quota windows move slowly; remaining% only changes with real consumption |
| Failure backoff | double the interval per consecutive failure, cap 30 min; reset on success | be a polite client when offline/VPN down |
| Countdown text (`2h13m`) | recomputed every 1s refresh tick from `resets_at` | pure arithmetic, zero I/O; keeps the display live |
| Kimi token refresh | only on 401; re-read the credentials file first, refresh + atomic writeback only if still expired | the credentials file has three concurrent consumers (Kimi CLI, Raycast extension, sidebar); be the most conservative one |
| Codex fetch | spawn `codex app-server` per fetch, 10s hard timeout, kill on timeout | protocol proven by the Raycast extension; a resident app-server is not worth it |
| Startup | first fetch immediately (async, never blocks first frame) | data on first open |
| Click on the `Quota` header row | force an immediate refetch | see §9 |

Threading: extend `src/app/workers.rs` with a `quota_poll_loop` modeled on
`session_poll_loop` (lines 45–53) returning results through
`mpsc::channel::<QuotaFetchResult>`, drained in `src/app.rs` next to the
`session_rx.try_recv()` at lines 110-112. The force-refresh signal is an
`Arc<AtomicBool>` shared with the loop; the loop sleeps in 1s increments so
the flag takes effect within a second. `QuotaFetchResult` carries per-
subscription `Result`s so one failure cannot mask the other's success.

Never log or snapshot credential values. Tests must never perform network
I/O: the fetch layer takes injectable closures/command runners (see how
`git::fetch_pr_number` is injected in `workers.rs:77-82`) and unit tests feed
fixture JSON.

## 7. Phase 1 — Filler Mechanism + Kimi Quota

Independently mergeable: after this phase the sidebar shows Kimi quota (or
nothing, if credentials are absent) with the full degradation behavior.

### 7.1 Kimi protocol (from `kimi.ts`)

- Credentials: `~/.kimi-code/credentials/kimi-code.json` →
  `{ access_token, refresh_token, expires_at }` (file mode 0600). Missing
  file → subscription permanently absent (render nothing).
- Fetch: `GET https://api.kimi.com/coding/v1/usages` with header
  `Authorization: Bearer <access_token>`. Use a `curl` subprocess
  (`curl -sS -m 10 -H ... <url>`), matching the repo's subprocess idiom
  (tmux/ps/lsof already spawned this way; `src/port.rs:163` is a template
  for machine-readable subprocess output).
- Response shape (verify against `kimi.ts`): `limits[]` where each entry has
  a window descriptor (`duration`/`timeUnit`), `used`, `limit`, `remaining`,
  `resetTime`; the top-level `usage` object is the weekly quota. Map the
  5-hour-ish window to label `5h` and the weekly to `wk`; ignore other
  windows. `remaining_percent = round(100 × remaining / limit)` (or the
  API's own percentage field if `kimi.ts` uses one — follow `kimi.ts`).
- 401 handling (copy `kimi.ts:208-227`): re-read the credentials file; if the
  file now has a different/newer token, retry once with it; only if it is
  still expired, refresh via `POST https://auth.kimi.com/api/oauth/token`
  (public client_id — copy the exact value from `kimi.ts`) and write the
  result back **atomically** (temp file + rename, preserving 0600).

### 7.2 Files

New:

- `src/quota/mod.rs` — types (`QuotaState`, `SubscriptionQuota`,
  `QuotaWindow`), the 5-min/backoff poll loop body, countdown formatting
  (`2h13m`, `4d`, `41m`), staleness logic. Register `pub mod quota;` in
  `src/lib.rs`.
- `src/quota/kimi.rs` — credentials read/refresh, `curl` fetch, response
  parsing. Parsing takes `&str` JSON so tests never spawn curl.
- `src/ui/panes/quota.rs` — renderer: full/compact modes, bars, colors,
  stale dimming, click-target row for the header (see §9). Submodule of
  `src/ui/panes.rs` (`mod quota;` alongside `mod row;` etc.).

Edited:

- `src/state.rs` — `AppState.quota: QuotaState`,
  `AppState.quota_enabled: bool`; `apply_quota_result()` method that stores
  per-subscription results and sets/clears `fetch_failed`.
- `src/tmux/options.rs` — `SIDEBAR_QUOTA` constant.
- `agent-sidebar.conf` — default `on`.
- `src/ui/mod.rs` — `quota_enabled_from_options` helper + tests (this is an
  additive helper next to `pet_enabled_from_options`, not a layout change).
- `src/app/setup.rs` — `state.quota_enabled = ui::quota_enabled_from_tmux();`
  next to line 18.
- `src/app/workers.rs` — `quota_poll_loop` + `quota_rx` in `Workers`;
  spawn it only when `state.quota_enabled`.
- `src/app.rs` — drain `quota_rx.try_recv()` → `state.apply_quota_result(...)`,
  set `needs_redraw`.
- `src/ui/panes.rs` — `PaneLayout` gains quota reservation per §4
  (`compute` takes `&AppState` or the resolved quota height; keep the pure
  `Rect` math testable like the existing `PaneLayout` tests at lines
  537-597). `draw_agents` calls `quota::render` after `render_pane_rows`.

### 7.3 Tests

- `quota_enabled_from_options`: missing → true; `off/false/0` → false;
  case-insensitive.
- Kimi parser: fixture JSON (build from the shape in `kimi.ts`, scrubbed of
  real values) → correct windows, labels, percentages, reset times; malformed
  JSON → `None`, no panic.
- Countdown formatting: minutes, hours+minutes, days.
- Staleness: `fetch_failed` → dim + age marker present in snapshot.
- `PaneLayout`: quota reservation math at full/compact/hidden thresholds,
  including zero-height and tiny areas (extend the existing test style).
- Inline `insta::assert_snapshot!` frames in `tests/ui_snapshot.rs` using
  `test_helpers::make_state` / `render_to_string` (helper at
  `tests/test_helpers.rs:94,164`): full mode, compact mode, hidden when list
  fills the chunk, single-subscription, stale/dimmed. Snapshots must contain
  no real quota data — fixtures only.
- Worker loop: flag/force-refresh behavior modeled on the existing
  `git_poll_loop` tests (`src/app/workers.rs:90-171`).

### 7.4 Docs in this phase

- `docs/state-management.md`: add `quota` + `quota_enabled` to the Local
  State table and the quota row to the Update Cycle Summary (5-min background
  thread).
- README: one bullet under features if the README lists sidebar contents.

## 8. Phase 2 — Codex Quota

Independently mergeable: adds the second subscription row(s); no Phase 1
behavior changes.

### 8.1 Codex protocol (from `usage.ts`)

- Spawn per fetch:
  `/opt/homebrew/bin/codex app-server --listen stdio://` with
  `-c model_provider="openai"` and
  `-c chatgpt_base_url="https://chatgpt.com/backend-api/"`.
  Resolve the binary via `which codex`-style lookup if the repo already has a
  resolver; otherwise use the extension's absolute path with a fallback to
  `codex` on `PATH`. Absent binary → subscription permanently absent.
- Strip `OPENAI_API_KEY` and `CODEX_*` from the child environment (the
  extension does this deliberately so ChatGPT-account auth in
  `~/.codex/auth.json` wins).
- JSON-RPC over stdio, in order: `initialize` → `account/read` →
  `account/rateLimits/read` (exact method names/shapes from `usage.ts`,
  including its stage state machine around line 239).
- Account consistency: read `tokens.account_id` from `~/.codex/auth.json`
  before and after the fetch; if it changed mid-fetch, discard the result
  (guards against the user switching accounts during the query).
- Windows: `windowDurationMins == 300` → `5h`, `== 10080` → `wk`; each has a
  remaining percentage and reset timestamp. (Credits/planType/email exist in
  the payload — ignore them in v1.)
- 10s overall timeout; kill the child on timeout and mark the fetch failed.

### 8.2 Files and tests

- New `src/quota/codex.rs` — spawn, stdio protocol, parsing. The stdio
  exchange is unit-testable by splitting protocol logic (`next_request(state)`,
  `handle_response(state, line)`) from process I/O, mirroring the extension's
  stage machine.
- Wire into the Phase 1 poll loop and renderer (second subscription row;
  heights already scale by `subscription_count`).
- Parser/protocol unit tests with recorded JSON-RPC transcripts; snapshot
  test for the two-subscription full/compact frames.
- `docs/state-management.md` cadence note unchanged (same thread).

## 9. Click-to-Refresh (both phases)

- The `Quota` header row is a click target. Follow the existing hit-target
  pattern: renderer records the row's columns into a `FrameLayout` field (add
  `quota_header_row: Option<u16>` to `FrameLayout` in `src/state.rs`),
  `src/app/input.rs` routes clicks on that row (see how
  `handle_bottom_tab_click` is routed at `src/app/input.rs:33-44`).
- Handler sets the `Arc<AtomicBool>` force flag shared with
  `quota_poll_loop` (§6). No direct fetch from the input path.
- Test: click routing unit test modeled on the bottom-tab click tests.

## 10. Phase 3 — Pet Relocation + Semantic Fix

Independently mergeable; only touches pet behavior.

1. **Fix the status regression** (`src/state/pet.rs:53-59`):
   `running_count()` filters `status == PaneStatus::Running`, so fork-added
   `Background`/`Waiting` panes (both mean "still working" — see
   `PaneStatus::is_active()` at `src/tmux/types.rs:185-187`) send the pet
   home. Change the filter to `p.status.is_active()`. Update the 11 state
   machine tests in `src/state/pet.rs:210-429` and add a regression test for
   a Background-only pane.
2. **Relocate the pet into the filler area**: render the pet scene in the
   spare rows between the list and the quota block (§4), instead of the
   bottom-panel divider band. This removes the double-gating bug where the
   pet required `@sidebar_bottom_height > 0` **and** `@sidebar_pet on`
   (`src/ui/mod.rs:66-70,87-93`):
   - Pet renders whenever `@sidebar_pet on` and
     `spare - quota_height >= PET_SCENE_HEIGHT` — no bottom panel required.
   - When the bottom panel **is** enabled, the divider reverts to a plain
     1-row gap (no second pet). Decide by whether
     `state.bottom_panel_height > 0`; keep `draw_pet`'s sprite math as-is and
     only change where it is called from. This is the one place this plan
     touches `src/ui/mod.rs` lines 66-70/87-93 — keep the edit minimal
     (remove pet from the divider branch; the new call site lives in
     `src/ui/panes.rs`).
   - The 200ms tick (`src/app.rs:77-82`) and `tick_pet` are unchanged.
3. Snapshot tests: pet visible with quota below it; pet hidden but quota
   visible when only quota fits; both hidden when the list fills the chunk;
   no pet in the divider when the bottom panel is on.
4. `docs/state-management.md`: update the pet rows' scope notes if the
   render location is documented there (it is, under Local State).

## 11. Verification and Acceptance

Per phase, before committing:

```bash
cargo fmt
./scripts/verify.sh quick     # fmt check + clippy
cargo test                    # isolated; must stay green
```

Before final handoff: `./scripts/verify.sh full` (adds release build), then
`./scripts/fork-delta.sh` and confirm the delta contains only this feature.

Manual acceptance (live tmux, in order):

1. `@sidebar_quota` unset → after sidebar restart, quota rows appear within
   ~10s for whichever subscriptions have credentials on this machine.
2. Remaining percentages match the Raycast extension (open it side by side).
3. Reset countdowns tick down locally every minute without new fetches.
4. Start agents until the list fills the pane → quota collapses full →
   compact → hidden, in that order, and the list scrolls normally.
5. `tmux set -g @sidebar_quota off` + restart → no quota rows, layout
   identical to before the feature.
6. Disconnect network → after the next fetch tick, rows dim and show an age
   marker; reconnect → rows recover within one (backed-off) cycle.
7. Click the `Quota` header → data refreshes within ~2s.
8. Phase 3: `@sidebar_pet on`, bottom height 0 → pet walks in the filler
   area above the quota block; a Background-only agent keeps the pet working.

## 12. Risk Boundaries and Rollback

- Credentials: read-only except the Kimi refresh writeback, which must be
  atomic (temp + rename, 0600) and only after the re-read confirms expiry.
  Never print tokens to logs, errors, or snapshots.
- Network: all fetches are off the TUI thread with hard timeouts; the UI
  must render identically with zero network.
- Rollback: `tmux set -g @sidebar_quota off` + restart removes the feature
  at runtime. Code-level removal: delete `src/quota/`,
  `src/ui/panes/quota.rs`, the `quota` fields/methods in `state.rs`,
  `workers.rs`/`app.rs` wiring, the `SIDEBAR_QUOTA` constant and conf line,
  and revert the `PaneLayout` change — each phase is a clean commit set, so
  reverting Phase N never breaks Phase N-1.
- Upstream compatibility: every change is additive except the Phase 3 pet
  call-site move in `src/ui/mod.rs` — that one is called out in the decision
  record with a reapplication strategy.
