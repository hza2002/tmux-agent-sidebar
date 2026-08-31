# Decision: Local Source Runtime

## Context

This fork has one maintainer and runs on the maintainer's machine. A separate
TPM checkout and downloaded release binary allowed three versions to drift:
the active source checkout, the installed plugin source, and the installed
binary. The installer also wrote downloads directly over the active binary, so
a network failure could leave a truncated executable.

## Chosen Seam

The tmux launcher, hook wrapper, and installer resolve only
`target/release/tmux-agent-sidebar` from the local plugin working copy. Missing,
version-mismatched, or older-than-source binaries are rebuilt with
`cargo build --release`. The TUI does not query GitHub Releases, and the
installer has no remote download action.

The local plugin path is a symlink to the maintained working copy and is loaded
directly from `tmux.conf`, outside TPM management. That checkout is the single
source of truth for source, configuration, and runtime.

## Alternatives Rejected

- Atomic release downloads would prevent truncation but retain an unnecessary
  second distribution lane and version reconciliation problem.
- Comparing semantic versions before prompting would fix the false-positive
  update dialog but still allow the installed checkout to drift from the fork.
- A tmux option for remote checks would preserve unused complexity and make the
  local-only invariant depend on mutable runtime configuration.

## Upstream Compatibility

The Rust application architecture and agent event flow are unchanged. The fork
keeps the upstream remote for source synchronization, but runtime installation
policy is intentionally fork-owned. Upstream changes to launchers, installers,
or release notices must be reconciled without restoring a remote runtime lane.

## Conflict and Removal Strategy

Policy tests reject GitHub Release endpoints, download actions, and `bin/` or
`PATH` binary fallbacks in runtime launchers. During upstream merges, preserve
new upstream script structure and reapply the local-only resolution rules. To
remove this policy, restore an atomic verified release installer and delete the
policy tests and this decision record together.
