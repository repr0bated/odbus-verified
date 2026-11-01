#!/bin/sh
# install-host.sh — install cf-mesh mesh-exec onto the host rootfs.
#
#   sudo deploy/runit/cf-mesh/install-host.sh
#
# warp-svc runs in a private mount namespace (unshare --propagation private).
# If that namespace retains opdbus-release overlays or FUSE views, those
# mounts pin the release subvolume and the next slot rotate fails with
# EBUSY. mesh-exec therefore drops those mounts before execing warp-svc,
# and the script itself must live on the host rootfs — never under a
# release overlay target.
#
# Installs:
#   /usr/local/sbin/mesh-exec
#   /etc/cloudflare-mesh/before-host-termination/mesh-exec  (restore copy)
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$(readlink -f "$0")")" && pwd)
SRC="$SCRIPT_DIR/mesh-exec"
DEST=/usr/local/sbin/mesh-exec
RESTORE_DIR=/etc/cloudflare-mesh/before-host-termination
RESTORE="$RESTORE_DIR/mesh-exec"

die() { printf 'cf-mesh-install: %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "run as root"
[ -f "$SRC" ] || die "missing source: $SRC"
bash -n "$SRC" || die "syntax check failed: $SRC"

install -d -o root -g root -m 0755 /usr/local/sbin
install -d -o root -g root -m 0700 "$RESTORE_DIR"

if [ -e "$DEST" ] && ! cmp -s "$SRC" "$DEST"; then
  bak="$DEST.bak-$(date -u +%Y%m%dT%H%M%SZ)"
  cp -p "$DEST" "$bak"
  printf 'cf-mesh-install: backed up %s -> %s\n' "$DEST" "$bak"
fi

install -o root -g root -m 0755 "$SRC" "$DEST"
install -o root -g root -m 0700 "$SRC" "$RESTORE"

src_sum=$(sha256sum "$SRC" | cut -d' ' -f1)
dest_sum=$(sha256sum "$DEST" | cut -d' ' -f1)
restore_sum=$(sha256sum "$RESTORE" | cut -d' ' -f1)
[ "$src_sum" = "$dest_sum" ] || die "digest mismatch after install: $DEST"
[ "$src_sum" = "$restore_sum" ] || die "digest mismatch after install: $RESTORE"

# Confirm the release-unpin stanza is present (guards against installing an old copy).
grep -q 'opdbus-release:' "$DEST" || die "installed mesh-exec missing opdbus-release umount"
grep -q 'unshare --mount --propagation private' "$DEST" || die "installed mesh-exec missing private mount ns"

printf 'cf-mesh-install: ok %s\n' "$src_sum"
printf 'cf-mesh-install: %s\n' "$DEST"
printf 'cf-mesh-install: %s\n' "$RESTORE"
