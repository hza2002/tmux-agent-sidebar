# Local usage and historical prices

Quota and Usage share one footer and the ` Usage` header, controlled by
`@sidebar_quota`. Compact mode omits the header entirely to preserve data rows.
Quota reports provider remaining percentages and reset countdowns. Usage reads
local session logs in the existing background worker; it is not a vendor bill
or an account-wide usage report.

## Windows and interaction

Codex has two scope rows. `cx账号` (first) selects the local running daemon's
ChatGPT account via a bounded read-only `account/read`, refreshed every 30s and
on requested refreshes. It never substitutes the current `auth.json` when the
server cannot be read. `cx全部` counts all local Codex metering records.

Runtime prompt/stop observations are matched by exact session and turn IDs.
Before the persisted attribution boundary, unobserved records retain the creator
account as a historical approximation. After it, absent observations, embedded
processes and conflicting observations remain unassigned. Account observations
assume the accepted deployment: one official account in the shared daemon and
third-party providers using `--no-daemon`; they are not billing receipts and do
not support custom per-thread provider routing inside that daemon. Records with
no account remain in the all-accounts row. DeepSeek keeps its model-based spend
bucket. `usage-history status` exposes effective account counts, unassigned
counts, runtime turn counts and the attribution boundary.

The account row owns `本期`: the weekly reset timestamp belongs to one account,
so there is no shared cross-account cycle. Its windows are `本期` then
`当天`, `24h`, `7d`, and `30d`. The all-accounts row has rolling windows only.
The DeepSeek row keeps its own window. Today means local midnight through the
scan timestamp; 24h, 7d (168 hours), and 30d (720 hours) are elapsed windows.
A failed quota refresh retains the last known cycle boundary only for the same
account. An expired, missing or differently attributed boundary leaves `本期`
unavailable. A successful response without a weekly window, or a missing
subscription, clears the boundary. Each boundary carries the account used by
the quota request; the usage worker uses it only for that observed daemon
account. The subscription quota display itself still follows the quota client.

Click a provider's window label to advance only that row. The header keeps a
static `Usage` title and a right-aligned control group. Its interval control is
`󰬁当天`, `󰬁24h`, `󰬁7d`, or `󰬁30d`: the `t` glyph and the shared window label
share that window's color and both advance every usage row when clicked. The
control cycles through `本期` (account row = 本期, both other rows = 当天),
`当天`, `24h`, `7d`, `30d`, then back. A custom row combination shows `󰬁混选`
in a neutral color; the next interval click returns to `本期`. A dim leader
line connects the title to the control. Clicking `󰫮日均` toggles the row-level daily mode for
every eligible row. The plain `t` and `a` keys perform the same two actions;
`本期` is selected by clicking the account row's label, and the `Usage` title
itself is not clickable. The `󰬁` / `󰫮` glyphs are the Nerd Font Material
Design `alpha_t` / `alpha_a` icons.

Click an amount or token to toggle that row between total and daily rate. The
mode is row-level: amount and tokens switch together. Rows add no marker; the
header's `日均` control brightens while any eligible row is in daily mode,
including scopes temporarily hidden by height pressure, and is hidden when none
supports it, so numbers carry no `均` suffix. Compact
mode omits the header and therefore has no daily indicator. Daily mode is
available on `本期`, `7d`, and `30d`. `当天` is a partial day and `24h` would
equal the total itself, so both stay total-only. Missing prices leave the
amount in total mode while its tokens can still switch.

Daily rate is observed usage multiplied by 86,400 seconds and divided by the
observed interval. `7d` and `30d` use their fixed durations; `本期` uses the
elapsed part of the seven-day cycle, so early in a cycle it is a partial-window
normalization, not a forecast. Stale snapshots retain their original
denominator. Zero-duration windows stay in total mode until data is available.

