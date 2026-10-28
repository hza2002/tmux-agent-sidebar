# Codex Session Account Attribution: Research and Options

Status: historical research. The bounded runtime-observation subset is now
implemented by the [runtime attribution decision](../decisions/2026-10-28-codex-runtime-attribution.md).
That decision supersedes proposals below, especially claims of exact per-request
billing identity: `account/read` observes a daemon login, not a billing receipt.
The accepted topology uses the shared daemon only for the official account;
embedded, missing and conflicting future observations remain unassigned.
Original transcript evidence and historical creator-account fallback are retained.
Real provider continuation and OAuth-refresh behavior are not established by the
isolated tests. The maintained contract is [usage metering](../usage-metering.md).

Date: 2026-10-27. Investigation environment: Codex 0.164.0, macOS, one local
`~/.codex`, one running app-server. The local account ids and email are omitted
below; placeholders such as `acct-A` identify distinct accounts.

Related material:

- [Local usage and historical prices](../usage-metering.md)
- [Codex daemon hook routing](../codex-daemon-routing.md)
- [Account-scoped usage rows decision](../decisions/2026-10-27-usage-account-scope.md)
- [Architecture map](../maintainers/architecture-map.md)
- [Fork strategy](../maintainers/fork-strategy.md)

## 1. Problem

A Codex rollout's `session_meta.creator_account_id` is fixed when the session is
created. Resuming that session under another subscription does not rewrite it.
A user with two subscriptions can therefore produce:

- a session created by account A;
- model turns served by account B after an account switch and resume;
- one rollout file whose every `token_count` event still claims account A.

The current Usage view consequently attributes B's turns to A. When the
signed-in account equals the only account with local usage, `cx账号` and
`cx全部` can show identical numbers even though account switching occurred.

A second, related problem is that cc-switch-style tools may replace
`auth.json` while the running Codex process keeps the previously loaded
credentials. The sidebar reads `auth.json`; the running session may still be
served by the old account until the session or server restarts.

The questions for this investigation are:

1. Is the serving account recoverable per turn from local data?
2. At what granularity?
3. Which source is stable enough to build a product feature on?
4. Can the per-turn quota data in the logs also improve the quota and `本期`
   display?

## 2. Evidence from local sources

### 2.1 Rollout fields

A survey of a full local day of Codex rollouts found these account-related
fields:

| Field | Level | Meaning | Per-turn account? |
|---|---|---|---|
| `session_meta.creator_account_id` | session | account that created the rollout | no |
| `session_meta.creator_user_id` | session | user identity at creation | no |
| `token_count.rate_limits.plan_type` | turn | plan served for that turn | no; plan is not identity |
| `token_count.rate_limits.primary/secondary` | turn | `window_minutes`, `resets_at`, `used_percent` | no; quota shape, not account id |
| `token_count.info.*` | turn | token counts | no |
| `turn_context.*` | turn | model, tier, turn id | no |

No evaluated source contained a per-turn `account_id` or `user_id`:

- `token_count`, `turn_context`, and `thread_settings_applied` payloads do not
  carry one.
- `~/.codex/state_5.sqlite.threads` has `creator_account_id` /
  `creator_user_id` per thread; `thread_history_1.sqlite` items do not carry an
  account.
- `~/.codex/session_index.jsonl` has no account field.
- `~/.codex/log/codex-login.log` recorded no entries for the recent account
  switches (its last entry predates them).
- `~/.codex/logs_2.sqlite` has app-server request logs and one plugin-manager
  `authenticated_account_id`, but no reliable per-turn account record.

### 2.2 Mixed-session observation

One session created by `acct-A` contained both accounts' turns:

| `rate_limits.plan_type` | `token_count` events | Raw tokens |
|---|---:|---:|
| `prolite` | 126 | 30.2M |
| `plus` | 8 | 2.4M |

The switch directions were visible at two timestamps approximately 8 minutes
apart. The same session therefore demonstrably contains turns served by two
different accounts while its `creator_account_id` remains `acct-A`.

The two accounts' weekly windows also differed in shape and reset time:

- one account had a weekly-only primary window (10080 minutes);
- the other had a 300-minute primary and a 10080-minute secondary;
- their weekly reset timestamps differed by about 23.5 hours;
- one account's reset timestamp jittered by one second between events.

