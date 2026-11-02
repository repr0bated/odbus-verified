#!/bin/sh
# No stubs presented as built. Identifier placeholders like <door-ip> are fine;
# unfinished work is not. Runs from the pre-commit hook.
#  1. staged code/docs may not contain stub markers;
#  2. book/src/SUMMARY.md may list a folder README outside "# Planned" only if
#     that folder holds more than its README (i.e. something is implemented).
set -eu
cd "$(git rev-parse --show-toplevel)"
bad=0
markers_cs='\b(TODO|FIXME|XXX|TBD)\b|todo!\(|unimplemented!\('
markers_ci='coming soon|will appear here|not yet implemented'
for f in $(git diff --cached --name-only --diff-filter=ACMR); do
    # Vendored third-party docs are upstream text, not our work.
    case "$f" in tools/check-no-stubs.sh|*.lock|*.json|*/reference/docs/*) continue ;; esac
    [ -f "$f" ] || continue
    # Lint settings that forbid stubs (e.g. `todo = "deny"`) are not stubs.
    hits=$( { git show ":$f" | grep -n -E "$markers_cs"; git show ":$f" | grep -n -i -E "$markers_ci"; } | grep -v -E '^[0-9]+:[[:space:]]*(todo|unimplemented|dbg_macro)[[:space:]]*=[[:space:]]*"deny"' || true)
    if [ -n "$hits" ]; then
        echo "check-no-stubs: $f contains a stub marker:"
        printf '%s\n' "$hits" | cut -c1-100 | sed 's/^/  /'
        bad=1
    fi
done
summary=book/src/SUMMARY.md
if [ -f "$summary" ]; then
    section=""
    while IFS= read -r line; do
        case "$line" in "# "*) section=$line; continue ;; esac
        link=$(printf '%s' "$line" | sed -n 's/.*](\([^)]*\.md\)).*/\1/p')
        [ -n "$link" ] || continue
        [ "$section" = "# Planned" ] && continue
        page=book/src/$link
        [ -L "$page" ] || continue
        target=$(readlink -f "$page")
        case "$(basename "$target")" in README.md) ;; *) continue ;; esac
        dir=$(dirname "$target")
        others=$(find "$dir" -mindepth 1 -not -name README.md -not -path '*/target*' | head -1)
        if [ -z "$others" ]; then
            echo "check-no-stubs: $link is only a README ($(basename "$dir")/); list it under '# Planned' in $summary"
            bad=1
        fi
    done < "$summary"
fi
[ "$bad" = 0 ] || { echo "Commit refused: finish it, or label it Planned."; exit 1; }
