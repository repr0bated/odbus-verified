#!/usr/bin/env bash
# Routing/argument/status tests use fake runners and never invoke real Cargo/models.
set -euo pipefail
repo_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/cache"
cat > "$fixture/bin/native-cargo" <<'EOF'
#!/usr/bin/env bash
printf 'cwd=%s\n' "$PWD"
printf '<%s>\n' "$@"
printf 'native-stderr\n' >&2
exit "${CARGO_TEST_EXIT:-0}"
EOF
cat > "$fixture/bin/runner" <<'EOF'
#!/usr/bin/env bash
prompt=${!#}
while IFS= read -r line; do
    if [[ $line == *cargo-dispatch.sh*' --worker '* ]]; then
        bash -c "$line"
        exit 0
    fi
done <<< "$prompt"
exit 92
EOF
cat > "$fixture/bin/no-completion" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$fixture/bin/"*
export CARGO_REAL_BINARY="$fixture/bin/native-cargo" CARGO_DISPATCH_CACHE="$fixture/cache"
export CARGO_RUNNER_GPT="$fixture/bin/runner" CARGO_RUNNER_GROK="$fixture/bin/runner" CARGO_RUNNER_CLAUDE="$fixture/bin/runner"
unset CARGO_DISPATCHED
cd -- "$repo_root"

for mapping in gpt:gpt-6-luna grok:composer-2.5 claude:haiku; do
    family=${mapping%%:*}; model=${mapping#*:}
    CARGO_CALLER_FAMILY=$family CARGO_DISPATCH_DRY_RUN=1 "$repo_root/scripts/cargo-dispatch.sh" metadata > "$fixture/route"
    rg -Fx "caller=$family" "$fixture/route" >/dev/null
    rg -Fx "model=$model" "$fixture/route" >/dev/null
done

args=(metadata '--manifest-path' 'path with spaces/Cargo.toml' '' '$(touch should-not-exist)' '`touch should-not-exist`' $'line\nbreak')
for family in gpt grok claude; do
    CARGO_CALLER_FAMILY=$family "$repo_root/scripts/cargo-dispatch.sh" "${args[@]}" > "$fixture/actual" 2> "$fixture/stderr"
    CARGO_DISPATCHED=1 "$fixture/bin/native-cargo" "${args[@]}" > "$fixture/expected" 2>/dev/null
    cmp "$fixture/actual" "$fixture/expected"
    rg -Fx 'native-stderr' "$fixture/stderr" >/dev/null
    test ! -e should-not-exist
done

set +e
CARGO_CALLER_FAMILY=gpt CARGO_TEST_EXIT=43 "$repo_root/scripts/cargo-dispatch.sh" test > "$fixture/failed" 2>/dev/null
status=$?
set -e
test "$status" = 43
rg -Fx '<test>' "$fixture/failed" >/dev/null

set +e
CARGO_CALLER_FAMILY=gpt CARGO_RUNNER_GPT="$fixture/bin/no-completion" "$repo_root/scripts/cargo-dispatch.sh" check > "$fixture/incomplete" 2>/dev/null
status=$?
set -e
test "$status" = 125
test ! -s "$fixture/incomplete"

# The sourced command and worker guard retain actual-command syntax.
source "$repo_root/scripts/cargo-aliases.bash"
test "$(type -t cargo)" = function
CARGO_CALLER_FAMILY=claude CARGO_DISPATCH_DRY_RUN=1 cargo clippy > "$fixture/alias"
rg -Fx 'model=haiku' "$fixture/alias" >/dev/null
CARGO_DISPATCHED=1 cargo check > "$fixture/bypass" 2>/dev/null
rg -Fx '<check>' "$fixture/bypass" >/dev/null
printf '%s\n' 'Cargo dispatch routing, argument isolation, output/status, missing completion and recursion checks passed.'
