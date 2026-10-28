# Decision: Mixed Codex routing and runtime account observations

## Context

A scoped hook can originate in either the shared app-server or an embedded TUI.
The shared server's loaded-thread catalog cannot identify an embedded thread.
A rollout's creator account does not change when another account resumes it.
The accepted local workflow uses one official account in the shared server and
third-party providers in embedded processes, with one execution owner per thread.

## Chosen Seam

Keep hook dispatch, pane handlers, the single usage collector and UI snapshots.
Positive hook-to-interactive-client ancestry takes precedence over title routing,
even with scoped hooks. Daemon routing excludes explicitly embedded clients.
Revalidate ancestry and pane lifecycle under the existing pane lock. Do not add
a second hook registration or use an inherited pane as a daemon fallback.

Move the bounded read-only control transport to the Codex adapter so hook routing
and the usage worker can share it. Prompt/Stop hooks observe the configured
server's account with refresh disabled; embedded observations carry no official
account. Publish only session, turn, timestamp, runtime and optional account ID
through private atomic spool files. Collection remains gated by sidebar quota.
The existing SQLite writer imports them idempotently into a reserved metadata
namespace, then deletes the spool file only after commit. This additive extension
needs no schema or normalizer change: source evidence, keys, counters and parser
checkpoints remain unchanged. Backup includes the metadata; export includes it
explicitly. Effective query projection joins on the selected evidence's exact
session and turn. Conflicting observations stay unknown, including after replay.

Persist a collection boundary. Before it, absent observations retain the creator
account as the explicitly accepted historical approximation. After it, absent
observations have no account, never the current disk login. Late observations
update query results without replaying transcripts. Status exposes counts and
the boundary. Older binaries can read the unchanged ledger but do not understand
this attribution overlay; they must not be used for account-scoped reporting.

The account row selects the local running daemon account, not auth.json. The
quota cycle carries the identity used by its quota request and is suppressed
when that identity differs from the observed daemon account. Failed refreshes
cannot transfer a cached cycle to another account. The quota client's
independently fetched subscription display is not the usage identity source.

## Evidence Limits

A daemon login observation is not a provider billing receipt. Attribution assumes
the accepted topology: shared server for the official account, embedded processes
for third parties. Per-thread custom routing inside that server is not supported
by this policy. No credentials, email, endpoint, prompt or transcript are stored.
Embedded official logins, missing hooks/turn IDs, transport failures and conflicts
remain unassigned. DeepSeek usage retains the existing model-based price bucket;
this change does not invent a provider identity from a model name. Subagent turns
without their own observations also remain unassigned. Observations at prompt and
stop cannot prove account changes within a request; ambiguity must not be hidden.

## Alternatives Rejected

- Polling auth.json or copying creator identity onto new turns: stale identity.
- Registering both legacy and scoped hooks: duplicate/conflicting pane writes.
- Hook processes writing the ledger: bypasses the existing collector lock.
- Rewriting imported metering records: changes canonical evidence and replay.
- A new database/service or credential store: unnecessary ownership and migration.

## Upstream Compatibility

The mixed-mode fix stays in the routing leaf and existing identity helpers. Usage
attribution stays in the fork-owned usage leaf and its existing worker. No raw
agent fields leak into lifecycle handlers; pane query and render flow is intact.

## Conflict and Removal Strategy

Preserve upstream dispatch and handler structure. Reapply the ancestry selection
before daemon title lookup, and keep attribution outside lifecycle mutation.
To remove attribution, stop publishing/importing the reserved metadata and remove
the query overlay; retain metadata for audits. Rollouts and original ledger
records require no rollback. Remove the transport extension when Codex supplies
stable per-turn serving identity in its own logs.
