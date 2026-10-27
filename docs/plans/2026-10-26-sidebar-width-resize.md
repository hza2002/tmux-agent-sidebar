# Sidebar Width Across Terminal Resizes

> Historical research, archived at closeout on 2026-10-27. The original
> discussion below is preserved. Sidebar width restoration shipped in
> `v0.16.0-hza.1`; the [accepted decision](../decisions/2026-10-26-sidebar-width-restoration.md) describes the implemented
> scope. Unselected alternatives are not outstanding release requirements.

Status: open design discussion. No production change is implemented.
Date: 2026-10-26.
Audience: maintainer and an independent architecture reviewer.

This document isolates one problem — the sidebar pane changing width when the
terminal (Ghostty, tiled by yabai) changes size — and lays out the evidence,
constraints, candidate designs, and open questions. It is written so a reviewer
with no prior conversation context can challenge the assumptions and recommend
a best practice.

## 1. Problem

The user runs Ghostty with yabai tiling. Opening or closing another window in
the same space changes Ghostty's terminal size, so the tmux client resizes and
tmux reflows the pane layout. A tmux window containing the sidebar is then
re-laid out with a different sidebar width. The sidebar remains usable, but its
width no longer matches `@sidebar_width`, and the user recovers by toggling the
sidebar off and on again, which recreates the slot at the configured width.

Goal: keep the sidebar at its configured width across terminal resizes, without
creating surprising behavior for users who adjust the width by hand.

## 2. Current Width Pipeline

The sidebar width is entirely a tmux layout property. The sidebar process never
sets its own pane width; the UI renders to whatever width tmux gives it.

1. **Option and default.** `@sidebar_width` is a global tmux option. If it is
   unset when the plugin config loads, `agent-sidebar.conf:53` seeds it to `35`.
   The value is a column count or a percentage of the window width. The user's
   live server currently has `@sidebar_width 35` and `@sidebar_position left`.
2. **Resolution.** `resolve_sidebar_width` (`src/cli/toggle.rs:515`) reads the
   effective option for a target window. Empty falls back to
   `DEFAULT_SIDEBAR_WIDTH_COLUMNS = 35` (`src/cli/toggle.rs:12`). A value ending
   in `%` is resolved against the window's current width as
   `window_width * percent / 100`, with a floor of 1 column.
3. **Application.** The width is applied only when a window first needs a slot:
   `create_slot` (`src/cli/toggle/topology.rs:184`) runs
   `split-window <flags> -d -l <width>` and marks the new pane
   `@pane_role=sidebar-slot`. `@sidebar_position` is used here as well.
4. **Reuse.** After creation, the singleton live sidebar moves between windows
   with `swap-pane -d` (`src/cli/toggle.rs:540`, `move_sidebar`). The live pane
   inherits the destination slot's layout cell. Existing slot geometry is
   authoritative: it is never resized back to `@sidebar_width` on window
   switches. This is deliberate (decision record
   `docs/decisions/2026-09-01-singleton-sidebar-pane.md`, commit `cc7a31e`,
   which removed a `resize_sidebar` call from `follow` because it overwrote
   user-adjusted slots).
5. **Hooks today.** `agent-sidebar.conf:102` follows on window switches,
   `:105`/`:106` on client attach/detach, and `:107` on client resize:

   ```tmux
   set-hook -ga client-resized 'run-shell "\"#{@agent_sidebar_bin}\" follow \"#{hook_client}\" \"#{window_id}\""'
   ```

   `follow` (`src/cli/toggle.rs:326`) moves/reconciles the singleton but
   intentionally does not touch widths. `client-resized` is synchronous
   (`run-shell` without `-b`); attach/detach and session-change hooks are
   asynchronous.

So the configured width is only an *initial* width today. Once a slot exists,
tmux owns the geometry.

## 3. Evidence

All measurements used an isolated tmux server (`TMUX_TMPDIR`), tmux
`3.8-fork+6757fa17f111`. The live tmux server was not touched.

### 3.1 tmux has no fixed pane width

A two-column window 200 columns wide with a 35-column sidebar:

| Window width | Main column | Sidebar |
| -----------: | ----------: | ------: |
|          200 |         164 |      35 |
|          150 |         139 |      10 |
|          210 |         169 |      40 |
|          240 |         184 |      55 |
|          180 |         154 |      25 |
|          120 |         118 |       1 |
|          200 |         158 |      41 |

Two observations matter:

