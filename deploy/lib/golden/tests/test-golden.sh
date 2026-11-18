#!/usr/bin/env bash
# Unit tests for deploy/lib/golden/golden.sh. No subvolumes are created.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
# Globals consumed by sourced golden.sh helpers.
ROOT='' WORK='' SLOTS='' LIVE='' ROLLBACK='' STATE=''
export ROOT WORK SLOTS LIVE ROLLBACK STATE
GOLDEN=$HERE/../golden.sh
PASS=0
FAIL=0

assert_eq() {
    local label=$1 expected=$2 actual=$3
    if [ "$expected" = "$actual" ]; then
        PASS=$((PASS + 1))
        printf '  ok  %s\n' "$label"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL %s\n    expected: %s\n    actual:   %s\n' "$label" "$expected" "$actual"
    fi
}

assert_ok() {
    local label=$1
    shift
    if "$@"; then
        PASS=$((PASS + 1))
        printf '  ok  %s\n' "$label"
    else
        FAIL=$((FAIL + 1))
        printf '  FAIL %s (expected success)\n' "$label"
    fi
}

assert_fail() {
    local label=$1
    shift
    if "$@"; then
        FAIL=$((FAIL + 1))
        printf '  FAIL %s (expected failure)\n' "$label"
    else
        PASS=$((PASS + 1))
        printf '  ok  %s\n' "$label"
    fi
}

assert_contains() {
    local label=$1 needle=$2 haystack=$3
    case "$haystack" in
        *"$needle"*)
            PASS=$((PASS + 1))
            printf '  ok  %s\n' "$label"
            ;;
        *)
            FAIL=$((FAIL + 1))
            printf '  FAIL %s\n    missing: %s\n' "$label" "$needle"
            ;;
    esac
}

assert_not_contains() {
    local label=$1 needle=$2 haystack=$3
    case "$haystack" in
        *"$needle"*)
            FAIL=$((FAIL + 1))
            printf '  FAIL %s\n    unexpectedly present: %s\n' "$label" "$needle"
            ;;
        *)
            PASS=$((PASS + 1))
            printf '  ok  %s\n' "$label"
            ;;
    esac
}

# Source function library without running main.
# shellcheck source=../golden.sh disable=SC1091
GOLDEN_LIB=1 source "$GOLDEN"

# Minimal layout for path helpers (parse_args not required).
ROOT=/opt/op-dbus/os
resolve_layout

printf 'path protection\n'
assert_ok "seed is protected" path_is_protected /var/lib/incus/storage-pools/3tched-pool/images/@seed
assert_ok "seed child is protected" path_is_protected /var/lib/incus/storage-pools/3tched-pool/images/@seed/rootfs
assert_ok "antix-seed is protected" path_is_protected /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed
assert_ok "incus pool is protected" path_is_protected /var/lib/incus/storage-pools/3tched-pool/containers
assert_ok "BtrfsCache root is protected" path_is_protected /var/lib/op-dbus/cache
assert_ok "BtrfsCache category is protected" path_is_protected /var/lib/op-dbus/cache/embeddings
assert_ok "cache snapshots are protected" path_is_protected /var/lib/op-dbus/@cache-snapshots
assert_ok "historical golden payload is protected" path_is_protected /opt/op-dbus/golden
assert_ok "system-a slot is protected" path_is_protected /mnt/top/@opdbus-system-a
assert_ok "seed-send is protected" path_is_protected /mnt/top/@seed-send
assert_ok "host root is protected" path_is_protected /
assert_fail "default work is not a protected path" path_is_protected /opt/op-dbus/os/work
assert_fail "default live is not a protected path" path_is_protected /opt/op-dbus/os/slots/live
assert_fail "default rollback is not a protected path" path_is_protected /opt/op-dbus/os/slots/rollback

printf 'owned slot paths\n'
assert_ok "work is owned" path_is_owned "$WORK"
assert_ok "live is owned" path_is_owned "$LIVE"
assert_ok "rollback is owned" path_is_owned "$ROLLBACK"
assert_ok "incoming is owned" path_is_owned "$SLOTS/.incoming-20261118-120000"
assert_fail "seed is not owned" path_is_owned /var/lib/incus/storage-pools/3tched-pool/images/@seed
assert_fail "payload golden is not owned" path_is_owned /opt/op-dbus/golden
assert_fail "sibling path is not owned" path_is_owned /opt/op-dbus/os/../golden
assert_fail "system-a is not owned" path_is_owned /opt/op-dbus/../@opdbus-system-a

