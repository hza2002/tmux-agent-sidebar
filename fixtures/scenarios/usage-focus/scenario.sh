#!/usr/bin/env bash
# Deterministic quota/usage UI, isolated from real accounts and logs.
set -euo pipefail
OUT="${1:?usage: scenario.sh <output-dir>}"
EXTRA_ARGS=()
source "$(cd "$(dirname "$0")/../common" && pwd)/_lib.sh"
setup "usage-focus"
trap cleanup EXIT
mkdir -p "$OUT"
# Locate a field in the current terminal cells, so resize/mode changes cannot
# accidentally exercise an old hard-coded column.
click_field() {
    local label="$1" value="$2" col row input
    read -r col row < <(tmux capture-pane -p -t "$SIDEBAR_PANE" | python3 -c '
import sys, re, unicodedata
label, value = sys.argv[1:]
def width(text):
    return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in text)
for y, line in enumerate(sys.stdin):
    if re.match(label, line):
        alternatives = {
            "18.2M": ["18.2M", "18,200,000"],
            "12.3M": ["12.3M", "12,300,000"],
            "1.8M": ["1.8M", "1,757,142"],
            "2.6M": ["2.6M", "2,600,000"],
        }.get(value, [value])
        value = next(candidate for candidate in alternatives if candidate in line)
        start = line.index(value)
        print(width(line[:start]) + max(1, width(value) // 2) + 1, y + 1)
        break
else:
    raise SystemExit("usage field not rendered")
' "$label" "$value")
    printf -v input '\033[<0;%s;%sM' "$col" "$row"
    tmux send-keys -t "$SIDEBAR_PANE" -l "$input"
    sleep 0.2
}
for mode in empty empty-narrow zero empty-cycle full narrow wide daily both-daily both-daily-narrow click compact off-peak error error-narrow history-error history-error-narrow history-error-popup history-import; do
    width=35
    height=20
    arg=""
    if [[ "$mode" == empty || "$mode" == zero || "$mode" == empty-cycle ]]; then arg="$mode"; fi
    if [[ "$mode" == empty-narrow ]]; then width=28; arg="empty"; fi
    if [[ "$mode" == narrow ]]; then width=28; height=12; fi
    if [[ "$mode" == wide ]]; then width=60; arg="wide"; fi
    if [[ "$mode" == daily ]]; then arg="daily"; fi
    if [[ "$mode" == both-daily ]]; then arg="both-daily"; fi
    if [[ "$mode" == both-daily-narrow ]]; then width=28; arg="both-daily"; fi
    if [[ "$mode" == click ]]; then arg="click"; fi
    if [[ "$mode" == compact ]]; then width=28; height=5; arg="compact"; fi
    if [[ "$mode" == off-peak ]]; then arg="off-peak"; fi
    if [[ "$mode" == error ]]; then arg="error"; fi
    if [[ "$mode" == error-narrow ]]; then width=28; arg="error"; fi
    if [[ "$mode" == history-error ]]; then arg="history-error"; fi
    if [[ "$mode" == history-error-narrow ]]; then width=28; arg="history-error"; fi
    if [[ "$mode" == history-error-popup ]]; then width=40; arg="history-error-popup"; fi
    if [[ "$mode" == history-import ]]; then arg="history-import"; fi
    tmux -f "$TMUX_CONF" new-session -d -s "$SESSION" -x "$width" -y "$height" -c "$ROOT"
    WINDOW=$(tmux display-message -t "$SESSION" -p '#{window_id}')
    SIDEBAR_PANE=$(tmux list-panes -t "$WINDOW" -F '#{pane_id}' | head -n 1)
    tmux respawn-pane -k -t "$SIDEBAR_PANE" "env -u NO_COLOR '$ROOT/target/release/examples/usage_preview' $arg"
    sleep 1
    if [[ "$mode" == click ]]; then
        click_field '^ *cx7d .*账号' '$128.40'
        account_row=$(tmux capture-pane -p -t "$SIDEBAR_PANE" | rg 'cx7d .*账号')
        [[ "$account_row" == *'$18.34'*'2.6M'* ]]
        all_row=$(tmux capture-pane -p -t "$SIDEBAR_PANE" | rg 'cx7d .*全部')
        [[ "$all_row" == *'$128.40'*'18.2M'* ]]
        click_field '^ *ds7d ' '¥1.47'
    fi
    tmux capture-pane -p -e -t "$SIDEBAR_PANE" > "$OUT/usage-$mode.ansi"
    tmux capture-pane -p -t "$SIDEBAR_PANE" > "$OUT/usage-$mode.txt"
    "$BIN" capture --session "$SESSION" --window "$WINDOW" --frame-out "$OUT/usage-$mode.html"
    if [[ "$mode" == click ]]; then
        ds_row=$(rg 'ds7d' "$OUT/usage-click.txt")
        [[ "$ds_row" == *'¥0.21'*'1.8M'* ]]
        tmux resize-window -t "$WINDOW" -x 45
        sleep 0.2
        click_field '^ *cx7d .*账号' '2.6M'
        "$BIN" capture --session "$SESSION" --window "$WINDOW" --frame-out "$OUT/usage-click-wide.html"
        tmux resize-window -t "$WINDOW" -x 28
        sleep 0.2
        tmux capture-pane -p -t "$SIDEBAR_PANE" > "$OUT/usage-click-narrow.txt"
        click_field '^ *cx7d .*全部' '$128.40'
        tmux capture-pane -p -t "$SIDEBAR_PANE" > "$OUT/usage-click-narrow.txt"
        recent_row=$(rg 'cx7d .*全部' "$OUT/usage-click-narrow.txt")
        [[ "$recent_row" == *'$18.34'*'2.6M'* ]]
        "$BIN" capture --session "$SESSION" --window "$WINDOW" --frame-out "$OUT/usage-click-narrow.html"
        click_field '^ *cx7d .*全部' '2.6M'
        click_field '^ *ds7d ' '1.8M'
        tmux capture-pane -p -t "$SIDEBAR_PANE" > "$OUT/usage-click-restored.txt"
        ds_row=$(rg 'ds7d' "$OUT/usage-click-restored.txt")
        [[ "$ds_row" == *'¥1.47'*'12.3M'* && "$ds_row" != *均* ]]
        recent_row=$(rg 'cx7d .*全部' "$OUT/usage-click-restored.txt")
        [[ "$recent_row" == *'$128.40'*'18.2M'* && "$recent_row" != *均* ]]
    fi
    tmux kill-session -t "$SESSION"
done
