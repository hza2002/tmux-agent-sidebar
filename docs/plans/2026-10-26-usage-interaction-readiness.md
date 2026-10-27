# Usage Interaction Implementation Readiness

Status: UI-only follow-up implemented and verified on 2026-10-26. Storage remains under independent review.
Date: 2026-10-26.

This plan reconciles the current uncommitted Usage implementation with subsequent
product discussion. It can ship using the existing in-memory worker and does
not depend on selecting a persistence or evidence-archival design. See the
[storage reference](2026-10-26-usage-history-storage-reference.md) for that review.

## Pre-follow-up baseline and accepted changes

| Surface | Pre-follow-up behavior | Accepted contract |
| --- | --- | --- |
| Windows | Both providers support today/24h/7d/30d; matching suffixes | Keep |
| Defaults | Both use explicitly labeled local-calendar today | Keep |
| Selection | Provider rows switch independently; Usage title switches both; plain `u` is the keyboard equivalent | Keep the actions, restrict mouse hit regions |
| Mixed windows | Title omits a window suffix; title action realigns to today | Keep |
| Cached switching | Worker precomputes windows; selection does not scan | Keep |
| Mouse targeting | Usage block actions use row numbers, including the entire title row | Use rendered column bounds; status and gaps consume clicks without changing selection or triggering refresh |
| Monetary display | Total amount, with a separate daily-average suffix if space permits | Click amount to switch total/average in the same field |
| Token display | Core total/cache plus optional output and input suffixes | Always retain core total tokens; remove output/input suffixes |
| Current DS period | Header-right `ds高峰`/`ds低谷`; compact fallback on DS row | Add a Nerd Font icon and retain the independent current-period meaning |
| Quota reset text | Brighter live reset countdown; subdued stale/error metadata | Keep; preserve concurrent quota recovery changes |
| Previous-window change | Not implemented; 31-day normalized retention | Defer to a separately approved data-coverage change |

These changes are uncommitted and are not a released contract. Other local
changes, including quota error/recovery work and unrelated untracked plans,
must be preserved. Recheck `git status` and the diff before implementation.

## Accepted interaction contract

1. Clicking only the rendered left title label (`Usage` plus a shared-window
   suffix) switches all recent usage windows. The icon can share that left hit
   target. The right DS status and intervening whitespace have no action.
2. Clicking the rendered provider/window label (`ds7d`, `cx24h`, etc.) switches
   only that provider's time window. Blank padding is not part of the label.
3. Clicking a priced amount in a multi-day or Codex cycle row toggles that row's
   amount between total and daily average. Default is total. Display modes are independent for DS, Codex recent, and
   Codex cycle, as authorized for implementation.
4. Total mode uses the existing currency amount. Average mode adds `/d` in the
   same column so its meaning survives narrow layouts. Changing windows
   preserves a row's amount preference; today/24h render total without erasing
   the preference. Unpriced or unavailable amounts do not offer the toggle.
5. Total tokens and cache-hit rate are informational. They do not switch a
   window or monetary mode. Remove the extra `出…` / `入…` suffixes.
6. The Codex current-cycle label opens existing details because its quota cycle
   is a different concept from a rolling window. Its amount can toggle only
   when the valid cycle spans more than one elapsed day. Details retain their
   existing keyboard navigation and total-amount meaning.
7. Plain `u` keeps the existing all-window action, including title-less compact
   mode. Existing text input and `Ctrl+u` navigation retain their behavior.
8. Existing subscription rows and a quota-only title keep quota refresh behavior.
   Clicking informational Usage fields must not fall through into that action
   or activate an agent pane behind the footer.

Daily averages use the same window duration rules as the current implementation:
7/30 days for rolling windows and elapsed cycle days rounded up, minimum one.
Today and elapsed 24h do not expose a redundant average toggle. Amount colors
continue using the existing utilization/spend rules, independent of display mode.

## DS period icons and responsive layout

