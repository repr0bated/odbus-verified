#!/bin/sh
# activate-golden.sh — front door for the golden OS-slot release.
#
# Rewritten 2026-11-18 to match the approved design: a btrfs subvolume holds
# the OS, cargo builds in a chroot of that OS, a read-only snapshot of it
# becomes the live overlay, and there are exactly two slots (live and one
# rollback). Dry-run / plan is the default; pass a verb and --apply to mutate.
#
# The previous flow (payload staging into /opt/op-dbus/golden, then the
# native op-release-launch kit rotating @opdbus-system-a/b) is not invoked
# from here. That kit remains in deploy/release/ and is not deleted.
#
#   ./activate-golden.sh              # plan (dry-run)
#   ./activate-golden.sh status
#   ./activate-golden.sh cut          # print the cut plan
#   sudo ./activate-golden.sh cut --apply   # actually rotate slots (requires a work OS)
set -eu
REPO=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
# Default to plan when invoked with no arguments.
if [ "$#" -eq 0 ]; then
    set -- plan
fi
exec "$REPO/deploy/lib/golden/golden.sh" "$@"
