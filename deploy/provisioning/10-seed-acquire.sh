#!/bin/sh
# Step 1: make sure a seed subvolume exists at images/$SEED_NAME on the pool.
#   10-seed-acquire.sh check                 case A: seed exists -> report it
#   10-seed-acquire.sh receive <send-file>   case B1: btrfs receive a sent seed
#                                            (e.g. @seed-send from sdb3)
#   10-seed-acquire.sh image <raw.img>       case B2: copy rootfs out of a raw
#                                            seed image (e.g. /boot-disk/blob/seed-blob.img)
#   10-seed-acquire.sh debootstrap [suite]   case B3: fresh Debian+runit rootfs
# Never deletes or overwrites an existing seed. Result is a WRITABLE seed with
# the Incus layout (metadata.yaml + rootfs/); step 20 seals it read-only.
. "$(dirname -- "$0")/lib.sh"; need_root
mode=${1:-check}
write_metadata() {
  [ -f "$SEED_DIR/metadata.yaml" ] && return 0
  cat >"$SEED_DIR/metadata.yaml" <<M
architecture: x86_64
creation_date: $(date +%s)
properties:
  description: op-dbus identity seed ($SEED_NAME)
  os: $(. "$SEED_DIR/rootfs/etc/os-release"; echo "$ID")
  release: $(. "$SEED_DIR/rootfs/etc/os-release"; echo "${VERSION_ID:-unknown}")
M
}
case $mode in
check)
  [ -d "$SEED_DIR/rootfs" ] || fail "no seed at $SEED_DIR; use receive|image|debootstrap"
  echo "seed $SEED_DIR ro=$(btrfs property get -ts "$SEED_DIR" ro | cut -d= -f2)"
  . "$SEED_DIR/rootfs/etc/os-release"; echo "os: $PRETTY_NAME  init: $(readlink "$SEED_DIR/rootfs/sbin/init" || echo none)" ;;
receive)
  [ -e "$SEED_DIR" ] && fail "$SEED_DIR exists; pick another SEED_NAME"
  f=${2:?send file}; tmp=$POOL_DIR/images/.recv-$STAMP; mkdir "$tmp"
  btrfs receive -f "$f" "$tmp"
  got=$(ls "$tmp"); [ "$(echo "$got" | wc -l)" = 1 ] || fail "expected one subvolume in $tmp"
  btrfs subvolume snapshot "$tmp/$got" "$SEED_DIR"   # writable copy; received ro kept in $tmp
  [ -d "$SEED_DIR/rootfs" ] || fail "received subvolume lacks rootfs/ (wrap it: see README)"
  write_metadata ;;
image)
  [ -e "$SEED_DIR" ] && fail "$SEED_DIR exists; pick another SEED_NAME"
  img=${2:?raw image}; mnt=$(mktemp -d); loop=$(losetup -fP --show -r "$img")
  trap 'umount "$mnt" 2>/dev/null; losetup -d "$loop"; rmdir "$mnt"' EXIT
  part=${loop}p1; [ -b "$part" ] || part=$loop
  mount -o ro "$part" "$mnt"
  src=$mnt; [ -d "$mnt/@" ] && src=$mnt/@; [ -d "$src/rootfs" ] && src=$src/rootfs
  btrfs subvolume create "$SEED_DIR"; mkdir "$SEED_DIR/rootfs"
  cp -a --reflink=auto "$src"/. "$SEED_DIR/rootfs/"
  write_metadata ;;
debootstrap)
  [ -e "$SEED_DIR" ] && fail "$SEED_DIR exists; pick another SEED_NAME"
  suite=${2:-trixie}
  btrfs subvolume create "$SEED_DIR"
  debootstrap --variant=minbase --include=runit,runit-init,iproute2,openssl,ca-certificates,fuse3,procps \
      "$suite" "$SEED_DIR/rootfs" http://deb.debian.org/debian
  write_metadata ;;
*) fail "unknown mode $mode" ;;
esac
log "seed ready (writable) at $SEED_DIR"
