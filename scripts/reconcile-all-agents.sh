#!/bin/sh
# Reconcile every worktree of this repository to the main/dev branch policy.
#   scripts/reconcile-all-agents.sh            report only (default)
#   scripts/reconcile-all-agents.sh --apply    archive off-policy local branches as tags and
#                                              delete those with no worktree; sets core.hooksPath
# Never removes worktrees, never deletes remote branches, never discards uncommitted work.
set -eu
root=$(git rev-parse --show-toplevel)
cd "$root"
apply=0; [ "${1:-}" = "--apply" ] && apply=1
ALLOWED="main dev"

git config core.hooksPath .githooks
[ "$(git config core.hooksPath)" = ".githooks" ] || { echo "hooksPath not set"; exit 1; }

echo "== worktrees"
git worktree list --porcelain | awk '/^worktree /{w=$2} /^branch /{b=$2; sub("refs/heads/","",b); print w"\t"b} /^detached/{print w"\t(detached)"}' |
while IFS="$(printf '\t')" read -r wt br; do
  dirty=$(git -C "$wt" status --short 2>/dev/null | wc -l)
  case " $ALLOWED " in
    *" $br "*) state="ok" ;;
    *) state="OFF-POLICY" ;;
  esac
  echo "$state  $br  dirty=$dirty  $wt"
done

echo "== branch policy"
if [ "$apply" -eq 1 ]; then
  scripts/branch-policy.sh --apply || true
else
  scripts/branch-policy.sh || true
fi

echo "== summary"
echo "Off-policy worktrees need their owner to move commits to dev, archive the branch, and remove the worktree."
echo "Remote branches outside the policy need owner approval before deletion."