The proposed icons are `nf-fa-fire` (`U+F06D`) for peak and `nf-fa-leaf`
(`U+F06C`) for off-peak, verified against the
[official Nerd Fonts glyph map](https://github.com/ryanoasis/nerd-fonts/blob/master/glyphnames.json).
Use existing bright-yellow peak and bright-aqua off-peak theme colors. High/low
is current pricing state, not the selected historical window's pricing or a
claim about wall-clock day/night.

Prefer `ds` + icon + Chinese label with no inserted spaces when width permits;
shrink the right status to `ds` + icon before sacrificing core row data.
Title-less compact views keep an icon/text status on the DS row. Use actual
terminal display widths rather than byte or character counts, preserve complete
glyphs, and never put a window/amount click target on the status.

Total tokens outrank averages, changes, and extra token breakdown. The first
implementation adds no comparison field. Later comparisons can occupy spare
columns if complete; they must yield before core totals and must not displace
tokens merely to show both a total amount and a separate average amount.

## Implementation seam and scope

- `src/usage.rs`: ephemeral row-specific amount preferences and selection methods;
  no database or source-policy change.
- `src/ui/panes/quota.rs`: format the selected monetary mode, remove redundant
  suffixes, render the DS icon, and produce typed rectangular click targets from
  the final width-adapted spans. Existing `QuotaLine` spans provide the seam.
- `src/state/layout.rs`: store ephemeral targets and dispatch by row and column.
  Replace row-wide Usage actions without changing subscription refresh behavior.
- `src/ui/panes.rs`: propagate and clear rendered targets each frame, including
  when hidden or collapsed. Do not preserve stale clickable rectangles.
- `src/app/input.rs`: pass through coordinates via the existing mouse path;
  retain plain `u` and popup/text-input behavior.
- Tests, `examples/usage_preview.rs`, focused scenario fixtures, and usage/key
  documentation move with the behavior. Update the existing Usage decision record.

No new commands, setup steps, hooks, dependencies, disk data, or threads are
needed. The scope touches more than eight files including tests and docs, but
keeps the current data-flow direction and module ownership. Before coding,
record any new integration/hotspot impact required by the fork strategy in the
existing decision record.

## Acceptance checks and handoff

- Click first/middle/last cells of each active label/amount; test adjacent gaps,
  token/cache fields, DS status, and hidden/collapsed targets as no-ops.
- Switch each provider independently, realign through title/`u`, toggle amounts
  per the confirmed scope, and preserve behavior through background updates.
- Cover today/24h/7d/30d, valid/expired/missing cycles, priced/unpriced/pending
  values, larger amounts, and current peak/off-peak transitions.
- Render complete inline snapshots at 28, 35, 45 and 60 columns, plus compact
  height pressure. Assert styles for both icons and reset countdowns.
- Regenerate snapshots through their owning tests; do not edit generated text
  with replacement scripts. Capture and inspect the focused isolated tmux
  fixture using the repository showcase skills.
- Run `cargo fmt`, `./scripts/verify.sh full` and `./scripts/fork-delta.sh`.
  The full verifier includes a release build and isolated tmux tests.

Keep this change independently usable before any storage work. Live sidebar restarts, commits, tags, and pushes require separate maintainer
instructions. Reverting the UI follow-up requires
no stored-data migration. Report verification results and the need to restart
the sidebar to use the rebuilt Rust binary; hook/plugin updates are unnecessary.

## Deferred comparison semantics

A future approved comparison change should compare today with yesterday through
the same local clock time, rolling windows with adjacent equal-duration windows,
and a cycle with the previous cycle's matching progress. DST and unavailable
quota boundaries require explicit tests. Zero baselines, unknown prices,
incomplete coverage, and source loss require distinct outcomes; a percentage
must not imply coverage that the collector cannot establish.

The current 31-day cache does not support a complete previous 30-day window.
Neither a 90-day cache nor any archive has been implemented. Source retention
and completeness policy remain with the architecture review, so comparison work
must not silently select one of those storage contracts.

## Verification result

The UI follow-up passed `./scripts/verify.sh full` (format, strict Clippy,
unit/integration tests, isolated real-tmux lifecycle and Codex-routing tests,
and release build). Inline snapshots cover monetary modes and supported widths;
metadata and state tests cover field boundaries, no-op regions, ineligible
amounts, independent preferences, and stale-target cleanup. The focused fixture
was rendered and inspected for total, average, compact, peak/off-peak, narrow,
wide, details, and quota-error states. Its HTML/PNG exporter has the existing
RGB/font/wide-cell limitations described in the usage guide; ANSI captures and
Ratatui/style assertions remain the alignment and color evidence.

No persistence, archival, retention expansion, or previous-window comparison was
implemented. No live sidebar was restarted. Source commits are handled separately when
requested by the maintainer.


## Follow-up: direct token and amount toggles

The subsequently approved interaction extends the existing amount control to
independent token controls on each row. Both default to total; active daily
fields show `日均` without a slash or a separate button. Today/24h retain total
presentation and saved preferences. An available Codex cycle is eligible even
within its first day; its existing ceil(elapsed days), minimum-one divisor is
unchanged, so the first day's values equal their totals. Missing prices disable
only the monetary toggle. The current inferred weekly-cycle interval is retained;
previous-period comparisons and additional spare-space content remain out of scope.

Coverage includes full-frame click/redraw snapshots, independent preferences
across refresh/window changes, missing prices, stale cycle boundaries and narrow
layouts. The isolated usage fixture accepts actual SGR mouse input for the
first-day cycle and captures amount-only and both-daily outcomes.


Follow-up verification passed: 48 focused Usage tests, full verification
(1,633 library tests plus integration and isolated tmux/daemon suites), release
build, and isolated SGR mouse clicks through amount-only, both-daily and restored
total states. The first-day regression failed with the old eligibility guard in
an isolated source copy, then passed with the fix. Updated inline snapshots were
regenerated through cargo-insta. Narrow, compact and mixed-mode captures were
inspected against terminal text; existing exporter RGB/font limitations remain.
No live sidebar was restarted and no commit was created.


## Correction after live interaction feedback

The first follow-up still disabled today's controls and rounded a partial cycle
up to one day. Live inspection found DS on today and Codex within its first day,
explaining both reported symptoms. This supersedes the previous eligibility and
ceil-day rules: every positive-duration window supports direct amount/token
switching, and daily rates use the snapshot's actual elapsed seconds. Today
starts at local midnight; cycle starts retain the existing inferred boundary.
Rendering stale snapshots must not advance the denominator. The compact `ø`
prefix replaces `日均` and `/d`; it means normalized daily rate, not a forecast.

A regression reproduces noon DS and an eight-hour Codex cycle, verifies changed
rates, stale-snapshot stability, 24h equality and zero-duration rejection. The
isolated mouse fixture now verifies DS today as well as the first cycle day,
including both fields and switching back to totals.


Correction verification passed: the new regression failed on the original
Today eligibility rule, then passed with elapsed-time rates. All 49 focused
Usage tests and the full verifier passed (1,634 library tests plus integration,
isolated tmux/daemon checks and release build). The isolated SGR fixture verified
DS today and Codex's eight-hour cycle numerically, both fields independently,
and restoration to totals. Narrow and compact captures were inspected; generated
website assets and the live sidebar were not modified.


## Accepted layout and cycle refinement (2026-10-27)

Cycle amounts and tokens now remain totals; only its label opens details.
Recent-window amount/token toggles retain independent preferences and elapsed-time
rates, marked with the `均` suffix. The Usage renderer measures visible fields
and aligns rows through one shared layout pass. Remaining width is elastic;
cache ratios anchor the right edge, including compact DS rows with period
metadata before cache. Shrinking consumes whitespace before shortening labels
and omitting cache. Final spans remain the only source of mouse hit rectangles.
No storage, pricing, quota polling, configuration or comparison feature changes.


Refinement verification passed: visible-column alignment and right-edge cache
checks at 28/35/45/60 columns, hidden-row independence, full-frame click/redraw
snapshots, and the isolated SGR mouse scenario across 35 -> 45 -> 28 columns.
The scenario confirms inert cycle numbers, independent recent amount/token
switches and restoration after resize. Full verification passed (1,635 library
tests plus integration and isolated tmux/daemon suites, strict Clippy, formatting
and release build). Updated snapshots were regenerated through their owning
tests. Captures were checked against terminal text because the existing HTML
exporter has RGB/font/CJK-width limitations. No live sidebar restart or commit.


## Empty-value alignment (2026-10-27)

Pending amount/token/cache fields keep distinct semantic span kinds and join the
same layout pass without gaining click actions. Token numeric, unit and average
marker positions align separately. Cache ratios reserve the percentage slot
through 100% and unknown `--`; trailing warning space is shared. Unknown tokens
remain `--`, known zero remains `0`, and missing units stay blank. An unavailable
cycle retains a `待刷新` monetary placeholder alongside token/cache placeholders.
The regression failed with only two participating rows before the fix and now
covers mixed known/pending/zero values, 99%/100%/unknown ratios, warnings and an
expired cycle across narrow and regular widths.

Verification passed: 51 focused Usage tests, 89 UI integration tests, and the
full verifier (1,636 library tests, integration and isolated tmux/daemon suites,
formatting, strict Clippy and release build). The isolated capture scenario
passed real mouse clicks and resizing; pending, zero and unavailable-cycle
frames were checked against terminal output and inline snapshots. The fork
delta was inspected. No live sidebar restart or commit.

## Responsive token precision (2026-10-27)

The Usage leaf renderer spends spare width on two or three token decimal
places before distributing whitespace. Candidates use original counts and the
existing daily denominator, omit extra trailing zeros, preserve labels and
cache, and require two-cell numeric field gaps. Currency retains two decimals.
Narrow views keep compact precision; pending and zero fields keep their slots.

Verification passed: 52 focused Usage tests, full verifier including isolated
tmux/daemon tests, formatting, strict Clippy and release build. Inline snapshots
were regenerated through their owning tests. The isolated capture scenario
passed real clicks and resizing; captures and terminal text were inspected.
The fork delta was inspected. No live sidebar restart or commit.

## Full token counts when space permits (2026-10-27)

The final responsive token format uses integer counts with thousands separators.
All visible Usage rows share the format choice; candidates retain labels, cache
and minimum spacing through the existing layout guard. Counts and daily rates
use the original integer values, including values beyond floating-point exact
integer range. Currency precision and click behavior are unchanged.

Coverage includes 28/31/32/35/45/60-column snapshots, grouped integer boundaries
through u64::MAX, pending and zero fields, cache alignment and resize/click
targets. The isolated mouse fixture locates either compact or full token fields.

Verification passed: 52 focused Usage tests, 89 UI integration tests and the
full verifier (formatting, strict Clippy, all tests, isolated tmux/daemon suites
and release build). Owning tests regenerated inline snapshots. Isolated real
mouse/resize captures passed and were inspected against terminal output; the
fork delta was inspected. No live sidebar restart or commit.