### 2.3 Why the obvious signals are not identity keys

- **`plan_type`** is a plan, not an account. It cannot separate multiple Pro
  accounts, and it changes when one account upgrades or downgrades.
- **Reset timestamps** fail under OpenAI-initiated resets (which can align many
  accounts) and user reset cards (which can move one account independently).
  Collisions are then possible, and the mapping needs bootstrapping per cycle.
- **`used_percent`** changes with every request and is not an identifier.
- **`auth.json` polling** observes the signed-in account, not necessarily the
  account loaded by the running server. A cc-switch can replace the file while
  the server keeps serving the previous account until restart. This was
  observed directly during the investigation: at one check, `auth.json`
  identified `acct-Pro` while the running app-server reported `acct-Plus`.

### 2.4 Control-socket probe

A read-only probe connected to:

```
~/.codex/app-server-control/app-server-control.sock
```

which is a symlink to the daemon's private Unix socket. It performed a standard
WebSocket handshake at `GET /rpc` and sent JSON-RPC messages after
`initialize` / `initialized`.

Working methods:

- `thread/loaded/list` (already used by the existing hook routing transport);
- `account/read` with `{"refreshToken": false}`.

`account/read` returned the running server's account, not `auth.json`:

```json
{
  "account": {
    "type": "chatgpt",
    "email": "<redacted>",
    "planType": "plus"
  },
  "requiresOpenaiAuth": true,
  "workspaceRouting": {
    "chatgptAccountId": "<account UUID>",
    "backendOrigin": "https://chatgpt.com",
    "accountRoutingOverride": "NO_CONSTRAINT"
  }
}
```

The useful field is `workspaceRouting.chatgptAccountId`: the account id the
running server is actually using. The email is present but must not be
persisted.

`account/rateLimits/read` did not produce a response on this control socket
within a 3-second probe timeout. It works over the stdio app-server used by the
quota fallback (`src/quota/codex.rs`), which is a different transport.

Caveats:

- The probe was run against Codex 0.164.0. `workspaceRouting` is a newer schema
  and may change.
- The control socket is a local daemon surface, not a stable public API.
- The existing hook routing already depends on this socket for
  `thread/loaded/list`; adding one read is a small extension of an existing
  dependency, not a new subsystem.

## 3. Options

| Option | Account key | Per-turn? | Strengths | Failure modes |
|---|---|---|---|---|
| A. Status quo | session creator | no | exact for sessions, zero work | wrong across account resume |
| B. `plan_type` scope | plan | yes | cheap, no new source | same-plan accounts, plan changes |
| C. Reset-window fingerprint | reset cluster | yes | account-level in principle | global resets, reset cards, collisions, bootstrapping |
| D. `auth.json` timeline | signed-in account | time-based | cheap; no hooks | server may keep the old account; cc-switch lag |
| E. Control-socket `account/read` timeline | running server account | per hook event / time | exact serving account; immune to plan/reset | hook/control-socket coverage and schema drift |
| F. E + per-turn log quota supplement | E for identity; logs for quota | per turn | also fixes serving-account quota and `本期` | passive/stale quota; extra parser/UI work |

Options B, C, and D should be treated as unsuitable for independent
per-account statistics. Option E is the only candidate that reads the account
from the process that actually serves the request. Option F is an optional
second phase.

## 4. Candidate design E: serving-account observations

### 4.1 Observation source

For Codex lifecycle hook events that already exist:

- `SessionStart` (`startup|resume`);
- `UserPromptSubmit`;
- `Stop`;

the hook reads the control socket's `account/read` response and records:

```json
{"ts": 179..., "session": "...", "turn": "...", "account": "acct-UUID"}
```

Only the account id is retained. The email, tokens, prompt, and response text
are never persisted.

The Codex hook registrations and the control-socket environment are already
present in the user's setup. If the observation is added inside the existing
`codex_routing` path, no `hooks.json` change, re-paste, or re-trust is needed;
the Rust hook binary resolves the live release build on the next event.

### 4.2 Attribution

Candidate rule:

- For each `token_count` event, find the latest observation at or before the
  event timestamp.
- If none exists, fall back to the session creator account.
- The account row's scope is the latest serving-account observation (fallback:
  `auth.json` account id).
- `cx全部` remains unchanged.

This avoids plan/reset dependence and handles the cc-switch lag: the
observation is the server account, not the file account.

