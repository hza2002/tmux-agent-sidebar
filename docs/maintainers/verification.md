# Verification and Local Runtime

## Canonical Verifier

Use the repository wrapper instead of assembling commands ad hoc:

```bash
./scripts/verify.sh quick
./scripts/verify.sh full
```

`quick` runs a formatting check and strict Clippy. `full` additionally runs the
test suite with `TMUX` and `TMUX_PANE` removed and private tmux, temporary-file,
and activity-log directories, then builds the release binary.

This isolation is mandatory. Some state and input tests exercise code paths that
can select panes or persist tmux options in production. Unit-test command shims
are inert, and the wrapper provides a second boundary for integration tests.
Tests and capture fixtures must never run `tmux kill-server`; clean up only the
explicitly named session on an isolated socket.

## Direct Commands

```bash
cargo test
cargo test <test_name>
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
cargo fmt --check
cargo build --release
```

Run direct `cargo test` only outside tmux or with an isolated `TMUX_TMPDIR`.
Prefer the wrapper during agent work.

## Live Quota Probes

The quota unit tests inject `CurlRunner` mocks and therefore only verify what
we believe the providers and the local curl look like — they once all passed
while a real curl 8.7.1 pretty-printed `%{header_json}` over many lines and
every live fetch failed. `tests/quota_probe.rs` is the backstop: two
`#[ignore]`d end-to-end probes that read the developer's real credentials
(`~/.codex/auth.json`, `~/.kimi-code`), call the real `fetch_quota` entry
points over real curl, and assert the parsed windows are plausible (known
labels, `remaining_percent <= 100`, future Unix-seconds reset stamps), not
merely error-free.

Run them manually after changing `src/quota/`:

```bash
cargo test --test quota_probe -- --ignored
```

The probes depend on the network and on being logged into both providers, so
they can fail environmentally (throttling, provider outages, VPN down, a
provider that is briefly late rolling a window). A missing login skips that
provider's assertions instead of failing. When a probe fails, first check
whether the real service is having a problem before suspecting the code.

## Test Selection

| Change | Minimum verification |
|---|---|
| Pure documentation | link/path readback, `./scripts/verify.sh quick` if code-adjacent instructions changed |
| Hook adapter or handler | targeted lifecycle tests, then full verifier |
| Tmux query or pane lifecycle | targeted parser/fixture tests, full verifier, isolated real-tmux smoke |
| State/filter/group logic | targeted unit tests, full verifier |
| Quota fetcher or parser (`src/quota/`) | targeted unit tests, full verifier, then a manual live-probe run (`cargo test --test quota_probe -- --ignored`) |
| UI rendering | inline snapshots, styled tests, full verifier, visual capture |
| Release/install logic | full verifier and installed binary/version/signature readback |

Any test that renders a frame must assert the complete output with an inline
`insta::assert_snapshot!`. Do not replace a visual assertion with substring
checks.

## Local Plugin Installation

Resolve the actual configured binary before assuming a plugin path:

```bash
tmux show-options -gv @agent_sidebar_bin
```

The local plugin path must point directly at this checkout. Build the only
supported runtime artifact in place:

```bash
cargo build --release
test "$(tmux show-options -gv @agent_sidebar_bin)" = \
  "$PWD/target/release/tmux-agent-sidebar"
```

Do not copy or download a binary into `bin/`; launchers intentionally ignore
that legacy location so a stale artifact cannot mask the current local build.

Do not restart a user's active sidebar as a side effect of building or testing.
When runtime verification is explicitly required, use:

```bash
<installed-binary> restart-sidebars
```

`restart-sidebars` consolidates legacy duplicates and respawns the singleton
sidebar pane in place, resets only the status filter to `all`, and must not
change an attached client's current session, window, or pane. Capture the active
client/pane before and after any live verification.

## Fork Delta Review

```bash
./scripts/fork-delta.sh upstream/main
```

The report is read-only. It shows ahead/behind counts, committed churn and
skeleton touchpoints, highest-churn files, and uncommitted production paths. It
never fetches remotes; fetch explicitly when freshness matters.
