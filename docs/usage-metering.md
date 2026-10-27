# Local usage and historical prices

Quota and Usage share one footer and the ` Usage` header, controlled by
`@sidebar_quota`. Compact mode omits the header entirely to preserve data rows.
Quota reports provider remaining percentages and reset countdowns. Usage reads
local session logs in the existing background worker; it is not a vendor bill
or an account-wide usage report.

## Windows and interaction

Codex has a current-cycle row and a recent row. The cycle follows the weekly
quota reset timestamp and the recognized seven-day quota duration. It is not
the subscription payment month or a calendar week. A failed quota refresh
retains the last known cycle boundary and does not dim the locally scanned
cycle totals. Only expired or missing cycle boundaries leave
the cycle unavailable until a successful refresh. A successful response without
a weekly window, or a missing subscription, clears the boundary.

DeepSeek and the Codex recent row default to local-calendar today. Both use the
same suffixes: `ds当天` / `cx当天`, `ds24h` / `cx24h`, `ds7d` / `cx7d`, and
`ds30d` / `cx30d`. Today means local midnight through the scan timestamp;
24h, 7d (168 hours), and 30d (720 hours) are elapsed rolling windows.

Click a provider's window label to advance only that row. When both rows agree,
the header shows `Usage 当天`, `Usage 24h`, `Usage 7d`, or `Usage 30d`; mixed
selections show only `Usage`. Click only the left Usage title/window label to advance both rows
from a shared window, or return both to today when their selections differ.
The plain `u` key performs the same action, including in compact mode without
a header; text inputs retain `u` as text and `Ctrl+u` retains navigation.
The shortcut is inactive when Usage has not loaded or `@sidebar_quota` is off.
A weekly reset does not clear these windows.
All windows for both providers are computed at one timestamp in the existing
background scan. Switching selects cached results immediately, without clearing
numbers or requesting another source scan. Activity-triggered and 30-second
background refreshes publish all windows together.

Click the Codex cycle label to open details. Tab switches details between cycle
and the selected recent window; arrow keys scroll and Escape closes. Details
include per-model input, cache-hit and output counts, UTC interval bounds, and
price-book revision. The quota-cycle row remains independent of the shared selection.

Click an amount or token number on a recent-window row to toggle that field
independently between total and daily rate. A daily value uses the compact `均`
suffix (for example `¥15.60均` or `3.2M均`); there is no separate button or slash
suffix. DS and Codex recent keep independent preferences across refreshes and
window changes. Every available, positive-duration recent window supports the
toggle, including today. Missing prices disable only the monetary toggle.
The Codex cycle always displays totals; its label still opens details, while
its amount and token fields have no toggle action.

Daily rate is observed usage multiplied by 86,400 seconds and divided by the
actual observed interval. Today uses local midnight through the snapshot time;
rolling 24h/7d/30d use their exact durations. A 24h total and its daily rate are
numerically equal. A partial-day rate is an elapsed-time normalization, not a
forecast. Stale snapshots retain their original denominator. Zero-duration
windows stay in total mode until data is available. The cycle start remains
the next weekly reset minus seven days. Display modes change neither stored
totals, details totals, cache-hit ratios nor amount colors.

Usage rows share one responsive layout pass. It measures only currently visible
names and numbers, aligns numeric columns and places cache ratios at the right
edge. Spare width first increases token precision from one to two or three
decimal places, using original counts and the same elapsed-time denominator.
Extra trailing zeros are omitted. When all visible rows have enough room,
tokens use full integer counts with thousands separators instead of K/M/B
(e.g. `18,245,678`). Daily mode uses the integer daily rate and retains `均`.
Precision never displaces cache metadata,
shortens labels or consumes the minimum two-cell gaps between numeric fields.
Amounts retain currency precision (two decimal places). Remaining width becomes
elastic spacing between amount, tokens and cache.
Narrow views first consume spacing, then shorten labels and finally omit cache
metadata as needed. Compact DS period metadata precedes cache. Hit targets are
computed from the final rendered spans, excluding all alignment padding. Loading and
unavailable fields participate in the same columns as known values. Token
numbers, K/M/B units and the optional `均` marker have separate alignment slots;
missing values use `--` and real zero usage uses `0`, with no invented unit.
Cache icons reserve a percentage slot for `--`, 0–100%, and a shared trailing
warning slot, so missing values or warnings do not shift the icon column.

DeepSeek retains amount, tokens, and cache-hit percentage. The header
right-aligns `ds高峰` (bright yellow, `nf-fa-fire`) or `ds低谷` (bright aqua,
`nf-fa-leaf`) using the current price schedule, independent of historical window
selection. Narrow status can shrink to `ds` plus icon. Compact mode keeps the
signal on the DS row; it shortens the status and compresses spacing before
omitting cache detail, keeping token counts ahead of optional detail.
The signal describes the current instant, never the historical window's rate.

Only rendered label/amount/token text is clickable, including complete wide
characters; field padding, cache fields, the DS status and title-row whitespace
are informational. Clicking them neither switches windows nor refreshes quotas or
selects a pane behind the footer. Targets are rebuilt and cleared every frame.

