#!/bin/sh
# Explicit operator-run fixture: no privileges, real mounts, or live services.
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT HUP INT TERM
export fixture
mkdir "$fixture/bin"
# Only the disposable copy substitutes paths and identity for fixtures.
sed -e 's|^PATH=.*|PATH="$fixture/bin:/usr/bin:/bin"|' \
    -e 's|^release_ready=.*|release_ready=$fixture/ready|' \
    -e 's|^adapter=.*|adapter=$fixture/adapter|' \
    "$source_dir/libexec-3tched/recover-3tchedfs" > "$fixture/recover"
cat > "$fixture/bin/id" <<'EOF'
#!/bin/sh
printf '0\n'
EOF
cat > "$fixture/bin/findmnt" <<'EOF'
#!/bin/sh
[ ! -f "$fixture/table-error" ] || exit 2
target=
while [ "$#" -gt 0 ]; do
    if [ "$1" = -M ]; then shift; target=$1; fi
    shift
done
if [ -z "$target" ]; then printf '/\n'; exit 0; fi
name=${target##*/}
[ -f "$fixture/$name" ] || exit 1
cat "$fixture/$name"
EOF
cat > "$fixture/adapter" <<'EOF'
#!/bin/sh
[ "$#" = 4 ] && [ "$2" = "/run/mounts/$1" ] && [ "$3" = -o ] &&
    [ "$4" = rw,nosuid,nodev,noexec,noatime ] || exit 90
printf '%s\n' "$1" >> "$fixture/calls"
[ ! -f "$fixture/fail-$1" ] || exit 1
printf '%s fuse.3tchedFS\n' "$1" > "$fixture/$1"
EOF
chmod +x "$fixture/bin/id" "$fixture/bin/findmnt" "$fixture/adapter"
reset_case() {
    rm -f "$fixture/3tchedFS" "$fixture/threetched" "$fixture/calls" \
        "$fixture/fail-3tchedFS" "$fixture/table-error"
    printf 'complete fixture\n' > "$fixture/ready"
}
assert_no_calls() { [ ! -e "$fixture/calls" ]; }

reset_case
sh "$fixture/recover" --once
[ "$(cat "$fixture/calls")" = "$(printf '3tchedFS\nthreetched')" ]
printf 'PASS: both initially absent views restored\n'

rm "$fixture/calls"
sh "$fixture/recover" --once
assert_no_calls
printf 'PASS: healthy mounts untouched\n'

rm "$fixture/threetched"
sh "$fixture/recover" --once
[ "$(cat "$fixture/calls")" = threetched ]
printf 'PASS: one missing view restored independently\n'

reset_case
touch "$fixture/fail-3tchedFS"
if sh "$fixture/recover" --once; then exit 1; fi
[ "$(cat "$fixture/calls")" = "$(printf '3tchedFS\nthreetched')" ]
[ -f "$fixture/threetched" ]
printf 'PASS: failed first view does not block second\n'

reset_case
printf 'transition fixture\n' > "$fixture/ready"
sh "$fixture/recover" --once
assert_no_calls
rm "$fixture/ready"
sh "$fixture/recover" --once
assert_no_calls
printf 'PASS: transition and missing release readiness pause recovery\n'

reset_case
printf 'foreign ext4\n' > "$fixture/3tchedFS"
printf 'threetched fuse.3tchedFS\n' > "$fixture/threetched"
if sh "$fixture/recover" --once; then exit 1; fi
assert_no_calls
[ "$(cat "$fixture/3tchedFS")" = 'foreign ext4' ]
printf 'PASS: foreign occupied target untouched\n'

reset_case
touch "$fixture/table-error"
if sh "$fixture/recover" --once; then exit 1; fi
assert_no_calls
printf 'PASS: mount-table failure does not trigger recovery\n'