Usage rows align on fixed field budgets: name/window 6 cells, amount 4–8
(growing only for unusually large daily rates), tokens 6, cache 6, and a
right-aligned 4-cell tail (`账号`/`全部`/`高峰`/`低谷`). Row amounts omit
thousands separators so the normal budget is eight cells (`¥9999.99`); the
`usage-history` output keeps grouping. Columns do not expand token precision or
stretch with value changes; the only elastic space is the gap before the tail.
Narrow views drop the optional warning, then cache, then shorten `cx本期` to
`cx周` and `ds当天` to `ds天`. The scope tail is never dropped. Missing values
use `--`; real zero usage uses `0`, with no invented unit.

The DeepSeek row ends with the current price period (`高峰` or `低谷`), without
an icon or `ds` prefix. The signal describes the current instant, never the
historical window's rate. Reset countdowns live on the subscription quota rows,
not on usage rows.

Only rendered label, amount, and token text is clickable, including complete
wide characters; padding, cache, the daily control, and the tail are
informational. Clicking them neither switches windows nor refreshes quotas or
selects a pane behind the footer. Targets are rebuilt and cleared every frame.

Usage only occupies spare agent-list space. Height pressure drops the
all-accounts row before the account row; subscription quota remains available.
Codex rows follow the DeepSeek field order: name/window, amount, tokens, cache
hit rate, then the scope or period tail. Existing subscription rows keep their
manual quota-refresh action, including when a subscription has only a weekly
window. `cx24h`, `cx7d`, and `cx30d` name rolling windows.
The database glyph `` identifies cache-hit rate in both full and narrow modes.
Codex amounts are bold and indicate subscription utilization: daily average USD
up to 25 is muted, up to 75 blue, up to 150 aqua, and above 150 green. These fixed
bands were calibrated once from ten complete active days: median about USD 58
and upper quartile about USD 151. They do not drift with each refresh. Cycle
averages use elapsed days rounded up, with a one-day minimum. Colors indicate
API-equivalent usage, not actual profit or subscription charges. DeepSeek retains
its separate spend-warning ladder. Incomplete amounts remain muted. The
estimation assumptions are documented below and inspectable with
`usage-history status`.

## Coverage and unavailable costs

The scanner reads Codex sessions and archived sessions plus Claude Code project
logs for DeepSeek. This is local coverage only. Account attribution follows the
runtime observation policy above; missing hooks, turn IDs or unsupported runtime
modes reduce account coverage. Subagents without their own observations remain
unassigned. No view claims account-wide billing completeness.

Hooks spool allowlisted observations beside `usage.sqlite3` in the private
`usage.attribution/` directory. Only the existing collector writes SQLite; it
imports at most 256 spool entries per pass, commits before deleting each input,
and retains conflicting observations as unknown. The ledger uses additive
`codex_turn:` metadata and `codex_attribution_since`; raw source evidence and
records remain unchanged. Late observations affect the next query without a
transcript replay. SQLite backups retain the overlay; JSONL exports include
`runtime_attribution` entries alongside the original evidence/events. Schema and
normalizer remain version 1 because extraction and canonical identity did not
change. Older binaries ignore the overlay and must not be used for account
reporting after rollout. See the [runtime decision](decisions/2026-10-28-codex-runtime-attribution.md).

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
how many records used that assumption in `usage-history status`, without an unpriced marker.
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
The row amount's `?` marks an incomplete total; `usage-history status` reports
missing prices, unknown tiers, and unsupported cache writes. The shared footer
title is ` Usage`; local-log scope is documented above and in that status output.

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
workers, credentials, network, or developer session logs. It captures full, narrow, wide (7d), daily-average, compact, and off-peak views
on an isolated tmux server. The existing HTML capture
parser drops RGB colors and treats wide characters as single cells; use the
raw `.ansi`/`.txt` captures and inline Ratatui snapshots to verify color and
column alignment rather than interpreting those export artifacts as UI defects.

Quota errors retain their existing short marker position. When known, the
marker distinguishes DNS, connection, TLS, certificate, timeout, local-process,
and HTTP 5xx failures. Narrow layouts omit the age before clipping the reason.
The isolated Usage fixture also captures quota errors, history errors (including narrow and popup views), and history import state.
