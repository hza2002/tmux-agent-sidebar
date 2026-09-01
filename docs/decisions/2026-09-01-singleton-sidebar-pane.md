# Decision: Singleton Sidebar Pane

## Context

The sidebar renders server-wide agent state, so one pane per tmux window runs
the same query and render loop repeatedly. The primary workflow has one
attached client and switches between sessions and windows while expecting the
same fixed-position sidebar to remain visible.

## Chosen Seam

Sidebar lifecycle remains owned by `cli/toggle.rs`, with empty-pane topology
operations isolated in the private `cli/toggle/topology.rs` leaf module. At
most one live pane marked with `@pane_role=sidebar` is retained across the tmux
server. The durable `@agent_sidebar_enabled` option records user intent; live
pane and slot locations are always derived from one authoritative tmux pane
inventory rather than persisted separately.

Windows materialize an owned `@pane_role=sidebar-slot` lazily. The first visit to
a window without a slot creates a processless tmux pane using an empty command.
Later visits use `swap-pane -d` to exchange the live sidebar and empty slot, so
the slot's current layout cell remains stable. Existing slot geometry is
authoritative: tmux resize behavior and user layout changes are preserved rather
than repaired back to configured defaults.
Slots are owned only when their role, zero PID, live state, and empty TTY all
match. Live sidebar ownership additionally requires either the lifecycle token
`@agent_sidebar_owner=tmux-agent-sidebar` or `@sidebar_pid` matching the pane
PID. The token is written before process startup, closing the PID publication
race. An invalid role marker is removed without killing that pane's process.

Explicit toggle commands enable, focus, summon, or disable the singleton.
Lifecycle hooks reconcile the sole attached client's current target, recover a
missing enabled live pane, and remove infrastructure-only windows. Notifications
that fire before tmux commits a client/session switch schedule a background
re-query, so every invocation converges on current tmux state rather than
replaying an event target.

Lifecycle commands are serialized per tmux server by a recoverable local lock.
This closes the list-then-create race between explicit toggles and delayed
auto-create hooks without introducing a daemon or a permanent tmux lock.

Slot creation uses the existing configurable `@sidebar_width` and
`@sidebar_position` options. These options apply only when a slot is created;
an existing slot is reused without geometry queries or resizing. Resize and
layout hooks never fan out across every window. The plugin continues to seed
defaults in `agent-sidebar.conf`, and the prefix binding remains controlled by
`@sidebar_key`.

Only the active pane and zoom state needed around a swap are retained. This
bounded restoration metadata is required because tmux 3.0 `swap-pane` unzooms
both windows and does not support the later `-Z` flag. Full window layouts are
not replayed because they can become stale and overwrite legitimate pane
changes made while the sidebar is present.

Disabling writes the intent off before removing slots and the live pane, so
cleanup hooks cannot recreate the sidebar. If the last ordinary pane exits, an
empty-slot-only window is released. A live-only window moves the sidebar to an
existing ordinary pane when possible; otherwise the live pane exits so tmux may
end the final window and server naturally.

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
- Eagerly creating slots in every window has no reliable declarative-layout
  completion signal and can race tmuxinator or `select-layout` workflows.
- Capturing and injecting ANSI frames adds cache correctness, history growth,
  and stale multi-client states without improving lifecycle correctness.
- Replaying saved `window_layout` values risks reverting newer user changes.

## Upstream Compatibility

The TUI, state model, and rendering paths are unchanged. The tmux query boundary
only excludes the new `sidebar-slot` role alongside the live sidebar role. Fork
behavior is otherwise contained in the lifecycle module, its private topology
leaf, plugin hooks, and related docs. When syncing upstream changes in
`toggle.rs`, `tmux/query.rs`, or `agent-sidebar.conf`, preserve new upstream
structure first, then reapply this role filter and singleton policy through the
same seams.

## Failure and Removal Strategy

Ambiguous swap failures re-query the live pane location; an uncommitted failure
leaves both live pane and slot in place for the next reconciliation. Closing the
last sidebar pane still protects ordinary user windows, while slot-only windows
are deliberately allowed to close. Later hooks never recreate an explicitly
disabled sidebar.

Removing this policy requires disabling intent, deleting only strictly
validated `sidebar-slot` panes, restoring the previous cross-window movement,
removing the lifecycle repair hooks and topology leaf, and deleting the slot
role from the query filter.
