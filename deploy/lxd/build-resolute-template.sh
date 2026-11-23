#!/bin/bash
# Clean Ubuntu 26.04 (Resolute Raccoon) unified LXD image. Run as root in a
# private mount+pid namespace. No network service, keys, machine id or login.
set -euo pipefail
umask 022
REPO=$(cd -- "$(dirname -- "$0")/../.." && pwd)
W=${1:-/var/lib/op-dbus/lxd-template-build-$(date -u +%Y%m%dT%H%M%SZ)}
OUT=${2:-/var/lib/op-dbus/lxd-images/ubuntu-resolute-template-$(date -u +%Y%m%dT%H%M%SZ).tar.zst}
U=https://cdimage.ubuntu.com/ubuntu-base/releases/26.04/release; T=ubuntu-base-26.04.1-base-amd64.tar.gz
KR=/mnt/cozo-btrfs-lxd/seeds/@ubuntu-subsidiary/rootfs/usr/share/keyrings/ubuntu-archive-keyring.gpg
BUILD=/mnt/@opdbus-system-a/opt/op-build
ZCALL=$BUILD/target/x86_64-unknown-linux-gnu/release/zcall
ZCALL_SHA=$(jq -er '.artifacts.zcall | select(test("^[0-9a-f]{64}$"))' "$BUILD/execution-receipt.json")
printf '%s  %s\n' "$ZCALL_SHA" "$ZCALL" | sha256sum -c -
GUEST_BACKEND=$BUILD/target/x86_64-unknown-linux-gnu/release/op-guest-backend
GUEST_BACKEND_SHA=$(jq -er '.artifacts["op-guest-backend"] | select(test("^[0-9a-f]{64}$"))' "$BUILD/execution-receipt.json")
printf '%s  %s\n' "$GUEST_BACKEND_SHA" "$GUEST_BACKEND" | sha256sum -c -
[ ! -e "$W" ] && [ ! -e "$OUT" ] || { echo 'build dir or output exists'; exit 1; }
install -d -m 0700 "$W" "$W/dl"; cd "$W/dl"
for f in SHA256SUMS SHA256SUMS.gpg "$T"; do curl -fsS --retry 3 -o "$f" "$U/$f"; done
gpgv --keyring "$KR" SHA256SUMS.gpg SHA256SUMS
grep -E "[ *]$T\$" SHA256SUMS | sha256sum -c -
R=$W/img/rootfs; install -d -m 0755 "$W/img" "$R"
tar --numeric-owner --xattrs -xpf "$T" -C "$R"
mount --bind "$R" "$R"; mount --make-private "$R"
mount -t proc proc "$R/proc"
mount -t tmpfs -o mode=0755 tmpfs "$R/dev"
for n in null zero full random urandom tty; do touch "$R/dev/$n"; mount --bind "/dev/$n" "$R/dev/$n"; done
ln -s /proc/self/fd "$R/dev/fd"
mv "$R/etc/resolv.conf" "$W/resolv.orig" 2>/dev/null || true
cp -L /etc/resolv.conf "$R/etc/resolv.conf"
printf '#!/bin/sh\nexit 101\n' >"$R/usr/sbin/policy-rc.d"; chmod 755 "$R/usr/sbin/policy-rc.d"
grep -rhoE 'https?://[^ ]+' "$R/etc/apt/sources.list.d" "$R/etc/apt/sources.list" 2>/dev/null | sort -u
X="env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root LANG=C.UTF-8 DEBIAN_FRONTEND=noninteractive chroot $R"
$X apt-get -o Acquire::Retries=3 update
$X apt-get -y -o Dpkg::Options::=--force-confold dist-upgrade
$X apt-get -y --no-install-recommends install systemd systemd-sysv dbus accountsservice packagekit btrfs-progs libssl3t64 libudev1 libgcc-s1 libstdc++6 ca-certificates jq curl less nano iproute2 procps fuse3 socat
$X apt-get clean
$X systemctl mask systemd-networkd-wait-online.service getty@.service systemd-udevd.service >/dev/null 2>&1 || true
$X dpkg-query -W -f '${Package} ${Version}\n' >"$W/packages.txt"
rm -f "$R/usr/sbin/policy-rc.d" "$R/etc/resolv.conf" "$R/etc/hostname" "$R/var/lib/dbus/machine-id" "$R"/etc/ssh/ssh_host_* "$R/var/lib/systemd/random-seed"
rm -rf "$R"/var/lib/apt/lists/* "$R"/var/log/*.log "$R"/var/log/apt/* "$R"/root/.bash_history "$R"/tmp/* 2>/dev/null || true
: >"$R/etc/machine-id"; : >"$R/etc/resolv.conf"; : >"$R/etc/hostname"
install -d -m 0755 "$R/agent/user" "$R/run"
umount -R "$R/dev"; umount "$R/proc"; umount "$R"
"$REPO/deploy/lxd/finish-resolute-template.sh" "$W" "$OUT"
