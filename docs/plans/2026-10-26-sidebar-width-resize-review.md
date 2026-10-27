# Sidebar Width Resize: Independent Review Reference

> Historical research, archived at closeout on 2026-10-27. The original
> discussion below is preserved. Sidebar width restoration shipped in
> `v0.16.0-hza.1`; the [accepted decision](../decisions/2026-10-26-sidebar-width-restoration.md) describes the implemented
> scope. Unselected alternatives are not outstanding release requirements.

Date: 2026-10-26.
Status: discussion material; no implementation or design approval.

Read this alongside [the original proposal](2026-10-26-sidebar-width-resize.md).
This supplement records additional findings and questions from a review of
that proposal. It does not supersede the existing singleton decision or certify
the proposed implementation.

## 1. Problem and existing contract

Terminal resizes can change the sidebar's width because tmux owns pane layout.
The original proposal reports isolated-server measurements showing width drift,
including drift that remains after the terminal returns to its original size.
Those experiments were not repeated in this review.

The current implementation applies `@sidebar_width` when creating a slot.
Existing slots retain their geometry when the singleton sidebar moves between
windows. The configured default is 35 columns; percentage values are resolved
against the target window's width.

The existing [singleton decision](../decisions/2026-09-01-singleton-sidebar-pane.md)
explicitly preserves existing slot geometry and prohibits resize/layout hooks
from scanning every window. Making configuration authoritative after resize
would change this contract. It should be documented as a fork policy change,
even though it addresses an observed usability problem.

## 2. Evidence checked in this review

| Source | Finding | Limit |
| --- | --- | --- |
| `agent-sidebar.conf` | Seeds width to 35; `client-resized` invokes `follow` | Does not establish notification ordering |
| `src/cli/toggle.rs` | Width resolution supports percentages; automatic follow returns early unless exactly one client is attached | A successful return can mean that follow did nothing |
| `src/cli/toggle/topology.rs` | Ownership is validated; inventory deduplicates records by pane ID | A single added session field would not represent all memberships of a linked window |
| `README.md` and installation documentation | Advertise tmux 3.0+ | Newer hooks cannot be assumed available across that range |
| Local `tmux -V` | Reports `3.8-fork+6757fa17f111` | Version identity alone does not verify hook behavior |
| Strings in the local tmux binary | Includes `window-resized`, `window-unzoomed`, and `window-zoomed` | Presence is not an execution test |
| Current official tmux manual | Documents window resize and zoom notifications | Does not establish their availability in tmux 3.0 |

