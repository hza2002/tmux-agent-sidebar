# Usage History Storage and Source Authority

Status: open architecture review. No persistence or archival option is selected.
Date: 2026-10-26.
Audience: maintainer and an independent senior architecture reviewer.

The sidebar currently derives local usage from agent session logs and retains
parsed records in memory. Longer comparisons and historical analysis raise a
separate question: should the sidebar index existing logs, preserve observations
after those logs disappear, or archive enough source evidence to replay its
calculations? These choices have different correctness and ownership contracts.

This reference separates requirements, implementation facts, external evidence,
estimates, and unresolved choices. It does not authorize a database dependency,
an archive, a retention-policy change, or edits to agent configuration.

## Review request

Please evaluate the options without assuming that SQLite, indefinite retention,
or a separate evidence archive is necessary. In particular:

1. Define which data is authoritative and what a deletion or correction means.
2. Identify the smallest design that supports the required history lifetime.
3. Challenge replay and deduplication guarantees against the actual parsers.
4. Assess costs at current scale and at ten times that scale.
5. Recommend an independently verifiable rollout and an explicit failure policy.

The existing [metering reference](2026-10-26-usage-metering-reference.md) covers
source selection, normalization, and pricing more broadly. The current contract
is in the [decision record](../decisions/2026-10-26-usage-metering.md) and
[usage guide](../usage-metering.md). This reference reopens storage questions;
it does not supersede those contracts.

## Product needs and unresolved scope

Established needs:

- Local Codex and DeepSeek usage, with total tokens, cache-hit rate, and
  historical API-equivalent amounts. Local records are not an account-wide bill.
- Today and rolling 24h/7d/30d views, plus the current Codex quota cycle.
- Responsive switching without source I/O on the TUI thread.
- Interest in equal-window comparisons and longer-term analysis.
- Accurate handling of copied history, source changes, and missing evidence.

The desired retention contract remains open. "Store everything" could mean
all available normalized records, all future observed usage, selected raw usage
events, or complete versioned transcripts. These are not interchangeable.
The maintainer has requested review before choosing among them.

## Current implementation facts

These facts describe the checkout inspected on 2026-10-26, including uncommitted
Usage changes. HEAD was `cd98164`; concurrent work may change that baseline.
Use symbol names rather than assuming line numbers remain stable.

| Area | Current behavior | Source |
| --- | --- | --- |
| Discovery | Codex `sessions` and `archived_sessions` under `CODEX_HOME` or `~/.codex`; Claude `~/.claude/projects` | `usage::roots` |
| File selection | Recursive `.jsonl` discovery, modification-time prefilter, bounded traversal, symlinks skipped | `jsonl_files_since` |
| Retention | Parsed records older than 31 elapsed days are discarded | `Scanner::scan_roots` |
| Persistence | No usage database, evidence archive, or disk checkpoint | `Scanner`, `UsageState`, `poll_loop` |
| Refresh | Existing worker, activity-triggered requests and 30-second fallback; cached window selection in the UI | `usage::poll_loop`, `app::workers` |
| File identity | Path-indexed cache; device/inode, modification time, size, offset, and prefix fingerprint | `CachedFile`, `Scanner::refresh_file` |
| Append | Resume complete lines with parser context; defer an incomplete tail | `Scanner::refresh_file` |
| Rewrite | Same-size changes, truncation and changed identity/prefix trigger replacement | `Scanner::refresh_file` |
| Detection limit | A middle edit followed by growth can evade the bounded prefix check | `Scanner::refresh_file` |
| Missing file | Discovery prunes unobserved files; an open/stat failure can also remove its cached contribution and flag incompleteness | `scan_roots`, `refresh_file` |
| Dependencies | `serde_json` exists; no SQLite dependency is declared | `Cargo.toml` |

Code: [scanner and worker](../../src/usage.rs),
[source parsers](../../src/usage/source.rs),
[Codex summaries](../../src/usage/metering.rs),
[historical price book](../../src/usage/pricing.rs).

Current missing-file behavior is not a proposed long-term preservation policy.
A durable design must distinguish a completed inventory from a failed read or
an unavailable root. A modification-time filter is useful for window scanning;
it cannot establish complete historical coverage of files restored with old
timestamps or moved outside the configured roots.

### Parsing and identity limits

Codex parsing carries session, turn, model, tier, account, and cumulative-counter
context across lines. Repeated quota updates can repeat the last usage record;
copied sessions can repeat earlier usage. Some records lack sufficient identity
evidence, which the current summaries disclose.

Claude streaming records can repeat a message with growing counters. The current
aggregation selects the largest token record with a deterministic tie-break.
Its identity model and cross-session collision behavior need review before they
become durable uniqueness constraints.

