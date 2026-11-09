#!/bin/sh
# SessionStart hook: reconcile off-policy local branches, then tell the agent what remains.
# Local branches are archived as tags before deletion. Worktrees and remote branches are never touched.
# The policy script and hooks are taken from the main repository, because older branches may lack them.
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
main_repo=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
cd "$root"
git config core.hooksPath "$main_repo/.githooks" 2>/dev/null || true
report=$("$main_repo/scripts/branch-policy.sh" --apply 2>&1 || true)
msg="Branch policy (owner, 2026-11-08): only main and dev may be used. This session ran the branch policy check with --apply, which archived stale local branches as tags. Remaining items, reported by the check:
$(printf '%s\n' "$report" | sed 's/^/  /')
Do not work on any other branch. For a VIOLATION held by a worktree, move your commits to dev, archive the branch, and remove your worktree. Remote branch deletion needs owner approval."
jq -n --arg m "$msg" '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $m}}'