Timestamp matching is the simplest variant. Session/turn matching would be more
precise but requires the usage store to expose a turn/session key per
aggregated record; the current production projection deliberately drops the
large event identity. This is an open decision (Section 7).

### 4.3 Storage and integration

The smallest variant is a private append-only sidecar:

```
$XDG_STATE_HOME/tmux-agent-sidebar/codex-account-timeline.jsonl
```

- Written by the hook on lifecycle events.
- Read by the usage worker during `history::snapshot`.
- Applied as a query-time override of `Record.codex.account`; the durable
  history DB is not rewritten.
- No schema, normalizer, or extractor version change is required.
- If the sidecar is lost, attribution reverts to the session creator. The
  sidecar must therefore be included in backup/export documentation.

A DB table is a more durable alternative but requires a schema/migration
decision. This is an open decision.

### 4.4 Coverage and fallbacks

- Hook/control-socket available: exact serving account, per turn.
- Hook not fired for a client (for example, a client that does not execute
  Codex hooks): the usage worker could poll `account/read` periodically while
  the sidebar runs. That requires sharing the WebSocket transport outside the
  hook module and discovering the control socket from the sidebar process.
- No observation: fall back to the session creator. Do not fall back to
  `auth.json` as if it were an exact observation.
- The observation is a snapshot of the server's account at the event time. A
  server restart or account reload between the prompt and the stop is expected
  to produce another observation; a mid-turn change without any hook remains an
  edge case.

### 4.5 `本期` and quota consistency

The account row's `本期` boundary currently comes from the quota API for the
`auth.json` account. When the serving account differs, that boundary belongs to
the wrong account. Candidate responses:

1. Mark the mismatch and disable/hide `本期` for the serving account until the
   accounts align; or
2. Use the serving account's latest log rate limits to derive the weekly
   boundary (part of option F).

The quota row itself is also auth-based. At minimum, a notice should disclose
"running Codex account differs from the signed-in account"; the quota row and
the account row otherwise describe different accounts.

## 5. Candidate supplement F: per-turn `rate_limits`

Every `token_count` event carries the serving account's quota snapshot:

- `plan_type`;
- `primary/secondary.window_minutes` (300 = 5h, 10080 = weekly);
- `primary/secondary.used_percent`;
- `primary/secondary.resets_at`.

These values can supplement the proactive quota display:

- **Serving-account quota row**: when the serving account differs from
  `auth.json`, or the quota API fails, show the latest log snapshot for the
  serving account. Convert `used_percent` to remaining (`100 - used`) and map
  300/10080 to the existing `5h`/`wk` labels.
- **`本期` boundary**: the serving account's weekly `resets_at` gives a
  correct cycle boundary even when auth and serving differ.
- **Plan/window shape**: the serving account's plan and window durations can
  explain a different 5h/weekly shape.

Constraints:

- The log snapshot is passive. It is only as fresh as the last turn and must be
  marked stale; it must not masquerade as a live API result.
- A new session with no turns has no log quota data. The API remains the
  primary source when it describes the serving account.
- The API should stay primary when `auth.json` and the serving account match.
  The log snapshot is a fallback for mismatches and failures.
- Using the data requires parsing and retaining `rate_limits` (or at least the
  latest observation per serving account), plus quota-state/UI changes. This is
  a separately sized phase, not part of the account-observation MVP.
- A simpler interim response is the mismatch notice plus restarting the server;
  option F can be deferred until the mismatch is shown to matter in practice.

## 6. Change surface and risks

### 6.1 Account-observation MVP

| Area | Files | Change | Approximate size |
|---|---|---|---|
| Control transport | `src/cli/hook/codex_routing/transport.rs` | `account/read`; parse `workspaceRouting.chatgptAccountId`; mock test | 80–120 lines |
| Hook dispatch | `src/cli/hook/codex_routing.rs` | query on start/prompt/stop; append observation | 40–80 lines |
| Sidecar | new `src/usage/account.rs` | path, append, load, timestamp lookup; private permissions | 100–150 lines |
| Aggregation | `src/usage/history.rs` | serving-account scope; query-time account override | 30–50 lines |
| UI | quota/usage notices | mismatch disclosure; optional `本期` guard | small–medium |
| Docs | `docs/decisions/`, usage/state docs | source, limits, sidecar backup | 2–3 files |

