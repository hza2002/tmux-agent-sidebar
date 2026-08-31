#!/usr/bin/env bash
# Thin wrapper: delegates to the Rust binary. Called by Claude Code /
# Codex hooks (settings.json).
#
# Why this file exists even though `tmux-agent-sidebar setup` can emit
# absolute binary paths:
#
# 1. Late binding. settings.json only needs to know where `hook.sh`
#    lives. The actual binary is resolved fresh on every hook fire, so
#    the user can rebuild the local release binary or relocate the plugin
#    directory without having to regenerate their agent config. Without this
#    indirection, any setup-generated path becomes a stale snapshot the moment
#    the working copy moves.
#
# 2. Graceful absence. If the binary is missing — during a rebuild,
#    mid-uninstall, or on a fresh clone before `cargo build` — this
#    script exits 0 silently, so the agent session never sees a hook
#    failure. A direct binary invocation would surface "no such file"
#    errors into the user's workflow.
#
# Keep this wrapper small and side-effect-free. Any logic that needs to
# know event semantics belongs in the Rust `hook` subcommand.
PLUGIN_DIR="$(cd "$(dirname "$0")" && pwd -P)"
# Fallback location used when this script is executed from a Claude Code
# plugin install (e.g. `${CLAUDE_PLUGIN_ROOT}/hook.sh`). The plugin cache
# never contains the binary, so hop over to the tmux plugin directory
# where TPM placed it.
XDG_TPM_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/tmux/plugins/tmux-agent-sidebar"
LEGACY_TPM_DIR="$HOME/.tmux/plugins/tmux-agent-sidebar"
if [ -x "$PLUGIN_DIR/target/release/tmux-agent-sidebar" ]; then
  BIN="$PLUGIN_DIR/target/release/tmux-agent-sidebar"
elif [ -x "$XDG_TPM_DIR/target/release/tmux-agent-sidebar" ]; then
  BIN="$XDG_TPM_DIR/target/release/tmux-agent-sidebar"
elif [ -x "$LEGACY_TPM_DIR/target/release/tmux-agent-sidebar" ]; then
  BIN="$LEGACY_TPM_DIR/target/release/tmux-agent-sidebar"
else
  exit 0
fi
exec "$BIN" hook "$@"
