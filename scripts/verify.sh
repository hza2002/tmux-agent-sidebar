#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode="${1:-full}"

case "$mode" in
  quick | full) ;;
  *)
    echo "usage: $0 [quick|full]" >&2
    exit 2
    ;;
esac

cd "$repo_root"

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

if [[ "$mode" == "quick" ]]; then
  exit 0
fi

tmp_base="${TMPDIR:-/tmp}"
test_prefix="${tmp_base%/}/tmux-agent-sidebar-test."
test_tmux_dir="$(mktemp -d "${test_prefix}XXXXXX")"

cleanup() {
  case "$test_tmux_dir" in
    "$test_prefix"*)
      rm -rf -- "$test_tmux_dir"
      ;;
    *)
      echo "refusing to remove unexpected temporary path: $test_tmux_dir" >&2
      ;;
  esac
}
trap cleanup EXIT

mkdir -p "$test_tmux_dir/activity" "$test_tmux_dir/tmp" "$test_tmux_dir/data" "$test_tmux_dir/state"
# Child sidebar processes must never initialize the developer's usage history.
export XDG_DATA_HOME="$test_tmux_dir/data"
export XDG_STATE_HOME="$test_tmux_dir/state"

echo "==> cargo test (isolated tmux: $test_tmux_dir)"
env -u TMUX -u TMUX_PANE \
  TMPDIR="$test_tmux_dir/tmp" \
  TMUX_TMPDIR="$test_tmux_dir" \
  TMUX_AGENT_ACTIVITY_DIR="$test_tmux_dir/activity" \
  cargo test

echo "==> cargo test (real-tmux lifecycle, isolated socket)"
# The #[ignore]d lifecycle tests exercise toggle/maintain against a real tmux
# server on its own socket. Skipping them let a tmux output-format drift
# (empty pane_pid for command-less panes) silently break slot creation.
env -u TMUX -u TMUX_PANE \
  TMPDIR="$test_tmux_dir/tmp" \
  TMUX_TMPDIR="$test_tmux_dir" \
  cargo test --test sidebar_lifecycle_tests -- --ignored

echo "==> cargo test (Codex routing, isolated tmux and fake daemon)"
env -u TMUX -u TMUX_PANE \
  TMPDIR="$test_tmux_dir/tmp" \
  TMUX_TMPDIR="$test_tmux_dir" \
  TMUX_AGENT_ACTIVITY_DIR="$test_tmux_dir/activity" \
  cargo test --test codex_daemon_routing_tests -- --ignored

echo "==> cargo build --release"
cargo build --release
