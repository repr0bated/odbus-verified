#!/bin/sh
# Mount a path from an existing, running container locally.
# Creation and lifecycle belong to the admitted plugin owner (ID-N allocation).
#
# Usage:
#   ./incus-debian-mount.sh <container-name> <container-path> <local-mountpoint>
#
# Example:
#   ./incus-debian-mount.sh nlm-auth /root/auth /tmp/nlm-auth
#
set -eu

NAME="${1:-debian-work}"
CONTAINER_PATH="${2:-/root}"
LOCAL_MNT="${3:-/tmp/${NAME}-mount}"

# This maintenance helper never creates or starts a guest, including after a
# failed read. Use the identity-bound declared create/start operations first.
state=$(sudo incus --force-local query "/1.0/instances/$NAME/state?project=default") || exit "$?"
printf '%s\n' "$state" | jq -e '.status=="Running"' >/dev/null || {
    printf '%s\n' 'Container must already be running; use the admitted lifecycle operation.' >&2
    exit 1
}

# Ensure local mountpoint exists
mkdir -p "$LOCAL_MNT"

echo "Mounting $NAME$CONTAINER_PATH -> $LOCAL_MNT"
echo "Press Ctrl+C to unmount and exit"
echo ""

# Mount — blocks until Ctrl+C
sudo incus file mount "$NAME$CONTAINER_PATH" "$LOCAL_MNT"
