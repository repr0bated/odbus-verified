#!/bin/sh
# activate-golden.sh — the one command for deploying a new version.
#
#   sudo ./activate-golden.sh
#
# Run it after the release build and seal:
#   CXXFLAGS="-include cstdint" cargo build --workspace --release
#   sudo target/release/opblob seal-shm && sudo target/release/opblob persist
# OP_RELEASE_DIR selects one prepared optimized artifact directory for both
# release-controller verification and golden preparation (default target/release).
#
# What it does:
# 1. Native provision-slots reconciles any interrupted transaction and prepares
#    the owned golden/slot layout. Recovery-only work stops this invocation.
# 2. Prepares /opt/op-dbus/golden from target/release and the sealed catalog
#    (internal helper deploy/lib/golden/build-golden.sh; it never builds or seals).
# 3. Native release sends/receives the immutable generation into the inactive
#    slot, rotates read-only overlays at the inventory's owned host targets,
#    and restores affected consumers' wanted states. Only active + previous remain.
#
# Undo the last release:
#   sudo /usr/local/libexec/opdbus-release/op-release-launch rollback
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
export PATH LC_ALL

REPO=$(CDPATH= cd -- "$(dirname -- "$(readlink -f "$0")")" && pwd)
OP_RELEASE_DIR=$(realpath -e -- "${OP_RELEASE_DIR:-"$REPO/target/release"}")
export OP_RELEASE_DIR
RELEASE_BASE=/usr/local/libexec/opdbus-release
RELEASE_LAUNCH=$RELEASE_BASE/op-release-launch
RELEASE_BIN=$RELEASE_BASE/op-release
RELEASE_BOOT=/etc/init.d/op-release-activate

[ "$(id -u)" = 0 ] || { echo 'activate-golden.sh: run as root (sudo ./activate-golden.sh)' >&2; exit 1; }
[ "$#" = 0 ] || { echo 'activate-golden.sh: takes no arguments' >&2; exit 1; }
# Check the execution path before invoking any installed kit code. The launcher
# independently checks the complete approved manifest and kernel/BTF pair.
for directory in / /usr /usr/local /usr/local/libexec "$RELEASE_BASE" /etc /etc/init.d; do
    directory_mode=0$(stat -c %a "$directory")
    [ -d "$directory" ] && [ ! -L "$directory" ] &&
        [ "$(stat -c %u:%g "$directory")" = 0:0 ] &&
        [ "$((directory_mode & 07022))" -eq 0 ] ||
        { echo "activate-golden.sh: insecure release-kit directory: $directory" >&2; exit 1; }
done
for artifact in "$RELEASE_LAUNCH" "$RELEASE_BIN" "$RELEASE_BOOT"; do
    [ -f "$artifact" ] && [ ! -L "$artifact" ] &&
        [ "$(stat -c %u:%g:%a:%h "$artifact")" = 0:0:755:1 ] ||
        { echo "activate-golden.sh: install the approved root-owned release kit: $artifact" >&2; exit 1; }
done
cmp -s "$RELEASE_LAUNCH" "$REPO/deploy/release/op-release-launch" &&
    cmp -s "$RELEASE_BIN" "$OP_RELEASE_DIR/op-release" &&
    cmp -s "$RELEASE_BOOT" "$REPO/deploy/release/op-release-activate" ||
    { echo 'activate-golden.sh: installed release kit differs from this build; refusing an old or mixed kit' >&2; exit 1; }

"$RELEASE_LAUNCH" provision-slots
"$REPO/deploy/lib/golden/build-golden.sh"
exec "$RELEASE_LAUNCH" release
