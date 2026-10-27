# Decision: Local usage windows and historical price books

## Context

Quota remaining and local token consumption answer different questions. The
confirmed product separates Quota and Usage metrics under one shared footer
header to save a row, with Codex cycle and
rolling 24h/7d/30d rows and the existing DeepSeek cache/price-period fields.

The persistence/retention portion below records the original implementation.
It is superseded by the [durable history decision](2026-10-26-durable-usage-history.md).
Window, pricing and presentation contracts remain in force.

## Chosen Seam

Extend the existing usage worker, its mailbox, and the quota leaf renderer.
Use a rebuildable in-memory per-file cache (reference Option 0a), with append
checkpoints and parser context. Persist no prompts, credentials, or derived
ledger. Discover all source directories so old sessions that keep appending
remain visible. Rewrites replace a file's cached contribution; global dedup is
performed at query time. No hook installation or additional worker is needed.

Price books are checked-in, effective-dated versions. Each version owns its
peak schedule, timezone offset, and service-tier factors. Money uses integer sub-micro intermediates
and rounds after aggregation. Unknown historical prices, unsupported billing,
and unknown tiers are explicit. Initial OpenAI rates are verified against
https://developers.openai.com/api/docs/models/gpt-6-astra and
https://developers.openai.com/api/docs/models/gpt-6.1-sol on 2026-10-26.
The Sol release price is documented on 2026-09-29 in the official changelog;
Revision `2026-10-26.2` uses the official Astra rates as an initial baseline
from its 2026-09-03 release. Before verification, unchanged pricing is an
explicit estimation assumption disclosed in details. Verification time is
separate from the effective date; future changes still append dated versions.

Historical `thread_settings_applied` events provide the selected service tier.
A `turn_context` that omits tier preserves that setting. Missing tier uses a
disclosed Standard estimation baseline; explicit unsupported tiers remain
unpriced. Model and event time drive pricing regardless of provider/account.
Selected mode does not establish the actual server-served tier.

The weekly quota's reset timestamp defines the cycle end; its recognized
604800-second duration defines the start. Expired or unavailable metadata must
not produce a new cycle. Local logs do not establish account-wide coverage:
show local scope and account/tier uncertainty in details. Quota percentages
are never converted to dollars.

## Alternatives Rejected

A durable SQL or JSONL ledger adds migration, locking, replacement, and recovery
protocols without a product requirement for persistence independent of source
logs. The in-memory cache can be rebuilt and benchmarked before reconsidering.
A read-only debug-build benchmark over 194 local Codex files (13,709 retained
records) took 5.15 seconds on first scan and 50 milliseconds for the cached scan
and four queries. This supports keeping startup work in the existing worker
without a database. Per-request hooks add setup and process overhead; existing status changes and
30-second polling meet the required turn-boundary freshness.

## Upstream Compatibility

Keep worker -> local state -> leaf renderer flow. Quota fetching remains owned
by the existing quota module. Layout records separate usage click targets;
height collapse preserves agent-list priority. No change to hook registration,
CLI command entry points, or tmux queries.

## Conflict and Removal Strategy

Preserve upstream worker, state, and layout ownership during merges. Reapply
usage mailbox fields and leaf renderer calls. Removing the feature deletes the
usage modules and their existing worker/state integrations; there is no stored
data migration. Runtime visibility continues to share the quota switch.

## Shared window interaction (2026-10-26)

Both providers default to explicitly labeled local-calendar today and support
today, elapsed 24h, 7d, and 30d with matching row-label suffixes. This replaces DeepSeek's
calendar-day 7/30-day windows and Codex's default 7d selection. The independent
Codex quota-cycle row retains its details action. Clicking a provider row
switches only that provider. The Usage header derives a common suffix when
selections agree and omits it when they differ. Clicking the header (or plain
`u`, including compact mode) advances a common selection or aligns a mixed
selection to today. There is no separate linking mode or remembered global
window. Input handling and rendered header hit targets stay in the existing
input -> state -> layout seams.

The existing worker scans once and publishes every provider window at the same
timestamp. The UI selects cached totals instead of clearing DeepSeek's row and
sending a window mailbox request. Activity and periodic refresh ownership stay
in the existing worker; no new hooks, threads, persistence, or configuration are
needed. Removal restores per-provider selection without migrating stored data.

## Current price signal and spare columns

The current DeepSeek peak/off-peak signal belongs to the Usage header rather
than a historical usage window. Right-align `ds高峰` / `ds低谷` using the existing
peak/off-peak theme colors; compact views keep the signal on the DeepSeek row,
prioritizing it over cache/token detail when width is tight. Move only the
presentation, not historical pricing or refresh ownership.

Live quota reset times use active text brightness while stale/error metadata
stays subdued. Usage rows add daily averages and output/input counts only when
complete fields fit. Preserve row count, quota-provider coverage, alignment of
core fields, and the existing width-aware renderer seam.

## Field interaction follow-up

The approved UI follow-up replaces row-wide Usage clicks with typed rectangular
targets produced from the final rendered spans. Only the left header/window
labels switch windows; monetary fields toggle total/daily-average display for
that row. Token/cache fields, padding and the DS price signal have no action.
The footer consumes those clicks so they cannot refresh quotas or select a
pane behind it. Subscription rows retain their existing refresh behavior.

Three independent ephemeral amount preferences belong to `UsageState`; no
worker, source, retention, or pricing contract changes. Total tokens remain
primary. Remove the separate average and input/output suffixes. Current DS
status uses fire/leaf Nerd Font icons, shortened before core fields under width
pressure. Frame targets stay owned by the existing renderer/layout seam and
are replaced on each frame. No parallel input pipeline or durable store is
introduced. Removal only restores UI formatting and hit targets; it requires
no data migration. This section supersedes the row-wide/spare-suffix behavior
described above.


## Responsive Usage fields (2026-10-27)

Keep the cycle row as cumulative totals with a label-only details action.
Recent rows support independently clickable amount/token daily rates, using
observed elapsed time and a compact `均` suffix. A shared leaf-renderer pass
measures visible fields, aligns numeric columns, and distributes spare columns
as elastic whitespace with cache ratios at the right edge. Compact DS period
metadata precedes cache. Shrinking removes whitespace before shortening labels
and hiding cache; final spans determine click targets. This replaces ad hoc
per-row padding policies without a general table framework or new settings.

State still owns ephemeral display preferences, the worker owns statistics,
and the renderer owns only presentation. No pricing, storage schema, collection
or quota-query boundary changes. Upstream merges retain those existing seams;
removal only affects ephemeral layout/preferences and requires no data migration.
