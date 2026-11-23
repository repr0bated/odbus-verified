#!/bin/sh
# One-time: serve the new LXD Cozo store as /dev/ublkb4 and create its btrfs.
set -eu
BIN=/usr/local/libexec/opdbus-ublk
sha256sum --strict --check --quiet "$BIN/manifest.sha256"
[ ! -b /dev/ublkb4 ] && [ ! -c /dev/ublkc4 ] || { echo "device 4 in use"; exit 1; }
modprobe ublk_drv
export OP_UBLK_COZO_DB_PATH=/var/lib/op-dbus/ublk-lxd UBLK_DEVICE=pool-default-persist UBLK_DEV_ID=4 OP_UBLK_NO_COMPRESSION=0
unshare --mount "$BIN/cozo-ublk" & d=$!
i=0; until [ -b /dev/ublkb4 ]; do kill -0 $d || { echo "daemon exited"; exit 1; }; [ $i -ge 80 ] && { echo "no device"; kill $d; exit 1; }; sleep .25; i=$((i+1)); done
[ -z "$(blkid -p -o value -s TYPE /dev/ublkb4 2>/dev/null)" ] || { echo "device not empty"; kill -TERM $d; wait $d; exit 1; }
mkfs.btrfs -K -L cozo-lxd /dev/ublkb4
blkid -p -s UUID -o value /dev/ublkb4 > /var/lib/op-dbus/ublk-lxd.btrfs-uuid
echo "uuid $(cat /var/lib/op-dbus/ublk-lxd.btrfs-uuid)"
sync; kill -TERM $d; wait $d || true
echo "daemon stopped"
