#!/usr/bin/env bash
# Unit tests for deploy/lib/golden/golden.sh. No subvolumes are created.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
# Globals consumed by sourced golden.sh helpers.
ROOT='' WORK='' SLOTS='' LIVE='' ROLLBACK='' STATE='' SEED='' POOL_ROOT=''
export ROOT WORK SLOTS LIVE ROLLBACK STATE SEED POOL_ROOT
# Never let a test probe the real pool (its I/O may be hung).
unset GOLDEN_SEED GOLDEN_ROOT GOLDEN_POOL_ROOT GOLDEN_OS_SUBDIR GOLDEN_LOWER
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


P=/var/lib/incus/storage-pools/3tched-pool
# Default layout for path helpers (parse_args not required). Strings only.
POOL_ROOT=$P SEED='' ROOT=''
resolve_layout

printf 'defaults\n'
assert_eq "default pool" "$P" "$POOL_ROOT"
assert_eq "default seed is the pool seed" "$P/images/@antix-seed" "$SEED"
assert_eq "default root sits next to the seed" "$P/golden-os" "$ROOT"
assert_eq "default work on the pool" "$P/golden-os/work" "$WORK"
assert_eq "default live on the pool" "$P/golden-os/slots/live" "$LIVE"
assert_eq "OS dir of the seed is rootfs" "$P/images/@antix-seed/rootfs" "$(os_dir_of "$SEED")"
OS_SUBDIR=''
assert_eq "flat OS dir when GOLDEN_OS_SUBDIR is empty" "$LIVE" "$(os_dir_of "$LIVE")"
OS_SUBDIR=rootfs
export OS_SUBDIR
assert_eq "normalize_path is lexical" "/a/c" "$(normalize_path /a/b/../c)"

printf 'path protection\n'
assert_ok "seed is protected" path_is_protected $P/images/@seed
assert_ok "seed child is protected" path_is_protected $P/images/@seed/rootfs
assert_ok "antix-seed is protected" path_is_protected $P/images/@antix-seed
assert_ok "antix-seed rootfs is protected" path_is_protected $P/images/@antix-seed/rootfs
assert_ok "incus containers are protected" path_is_protected $P/containers
assert_ok "incus images dir is protected" path_is_protected $P/images
assert_ok "BtrfsCache root is protected" path_is_protected /var/lib/op-dbus/cache
assert_ok "BtrfsCache category is protected" path_is_protected /var/lib/op-dbus/cache/embeddings
assert_ok "cache snapshots are protected" path_is_protected /var/lib/op-dbus/@cache-snapshots
assert_ok "historical golden payload is protected" path_is_protected /opt/op-dbus/golden
assert_ok "system-a slot is protected" path_is_protected /mnt/top/@opdbus-system-a
assert_ok "seed-send is protected" path_is_protected /mnt/top/@seed-send
assert_ok "host root is protected" path_is_protected /
assert_fail "default work is not a protected path" path_is_protected "$WORK"
assert_fail "default live is not a protected path" path_is_protected "$LIVE"
assert_fail "default rollback is not a protected path" path_is_protected "$ROLLBACK"
SEED=/srv/seeds/custom-os
assert_ok "a configured seed is protected" path_is_protected /srv/seeds/custom-os/rootfs
SEED=$P/images/@antix-seed

printf 'owned slot paths\n'
assert_ok "work is owned" path_is_owned "$WORK"
assert_ok "live is owned" path_is_owned "$LIVE"
assert_ok "rollback is owned" path_is_owned "$ROLLBACK"
assert_ok "incoming is owned" path_is_owned "$SLOTS/.incoming-20261118-120000"
assert_fail "seed is not owned" path_is_owned $P/images/@antix-seed
assert_fail "old received copy name under root is not owned" path_is_owned "$ROOT/@antix-seed"
assert_fail "other pool dirs are not owned" path_is_owned $P/containers
assert_fail "payload golden is not owned" path_is_owned /opt/op-dbus/golden
assert_fail "sibling path is not owned" path_is_owned "$ROOT/../containers"

