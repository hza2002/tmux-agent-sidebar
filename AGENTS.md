# Agent Guide

## Mission

This repository is an independently evolving fork of
`hiroppy/tmux-agent-sidebar`. Prioritize this fork's workflows, correctness,
and long-term maintainability. Upstream is a reference and a source of selected
fixes and features, not a version or architecture this repository must track.

## Non-Negotiable Rules

- Preserve useful module boundaries and explicit data flow. Structural changes
  are allowed when justified by this fork's needs and a documented decision.
- Prefer configuration, an existing extension seam, or a focused leaf module
  when these fully solve the problem. Avoid duplicate subsystems and unnecessary
  abstractions; upstream merge compatibility is not a design requirement.
- Keep changes focused and behavior ownership clear.
- Import upstream changes selectively after review. Resolve conflicts against
  this fork's intended behavior; never accept `ours` or `theirs` wholesale in
  a hotspot without reading both sides.
- The Claude Code plugin installs from a staged copy of this checkout, not from
  a symlink into it. Changing `hooks/hooks.json` or `hook.sh` therefore reaches
  Claude Code only after `/plugin update` — say so in the handoff instead of
  assuming the next session picks it up. Rust changes need no update: `hook.sh`
  resolves the live release build.
- Codex and Kimi read their hook declarations from the user's own config files
  (`~/.codex/hooks.json`, `~/.kimi-code/config.toml`). Changing either
  adapter's `HOOK_REGISTRATIONS` table reaches them only after `setup <agent>`
  is re-pasted, so name that re-paste in the handoff. The sidebar's
  Missing-hooks notice covers Codex drift; Kimi drift is silent.
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
| Hook declarations or the plugin install entry (`hooks/hooks.json`, `hook.sh`, `.claude-plugin/marketplace.json`) | `docs/decisions/2026-09-18-plugin-source-staging.md` |
| State fields, update cadence, or pane options | `docs/state-management.md` |
| Codex daemon routing, setup, or regression | `docs/codex-daemon-routing.md`, `docs/decisions/2026-10-11-codex-daemon-routing.md` |
| Historical daemon investigation, or picking the next fork improvement | `docs/maintainers/codex-daemon-and-fork-survey.md` |
| Claude/Codex hook coverage research | `.agents/skills/sync-upstream-features/SKILL.md` |
| Release or version work | `.agents/skills/version-release/SKILL.md` |

Historical implementation plans under `docs/superpowers/` are evidence, not
current architecture. Read them only when the task explicitly concerns that
feature's history.

## Working Method

1. Run `git status --short --branch -uall` and preserve unrelated work.
2. Classify the change as upstream import, product policy, bug fix, feature,
   or structural refactor. Do not hide one class inside another.
3. Identify the narrowest existing seam from the architecture map.
4. If the change alters an entry point, core data flow, module ownership, or
   three or more hotspots, add or update a short decision record under
   `docs/decisions/` before implementation.
5. Add regression coverage proportional to the behavior changed.
6. Run verification appropriate to the change and inspect the task diff before
   handoff. Use the fork delta report when evaluating upstream imports.

## Verification

```bash
./scripts/verify.sh quick       # format check + clippy
./scripts/verify.sh full        # isolated tests + real-tmux lifecycle tests + quick checks + release build
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
