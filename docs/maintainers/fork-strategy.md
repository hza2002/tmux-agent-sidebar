# Independent Fork Strategy

## Objective

This repository evolves independently from `hiroppy/tmux-agent-sidebar`.
Upstream is a reference and a source of selected fixes and features. Tracking
its latest release, preserving its internal architecture, and minimizing merge
conflicts are not product requirements.

Prioritize the owner's workflows, correctness, clear ownership, and long-term
maintenance. Independence does not justify unnecessary rewrites or abstractions.

## Design Order

Prefer configuration, existing extension seams, and focused leaf modules when
these fully solve the problem. Change core structure when that produces a
simpler, more correct, or more maintainable system. Assess total implementation,
runtime, migration, and maintenance costs rather than divergence from upstream.
Do not maintain parallel replacements for the same responsibility without a
specific reason and removal strategy.

## Current Architecture

The current dependency direction remains useful:

```text
CLI/TUI entry points
  -> adapter + internal event normalization
  -> handlers write tmux-owned state
  -> tmux query builds PaneInfo/SessionInfo
  -> AppState derives local UI state
  -> ui renders state and records hit targets
```

Keep raw agent parsing at the adapter boundary and durable writes out of
rendering. Preserve one bulk tmux query path. These are local correctness and
ownership rules, not obligations to upstream. A proposal to change them must
explain the replacement contract and update the architecture map.

## Change Classification

| Class | Treatment |
|---|---|
| Upstream import | Select and review useful changes against local behavior |
| Product policy or feature | Keep scope explicit and add proportional coverage |
| Bug fix | Fix the local contract; contribute upstream when useful |
| Structural refactor | Record the benefit, migration, and removal path |

Do not mix upstream imports with unrelated cleanup. Preserve attribution and
license notices for imported work.

## Decision Records

Add or update a record under `docs/decisions/` when a change alters an entry
point, core data flow, module ownership, or three or more architecture hotspots.
Include context, chosen seam, alternatives, compatibility with existing local
behavior, verification, migration, and rollback or removal. Upstream compatibility
is relevant only when the specific change requires it.

## Selective Upstream Imports

There is no scheduled upstream merge or automatic release after synchronization.

1. Fetch upstream when evaluating a particular fix or feature.
2. Record the upstream commit or PR and the local behavior it would improve.
   Read the patch and its tests against the current implementation. Use
   `git cherry-pick -x` for a direct import or cite the source commit in a
   focused port so a future maintainer can trace the change.
3. Choose a focused port, cherry-pick, or explicitly requested merge. Preserve
   local contracts; do not treat upstream structure as authoritative.
4. Add regression coverage and run `./scripts/verify.sh full` for production
   changes. Inspect the task diff; use `./scripts/fork-delta.sh upstream/main`
   when a broader comparison helps the review.
5. Commit, push, and release only within the user's authorized scope. Do not
   rebase published history as part of an import.

Never resolve hotspots with file-wide `ours` or `theirs` without semantic review.
Retain upstream remote access for reference; no automatic synchronization is
required. Reviewed version tags remain the release path.

## Periodic Review

Review complexity, duplicated responsibilities, regressions, runtime cost, and
whether features still serve the owner's workflow. Divergence size alone is not
a defect or a reason to rewrite working code.