Usage only occupies spare agent-list space. Narrow mode compresses fields while
retaining both Codex windows when height permits. Height pressure drops the
recent row before the cycle row; subscription quota remains available.
Codex follows the DeepSeek field order: name/window, amount, tokens, cache
hit rate. The `cx本期` row (shortened to `cx周` when needed) is the current weekly
quota cycle, not a calendar week. Spare columns restore reset countdowns first,
prioritizing the short-window reset if both do not fit. Remaining space expands
the cycle name and keeps usage columns aligned; growing amounts take precedence.
Live quota reset countdowns use active text brightness; stale/error age metadata
retains its subdued styling. No missing quota window is synthesized to fill space.
Usage rows keep one monetary field, total tokens, and cache rate; they do not
append separate averages or input/output token breakdowns. Input/output counts
remain available in Codex details. Existing subscription rows keep their manual
quota-refresh action, including when a subscription has only a weekly window.
`cx24h`, `cx7d`, and `cx30d` name the rolling window. Both share amount/token alignment and the
existing token/cache colors. The database glyph `` identifies cache-hit rate in both full and narrow modes.
Codex amounts are bold and indicate subscription utilization: daily average USD
up to 25 is muted, up to 75 blue, up to 150 aqua, and above 150 green. These fixed
bands were calibrated once from ten complete active days: median about USD 58
and upper quartile about USD 151. They do not drift with each refresh. Cycle
averages use elapsed days rounded up, with a one-day minimum. Colors indicate
API-equivalent usage, not actual profit or subscription charges. DeepSeek retains
its separate spend-warning ladder. Incomplete amounts remain muted. The estimate
explanation lives in details instead of a repeated approximation glyph.

## Coverage and unavailable costs

The scanner reads Codex sessions and archived sessions plus Claude Code project
logs for DeepSeek. This is local coverage only. Session metadata may contain an
account identity, but the current Usage view does not assert that all local
records belong to the subscription account. Details state this boundary.

`?` marks unpriced records; a displayed amount with `?` is only the priced
subtotal. If none of those records can be priced, show `$--?`, not zero. Explicit
unrecognized service tiers, unsupported cache-write semantics, unknown models, and missing
historical prices remain unpriced. Image generation charges are not inferred
from text-model usage. `!` indicates an observed source-read or parsing failure.
An idle source is not stale solely because no new event appeared.

Tier selection follows historical `thread_settings_applied` events and explicit
`turn_context.service_tier` fields. A context that omits the field preserves the
last settings event; an explicit null clears it. `default` maps to Standard.
When no tier is recorded, use the Standard API-equivalent baseline and disclose
how many records used that assumption in details, without an unpriced marker.
Model and event time determine the price version; provider routing and account
identity do not gate this estimate. Explicit Fast/Ultrafast settings retain their
versioned factors, and explicit unsupported tiers remain unpriced.
These events record the selected mode, not proof of the server-served tier;
Fast downgrades can therefore differ from this API-equivalent estimate.

## Historical price maintenance

`src/usage/pricing.rs` is the single production price book. Each `PriceVersion`
has a model, half-open effective interval, base rates, optional peak schedule,
optional long-context rates, and service-tier factors. A peak schedule owns weekday bits, local hour
intervals, and its UTC offset. Current DeepSeek schedules use UTC. Do not use a
fixed-offset schedule for a timezone with daylight-saving transitions without
adding explicit transition support and tests.

Choose the version by event time first, then apply its schedule. Append normal
price changes and close the previous interval at the new boundary. Never
rewrite old rates or peak hours to represent a new commercial price. Explicit
historical corrections increment `REVISION`, document the reason, and retain
the previous revision in repository history so estimates remain reproducible.
Reject overlapping intervals; missing intervals remain unpriced. All token/rate
products use integer arithmetic, with rounding after aggregation. The existing
DeepSeek presentation converts the final sum to its established CNY view model.

OpenAI sources checked on 2026-10-26:

- [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra):
  Standard rates and long-context threshold. The initial version starts on the
  release date (2026-09-03). Before the verification date, this is an explicit
  baseline assumption that the published rates were unchanged, not a verified
  historical price. Details disclose this whenever the selected window uses it.
- [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol):
  Standard and long-context rates.
