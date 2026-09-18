# Decision: Activity pane rendering, highlighting, and copy

## Context

The tab band's Activity block rendered every entry the same way: a
timestamp/tool row with the tool right-aligned, plus the label wrapped across
up to three rows. Three problems followed.

- A long log spent two to four rows per entry, so the block pushed Git out of
  the shared rows even when the reader was not looking at Activity.
- Labels were painted in one muted color, so a shell command was
  indistinguishable from a basename or a paragraph.
- `y` copied "the newest entry" because the block had no cursor. The reader
  could see ten commands and copy only the first one.

The user asked for a faithful highlighter rather than a guess at shell syntax,
for the highlighted entry to be the entry `y` copies, and for the footer to be
one key press away from the agent list.

## Chosen Seam

- **Real grammar, not a heuristic.** Highlighting is driven by
  `tree-sitter-bash`'s own `HIGHLIGHT_QUERY` through `tree-sitter-highlight`
  (both MIT). `src/ui/bottom/syntax.rs` only maps the grammar's highlight names
  onto existing theme slots (command word `text_active` + bold, options
  `activity_interaction`, strings `activity_edit`, operators/keywords/comments
  `text_inactive`, numbers/properties `activity_read`); everything the grammar
  leaves unlabelled — a bare word, a glob, a path — stays on the body color.
  The configuration is compiled once per process on first use, and any parse or
  encoding failure falls back to the plain label.
- **Measured cost** (release, `lto = true`, `strip = true`, this machine):
  binary 1.58 MB → 4.29 MB (+2.71 MB); clean build +~30 s; one-time query
  compile ~8 ms; ~12 µs per 62-column command line (≈0.6 ms for a 50-entry log,
  and the render path builds the block twice per frame). Accepted as the price
  of a real parser; the knobs if it ever bites are `opt-level = "z"` and
  skipping the highlight pass for the height-only build.
- **Only command labels reach the parser.** `ActivityEntry::is_command_tool`
  gates it to the tools whose hook label is a command line (Bash, PowerShell,
  Monitor); a basename, a glob, a URL, or a subagent paragraph takes
  `plain_spans` and never touches the grammar.
- **One row per entry; only the cursor entry wraps.** The cursor entry wraps as
  far as its label needs — no fixed three-row cap — and every other entry stays
  on one row. The first cut wrapped every entry once the block owned the
  keyboard, which is the opposite trade: it spent the band's rows on text nobody
  asked to read while still cutting off the long command the reader was actually
  looking at. Wrapping is also not gated on focus: the cursor marks what is
  being read, and the reader is usually in the agent pane, not in the sidebar.
- **No tool column.** A shell label *is* the command, so a `Bash` column only
  took width from the thing being read; the fork had also padded it into a
  fixed column, which left the gap the user complained about. Command rows are
  `HH:MM command…`; a non-command row keeps its tool name because that is the
  only thing separating a filename from a pattern (`HH:MM Edit activity.rs`).
  Gaps are single spaces, and a wrapped entry's continuation rows start flush at
  the gutter: the first row pays for `HH:MM`, the rows below it use the whole
  width, so nothing is spent on a hanging indent.
- **The stored label keeps its pipes.** Found while checking the wrapped output
  live: `write_activity_entry` ran the label through `sanitize_tmux_value`,
  which replaces `|` with a space because tmux's own option/format path needs
  that. The log does not: its readers split on the first two `|` only, so a
  pipeline was being stored, displayed, and copied with its pipes missing —
  `y` handed back a command that no longer ran. Only newlines are replaced now.
- **The guide line is the whole cursor.** `ActivityState.selected` is the entry
  the block marks and the entry `y` copies, and `┃` in the accent color — the
  agent list's own marker — is the only thing that marks it. An earlier cut also
  painted `selection_bg` across the entry's rows; on wrapped, syntax-colored
  text that made the command harder to read rather than easier, and the marker
  already spans every row. `scroll_bottom`'s Activity arm moves the cursor
  instead of a viewport, so `j`/`k`, `Ctrl-D`/`Ctrl-U`, `gg`/`G`, and the wheel
  all move the selection, and the cursor scrolls itself into view each frame
  the way the agent list keeps its selected pane visible.
