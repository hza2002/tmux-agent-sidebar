# Decision: Claude replies and background work

## Context

Claude can finish a parent response while background agents and commands keep
running. Clearing children at Stop loses work, while treating every background
task as a reason to withhold a reply hides useful responses. The existing
child-list guard also rejects legitimate parent prompts while children run.

## Chosen Seam

Keep the adapter -> event -> hook -> tmux query -> refresh -> renderer flow.
Normalize optional background snapshots at the Claude adapter. Reconcile parent
Stop snapshots, preserve unknown snapshots, and remove the stopping child from
SubagentStop snapshots. Only parent Stop replaces non-subagent registry work;
child snapshots may precede a newer parent observation. Track child identities
separately from parent ownership.
Use the existing response-ready/reviewing flow and Background status; reviewing
a response does not end live work. Child drain does not publish parent completion.
Monitors and scheduled future wakeups do not make a session busy.

Keep existing subagent and shell presentation. One pane option carries other
live background tasks through the bulk query. No new hook, daemon, timer-based
completion, configuration, or transcript polling is required. Two small hook-only
options retain stopped child IDs against delayed snapshots and the parent state
under pending child permissions. A resumed child Start or observed child activity
clears its stopped marker; a new parent run rebases pending permission state.

## Alternatives Rejected

Deferring all completion until every process exits would hide replies and keep
long-lived services perpetually unfinished. Retaining children without fixing
event ownership would reject parent wakeups and notifications. A separate task
registry service would duplicate Claude's registry and the existing state flow.

## Upstream Compatibility

This is a lifecycle correctness fix with the fork's existing reply-review policy.
Other adapters retain their existing completion behavior. Missing fields remain
unknown, not evidence that work ended. Stop hooks can be continued by other hooks;
this integration reports observed replies and activity, not irreversible task
completion. Snapshots are observations, not a transactional task event stream.

## Conflict and Removal Strategy

Preserve upstream adapter, handler, query and rendering ownership when merging.
Reapply this behavior through those seams; do not retain a parallel pipeline.
Removing the extra pane option and normalized snapshot fields restores the old
schema without migrating user data. Regressions cover parent/child isolation,
reply review with live work, snapshot absence/emptiness, and child drain.
