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
