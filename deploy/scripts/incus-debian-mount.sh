#!/bin/sh
# Create a Debian container and mount a path from it locally.
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

# Create and start container if it doesn't exist
if ! sudo incus info "$NAME" >/dev/null 2>&1; then
    echo "Creating container: $NAME"
    sudo incus launch images:debian/12 "$NAME"
    echo "Waiting for container to be ready..."
    sleep 3
else
    echo "Container $NAME already exists"
    sudo incus start "$NAME" 2>/dev/null || true
fi

# Ensure local mountpoint exists
mkdir -p "$LOCAL_MNT"

echo "Mounting $NAME$CONTAINER_PATH -> $LOCAL_MNT"
echo "Press Ctrl+C to unmount and exit"
echo ""

# Mount — blocks until Ctrl+C
sudo incus file mount "$NAME$CONTAINER_PATH" "$LOCAL_MNT"
