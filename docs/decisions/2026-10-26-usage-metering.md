# Decision: Local usage windows and historical price books

## Context

Quota remaining and local token consumption answer different questions. The
confirmed product separates Quota and Usage metrics under one shared footer
header to save a row, with Codex cycle and
rolling 24h/7d/30d rows and the existing DeepSeek cache/price-period fields.

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
