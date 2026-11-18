#!/usr/bin/env bash
# Unit tests for deploy/lib/golden/golden.sh. No subvolumes are created.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
# Globals consumed by sourced golden.sh helpers.
ROOT='' WORK='' SLOTS='' LIVE='' ROLLBACK='' STATE=''
export ROOT WORK SLOTS LIVE ROLLBACK STATE FROM_OS IMAGE SEND_RECEIVE
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
IMAGE=$ROOT/@antix-seed
assert_ok "received release image is owned" path_is_owned "$ROOT/@antix-seed"
IMAGE=
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

printf 'same-filesystem check\n'
# Mount-table only; no real btrfs or seed inodes.
GOLDEN_BTRFS_MOUNTS="$(printf '%s\t%s\t%s\n%s\t%s\t%s\n' \
    /opt/op-dbus 69ee351a-e4a4-44db-a318-b4574a2439ae '/dev/sdb3[/@opdbus-prep]' \
    /var/lib/incus/storage-pools/3tched-pool 2f1ee116-e5a7-481f-a4ae-ddaff2bd2b1a /dev/ublkb1)"
export GOLDEN_BTRFS_MOUNTS
assert_eq "ident for /opt/op-dbus/os" "uuid:69ee351a-e4a4-44db-a318-b4574a2439ae" "$(btrfs_ident_for_path /opt/op-dbus/os)"
assert_eq "ident for @antix-seed" "uuid:2f1ee116-e5a7-481f-a4ae-ddaff2bd2b1a" "$(btrfs_ident_for_path /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed)"
assert_fail "antix-seed and /opt/op-dbus/os are different filesystems" same_btrfs_filesystem /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed /opt/op-dbus/os
assert_ok "two paths under @opdbus-prep are the same filesystem" same_btrfs_filesystem /opt/op-dbus/os/work /opt/op-dbus/os

# Empty UUID must not steal the SOURCE device (antix1 findmnt on 3tched-pool).
_saved_mounts=$GOLDEN_BTRFS_MOUNTS
GOLDEN_BTRFS_MOUNTS='TARGET="/var/lib/incus/storage-pools/3tched-pool" UUID="" SOURCE="/dev/ublkb1"
TARGET="/opt/op-dbus" UUID="69ee351a-e4a4-44db-a318-b4574a2439ae" SOURCE="/dev/sdb3[/@opdbus-prep]"'
assert_eq "empty UUID ident is the device" "dev:/dev/ublkb1" "$(btrfs_ident_for_path /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed)"
assert_eq "pairs UUID ident stays a uuid" "uuid:69ee351a-e4a4-44db-a318-b4574a2439ae" "$(btrfs_ident_for_path /opt/op-dbus/os)"
GOLDEN_BTRFS_MOUNTS=$_saved_mounts

ROOT=/opt/op-dbus/os
FROM_OS=/var/lib/incus/storage-pools/3tched-pool/images/@antix-seed
resolve_layout
if ( require_same_btrfs_for_from ) >/tmp/golden-test-out 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL cross-filesystem --from should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  cross-filesystem --from refused\n'
fi
assert_contains "cross-fs error names send/receive" "--send-receive" "$(cat /tmp/golden-test-err)"
assert_contains "cross-fs error names full copy" "rsync -aHAX" "$(cat /tmp/golden-test-err)"
assert_contains "cross-fs error says snapshot only works same fs" "only works on the same filesystem" "$(cat /tmp/golden-test-err)"

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

if "$GOLDEN" --root /opt/op-dbus/os init --from /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed >"$out" 2>&1; then
    FAIL=$((FAIL + 1))
    printf '  FAIL CLI cross-filesystem init should have exited non-zero
'
else
    PASS=$((PASS + 1))
    printf '  ok  CLI cross-filesystem init exits non-zero
'
fi
assert_contains "CLI cross-fs init mentions send/receive" "--send-receive" "$(cat "$out")"
assert_ok "CLI cross-fs init did not create /opt/op-dbus/os" test ! -e /opt/op-dbus/os

GOLDEN_FAKE_SOURCE_RO=true
GOLDEN_SKIP_SPACE=1
export GOLDEN_FAKE_SOURCE_RO GOLDEN_SKIP_SPACE
if "$GOLDEN" --root /opt/op-dbus/os init --from /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed --send-receive >"$out" 2>&1; then
    PASS=$((PASS + 1))
    printf '  ok  CLI --send-receive dry-run exits 0\n'
else
    FAIL=$((FAIL + 1))
    printf '  FAIL CLI --send-receive dry-run should have succeeded\n'
    cat "$out"
fi
assert_contains "send-receive plan sends the seed" "btrfs send -- /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed" "$(cat "$out")"
assert_contains "send-receive plan receives under ROOT" "btrfs receive -- /opt/op-dbus/os" "$(cat "$out")"
assert_contains "send-receive keeps received image" "received release image" "$(cat "$out")"
assert_contains "send-receive snapshots image to work" "subvolume snapshot -- /opt/op-dbus/os/@antix-seed /opt/op-dbus/os/work" "$(cat "$out")"
assert_ok "send-receive dry-run did not create /opt/op-dbus/os" test ! -e /opt/op-dbus/os

# Same-FS --from of a received image under ROOT is allowed.
FROM_OS=/opt/op-dbus/os/@antix-seed
ROOT=/opt/op-dbus/os
SEND_RECEIVE=0
resolve_layout
if ( from_path_allowed "$FROM_OS" && require_same_btrfs_for_from ); then
    PASS=$((PASS + 1))
    printf '  ok  --from a received image under ROOT is allowed\n'
else
    FAIL=$((FAIL + 1))
    printf '  FAIL --from received image under ROOT should be allowed\n'
fi
if ( from_path_allowed /opt/op-dbus/os/work ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL --from work should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  --from work is refused\n'
fi

"$GOLDEN" --root /opt/op-dbus/os release >"$out" 2>&1
assert_contains "release plan rotates live to rollback" "demote current live to rollback" "$(cat "$out")"
assert_contains "release plan does not remount host root" "will not remount the host root" "$(cat "$out")"
assert_not_contains "release plan has no btrfstune" "btrfstune" "$(cat "$out")"

"$GOLDEN" --root /opt/op-dbus/os rotate >"$out" 2>&1
assert_contains "rotate plan is slot rotation" "plan: rotate" "$(cat "$out")"
assert_not_contains "rotate plan does not mount" "would: mount -t overlay" "$(cat "$out")"

if grep -q btrfstune "$GOLDEN"; then
    FAIL=$((FAIL + 1))
    printf '  FAIL golden.sh unexpectedly mentions btrfstune\n'
else
    PASS=$((PASS + 1))
    printf '  ok  golden.sh has no btrfstune\n'
fi

unset GOLDEN_BTRFS_MOUNTS GOLDEN_FAKE_SOURCE_RO GOLDEN_SKIP_SPACE

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
assert_contains "help lists release" "release" "$(cat "$out")"
assert_contains "help lists send-receive" "--send-receive" "$(cat "$out")"

# Wrapper scripts stay executable and parse.
assert_ok "build-golden.sh is a thin wrapper" grep -q 'exec .*/golden.sh' "$HERE/../build-golden.sh"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ]
