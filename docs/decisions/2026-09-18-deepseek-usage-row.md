# Decision: DeepSeek spend row in the quota block

## Context

The fork's quota block shows remaining ChatGPT (Codex) and Kimi Code
subscription windows. DeepSeek is different: it is pay-per-token and billed in
RMB, and the fork's user runs it inside both Codex and Claude Code, so the
question "what has today cost" has no first-party UI on this machine.

The row has to answer that from local data only. Three properties drove every
choice below:

1. It sits in the render path's neighbourhood — the sidebar repaints every
   second — so nothing may block, spawn a slow process, or re-read gigabytes at
   tick rate.
2. Prices are published as a table with effective dates, so history must keep
   the rates that applied when the usage happened.
3. The number must be honest: a wrong id or an unknown model has to show up as
   an explicit marker, never as a plausible-looking wrong amount.

## Chosen Seam

### The row

```text
 ds    ¥1.47  12.3M 缓存98% 空闲
```

`ds` / `ds7` / `ds30` names the window; then the amount, the window's token
total, and the share of input tokens served from DeepSeek's cache. Only the
today window ends with the current price period (`空闲` / `高峰`) — a range
mixes both periods, so it never claims one.

Each field owns a colour, and exactly one of them is dynamic. The **amount**
uses the quota scale's hues on an average-daily-spend ladder (`≤¥5` healthy,
`≤¥15` warn, `≤¥30` low, above that critical) and is bold; the DeepSeek name
carries the agent identity blue; the token total, cache share and price period
each get a fixed hue via `@sidebar_color_quota_spend_{tokens,cache,off_peak,
peak}`. Because only the amount moves between tiers, a *changing* colour on the
row always means money, while a fixed colour identifies a field. Peak adds
weight on top of its colour, since it is the one field whose meaning flips
while the row is on screen. An unpriced amount is muted and unpainted — a
ladder colour would claim a certainty that the trailing `?` says we do not
have.

A fork-owned leaf module, `src/usage.rs`, plus four small integration points:

- **Reading.** The module parses the agents' own session logs — Codex
  `$CODEX_HOME/sessions/**/*.jsonl` (default `~/.codex`) and Claude Code
  `~/.claude/projects/**/*.jsonl`, including `subagents/agent-*.jsonl`. Files
  are discovered by walking those trees and keeping the ones whose mtime is at
  or after the window start; a file that received an in-window event was
  necessarily written then, so mtime is a safe pre-filter and no date
  directories are required.
- **Scanning.** `Scanner` owns a `(path, mtime, size) -> records` cache and runs
  on one background thread (`usage::poll_loop`), spawned next to
  `quota_poll_loop` in `src/app/workers.rs`. Unchanged files are never
  re-parsed, so a steady-state tick costs a stat sweep. Results arrive through a
  channel as `usage::Update`, tagged with the window they belong to.
- **Trigger.** Any agent pane whose `@pane_status_changed_at` advances sets the
  scanner's force flag from `AppState::refresh`. The `Stop` hook already
  records completion as a pane status transition, so this needs no new
  hook-to-sidebar channel, no PID lookup, and no signal.
- **Rendering.** `src/ui/panes/quota.rs` renders the row through the existing
  `QuotaLine`/`QuotaSpan` mechanism, so it inherits the full → compact → hidden
  ladder and the width gate. A 30s fallback poll covers a missed trigger.

## Pricing

DeepSeek's RMB list is the primary table; the English page is the same numbers
divided by a fixed 6.67, so no exchange rate belongs in the code. Source:
<https://api-docs.deepseek.com/zh-cn/quick_start/pricing>.

- `deepseek-flash` — off-peak ¥1 / ¥0.02 / ¥4, peak ¥2 / ¥0.04 / ¥8 per million
  (uncached input / cache-hit input / output).
- `deepseek-v4-pro` — off-peak ¥4.5 / ¥0.15 / ¥13.5, peak ¥9 / ¥0.30 / ¥27.
- Peak windows are **UTC** Monday–Friday 01:00–04:00 and 06:00–10:00, half-open;
  everything else, including all weekend hours, is off-peak. Peak is decided per
  event, not per day: applying peak rates to a whole day overstates night and
  weekend usage (measured on this machine: ¥1.91 actual versus ¥2.28 all-peak,
  ¥1.14 all-off-peak).
