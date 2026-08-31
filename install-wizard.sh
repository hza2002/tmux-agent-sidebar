#!/usr/bin/env bash

set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$PLUGIN_DIR/target/release/tmux-agent-sidebar"
action="${1:-}"

function finish {
    local exit_code=$?
    # When run without arguments (interactive menu), the menu spawns a
    # new-window with the action — that child process handles the reload.
    if [[ -z "$action" ]]; then
        exit $exit_code
    fi
    if [[ $exit_code -eq 0 ]]; then
        echo "Reloading tmux.conf"
        tmux source ~/.tmux.conf
        exit 0
    else
        echo "Something went wrong. Press any key to close this window."
        read -n 1
        exit 1
    fi
}
trap finish EXIT

function stop_running_instances() {
    # Kill any running instances so the next launch picks up the new binary.
    # Match the full binary path to avoid touching unrelated processes.
    pkill -f "$BINARY" 2>/dev/null || true
}

function post_install_fixups() {
    # Keep the local Cargo artifact executable under macOS Gatekeeper.
    if [[ "$(uname -s)" == "Darwin" ]]; then
        xattr -d com.apple.provenance "$BINARY" 2>/dev/null || true
        xattr -d com.apple.quarantine "$BINARY" 2>/dev/null || true
        codesign --force --sign - "$BINARY" >/dev/null 2>&1 || true
    fi

    stop_running_instances
}

function build_from_source() {
    echo "Building from source..."

    if ! command -v cargo &>/dev/null; then
        echo "Rust is not installed. Please install it first."
        echo ""
        echo "  https://rustup.rs/"
        echo ""
        return 1
    fi

    cargo build --release --manifest-path "$PLUGIN_DIR/Cargo.toml"

    post_install_fixups

    echo "Build complete!"
}

# Direct action dispatch
case "$action" in
    build-from-source)
        build_from_source
        exit $?
        ;;
esac

# Interactive menu
function get_message() {
    if [[ "${SIDEBAR_UPDATE:-}" == "1" ]]; then
        echo "The local source changed. Rebuild the sidebar binary."
    else
        echo "Build tmux-agent-sidebar from this local working copy."
    fi
}

tmux display-menu -T "tmux-agent-sidebar" \
    "" \
    "- " "" "" \
    "-  #[nodim,bold]tmux-agent-sidebar" "" "" \
    "- " "" "" \
    "-  $(get_message) " "" "" \
    "- " "" "" \
    "" \
    "Build local release binary" b "new-window \"$PLUGIN_DIR/install-wizard.sh build-from-source\"" \
    "" \
    "Exit" q ""
