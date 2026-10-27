# Durable Usage History: Implementation Plan

Status: implementation authorized and completed; final verification evidence is recorded below.
Date: 2026-10-26. Baseline: `6ea0dfa`.

## Intent and accepted product contract

Keep personal Codex and DeepSeek metering history independently of transcript retention. Preserve the existing worker -> usage state -> leaf UI flow. A future local web view will consume the same query/calculation layer; it is not part of this implementation.

- Import every available supported JSONL source under the existing configured roots, without an age cutoff. Prioritize recent files, then finish the inventory in the background.
- Retain compact event-level history indefinitely. Transcript disappearance is not a usage deletion. No automatic detail-to-rollup pruning.
- Correctness takes priority over immediate updates. Continue the existing worker and 30-second fallback; no always-on collector, new hooks, or agent configuration changes. Preserve the existing `@sidebar_quota` enable switch: explicitly turning it off stops collection; layout-driven hiding of rows does not. The notices UI remains active for an enabled worker even when the footer cannot fit.
- A stopped collector cannot recover logs deleted before the next scan. Import completion means the discovered inventory was processed, not proof of account-wide or historical completeness.
- Preserve only an explicit metering field allowlist. Never archive full JSONL lines, prompts, responses, tools, code, or credentials.
- Store original directory context; do not introduce stable project identity, rename matching, remote matching, or repository marker files.
- Use historical price books, including time-dependent schedules. Money and aggregate views are derived, not the retained source of truth.
- Errors must remain visible and diagnosable. Destructive migrations and bulk reinterpretations require a verified backup first. Cloud synchronization and routine external backup management are out of scope.

## Architecture and storage choice

Choose SQLite through a single pinned `rusqlite` dependency with bundled SQLite and backup support. No ORM, service, event bus, or full Event Sourcing framework. Resolve and audit the bundled SQLite version before locking the dependency; require SQLite 3.51.3 or later (the WAL-reset fix); assert the actual bundled runtime version in tests. The library version must be recorded in diagnostic output.

Store at `$XDG_DATA_HOME/tmux-agent-sidebar/usage.sqlite3`, falling back to `~/.local/share/tmux-agent-sidebar/usage.sqlite3`. Honor only absolute nonempty XDG paths. Create the private directory with mode 0700 and database/backup/log files with mode 0600. Do not change permissions outside the owned directory. Do not place the live database in cache, the checkout, an agent log root, or a cloud-synchronized location automatically.

Use WAL, `synchronous=FULL`, foreign keys, and a bounded busy timeout (2 seconds). Read back important PRAGMA results. SQLite supplies atomicity; a process-held advisory lock next to the database serializes collectors and maintenance operations across tmux servers. A non-owner reads committed snapshots and periodically retries the lock without advancing any cursor. A dead owner releases the OS lock. This is coordination inside existing workers, not a daemon. Destructive maintenance requires collectors stopped and fails clearly if the lock is busy; no force takeover or lock-handoff protocol. The owner covers its configured roots. Followers compare their configured root set with persisted owner coverage; differing roots produce a visible unsupported-multiple-root warning, rather than silently claiming their logs were imported. Automatic cross-process root registration is out of scope.

Validate database identity and schema version before write PRAGMAs or queries; only the elected owner creates or migrates a store and rechecks versions after acquiring the lock. Followers never create a replacement database. Missing stores report initializing/unavailable, not complete zero. Use short write transactions and no log parsing while holding a SQLite write transaction. Parse a bounded batch, acquire transaction, verify its expected cursor/generation, write evidence and effective records, and advance parser context/cursor together. Cancellation or failure rolls the batch back. Snapshot queries use one read transaction and one timestamp for every displayed window.

## Retained fields and small relational model

Source parsing must emit a versioned, application-owned metering structure, not persist today's `Record` or a competitor's internal structure directly.