- `RATE_PERIODS` carries an `effective_from` instant per family, and an event is
  priced with the last period at or before its own timestamp. A published change
  is a **new entry plus a boundary test**; existing entries are never edited, so
  history keeps the rates that applied when it happened.
- Maintenance is manual and deliberately so: one vendor, two families, twelve
  numbers. When the official page changes, append a period and a mapping.

## Sources and token semantics

The two agents write different usage shapes, and the split rule differs:

| | Codex | Claude Code |
|---|---|---|
| record | `type=turn_context` sets the model, `event_msg/token_count` carries `info.last_token_usage` | assistant `message.model` and `message.usage` |
| uncached input | `input_tokens - cached_input_tokens` (input **includes** cache) | `input_tokens` (input **excludes** cache) |
| cache hits | `cached_input_tokens` | `cache_read_input_tokens` |
| duplicates | identical events repeated across resumed or forked session files | one entry per streamed content block with the same `message.id` |

Cache writes (`cache_creation_input_tokens`) are billed as uncached input:
DeepSeek publishes no separate write price and its Anthropic endpoint has
reported zero. Claude duplicates are collapsed by keeping the entry with the
largest token total, matching ccusage's tie-break; omitting that collapse
overstates usage by 2–5× on real logs.

Model ids are matched exactly, against the two ids DeepSeek documents today.
Retired ids are not mapped: they are no longer selectable. Any id that starts
with `deepseek` but has no price entry still contributes tokens and marks the
amount with a trailing `?`, so a rename or a provider switch shows up instead of
silently pricing at zero.

## Alternatives Rejected

- **Shelling out to `ccusage`.** Online mode refetches its price catalog on every
  run (1.4–9.5s measured here); `--offline` uses a bundled snapshot that has no
  `deepseek-flash` price, so it reports a cost of zero for the model actually in
  use. Its current Rust implementation does price DeepSeek per event
  (`pricing.find_at(model, timestamp)`), but its bundled dollar table does not
  match the official RMB page, so it is a design reference, not a dependency.
- **Reading cc-switch's SQLite database.** It already stores per-request rows and
  daily rollups for both agents, and answers a 30-day query in 70ms. It was
  rejected because those rows are *derived from the same session logs* (the
  `_codex_session` / `_session` provider ids), so it is a cache rather than a
  better truth; its DeepSeek pricing is unusable (both families pinned to flash
  peak rates, no schedule, no effective dates); reading SQLite would need a new
  crate or a `sqlite3` subprocess in the refresh path; and the feature would go
  blank whenever the app is not running. The database stays a *verification
  oracle* during development — a same-day comparison matched cache-hit tokens
  exactly.
- **Prefix-matching `deepseek*` for pricing.** It would silently absorb any
  future id at whatever rate happened to be in the table.
- **A CLI subcommand for 7/30-day reports.** Replaced by a click on the row: it
  needs no new public command surface, and the scanner's cache makes switching
  cheap.

## Upstream Compatibility

Additive except for two small existing-code edits: `AppState` gains a `usage`
field, and `FrameLayout` gains `quota_spend_row` so the click can tell the spend
row from the rest of the block. No change to adapters, hook handlers, the query
layer, or CLI entry points; `src/ui/mod.rs` is untouched. The feature is
fork-only personal policy and is not intended to be upstreamed.

## Conflict and Removal Strategy

Upstream merges keep upstream's structure and reapply: (1) the `usage` field in
`AppState::new`, (2) `usage_rx` in `src/app/workers.rs` and its drain in
`src/app.rs`, (3) the trigger call in `AppState::refresh`, (4) the usage row and
`QuotaRows` return in `src/ui/panes/quota.rs`, and (5) the `quota_spend_row`
click branch in `src/state/layout.rs`. Runtime off switch: `tmux set -g
@sidebar_quota off` (the row shares the block's switch). Full removal deletes
`src/usage.rs`, the five call sites above, the `agent_deepseek` color slot, and
this record; nothing is persisted, so no data migration is involved.
