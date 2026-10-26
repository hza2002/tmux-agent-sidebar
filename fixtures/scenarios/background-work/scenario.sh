#!/usr/bin/env bash
# Isolated reply-ready and reviewed background-work showcase.
set -euo pipefail
OUT="${1:?usage: scenario.sh <output-dir> [extra capture args...]}"
shift
EXTRA_ARGS=("$@")
source "$(cd "$(dirname "$0")/../common" && pwd)/_lib.sh"
export FOCUS=PANE_WAITING
export BOTTOM_HEIGHT=0
export CROP_ROWS=0:32
export CROP_COLS=0:46
setup "background-work"
trap cleanup EXIT
mkdir -p "$OUT"
build_layout
paint_stream "$MAIN_PANE" "$ROOT/fixtures/scenarios/hero/main-pane.stream"
run_fake_agent "$PANE_WAITING" claude
tmux set-option -p -t "$PANE_WAITING" @pane_agent claude
tmux set-option -p -t "$PANE_WAITING" @pane_wait_reason response_ready
tmux set-option -p -t "$PANE_WAITING" @pane_subagents 'Explore:review1'
tmux set-option -p -t "$PANE_WAITING" @pane_prompt 'Review ready; integration tests still running'
tmux set-option -p -t "$PANE_WAITING" @pane_prompt_source response
tmux set-option -p -t "$PANE_WAITING" @pane_background_tasks '[{"id":"build","kind":"shell","label":"cargo test --workspace"},{"id":"ci","kind":"workflow","label":"Integration checks"}]'
tmux set-option -p -t "$PANE_BACKGROUND" @pane_subagents 'Plan:plan1'
start_sidebar
capture_single