| Group | Retain | Semantics |
| --- | --- | --- |
| Provenance | source kind, source file association, generation, byte offset, observed time, extractor version | Locations identify observations, not logical usage |
| Time | original event timestamp and parsed UTC precision | Preserve subsecond identity evidence; query bounds remain explicit |
| Identity | available session, turn, message, request/response IDs; event kind | Nullable; never fabricate a globally reliable identity |
| Replay context | Claude sidechain flag and observed session aliases; Codex parent session, fork boundary time, compaction response association, thread token counters and derived reset epoch | Selected metering context only; retain facts required by the adapter decision table |
| Model | raw model, normalized model, explicit selected tier and its evidence origin | Keep missing separate from explicit standard/unsupported |
| Counters | source-reported input, cached input, cache creation (including duration split if supplied), output, reasoning, total; cumulative counters where supplied | Nullable unsigned values; checked storage conversion; preserve source input semantics |
| Context | original cwd and available non-secret account identifier | No project matching, no authentication material |
| Interpretation | normalized token components, identity confidence, normalization version, quality flags | Explain assumptions; do not double-count reasoning included in output |

Use four logical tables, with indexes only for demonstrated query/identity paths:

1. `source_files`: root/source, path, file identity, generation, size/mtime, consumed offset, serialized allowlisted parser context, boundary fingerprints, extraction version and scan result.
2. `observations`: immutable selected metering fields, deduplicated by a canonical encoding of EVERY retained allowlisted source field (including raw model, tier/evidence origin, cwd, account, event kind, timestamp/IDs, replay context and counters), preserving missing versus explicit null versus zero, with exact content comparison (a hash is only a lookup accelerator). Keep an event foreign key as mutable association; a schema does not need a separate association table. Preserve the first source location and compact session alias context, not every duplicate occurrence. Logical usage identity and file-origin identity are separate: only weak/anonymous observations include a stable source-file association in evidence equality; strong copies can share evidence with compact session aliases. Different anonymous files are not merged merely because counters match. Re-reading the same file generation/offset with identical evidence adds nothing. No full source JSON blobs.
3. `events`: current effective normalized usage, logical identity/confidence, selected observation, selection reason/version, timestamp/model/directory fields and quality flags. The persisted selection is the decision record for unresolved conflicts; prior materially distinct observations remain available. Index event time and identity; add directory/model indexes only after measuring real queries.
4. `issues`: deduplicated unresolved/recovered diagnostic records with source location, stable category, first/last seen, occurrence count and last successful scan.

Schema version uses `user_version`; extractor/normalizer versions are separate. A small metadata table is allowed only for persisted inventory progress and database identity. Avoid persisting cost totals or daily aggregates in v1.

## Identity, stability, and correction rules

Do not wait for an entire session to end, or pretend a quiet file proves finality. Read complete lines. Apply source-specific known usage semantics before persistence; later supplements update the effective event, not a new charge. Persist distinct metering evidence only when necessary to reconcile usage.

- Claude/DeepSeek: follow the explicit identity table below. Consolidate known streaming variants using the complete larger usage record, not independent maxima per component. Preserve parent/sidechain provenance so replay copies do not become new calls. Fallback identities remain weak, never asserted collision-free.
- Codex: reconcile `token_usage_record`, `token_count` and compaction evidence before deciding which observation is countable; a response ID alone does not authorize another charge. Repeated unchanged cumulative counters are not new usage. Retain last and cumulative usage; use supported cumulative differences only when single-event counters are unavailable. Fork replay is linked using supplied parent and boundary evidence, not filename equality. See the decision table below.
- Source copies/moves must not inflate strong-identity events. For weak identity, conservatively retain distinct observations and disclose possible duplication. V1 does not compare whole transcript contents to merge anonymous copies. Never merge unrelated calls merely because token counts match.
- An unexplained conflict retains the existing effective record and adds a visible issue; those totals are provisional. Persist the selected observation and decision, so replay of the database reproduces that choice. Fresh import and incremental import must agree for non-conflicting fixtures. A fresh rebuild from raw logs alone cannot promise to reproduce a prior conflict decision; do not claim order independence in that case.
- A supported normalization correction may raise or lower totals. In v1 it runs offline, after backup, in one SQLite maintenance transaction with bounded streaming buffers. Old interpretation remains effective until commit; interruption rolls back. Budget temporary WAL/disk growth and stop safely on disk-full. Do not implement shadow generations or partially committed reinterpretation batches.
- Source deletions or omissions in a rewritten file do not retract historical events. A reliable same-event revised observation may replace interpretation; arbitrary transcript edits are not user commands to erase usage.

