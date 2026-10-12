# Decision: Route Codex daemon hooks through client titles

Date: 2026-10-11
Status: Implemented; full verifier and isolated Codex 0.162.1 acceptance passed.
Classification: Bug fix in shared behavior, with a fork-specific integration.

## Context

Codex 0.162.1 runs hooks inside a shared daemon. Its inherited `TMUX_PANE`
and `TMUX` identify the daemon's original environment, which can belong to
another client or server. A thread can have zero or several attached TUIs.
The sidebar needs the panes currently displaying that thread.

This fork also owns response review transitions. Importing a daemon status
poller would create a competing status authority and overwrite those transitions.
The evidence and executable design are in the
[design document](../plans/2026-10-11-codex-daemon-routing-design.md).

## Chosen Seam

Add a Codex routing leaf at the hook entry point. Normalize the event first,
then route it using the client-written thread identity in `pane_title` and a
qualified live TUI process. For truncated titles, resolve the prefix against
one bounded, read-only `thread/loaded/list` snapshot. Query IDs only.

Keep existing handlers and tmux options authoritative for hook transitions,
and keep refresh authoritative for the fork's response review transitions.
Fan out to matching panes under the existing per-pane serialization contract.
Revalidate the title and process after taking each lock. Emit a Stop protocol
response once per hook invocation, including when routing fails.
Daemon-routed permission, activity, and Stop require an established matching
session and turn, a `title-v1` routing marker, and the selected client PID; they cannot bootstrap partial lifecycle metadata.

Pin the tmux server and Codex control socket explicitly in generated Codex
hook commands. Setup remains a configuration generator. Add prerequisite
notices and stale-title rejection through the existing query/refresh seams.
Independent implementation review added two required safeguards. Only panes
with `@pane_codex_routing=title-v1` undergo title invalidation; their qualified
client PID is recorded in `@pane_codex_client_pid`. Legacy lifecycle starts
clear those options. Legacy routing requires a freshly verified interactive
client ancestor outside daemon and exec subtrees, both before and after locking.
Locks and activity filenames share canonical tmux socket identity so different
servers' identical pane IDs cannot share or delete one another's activity.
Existing unscoped activity files are not migrated because their server identity
cannot be recovered; new activity starts in the scoped namespace.

Use `tungstenite` with only the features needed for Unix WebSocket framing;
do not implement WebSocket framing in the product or add a TOML editing dependency.

This is a compatibility design for the installed Codex interface. A truncated,
asynchronously updated title is not a contractual full-ID client binding.
The design rejects observed ambiguity and routing failures, but does not claim
linearizable selection or unconditional recovery of missed lifecycle events.

## Alternatives Rejected

- Importing danhper's complete pull/push implementation would override this
  fork's review states and couple hook routing to pull-created pane metadata.
- Unchecked prefix routing accepts conflicting thread identities.
- Cwd, active pane, and bare ancestry identify neither the selected thread nor
  all attached clients. A managed daemon can remain below its first TUI in the
  process tree.
- The installed `app-server proxy` is a raw byte relay. It cannot reuse the
  quota client's newline JSON protocol against the WebSocket control socket.
- A session journal would replace the established hook-to-tmux state pipeline.
- `--no-daemon` remains a verified operational fallback, but removes capabilities
  that the requested daemon-compatible fix must retain.

## Upstream Compatibility

Preserve the adapter/event/handler/query/refresh/UI dependency direction.
Keep Codex endpoint and prefix resolution local to the routing leaf. Other
agents retain their current entry path. Setup keeps registrations owned by
the adapter table. Query remains one bulk `list-panes -a` operation.

The ID lookup dependency is justified by the installed title truncation and
the requirement to reject detected collisions. It can be removed if Codex
provides a supported full-ID selection signal.

## Conflict and Removal Strategy

During upstream merges, retain upstream hook dispatch and query structure,
then reapply routing before handler dispatch and validation before projection.
Keep shared lock identity changes separate from Codex routing policy.

Replace title parsing with a supported per-client displayed-thread signal
when available. Remove the ID lookup if that signal carries the full identity.
Do not retain the compatibility machinery once upstream supplies equivalent
binding semantics. A later status reconciliation feature needs its own decision
about ownership of hook state and local review state.