- tmux distributes the window-size delta roughly **equally across columns**,
  not proportionally to column width. A 200→150 shrink removes 25 columns from
  both the 164-column main column and the 35-column sidebar. The narrow
  sidebar therefore absorbs a disproportionate share.
- There is hysteresis. At 120 columns the sidebar is clamped to 1; growing back
  to 200 ends at 158/41 instead of the original 164/35. A single extreme resize
  permanently moves the split until the layout is recreated.

A three-pane probe (main column split vertically into two panes, plus the
sidebar column) shows the same per-column distribution: 200→150 gives
139/139/10, and 240→200 gives 164/164/35.

### 3.2 Native tmux options do not solve it

`main-vertical` with `main-pane-width`/`other-pane-width` applies those sizes
when the layout is selected, but not on later window resizes. Measured: 164/35
after `select-layout`; 139/10 after resizing to 150. It is also not general —
it assumes a two-pane (one main + one other) window shape and would restructure
windows that have additional panes. `window-size manual` stops the window from
following the client, which clips content. There is no tmux feature for a
pinned pane width.

### 3.3 Hook behavior

Probed on the isolated server, including a real pty client attached via a
Python `pty` harness:

- A pty resize (the same signal Ghostty sends) fires `client-resized`; the
  probe observed two invocations per resize. Any handler must be idempotent.
- `after-resize-window` does **not** fire for a client-driven resize.
- `after-resize-pane` fires for explicit `resize-pane` commands, including
  `-Z` zoom/unzoom. Inside the hook, `#{pane_id}` and `#{window_id}` identify
  the *target* pane and window even when the target is in an inactive window.
- `resize-pane` on a zoomed window **unzooms** it. A test hook that resized the
  sidebar during a client resize dropped `window_zoomed_flag` from 1 to 0,
  while the no-hook control kept zoom at 1.
- A client resize while a window is zoomed also distorts the hidden layout.
  Example: zoom the main pane at 200 columns (35-column sidebar), resize the
  client to 210, unzoom → sidebar width 66. Skipping repair while zoomed
  therefore leaves a wrong width until a later resize.

### 3.4 A repair hook works for the common case

On the isolated server, a `client-resized` hook that ran
`resize-pane -t <sidebar> -x 35` kept the sidebar at 35 through the sequence
200→150→210→120→200. Narrow windows clamp the sidebar (at 30 columns the
sidebar is 28 and the main pane 1), and growing back restores 35. There was no
feedback loop: resizing a pane does not fire `client-resized`.

## 4. Constraints From Existing Decisions

- One live sidebar pane exists per tmux server; windows get lazily created
  empty slots and the live pane is swapped between them
  (`docs/decisions/2026-09-01-singleton-sidebar-pane.md`).
- "Existing slot geometry is authoritative: tmux resize behavior and user
  layout changes are preserved rather than repaired back to configured
  defaults." The same record says "Resize and layout hooks never fan out
  across every window."
- Commit `cc7a31e` removed width repair from `follow` specifically because it
  clobbered user-adjusted widths after window switches.
- Fork strategy (`docs/maintainers/fork-strategy.md`): preserve upstream
  structure, keep fork behavior explicit and locally identifiable, prefer the
  smallest seam, add regression coverage proportional to the change.
- Unit tests must not touch the live tmux server; real-tmux tests use an
  isolated server or `TMUX_TMPDIR`.

Any accepted design must either respect the existing decision record or
explicitly supersede the parts it changes.

## 5. Candidate Designs

### A. Config-authoritative repair on client resize (current maintainer lean)

The width contract is the option. A terminal resize is the only event that
re-applies it.

- Extend the existing `client-resized` hook to
  `follow --fit-width "#{hook_client}" "#{window_id}"`. No new hook is
  registered; the existing event path gains one flag.
- After `follow_sidebar` completes, repair validated `sidebar` and
  `sidebar-slot` panes in the resized client's session:
  `resize-pane -t <pane> -x <resolve_sidebar_width(window)>`, skipping panes
  already at the target width.
- Skip windows with `window_zoomed_flag=1`, because resizing would force an
  unzoom. If drift happened while zoomed, the next client resize or an explicit
  toggle repairs it.
- Do not repair on `after-select-window` or on any manual resize path, so
  `cc7a31e` behavior is preserved. Manual width changes remain local to the
  current window's slot and survive normal navigation; they are reset by the
  next terminal resize.

Semantics the maintainer has accepted: **fixed configured width; manual pane
resizing is temporary until the next terminal resize.** To change the width
permanently, change `@sidebar_width`.

