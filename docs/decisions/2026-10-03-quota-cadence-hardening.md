# Decision: Quota cadence hardening and direct Codex fetch

## Context

The adaptive cadence from
[2026-10-02-quota-adaptive-cadence.md](2026-10-02-quota-adaptive-cadence.md)
shipped with three latent bugs, an underspecified failure path, and a Codex
fetch that spawns a `codex app-server` subprocess for every poll:

1. The settle-after-fetch anchored the next wait on the pre-fetch timestamp,
   so every cycle ran long by the fetch duration (up to the 10s timeout).
2. The "snapshot moved" detector compared `resets_at` too, so routine reset
   stamp drift was misread as consumption and pinned the cadence at the floor.
3. Pane flapping reset the doubling backoff on every activity flip, so a
   twitchy pane held the cadence at the floor indefinitely.

The failure path also had no throttle awareness: a 429 answer was retried on
the plain doubling schedule, ignoring the provider's `Retry-After`, and every
subscription on the machine polled on the same five-minute phase.

Finally, the Codex CLI's usage page and several community clients read
`GET https://chatgpt.com/backend-api/wham/usage` directly with the bearer
token from `~/.codex/auth.json` — no subprocess needed.

## Chosen Seam

Everything stays inside the fork-owned `src/quota/` leaf; the worker, channel,
and state architecture is untouched.

### Cadence engine (`src/quota/mod.rs`)

- The wait is measured from a fresh post-fetch clock, so fetch time no longer
  stretches the cycle, and the moved detector compares only label + remaining
  percent. Reset drift alone no longer counts as movement.
- Failure backoff switches to decorrelated jitter
  (`random(REFRESH_INTERVAL, prev × 3)`, capped at 30 min) so repeated
  failures do not march in lockstep, and a 429's `Retry-After` raises the
  floor of that draw. The new `FetchError { message, retry_after }` carries
  the hint from the fetchers to the cadence. (Later on 2026-10-03,
  `FetchError` also gained a `kind` field, and `QuotaState::apply` now keeps
  it on the stale snapshot instead of dropping the error: the dimmed row's
  age marker carries a two-character failure tag — 限流 / 登录 / 网络 / 数据 /
  错误 — so a fetch failure is no longer invisible beyond the dimming. A
  further 2026-10-03 follow-up covers the case the tag could not reach: a
  subscription whose *first* fetch fails had no snapshot to dim and simply
  vanished from the block, which read as "the row disappeared" and cost two
  debugging sessions. `QuotaState` now records a `FetchFailure`
  (first-failure instant, latest kind) and the block renders a dimmed
  placeholder row with the same age-plus-kind marker (` codex 5h   -- wk   --
  ·1m 网络`) until `Available` replaces it with real data or `Unavailable`
  (no credentials) retires the subscription for good.)
- Idle polls add a 0–30s upward jitter so the two subscriptions (and any
  other clients on the same schedule) dephase. Reset-boundary pull-forward
  lands 0–20s after the boundary — never before it.
- When a reset boundary passes without a new stamp (provider lagging), the
  cadence retries on a short 45s rope up to four times before settling back
  to the idle schedule — a rolling window is picked up within minutes instead
  of an hour. The check is per window: a far-future weekly stamp must not
  shadow a 5h window that just missed its roll; once the rope is exhausted
  the next future boundary still clamps the wait.
- When an agent goes idle the cadence schedules one wrap-up fetch instead of
  stretching to the full idle interval: the last turn's consumption should
  show up promptly.

### Burn-rate active cadence

The "double per unchanged snapshot" rule is replaced by a rate estimate: an
EWMA (α=0.4) of observed consumption in percent-per-minute. While active, the
interval is `clamp(1% ÷ rate, floor, 5 min)` — the cadence chases a visible
change roughly once it has one percent more to show. Floors are 90s for Codex
and 60s for Kimi. When no movement has been observed yet (rate = 0), the
previous probe-doubling still applies, capped at 5 min. An unmoved active
poll is a zero-rate sample, so the estimate decays by `(1 - α)` and a rate
learned from a past burst relaxes instead of pinning the floor for the rest
of a quiet-but-active pane. A refill (percent goes up) zeroes the rate so the
post-reset burst is re-measured cleanly. The rate survives active/idle flips,
so a resumed session keeps its pace.

