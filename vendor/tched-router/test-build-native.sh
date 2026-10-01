#!/bin/sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$package_dir/source.pin"
test_root=$(mktemp -d "${TMPDIR:-/tmp}/tched-router-native-test.XXXXXX")
cleanup() { status=$?; rm -rf -- "$test_root"; exit "$status"; }
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

mkdir -p "$test_root/cache"
printf '%s\n' 'not the pinned source archive' > "$test_root/invalid.tar"
if "$package_dir/build-native.sh" \
    --cache-dir "$test_root/cache" \
    --source-archive "$test_root/invalid.tar" \
    --output-dir "$test_root/output" \
    >"$test_root/archive.out" 2>&1; then
    echo "invalid cached source archive was accepted" >&2
    exit 1
fi
grep -F "source archive digest mismatch" "$test_root/archive.out" >/dev/null || {
    echo "invalid cached source archive failed for the wrong reason" >&2
    exit 1
}
[ ! -e "$test_root/output/zeroclaw" ] || {
    echo "invalid source produced a native artifact" >&2
    exit 1
}

# A corrupt cache must not force a fetch or become build input: the default
# checked-in archive replaces it with the exact deterministic upstream tar.
printf '%s\n' 'corrupt cache' \
    > "$test_root/cache/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar"
"$package_dir/build-native.sh" \
    --cache-dir "$test_root/cache" \
    --output-dir "$test_root/output" \
    --verify-source-only > "$test_root/vendored.out"
sha256sum -c - <<EOF >/dev/null
$SOURCE_ARCHIVE_SHA256  $test_root/cache/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar
EOF
grep -F "$SOURCE_ARCHIVE_SHA256" "$test_root/vendored.out" >/dev/null || {
    echo "vendored source was not reported as the verified input" >&2
    exit 1
}

wrong_base="$test_root/wrong-base"
mkdir -p "$wrong_base"
: > "$wrong_base/Cargo.toml"
sed -n '/^# Existing files/,/^# New files/{/^#/d;p;}' "$package_dir/patch-inventory" |
while IFS= read -r path; do
    mkdir -p "$wrong_base/$(dirname "$path")"
    : > "$wrong_base/$path"
done
if "$package_dir/apply-native-patches.sh" "$wrong_base" \
    >"$test_root/base.out" 2>&1; then
    echo "wrong pinned base was accepted" >&2
    exit 1
fi
grep -F "base source hash mismatch" "$test_root/base.out" >/dev/null || {
    cat "$test_root/base.out" >&2
    echo "wrong pinned base failed for the wrong reason" >&2
    exit 1
}
for path in \
    crates/zeroclaw-gateway/src/ws_stateless.rs \
    crates/zeroclaw-gateway/src/ws_stateless_tests.rs \
    crates/zeroclaw-providers/src/opencode_session.rs \
    crates/zeroclaw-providers/src/graph_turn_reminder.rs \
    crates/zeroclaw-providers/src/graph_turn_reminder_tests.rs \
    crates/zeroclaw-providers/src/sled_identity.rs \
    crates/zeroclaw-providers/src/stateless.rs; do
    [ ! -e "$wrong_base/$path" ] || {
        echo "wrong base was mutated before rejection" >&2
        exit 1
    }
done

printf '%s\n' "native patch-package rejection tests passed"
