---
name: version-release
description: Prepare or perform a tmux-agent-sidebar version bump, release commit, and version tag. Use for release or tag requests; require explicit authorization before committing, tagging, or pushing.
---

# Version Release

Keep `Cargo.toml`, `Cargo.lock`, and `.claude-plugin/plugin.json` synchronized.
The plugin manifest version controls Claude Code marketplace updates, and
`tests/plugin_hooks_tests.rs` enforces equality with the Cargo version.

## Determine The Version

1. Read the current version from `Cargo.toml` and the latest `v*` tag.
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
