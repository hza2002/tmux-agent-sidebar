# Fork Strategy

## Objective

This fork exists to apply personal workflow and visual preferences while
continuously tracking the latest `hiroppy/tmux-agent-sidebar` release. It is not
an independent product architecture. Upstream remains the source of truth for
the application's skeleton; the fork owns a bounded behavior layer on top.

Success means all three statements remain true:

1. A normal upstream merge is the default update path.
2. A maintainer can identify fork-owned behavior without reconstructing history.
3. Removing a personalization leaves recognizable upstream code, not a second
   implementation of the same subsystem.

## Design Order

Use the first option that fully solves the problem:

1. **Configuration:** tmux option, theme value, or existing runtime setting.
2. **Existing seam:** adapter registration, state submodule, row renderer,
   command handler, or established helper trait.
3. **Leaf extension:** a small new module with one narrow integration point.
4. **Core edit:** a localized change to upstream orchestration.
5. **Skeleton change:** only with a decision record and no viable smaller seam.

The order is a design constraint, not a preference. A core edit should explain
why configuration and existing seams were insufficient.

## Upstream Skeleton

Preserve these relationships:

```text
CLI/TUI entry points
  -> adapter + internal event normalization
  -> hook handlers write tmux-owned state
  -> tmux query builds PaneInfo/SessionInfo
  -> AppState derives local UI state
  -> ui renders state and records hit targets
```

Avoid changes that reverse these dependencies, let rendering mutate durable
state directly, make handlers parse raw agent JSON, or add a second tmux query
pipeline beside the existing one.

The following are skeleton touchpoints. Changes are allowed, but require an
explicit compatibility explanation in the commit or decision record:

- `src/main.rs`, `src/cli/mod.rs`: process and command entry points
- `src/adapter/`, `src/event.rs`, `src/event/`: event normalization contract
- `src/tmux/query.rs`, `src/tmux/types.rs`: tmux-to-domain boundary
- `src/app.rs`, `src/state.rs`, `src/state/refresh.rs`: orchestration and state flow
- `src/ui/mod.rs`: top-level render composition

## Fork-Owned Behavior

Current intentional customizations include:

- workflow-oriented repository ordering and response review lifecycle;
- searchable repository filtering and compact status/header presentation;
- Gruvbox/Nerd Font status presentation and personal tmux controls;
- scoped hook maintenance notices and notification preferences;
- fork release automation and installed-runtime restart behavior.

Keep each behavior near its natural upstream seam. Do not create a generic
`fork` module or scatter `if fork` branches throughout the codebase.

## Change Classification

Every change should have one primary class:

| Class | Treatment |
|---|---|
| Upstream sync | Merge without unrelated fork edits; verify, then resolve only real conflicts |
| Personal policy | Keep as a focused fork commit with user-facing tests |
| Bug fix in shared behavior | Prefer an upstream-compatible implementation; consider contributing it upstream |
| Structural refactor | Avoid unless upstream already moved in that direction or a decision record justifies it |

Do not combine an upstream merge with opportunistic formatting or refactoring.
That destroys useful conflict history.

## Soft Divergence Budget

The budget triggers explanation, not automatic rejection. Add a decision record
under `docs/decisions/` when a change does any of the following:

- changes an entry point or the core data-flow direction;
- introduces a parallel abstraction for an upstream-owned subsystem;
- touches three or more hotspot modules from the architecture map;
- adds more than one new integration point for a single personalization;
- makes a future upstream version require preserving an old internal layout.

A decision record should contain: context, chosen seam, alternatives rejected,
upstream compatibility, conflict strategy, and removal path.

## Upstream Sync Procedure

1. Fetch both remotes and inspect divergence:

   ```bash
   git fetch --all --prune
   git rev-list --left-right --count HEAD...upstream/main
   ./scripts/fork-delta.sh upstream/main
   ```

2. Merge `upstream/main` as its own operation. Do not rebase published fork
   history and do not mix personal changes into the merge commit.
3. For each conflict, read the new upstream implementation and the fork intent.
   Preserve upstream ownership and reapply only the behavior that still matters.
4. Review upstream changes to `AGENTS.md` and port still-relevant repository facts
   into the fork's compact guide or progressively disclosed maintainer docs.
5. Run `./scripts/verify.sh full`.
6. Re-run `./scripts/fork-delta.sh upstream/main` and inspect newly touched
   skeleton files and top-churn hotspots.
7. Push only after the worktree and remote relationship are rechecked.

The scheduled GitHub workflow may merge conflict-free upstream updates. A
workflow conflict is a signal for manual semantic reconciliation, not a reason
to force one side. Successful automated syncs publish a `fork-<sha>` prerelease
for testing, while reviewed `v*-hza.*` releases remain the only `latest` lane.

## Conflict Resolution Rules

- Never resolve a hotspot with file-wide `ours` or `theirs`.
- Prefer upstream naming and file placement when both implementations are valid.
- Reapply fork behavior as a small follow-up edit if doing so keeps the merge
  itself understandable.
- Delete obsolete fork code when upstream now provides equivalent behavior.
- Add a regression test for any conflict whose correct resolution is not
  obvious from types or existing tests.

## Periodic Review

Use `./scripts/fork-delta.sh` after upstream syncs and before large fork work.
Review the fork when production churn grows much faster than net functionality,
or when the same hotspot conflicts repeatedly. The correct response is usually
to narrow the seam or upstream a shared fix, not to add another compatibility
layer.
