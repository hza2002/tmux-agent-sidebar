# Reference Design: Agent Usage Metering and API-Equivalent Cost

> Status: product decisions confirmed; implementation uses Option 0a, 2026-10-26.
>
> Implementation contract: [decision record](../decisions/2026-10-26-usage-metering.md)
> and [usage guide](../usage-metering.md). Revision 2026-10-26.2 uses an explicitly
> disclosed official-price baseline for initial Astra history; verification time
> does not truncate coverage. Future changes remain effective-dated versions.
> Missing tier uses a disclosed Standard estimation baseline; explicit service
> tiers retain their own pricing. Provider/account do not gate model estimates.
> The footer now shares one ` Usage` header; compact mode omits headings.
> The durable-store sections below remain
> reference alternatives, not a description of the implemented scanner.
>
> This document is not a decision record. It separates verified facts,
> assumptions, options, and a provisional recommendation so a reviewer can
> challenge any layer independently. The reviewer is asked to find missing
> options, invalid assumptions, and weaker-than-claimed guarantees before a
> decision record is written.

## Reviewer Brief

Review this document as an architecture proposal for a local-only usage
metering subsystem in `tmux-agent-sidebar`. The immediate product need is a
Codex token row with API-equivalent cost, alongside DeepSeek under the shared footer header, but
the design question is broader: what is the best-practice architecture for
collecting, normalizing, pricing, storing, and presenting agent usage in this
repository?

Please pay particular attention to:

1. Whether the event source choice (vendor session logs vs hooks vs app-server)
   is justified by the stated requirements, not by familiarity with the current
   implementation.
2. Whether the proposed correctness guarantees (idempotency, replay,
   time-window semantics, deduplication) are actually achievable with the
   available source data.
3. Whether the proposed abstraction boundaries are the right ones, or whether
   they add layers without buying correctness.
4. Whether the storage recommendation and the dependency trade-off are sound.
5. Which claims are unverified and should be tested before implementation.

## Contents

