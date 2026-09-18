# Decision: two stacked band blocks with row-level compression

Status: approved; implementation in progress (phase 2 lands on top of
`feat(sidebar): quota correctness, idle-row tab band, opt-in pet`).

## Context

The tab band (`@sidebar_band`) currently hosts **one** of the bottom tabs in
the agents panel's idle rows. Two problems surfaced in daily use:

1. In this fork's layout the band always resolves to the Git tab: the
   auto-switch picks Activity only when the focused pane is an agent pane in
   the sidebar's own window, and the sidebar window has no agent panes. Activity
   is effectively unreachable, and switching needs the mouse.
2. The band is all-or-nothing (6 rows, then hidden). With a tall sidebar and a
   handful of agents there is usually far more room than one tab can use, and
   the Git block wastes rows on structural blank lines.

## Chosen approach

Render **both** tabs as stacked blocks in the same idle area — Activity on top,
Git below — and let the space decide how much of each survives.

- Allocation, top-down: the quota block (pinned, unchanged), then the agent
  list, then Activity, then Git. The band is funded only by rows the list does
  **not** need, so the list never scrolls because of the band.
  *(Reversed on 2026-09-18: funding the band from the rows of "silent" panes was
  tried in use and rejected — it shrank the list's viewport, so the list could
  never be read to the end and keeping the selected row visible pushed the group
  headers off the top of the panel.)*
- Compression is **row-level, never mode-level**: a block keeps whatever rows
  fit. Git's body scrolls/clips below its fixed header (the `+N more` count
  already rides on the section header), Activity drops whole entries from the
  oldest end. There is no full/compact switch and no merged summary line.
- **Separator rule**: a separator or blank row belongs to the content it
  introduces. It may only disappear together with that content — never
  "separator kept with nothing under it", never "content kept without its
  separator". Today `bottom/activity.rs` violates this with an unconditional
  leading blank row; the band's line builders must enforce the rule and every
  affected snapshot is regenerated.
- Keyboard: `Left`/`Right` (and `h`/`l`, matching the existing keymap) move the
  focus between the two blocks while the band owns the keyboard
  (`Focus::ActivityLog`); `j`/`k`, `Ctrl-D`/`Ctrl-U`, `g`/`G` scroll the focused
  block. With only one block on screen the same keys switch which one is shown.

## Rejected

- **A dedicated key (`T`, then `w`).** Both are free, but `T` reads as "tab"
  while the tab concept is being removed, and the fork's keymap already binds
  direction keys for movement. `Left`/`Right` cost no new mnemonic and are
  inert outside `Focus::Filter` today (verified in `src/app/input.rs`).
- **A merged one-line summary** when both blocks do not fit. Its semantics are
  undefined when one side is empty, it invents a third rendering mode, and it
  destroys the PR hyperlink's only anchor.
- **A separate band-focus state.** `state.bottom_tab` already means "the tab
  that owns the keyboard and scroll"; reusing it keeps four routing call sites
  (`scroll_bottom`, boundary jumps, half-page, at-top tests) unchanged.
- **Generalizing `draw_bottom`.** The bottom panel keeps its own renderer and
  signature, so its 34 snapshots and the `src/ui/mod.rs` skeleton touchpoint
  stay untouched; the band gets its own leaf renderer with a row budget.

## Upstream compatibility

Fork-only UI. No change to the query layer, adapters, hook handlers, or CLI
entry points. `src/ui/mod.rs` stays as-is; the new code lives in
`src/ui/panes.rs`, `src/ui/bottom.rs` (new band leaf) and the two tab modules.
Removal deletes the band leaf, the two-block allocation, `band_blocks`, and the
`@sidebar_band` option, restoring the single-tab band.

## Verification

- Allocation property test: for a matrix of panel heights, protected rows,
  quota rows and block wishes, `quota + blocks + pet <= spare` and therefore
  `scroll_visible_height() >= protected`.
- Threshold snapshots: spare = 0/1/2, both blocks fit, Git with `+N more`,
  Activity with entries dropped, band off, bottom panel visible.
- Routing tests: a click on a block's title selects that block; a click on its
  body focuses it; `Left`/`Right` move the focus.
- Capture fixtures `website/src/assets/captures/{activity,git}-focus.png` are
  band crops and must be regenerated through the capture workflow.
