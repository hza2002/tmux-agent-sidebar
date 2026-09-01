# Decision: Singleton Sidebar Pane

## Context

The sidebar renders server-wide agent state, so one pane per tmux window runs
the same query and render loop repeatedly. The primary workflow has one
attached client and switches between sessions and windows while expecting the
same fixed-position sidebar to remain visible.

## Chosen Seam

Sidebar lifecycle remains owned by `cli/toggle.rs`. At most one pane marked
with `@pane_role=sidebar` is retained across the tmux server. Explicit toggle
commands create, focus, move, or close that pane. Lifecycle hooks ask an
internal `follow` command to move an existing sidebar to the sole attached
client's current window; hooks never create a sidebar. Notifications that fire
before tmux commits a client/session switch schedule a background re-query, so
every invocation converges on the client's current state rather than retaining
an event target.

Lifecycle commands are serialized per tmux server by a recoverable local lock.
This closes the list-then-create race between explicit toggles and delayed
auto-create hooks without introducing a daemon or a permanent tmux lock.

Pane movement uses tmux's `move-pane` operation and the existing configurable
`@sidebar_width` and `@sidebar_position` options. The plugin continues to seed
their defaults in `agent-sidebar.conf`. The prefix binding remains controlled
by `@sidebar_key`, including its existing `off` behavior.

Only the active pane and zoom state needed around a move are retained. Full
window layouts are not replayed because they can become stale and overwrite
legitimate pane changes made while the sidebar is present.

## Multi-Client Policy

A single pane cannot be visible in two client windows simultaneously. Automatic
following therefore runs only when exactly one client is attached. With
multiple attached clients the pane stays where it is; an explicit toggle may
still summon it to the invoking window.

## Alternatives Rejected

- One sidebar process per window duplicates identical polling and rendering.
- A daemon plus thin pane clients adds IPC and still leaves one client process
  per pane.
- Recreating the pane on every switch loses process identity and interactive
  state.
- Replaying saved `window_layout` values risks reverting newer user changes.

## Upstream Compatibility

The TUI, state query, and rendering paths are unchanged. Fork behavior is
contained in the existing lifecycle module, plugin hooks, and related docs.
When syncing upstream changes in `toggle.rs` or `agent-sidebar.conf`, preserve
new upstream structure first, then reapply the singleton policy through these
same seams.

## Failure and Removal Strategy

Follow failures leave the existing pane in place. Closing the last sidebar pane
first creates a normal shell pane so tmux never loses the window's final pane.
Ambiguous move or close failures preserve that safety pane; a visible extra
shell is preferable to destroying a window or session. Later window switches do
not recreate a closed sidebar. Removing this policy requires restoring
per-window creation semantics, deleting the follow hooks and command, and
removing this decision record together.
