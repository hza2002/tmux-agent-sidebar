# Decision: `@sidebar_quota_kimi` — per-subscription switch for the Kimi quota row

## Context

The fork's quota block ([2026-09-17-quota-block.md](2026-09-17-quota-block.md))
renders Codex and Kimi rows. The user has ended their Kimi Code subscription.
Three surfaces then go stale at once: the row shows a number that no longer
reflects anything (or, once the stored credentials stop authenticating, a
permanent dimmed failure placeholder such as ` kimi  5h   -- wk   -- ·1m 登录`);
`quota_poll_loop` keeps calling `api.kimi.com` on the active cadence; and the
fetch's refresh path keeps rotating tokens in
`~/.kimi-code/credentials/kimi-code.json` — a file shared with the Kimi CLI,
the user's status-line script, and the Raycast extension. The agent adapter
itself needs no change: with no Kimi panes it renders nothing, and the user's
`@sidebar_hook_check_agents` already excludes Kimi.

## Chosen Seam

Configuration: one new tmux option `@sidebar_quota_kimi` (default `on`),
handled exactly like `@sidebar_quota`. When `off` at startup:

- `QuotaState.kimi_enabled` is `false` (set in `app/setup.rs` from the option,
  parsed by the same on/off helper `@sidebar_quota` uses);
- `app/workers.rs` injects a stub Kimi fetcher returning
  `QuotaFetch::Unavailable` at the existing fetch-injection point — the state
  machine's established "this subscription does not exist" outcome, so no HTTP
  request, no credentials read or token rotation, and no row;
- `QuotaState::enabled` gates the first-result placeholder phase and
  `expected_count`, so the block never reserves a Kimi row at startup;
- the renderer's pending-placeholder branch filters through the same method.

## Alternatives Rejected

- Removing the credentials file (no code): destroys login state shared with
  the Kimi CLI, the status-line script, and the Raycast extension, and any
  future Kimi CLI run recreates the file — the row would silently return.
- `@sidebar_quota off`: hides the Codex row too.
- Deleting the fork's Kimi quota client: a "hide it" request does not justify
  burning the path back. The client, its parsing tests, and the cadence code
  are unaffected by the switch and become live again with one option flip.

## Upstream Compatibility

`src/quota/`, `src/ui/panes/quota.rs`, and the quota block are fork-only;
`src/ui/mod.rs` gains one function beside the fork-added `quota_enabled_from_tmux`
(existing on/off parsing factored into a shared private helper);
`src/state.rs` is untouched — the flag lives on the fork-owned `QuotaState`.
The `workers.rs` edit is one argument at an existing injection point. Default
`on`, so behavior is unchanged unless the option is set.

## Conflict and Removal Strategy

On upstream merges, keep upstream's `workers.rs` wiring and re-add the fetch
choice; every other edit is additive. Full removal restores
`crate::quota::kimi::fetch_quota` as the unconditional fetcher and deletes the
`SIDEBAR_QUOTA_KIMI` constant, the parse helper, `QuotaState::kimi_enabled` /
`enabled`, the renderer filter, the conf seed, and this record.
