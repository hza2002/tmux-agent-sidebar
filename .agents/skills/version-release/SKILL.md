---
name: version-release
description: Prepare or perform a tmux-agent-sidebar version bump, release commit, and version tag. Use for release or tag requests; require explicit authorization before committing, tagging, or pushing.
---

# Version Release

Keep `Cargo.toml`, `Cargo.lock`, and `.claude-plugin/plugin.json` synchronized.
The plugin manifest version controls Claude Code marketplace updates, and
`tests/plugin_hooks_tests.rs` enforces equality with the Cargo version.

## Determine The Version

1. Read the current version from `Cargo.toml` and the latest published fork
   release. Resolve the repository from `git remote get-url origin` and pass
   `--repo <owner/repository>` to every `gh` command; fork checkouts can default
   to upstream. Do not use an automatic `fork-*` tag as the stable baseline.
2. Review every commit since that tag.
3. Propose the next version before editing:

| Commit range | Pre-1.0 bump |
|---|---|
| Any feature or breaking behavior | minor |
| Fixes, refactors, chores, tests, or docs only | patch |
| Empty range | stop; there is nothing to release |

Ask when classification is genuinely ambiguous. The major version remains `0`
until the project explicitly adopts 1.0.

## Prepare

1. Update the version in `Cargo.toml` and `.claude-plugin/plugin.json`.
2. Run `cargo check` to update `Cargo.lock`.
3. Run `./scripts/verify.sh full`.
4. Confirm the three version surfaces still agree and inspect the release diff.

## Publish

Commit, create `v<version>`, and push the branch/tag only when the current user
request explicitly authorizes those actions. Recheck `HEAD`, worktree status,
and remote state immediately before each public Git operation.


## Verify Delivery

- Fetch/read origin before publishing; reconcile remote changes without dropping
  local work. Keep upstream synchronization separate from feature commits.
- A pushed tag triggers the release workflow; it does not prove publication.
  Check CI and Release workflow status for the exact commit, then inspect the
  release tag, stable lane and all four expected platform assets. Treat network
  failures as unknown state and read back refs before retrying a write.
- Keep source, published assets, installed binary and running process status
  distinct in the handoff. Building or publishing does not restart a sidebar.
- Documentation/test/harness-only closeout commits do not require retagging an
  already-published release. Never move an existing release tag.
