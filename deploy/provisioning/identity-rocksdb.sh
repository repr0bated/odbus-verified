#!/bin/sh
# Per-container ext4 RocksDB store (owner, 2026-11-22; task 27.3), the design of
# the retired deploy/seed/3tched-seed-launch (ROCKSDB=$CDIR/rocksdb.img, 10G,
# chattr +C while empty, mkfs.ext4 -L rocksdb, mounted at
# var/lib/op-dbus/identity-cozo), carried onto Incus:
#   image   $IDENTITY_ROCKSDB_DIR/<NAME>.img        (host, sparse, +C, 0600)
#   mount   /run/opdbus/identity-rocksdb/<NAME>     (host loop mount, 0700)
#   device  identity-rocksdb -> /var/lib/op-dbus/identity-cozo (shift)
# The container's identity owner and its personal cognitive store
# (COGNITIVE_DB_PATH=/var/lib/op-dbus/identity-cozo/personal-cognitive: personal
# memory and soul) live on it. /run is empty after boot, so a container whose
# image is not mounted fails to start instead of writing to the rootfs.
#   identity-rocksdb.sh create <NAME>   create + format the image (never reformats)
#   identity-rocksdb.sh mount <NAME>    loop-mount one image (idempotent)
#   identity-rocksdb.sh mount-all       mount every image (host boot service)
# Nothing is deleted or reformatted.
set -eu
DIR=${IDENTITY_ROCKSDB_DIR:-/var/lib/opdbus/identity-rocksdb}
RUN=/run/opdbus/identity-rocksdb
SIZE=${IDENTITY_ROCKSDB_SIZE:-10G}
fail() { echo "identity-rocksdb: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
valid() { case $1 in ''|*[!A-Za-z0-9_-]*|[!A-Za-z0-9]*) fail "invalid container name '$1'";; esac; }

create() { n=$1; valid "$n"; img=$DIR/$n.img
  install -d -m 0700 "$DIR"
  if [ -e "$img" ]; then echo "  image $img present"; return 0; fi
  touch "$img"; chattr +C "$img" 2>/dev/null || true   # NOCOW only takes while empty
  truncate -s "$SIZE" "$img"; chmod 600 "$img"
  mkfs.ext4 -q -F -L rocksdb -E lazy_itable_init=1,lazy_journal_init=1 "$img"
  echo "  + image $img ($SIZE ext4)"; }

mount_one() { n=$1; valid "$n"; img=$DIR/$n.img; mnt=$RUN/$n
  [ -f "$img" ] || fail "no image $img"
  install -d -m 0755 /run/opdbus "$RUN"; install -d -m 0700 "$mnt"
  if mountpoint -q "$mnt"; then echo "  $mnt mounted"; return 0; fi
  mount -t ext4 -o loop,noatime,nodiratime "$img" "$mnt"
  chmod 0700 "$mnt"                                   # Cozo enforces 0700 on its root
  echo "  + mounted $img at $mnt"; }

case ${1:-} in
create) create "${2:?container name}" ;;
mount) mount_one "${2:?container name}" ;;
mount-all)
  [ -d "$DIR" ] || exit 0
  for img in "$DIR"/*.img; do [ -f "$img" ] || continue; n=${img##*/}; mount_one "${n%.img}"; done ;;
*) fail "usage: $0 create <NAME> | mount <NAME> | mount-all" ;;
esac
