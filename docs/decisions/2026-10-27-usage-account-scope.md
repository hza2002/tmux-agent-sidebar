# Decision: Account-scoped usage rows and a compact metering layout

Account identity selection and attribution are superseded by the
[runtime attribution decision](2026-10-28-codex-runtime-attribution.md).
The row layout and interactions below remain applicable.

## Context

Two Codex subscriptions (Plus and Pro) can share one `~/.codex` session tree.
Codex writes `session_meta.creator_account_id` into every rollout, and the
existing history store already persists it as `events.record.codex.account`.
The Usage view, however, mixed scopes: the `cx本期` window followed the
signed-in account's weekly reset while its numbers summed every local account,
and the rolling rows summed every account. Switching login moved the window but
not the numerator, so the cycle number could not be explained.

The signed-in account is available without another request:
`~/.codex/auth.json` holds `tokens.account_id`, which is the
`chatgpt_account_id` claim of the ID token. Token refresh rotates the tokens but
not that id, and the quota client already refuses API-key mode.

Separately, the `cx本期` details modal was rarely discovered or used. It
duplicated the row's window and total, buried a per-model table under static
disclaimers, and its only trigger (`cx本期` label) conflicts with the new
window-switching interaction. The responsive row layout also spent spare width
on token precision and elastic gaps, which made columns move as numbers grew.

## Chosen Seam

Keep the existing worker -> `Update` mailbox -> `UsageState` -> quota leaf
renderer flow. Add a second aggregated `metering::Snapshot` in
`usage::history::snapshot`: one for all local records and one for records whose
persisted `codex.account` equals the current `auth.json` account id. Both come
from the same database pass and timestamp; no second scan, worker, or schema
migration.

Present two Codex scope rows from that pair:

- `cx账号` (top) counts only the signed-in account. Its window sequence is
  `本期` then the rolling windows; `本期` is valid only here.
- `cx全部` (second) counts every local record and has rolling windows only.
- The DeepSeek row follows and keeps its own window.

The account row owns `本期` because a weekly reset boundary belongs to one
account; there is no account-independent cycle. Records without
`creator_account_id` remain in the all-accounts row and are excluded from the
account row. `usage-history status` gains per-account event counts and the
unattributed count so the exclusion is inspectable.

Interaction:

- Click a window label to advance that row; click an amount or token to toggle
  the row between totals and daily rates.
- `t` advances the shared window for every usage row; mixed selections reset
  to `本期` for the account row and today for the other rows.
- `a` toggles daily mode for every row that supports it.
- The header keeps a static `Usage` title joined by a dim leader line to a
  `󰬁当天`/`󰬁24h`/`󰬁7d`/`󰬁30d` interval control; the glyph and window label share
  the window color and cycle every usage row. Mixed selections show `󰬁混选`.
  `󰫮日均` toggles daily mode for every eligible row and brightens while active.
  The title itself is not clickable. Compact mode has no header and therefore
  no daily indicator.

Layout:

- Revert the post-`f788e99` precision/elastic layout to fixed field budgets:
  name 6, token 6, cache 6, tail 4, amount 4-8 (9 only for daily rates that
  exceed the normal range). Amounts omit thousands separators on rows;
  `usage-history` output keeps grouping.
- The tail is a right-aligned 4-cell slot: `账号` / `全部` on Codex rows and
  `高峰` / `低谷` on the DeepSeek row. The peak state moves off the header.
- Daily mode is row-level and available on `本期`, `7d`, and `30d` only.
  `当天` is partial and `24h` equals the total, so both stay total-only. Rows
  add no marker; the header uses Material Design `alpha_t` / `alpha_a` glyphs
  for the interval and `日均` controls. `日均` brightens while any eligible row,
  including a height-hidden scope, is in daily mode and is hidden when no row
  supports it. Numbers carry no `均`
  suffix.
- Explicit width fallbacks: warning `!`, then cache, then shortened window
  labels. The scope tail is never dropped.
- Remove the `cx本期` details modal and its snapshots. Health details remain in
  the notices popup; per-model breakdown is deferred to a CLI report if it is
  ever needed.

## Alternatives Rejected

- **Filter at scan time:** would remove the all-accounts row and prevent the
  two views from sharing one pass.
- **Separate `CODEX_HOME` per subscription:** separates the session tree, which
  breaks `codex resume` continuity, and the quota client ignores `CODEX_HOME`
  anyway.
- **Second worker or database column:** both scopes are derivable from one
  query and the already-persisted account field.
- **Keep the modal:** low value, high interaction cost, and it conflicts with
  window switching on the label. Per-model data is not used daily.
- **Keep dynamic precision:** columns would move on every value change; fixed
  budgets preserve cross-row alignment.

## Upstream Compatibility

The worker/state/renderer direction is unchanged. `usage/` is already
fork-owned, and the quota subscription rows keep their upstream structure and
seam. The visible changes are confined to the quota leaf renderer, the
fork-owned usage state, and the usage-history CLI status. No entry point,
event normalization contract, or tmux boundary changes.

## Conflict Strategy

On upstream merges, take upstream's `ui/panes/quota.rs` structure first, then
reapply this decision at the usage-row constructor and `layout_usage_rows`.
Keep upstream-only quota fields and subscription rendering intact. Preserve
the fork-owned `usage/` modules wholesale unless upstream introduces a
conflicting state shape; in that case port the account snapshot into the new
shape rather than reintroducing the old `Update`.

## Removal Path

Delete the account snapshot and the `账号` tail to return to a single
all-accounts Codex row; restore the prior window selection and header peak
status. The fixed layout can be replaced by the newer upstream responsive pass
without touching the data model. `usage-history status` account counts are
additive and safe to keep.
