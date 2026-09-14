#!/bin/sh
# Create a Btrfs subvolume, bootstrap a Debian rootfs into it, and chroot.
#
# Usage:
#   sudo ./btrfs-chroot.sh [subvol-name] [packages...]
#
# Example:
#   sudo ./btrfs-chroot.sh nlm-chrome chromium nodejs npm
#
set -eu

BTRFS_DEV="${BTRFS_DEV:-/dev/sda3}"
BTRFS_MNT="${BTRFS_MNT:-/mnt/btrfs-top}"
SUBVOL_NAME="${1:-debian-chroot}"
shift || true
EXTRA_PKGS="${*:-}"

SUBVOL_PATH="$BTRFS_MNT/@chroots/$SUBVOL_NAME"
CHROOT="$BTRFS_MNT/@chroots/$SUBVOL_NAME/root"

# ── Mount Btrfs top-level ────────────────────────────────────────────────────
mkdir -p "$BTRFS_MNT"
if ! mountpoint -q "$BTRFS_MNT"; then
    mount -o subvolid=5,noatime "$BTRFS_DEV" "$BTRFS_MNT"
fi

# ── Create subvolume if needed ───────────────────────────────────────────────
mkdir -p "$BTRFS_MNT/@chroots"
if [ ! -d "$SUBVOL_PATH" ]; then
    btrfs subvolume create "$SUBVOL_PATH"
    echo "Created subvolume: $SUBVOL_PATH"
fi
mkdir -p "$CHROOT"

# ── Bootstrap Debian if not already done ────────────────────────────────────
if [ ! -f "$CHROOT/etc/debian_version" ]; then
    echo "Bootstrapping Debian bookworm into $CHROOT ..."
    if ! command -v debootstrap >/dev/null 2>&1; then
        pacman -Sy --noconfirm debootstrap 2>/dev/null || \
        apt-get install -y debootstrap 2>/dev/null || \
        { echo "Install debootstrap first"; exit 1; }
    fi
    debootstrap --arch=amd64 bookworm "$CHROOT" https://deb.debian.org/debian
    echo "Bootstrap complete"
fi

# ── Install extra packages inside chroot ────────────────────────────────────
if [ -n "$EXTRA_PKGS" ]; then
    echo "Installing: $EXTRA_PKGS"
    chroot "$CHROOT" apt-get update -qq
    # shellcheck disable=SC2086
    chroot "$CHROOT" apt-get install -y $EXTRA_PKGS
fi

# ── Bind mounts ─────────────────────────────────────────────────────────────
for fs in dev dev/pts proc sys run; do
    mountpoint -q "$CHROOT/$fs" || mount --bind "/$fs" "$CHROOT/$fs"
done

# Copy resolv.conf for DNS
cp /etc/resolv.conf "$CHROOT/etc/resolv.conf"

# ── Snapshot before chroot (cheap safety net) ───────────────────────────────
SNAP="$BTRFS_MNT/@chroots/${SUBVOL_NAME}-snap-$(date +%s)"
btrfs subvolume snapshot "$SUBVOL_PATH" "$SNAP" 2>/dev/null && \
    echo "Snapshot: $SNAP" || true

echo ""
echo "Entering chroot: $CHROOT"
echo "Type 'exit' to leave. Bind mounts will be cleaned up automatically."
echo ""

# ── Enter chroot ─────────────────────────────────────────────────────────────
chroot "$CHROOT" /bin/bash || true

# ── Cleanup bind mounts ──────────────────────────────────────────────────────
echo "Cleaning up bind mounts..."
for fs in run sys proc dev/pts dev; do
    umount -lf "$CHROOT/$fs" 2>/dev/null || true
done

echo "Done. Subvolume remains at: $SUBVOL_PATH"
echo "To delete: sudo btrfs subvolume delete $SUBVOL_PATH"
