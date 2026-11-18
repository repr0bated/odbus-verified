#!/bin/sh
# build-golden.sh — entry point for the approved golden release design.
#
# Rewritten 2026-11-18. The golden unit is now a btrfs OS subvolume: cargo
# builds inside a chroot of that OS, the tree is snapshotted read-only, and
# that snapshot is the live overlay. Exactly two slots exist, live and
# rollback; a cut demotes live to rollback and deletes the previous rollback.
#
# Dry-run is the default. See deploy/lib/golden/README.md.
# The previous application-payload stager is build-golden-payload.sh.
set -eu
DIR=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
exec "$DIR/golden.sh" "$@"