Advantages: one flag, one function, no new option, no hidden state, no new
hook registration; repairs exactly the event that causes drift; easy to unit
test. Disadvantages: manual width does not survive a terminal resize; a zoomed
window defers repair.

### B. Intent capture with a per-window override

Treat a manual resize as explicit intent, like dragging a sidebar in an editor.

- New `after-resize-pane` hook runs a `note-width` command with the target
  window. When the window is not zoomed and contains a validated live sidebar,
  compare the sidebar's width with the effective target. If different, store a
  window-level `@sidebar_width_override`; if it matches the configured width,
  clear the override.
- `client-resized` repair prefers the override, falling back to
  `@sidebar_width`.
- The repair itself fires `after-resize-pane`, but it leaves the pane at the
  effective target, so `note-width` is a no-op. No feedback loop.
- Zoom requires a `@sidebar_width_stale` marker: a client resize of a zoomed
  window sets it and skips; the unzoom hook sees the marker, repairs instead
  of recording, then clears it. Without this, the hidden-layout distortion is
  misread as a manual resize.
- Reset: recreate the slot (toggle off/on) or unset the option.

Advantages: respects manual intent; matches the existing "slot geometry is
authoritative" decision. Disadvantages: a second hook, two internal options,
more state and tests; a manual width beats later `@sidebar_width` changes until
reset; percentage settings degrade to absolute columns on a manual resize.

### Rejected alternatives

- **Config-only tmux hook** (a shell loop in `agent-sidebar.conf`): cannot
  reuse the strict slot validation, percentage resolution, and lifecycle lock;
  quoting is fragile; zoom handling is worse.
- **`window-size manual`**: the window stops following the client; content is
  clipped or scrolled out of view.
- **`main-vertical` + `other-pane-width`**: only applies at layout selection,
  only for the two-pane shape, and restructures windows with more panes.
- **Sidebar process self-resizes on SIGWINCH**: moves pane-geometry ownership
  into the TUI process, complicates test isolation and tmux-socket access, and
  duplicates lifecycle logic that already has a lock.
- **Patch the tmux fork**: tmux has no pinned-width concept; this would be a
  large C change and a permanent upstream divergence.
- **Do nothing**: the current recovery is toggling twice, which recreates the
  slot and restarts the sidebar process. Functional but poor.

## 6. Evaluation Criteria

Any recommended design should be judged on:

1. Predictability: is there one stated source of truth for width?
2. Least surprise: never force an unzoom, never touch panes the plugin does
   not own, never repair on unrelated navigation.
3. Scope: only owned panes (`sidebar`, `sidebar-slot`), only the session whose
   client resized, idempotent on duplicate hook invocations.
4. Mergeability: a small, locally identifiable fork delta in
   `agent-sidebar.conf` and `cli/toggle*`, with no new upstream module.
5. Failure handling: best-effort per pane; a vanished pane or a failed
   `resize-pane` must not fail the whole follow path.
6. Performance: the existing `client-resized` hook already spawns the binary
   synchronously; the repair should add only `resize-pane` calls for owned
   panes whose width differs, typically one to three windows.
7. Testability: unit coverage with the `FixtureTmux` client and an
   isolated-tmux lifecycle regression; no live-server dependency.
8. State count: each new option or marker is permanent maintenance surface and
   needs a reset story.

## 7. Open Questions for the Reviewer

1. Is manual pane resizing a supported contract, or is `@sidebar_width` the
   only width contract? The maintainer leans config-only (Option A).
2. Repair scope: all owned panes in the resized client's session, or only the
   active window? All its windows resize together, so session-wide repair
   avoids stale widths when switching windows, but it relaxes the old "never
   fan out" invariant.
3. Zoom: skip and accept deferred repair, add the stale-marker machinery, or
   unzoom/repair/rezoom? (The last was unreliable in a probe because the
   zoomed pane is not necessarily the active pane, so re-zooming the right
   pane needs additional identification.)
4. Multiple clients: `follow` currently no-ops unless exactly one client is
   attached. Should width repair also no-op, or always enforce the configured
   width? The width is absolute, so enforcing seems harmless, but it changes
   shared-window layout for other clients.
5. Should repair also run on `client-attached`? In the pty probe, attach
   produced a `client-resized` event, so a separate path may be redundant.
6. Should `select-layout` and other user layout changes be exempt? Option A
   leaves them alone until the next terminal resize. Is that the right
   boundary?