The field allowlist supports re-pricing and normalization/identity changes using retained fields only. It does not guarantee extraction of new event types or previously omitted fields after source deletion. Unknown model usage with a recognized usage structure is retained as unpriced. Unknown unrelated JSON types are ignored; malformed recognized metering and invalid required fields create issues without persisting source text.


## Adapter identity and countability decision table

Identity is a matching policy, not a single opaque hash of model and token values. Model, tier, price and normalized counters are mutable interpretation fields, never unconditional strong-identity components. Exact canonical evidence is retained for decisions that cannot use a strong ID.

| Source evidence | Match / action | Confidence and collision rule |
| --- | --- | --- |
| Claude message ID + request ID | source + message ID + request ID; corroborate session aliases | Strong only for a consistent source namespace; conflicting reuse is an issue, not a merge of counters |
| Claude message ID without request ID | source + session + message ID | Conditional; source-local missing session falls back to a file association and warning |
| Claude parent and sidechain copies with changed request IDs | same message ID plus explicit sidechain/session-alias relationship; prefer original parent evidence | Without that relationship do not merge across sessions; retain weak identity warning |
| Claude no usable message ID | source file association + event timestamp + original complete-line offset, plus exact evidence | Weak; stable for rescans, not guaranteed copy-dedup; unknown directory/model never becomes identity |
| Codex ordinary token_count | session + turn + reset epoch + cumulative counter vector, or raw timestamp + source-local ordinal if cumulative counters absent | Conditional. Classify reset using the explicit predicate below; increment the persisted epoch only for a recognized reset. Missing identity stays weak. A model-only correction does not add an event |
| Codex repeated cumulative vector within an epoch | no new usage; link relevant extra context | A counter change is evidence, not globally unique request identity; conflicting reuse becomes an issue |
| Codex copied or forked prefix | explicit parent session + fork boundary + matching ordered metering evidence maps to parent events | If parent evidence is unavailable, retain unresolved provenance and an incomplete/possible-duplication flag; no unsupported guarantee of exact fork totals |

Counter reset example: same turn observes cumulative 100, 200, 50, 100; the second 100 belongs to reset epoch 1 and must not collapse into epoch 0. Preserve the original last-usage record when present rather than assuming the difference is always the correct billed amount. Counter classification is explicit: (1) identical comparable vectors are repeats; (2) all comparable counters nondecreasing are advances; (3) a reset requires cumulative total_tokens to be present in both records and strictly decrease, with every comparable primary input/output counter nonincreasing; increment the inferred reset epoch and mark its provenance as inferred; (4) every other decrease, including mixed increase/decrease or a decrease without comparable total_tokens, is reset-ambiguous. A reset-ambiguous record cannot use cumulative-delta counting; valid last-usage evidence may contribute with a weak identity and visible quality flag. If last usage is absent, retain unresolved evidence without inventing a delta. Never silently saturate negative deltas to zero. Cache/reasoning counters must also validate against their parent counters; contradictory vectors remain issues, not confident resets.

Codex response record reconciliation (following the inspected ccusage adapter, with our persistent evidence boundary):

| Available observations | Effective counting |
| --- | --- |
| token_count only | count supported last usage, or validated cumulative delta; unchanged cumulative notifications contribute zero |
| token_usage_record only, no compaction association | retain pending evidence, do not count it as an additional request; expose unresolved metering coverage if still unmatched after a completed scan |
| response record + matching ordinary token_count | one usage contribution; match response context with raw usage or thread cumulative evidence, never any unrelated equal token tuple |
| local compaction response + advancing matching token_count + marker | count once through token_count; marker does not add another event |
| remote compaction response + matching marker, no token_count advance | count response usage once; marker may precede response record |
| incompatible observations for one response | hold prior accepted contribution and create a conflict issue |

