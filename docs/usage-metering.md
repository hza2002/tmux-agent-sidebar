# Local usage and historical prices

Quota and Usage share one footer and the ` Usage` header, controlled by
`@sidebar_quota`. Compact mode omits the header entirely to preserve data rows.
Quota reports provider remaining percentages and reset countdowns. Usage reads
local session logs in the existing background worker; it is not a vendor bill
or an account-wide usage report.

## Windows and interaction

Codex has a current-cycle row and a recent row. The cycle follows the weekly
quota reset timestamp and the recognized seven-day quota duration. It is not
the subscription payment month or a calendar week. Expired, stale, or missing
quota metadata leaves the cycle unavailable until a successful refresh.

Click the recent row to cycle exact elapsed 24h, 7d, and 30d windows; 7d is the
default. A weekly reset does not clear these rolling windows. Click the cycle
row to open details. Tab switches details between cycle and the selected recent
window; arrow keys scroll and Escape closes. Details include per-model input,
cache-hit and output counts, UTC interval bounds, and price-book revision.

DeepSeek keeps its local-calendar today/7-day/30-day click cycle, amount, tokens,
cache-hit percentage, and today's current peak/off-peak label. Multi-day rows
combine price periods and do not claim one peak/off-peak state.

Usage only occupies spare agent-list space. Narrow mode compresses fields while
retaining both Codex windows when height permits. Height pressure drops the
recent row before the cycle row; subscription quota remains available.
Codex follows the DeepSeek field order: name/window, amount, total tokens, cache
hit rate. The `cx本期` row (shortened to `cx周` when needed) is the current weekly
quota cycle, not a calendar week. Spare columns restore reset countdowns first,
prioritizing the short-window reset if both do not fit. Remaining space expands
the cycle name and keeps usage columns aligned; growing amounts take precedence.
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

One worker scans after pane status changes and on a 30-second fallback. It
retains complete-line offsets, parser context, and usage-only records in memory.
No new hooks, database, credentials, or persisted prompt content are introduced.
Unchanged files use cached records; appends resume at the last complete line.
Same-size changes, truncations, and identity/prefix changes replace the file's
contribution. Arbitrary middle-of-file edits followed by growth are not fully
detectable with bounded append fingerprints; restart rebuilds from source logs.

Cumulative Codex counters distinguish new usage from repeated quota updates;
turn identity and counters deduplicate copied history. Missing identity evidence
is disclosed. Claude streaming entries retain the largest token record with a
deterministic tie-break. Retention is 31 days of normalized records; original
logs remain the rebuild source. No guarantee survives deletion of those logs.

## Verification fixture

Build `cargo build --release --example usage_preview`, then run
`fixtures/scenarios/usage-focus/scenario.sh <temporary-output-directory>`.
The fixture uses the real renderer with deterministic in-memory state, no
workers, credentials, network, or developer session logs. It captures full,
narrow, and detail views on an isolated tmux server. The existing HTML capture
parser drops RGB colors and treats wide characters as single cells; use the
raw `.ansi`/`.txt` captures and inline Ratatui snapshots to verify color and
column alignment rather than interpreting those export artifacts as UI defects.
