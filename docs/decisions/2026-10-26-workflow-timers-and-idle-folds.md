# Decision: Workflow Timers and Idle Session Folding

## Context

The sidebar should make outstanding work easy to find without hiding replies or
background activity. A single turn-start clock does not explain how long a
permission request or unread reply has awaited attention. Repository folding
alone also hides active work together with idle sessions.

These are fork-owned presentation and workflow policies. Configuration hot
reload, additional quota sources, and new agent integrations are outside this
change.

## Chosen Seam

Keep the existing hook-to-tmux-to-query-to-state-to-render direction. Hook
transitions own meaningful timestamps; the row renderer chooses the clock for
the observed workflow state. Running uses the current turn start, actionable
waiting uses the request age, unread replies use their availability age, and
errors use their occurrence age. Idle and reviewed background rows omit clocks.
Repeated reports must not restart a clock, and a permission overlay must not
erase the underlying reply's age. Unknown timestamps are omitted rather than
replaced by an unrelated turn-start time. Waiting and unread ages take priority
over branch metadata when space is limited; running duration remains secondary.

Use the existing list derivation and input seams to fold only read idle sessions
without background work. The default is expanded. In the agent list, `z`
toggles the global fold mode and clears local exceptions. `Enter` on a repository
header toggles that repository's idle sessions; on a pane it retains its existing
navigation behavior. Merely focusing a header never expands it. A fully idle
repository retains one navigable header with an idle count. Mixed repositories
retain active rows and show `▸ <idle icon> N` only while idle sessions are hidden. Expanded
headers omit fold indicators; toggling does not occupy the bottom row with hints.
New activity becomes visible automatically.

Preserve selection by stable repository or pane identity. If a selected idle
pane is hidden, select its repository header. Derive keyboard targets and mouse
hit targets from the same visible rows. Persist the global user preference
through existing tmux global state; local repository exceptions remain UI state.

## Alternatives Rejected

- Polling additional sources for timer state would duplicate hook ownership.
- Reusing every status write as a new transition would reset ages on repeated
  events and confuse workflow ordering with elapsed time.
- Hiding entire repositories would obscure actionable work.
- Automatic age-based folding would infer importance from elapsed time.
- Expanding on focus would make keyboard traversal move its own targets.
- Vim's full multi-key fold vocabulary adds unnecessary interaction state for
  one foldable category. A single `z` toggle plus contextual `Enter` is sufficient.

## Upstream Compatibility

Retain upstream's state, grouping, query, and renderer ownership. New metadata
must participate in the existing bulk query and cleanup contract. No new worker,
dependency, hook registration, or alternate list renderer is required.

## Conflict and Removal Strategy

On upstream changes, preserve the new event and list structures first, then
reapply timestamp semantics and idle visibility at their existing seams. Remove
the timer metadata and consumers together if upstream supplies equivalent
timestamps. Idle folding can be removed with its input actions, global preference,
local exceptions, and row presentation without changing agent workflow state.
