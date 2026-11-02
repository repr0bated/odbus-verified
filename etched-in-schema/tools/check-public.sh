#!/bin/sh
# Refuse staged content that must never appear in this public repository:
# real values listed in local/IDENTIFIERS.md, private/host addresses, and
# anything shaped like a credential. Runs as the pre-commit hook.
set -eu
cd "$(git rev-parse --show-toplevel)"
staged=$(git diff --cached --name-only --diff-filter=ACMR)
[ -n "$staged" ] || exit 0
patterns=$(mktemp); trap 'rm -f "$patterns"' EXIT
if [ -f local/IDENTIFIERS.md ]; then
    sed -n 's/^| `<[^>]*>` | `\(.*\)` |$/\1/p' local/IDENTIFIERS.md | sed 's/[.[\*^$/]/\\&/g' >> "$patterns"
fi
cat >> "$patterns" <<'P'
\b10\.(0|200)\.[0-9]+\.[0-9]+\b
\b(192\.168|172\.(1[6-9]|2[0-9]|3[01]))\.[0-9]+\.[0-9]+\b
-----BEGIN [A-Z ]*PRIVATE KEY-----
PrivateKey[[:space:]]*=
\bsk-[A-Za-z0-9]{20,}
(api[_-]?key|token|secret)[\"']?[[:space:]]*[:=][[:space:]]*[\"']?[A-Za-z0-9_./+-]{24,}
P
bad=0
for f in $staged; do
    [ -f "$f" ] || continue
    case "$f" in local/*|CONTEXT.md) echo "check-public: $f must not be committed"; bad=1; continue ;; esac
    if git show ":$f" | grep -n -E -f "$patterns" >/dev/null 2>&1; then
        echo "check-public: $f contains a private value (address, identity ID or credential):"
        git show ":$f" | grep -n -E -f "$patterns" | cut -c1-12 | sed 's/^/  line /'
        bad=1
    fi
done
[ "$bad" = 0 ] || { echo "Use placeholders (see local/IDENTIFIERS.md). Commit refused."; exit 1; }
