---
title: Installation
description: Run the maintained fork directly from its local working copy.
---

## Requirements

- tmux 3.0+
- [GitHub CLI](https://cli.github.com/) (optional, for displaying PR numbers in the Git tab)
- [Rust](https://rustup.rs/)

## Local working copy

Load the maintained working copy directly from `tmux.conf`:

```bash
run-shell '~/.config/tmux/plugins/tmux-agent-sidebar/tmux-agent-sidebar.tmux'
```

Clone the fork, point the local plugin path at that maintained checkout, and build
the release binary locally:

```sh
git clone git@github.com:hza2002/tmux-agent-sidebar.git \
  ~/repo/archives/tmux-agent-sidebar
mkdir -p ~/.config/tmux/plugins
ln -s ~/repo/archives/tmux-agent-sidebar \
  ~/.config/tmux/plugins/tmux-agent-sidebar
cd ~/repo/archives/tmux-agent-sidebar
cargo build --release
tmux source ~/.config/tmux/tmux.conf
```

The launcher and agent hooks use only
`target/release/tmux-agent-sidebar`. They never query GitHub Releases, download
a binary, or fall back to a stale `bin/` artifact.

After local code changes, rebuild and restart existing sidebar panes:

```sh
cargo build --release
target/release/tmux-agent-sidebar restart-sidebars
```

Push maintained fork changes to `origin`. The `upstream` remote exists only for
fetching and merging changes from `hiroppy/tmux-agent-sidebar`; it is not a
runtime update source.

## Reload tmux config

After editing `tmux.conf`, press `prefix + r` (or run `tmux source ~/.tmux.conf`) to reload.

## Next steps

The sidebar receives status updates through agent hooks — continue with the agent you use:

- [Claude Code setup](/tmux-agent-sidebar/getting-started/claude-code/)
- [Codex setup](/tmux-agent-sidebar/getting-started/codex/)
- [OpenCode setup](/tmux-agent-sidebar/getting-started/opencode/)
