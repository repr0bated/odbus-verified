#!/bin/sh
# Dash-compatible test for threetched build script v2

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$package_dir/source.pin"
test_root=$(mktemp -d "/tmp/tched-router-threetched-test.XXXXXX")
cleanup() {
    rm -rf -- "$test_root"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$test_root/cache"
printf '%s\n' 'not the pinned source archive' > "$test_root/invalid.tar"
"$package_dir/build-threetched.sh" \
    --cache-dir "$test_root/cache" \
    --source-archive "$test_root/invalid.tar" \
    --output-dir "$test_root/output" \
    >"$test_root/archive.out" 2>&1 || :
if grep -F "source archive SHA256 mismatch" "$test_root/archive.out" >/dev/null 2>&1; then
    :
else
    echo "FAIL: invalid cached source archive failed for the wrong reason" >&2
    exit 1
fi
if [ -e "$test_root/output/threetched" ]; then
    echo "FAIL: invalid source produced a threetched artifact" >&2
    exit 1
fi

# A corrupt cache must not force a fetch or become build input
printf '%s\n' 'corrupt cache' \
    > "$test_root/cache/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar"
"$package_dir/build-threetched.sh" \
    --cache-dir "$test_root/cache" \
    --output-dir "$test_root/output" \
    --verify-source-only > "$test_root/vendored.out"
_check_input="$test_root/cache.sha256"
printf '%s  %s\n' "$SOURCE_ARCHIVE_SHA256" "$test_root/cache/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar" > "$_check_input"
if ! sha256sum -c "$_check_input" >/dev/null; then
    echo "FAIL: vendored source SHA did not match" >&2
    exit 1
fi
if ! grep -F "$SOURCE_ARCHIVE_SHA256" "$test_root/vendored.out" >/dev/null 2>&1; then
    echo "FAIL: vendored source was not reported as the verified input" >&2
    exit 1
fi

# Source pin is readable and well-formed
for _pin_key in UPSTREAM_URL UPSTREAM_TAG UPSTREAM_COMMIT SOURCE_ARCHIVE_SHA256 VENDORED_SOURCE_GZIP_SHA256 MINIMUM_RUST_VERSION; do
    if ! grep -q "^${_pin_key}=" "$package_dir/source.pin"; then
        echo "FAIL: source.pin missing ${_pin_key}" >&2
        exit 1
    fi
done

echo "threetched v2 build script tests passed"
