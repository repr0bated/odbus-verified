#!/bin/sh
# Install a Claude Code SessionStart hook in every worktree of this repository.
# The hook runs scripts/branch-policy-session.sh at session start, which reconciles
# off-policy local branches (archived as tags first) and reports the rest to the agent.
#   scripts/install-session-reconcile.sh            dry run: show what would change
#   scripts/install-session-reconcile.sh --install   write .claude/settings.local.json in each worktree
# Existing settings in each file are kept; only the SessionStart hook is added or replaced.
set -eu
root=$(git rev-parse --show-toplevel)
cd "$root"
install=0; [ "${1:-}" = "--install" ] && install=1
cmd="$root/scripts/branch-policy-session.sh"
[ -x "$cmd" ] || { echo "missing $cmd"; exit 1; }

git worktree list --porcelain | awk '/^worktree /{print $2}' | while read -r wt; do
  f="$wt/.claude/settings.local.json"
  new=$(jq -n --arg c "$cmd" '{hooks:{SessionStart:[{hooks:[{type:"command",command:$c,statusMessage:"Reconciling branch policy"}]}]}}')
  if [ -f "$f" ]; then
    merged=$(jq --argjson h "$new" '.hooks = ((.hooks // {}) + $h.hooks)' "$f")
  else
    merged="$new"
  fi
  if [ "$install" -eq 1 ]; then
    mkdir -p "$wt/.claude"
    printf '%s\n' "$merged" > "$f"
    echo "installed: $f"
  else
    echo "would install: $f"
  fi
done
[ "$install" -eq 1 ] || echo "dry run. Re-run with --install to write."