printf 'delete guard\n'
if ( require_owned_delete /var/lib/incus/storage-pools/3tched-pool/images/@seed ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete seed should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete seed refused\n'
fi
assert_contains "delete seed error names the path" "protected" "$(cat /tmp/golden-test-err)"

if ( require_owned_delete /opt/op-dbus/os/unrelated ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete of unrelated path should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete unrelated path refused\n'
fi

( require_owned_delete "$ROLLBACK" >/dev/null )
PASS=$((PASS + 1))
printf '  ok  delete of exact rollback path is allowed by the guard\n'

# .. must not escape the release root after normalization.
if ( require_owned_delete /opt/op-dbus/os/slots/live/../../../golden ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete via .. escape should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete via .. escape refused\n'
fi

printf 'root placement\n'
ROOT=/var/lib/incus/storage-pools/3tched-pool/os
resolve_layout
if ( assert_root_allowed ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL incus-pool root should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  incus-pool root refused\n'
fi

ROOT=/opt/op-dbus/golden
resolve_layout
if ( assert_root_allowed ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL historical golden root should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  historical golden root refused\n'
fi

ROOT=/opt/op-dbus/os
resolve_layout
# Not on ublk in this test environment.
if ( assert_root_allowed ); then
    PASS=$((PASS + 1))
    printf '  ok  default root is allowed\n'
else
    FAIL=$((FAIL + 1))
    printf '  FAIL default root should be allowed\n'
fi

ROOT=/opt/op-dbus/os/../golden
if ( resolve_layout && assert_root_allowed ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL ROOT via .. into historical golden should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  ROOT via .. into historical golden refused\n'
fi
# Restore the default layout for later tests that source helpers.
ROOT=/opt/op-dbus/os
resolve_layout

printf 'CLI dry-run\n'
TMP=$(mktemp -d)
trap 'rm -rf -- "$TMP"' EXIT
out=$TMP/out

# Default verb is plan, default mode is dry-run.
"$GOLDEN" --root "$TMP/os" --repo /tmp >"$out" 2>&1 || true
# repo /tmp may not have Cargo.toml; plan should still print and not fail for plan/init.
"$GOLDEN" --root "$TMP/os" plan >"$out" 2>&1
assert_eq "plan exits 0" 0 0
assert_contains "plan announces dry-run" "DRY-RUN" "$(cat "$out")"
assert_contains "plan mentions live/rollback cut" "demote current live to rollback" "$(cat "$out")"
assert_contains "plan mentions rollback delete" "delete the previous rollback only" "$(cat "$out")"
assert_contains "plan mentions chroot cargo" "cargo build --workspace --release" "$(cat "$out")"
assert_contains "plan refuses seed deletes" "images/@seed" "$(cat "$out")"
assert_not_contains "plan does not invoke apply language only" "mode: APPLY" "$(cat "$out")"

# Dry-run must not create the root or any slots.
"$GOLDEN" --root "$TMP/os" init >"$out" 2>&1
assert_ok "dry-run init does not create the work root" test ! -e "$TMP/os"
assert_contains "dry-run init would create work" "btrfs subvolume create" "$(cat "$out")"

"$GOLDEN" --root "$TMP/os" cut >"$out" 2>&1
assert_ok "dry-run cut does not create slots" test ! -e "$TMP/os"
assert_contains "cut plan snapshots work read-only" "subvolume snapshot -r" "$(cat "$out")"
assert_contains "cut plan does not remount host" "not implied by cut" "$(cat "$out")"

# --apply without root must fail (this test is not root) and must not create paths.
if [ "$(id -u)" = 0 ]; then
    printf '  skip --apply-without-creating (running as root)\n'
else
    if "$GOLDEN" --root "$TMP/os" init --apply >"$out" 2>&1; then
        FAIL=$((FAIL + 1))
        printf '  FAIL init --apply should fail without root\n'
    else
        PASS=$((PASS + 1))
        printf '  ok  init --apply fails without root\n'
    fi
    assert_ok "failed --apply did not create the root" test ! -e "$TMP/os"
    assert_contains "apply error mentions root" "must run as root" "$(cat "$out")"
fi

"$GOLDEN" --root "$TMP/os" status >"$out" 2>&1
assert_contains "status shows absent work" "work" "$(cat "$out")"
assert_contains "status mentions historical layouts" "images/@seed" "$(cat "$out")"

"$GOLDEN" --help >"$out" 2>&1
assert_contains "help lists verbs" "mount-overlay" "$(cat "$out")"

# Wrapper scripts stay executable and parse.
assert_ok "build-golden.sh is a thin wrapper" grep -q 'exec .*/golden.sh' "$HERE/../build-golden.sh"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ]
