#!/bin/sh
# Reconcile every worktree of this repository to the main/dev branch policy.
#   scripts/reconcile-all-agents.sh                  report only (default)
#   scripts/reconcile-all-agents.sh --apply          archive and delete off-policy local branches with no worktree
#   scripts/reconcile-all-agents.sh --owner-approve  owner-approved run: also removes CLEAN off-policy
#                                                    worktrees and deletes off-policy REMOTE branches.
#                                                    Every affected branch is saved as a tag first.
# Worktrees with uncommitted changes are never removed. Root-owned entries are skipped.
set -eu
root=$(git rev-parse --show-toplevel)
cd "$root"
mode=report
case "${1:-}" in
  --apply) mode=apply ;;
  --owner-approve) mode=owner ;;
  "") ;;
  *) echo "usage: $0 [--apply|--owner-approve]"; exit 2 ;;
esac
ALLOWED="main dev"
on_policy() { case " $ALLOWED " in *" $1 "*) return 0 ;; esac; return 1; }

git config core.hooksPath .githooks
[ "$(git config core.hooksPath)" = ".githooks" ] || { echo "hooksPath not set"; exit 1; }
git fetch -q origin --prune || echo "WARN: fetch failed; remote state may be stale"

echo "== worktrees"
git worktree list --porcelain | awk '/^worktree /{w=$2} /^branch /{b=$2; sub("refs/heads/","",b); print w"\t"b} /^detached/{print w"\t(detached)"}' > /tmp/claude-1000/wt-list.$$
while IFS="$(printf '\t')" read -r wt br; do
  dirty=$(git -C "$wt" status --short 2>/dev/null | wc -l)
  if on_policy "$br"; then state="ok"; else state="OFF-POLICY"; fi
  echo "$state  $br  dirty=$dirty  $wt"
  if [ "$mode" = owner ] && [ "$state" = "OFF-POLICY" ] && [ "$br" != "(detached)" ]; then
    if [ "$dirty" -ne 0 ]; then echo "  skip removal: uncommitted changes"; continue; fi
    git tag -f "archive/$br" "$br" >/dev/null
    if git worktree remove "$wt" 2>/dev/null; then echo "  removed worktree (branch saved as archive/$br)"; else echo "  could not remove worktree (locked or not owned)"; fi
  fi
done < /tmp/claude-1000/wt-list.$$
rm -f /tmp/claude-1000/wt-list.$$
git worktree prune 2>/dev/null || true

echo "== local branches"
if [ "$mode" != report ]; then
  scripts/branch-policy.sh --apply || true
else
  scripts/branch-policy.sh || true
fi

if [ "$mode" = owner ]; then
  echo "== remote branches (owner-approved deletion)"
  for r in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin); do
    name=${r#origin/}; [ "$name" = HEAD ] && continue; [ "$name" = origin ] && continue
    on_policy "$name" && continue
    flat=$(echo "$name" | tr "/" "_"); git tag -f "archive/remote-$flat" "origin/$name" >/dev/null
    if git push -q origin --delete "$name"; then echo "deleted origin/$name (saved as archive/remote-$flat)"; else echo "FAILED origin/$name"; fi
  done
fi

echo "== summary"
[ "$mode" = report ] && echo "Report only. Run with --apply or --owner-approve to change anything."
exit 0
