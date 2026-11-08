#!/bin/sh
# SessionStart hook: reconcile off-policy local branches, then tell the agent what remains.
# Local branches are archived as tags before deletion. Worktrees and remote branches are never touched.
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
cd "$root"
git config core.hooksPath .githooks 2>/dev/null || true
report=$(scripts/branch-policy.sh --apply 2>&1 || true)
msg="Branch policy (owner, 2026-11-08): only main and dev may be used. This session ran scripts/branch-policy.sh --apply, which archived stale local branches as tags. Remaining items, reported by the check:
$(printf '%s\n' "$report" | sed 's/^/  /')
Do not work on any other branch. For a VIOLATION held by a worktree, move your commits to dev, archive the branch, and remove your worktree. Remote branch deletion needs owner approval."
jq -n --arg m "$msg" '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $m}}'