The first session header owns the rollout identity and fork boundary; embedded ancestor headers cannot overwrite them. Explicit response `thread_id` (or `session_id`) owns response identity. Retain response `turn_id`, `root_turn_id`, turn counters and thread counters as selected evidence. Adjacent ordinary response/count records can report different cumulative scopes: same-turn, immediately preceding response usage may match exact per-call counters even when thread cumulative counters differ. Consume response context at the first count so a later equal tuple cannot acquire a stale association. Compaction markers are not usage and do not independently create possible-duplication warnings.

Reconcile over all retained matching evidence, not a one-way emitted flag. Persist pending association and matched response identity so restart or a later marker cannot add an already counted response. Explicitly test response+marker counted first, then restart, then matching advancing token_count: update the representation of ONE existing event atomically. The reverse order (token_count, marker, response) must also converge to one event. These cases require synthetic append/restart and source-order fixtures before choosing SQL uniqueness constraints. Unknown new source schemas remain explicit coverage gaps; this is not a promise to understand future formats.

## Import, checkpoints, and bounded resource use

Inventory all supported roots without mtime filtering; retain symlink exclusion and traversal bounds, but report bounds/read failures as incomplete inventory rather than successful completion. Record progress per file; restart resumes committed batches. A failed/unavailable root never removes durable history.

Load persisted recent query results before starting backfill. On empty stores publish partial results with an importing state, never a false complete zero. Stream file parsing in bounded batches (up to 1,000 observations or 4 MiB selected evidence). Serialize parser context and partial-line offset correctly; pending response/compaction evidence is stored in observations rather than an unbounded in-memory context. An incomplete trailing line is retried without an error. Bound raw lines at 8 MiB using a bounded reader which discards the remainder to newline without buffering it. Oversized lines create a persistent source-incomplete issue with offset, even when the scanner cannot prove they contain usage. Advancing past such a line never clears the incomplete-coverage warning, even when file enumeration finishes; a parser-version change reparses extant affected files. The local bounded line-size probe found 614 files, 368,810 complete lines, a maximum line of 6,161,647 bytes and zero lines above 8 MiB. This supports the initial cap for current sources, not arbitrary future logs. If legitimate metering lines exceed it, use streaming allowlisted extraction instead of raising the cap repeatedly. Interleave backfill with recent-file refresh so active files do not starve historical work or vice versa.

Detect changed identity, truncation and same-size rewrites and reparse into a new source generation. For appends validate the existing prefix and the consumed-boundary fingerprint. These checks do not prove arbitrary middle-edit detection. Do not add a full-content startup audit or digest tree in v1. Detected identity/truncation/same-size changes and extractor-version changes trigger full re-read of affected extant files. A middle edit followed by growth that preserves the validated boundaries can escape detection; this limitation is explicit, not a claim of verified file integrity. Selected-evidence dedup prevents a new file generation from multiplying unchanged evidence. Changes while reading are retried; never publish mixed-generation results as a completed replacement.

Queries stream only the requested interval through the existing historical pricing functions. Recent memory does not grow with lifetime history; do not deserialize all retained events at startup. Preserve today/timezone, rolling elapsed bounds and externally supplied quota-cycle semantics.

## Failure visibility and recovery

Extend the existing usage update with structured import/storage/source health. Keep the current header notices indicator and popup as the persistent error entry point, including when usage rows are layout-hidden (not when collection is explicitly disabled); use row quality indication for affected totals. No extra permanent footer row. Details include category, affected source, first/last occurrence, last successful persistence, diagnostic path, and a concrete next action. Recoveries clear the active indicator but preserve the diagnostic record.

Write sanitized JSONL diagnostic records under `$XDG_STATE_HOME/tmux-agent-sidebar/usage-history.log` (fallback `~/.local/state/...`), capped at 5 MiB with one rotated file. Deduplicate repeated failures, recording transitions and counts. Serialize diagnostic append/rotation across collector and CLI processes with a short advisory file lock; no additional logging service. If database writes fail, diagnostics must still work independently; if file logging also fails, expose both failures in the mailbox and stderr. Never emit raw JSONL contents, SQL parameters containing evidence, or credentials. File path/offset is local diagnostic context.