- [Fast mode](https://developers.openai.com/api/docs/guides/fast-mode):
  selected mode can differ from the actual served tier.
- [API changelog](https://developers.openai.com/api/docs/changelog): Sol's
  2026-09-29 release establishes its initial Standard short-context rates.

Revision `2026-10-26.2` corrects the initial Astra coverage: verification time
is provenance, not an effective date. This approved initial baseline backfill
does not permit filling future gaps or overwriting historical versions with
new prices. Unknown models and records before model release remain unpriced.
Details identify missing prices, unknown tiers, and unsupported cache writes.
The shared footer title is ` Usage`; local-log scope is explained in details.

## Runtime and recovery

An enabled sidebar collects after pane status changes and on a 30-second
fallback. `@sidebar_quota off` stops collection; layout-driven footer hiding does
not. One collector owns the local history lock; other sidebar processes read
committed snapshots. Different configured root sets are reported instead of
silently claiming equivalent coverage. No new daemon, hook, or agent setup is
required.

The worker imports every available supported JSONL file, including old-mtime
files and archived Codex sessions. Recent files are prioritized; historical
backfill resumes at committed complete-line checkpoints and interleaves recent
refreshes. An incomplete final line is retried. Sources are read only.

History lives at `$XDG_DATA_HOME/tmux-agent-sidebar/usage.sqlite3`, or
`~/.local/share/tmux-agent-sidebar/usage.sqlite3` when XDG is unset or relative.
The private store retains allowlisted metering evidence and effective events
without automatic expiry. Source deletion does not delete history. No prompts,
responses, tools, credentials, or project contents are archived. Original cwd
is context only; directory renames are not matched into a project identity.

Counters, model/tier context, and event identities remain available for
historical repricing. Same-event supplements update one effective contribution;
conflicting evidence preserves the prior selection and raises an issue. Weak
identities, unavailable fork parents and unmatched response usage are disclosed.
Fresh unmatched responses remain visible in `usage-history status` and excluded
from totals, but receive three fallback scan intervals (90 seconds) to pair
before raising an incomplete-history notice. The age follows each persisted
observation across restart; new traffic cannot postpone an older warning.
Storage and parsing failures still surface immediately.
Price-only changes recompute from retained records without original transcripts.
Future normalization changes require a deliberate backed-up migration; the current
binary refuses unknown schema or normalizer versions rather than guessing.

Unchanged files require only metadata checks. Appends validate prefix and cursor
boundary fingerprints; detected replacement/truncation/same-size rewrites reparse
the source without deleting retained history. Arbitrary middle edits followed by
growth can evade these bounded checks. The 8 MiB raw-line limit prevents unbounded
allocations; oversized lines produce a persistent incomplete-source warning, not
a claim of complete import. There is no full-transcript archive or integrity scan.

The Usage header marks imports and failures. Persistent failures also appear in
the existing header notices indicator and popup, even when the footer cannot fit.
A failed first read does not display zero usage; later failures retain the last
usable snapshot with an error. Diagnostics are independent of SQLite and live at
`$XDG_STATE_HOME/tmux-agent-sidebar/usage-history.log` (fallback
`~/.local/state/tmux-agent-sidebar/usage-history.log`), with one rotated 5 MiB log.
Diagnostics include locations and sanitized causes, never complete source lines.

Inspect and maintain history using the installed binary:

```bash
tmux-agent-sidebar usage-history status
tmux-agent-sidebar usage-history backup /absolute/new-backup.sqlite3
tmux-agent-sidebar usage-history export /absolute/new-evidence.jsonl
```

Backup/export refuse to overwrite an existing destination. Backup uses SQLite's
consistent backup API; copying only a live database file can miss WAL contents.
Export contains versioned evidence and selected interpretations, not a supported
restore format. Long operations have a cooperative 60-second deadline and remove
only their own incomplete output on failure.

To explicitly clear history, stop every collector first, then run:

```bash
tmux-agent-sidebar usage-history purge --all --confirm
```

Purge makes and validates a private backup before clearing records and cursors.
If backup fails, purge stops. Existing source logs will be imported again when
collection resumes. Source files are never deleted. Ordinary cache cleanup must
not delete this database, which may be the only surviving usage history.

For offline restoration: stop all collectors and CLI readers; preserve the old
database and any matching `-wal`/`-shm` sidecars together in a separate directory;
install a verified backup at the original database path with private permissions
and no unrelated sidecars; run `usage-history status` before resuming collection.
Never delete a WAL as a generic repair step. Corrupt/unknown-version files are
preserved, not automatically rebuilt. Cloud backup/synchronization is external
to this project; do not synchronize an actively opened SQLite database as one file.

Import completion covers available sources, not an account-wide completeness
claim. Logs deleted while no enabled sidebar ran cannot be recovered. See the
[durable history decision](decisions/2026-10-26-durable-usage-history.md).

## Verification fixture

Build `cargo build --release --example usage_preview`, then run
`fixtures/scenarios/usage-focus/scenario.sh <temporary-output-directory>`.
The fixture uses the real renderer with deterministic in-memory state, no
workers, credentials, network, or developer session logs. It captures full,
narrow, wide (7d), daily-average, compact, off-peak, and detail views on an isolated tmux server. The existing HTML capture
parser drops RGB colors and treats wide characters as single cells; use the
raw `.ansi`/`.txt` captures and inline Ratatui snapshots to verify color and
column alignment rather than interpreting those export artifacts as UI defects.

Quota errors retain their existing short marker position. When known, the
marker distinguishes DNS, connection, TLS, certificate, timeout, local-process,
and HTTP 5xx failures. Narrow layouts omit the age before clipping the reason.
The isolated Usage fixture also captures quota errors, history errors (including narrow and popup views), and history import state.
