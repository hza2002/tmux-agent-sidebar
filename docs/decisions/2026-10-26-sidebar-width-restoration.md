# Decision: Restore Sidebar Width After Window Resizes

## Context

Tmux redistributes pane geometry when the terminal changes size. The sidebar's
configured width previously applied only at slot creation, so repeated resizes
could leave it substantially narrower or wider than requested. This is a fork
policy change to the geometry contract in the singleton sidebar decision.

The configured width remains the sole durable width preference. Manual pane
resizing is temporary until the next window-size change; ordinary navigation
and layout edits must preserve it. Zoom must retain the entire window, and
automatic width correction must remain disabled with multiple attached clients.

## Chosen Seam

Use the existing lifecycle lock and topology ownership checks through a narrow
window-local width observer. A background `window-layout-changed` hook passes
the affected window ID and captures its dimensions before launching the observer.
Callbacks with valid captured dimensions equal to their captured baseline and
no pending zoom repair return before acquiring the lifecycle lock. Pane swaps
can emit these callbacks during ordinary focus changes; they are not resizes.
Missing/invalid baselines, actual changes and pending repairs retain the locked
observation path. The observer records window dimensions and distinguishes
actual width/height changes from pane-only layout edits. It operates on existing,
strictly validated sidebar and empty-slot panes only. It cannot create or move
the sidebar and does not change focus.

Record a dimension baseline when creating an owned slot and initialize missing
baselines on plugin configuration reload without resizing existing windows. A
window without a baseline is observed without guessing that its manual width is
drift. Captured event dimensions also detect a rapid shrink/grow cycle that ends
at the original size before the background observer gets to run. Each accepted
resize advances the observation revision. A captured revision prevents a delayed
old callback from overwriting a manual width set after the resize was repaired.
Record dimension changes before issuing a repair so repeated notifications and
tmux's width clamping cannot cause a repair loop. Resolve percentages against
the current window width using the existing configuration helper.

While zoomed, record a pending repair only if the window dimensions changed.
When zoom ends, repair only that pending change. The final conditional tmux
command rechecks zoom in either direction and clears pending state after a
completed repair; it must not depend on another queued callback arriving. An ordinary zoom/unzoom cycle
preserves a manual width. The pending state belongs to the window, so moving the
singleton through an empty slot does not move the pending layout repair.

With zero or multiple clients, or while sidebar intent is disabled, do not
repair. Refresh observation state and discard pending repair so an old event
cannot overwrite a later manual adjustment. Disappearing ownership also clears
the observer's window metadata. No custom width cap or automatic hiding is
added: tmux applies its normal size constraints.

## Alternatives Rejected

- Repairing after `client-resized` can observe old dimensions. The current
  manual explicitly permits `window-resized` to occur later.
- Session-wide scans assume all windows resize together and mishandle linked
  window membership. Observe the physical window that changed instead.
- Native `window-resized` is absent in tmux 3.0–3.2; native zoom hooks are absent
  through 3.7. A single legacy layout observer preserves the advertised 3.0
  baseline without separate version-dependent event paths. The pre-existing
  optional `after-kill-pane` maintenance hook is capability-checked on config
  load, since tmux 3.0 rejects that hook and would otherwise fail configuration
  reload before verification completes.
- Per-window manual width overrides add another preference and reset policy.
- Unzoom/repair/rezoom changes active user layout and is unnecessary.
- TUI polling or SIGWINCH repair would duplicate lifecycle ownership.

## Upstream Compatibility

The TUI, agent events, quota, and list presentation are unchanged. Width policy
lives beside the existing toggle/topology lifecycle; hook registration stays in
the tmux plugin configuration. Existing navigation width-preservation tests
remain valid. The new observer metadata is derived bookkeeping, not a second
source of desired width.

## Conflict and Removal Strategy

Preserve upstream lifecycle and ownership changes first, then reapply the small
width observer. Removing its hook, command, and window bookkeeping restores
creation-only sizing without migrating user data. Reload the tmux plugin config
after updating the hook declaration; rebuilding the binary alone does not
register the observer in an already running tmux server.

## Evidence and Verification

Isolated PTY experiments on the maintainer's tmux build found client resize
notifications preceding final window geometry, while layout notifications
observe the completed resize. Pane resize and layout selection also emit layout
notifications but retain window dimensions. Zoom-internal notifications must
always be checked against the current zoom flag.

The official tmux 3.0 sources emit `window-layout-changed` after window resizing
and on zoom transitions. Relevant primary sources:

- <https://github.com/tmux/tmux/blob/3.0/resize.c>
- <https://github.com/tmux/tmux/blob/3.0/window.c>
- <https://github.com/tmux/tmux/blob/3.0/options-table.c>
- <https://man.openbsd.org/tmux.1#HOOKS>

Verify fixed/percentage widths, repeated and height-only resizes, manual edits,
zoom deferral, linked/inactive windows, client-count guards, ownership loss, and
narrow-window clamping on isolated tmux servers. Run the full repository verifier
and inspect the fork delta before handoff.

The final three width integration cases passed on the maintainer's tmux 3.8
fork and an isolated build of the official tmux 3.0 source. They exercise real
PTY resizes and actual plugin configuration reload, including duplicate hook
registration, percentage and clamped widths, delayed resize bursts, zoom, linked
windows, and multiple clients. Tests wait for observed dimensions and a lifecycle
barrier before asserting that a later manual edit survives; an edit made while
a resize is still being processed can race that pending restoration.