On transient storage failure, keep last committed query results. If the store is unavailable, a recent-log scan using the SAME new extraction, consolidation and pricing functions may supply a clearly marked non-durable fallback; never add fallback totals to historical totals. Preserve committed results if available; only use fallback when the store cannot be read. Its coverage is source-limited, not guaranteed equal to durable history. Do not retain a parallel old-parser statistics engine. Retry on existing cadence, with no tight loops. Do not clear or create a replacement database on corruption, unknown schema, or permission failure.

Add a focused CLI leaf `usage-history` with `status`, `backup <new-path>`, `export <new-path>` and `purge --all --confirm`. Status reports versions, import and error state without dumping evidence, and distinguishes disabled/not-running collection from failure. `status`, compatible-schema `backup` and `export` may run online without the collector lock. A live-process ownership record reports last owner roots/heartbeat; a released lock means no active collector, not proof of which tmux option caused it. Backup uses SQLite's backup API and refuses to overwrite a destination. Export streams versioned allowlisted evidence/interpretation JSONL from one consistent read snapshot; export is not advertised as a restore format in v1. Both online export and backup have a 60-second operation deadline: abort with an actionable error, close the read snapshot and remove only their own incomplete output on expiry. Test a deliberately slow destination and concurrent WAL growth; a failed output must never be reported as a completed backup. Purge requires all collectors stopped, the exclusive maintenance lock, an automatic verified backup and explicit confirmation; it clears history and cursors, and warns that retained source logs will be imported again. No source transcript is modified. Explain offline restoration: stop every database handle (collectors and export/status/backup readers), preserve the original database and its WAL/SHM sidecars together in a quarantine directory, install a verified backup at the original path without unrelated sidecars, then reopen and verify. Never delete a WAL as a generic repair. Do not provide a live in-place restore command in v1.

Before schema migration or bulk reinterpretation: stop other collectors, acquire exclusive maintenance/collector lock; make a SQLite-API backup to an exclusive new file in the owned backups directory; validate integrity and schema; sync backup and parent directory; only then mutate. Backup failure stops the operation. Migration failure rolls back and preserves both files. Keep completed pre-change backups; do not implement cloud sync or timed backups. A newer schema is refused without mutation. A downgraded executable must not overwrite history.

## Implementation scope and sequence

This spans more than eight files. Expected seams: `Cargo.toml`/lock; `src/usage.rs`; `src/usage/source.rs`; new `usage/history.rs`, `usage/history/store.rs`, `usage/history/identity.rs`; `usage/metering.rs`; `src/cli/mod.rs` and a `cli/usage_history.rs` leaf; `src/app/workers.rs` enablement tests and mailbox consumption; existing notices state/popup and usage leaf renderer; tests and maintained usage/state/architecture documentation. No changes to hook routing, tmux lifecycle or agent configuration.

1. After review, record the ADR and field/identity contracts. Implement typed extraction and identity fixtures first; maintain existing in-memory behavior until its regression tests pass.
2. Add store, transaction and backup/migration machinery with synthetic fixtures. Test evidence replay and price-only recomputation without source files.
3. Integrate background all-history import, health reporting and recent SQL queries. Persistence and its visible failure behavior ship together; no silent-store intermediate release.
4. Add maintenance CLI, documentation, isolated verification and performance evidence. The first deliverable is independently useful durable history; the website is a separate later feature.

## Acceptance and evidence

Test append/partial tail, repetition, streaming growth, copied/moved/forked files, weak/colliding IDs, counter resets, missing model/tier, unknown models, rewrite/truncation, unavailable roots, deleted sources, old-mtime restores, interruption and resume. Verify fresh rebuild and incremental scan agree for non-conflicting strong-identity fixtures, independent of file enumeration order. Malformed required metering creates a visible issue, while optional missing fields and unrelated events do not flood warnings.

Inject failures before/after evidence, event and cursor writes; reopen after process termination and verify no gap/double count. Test competing collectors, busy timeout, disk-full simulation, corruption, newer schema refusal, backup failure, migration rollback, purge backup and database absent at startup. Test bounded buffers with a large file/line and import cancellation. Use private temporary data/state roots; no production database or live tmux access in tests.