Persisting the existing `Record` type alone does not preserve all raw evidence.
For example, DeepSeek records retain a family classification rather than every
original model identifier. A source line can also contain unrelated message
content. Selecting whole JSONL lines is not automatically a content-free archive.

## External evidence and its limits

Claude Code documents automatic cleanup of session transcripts under its
`cleanupPeriodDays` policy, with a default of 30 days. It also documents
transcript variants set aside as orphaned or superseded files. This establishes
that retaining originals indefinitely is not a safe default assumption.
[Claude directory reference](https://code.claude.com/docs/en/claude-directory#application-data)

The OpenAI configuration reference describes `history.persistence` and
`history.max_bytes` for `history.jsonl`. Those settings do not establish a
retention guarantee for the rollout files consumed by this scanner. No Codex
rollout permanence guarantee was established in this review.
[OpenAI configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)

A persistent materialized view remains a derived read model when its source
data is available for rebuilding. Persistence alone does not make it an archive
or an independent authority.
[Materialized view pattern](https://learn.microsoft.com/en-us/azure/architecture/patterns/materialized-view)

SQLite supports local application files and transactional updates. That makes
it a candidate container, not a choice of data ownership. WAL supports concurrent
readers and a writer but introduces checkpoint and sidecar-file considerations;
backup must follow the selected journaling mode's rules.
[Application files](https://sqlite.org/appfileformat.html),
[WAL](https://sqlite.org/wal.html)

Sources were inspected on 2026-10-26. Agent versions and retention policies can
change; the implementation must not infer successful capture from documentation
alone. No agent settings were changed during this review.

## Terms and authority

| Term | Meaning in this review |
| --- | --- |
| Upstream log | Agent-owned local file containing observed events; evidence of local usage, not the provider's final bill |
| Normalized record | A parser interpretation with usage fields and identity; subject to parser corrections |
| Persistent index/cache | Derived records saved across restarts; rebuildable while the required sources remain available |
| Observation history | Retained interpretations after source disappearance; replay capability is limited by retained fields |
| Evidence archive | Preserved source material and context from which a declared parser version or scope can be replayed |
| Aggregate | A query result derived from records and explicit window/price rules |

An archived copy does not create an independent editable truth if its provenance
and revision rules are explicit. Conversely, a database called a "cache" becomes
irreplaceable history when its sources disappear. Its operational contract must
acknowledge that transition rather than relying on the label.

## Candidate designs

| Option | Retained data | Benefit | Main obligation |
| --- | --- | --- | --- |
| A. Expanded memory cache | Recent parsed records; e.g. 90 days | Smallest change for window comparisons | Requires adequate original-log retention; restart reparses; no survival after source loss |
| B. Persistent derived index | Parsed records, provenance, parser versions and checkpoints | Faster restart and historical queries over available originals | Define invalidation, reconciliation and source-deletion behavior; no independent replay after source loss |
| C. Persistent observation history | Normalized events retained after originals disappear | Compact long-term statistics | Accept bounded reparsing capability; retain uncertainty, revisions and observation provenance |
| D. Selected evidence plus index | Raw metering events and required context, plus derived records | Replay for a declared metering scope without full conversations | Prove context sufficiency; handle unknown events and content embedded in usage lines; version extraction rules |
| E. Full versioned source archive plus index | Complete captured file generations or equivalent immutable snapshots | Broadest future replay potential | More disk, sensitive content, backup/deletion policy, and source-version reconciliation |

Options C and D may be sufficient for longitudinal trends even when they cannot
support arbitrary future reparsing. Option E preserves more evidence but still
cannot recover events that disappeared before the collector observed them.
Increasing the upstream retention setting or using an existing user backup is
another way to support A/B; it changes a user-owned policy and does not protect
against every move, overwrite, failure, or missed collection interval.

JSONL, sharded JSON files and SQLite are implementation choices beneath these
contracts. JSONL offers inspectable append data; revisions, atomic replacement,
multiple writers, indexes and compaction need explicit protocols. SQLite offers
transactions, indexed queries and constraints at the cost of a dependency,
migration and operational responsibilities. A file extension cannot resolve
source authority or deletion semantics.

## Decisions required before implementation

### Source changes, revisions, and absence

Choose whether history means the latest source contents or everything observed
at a point in time. An edited source may be a correction, an intentional rewrite,
or a partial restore; "latest wins" is a policy, not proof of correctness.
Preserving old generations for audit does not mean counting both generations.

Paths should locate evidence, not act as the sole event identity. Moves and
copies across roots must not inflate totals. Missing IDs require an explicit
fallback and uncertainty policy rather than a fabricated stable key.

For source disappearance, distinguish read failure, move, completed deletion,
and intentional user erasure where possible. Decide whether historical
contributions remain. An explicit archive-purge operation may be needed if
deleting a source transcript no longer removes its retained evidence.

### Replay scope and pricing

Specify which future changes must be recoverable: price-book corrections,
normalization fixes, deduplication changes, newly recognized models, new source
event types, or arbitrary transcript analysis. Retaining sufficient fields for
one class does not establish support for the others.

Keep original counters and effective model/tier evidence separate from computed
costs. Calculations should identify parser/normalization versions and price-book
revisions. A change of interpretation must not look like new usage.

### Coverage and collection lifetime

The existing collector runs while the sidebar worker runs. It is not an always-on
archiver. Determine whether that availability is sufficient. A new daemon or
hook path would be a separate architectural decision and setup cost.

Record coverage and errors independently of zero usage. A successful import of
available files does not prove historical completeness. A partial previous
window must not yield a seemingly exact comparison percentage.

### Consistency and failure

Checkpoint advancement and contribution replacement must survive interruption
without double counting or skipping records. Publish a complete consistent
snapshot, not half of a source replacement. Decide how multiple sidebar processes
share the store, and how lock contention, corruption, disk-full errors and schema
upgrades affect the existing in-memory path.

A reset of a rebuildable index and deletion of an irreplaceable archive must be
different operations. Rebuilding must not silently discard the only surviving
historical evidence.

## Scale and cost evidence

A metadata-only local inventory on 2026-10-26 found 213 Codex JSONL files totaling
500.2 MiB and 387 Claude JSONL files totaling 1,000.1 MiB. These are original
transcript sizes, not normalized-event counts or evidence-archive sizes.

The existing decision record reports an earlier debug-build sample of 194 Codex
files and 13,709 retained records: 5.15 seconds for an initial scan and roughly
50 milliseconds for a cached scan plus queries. This is a dated baseline, not a
measurement of persistent storage, all sources, or 90-day retention.

For budgeting only, assuming 0.3–1 KB per durable normalized event gives:

| Event count | Estimated record/index budget |
| --- | --- |
| 100,000 | 30–100 MB |
| 1,000,000 | 0.3–1 GB |

Actual schema, identity strings, source mappings, indexes, revisions, journals,
compression and raw evidence can change these figures substantially. An
illustrative compact JSON payload was about 247 bytes; it did not include all
those costs. No measured RAM budget or archive compression ratio is available.

Before selecting a store, measure initial import, warm startup, append/rewrite
cost, current/previous-window queries, peak RSS, database/archive growth, and
recovery at current scale and 10x scale. Long-term disk history need not imply
loading all historical records into RAM or rewriting the whole store per poll.

## Validation matrix

Any selected option should test the guarantees it actually promises:

- Append, incomplete tail, duplicate import, copied/forked session, and replay.
- Truncation, same-size rewrite, middle rewrite plus growth, changed inode, and
  conflicting revisions of the same logical event.
- Move within/between roots, restore with old modification time, unreadable root,
  missing file, and source cleanup while the collector is stopped.
- Crash between data write and checkpoint, interrupted replacement, concurrent
  readers/writers, disk full, schema upgrade, and malformed retained evidence.
- Parser/price revision, unknown model or tier, cumulative-counter reset,
  streaming revisions, and identity collisions or missing identity.
- Today across timezone/DST boundaries, exact rolling bounds, incomplete prior
  windows, zero baselines, and quota-cycle metadata becoming stale or changing.
- Index loss/rebuild and archive loss/restore, with different documented outcomes.

Use synthetic fixtures and isolated tmux tests. Read-only local-log benchmarks
require explicit paths and must emit aggregate metrics rather than transcripts.

## Repository integration and decision handoff

Preserve the existing worker -> usage state -> leaf renderer flow. No persistence
choice should introduce source I/O on the render/input thread or a replacement
for upstream lifecycle/state ownership. The repository prefers existing seams
and small dependencies; justify any new store against the chosen requirements.

Before implementing a selected option, record: authority and deletion policy,
retention, replay scope, identities and revision semantics, storage format and
location, migration/recovery, backup expectations, source coverage, benchmark
acceptance bounds, and dependency choice. The previously suggested
`$XDG_DATA_HOME/tmux-agent-sidebar/usage.sqlite3` location (fallback
`~/.local/share/tmux-agent-sidebar/usage.sqlite3`) remains a proposal; no file has
been created there by this work.

Write a decision record only after review. Link it from the usage guide and this
reference, then implement and validate an independently usable change. UI-only
work can proceed under the separate [interaction readiness plan](2026-10-26-usage-interaction-readiness.md)
without selecting a durable storage contract.