- **The cursor survives refreshes.** `ActivityState::replace_entries` re-anchors
  it: row `0` means "the newest", so it keeps pointing at the newest command as
  the log grows (the reader watching an agent work wants the new arrival), while
  a cursor further down follows its entry by identity (a new arrival must not
  silently move the copy target onto a different command). An entry that falls
  out of the log clamps the cursor.
- **`←`/`→` enter the footer.** `AppState::focus_footer(tab)` is the click path
  and the key path: from the agent list `←` lands on the Activity block and `→`
  on Git; with the footer focused the same keys switch between the two blocks.
  Previously reaching the footer meant walking `j` to the last pane.
  The band border says both things at once, in three levels: accent for the
  block the keys are driving, `text_muted` for the block they would land on
  while the keyboard is still in the agent list, `border_inactive` for the rest.
  A single "selected" accent used to mark the armed block even when the keyboard
  was elsewhere, which read as "focused, but `j` does nothing".

## Alternatives Rejected

- **`syntect`** (Sublime grammars): +2.2 MB in a bare crate and ~106–130 µs per
  line, nine to ten times tree-sitter's cost. Cost alone would not have settled
  it, so the grammar was asked directly: on this repository's own labels it
  mislabels the text. `cargo test --all-targets` comes back as `" test"` →
  `meta.function-call.arguments`, `" --all-tar"` → `variable.parameter.option`,
  `"gets"` → `punctuation.definition.parameter`, and the command word `cargo`
  never leaves the base `source.shell.bash` scope — the one token worth
  emphasizing in a log of one-line commands would not be emphasized at all.
  `rg -n "setup" src/main.rs` and `git commit -m 'wip'` desync the same way.
  Cheaper per line *and* wrong on the input it would actually see.
- **`yash-syntax`** (real POSIX shell parser, pure Rust): the cheapest build of
  the three, but it is `GPL-3.0-or-later` and this repository is MIT, so taking
  it would relicense the binary.
- **The hand-rolled tokenizer** this replaces: fast and dependency-free, but it
  guessed at shell syntax (flags, paths, redirections) instead of parsing it,
  and it could not tell an option from a word the way the grammar can.
- **Copying the newest entry without a cursor** (the first cut): the user
  reversed it — the entry that is highlighted is the entry that should be
  copied.
- **An index-only cursor**: a new entry at the head would slide the selection
  onto a different command. **Identity-only anchoring**: a reader watching the
  head would fall behind as the log grows. The chosen rule is the hybrid above.
- **A cursor for the Git block too**: the git list is a glance at the working
  tree, not a list anything is copied from; it keeps a plain viewport.
- **Highlighting every label**: Edit logs a basename, Agent logs a paragraph —
  the grammar would misread prose as shell. `ActivityEntry::is_command_tool`
  gates the parser to the tools whose hook label is a command line.

## Upstream Compatibility

- No skeleton touchpoint changes: no entry point, no adapter/event contract, no
  tmux query, no `src/ui/mod.rs` composition, and `draw_bottom` is untouched.
- `Cargo.toml` gains three pinned dependencies (the only new ones in the fork).
- `src/state.rs` gains one field next to the existing `pending_osc52_copy`; the
  behaviors live in the topical `src/state/activity.rs` submodule. `src/app/input.rs`
  gains one match arm and two boundary changes; `src/activity.rs` gains one
  predicate next to `tool_color_class`.

## Conflict and Removal Strategy

- Removal: drop the three dependencies and `src/ui/bottom/syntax.rs`, restore a
  plain `command_spans`, delete `ActivityState::selected` and its methods plus
  the `y`/arrow arms, and revert `scroll_bottom`'s Activity arm to a viewport
  scroll.
- Merge risk is concentrated in `src/ui/bottom/activity.rs` (fork-shaped) and
  the small edits to `input.rs`/`tab.rs`; take upstream's activity renderer and
  reapply the cursor and the two-level branch on top of it.
