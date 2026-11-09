#!/bin/sh
# Branch policy: only main and dev may be used.
#   scripts/branch-policy.sh           check and report (exit 1 on violations)
#   scripts/branch-policy.sh --apply   archive off-policy branches as tags and delete
#                                      those with no worktree; worktrees are never removed
set -eu
ALLOWED="main dev"
root=$(git rev-parse --show-toplevel)
cd "$root"
apply=0; [ "${1:-}" = "--apply" ] && apply=1

hooks=$(git config core.hooksPath || true)
case "$hooks" in */.githooks|.githooks) ;; *) echo "WARN: core.hooksPath is '$hooks' (expected an absolute path to .githooks); run: git config core.hooksPath "$(git rev-parse --show-toplevel)/.githooks"" ;; esac

worktree_branches=$(git worktree list --porcelain | awk '/^branch /{sub("refs/heads/","",$2); print $2}')
bad=0
for b in $(git for-each-ref --format='%(refname:short)' refs/heads); do
  case " $ALLOWED " in *" $b "*) continue ;; esac
  bad=$((bad+1))
  if echo "$worktree_branches" | grep -qx "$b"; then
    wt=$(git worktree list --porcelain | awk -v ref="refs/heads/$b" '/^worktree /{p=substr($0,10)} $0=="branch "ref{print p; exit}')
    if [ "$apply" -eq 1 ]; then
      # Owner, 2026-11-09: branches held by other worktrees may be deleted.
      # Archive, detach the worktree (files and uncommitted changes kept), delete. Never remove the worktree.
      git tag -f "archive/$b" "$b" >/dev/null
      if git -C "$wt" switch --detach >/dev/null 2>&1 && git branch -D "$b" >/dev/null 2>&1; then
        echo "archived, detached worktree $wt, deleted: $b"
      else
        echo "could not delete (worktree $wt not detachable by this user): $b"
      fi
    else
      echo "VIOLATION (in use by worktree $wt; --apply archives, detaches and deletes): $b"
    fi
  elif [ "$apply" -eq 1 ]; then
    git tag -f "archive/$b" "$b" >/dev/null
    git branch -D "$b" >/dev/null 2>&1 && echo "archived and deleted: $b" || echo "could not delete: $b"
  else
    echo "VIOLATION: $b"
  fi
done
for r in $(git for-each-ref --format='%(refname)' refs/remotes/origin); do r=${r#refs/remotes/}; case "$r" in */HEAD) continue ;; esac
  name=${r#origin/}; [ "$name" = HEAD ] && continue
  case " $ALLOWED " in *" $name "*) continue ;; esac
  echo "REMOTE VIOLATION: origin/$name (remote deletion needs an explicit approval)"
  bad=$((bad+1))
done
[ "$bad" -eq 0 ] && echo "branch policy: OK (only $ALLOWED)"
[ "$bad" -eq 0 ]
