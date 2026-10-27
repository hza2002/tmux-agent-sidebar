# Decision: Durable Personal Usage History

## Context

The existing in-memory usage cache loses contributions when transcripts disappear
and retains only 31 days. The accepted product now requires all available history
to survive source cleanup, while retaining no conversation content. The reviewed
[implementation plan](../plans/2026-10-26-usage-history-implementation-plan.md)
records the field, identity, failure, and recovery contracts.

## Chosen Seam

Extend the existing usage worker with a leaf SQLite store and source-specific
allowlisted evidence extraction. Persist selected metering facts, effective
events, file checkpoints, and issues. Update evidence and checkpoints in one
transaction. Keep historical pricing derived and preserve raw model/counter
information for later corrections. The TUI receives computed snapshots and
structured health; it performs no database or source I/O.

Store under the XDG data directory, independently of agent logs and plugin
installation. Retain event detail without automatic expiry. Use a single elected
collector inside existing workers; no new daemon or hooks. Existing quota
enablement still controls collection. Missing originals never delete history.

Use the existing notices surface for persistent visible failures. Diagnostic
logging remains independent of database availability. Destructive maintenance
requires the collector to stop, a verified SQLite backup, and atomic publication.
An unknown schema or corrupt database must never trigger automatic replacement.

## Alternatives Rejected

An expanded memory cache cannot survive transcript cleanup. Daily-only snapshots
lose arbitrary-window, directory/session, and historical repricing capability.
Full transcript archival and full Event Sourcing add unrelated ownership and
privacy obligations. A JSONL ledger would require custom transaction, indexing,
replacement, and recovery protocols. A local web server is a later presentation
feature and is not needed for durable collection.

## Upstream Compatibility

Preserve worker -> state -> renderer ownership. Source interpretation belongs in
usage leaves; hook adapters, tmux lifecycle, and repository identity remain
unchanged. Maintenance commands enter through the existing CLI dispatch seam.
No stable project ID or automatic directory rename matching is introduced.

## Conflict and Removal Strategy

Preserve upstream worker/state entry points and reapply the history leaf and
mailbox health integration. Removing the feature must leave its database intact;
the store is personal history, not disposable cache. Rebuilding indexes is not
permission to erase retained observations. Backup/export remain the supported
escape hatch, and restoration must preserve original WAL sidecars together.