The [official manual's hooks section](https://man.openbsd.org/tmux.1#HOOKS)
documents `window-resized` and warns that it can occur after `client-resized`.
It also documents `window-unzoomed` and `window-zoomed`. Consulted on
2026-10-26; this is current documentation, not a version-specific compatibility
reference.

No live tmux server was changed. No new isolated hook experiment was performed
for this review. Repository checks were read-only before writing this document.

## 3. Separate width policy from event selection

Two decisions should be evaluated separately:

1. What determines the desired width: configuration, a recorded manual
   override, or existing geometry?
2. Which events justify restoring that width, and which window is targeted?

The original Option A combines configuration authority with a
`client-resized` handler that scans the client's session. Configuration
authority does not require that particular event or scope.

### Width policy alternatives

| Policy | Benefit | Cost or limitation |
| --- | --- | --- |
| Configuration remains an initial width | Preserves the current contract | Leaves terminal resize drift unresolved |
| Configuration defines the target after specified events | One durable source of truth; no override state | Manual widths expire at those events |
| Manual changes create per-window overrides | Can preserve a user's preferred local width | Requires intent classification, reset semantics, and interaction rules for later configuration changes |

The original proposal records a maintainer preference for configuration
authority. This review treats that as context, not approval of a particular
implementation or of resetting width after unzoom.

Option B's intent capture needs further validation. A difference between actual
and target width does not identify its cause. Scripts, explicit pane commands,
layout changes, zoom transitions, and space constraints can affect geometry.
The proposed equality-based recursion guard also needs testing when tmux cannot
reach the requested width: a clamped repair might otherwise be recorded as an
override.

## 4. Event and scope alternatives

| Event strategy | Potential benefit | Unresolved issue |
| --- | --- | --- |
| Extend `client-resized` and scan the session | Reuses an existing hook and follows the original proposal | Must verify that affected window sizes have settled; scans windows whether or not they changed |
| Repair only the window named by `window-resized` | Follows the object whose dimensions changed; may avoid session-wide scans | Must verify version support, target context, ordering, coverage, and whether repair retriggers the event |

The manual's ordering caveat means that a `client-resized` repair cannot assume
final window geometry without additional evidence. The original successful
probe remains useful evidence for its tested sequence; it is not proof of all
notification orderings.

Window-local repair could also reduce ambiguity around windows linked into
multiple sessions. The original sketch adds `session_id` to an inventory that
already deduplicates by pane ID. Filtering that inventory by one session ID may
lose another valid membership. This is a structural concern from code review;
the linked-window case has not been reproduced here.

Even window-local repair can affect multiple clients when they share a window.
Choosing a window event does not by itself settle the multi-client policy.

## 5. Zoom policies

The original experiments report that resizing a pane can unzoom its window,
and that resizing a client while zoomed can distort the hidden layout. These
findings motivate skipping width repair while zoom remains active.

| Policy | Result | Maintenance and behavior cost |
| --- | --- | --- |
| Skip while zoomed; wait for another resize | Smallest behavior change | Width can remain wrong after unzoom indefinitely |
| Skip while zoomed; restore configured width on every unzoom | Could avoid a stale marker and repair before normal use resumes | Unzoom also discards a temporary manual width when no terminal resize occurred |
| Record resize while zoomed; repair on unzoom only when marked | Preserves the narrower rule that resize expires a manual width | Adds marker ownership, cleanup, and transition tests |
| Unzoom, repair, then rezoom | Attempts immediate repair | Must preserve the correct zoomed pane, focus, and layout; original probes reported difficulty |

Repairing on every unzoom is a distinct product policy. It is not an equivalent
implementation of "only terminal resize restores the configured width."
Whether the `window-unzoomed` hook can implement this cleanly remains unverified.

## 6. Additional boundaries to decide

### Narrow windows

The original probe reports a 30-column window with a 28-column sidebar and a
1-column main pane after repair. Accepting tmux's clamp is mechanically simple,
but gives most available space to the sidebar.

The maintainer must decide whether this is acceptable or whether the sidebar
should yield space to ordinary panes. Any additional width cap needs an explicit
rule, including its interaction with percentage settings. No cap or minimum
main-pane width has been selected.

### Multi-client behavior

Current automatic following runs only with exactly one attached client. Width
repair could retain that guard or operate independently because it does not
move the singleton. Either policy is possible, but it must be intentional.

The original sketch runs repair after `follow_sidebar` returns successfully.
Since that function also returns success for no-op cases, the sketch does not
inherit its single-client or enabled-intent guards automatically.

### Compatibility

A design for the maintainer's current tmux fork may use capabilities absent
from the advertised tmux 3.0 baseline. Before selecting native window hooks,
establish their introduction versions and the intended support boundary.
Do not add a compatibility fallback until this boundary is decided; two event
paths create additional ordering and test obligations.

### Layout changes and attach

The original Option A leaves manual layout changes alone until a terminal
resize. A window event strategy needs tests showing whether it preserves that
boundary. Attach behavior also needs a test; an observed resize notification
during one attach sequence does not establish complete attach coverage.

## 7. Candidate for further evaluation

One candidate retains configuration authority while changing event scope:

- Resolve the target width using the existing helper.
- On an actual window resize, repair only validated owned panes in that window.
- Skip zoomed windows.
- Optionally repair on unzoom, if that policy is explicitly accepted.
- Keep geometry repair within the existing lifecycle/topology ownership and
  serialization boundaries.
- Leave ordinary follow/navigation behavior unchanged.

This candidate could avoid session scans and override state. It depends on
native event behavior and compatibility that have not yet been established.
It is a hypothesis for evaluation, not a selected design. The original Option A
remains the minimal implementation candidate using the already registered hook.

## 8. Validation needed before choosing an implementation

Run real tmux checks on an isolated server or with `TMUX_TMPDIR`, never the
developer's live server. Record hook targets, event order, final window and pane
dimensions, zoom state, and whether repair causes another event.

| Scenario | Question to answer |
| --- | --- |
| Real PTY client shrinks and grows repeatedly | Does repair run after the window reaches its final size? |
| Pane width repair itself | Does it retrigger the selected hook or cause a loop? |
| Inactive windows and multiple windows | Which windows resize, and does each receive an accurate event? |
| Window linked into two sessions | Is the window repaired once and targeted correctly? |
| Zoom, resize, then unzoom | Is zoom preserved until the user exits it, and is final width correct? |
| Manual width change, zoom, then unzoom without resize | Does behavior match the chosen manual-width contract? |
| Very narrow window, then growth | Is space allocation acceptable, and does the configured target recover? |
| Percentage width | Is resolution based on final window width? |
| Attach, detach, and multiple clients | Does repair follow the explicit client policy? |
| `select-layout`, pane split, and manual pane resize | Are unrelated geometry changes preserved as promised? |
| Ordinary window navigation | Does plain follow still preserve slot width? |
| Pane disappears or ownership is invalid | Does repair stop safely without disrupting follow? |
| Oldest supported tmux version | Are hooks supported and do relevant semantics match? |

Any accepted design should update the singleton decision and architecture
invariants where it changes them. Implementation verification should use the
repository's full verifier and fork-delta report, with proportional fixture and
isolated-tmux regressions.

## 9. Questions for the next reviewer

1. Is configuration authority the desired contract, including how temporary
   manual width changes expire?
2. Does the target tmux support a reliable window-local event path, and must the
   solution retain tmux 3.0 compatibility?
3. Should unzoom always restore width, restore only after a recorded resize, or
   leave repair deferred?
4. Should width repair retain the single-client restriction?
5. What should happen when the configured width leaves ordinary panes unusable?
6. Which candidate satisfies these decisions with the least persistent state
   and the smallest change to existing lifecycle ownership?