printf 'delete guard\n'
if ( require_owned_delete $P/images/@antix-seed ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete seed should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete seed refused\n'
fi
assert_contains "delete seed error names the path" "protected" "$(cat /tmp/golden-test-err)"

if ( require_owned_delete "$ROOT/unrelated" ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete of unrelated path should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete unrelated path refused\n'
fi

( require_owned_delete "$ROLLBACK" >/dev/null )
PASS=$((PASS + 1))
printf '  ok  delete of exact rollback path is allowed by the guard\n'

if ( require_owned_delete "$LIVE/../../../images/@antix-seed" ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL delete via .. escape should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  delete via .. escape refused\n'
fi

printf 'root placement\n'
GOLDEN_MOUNTS="$(printf '%s %s\n%s %s\n%s %s\n' / /dev/sda1 /opt/op-dbus '/dev/sdb3[/@opdbus-prep]' "$P" /dev/ublkb1)"
export GOLDEN_MOUNTS
assert_eq "backing from mount table only" "/dev/ublkb1" "$(backing_device "$P/golden-os/work")"
refused_root() {
    ROOT=$1
    ! ( resolve_layout && assert_root_allowed ) 2>/tmp/golden-test-err >/dev/null
}
assert_fail "pool release dir root is allowed (ublk by design)" refused_root "$P/golden-os"
assert_ok "incus images root refused" refused_root "$P/images/golden"
assert_ok "incus containers root refused" refused_root "$P/containers"
assert_ok "seed as root refused" refused_root "$P/images/@antix-seed"
assert_ok "pool root itself refused" refused_root "$P"
assert_ok "nested pool dir refused" refused_root "$P/golden-os/nested/../../custom"
assert_ok "/var/lib/incus root refused" refused_root /var/lib/incus/golden
assert_ok "historical golden root refused" refused_root /opt/op-dbus/golden
assert_ok "ROOT via .. into historical golden refused" refused_root /opt/op-dbus/os/../golden
assert_fail "host btrfs root still allowed when configured" refused_root /opt/op-dbus/os
GOLDEN_MOUNTS="$(printf '%s %s\n%s %s\n' / /dev/sda1 /mnt/other /dev/ublkb2)"
assert_ok "ublk root outside the pool refused" refused_root /mnt/other/golden
export GOLDEN_ALLOW_UBLK=1
assert_fail "ublk root outside the pool allowed with GOLDEN_ALLOW_UBLK=1" refused_root /mnt/other/golden
unset GOLDEN_ALLOW_UBLK GOLDEN_MOUNTS
ROOT='' SEED=''
resolve_layout

printf 'same-filesystem check\n'
# Mount-table only; no real btrfs or seed inodes.
GOLDEN_BTRFS_MOUNTS="$(printf '%s\t%s\t%s\n%s\t%s\t%s\n' \
    /opt/op-dbus 69ee351a-e4a4-44db-a318-b4574a2439ae '/dev/sdb3[/@opdbus-prep]' \
    "$P" 2f1ee116-e5a7-481f-a4ae-ddaff2bd2b1a /dev/ublkb1)"
export GOLDEN_BTRFS_MOUNTS
assert_eq "ident for /opt/op-dbus/os" "uuid:69ee351a-e4a4-44db-a318-b4574a2439ae" "$(btrfs_ident_for_path /opt/op-dbus/os)"
assert_eq "ident for @antix-seed" "uuid:2f1ee116-e5a7-481f-a4ae-ddaff2bd2b1a" "$(btrfs_ident_for_path $P/images/@antix-seed)"
assert_fail "antix-seed and /opt/op-dbus/os are different filesystems" same_btrfs_filesystem $P/images/@antix-seed /opt/op-dbus/os
assert_ok "seed and default root are the same filesystem" same_btrfs_filesystem "$SEED" "$ROOT"

# Empty UUID must not steal the SOURCE device (antix1 findmnt on 3tched-pool).
_saved_mounts=$GOLDEN_BTRFS_MOUNTS
GOLDEN_BTRFS_MOUNTS='TARGET="/var/lib/incus/storage-pools/3tched-pool" UUID="" SOURCE="/dev/ublkb1"
TARGET="/opt/op-dbus" UUID="69ee351a-e4a4-44db-a318-b4574a2439ae" SOURCE="/dev/sdb3[/@opdbus-prep]"'
assert_eq "empty UUID ident is the device" "dev:/dev/ublkb1" "$(btrfs_ident_for_path $P/images/@antix-seed)"
assert_eq "pairs UUID ident stays a uuid" "uuid:69ee351a-e4a4-44db-a318-b4574a2439ae" "$(btrfs_ident_for_path /opt/op-dbus/os)"
GOLDEN_BTRFS_MOUNTS=$_saved_mounts

assert_ok "default root passes the same-btrfs check" require_same_btrfs_as_seed
ROOT=/opt/op-dbus/os
resolve_layout
if ( require_same_btrfs_as_seed ) >/tmp/golden-test-out 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL cross-filesystem root should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  cross-filesystem root refused\n'
fi
assert_contains "cross-fs error says snapshot only works same fs" "only works on the same filesystem" "$(cat /tmp/golden-test-err)"
assert_contains "cross-fs error says no send/receive" "does not send/receive or copy" "$(cat /tmp/golden-test-err)"
assert_contains "cross-fs error points at the pool root" "$P/golden-os" "$(cat /tmp/golden-test-err)"
ROOT='' SEED=''
resolve_layout
if ( seed_path_allowed "$WORK" ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL seed = work should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  seed = work is refused\n'
fi
if ( seed_path_allowed "$ROOT/something" ) 2>/tmp/golden-test-err; then
    FAIL=$((FAIL + 1))
    printf '  FAIL seed inside root should have been refused\n'
else
    PASS=$((PASS + 1))
    printf '  ok  seed inside root is refused\n'
fi
assert_ok "pool seed is an allowed seed" seed_path_allowed "$SEED"
unset GOLDEN_BTRFS_MOUNTS

printf 'CLI dry-run\n'
TMP=$(mktemp -d)
trap 'rm -rf -- "$TMP"' EXIT
out=$TMP/out
# A fake pool under $TMP, registered as one btrfs in the mount table, so the
# CLI never probes the real 3tched-pool.
GOLDEN_POOL_ROOT=$TMP/pool
GOLDEN_BTRFS_MOUNTS="$(printf '%s\t%s\t%s\n%s\t%s\t%s\n' \
    "$TMP/pool" 11111111-2222-3333-4444-555555555555 /dev/fakepool \
    /opt/op-dbus 69ee351a-e4a4-44db-a318-b4574a2439ae '/dev/sdb3[/@opdbus-prep]')"
GOLDEN_PROBE_TIMEOUT=2
export GOLDEN_POOL_ROOT GOLDEN_BTRFS_MOUNTS GOLDEN_PROBE_TIMEOUT

# Default verb is plan, default mode is dry-run.
"$GOLDEN" >"$out" 2>&1
assert_contains "plan announces dry-run" "DRY-RUN" "$(cat "$out")"
assert_contains "plan names the seed" "seed: $TMP/pool/images/@antix-seed" "$(cat "$out")"
assert_contains "plan root defaults next to the seed" "root: $TMP/pool/golden-os" "$(cat "$out")"
assert_contains "plan mentions live/rollback cut" "demote current live to rollback" "$(cat "$out")"
assert_contains "plan mentions rollback delete" "delete the previous rollback only" "$(cat "$out")"
assert_contains "plan mentions chroot cargo" "/root/.cargo/bin/cargo build --workspace --release" "$(cat "$out")"
assert_contains "plan refuses seed deletes" "images/@seed" "$(cat "$out")"
assert_contains "plan init snapshots the seed into work" "btrfs subvolume snapshot -- $TMP/pool/images/@antix-seed $TMP/pool/golden-os/work" "$(cat "$out")"
assert_contains "plan says no send/receive" "no btrfs send/receive" "$(cat "$out")"
assert_not_contains "plan never runs btrfs send" "btrfs send --" "$(cat "$out")"
assert_not_contains "plan never runs btrfs receive" "btrfs receive" "$(cat "$out")"
assert_not_contains "plan has no /opt/op-dbus/os copy" "/opt/op-dbus/os" "$(cat "$out")"
assert_contains "plan overlay lower is the seed rootfs before a release" "lowerdir=$TMP/pool/images/@antix-seed/rootfs," "$(cat "$out")"
assert_not_contains "plan does not invoke apply language only" "mode: APPLY" "$(cat "$out")"
assert_ok "dry-run plan did not create the pool" test ! -e "$TMP/pool"

# Dry-run init must not create the root or any slots.
"$GOLDEN" init >"$out" 2>&1
assert_ok "dry-run init does not create the work root" test ! -e "$TMP/pool/golden-os"
assert_contains "dry-run init would snapshot the seed" "btrfs subvolume snapshot -- $TMP/pool/images/@antix-seed" "$(cat "$out")"

"$GOLDEN" --seed "$TMP/pool/images/@other" init >"$out" 2>&1
assert_contains "--seed overrides the seed" "snapshot -- $TMP/pool/images/@other $TMP/pool/golden-os/work" "$(cat "$out")"
"$GOLDEN" --from "$TMP/pool/images/@other" init >"$out" 2>&1
assert_contains "--from is an alias for --seed" "snapshot -- $TMP/pool/images/@other" "$(cat "$out")"
"$GOLDEN" --pool "$TMP/pool" --root "$TMP/pool/rel" init >"$out" 2>&1
assert_contains "--root on the seed's btrfs" "$TMP/pool/rel/work" "$(cat "$out")"

if "$GOLDEN" --root /opt/op-dbus/os init >"$out" 2>&1; then
    FAIL=$((FAIL + 1))
    printf '  FAIL CLI cross-filesystem init should have exited non-zero\n'
else
    PASS=$((PASS + 1))
    printf '  ok  CLI cross-filesystem init exits non-zero\n'
fi
assert_contains "CLI cross-fs init says snapshots need one fs" "only works on the same filesystem" "$(cat "$out")"
assert_ok "CLI cross-fs init did not create /opt/op-dbus/os/work" test ! -e /opt/op-dbus/os/work

if "$GOLDEN" init --send-receive >"$out" 2>&1; then
    FAIL=$((FAIL + 1))
    printf '  FAIL --send-receive should be rejected\n'
else
    PASS=$((PASS + 1))
    printf '  ok  --send-receive is rejected\n'
fi
assert_contains "--send-receive error explains removal" "was removed" "$(cat "$out")"

if "$GOLDEN" --lower bogus plan >"$out" 2>&1; then
    FAIL=$((FAIL + 1))
    printf '  FAIL --lower bogus should be rejected\n'
else
    PASS=$((PASS + 1))
    printf '  ok  --lower bogus is rejected\n'
fi

"$GOLDEN" mount-overlay --lower live >"$out" 2>&1
assert_contains "--lower live uses the live slot" "lowerdir=$TMP/pool/golden-os/slots/live/rootfs," "$(cat "$out")"
"$GOLDEN" mount-overlay --lower seed >"$out" 2>&1
assert_contains "--lower seed uses the seed" "lowerdir=$TMP/pool/images/@antix-seed/rootfs," "$(cat "$out")"
mkdir -p "$TMP/pool/golden-os/slots/live"
"$GOLDEN" mount-overlay >"$out" 2>&1
assert_contains "auto lower prefers the live release" "lowerdir=$TMP/pool/golden-os/slots/live/rootfs," "$(cat "$out")"
GOLDEN_OS_SUBDIR='' "$GOLDEN" mount-overlay >"$out" 2>&1
assert_contains "empty GOLDEN_OS_SUBDIR uses the flat subvolume" "lowerdir=$TMP/pool/golden-os/slots/live," "$(cat "$out")"
rm -rf -- "$TMP/pool"

"$GOLDEN" release >"$out" 2>&1
assert_contains "release plan rotates live to rollback" "demote current live to rollback" "$(cat "$out")"
assert_contains "release plan does not remount host root" "will not remount the host root" "$(cat "$out")"
assert_contains "release plan keeps the seed" "is the release image and stays put" "$(cat "$out")"
assert_not_contains "release plan has no btrfstune" "btrfstune" "$(cat "$out")"

"$GOLDEN" rotate >"$out" 2>&1
assert_contains "rotate plan is slot rotation" "plan: rotate" "$(cat "$out")"
assert_not_contains "rotate plan does not mount" "would: mount -t overlay" "$(cat "$out")"

if grep -q btrfstune "$GOLDEN"; then
    FAIL=$((FAIL + 1))
    printf '  FAIL golden.sh unexpectedly mentions btrfstune\n'
else
    PASS=$((PASS + 1))
    printf '  ok  golden.sh has no btrfstune\n'
fi
if grep -Eq 'btrfs (send|receive) --' "$GOLDEN"; then
    FAIL=$((FAIL + 1))
    printf '  FAIL golden.sh still runs btrfs send/receive\n'
else
    PASS=$((PASS + 1))
    printf '  ok  golden.sh has no btrfs send/receive\n'
fi
if grep -v '^ *#' "$GOLDEN" | grep -Eq 'findmnt .*-o *[A-Z,]*UUID'; then
    FAIL=$((FAIL + 1))
    printf '  FAIL golden.sh queries findmnt UUID\n'
else
    PASS=$((PASS + 1))
    printf '  ok  golden.sh does not query findmnt UUID\n'
fi

"$GOLDEN" cut >"$out" 2>&1
assert_ok "dry-run cut does not create slots" test ! -e "$TMP/pool"
assert_contains "cut plan snapshots work read-only" "subvolume snapshot -r" "$(cat "$out")"
assert_contains "cut plan does not remount host" "not implied by cut" "$(cat "$out")"

# --apply without root must fail (this test is not root) and must not create paths.
if [ "$(id -u)" = 0 ]; then
    printf '  skip --apply-without-creating (running as root)\n'
else
    if "$GOLDEN" init --apply >"$out" 2>&1; then
        FAIL=$((FAIL + 1))
        printf '  FAIL init --apply should fail without root\n'
    else
        PASS=$((PASS + 1))
        printf '  ok  init --apply fails without root\n'
    fi
    assert_ok "failed --apply did not create the root" test ! -e "$TMP/pool"
    assert_contains "apply error mentions root" "must run as root" "$(cat "$out")"
fi

"$GOLDEN" status >"$out" 2>&1
assert_contains "status shows absent work" "work" "$(cat "$out")"
assert_contains "status shows the seed" "seed:     absent   $TMP/pool/images/@antix-seed" "$(cat "$out")"
assert_contains "status mentions historical layouts" "images/@seed" "$(cat "$out")"

"$GOLDEN" --help >"$out" 2>&1
assert_contains "help lists verbs" "mount-overlay" "$(cat "$out")"
assert_contains "help lists release" "release" "$(cat "$out")"
assert_contains "help lists --seed" "--seed PATH" "$(cat "$out")"
assert_contains "help lists --pool" "--pool PATH" "$(cat "$out")"
assert_not_contains "help no longer offers --send-receive" "--send-receive" "$(cat "$out")"

# Wrapper scripts stay executable and parse.
assert_ok "build-golden.sh is a thin wrapper" grep -q 'exec .*/golden.sh' "$HERE/../build-golden.sh"

# chroot(1) chdirs to /: the build must cd into the exported tree.
assert_ok "chroot build cds into the exported source" grep -q 'cd /opt/op-build.src && exec' "$GOLDEN"
assert_contains "help lists --toolchain" "--toolchain" "$(cat "$out")"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ]
