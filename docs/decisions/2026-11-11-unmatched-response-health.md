# Decision: Unmatched response evidence is status-only

## Context

The durable usage store keeps a Codex `token_usage_record` pending until a
matching `token_count` or compaction marker reconciles it. The previous rule
raised `用量历史不完整` once a pending record was older than three fallback
scan intervals (90 seconds). A killed subagent or interrupted turn leaves a
final `token_usage_record` with no possible partner, so the notice stayed lit
forever for a record that cannot be recovered or acted on. Long tool calls in
active turns also exceeded 90 seconds, making the same notice flap on and off.

## Decision

Pending response evidence stays excluded from totals and remains visible in
`usage-history status` with its path, offset, session and response identity. It
no longer sets the header health notice or a history failure. The evidence is
not deleted: a later count or compaction partner still reconciles it into one
event. Storage, parsing, inventory and fork-parent failures keep their existing
immediate or persistent visibility.

Do not count a lone `token_usage_record` speculatively. The parent usage decision
retains the no-double-count contract: without a count or compaction association
the record cannot be proven uncharged. Accepting a small, enumerated undercount
is preferable to a permanent unactionable warning or a speculative duplicate.

## Consequences

`Usage` totals can be slightly low when a Codex turn is interrupted after its
response record is written. The gap is discoverable through
`tmux-agent-sidebar usage-history status` and never silently claimed as complete
by a header error. A systematic pairing regression is diagnosed through the same
status output instead of a repeated notice.