7. Should this behavior be opt-in behind `@sidebar_keep_width`? The maintainer
   leans no: a fixed-width sidebar is the product contract, and adding a knob
   adds surface without a second real use case.
8. If Option A is accepted, how should the 2026-09-01 decision record be
   amended? Suggested carve-out: existing slot geometry remains authoritative
   for navigation and manual changes; only a `client-resized` event
   re-applies the configured width to owned panes in that client's session.

## 8. Proposed Implementation Sketch (Option A, if accepted)

`agent-sidebar.conf:107`:

```tmux
set-hook -ga client-resized 'run-shell "\"#{@agent_sidebar_bin}\" follow --fit-width \"#{hook_client}\" \"#{window_id}\""'
```

`src/cli/toggle.rs` — parse the flag in the existing `follow_command`, run the
existing follow first, then the repair:

```rust
fn follow_command(args: &[String]) -> Result<(), String> {
    let _lock = acquire_lifecycle_lock()?;
    let mut fit_width = false;
    let mut positional = Vec::new();
    for arg in args {
        if arg == "--fit-width" {
            fit_width = true;
        } else {
            positional.push(arg.as_str());
        }
    }
    follow_sidebar(
        &LiveTmux,
        positional.first().copied().unwrap_or_default(),
        positional.get(1).copied().unwrap_or_default(),
    )?;
    if fit_width {
        topology::fit_sidebar_widths(
            &LiveTmux,
            positional.get(1).copied().unwrap_or_default(),
        )?;
    }
    Ok(())
}
```

`src/cli/toggle/topology.rs` — extend `PaneRecord`/`Inventory` with
`session_id`, `pane_width`, and `window_zoomed_flag`, then:

```rust
pub(super) fn fit_sidebar_widths(
    client: &impl TmuxClient,
    window_id: &str,
) -> Result<(), String> {
    if window_id.is_empty() {
        return Ok(());
    }
    let session_id = client.display(window_id, "#{session_id}");
    if session_id.is_empty() {
        return Ok(());
    }
    let inventory = Inventory::query(client)?;
    let mut widths = HashMap::new();
    for pane in inventory
        .panes
        .iter()
        .filter(|pane| pane.session_id == session_id)
        .filter(|pane| pane.is_sidebar() || pane.is_empty_slot())
    {
        if pane.zoomed {
            continue;
        }
        let width = widths
            .entry(pane.window_id.clone())
            .or_insert_with(|| resolve_sidebar_width(client, &pane.window_id))
            .clone();
        if pane.width.to_string() == width {
            continue;
        }
        // Best effort: the pane may disappear while iterating.
        let _ = client.run(&[
            "resize-pane",
            "-t",
            &pane.pane_id,
            "-x",
            &width,
        ]);
    }
    Ok(())
}
```

Tests to add:

- Unit (`FixtureTmux`): repairs a live sidebar and an empty slot; ignores a
  foreign pane, a pane in another session, and a zoomed window; skips panes
  already at the target width; resolves a percentage against the window width.
- Isolated tmux: create a 200-column window with a 35-column slot,
  `resize-window -x 150`, invoke the fit path, assert the sidebar returns to
  35; zoomed window remains zoomed and untouched.
- Existing `follow_preserves_width_when_sidebar_is_already_visible` stays as
  the regression guard for `cc7a31e` (plain follow must not resize).

Docs to update if implemented: the new decision record, the 2026-09-01
decision record carve-out, `docs/maintainers/architecture-map.md` invariants,
and the `@sidebar_width` row in
`website/src/content/docs/reference/tmux-options.md`.

Verification: `./scripts/verify.sh full` and `./scripts/fork-delta.sh` from the
primary checkout (not a linked worktree).

## 9. Reproduction Commands

Layout drift without a client (isolated server):

```sh
export TMUX_TMPDIR=$(mktemp -d)
tmux -L probe new-session -d -x 200 -y 50 -s t
tmux -L probe split-window -h -d -l 35 -t t
for w in 150 240 120 200; do
  tmux -L probe resize-window -t t -x "$w" -y 50
  printf '%s: ' "$w"
  tmux -L probe list-panes -t t -F '#{pane_width}' | paste -sd' ' -
done
tmux -L probe kill-server
```

A real pty client is needed to observe `client-resized`; use a Python `pty`
harness with `fcntl.ioctl(fd, termios.TIOCSWINSZ, ...)` and `SIGWINCH`, attach
`tmux -L probe attach -t t` in the child, and log hook firings to a file.
