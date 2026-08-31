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

## Test Selection

| Change | Minimum verification |
|---|---|
| Pure documentation | link/path readback, `./scripts/verify.sh quick` if code-adjacent instructions changed |
| Hook adapter or handler | targeted lifecycle tests, then full verifier |
| Tmux query or pane lifecycle | targeted parser/fixture tests, full verifier, isolated real-tmux smoke |
| State/filter/group logic | targeted unit tests, full verifier |
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

`restart-sidebars` respawns existing sidebar panes in place, resets only the
status filter to `all`, and must not select a client, session, window, or pane.
Capture the active client/pane before and after any live verification.

## Fork Delta Review

```bash
./scripts/fork-delta.sh upstream/main
```

The report is read-only. It shows ahead/behind counts, committed churn and
skeleton touchpoints, highest-churn files, and uncommitted production paths. It
never fetches remotes; fetch explicitly when freshness matters.
