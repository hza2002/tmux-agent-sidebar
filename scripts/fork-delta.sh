#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
baseline="${1:-upstream/main}"
cd "$repo_root"

if ! git rev-parse --verify --quiet "${baseline}^{commit}" >/dev/null; then
  echo "baseline not found: $baseline" >&2
  echo "fetch or add the upstream remote, then retry" >&2
  exit 2
fi

merge_base="$(git merge-base HEAD "$baseline")"
read -r ahead behind < <(git rev-list --left-right --count "HEAD...$baseline")

echo "Fork delta"
echo "  baseline:   $baseline"
echo "  merge base: $merge_base"
echo "  ahead:      $ahead"
echo "  behind:     $behind"
echo

git diff --shortstat "$merge_base..HEAD"

skeleton=(
  src/main.rs
  src/cli/mod.rs
  src/adapter
  src/event.rs
  src/event
  src/tmux/query.rs
  src/tmux/types.rs
  src/app.rs
  src/state.rs
  src/state/refresh.rs
  src/ui/mod.rs
)

echo
echo "Changed skeleton touchpoints (committed)"
skeleton_changed=false
for path in "${skeleton[@]}"; do
  if ! git diff --quiet "$merge_base..HEAD" -- "$path"; then
    echo "  $path"
    skeleton_changed=true
  fi
done
if [[ "$skeleton_changed" == false ]]; then
  echo "  (none)"
fi

echo
echo "Top production churn"
git diff --numstat "$merge_base..HEAD" -- src \
  | awk -F '\t' '{ print $1 + $2 "\t+" $1 "\t-" $2 "\t" $3 }' \
  | sort -nr \
  | sed -n '1,15p'

echo
echo "Uncommitted production changes"
worktree_changes="$(git status --short --untracked-files=all -- src)"
if [[ -n "$worktree_changes" ]]; then
  printf '%s\n' "$worktree_changes"
else
  echo "  (none)"
fi

echo
echo "This report is advisory and does not fetch remotes."
