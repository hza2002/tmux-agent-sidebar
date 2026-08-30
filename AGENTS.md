# Agent Guide

## Mission

This repository is a personal fork of `hiroppy/tmux-agent-sidebar`. Keep it on
the latest upstream version while adding private workflow and UI preferences.
The fork must remain easy to merge, understand, and maintain with coding agents.

## Non-Negotiable Rules

- Treat upstream's module boundaries, data flow, and entry points as the
  architectural skeleton. Preserve them unless a documented decision explains
  why a core change is unavoidable.
- Prefer, in order: configuration, an existing extension seam, a leaf module,
  then a small integration change. Do not create parallel replacements for
  upstream subsystems.
- Keep fork-only behavior explicit and locally identifiable. Avoid mixing
  personal policy with unrelated upstream cleanup.
- When resolving upstream conflicts, preserve the new upstream structure first,
  then reapply the fork behavior through the smallest compatible seam. Never
  accept `ours` or `theirs` wholesale in a hotspot without reading both sides.
- Unit tests must never connect to the developer's live tmux server. Real tmux
  tests must use an isolated server or `TMUX_TMPDIR`.
- Do not modify generated snapshots with search-and-replace. Regenerate them
  through the owning test or capture workflow.

Read [docs/maintainers/fork-strategy.md](docs/maintainers/fork-strategy.md)
before changing production behavior.

## Progressive Context

Load only the material needed for the current task:

| Task | Read |
|---|---|
| Any production change or upstream sync | `docs/maintainers/fork-strategy.md` |
| Architecture, state, tmux lifecycle, hooks, or a hotspot | `docs/maintainers/architecture-map.md` |
| Test, build, install, signing, or runtime verification | `docs/maintainers/verification.md` |
| State fields, update cadence, or pane options | `docs/state-management.md` |
| Claude/Codex hook coverage research | `.agents/skills/sync-upstream-features/SKILL.md` |
| Release or version work | `.agents/skills/version-release/SKILL.md` |

Historical implementation plans under `docs/superpowers/` are evidence, not
current architecture. Read them only when the task explicitly concerns that
feature's history.

## Working Method

1. Run `git status --short --branch -uall` and preserve unrelated work.
2. Classify the change as upstream sync, fork policy, bug fix, or upstreamable
   improvement. Do not hide one class inside another.
3. Identify the narrowest existing seam from the architecture map.
4. If the change alters an entry point, core data flow, module ownership, or
   three or more hotspots, add or update a short decision record under
   `docs/decisions/` before implementation.
5. Add regression coverage proportional to the behavior changed.
6. Run the verifier described below and inspect the fork delta before handoff.

## Verification

```bash
./scripts/verify.sh quick       # format check + clippy
./scripts/verify.sh full        # isolated tests + quick checks + release build
./scripts/fork-delta.sh         # report divergence from upstream/main
```

CI runs `cargo test`, `cargo clippy`, and `cargo fmt --check`. Before every
commit, run `cargo fmt`; the verifier intentionally does not modify files.
After implementation, `./scripts/verify.sh full` includes the required release
build.

Any test that renders a Ratatui frame must use an inline
`insta::assert_snapshot!`. Substring assertions are only acceptable for
non-visual state or target metadata.

## Repository Shape

The binary has a TUI mode and CLI subcommands. Agent hooks normalize external
events through `adapter/` and `event/`, handlers write tmux pane options, the
refresh loop queries tmux into `AppState`, and `ui/` renders that state. Keep
this direction of dependency intact.

See the architecture map for module ownership and hotspot-specific checks.

## Writing

- Use Rust 2024 and existing repository patterns.
- Keep changes small, explicit, and dependency-free unless the task proves a
  dependency is necessary.
- Write all files under `docs/` and `.agents/skills/` in English.
