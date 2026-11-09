#!/bin/sh
# Rebuild partition 1 (sealed OS) of the 3tched seed image in place, keeping p2.
#   OS from the read-only subvolume at $SRC via btrfs send, as default subvolume @,
#   seed-wide config applied (Xray egress proxy, container fstab, optional
#   PACKAGES and OVERLAY tree), then sealed.
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
	for m in "$DST/@/etc/resolv.conf" "$DST/@/dev" "$DST/@/sys" "$DST/@/proc"; do
		! mountpoint -q "$m" 2>/dev/null || umount -R "$m" || true
	done
	! mountpoint -q "$DST" 2>/dev/null || umount "$DST" || true
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

# Optional packages, installed in a chroot with the host's network (the seed's
# own apt proxy is the containers' 127.0.0.1:3128, absent on the host).
if [ -n "${PACKAGES:-}" ]; then
	for d in proc sys dev; do
		mount --rbind "/$d" "$R/$d"
		mount --make-rslave "$R/$d"
	done
	mount --bind /etc/resolv.conf "$R/etc/resolv.conf"
	printf '#!/bin/sh\nexit 101\n' >"$R/usr/sbin/policy-rc.d"
	chmod 0755 "$R/usr/sbin/policy-rc.d"
	APT="apt-get -q -o Acquire::http::Proxy=false -o Acquire::https::Proxy=false"
	chroot "$R" env DEBIAN_FRONTEND=noninteractive $APT update
	chroot "$R" env DEBIAN_FRONTEND=noninteractive $APT install -y --no-install-recommends $PACKAGES
	chroot "$R" apt-get clean
	rm -f "$R/usr/sbin/policy-rc.d"
	umount "$R/etc/resolv.conf"
	for d in dev sys proc; do umount -R "$R/$d"; done
fi

# Optional seed overlay: files laid over @ as-is (e.g. NextDNS + its DoH
# tunnel, resolved drop-in, profile.d proxy, 3tched-call, unit masks).
# Existing directories keep their ownership and mode.
if [ -n "${OVERLAY:-}" ]; then
	[ -d "$OVERLAY" ] || { echo "OVERLAY $OVERLAY is not a directory" >&2; exit 1; }
	(cd "$OVERLAY" && tar -cpf - --exclude=./ .) | tar -C "$R" -xpf - --no-overwrite-dir
fi

sync
umount "$DST"
btrfstune -S1 "${LOOP}p1"
losetup -d "$LOOP"
LOOP=
echo "resealed p1 of $IMG"
