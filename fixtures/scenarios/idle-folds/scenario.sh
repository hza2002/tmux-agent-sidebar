#!/usr/bin/env bash
# Exercise idle folding through the real keyboard path on an isolated server.
set -euo pipefail
OUT="${1:?usage: scenario.sh <output-dir>}"
EXTRA_ARGS=()
source "$(cd "$(dirname "$0")/../common" && pwd)/_lib.sh"
export FOCUS=SIDEBAR_PANE
export BOTTOM_HEIGHT=0
setup "idle-folds"
trap cleanup EXIT
mkdir -p "$OUT"
build_layout
paint_stream "$MAIN_PANE" "$ROOT/fixtures/scenarios/hero/main-pane.stream"

# One idle pane in the active repository, plus a fully idle repository.
idle_window=$(tmux new-window -d -t "$SESSION" -n idle -c "$ROOT" -P -F '#{window_id}')
mixed_idle=$(tmux display-message -p -t "$idle_window" '#{pane_id}')
_seed_pane "$mixed_idle" agent=claude status=idle prompt="Reviewed implementation notes"
run_fake_agent "$mixed_idle" claude
mkdir -p "$TMUX_DIR/archive-project"
git -C "$TMUX_DIR/archive-project" init -q
archive_first=$(tmux split-window -h -t "$idle_window" -c "$TMUX_DIR/archive-project" -P -F '#{pane_id}')
archive_second=$(tmux split-window -v -t "$archive_first" -c "$TMUX_DIR/archive-project" -P -F '#{pane_id}')
for pane in "$archive_first" "$archive_second"; do
    _seed_pane "$pane" agent=claude status=idle prompt="Reviewed archived conversation"
    run_fake_agent "$pane" claude
done
tmux set-option -p -t "$PANE_WAITING" @pane_wait_reason permission_prompt
tmux set-option -p -t "$PANE_WAITING" @pane_status_changed_at "$(($(date +%s) * 1000 - 45000))"
tmux set-option -p -t "$PANE_ERROR" @pane_status_changed_at "$(($(date +%s) * 1000 - 120000))"
tmux set-option -g @sidebar_quota off
tmux select-window -t "$WINDOW"
tmux select-pane -t "$SIDEBAR_PANE"
start_sidebar

capture_named() {
    "$BIN" capture --session "$SESSION" --window "$WINDOW" \
        --frame-out "$OUT/$1.html" --crop-rows 0:46 --crop-cols "0:${2:-46}"
}
capture_named idle-expanded
tmux send-keys -t "$SIDEBAR_PANE" z
sleep 0.3
capture_named idle-collapsed

# The fully idle repository is the final workflow group. Enter must expand it
# without switching to an agent pane, and the next global toggle expands all.
tmux send-keys -t "$SIDEBAR_PANE" G Enter
sleep 0.3
capture_named idle-local-expanded
[[ "$(tmux display-message -t "$WINDOW" -p '#{pane_id}')" == "$SIDEBAR_PANE" ]]
# Expanded headers leave keyboard navigation but remain mouse fold targets.
click_archive_header() {
    local row input
    row=$(tmux capture-pane -p -t "$SIDEBAR_PANE" | awk '/^archive-project/ {print NR; exit}')
    [[ -n "$row" ]]
    printf -v input '\033[<0;2;%sM' "$row"
    tmux send-keys -t "$SIDEBAR_PANE" -l "$input"
    sleep 0.3
}
click_archive_header
tmux capture-pane -p -t "$SIDEBAR_PANE" | rg '^archive-project.*▸'
capture_named idle-mouse-collapsed
click_archive_header
if tmux capture-pane -p -t "$SIDEBAR_PANE" | rg '^archive-project.*▸'; then
    echo "archive header remained collapsed after mouse expansion" >&2
    exit 1
fi
[[ "$(tmux display-message -t "$WINDOW" -p '#{pane_id}')" == "$SIDEBAR_PANE" ]]
tmux send-keys -t "$SIDEBAR_PANE" z
sleep 0.3
capture_named idle-global-expanded
tmux send-keys -t "$SIDEBAR_PANE" z
sleep 0.3

# A newly actionable pane must be visible despite the collapsed preference.
tmux set-option -p -t "$archive_first" @pane_status waiting
tmux set-option -p -t "$archive_first" @pane_wait_reason permission_prompt
tmux set-option -p -t "$archive_first" @pane_attention notification
tmux set-option -p -t "$archive_first" @pane_status_changed_at "$(($(date +%s) * 1000 - 5000))"
sleep 1.5
capture_named idle-new-attention
tmux set-option -p -t "$archive_second" @pane_status waiting
tmux set-option -p -t "$archive_second" @pane_wait_reason response_ready
tmux set-option -p -t "$archive_second" @pane_attention notification
tmux set-option -p -t "$archive_second" @pane_status_changed_at "$(($(date +%s) * 1000 - 90000))"
sleep 1.5
capture_named workflow-timers
tmux resize-pane -t "$SIDEBAR_PANE" -x 28
sleep 1.5
capture_named idle-narrow 28