- [Problem](#problem)
- [Confirmed Product Decisions](#confirmed-product-decisions)
- [Goals](#goals)
- [Non-Goals](#non-goals)
- [Repository Constraints](#repository-constraints)
- [Verified Facts](#verified-facts)
- [Assumptions and Unverified Claims](#assumptions-and-unverified-claims)
- [Requirements](#requirements)
- [Options](#options)
- [Comparison Matrix](#comparison-matrix)
- [Selection Guide](#selection-guide)
- [Provisional Recommendation](#provisional-recommendation)
- [Reference Architecture (conditional on a durable store)](#reference-architecture-conditional-on-a-durable-store)
- [Correctness Edge Cases](#correctness-edge-cases)
- [Performance Analysis](#performance-analysis)
- [Migration and Rollout](#migration-and-rollout)
- [Open Questions for the Reviewer](#open-questions-for-the-reviewer)
- [Validation Plan](#validation-plan)
- [Sources](#sources)
- [Glossary](#glossary)

## Problem

The sidebar's quota block shows subscription quota as remaining percentages for
Codex and Kimi. Those percentages do not expose the token composition or the
API-equivalent cost of the work that consumed them. The user wants, for Codex
in particular:

- token counts split by class (uncached input, cached input, output);
- per-model attribution for the two or three models actually used
  (`gpt-6-astra`, `gpt-6.1-sol`, and an occasional image model);
- the current weekly quota cycle and exact rolling 24-hour / 7-day / 30-day windows;
- an API-equivalent cost computed from official list prices, clearly labeled as
  an estimate rather than a billed amount.

The existing `ds` row already answers a narrower version of this question for
DeepSeek: it scans Codex and Claude Code session logs, filters DeepSeek models,
prices them with DeepSeek's published RMB table, and renders one row in the
quota block. See
[`docs/decisions/2026-09-18-deepseek-usage-row.md`](../decisions/2026-09-18-deepseek-usage-row.md)
and [`src/usage.rs`](../../src/usage.rs).

## Confirmed Product Decisions

These product decisions were confirmed with the user on 2026-10-26. Storage,
source correctness, and cycle-boundary discovery remain engineering decisions
subject to validation.

### Distinct quota and usage metrics

Quota and Usage are distinct metrics under one shared header, using the existing
background worker, state, and rendering seams. Compact mode omits the header. Quota shows remaining percentages and reset
countdowns. Usage shows tokens and money. Codex has two usage rows:

- The cycle row always reports the current weekly quota cycle, not the monthly
  subscription payment period.
- The recent row cycles through exact rolling 24h, 7d, and 30d windows on click;
  its default is 7d. Both token totals and estimated cost switch together.

Illustrative layout (values are examples; labels are English in this document):

```text
 Usage
 codex 5h 62% 2h10m wk 35% 3d8h
 kimi  5h 80% 3h20m wk 72% 4d2h
 cx周  $128.40 18.2M cache99%
 cx7d  $173.20 24.6M cache98%
 ds    CNY1.47 12.3M cache98% off-peak
```

This is a content sketch, not a width-verified terminal capture. Preserve the
DeepSeek amount, tokens, cache-hit ratio, and today's current peak/off-peak
indicator. Multi-day DeepSeek totals combine price periods and must not claim
one peak/off-peak state. Keep its existing local-calendar today/7-day/30-day
windows and spend colors; Codex's new recent windows use elapsed durations.
Codex has no peak/off-peak field under the current product requirements.

Codex estimates use a positive daily-average utilization color ladder, with the estimate
explanation in details. Color indicates magnitude, not subscription charges. Model breakdowns and disjoint token
classes belong in detail views, along with exact window bounds, local coverage,
and the price-book revision. Hide the recent row first when space is scarce,
then the Usage section; preserve quota visibility and agent-list priority.

### Cycle and rolling windows

| Codex view | Half-open query interval | Reset behavior |
|---|---|---|
| Current cycle | `[verified_cycle_start, now)` within the confirmed cycle | Changes only when the new cycle is confirmed |
| Last 24h | `[now - 86,400 seconds, now)` | Rolls continuously |
| Last 7d | `[now - 604,800 seconds, now)` | Rolls continuously |
| Last 30d | `[now - 2,592,000 seconds, now)` | Rolls continuously |

Use provider cycle metadata. Deriving the start from reset time minus duration
requires validation of the provider's actual reset semantics. Neither Monday
midnight nor a rolling 7-day interval is a substitute. If the boundary cannot
be established, show cycle unavailable and keep the recent row usable. At the
reset boundary, refresh quota metadata and show a pending cycle state until the
new boundary is confirmed; do not show old usage as a new cycle or invent zero.

The quota API and local logs may update at different cadences. Sharing cycle
bounds does not make tokens or API-equivalent dollars convertible to quota
percentages. The usage scope is local records, not guaranteed account-wide
consumption. Validate account attribution before claiming that records belong
to the subscription shown; model-provider matching alone is insufficient.
Incomplete coverage must remain visible.

### Maintain historical price books

For every usage event, first select the price version effective at its event
time, then apply that version's model/tier rates and dynamic schedule. A version
owns the complete schedule, including peak/off-peak definitions and timezone.
A future revision can change rates, peak hours, or both without changing the
interpretation of older events. Fixed rates are a schedule with one rate set.

Normal price changes append versions and retain history. Historical corrections
are explicit revisions with a reason and a new price-book revision identifier.
Keep the previous revision available for reproducing an earlier estimate.
Missing historical rules produce an unpriced marker, never today's rate.
Every window is the sum of individually priced events, including windows that
cross multiple price versions. Updating the price book invalidates cost caches
without changing token counts.

## Goals

- Local-only: no network calls, no credentials, no prompt content persisted.
- Accurate token accounting per model and per time window.
- API-equivalent cost with explicit currency, price version, and effective
  dates.
- Replayable while sources remain available: each estimate identifies its
  source coverage and price-book revision; retain historical pricing revisions.
- Incremental: steady-state work is proportional to new data, not total history.
- Near-real-time: updates at least at turn boundaries, optionally within a turn.
- Extensible to a small number of providers and models without a plugin system.
- Observable: missing, stale, unknown, or unpriced data is visible rather than
  silently counted as zero.

## Non-Goals

- Replacing vendor billing or claiming exact subscription spend.
- Multi-user, multi-tenant, or remote telemetry.
- Persisting prompts, assistant output, or tool output.
- A general-purpose analytics platform or a dynamic pricing DSL.
- Exact cost when the source does not expose the required tier or billing basis;
  in those cases the design must surface uncertainty.

## Repository Constraints

The following constraints come from the repository instructions and existing
architecture, not from the author's preference:

- `src/usage.rs` is fork-only. A rewrite has no upstream merge cost, but the
  existing `ds` row output is a regression surface.
- Rendering runs on the TUI thread and must not block, spawn slow processes, or
  re-read large histories at tick rate.
- The repository avoids new dependencies unless the task proves one is
  necessary. A durable, idempotent, queryable ledger is a candidate for such a
  proof; the reviewer should judge whether it clears the bar.
- State flow stays one-directional: sources -> normalized events -> local state
  -> render. A second parallel query or render pipeline is out of bounds.
- All tests must avoid the developer's live tmux server. Ratatui frame tests
  use inline `insta` snapshots.
- Files under `docs/` are written in English.

## Verified Facts

The following facts were observed locally or read from the installed Codex
0.164.0 schemas and upstream source. They are inputs to the options, not
conclusions.

### Codex session logs

| Fact | Evidence |
|---|---|
| A session is a JSONL rollout under `$CODEX_HOME/sessions/YYYY/MM/DD/`. | Local corpus; existing parser `src/usage.rs:645`. |
| `turn_context` events carry the active model and `turn_id`. | Local sample; parser reads `payload/model` at `src/usage.rs:671`. |
| `token_count` events carry `timestamp`, `info.last_token_usage`, `info.total_token_usage`, and a `rate_limits` block. | Local sample; parser reads `last_token_usage` at `src/usage.rs:697`. |
| `last_token_usage` has `input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`, `reasoning_output_tokens`, `total_tokens`. | Local sample. |
| `total_tokens = input_tokens + output_tokens`; `reasoning_output_tokens` is a subset of output in observed samples. | Local sample: 35,851 input + 191 output = 36,042 total, with 129 reasoning. |
| `input_tokens` includes cached input. | Existing decision record and parser comment `src/usage.rs:713`. |
| Forked sessions copy history and declare `forked_from_id` / `parent_thread_id` in `session_meta`. | Local sample for the metadata; duplicate-event regression test at `src/usage.rs:1152`. |
| Subagent sessions declare `thread_source: subagent` and `source.subagent` with the parent thread id. | Local sample. |
| The local corpus is ~1.8 GB total and ~501 MB for October 2026, across 191 `.jsonl` files. | `du -sh ~/.codex/sessions`; `find` plus `wc -l` over the October tree. |
| Only `gpt-6-astra` and `gpt-6.1-sol` appear as models in the local corpus. | `rg -o '"model":"[^"]+"' ~/.codex/sessions` over 1.8 GB. |
| No `image_generation` output events were found; only `input_image` content. | `rg -o '"type":"[^"]*image[^"]*"'` over 1.8 GB. |
| A 7 MB active rollout with 2,050 lines and 303 relevant lines parsed in ~9 ms in Python. | Local measurement; not a Rust benchmark. |

### Claude Code session logs

| Fact | Evidence |
|---|---|
| Assistant entries carry `message.model`, `message.usage`, and `message.id`. | Existing parser `src/usage.rs:739`; existing decision record. |
| `input_tokens` excludes cache; cache hits are in `cache_read_input_tokens`; writes are in `cache_creation_input_tokens`. | Existing decision record. |
| Streaming writes one entry per content block with the same `message.id`. | Existing dedup logic and regression test `src/usage.rs:1160`. |

### Codex hooks (0.164.0)

| Fact | Evidence |
|---|---|
| `PostModelRequest` is a supported hook event. | `HookEventName` in the generated app-server schema; upstream `codex-rs/hooks/schema/generated/model-request.command.input.schema.json`. |
| Its input includes `model`, `status` (`completed`/`failed`/`cancelled`), `error`, `token_usage`, `session_id`, `turn_id`, `cwd`, `transcript_path`, `agent_id`, `agent_type`. | Upstream schema. |
| `token_usage` has `input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`, `reasoning_output_tokens`, `total_tokens`. | Upstream schema. |
| The payload has no timestamp and no request id. | Upstream schema; top-level required fields are `cwd`, `model`, `session_id`, `transcript_path`, `turn_id`. |
| `PostModelRequest` is invoked per sampling request, not per user turn. | Upstream `codex-rs/core/src/session/turn.rs`: `try_run_sampling_request` calls `run_model_request_hooks` per request; a turn with tool calls makes multiple requests. |
| Hook handlers default to `async: false`; synchronous handlers are awaited, though they may run in parallel. | Upstream `codex-rs/core/config.schema.json` (`"async": {"default": false}`); dispatcher source. |
| Hook registration lives in the user's `~/.codex/hooks.json`; changing it requires re-running `setup codex`. | Repository instruction; local `~/.codex/hooks.json`. |

### Codex app-server (0.164.0)

Generated locally with
`codex app-server generate-json-schema --out <dir>`:

| Fact | Evidence |
|---|---|
| `account/usage/read` accepts an optional `threadId`. | `v2/NullableGetAccountTokenUsageParams.json`. |
| It returns a summary, daily usage buckets, and optional per-thread usage. | `v2/GetAccountTokenUsageResponse.json`. |
| Per-thread usage groups carry `model`, `inputTokens`, `cachedInputTokens`, `outputTokens`, `totalTokens`, `reasoningEffort`, `speed`, `estimatedUsageCreditsMicros`, and optionally `estimatedUsageUsdMicros`. | `v2/GetAccountTokenUsageResponse.json`. |
| `thread/tokenUsage/updated` notifications carry `threadId`, `turnId`, and last/total token usage, but no model. | `v2/ThreadTokenUsageUpdatedNotification.json`. |
| The sidebar already has read-only control-socket transport for Codex daemon routing. | `src/cli/hook/codex_routing/transport.rs`; `docs/codex-daemon-routing.md`. |

### Pre-implementation baseline

| Fact | Evidence |
|---|---|
| The scanner caches parsed records per file by `(mtime, size)`. | `src/usage.rs:559`. |
| A changed file is re-parsed from byte 0; there is no byte-offset checkpoint. | `src/usage.rs:559` and `parse_codex_file`. |
| Codex directories are bounded to the window start minus three days. | `src/usage.rs:511`; `DIR_SLACK_DAYS` at `src/usage.rs:273`. |
| Files are mtime-prefiltered and cheap-substring-filtered before JSON parsing. | `src/usage.rs:602`, `src/usage.rs:663`. |
| The force trigger is a pane status stamp; a 30-second timer is the fallback. | `src/state/refresh.rs:301`; `src/usage.rs:947`. |
| Cost is accumulated as `f64` in CNY. | `src/usage.rs:322`. |
| The Codex dedup key is `(timestamp, uncached, cached, output)`, without `turn_id` or model. | `RecordId::Codex` at `src/usage.rs:423`. |
| A turn-boundary status change already updates the row in roughly 1-2 seconds: sidebar refresh is 1 s, the scanner sleeps in 1 s slices. | `src/app.rs:46`; `src/usage.rs:975`. |

## Assumptions and Unverified Claims

These are not facts. They should be resolved before a final decision.

1. `PostModelRequest` fires for custom-provider models (the local
   `session_meta.model_provider` can be `custom`).
2. The hook's `token_usage` equals the log's `last_token_usage` for the same
   request, and the hook fires once per request with no retries or omissions.
3. The log line for a request is durably written before the corresponding
   `PostModelRequest` hook fires.
4. `token_count` covers image-generation usage. Vision input appears as
   `input_image` content and is likely counted as input tokens; image
   generation may be billed per image and may not appear in `token_count`.
5. Resolved for selected mode: `event_msg.thread_settings_applied` carries
   `thread_settings.service_tier`. A metadata-only scan of 194 local files found
   1,177 explicit `default` settings and nine absent/null settings. All 14,403
   usage notifications followed an explicit `default` setting. `turn_context`
   omits tier, so the parser retains the settings timeline. Server-served tier
   and automatic downgrade remain unverified.
6. `account/usage/read` returns non-null `estimatedUsageUsdMicros` for the
   user's ChatGPT plan, and can enumerate archived threads for historical
   windows.
7. The rollout JSONL format is stable across Codex versions. It is an internal
   format, not a documented contract.
8. The hook schema is stable across Codex versions. It is generated and
   official, but new (0.164.x).
9. A 90-day backfill over the current corpus completes within an acceptable
   one-time budget. This needs a Rust benchmark.
10. The sidebar is a singleton writer per machine. Multiple instances or
    multiple tmux servers would need a single-writer lock or a concurrent store.

## Requirements

### Functional

- F1: Collect per-request token usage with event time and model.
- F2: Attribute usage to provider, model, and optionally session/thread/turn.
- F3: Price events with historical versions of both rates and dynamic schedules.
- F4: Report Codex cycle and rolling 24h/7d/30d aggregates per model; preserve
  DeepSeek local-calendar windows and its cache/price-period fields.
- F5: Report cost in the provider's currency; do not perform implicit FX.
- F6: Mark unknown models, unknown tiers, and non-token billing explicitly.
- F7: Distinguish measured usage from estimated API-equivalent cost.
- F8: Work for logs written while the sidebar was not running.
- F9: Work when the sidebar is running outside tmux or without hooks.
- F10: Include subagent usage with attribution when supported by source evidence.
- F11: Keep quota and usage display sections separate; never imply local usage
  equals account-wide quota consumption.
- F12: Handle unknown or pending cycle boundaries without inventing a window.

### Non-Functional

- N1: Idempotent ingestion; re-reading a file or rebuilding the store must not
  change aggregates.
- N2: Replayable; derived data can be deleted and rebuilt from sources.
- N3: Incremental; steady-state work is proportional to appended bytes.
- N4: Non-blocking; no source I/O on the TUI thread.
- N5: Observable; coverage, lag, parse failures, unknown models, and unpriced
  events are queryable.
- N6: Testable without live tmux, real network, or the developer's real logs.
- N7: Privacy-preserving; only usage metadata is persisted.
- N8: Extensible to a small number of providers/models without a plugin
  framework.
- N9: Operable; one command to inspect status and rebuild.

## Options

### Option 0: Keep the current scanner and add a Codex row by parameterization

Mechanism: keep `src/usage.rs` as the single full-file scanner; generalize
`Spend` to carry a currency and add a second price table and row.

Strengths:

- Smallest behavioral delta from the working `ds` row.
- Existing tests and performance characteristics are understood.
- No new storage, dependency, or migration.

Weaknesses:

- The monolith already mixes ingestion, normalization, deduplication, pricing,
  aggregation, and presentation state. Parameterizing it increases coupling.
- Still re-parses changed files from byte 0.
- No checkpoints, no replayable event store, no coverage/lag observability.
- A second row requires touching the aggregate, state, height, and renderer;
  a third provider repeats the pattern.
- Currency and schedule differences leak into the aggregate.

Fit: adequate for one provider, weak for the stated multi-provider,
auditability, and observability goals.

### Option 0a: Incremental scanner with a rebuildable in-memory cache

Mechanism: extend the existing background scanner with parser checkpoints and
separate pricing/query functions, serving both sections through existing state.
Rebuild the cache from vendor logs after restart; add no durable event store.

This is the minimum additional option to measure against Option 1. It avoids
store concurrency, migrations, and new commands, but startup backfill may cost
more and inspection is limited to available logs. Measure startup time and
memory before claiming a durable store is necessary. It still needs correct
identity, parser recovery, source discovery, and coverage handling.

### Option 1: Canonical log ledger with incremental ingestion

Mechanism: source adapters read vendor logs, normalize into a canonical
`UsageEvent`, ingest into an event store with per-file checkpoints, price at
query time, and serve window aggregates.

Strengths:

- Source timestamps allow event-time windows; precision and event semantics
  still need validation before claiming accounting completeness.
- Historical backfill is available immediately from existing logs.
- Replay and re-ingestion are possible once event identity and replacement
  semantics have been validated.
- File checkpoints make steady-state ingestion proportional to new bytes.
- The store is independent of sidebar lifetime and tmux.
- One normalized event model supports multiple providers and future sources.
- Observability is natural: source files, checkpoints, parse errors, coverage.

Weaknesses:

- More components than the current scanner: adapters, store, checkpoints,
  query, price book.
- The vendor log is an internal format and may change. The adapter needs
  versioning and a drift signal.
- Codex model attribution depends on the last `turn_context`; a `token_count`
  before the first `turn_context` has no model.
- Existing dedup key must be improved (`turn_id`) to avoid merging independent
  sessions.
- Image-generation and non-token billing may not be present in the log.

Fit: strong for history, correctness, and extensibility; the main risk is log
schema stability.

### Option 2: Hook payload ledger

Mechanism: register `PostModelRequest`; each hook invocation writes its
`token_usage`, model, session, and turn into a durable store; the sidebar reads
the store.

Strengths:

- Per-request, low-latency data directly from an official schema.
- Model is explicit in the payload; no `turn_context` inference.
- Includes `status` and `agent_id`/`agent_type`, which logs do not expose
  directly.
- Works without parsing the rollout format.

Weaknesses:

- No timestamp and no request id in the payload. The hook process arrival time
  is not the request time; async or delayed hooks can cross day boundaries and
  break window attribution.
- No historical backfill. Before installation, there is no data.
- Fire-and-forget: a failed, timed-out, or uninstalled hook loses the event. The
  source log still has it.
- Per-request process spawn. The default is synchronous (`async: false`), so
  every model request waits for the hook; setting `async: true` introduces
  delay and possible loss.
- Requires `~/.codex/hooks.json` trust and a re-paste of `setup codex`.
- Multiple hook processes need a concurrent store or a single-writer channel.
- Deduplication is weak without a request id; retries or duplicate deliveries
  are hard to distinguish.
- Hard to reconstruct subagent/compaction semantics without additional
  correlation.

Fit: attractive for latency, weak as the sole ledger. It becomes much stronger
if combined with the log as the durable source of truth.

### Option 3: Hybrid: canonical log ledger + hook trigger

Mechanism: Option 1 is the ledger; hooks (and optionally filesystem events)
only enqueue an ingestion trigger for the corresponding source. The hook payload
is not used for accounting. Incremental ingestion makes the trigger cheap.

Strengths:

- All strengths of Option 1: timestamps, history, replay, dedup, tmux
  independence.
- Near-real-time when hooks are present; the 30-second timer and status stamps
  remain fallbacks when they are absent.
- Hook failure only degrades latency, not data.
- The hook payload can be used later for reconciliation without becoming the
  ledger.

Weaknesses:

- Still depends on the log schema.
- Still pays a hook process per request if per-request latency is enabled.
- Requires both a hook registration and an ingestion trigger path, so two
  configuration surfaces can drift.
- The log-flush-before-hook ordering is unverified; a trigger may need a small
  debounce or a retry.

Fit: optional latency extension if existing turn-boundary updates prove insufficient.

### Option 4: App-server usage API polling

Mechanism: query `account/usage/read` for loaded threads (the sidebar already
has control-socket transport) or subscribe to `thread/tokenUsage/updated`.

Strengths:

- Official API; per-model breakdown and estimated credits/USD are provided.
- No log parsing and no per-request hook process.
- Fits the existing daemon-routing architecture.

Weaknesses:

- `thread/tokenUsage/updated` does not carry the model.
- Per-thread usage requires enumerating threads; archived or deleted threads may
  be unavailable, so historical completeness is uncertain.
- `estimatedUsageUsdMicros` may be null for subscription billing; credits are
  not USD.
- Polling cost and cadence need design; a notification subscription is a new
  long-lived client.
- Not a complete local history; it is a vendor view with its own definition of
  "usage".

Fit: strong as a cross-check or a future primary if the API proves complete;
not sufficient alone for the stated historical, per-model cost goal without
validation.

### Option 5: External tool or database

Mechanism: shell out to `ccusage`, read cc-switch's SQLite database, or use
another existing usage aggregator.

Strengths:

- Reuses an existing implementation and price catalog.

Weaknesses:

- The existing decision record rejected `ccusage` (online price refresh latency;
  offline snapshot lacks the DeepSeek model) and cc-switch (derived from the same
  logs; DeepSeek pricing unusable; blank when the app is not running).
- Adds a runtime dependency and a second source of truth.
- Does not solve the Codex per-model API-equivalent cost requirement without
  additional mapping.

Fit: weak; retained as a verification oracle only. See
`docs/decisions/2026-09-18-deepseek-usage-row.md`.

## Comparison Matrix

`Strong` / `Adequate` / `Weak` / `Unknown` are qualitative, not scores.

| Criterion | 1 Log ledger | 2 Hook ledger | 3 Hybrid | 4 App-server | 5 External |
|---|---|---|---|---|---|
| Event timestamp fidelity | Strong | Weak (no timestamp) | Strong | Strong (thread/turn) | Varies |
| Historical backfill | Strong | Weak (none) | Strong | Unknown | Unknown |
| Idempotency / dedup | Unverified identity | Weak (no request id) | Unverified identity | Requires API validation | Varies |
| Real-time latency | Adequate (status/30 s) | Strong (per request) | Strong (per request) | Adequate (poll) | Varies |
| Durability / missed events | Strong | Weak | Strong | Strong (vendor) | Weak |
| Per-model attribution | Adequate (`turn_context`) | Strong (`model`) | Adequate | Strong (groups) | Varies |
| Schema stability / contract | Weak (internal log) | Strong (official hook) | Weak | Strong (official API) | Varies |
| Runtime overhead | Adequate (incremental scan) | Weak (process per request) | Adequate | Adequate (poll) | Varies |
| Installation / configuration | Strong (no hook) | Weak (hooks.json + trust) | Adequate | Adequate (daemon) | Varies |
| Works without hooks/tmux | Strong | Weak | Strong | Strong | Varies |
| Non-token billing (image) | Unknown | Unknown | Unknown | Unknown | Unknown |
| Operability / observability | Strong (own store) | Adequate | Strong | Adequate (vendor view) | Weak |
| Testability | Strong (fixtures) | Adequate (payload fixtures) | Strong | Adequate (mock RPC) | Weak |
| Implementation complexity | Medium | Medium | Medium-High | Medium | Low |

### Selection Guide

If the reviewer finds the priorities different from the ones assumed here, this
table maps the dominant priority to the option that best serves it.

| Dominant priority | Lean toward | Why |
|---|---|---|
| Historical completeness and auditability | 1 or 3 | Per-event timestamps, backfill, replay |
| Official schema stability | 2 or 4 | Hook and app-server schemas are generated contracts |
| Intra-turn latency | 2 or 3 | Per-request hook event or trigger |
| Minimal configuration and operational surface | 1 | No hook registration, no store daemon |
| No local price-table maintenance | 4 | Vendor-provided estimated credits/USD |
| No new dependency | 1 or 3 with JSONL | File-based ledger |
| Cross-source reconciliation | 3 | Log ledger plus hook/app-server cross-checks |
| Vendor-billed truth over local estimates | 4 | Vendor usage API, subject to completeness |

## Provisional Recommendation

Prefer log ingestion with the existing status trigger and fallback polling.
Option 1 is the provisional durable-store design; compare it against Option 0a
before committing to persistence. The confirmed product requires turn-boundary
updates, not a process spawn for every model request. Option 3 remains an
optional latency extension only if measured latency fails product needs.

Logs support historical windows without a new hook installation, but identity,
coverage, and parser recovery remain unverified correctness obligations. Do not
infer those guarantees from the choice of storage engine. Keep changes in the
existing usage worker and state/render seams.

If durable storage is justified by measured startup or operational needs, prefer
SQLite over building JSONL update, locking, and recovery protocols. This is an
engineering recommendation, not a confirmed dependency decision. Account-wide
API data can replace local accounting only after its scope, time resolution,
model breakdown, and billing basis are verified. Vendor USD estimates do not
automatically implement the historical list-price policy confirmed above.

### Strongest Counterarguments

- **Against Option 3.** It still trusts an internal log format. Option 2 uses an
  official schema. If log drift is judged the dominant risk, a hook ledger with
  a one-time log backfill may be safer. The missing hook timestamp can be
  mitigated by correlating `session_id`/`turn_id` with the last matching log
  entry, or by accepting hook-arrival time for intra-turn display while keeping
  logs authoritative for day boundaries.
- **For Option 4.** If `account/usage/read` returns complete per-model history
  and non-null USD for this plan, it may simplify usage collection. Its estimates
  still need billing-basis validation; they do not replace the confirmed
  historical price-book policy merely by being denominated in USD.
- **For Option 1.** If per-request latency is not a product requirement, the
  hook trigger buys little: the existing status stamp already updates at turn
  boundaries, and the trigger adds a hook registration and a process per model
  request.
- **Against all log-based options.** The vendor may change or remove the
  rollout format at any time; the hook and app-server schemas are generated
  contracts. A hybrid that treats the log as a cache and the hook as the
  eventual source could invert the provisional recommendation if the hook
  payload gains a timestamp or request id.

## Reference Architecture (conditional on a durable store)

### Layers

```text
Codex rollout ─┐
Claude projects ─┼─> Source adapters ─> Canonical UsageEvent ─> Event store ─> Query/Aggregate ─> UI rows
Existing trigger ─┘      normalize/id/checkpoint      idempotent      window/group      ds, cx
App-server (optional) -> Reconciliation diagnostics (does not add usage)
```

Boundaries:

- **Adapter**: reads one source, normalizes token classes, computes a stable
  event id, maintains a checkpoint. It does not know prices or UI.
- **Event store**: idempotent writes and time/model queries. It does not parse
  vendor formats.
- **Price book**: maps `(provider, model, tier, occurred_at)` to a currency and a
  rate schedule. It does not aggregate.
- **Query**: computes windows and groups. It does not read files.
- **Row spec**: maps a provider filter to a label, currency, and ladder. It does
  not own data.
- **Trigger**: invalidates or enqueues ingestion. It does not carry accounting
  data.

### Canonical event

```rust
pub struct UsageEvent {
    pub id: EventId,              // stable across files and re-ingestion
    pub occurred_at: EventTime,  // UTC, preserving source precision
    pub observed_at: i64,        // ingestion time
    pub source: SourceId,        // codex_rollout | claude_projects | hook | app_server
    pub provider: Provider,      // deepseek | openai | anthropic
    pub model: ModelId,          // normalized slug
    pub raw_model: Box<str>,     // diagnostic
    pub tier: Option<ServiceTier>, // absent keeps missing evidence; pricing uses a disclosed Standard baseline
    pub account_scope: AccountScope, // verified identity or unknown
    pub tokens: TokenUsage,      // disjoint classes
    pub billing: BillingBasis,   // Tokens | Images { count, size } | Unknown
    pub session_id: Option<Box<str>>,
    pub turn_id: Option<Box<str>>,
    pub request_id: Option<Box<str>>,
    pub quality: Quality,        // Measured | Estimated
}

pub struct TokenUsage {
    pub uncached_input: u64,
    pub cached_input: u64,
    pub cache_write_input: u64,
    pub output: u64,             // includes reasoning
    pub reasoning_output: u64,   // analysis only; never added to output
}
```

Normalization rules:

- Codex: the existing parser uses `input_tokens - cached_input_tokens`. Verify
  whether cache-write tokens overlap input before declaring disjoint classes;
  reject or flag inconsistent counters rather than silently saturating them.
  Reasoning is a subset of output and is never added a second time.
- Claude: `input_tokens` excludes cache; `cache_read_input_tokens` is cached;
  `cache_creation_input_tokens` is a cache write.
- Compute `total` from the disjoint classes; do not use the vendor's total as
  the canonical total without a consistency check.

### Event identity and deduplication

Identity requirements:

- Codex `turn_id` separates independent turns but is not a request id. A hash of
  turn, timestamp, model, and token classes is only a candidate fingerprint.
  Preserve timestamp precision and validate repeated notifications, multiple
  requests in one turn, fork copies, and cumulative counter behavior before
  choosing an identity. Retain source provenance and report unresolved ambiguity.
- Claude `message.id` requires a deterministic merge rule: retain the entry with
  the largest token total, consistent with the existing parser; define a stable
  tie-break for equal totals with different classes. A later larger entry must
  update the stored record and invalidate aggregates. `INSERT OR IGNORE` alone
  is insufficient. Missing IDs need a separately tested fallback policy.
- Do not combine hook or API records with log events for accounting until a
  cross-source identity policy is proved. Cross-checking is separate from adding.
- Missing model/turn context is a coverage failure. Retain diagnostic metadata
  without prompt content; never assign a session identity as if it were a model.

Re-reading and rebuilding must produce the same result regardless of file or
arrival order. Event identity and source provenance are separate: one event may
have evidence in multiple files, which matters when one file is replaced.

### Ingestion and checkpoints

A checkpoint records file identity/generation, offset at a complete-line
boundary, parser version, and the context needed to resume parsing (including
model, turn, session, and any counters used for identity). An offset alone loses
context when the next appended line is a usage event.

Algorithm requirements:

1. Discover sources independently of event-window filtering. Date directories
   plus a small slack cannot prove coverage: an older session may still append.
   Keep known files discoverable and validate how to find old files first seen
   after startup or downtime. Archive coverage is also a validation item.
2. Resume append-only files using the saved parser context. Keep the offset at
   the last complete line and reread incomplete tails; do not persist arbitrary
   prompt-containing line fragments in the derived store.
3. Commit event inserts/updates, source provenance, parser context, and the
   checkpoint atomically in a durable store. A crash must not advance a cursor
   past uncommitted events. An in-memory cache restarts from source logs.
4. On truncation or rewrite, replace that file generation's contributions.
   Merely reinserting new IDs leaves removed or corrected events behind. Retain
   events still supported by another file. Define rewrite detection limits:
   size, mtime, inode, and a prefix hash do not detect every in-place rewrite.
5. Parser upgrades rebuild the affected derived data. A replay must converge
   with a clean rebuild, including corrected events and deleted contributions.

A proposed 90-day retention horizon is an engineering sizing choice, not a
confirmed product requirement. Coverage describes successfully scanned sources
and missing intervals, not just the timestamp of the newest event. Rebuild is
reproducible only while source logs and the requested price revision remain
available; a derived store is not a replacement for source retention.

### Store

The architecture is storage-engine agnostic. Two viable engines:

| | Append-only JSONL | SQLite |
|---|---|---|
| Idempotency | Application-level event-id set | `UNIQUE(event_id)` + deterministic upsert |
| Checkpoints | JSON sidecar | Table |
| Window queries | Load events for the window | Index on `occurred_at`, `(provider, model)` |
| Concurrency | Single writer + `flock` | WAL + busy timeout |
| Migrations | Rebuild | `user_version` |
| Size | ~200 MB per million events | More compact |
| Dependency | None | `rusqlite` (bundled or system SQLite) |
| Debuggability | `jq`, plain files | `sqlite3` |
| Failure mode | Partial trailing line, repaired on read | Corruption, needs backup/rebuild |

Provisional position: if a new dependency is acceptable, SQLite gives stronger
schema-level guarantees for idempotency, checkpoints, and migrations. If the
repository's dependency-free rule is treated as binding, an append-only JSONL
ledger would also need transactional checkpoint recovery, deterministic
updates, source replacement, a single-writer protocol, and compaction before
it could claim the same invariants. The reviewer should decide whether
the dependency is justified.

### Pricing

```rust
pub struct PriceRule {
    pub provider: Provider,
    pub model: ModelId,
    pub tier: ServiceTier,
    pub currency: Currency,
    pub schedule: RateSchedule,
    pub effective_from: i64,
    pub effective_to: Option<i64>,
    pub version: u32,
}

pub enum RateSchedule {
    Flat(TokenRates), // includes an explicit cache-write pricing policy
    TimeOfDay {
        off_peak: TokenRates,
        peak: TokenRates,
        windows: &'static [PeakWindow],
        timezone: ScheduleTimezone,
    },
    PerImage { size: ImageSize, micros: i64 },
}
```

Rules:

- Money is stored as `i64` micros per currency, not `f64`. Compute with `i128`
  intermediates and round once.
- Select exactly one rule with `effective_from <= occurred_at < effective_to`,
  treating an absent end as open-ended. Reject overlaps for the same
  `(provider, model, tier)`; gaps return `Unpriced`.
- Each version owns its entire schedule, including timezone and peak windows.
  The current DeepSeek implementation versions rates but uses a global
  `is_peak()` function; that function cannot preserve history if peak hours
  change. Version the schedule before supporting such changes.
- Append normal price changes without overwriting historical rates or schedules.
  Closing the preceding interval at the new boundary preserves its past meaning.
  Historical corrections require an explicit revision and reason; retain prior
  price-book revisions so earlier estimates remain reproducible.
- Define cache-write pricing for every applicable model and source. Do not drop
  the class because a flat-rate example omits it. Unknown billing semantics
  remain unpriced. Preserve exact intermediate sums and round at the aggregate
  currency boundary, independent of event batching.
- Currency is part of the rule; DeepSeek is CNY, OpenAI is USD. Do not convert.
- Unknown model, explicit unsupported tier, or non-token billing returns `Unpriced(reason)`,
  never a zero cost.
- Provider inference: a model id that matches a known provider prefix (for
  example `deepseek`, `gpt-`, `codex-`) is assigned to that provider even when
  unpriced, so it appears in the row with an unpriced marker. A completely
  unknown id is still stored and surfaced in `usage status`, not silently
  dropped.
- Image-generation pricing requires verified billing units and source usage.
  A model-name prefix cannot establish per-image versus token billing; keep
  unsupported billing semantics unknown rather than selecting a guessed rule.
- Cost is computed at query time and tagged with the price-book version; price
  changes invalidate cached aggregates rather than rewriting events.
- API-equivalent cost is labeled as such. Subscription quota percentage and
  API list-price cost are different metrics and must not share a scale.

### Query and rows

Rows are views over the same normalized records, not storage partitions.
Implement the confirmed shared Quota/Usage footer and two Codex rows through
the existing state/render path. DeepSeek retains its established fields and
calendar-window semantics; Codex uses a cycle view and a rolling recent view.

- A Codex row needs source and account-scope predicates as well as model-provider
  matching. Unknown account attribution must be visible, not silently treated
  as the currently displayed subscription.
- Use one injected `now` per query snapshot. Codex rolling windows are exact
  elapsed durations; timezone only formats their timestamps. DeepSeek calendar
  boundaries use calendar arithmetic in the selected local timezone.
- Cycle bounds come from validated provider metadata. Persist or otherwise
  recover enough cycle identity to reject results computed for a previous cycle.
  Pending/unknown bounds leave the cycle row unavailable and recent rows usable.
- Aggregate cache identity includes data revision, price-book revision, resolved
  start/end, scope, and cycle identity where applicable. Invalidate on clock,
  timezone, and cycle transitions as relevant, even with no new log event.
- Per-model details use the same window and records as the summary. Show exact
  interval, local coverage, and price revision. Missing coverage and unpriced
  events are separate states; a recent successful scan does not prove coverage.
- Track source-check success separately from last event time. An idle source is
  not stale merely because it emitted no usage.
- Codex money uses a positive daily-average utilization color ladder. DeepSeek keeps its
  spend ladder, cache ratio, and today's current price-period indicator. Do not
  apply Codex estimates to quota percentages or a remaining-value calculation.

### Triggers and latency

- The ingestor exposes an `Ingest(source)` request. Triggers coalesce for a
  short debounce (for example 250 ms-1 s).
- Triggers: pane status stamp (existing; turn boundaries), optional
  `PostModelRequest`/`Stop` hook (per-request/intra-turn), optional filesystem
  watcher, and a 30-60 s fallback timer.
- The hook trigger does not carry accounting data. If the hook is absent or
  fails, only latency degrades.
- Verify whether the log line is flushed before the hook fires. If not, use a
  short debounce or a retry; do not assume ordering.

### Observability and operations

- A `usage status` command reports: sources, files, last event, ingestion lag,
  parse errors, unknown models, unpriced events, backfill progress, and store
  schema version.
- The UI row shows a stale marker when the source has not been ingested within a
  threshold, and an unpriced marker when cost is incomplete.
- A `usage rebuild` command deletes derived data and re-ingests from sources.
- Retention: keep raw events for the maximum query window plus a margin
  (proposed 90 days); price-book history is retained indefinitely.
- Store permissions are owner-only; no prompt or tool-output text is stored.

### Testing

- Golden fixtures for Codex and Claude log lines, including fork/resume and
  streaming duplicates.
- Ingest the same fixture twice; aggregates must be identical.
- Delete the store and rebuild; aggregates must be identical.
- Window boundary tests with a fixed clock and fixed timezone.
- Price tests: effective-date boundaries, overlapping rules, unknown
  model/tier, currency, image billing.
- A performance benchmark for a 90-day backfill and for incremental appends.
- UI snapshot tests for the shared Quota/Usage footer, both Codex rows,
  DeepSeek cache/period fields, width/height collapse, pending cycle, incomplete
  coverage, stale sources, and unpriced usage.
- No test connects to the live tmux server.

## Correctness Edge Cases

| Case | Proposed handling | Test |
|---|---|---|
| Resumed/forked Codex session copies history | Validate identity and preserve multi-file provenance | Fork fixture with parent + fork files |
| Two independent sessions with identical tokens/timestamp | Distinct `turn_id`; both count | Two files with same timestamp/tokens, different turn ids |
| Claude streaming duplicates | Keep largest entry per `message.id` | Existing streaming fixture |
| Model changes mid-session | Track the latest `turn_context`; attribute each `token_count` to it | Multi-model fixture |
| `token_count` without recovered context | Mark incomplete coverage; do not guess model | Restart between context and usage fixtures |
| Compaction requests | Included if they are model requests; attribute to the compaction model | Captured compaction fixture |
| Subagent sessions | Included; optional `agent_id`/`thread_source` attribution | Subagent fixture |
| Failed/cancelled request | Log may omit usage; hook may carry status but no tokens | Needs empirical validation |
| Local midnight / timezone change | Half-open local windows; store UTC timestamps; record query timezone | Fixed-clock/TZ tests |
| File truncation or rewrite | Replace source-generation contributions, preserving other provenance | Incremental result equals clean rebuild |
| Log format drift | Parser version; parse-error counter; status warning | Old/new fixture |
| Unknown model | Count tokens; mark unpriced | Unpriced row test |
| Image generation | Verify billing units; keep unsupported semantics unpriced | Pending source validation |
| Service tier / speed | Price per tier; mark unknown if absent | Needs empirical validation |

## Performance Analysis

Measured facts:

- The local Codex corpus is ~1.8 GB; October 2026 alone is ~501 MB across 191
  files.
- The largest active 7 MB rollout has 2,050 lines, 303 relevant lines, and
  parsed in ~9 ms in Python. A Rust parser should be faster, but this is not a
  Rust benchmark.
- The current scanner caches unchanged files, so steady-state cost is a stat
  sweep; a changed active file is fully re-parsed.
- A 30-day window reads only the date directories from the window start minus
  three days; it does not scan the full history.

Expected behavior under the proposed design:

- First backfill: reads all files in the horizon once; bounded by the same
  corpus size. Needs a Rust benchmark and a progress indicator.
- Steady state: reads only appended bytes in active files.
- Query: loads or indexes only the window's events. With 2-3 models and a
  personal corpus, this should be well within a 1-second refresh budget.
- Memory: an in-memory event-id set for a 90-day retention window; needs a
  measured estimate. If this exceeds the sidebar's budget, SQLite is the
  stronger choice.

## Migration and Rollout

These store migration steps apply only if the Option 0a comparison justifies a
durable store. Otherwise extend the existing worker without adding store commands.

1. Write a decision record after the review, including the chosen option and
   rejected alternatives.
2. Keep the existing `ds` row output as a golden regression test while the
   ledger is introduced.
3. Add the canonical event store and Codex adapter behind a feature flag or a
   new module; backfill in the background.
4. Compare backfilled DeepSeek totals with the current `ds` row for the same
   windows. Explain and test every difference; corrected deduplication or source
   discovery may legitimately change totals. Unexplained differences block rollout.
5. Switch the UI to the ledger, then remove the old scanner.
6. Rollback: retain the old path until differences have been reconciled and
   reset-boundary behavior validated. Disabling Usage must not affect Quota.

## Open Questions for the Reviewer

1. Is the rollout JSONL format stable enough to serve as the authoritative
   ledger? What evidence would justify that assumption?
2. Is the hook's `token_usage` identical to the log's `last_token_usage`? Run a
   side-by-side capture for astra, sol, and a tool-call loop.
3. Does `PostModelRequest` fire for custom providers, subagents, compaction, and
   failed/cancelled requests? What are the retry semantics?
4. Is the log line flushed before the hook fires? If not, what debounce or
   retry is required?
5. Does `token_count` cover image-generation usage? How is the user's image
   model billed?
6. Is service tier / speed available in the log or hook? If not, can an
   API-equivalent cost be honest for Fast/priority usage?
7. Does `account/usage/read` provide complete per-thread history and non-null
   USD for this plan? Can it be a primary source or only a cross-check?
8. Rows are settled: separate quota and usage, two Codex usage rows, and model
   details. Validate width, interaction, and height behavior against that contract.
9. What are the retention and backfill horizons?
10. Is a new dependency (SQLite) acceptable, or must the store remain
    dependency-free?
11. Event-time historical pricing is settled. Validate cache invalidation and
    reproducibility across explicit price-book corrections.
12. Is `turn_id` stable across fork/resume copies and across Codex versions?
13. How should the system handle multiple sidebar instances or multiple tmux
    servers writing/reading the store?
14. How should unknown models be surfaced without noisy repeated warnings?
15. What is the acceptable ingestion lag for the row, and does per-request hook
    triggering justify its process overhead?

## Validation Plan

These experiments should be run before committing to an option:

1. **Hook vs log capture.** Register a temporary `PostModelRequest` hook that
   appends `{received_at, payload}` to a temp file. Run one astra turn, one sol
   turn, and one turn with a tool loop. Compare hook events with log
   `token_count` events by count, model, token fields, and ordering.
2. **Log flush ordering.** In the same capture, compare hook `received_at` with
   the log line's write time and the event timestamp.
3. **Fork/resume identity.** Create a fork of a session; verify copied
   `token_count` events retain context and timestamp precision. Test both copied
   events and distinct same-turn requests against the candidate identity.
4. **Image model.** Run one image request; inspect the rollout and hook payload
   for image usage, token usage, and billing basis.
5. **Service tier.** Inspect `turn_context`, `session_meta`, and the hook payload
   for a Fast/priority field; if absent, decide the unpriced policy.
6. **App-server usage.** Call `account/usage/read` for an active thread and an
   archived thread; record whether `estimatedUsageUsdMicros` is present, whether
   token groups match the log, and whether historical threads are available.
7. **Backfill benchmark.** Implement a minimal Rust reader and measure a 90-day
   backfill over the local corpus, including memory and query latency.
8. **Dedup adversarial test.** Construct two independent sessions with identical
   token counts at the same timestamp; verify both are counted.
9. **Window and reset tests.** Verify exact rolling 24h/7d/30d intervals,
   DeepSeek calendar windows across DST, and confirmed, delayed, missing, or
   changed weekly cycle metadata. Cross midnight/reset with no new usage and
   verify cached summaries expire correctly.
10. **Store recovery and concurrency.** Exercise competing sidebar instances,
    restart after each commit boundary, larger Claude updates, and rewritten
    source files with shared fork provenance. Compare every result to a clean
    rebuild; SQLite WAL alone does not establish logical writer ownership.
11. **Historical schedule tests.** Cross a price-version boundary, a peak-hour
    boundary, and a revision changing both rates and peak hours/timezone. Test
    missing intervals, overlaps, cache-write policy, prior revision replay, and
    cost-cache invalidation with unchanged tokens.
12. **Minimal option benchmark.** Compare Option 0a startup, memory, and steady
    state with a durable store before adding persistence and operational commands.
13. **Account and cycle scope.** Verify cycle-start derivation and whether local
    records can be tied to the displayed account. Unknown attribution must remain
    visible and must not be represented as account-wide coverage.

## Sources

Local:

- `src/usage.rs`: current DeepSeek scanner, pricing, dedup, worker.
- `src/state/refresh.rs`: status-stamp trigger.
- `src/app.rs`: 1-second sidebar refresh.
- `src/ui/panes/quota.rs`: spend row rendering and ladder.
- `src/quota/mod.rs`: quota block height and state.
- `docs/decisions/2026-09-18-deepseek-usage-row.md`: existing design and
  rejected alternatives.
- `docs/maintainers/fork-strategy.md`: dependency and seam constraints.
- `~/.codex/sessions/**/*.jsonl`: local Codex rollout samples.
- `~/.codex/hooks.json`: current Codex hook registrations.
- `~/.codex/config.toml`: hook trust/state.

Upstream and generated:

- Codex hook input schema:
  <https://github.com/openai/codex/blob/main/codex-rs/hooks/schema/generated/model-request.command.input.schema.json>
- Codex Stop hook schema:
  <https://github.com/openai/codex/blob/main/codex-rs/hooks/schema/generated/stop.command.input.schema.json>
- Hook dispatch:
  <https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs>
  and
  <https://github.com/openai/codex/blob/main/codex-rs/hooks/src/engine/dispatcher.rs>
- Hook async default:
  <https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json>
- App-server schemas generated locally:
  `codex app-server generate-json-schema --out <dir>`; inspect
  `v2/GetAccountTokenUsageResponse.json`,
  `v2/NullableGetAccountTokenUsageParams.json`, and
  `v2/ThreadTokenUsageUpdatedNotification.json`.
- DeepSeek pricing:
  <https://api-docs.deepseek.com/zh-cn/quick_start/pricing>
- OpenAI API pricing (for the user-supplied Codex price table):
  <https://platform.openai.com/docs/pricing>

## Glossary

- **API-equivalent cost**: token usage multiplied by published API list prices;
  an estimate, not a billed subscription amount.
- **Canonical event**: a normalized usage record independent of the source
  format.
- **Checkpoint**: the byte offset and file identity needed to resume ingestion.
- **Event id**: a stable identifier used for idempotent writes and dedup.
- **Micros per million**: integer micros of the billing currency per 1,000,000
  tokens; the rate unit that keeps money arithmetic exact.
- **Price book**: the versioned, effective-dated set of price rules.
- **Row spec**: the UI mapping from a provider filter to a label and currency.
- **Derived store**: a rebuildable query structure whose source of truth is the
  vendor logs.
