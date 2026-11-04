# Decision: Passive Codex quota with daemon-owned fallback

## Context

Independent quota requests frequently time out and duplicate quota information
already carried by normal Codex traffic. The running daemon, not the disk login,
is the execution account: this workflow switches accounts with cc-switch and
restarts the daemon. A live read-only probe on 2026-11-04 confirmed that the
existing control WebSocket supports `account/rateLimits/read` (1.47 seconds),
returns `accountId`, and preserves the account across the query.

## Chosen Seam

Keep the existing usage collector, quota worker and main-loop channels. Extract
allowlisted quota observations from bounded recent-file reads in the usage
worker, independently of whether the line contains countable usage. The durable
metering scanner does not supply a second quota stream. Retain bounded ephemeral candidates;
never put quota in canonical evidence, parser checkpoints or SQLite. Resolve
exact session/turn attribution again on publication so late observations and
conflicts remain effective. Never infer identity from creator metadata.

The running daemon supplies the account for both usage and quota. Accept local
snapshots only for that account. The user explicitly accepts official rollout
quota and actively queried quota as equally authoritative observations. Event
time approximates snapshot time: cached re-emission can briefly regress values.
Reject older events and duplicate records, preserve API baselines for attribution
revocation, and do not let a slow API reply overwrite observations made after its
request began. Account changes invalidate old snapshots and cycle ownership.

Use the existing bounded control transport for active reads, verifying the
account before, in and after the response. Remove Codex's independent HTTP,
OAuth writeback and temporary app-server fallback. Missing daemon is a failure,
not permission to query another disk account. Kimi is unchanged.

Startup without data waits 60 seconds. Local observation interruption first
checks after five minutes for running/background panes and fifteen minutes for
idle/waiting or exhausted quota. Successful unchanged checks double their wait
up to four hours regardless of pane status. Fresh complete local coverage
suppresses ordinary active requests. Reset boundaries receive one check 30–60
seconds after the boundary and at most one further check two minutes later;
small reset timestamp drift must not create an endless retry sequence. Failures
back off independently. Current Codex RPC wraps backend errors without HTTP
status or Retry-After; local backoff covers those failures, without guessing from
free text. Kimi retains provider minimum waits. Active Codex reads set
`excludeResetCreditDetails: true` to omit unused reset-credit details. All triggers
coalesce in one worker; no concurrent quota requests are introduced.

API failure state and displayed freshness are separate. Recent local observations
can keep the display healthy while API retry state survives. The layout does not
change. Weekly usage cycles derive from the same validated account/window
projection, whether supplied by a rollout or an active read. Only a valid future
weekly boundary establishes a cycle; account changes, expiry and attribution
revocation invalidate it. Only changed cycle metadata forces history recomputation.

Separate file discovery from following known files, as in Vector's file source
and Fluent Bit's tail input. Keep full discovery at thirty seconds and existing
force signals, but check a bounded set of recent Codex files every second.
Reuse the existing worker, parser and read budgets; give history backlogs fair
progress. Account probes and unchanged history queries do not run on every tick.
New files can still wait for discovery; the fast path is for known recent files.
The hot set holds at most sixteen files, reads at most 256 KiB of appended data
per file per tick, and admits newly discovered files modified in the last fifteen
minutes. Startup/replacement tail reads share a 4 MiB budget per tick, with a
4 MiB per-file window and rotating service order; a long turn can place its
identity declaration far beyond the ordinary append budget. Header reads are
capped at 64 KiB. Hot metering catch-up and ordinary backlog
receive separate 100 ms and 200 ms processing budgets; full discovery appends
work rather than repeatedly pushing completed files ahead of the backlog.

On restart, recover recent quota through a bounded head/tail read without
replaying metering or changing checkpoints. Establish the file's own session and
fork boundary from its first header, and require an explicit turn context before
accepting tail observations. Never apply the checkpoint's final turn to earlier
records. Preserve original observation times and reject incomplete identity.
Read-only history followers use the same quota extraction in their existing
worker, without writing evidence or checkpoints. SQLite retains one writer;
quota remains ephemeral in every instance.

## Alternatives Rejected

- Local events merely forcing API queries: retains the expensive failing path.
- Fixed five-minute reconciliation during normal traffic: duplicates usable data.
- A resident extra server, new collector, or durable quota ledger: unnecessary.
- Guessing identity from current auth.json or creator_account_id: mixed accounts.
- Strict global sampling order: rollout carries no server snapshot revision;
  this display accepts bounded observation uncertainty rather than inventing one.

## Upstream Compatibility

Keep upstream orchestration and module ownership. Integration remains within the
fork-owned usage/quota leaves, the existing adapter control transport and one
main-loop usage-update application method. No hook declarations or schema change.

## Conflict and Removal Strategy

Preserve upstream parsing and worker/channel boundaries during merges. Passive
extraction is removable without replay or migration. Reverting the quota leaves
and their integration restores the previous behavior without touching transcripts,
SQLite history, credentials or hooks. No automatic live sidebar restart occurs.
