#!/bin/sh
# Rebuild partition 1 (sealed OS) of the 3tched seed image in place, keeping p2.
#   OS from the read-only subvolume at $SRC via btrfs send, as default subvolume @,
#   seed-wide config applied (Xray egress proxy, container fstab), then sealed.
# Every container sprouted from the old p1 must be re-provisioned afterwards.
set -eu

SRC=${SRC:-/mnt}
IMG=/var/lib/incus/storage-pools/3tched/3tched-seed.img
PROXY=http://127.0.0.1:3128
NO_PROXY=localhost,127.0.0.1,::1
WORK=$(mktemp -d)
DST=$WORK/dst
LOOP=

cleanup() {
	mountpoint -q "$DST" 2>/dev/null && umount "$DST"
	[ -n "$LOOP" ] && losetup -d "$LOOP" 2>/dev/null || true
	rm -rf "$WORK"
}
trap cleanup EXIT

[ "$(btrfs property get -ts "$SRC" ro)" = "ro=true" ] || { echo "$SRC must be a read-only subvolume" >&2; exit 1; }
[ -z "$(losetup -j "$IMG")" ] || { echo "$IMG is attached; stop its containers first" >&2; exit 1; }
mkdir -p "$DST"

LOOP=$(losetup -fP --show "$IMG")
udevadm settle 2>/dev/null || true

mkfs.btrfs -q -f -L 3tched-seed "${LOOP}p1"
mount "${LOOP}p1" "$DST"
btrfs send "$SRC" | btrfs receive "$DST"
NAME=$(ls "$DST")
[ "$NAME" = "@" ] || mv "$DST/$NAME" "$DST/@"
btrfs property set -f -ts "$DST/@" ro false
btrfs subvolume set-default "$DST/@"
R=$DST/@

# Container fstab: Incus provides root; there is no swap. Keep the 3tchedFS lines.
sed -i -E 's@^(UUID=[^ ]+[[:space:]]+(/|none)[[:space:]]+(btrfs|swap)[[:space:]].*)$@# seed: provided by Incus # \1@' "$R/etc/fstab"

# Egress: Incus proxy device (profile cockpitbot-seed) puts the host Xray HTTP
# egress at $PROXY inside every container.
for v in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY; do
	grep -q "^$v=" "$R/etc/environment" || echo "$v=$PROXY" >>"$R/etc/environment"
done
for v in no_proxy NO_PROXY; do
	grep -q "^$v=" "$R/etc/environment" || echo "$v=$NO_PROXY" >>"$R/etc/environment"
done
mkdir -p "$R/etc/systemd/system.conf.d"
cat >"$R/etc/systemd/system.conf.d/10-egress-proxy.conf" <<EOF
[Manager]
DefaultEnvironment="http_proxy=$PROXY" "https_proxy=$PROXY" "HTTP_PROXY=$PROXY" "HTTPS_PROXY=$PROXY" "no_proxy=$NO_PROXY" "NO_PROXY=$NO_PROXY"
EOF
cat >"$R/etc/apt/apt.conf.d/95egress-proxy" <<EOF
Acquire::http::Proxy "$PROXY";
Acquire::https::Proxy "$PROXY";
EOF

sync
umount "$DST"
btrfstune -S1 "${LOOP}p1"
losetup -d "$LOOP"
LOOP=
echo "resealed p1 of $IMG"
