#!/bin/sh
# Markdown link checker for the docs/ tree using lychee.
#
# Usage:
#   docs/check-links.sh [path ...]
#
# With no arguments it checks all Markdown under docs/. Any argument is passed
# through to lychee as a target path or glob.
#
# Requires lychee: https://github.com/lycheeverse/lychee
#   cargo install lychee   (or: doas apk add lychee, where packaged)

set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v lychee >/dev/null 2>&1; then
  echo "error: lychee not found on PATH." >&2
  echo "install with: cargo install lychee" >&2
  exit 127
fi

# Default target: every Markdown file under docs/.
if [ "$#" -eq 0 ]; then
  set -- 'docs/**/*.md'
fi

# --offline: do not hit the network; validate only local file links and anchors.
# --no-progress: quiet output suitable for CI logs.
exec lychee \
  --offline \
  --no-progress \
  --include-fragments \
  "$@"