No `hooks.json` change, no re-paste, no history schema/normalizer migration in
this MVP. The existing Codex hook registrations already cover the events.

### 6.2 Risks and mitigations

- **Control-socket schema drift**: parse defensively; on a missing field or
  error, record no observation and emit a diagnostic; usage falls back to the
  creator. Do not guess.
- **Control-socket unavailable**: fail open with a short deadline; do not delay
  the turn. Do not substitute `auth.json` as an exact observation.
- **Hook coverage**: hooks may not fire for every client or may be untrusted.
  Verify VS Code/daemon behavior; add a sidebar worker poll only if needed.
- **Sidecar durability**: query-time override is lost if the sidecar is
  deleted. Document backup/export; consider a DB table if durability outweighs
  migration cost.
- **Timestamp attribution**: long turns and mid-turn changes are edge cases.
  Session/turn keys are the stronger alternative but require store changes.
- **Multiple servers/accounts**: a global timeline can mix observations from
  different servers. The current investigation is single-daemon; multi-server
  support needs a server/socket dimension.
- **Privacy**: the response contains an email; only the account id may be
  retained.
- **Tests**: use a mock control socket; never connect tests to the developer's
  live daemon.

### 6.3 Simplifications worth considering

- Hook-only first; defer worker polling until coverage is verified.
- Timestamp matching instead of session/turn joins.
- Query-time sidecar instead of history migration.
- A mismatch notice and `本期` guard instead of implementing the quota
  supplement immediately.
- Defer option F entirely if restarting the server after a switch makes the
  auth/serving mismatch acceptably short.

## 7. Open questions for expert review

1. Is timestamp attribution sufficient, or is a session/turn key worth a store
   change?
2. Sidecar vs SQLite table for observation durability and backup?
3. Should the account row's scope be the serving account while the quota row
   remains signed-in-account based, or should both follow the same source?
4. When `auth.json` and the serving account differ, should `本期` be disabled,
   derived from log rate limits, or shown with an explicit mismatch warning?
5. Is the hook/control-socket dependency acceptable given its version
   sensitivity, or should the design wait for a stable upstream per-turn
   account field?
6. What is the minimum acceptable coverage: TUI/daemon only, or also VS Code
   and other clients?
7. Should historical backfill be abandoned explicitly (no observations before
   the feature), or should a best-effort plan/reset heuristic label old data
   as "inferred"?
8. Does the quota supplement justify parsing/storing `rate_limits`, or is a
   mismatch notice plus a server restart enough?

## 8. Non-goals

- Recovering account identity for turns recorded before observations existed.
- Account-wide or vendor-billing completeness; the view remains local-coverage
  only.
- Cloud/remote Codex tasks, other machines, or logs outside the configured
  roots.
- Per-request attribution when the serving account changes mid-turn without any
  lifecycle observation.
- Persisting emails, tokens, prompts, responses, or credentials.
- A stable public API contract for the Codex control socket.

## Appendix A: Current code seams

- `src/quota/codex.rs`: stdio `account/read` / `account/rateLimits/read`;
  `accountId` consistency check; `parse_rate_limits`.
- `src/cli/hook/codex_routing/transport.rs`: existing control-socket WebSocket
  client and `thread/loaded/list` usage.
- `src/adapter/codex.rs`: Codex hook registrations and `session_id`/`turn_id`
  parsing.
- `src/usage/history.rs::snapshot`: current account aggregation and
  `cx账号`/`cx全部` split.
- `src/usage/history/evidence.rs`: session `creator_account_id` and
  `token_count` evidence.
- `src/usage/source.rs`: legacy scanner used by tests; same field assumptions.
- `src/ui/panes/quota.rs`: quota rows, usage rows, `本期` boundary, and
  account/tail labels.

## Appendix B: Field inventory (sanitized)

Observed in local rollouts on 2026-10-27:

```
session_meta: id, source, cwd, model_provider, creator_account_id, creator_user_id
turn_context: turn_id, model, service_tier, ...
token_count: info.{last_token_usage,total_token_usage,model_context_window},
             rate_limits.{limit_id,limit_name,plan_type,credits,
                          primary.{used_percent,window_minutes,resets_at},
                          secondary.{...}|null}
```

No per-turn account id or user id was found in any of these payloads.