### Direct Codex fetch (`src/quota/codex.rs`)

Primary path is now `GET wham/usage` over the shared curl transport, with the
`Authorization: Bearer` and `ChatGPT-Account-Id` headers the CLI itself sends.
Windows are classified by `limit_window_seconds` (18000 = 5h, 604800 = week),
not by slot — the weekly window appears in `primary_window` when no 5h limit
applies, and either slot may be null.

`auth.json` has no expiry field; expiry is read from the access token's JWT
`exp` claim with a 300s skew. Refresh goes to
`POST https://auth.openai.com/oauth/token` with the Codex CLI's public client
id. Because OpenAI rotates refresh tokens, the refresh path mirrors the Kimi
client's conservative pattern: re-read before spending our refresh token,
re-read again before writing ours back, merge into the existing JSON (unknown
keys preserved, `last_refresh` updated), write atomically with 0600. The
Codex CLI's own writeback is a plain truncate-and-write, so ours is strictly
safer.

401/403, unparseable JSON, and schema drift fall back to the existing
app-server exchange, which is retained unchanged as
`fetch_via_app_server`. 429 and other HTTP failures are fatal for the direct
path — falling back to a heavier subprocess when throttled would make things
worse. (Later on 2026-10-03, the fallback's `parse_rate_limits` also switched
to duration-based classification — `windowDurationMins` 300 = 5h, 10080 =
weekly, anything else ignored, with the old primary/secondary slot mapping
kept only for responses that carry no duration — after live responses showed
the weekly window can occupy the `primary` slot there too.)

### Incidental fixes

- The roundtrip test for the new RFC3339 formatter exposed a pre-existing
  upstream bug in `kimi.rs`'s `parse_iso8601_secs`: the parsed seconds were
  validated but never added to the result. All existing fixtures had `:00`
  seconds, so it had never been observed. Fixed and pinned with a non-zero
  seconds case.
- `write_atomic` now creates the temp credentials file with mode 0600 from
  the start (`OpenOptionsExt::mode`) instead of relying on a post-write
  chmod, closing the window where rotated OAuth tokens were umask-readable.

## Risk Notes

- Request rate: the active floors (60s/90s) are conservative next to the
  five-minute industry baseline for these endpoints, and failure backoff plus
  `Retry-After` handling caps the damage if a provider pushes back. The
  direct Codex fetch is also strictly lighter than the app-server spawn it
  replaces.
- Token rotation race: if the Codex CLI refreshes while our refresh is in
  flight, the pre-writeback re-read adopts the sibling's newer tokens and
  discards ours, so neither client is stranded. Worst case both refresh
  concurrently and one rotated pair is wasted; the surviving file always
  holds a valid pair.
- wham/usage is an internal endpoint and may drift; that is exactly what the
  app-server fallback covers, and a drift event degrades to the pre-2026-10
  behavior rather than an outage.

## Alternatives Rejected

- Dropping the app-server path entirely: it is the only known-good recovery
  when the direct schema drifts, and it costs nothing while unused.
- Refreshing through the app-server (`account/read` with `refreshToken:
  true`): it would keep refresh logic in the subprocess, but couples every
  expiry to a 10-second process spawn and gives no writeback control.
- Learning separate rates per window (5h vs weekly): the weekly window moves
  too slowly to estimate; one rate per subscription, driven mostly by the 5h
  window, is enough.

## Upstream Compatibility

Fork-only, confined to `src/quota/` plus the decision records and
`docs/state-management.md`. The 2026-10-02 record's activity signal
(`quota_activity`) is unchanged; this record replaces only its "double per
unchanged snapshot" rule. Removal of the whole quota feature still follows
the removal list in
[2026-09-17-quota-block.md](2026-09-17-quota-block.md) plus these two
records.
