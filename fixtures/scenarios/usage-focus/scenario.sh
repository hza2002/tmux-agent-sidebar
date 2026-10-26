#!/usr/bin/env bash
# Deterministic quota/usage UI, isolated from real accounts and logs.
set -euo pipefail
OUT="${1:?usage: scenario.sh <output-dir>}"
EXTRA_ARGS=()
source "$(cd "$(dirname "$0")/../common" && pwd)/_lib.sh"
setup "usage-focus"
trap cleanup EXIT
mkdir -p "$OUT"
for mode in full narrow details; do
    width=35
    height=20
    arg=""
    if [[ "$mode" == narrow ]]; then width=28; height=12; fi
    if [[ "$mode" == details ]]; then width=40; height=22; arg="details"; fi
    tmux -f "$TMUX_CONF" new-session -d -s "$SESSION" -x "$width" -y "$height" -c "$ROOT"
    WINDOW=$(tmux display-message -t "$SESSION" -p '#{window_id}')
    SIDEBAR_PANE=$(tmux list-panes -t "$WINDOW" -F '#{pane_id}' | head -n 1)
    tmux respawn-pane -k -t "$SIDEBAR_PANE" "env -u NO_COLOR '$ROOT/target/release/examples/usage_preview' $arg"
    sleep 1
    tmux capture-pane -p -e -t "$SIDEBAR_PANE" > "$OUT/usage-$mode.ansi"
    tmux capture-pane -p -t "$SIDEBAR_PANE" > "$OUT/usage-$mode.txt"
    "$BIN" capture --session "$SESSION" --window "$WINDOW" --frame-out "$OUT/usage-$mode.html"
    tmux kill-session -t "$SESSION"
done