Verify source-independent re-pricing (including DS schedule and effective-date boundaries), stored-counter normalization correction, subsecond identity, timezone/DST/calendar versus rolling windows, and stale quota cycle metadata. Validate notice rendering with inline frame snapshots and an isolated UI showcase, including narrow/hidden-footer states.

Performance gates on the maintainer's machine, release build: first committed recent-window snapshot, labeled with last successful persistence and current import health, within 2 seconds on a warm store; recent four-window query p95 below 250 ms at current scale and 1 second at 10x; worker peak additional RSS below 128 MiB; no history-size-linear idle reparse; scan all current source bytes within 30 seconds and 10x synthetic equivalents within 5 minutes. Report median/p95, hardware/runtime versions, input sizes, event counts, DB/WAL/evidence/index bytes, and peak RSS. These are the original acceptance bounds; implementation measurements are recorded below. If missed, identify the actual bottleneck before adding rollups, caches or services.

Run `cargo fmt`, `./scripts/verify.sh full`, `./scripts/fork-delta.sh`; production behavior changes require the maintained docs/ADR. The design stage made no production changes. Implementation was separately authorized; no commit or push is included.

## Research evidence and limits

- Current sample: 611 source files, 1.53 GiB scanned; 104,561 pre-dedup candidate records; 35 MiB selected JSON; 53 MiB prototype SQLite with four indexes. This was a temporary Python size experiment, not the final schema, parser correctness or Rust performance benchmark. Evidence/revision and backup overhead are excluded.
- ccusage inspected at `e351d846191a0176520e8b3d8151dd80a60bc7e4`: Claude identity/consolidation in `rust/adapters/claude/src/daily.rs`; Codex counters in `rust/adapters/codex/src/parser.rs`, fork history in `replay.rs`. Borrow semantic tests, not an external runtime dependency. License attribution is required for copied code.
- cc-switch inspected at `42ef63471f0ed879913f8de0d0c7e3e580293193`: durable identity ledger and transactional imports are useful; 30-day detail pruning and non-effective-dated model pricing do not meet this product's contract.
- [SQLite WAL](https://sqlite.org/wal.html), [backup API](https://sqlite.org/backup.html), [synchronous modes](https://sqlite.org/pragma.html#pragma_synchronous), [rusqlite backup](https://docs.rs/rusqlite/latest/rusqlite/backup/index.html). Documentation checked on 2026-10-26; dependency safety/version selection must be pinned and tested during implementation.

## Independent review ledger

Round 1 complete: three independent reviewers found identity/replay omissions, conflict determinism contradiction, missing enable-switch behavior, startup-audit overreach, fallback semantic drift, unbounded raw lines, lock/maintenance ambiguity, and incomplete WAL restore instructions. All were accepted and addressed in this revision. Separate event-observation mapping and startup full audits were removed. Round 2: architecture and storage reviewers reported no remaining blockers; identity review required complete-field evidence equality and an explicit counter-reset predicate, now added. Delayed compaction is tested in both arrival orders across restart. Round 3: identity reviewer confirmed no remaining design blockers. All reviewers were independent on the first draft; each rechecked its own findings after revision. The primary agent reconciled all returned findings against the draft and source. No production files were changed.


| Independent reviewer | Initial findings | Final disposition |
| --- | --- | --- |
| Metering and identity | Response/count double-counting; missing replay context; conflicting rebuild promises; incomplete identity rules; incomplete evidence equality; reset ambiguity | Addressed in field contract, identity/countability table and conflict/reconciliation rules; no remaining design blocker |
| Storage and recovery | Lifetime lock versus maintenance; partial reinterpretation publication; unsafe vague WAL restoration; unbounded raw input | Offline maintenance, one transaction, explicit quarantine restore and bounded input; no remaining design blocker |
| Architecture and simplicity | Quota-switch lifecycle; unsupported startup audit; divergent fallback engine; raw input bounds; conflict determinism | Existing enable switch preserved, startup audit removed, shared semantics, raw cap, scoped equivalence; no remaining architecture blocker |

Review is static design validation, not proof of a working implementation. Transaction/crash fixtures, source-identity fixtures, UI visibility checks, bundled-version verification and the current/10x performance gates remain mandatory during implementation. No web server, cloud backup, project identity system or complete transcript archive was added to scope.


## Implementation and verification (2026-10-26)

The existing usage worker now imports through `usage/history.rs`. Its evidence,
store and diagnostics leaves own parsing/reconciliation, SQLite and independent
failure logging respectively. The legacy scanner remains test-only for historical
fixtures; there is one production collection path. UI consumers receive snapshots
and health through the existing mailbox. The maintenance CLI is a thin dispatch
leaf. No daemon, hook, website or project identity service was added.

Implementation review found and fixed replay ownership being overwritten by
embedded ancestor headers, false response/count mismatches between different
cumulative counter scopes, same-location rewrites being counted as new usage,
transient read errors not clearing, warm snapshots temporarily clearing notices,
and a forced-refresh busy loop. Regression fixtures cover these cases. Real
source verification is read-only, writes only temporary stores, and prints only
aggregate metering diagnostics. It never restarts the live sidebar.

Recovery coverage includes abrupt child-process termination inside an unfinished
transaction, SQLite `FULL`, stale checkpoints, competing collectors, corrupt and
unsupported stores, backup failure preventing purge, and cleanup on expired
maintenance deadlines. Collector fixtures exercise incomplete tails, same-size
rewrites, truncation, inode replacement, deleted sources, genuinely old mtimes,
oversized lines and persistent health. UI verification uses inline snapshots,
style assertions and 14 isolated captures, including narrow and hidden-footer
errors. Existing HTML capture limitations (RGB/CJK widths) were checked against
raw terminal output; the capture pipeline was not changed.

The first schema intentionally has no speculative migration framework. Future
schema/normalizer changes must implement their concrete verified-backup and
transactional migration before accepting older stores. Unsupported versions are
refused today. Deadline fixtures verify expiry and partial-output cleanup; they
do not claim an operating-system-level bound for a blocked filesystem call or
stress coverage of arbitrary slow destinations and concurrent WAL growth.

Performance was measured in release mode on Apple M1 Max, 32 GiB RAM, macOS
26.7.1, Rust 1.98.0 and bundled SQLite 3.54.1. The real corpus and synthetic
benchmark use temporary databases; no original transcript is modified.

- Current corpus: 617 files, 1,653,905,809 source bytes, 52,370 effective events
  and 122,104 observations. Import took 14.83 seconds; unchanged-source scan
  took 12 ms. Twenty recent-window queries measured median 80.65 ms, nearest-rank
  p95 82.55 ms and maximum 83.27 ms. The first query took 82.14 ms; the health
  read took 24.38 ms. Peak additional RSS was 37.19 MB. The open database was
  407.65 MB plus 9.54 MB WAL; canonical facts occupied 72.08 MB, encoded evidence
  126.22 MB and effective record JSON 15.39 MB. The remaining database footprint
  includes keys, indexes, row/page structure and checkpoints. Unresolved issues,
  pending responses and missing-parent associations were all zero.
- `./scripts/verify.sh full` passed: formatting, strict Clippy, 1,630 library
  tests plus integration suites, five isolated real-tmux lifecycle cases,
  twelve isolated daemon-routing cases and release build. `git diff --check`
  passed; the final fork delta was inspected. Three independent reviewers
  reported no remaining blocker in their assigned scopes; the primary agent
  integrated their changes and verified the final real-corpus result.
- The 520,000-effective-event synthetic fixture imported in 40.81 seconds, used
  1.785 GB on disk and added 9.52 MB peak RSS. Twenty recent-window queries had
  median 560 ms and maximum 834 ms (therefore p95 below the 1-second gate).
  This is approximately ten times the current effective-event count, not a
  simulation of every possible future transcript shape.
- An initial million-event run exposed unnecessary query ordering and identity
  allocation. Removing ordering and decoding only countable fields met the
  10x gate without rollups, derived SQL columns or another cache.

The persisted evidence/revision/index footprint is larger than the planning
prototype. It retains canonical source facts and normalized interpretations;
that overhead is intentional and is reported by `usage-history status`. No
source-content archive or automatic retention pruning was added to reduce it.
