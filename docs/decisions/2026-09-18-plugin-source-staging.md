# Decision: Stage a slim copy for the Claude Code plugin source

## Context

The marketplace entry installed the plugin in Claude Code's **link mode**: the
command printed this checkout's path and Claude Code served the plugin in place
from it. That kept the cache at a few kilobytes and made edits to `hook.sh` and
`hooks/hooks.json` live without `/plugin update`.

Claude Code does not load a plugin served in place when the session's working
directory is the plugin's source path or below it. The maintainer's Claude Code
sessions for this fork start in this repository, so the plugin loaded in no
session that mattered: `/plugin` reported `failed to load`, `/hooks` reported
zero hooks, no hook wrote `@pane_*` state, and the sidebar showed no Claude
panes. The rule is a pure function of the install record and the session
working directory in 2.1.273 — no setting, flag, or sandbox setting can allow
it, and `/plugin update` cannot clear it.

## Chosen Seam

The entry keeps its `command` source but stages a slim copy instead of printing
the checkout: it copies `.claude-plugin/`, `hooks/`, and `hook.sh` into
`~/.cache/tmux-agent-sidebar/plugin` and prints that path. Claude Code's default
for a command source is copy mode, so the plugin is served from its own cache
and the source path never contains a working directory.

`hook.sh` stays a thin wrapper, so hooks keep running the live
`target/release/tmux-agent-sidebar` through the tmux plugin directory it falls
back to. Only the wrapper and the hook declarations are copies, and the sidebar
already reports that drift: `plugin_state` compares the cached `hook.sh` and
`hooks/hooks.json` against the copies embedded in the running binary and raises
the Stale notice, which tells the maintainer to run `/plugin update`.

## Alternatives Rejected

- **Revert to upstream's `source: "./"`.** Works, but copies the whole repository
  — 9.6 GB and 310k entries, `target/` included — into Claude Code's cache on
  every install and update. That is the cost this fork set out to remove.
- **Drop the plugin and register the hooks in `~/.claude/settings.json`.** No
  cache and no install machinery, but it diverges from upstream's documented
  install path, requires re-pasting the generated block whenever a hook
  declaration changes, and leaves the plugin detection in `state/notices.rs`
  reporting an install that deliberately does not exist.
- **`claude --plugin-dir <checkout>` per session.** Loads without an install
  record and copies nothing, but it must be passed on every launch, the sidebar
  still reports the plugin as not installed, and a launch that forgets the flag
  loses every hook silently — the same failure class as the bug this record
  fixes.
- **Keep link mode and never start Claude Code in this repository.** This is the
  maintainer's main working directory for the fork.

## Upstream Compatibility

Only `.claude-plugin/marketplace.json` and the README's plugin section differ
from upstream, plus the maintainer note in `AGENTS.md`. Upstream's entry
(`"source": "./"`) stays valid for upstream users; this entry is fork-owned
install policy alongside the local-source runtime decision. The Rust
application, hook declarations, and adapters are untouched.

## Conflict and Removal Strategy

During upstream merges, preserve upstream's new marketplace or README structure
and reapply this entry through the same seam; do not accept either side
wholesale. To remove this policy, restore `"source": "./"` or link mode, delete
this record, and drop the maintainer note from `AGENTS.md`; `plugin_state` needs
no change, because it already treats a copy-mode cache as the stale-detection
baseline.
