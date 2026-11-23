#!/bin/bash
# Clean Ubuntu 26.04 (Resolute Raccoon) unified LXD image. Run as root in a
# private mount+pid namespace. No network service, keys, machine id or login.
set -euo pipefail
umask 022
W=/var/lib/op-dbus/lxd-template-build; OUT=/var/lib/op-dbus/lxd-images/ubuntu-resolute-template-20261123.tar.zst
U=https://cdimage.ubuntu.com/ubuntu-base/releases/26.04/release; T=ubuntu-base-26.04.1-base-amd64.tar.gz
KR=/mnt/cozo-btrfs-lxd/seeds/@ubuntu-subsidiary/rootfs/usr/share/keyrings/ubuntu-archive-keyring.gpg
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
$X apt-get -y --no-install-recommends install systemd systemd-sysv dbus ca-certificates jq less nano iproute2 procps fuse3
$X apt-get clean
$X systemctl mask systemd-networkd-wait-online.service getty@.service systemd-udevd.service >/dev/null 2>&1 || true
$X dpkg-query -W -f '${Package} ${Version}\n' >"$W/packages.txt"
rm -f "$R/usr/sbin/policy-rc.d" "$R/etc/resolv.conf" "$R/etc/hostname" "$R/var/lib/dbus/machine-id" "$R"/etc/ssh/ssh_host_* "$R/var/lib/systemd/random-seed"
rm -rf "$R"/var/lib/apt/lists/* "$R"/var/log/*.log "$R"/var/log/apt/* "$R"/root/.bash_history "$R"/tmp/* 2>/dev/null || true
: >"$R/etc/machine-id"; : >"$R/etc/resolv.conf"; : >"$R/etc/hostname"
install -d -m 0755 "$R/agent/user" "$R/run"
umount -R "$R/dev"; umount "$R/proc"; umount "$R"
awk -F: 'length($2)>3{print "password set: "$1; b=1}END{exit b}' "$R/etc/shadow"
install -d "$W/img/templates"
printf '{{ container.name }}\n' >"$W/img/templates/hostname.tpl"
printf '127.0.0.1 localhost\n127.0.1.1 {{ container.name }}\n::1 localhost ip6-localhost ip6-loopback\n' >"$W/img/templates/hosts.tpl"
cat >"$W/img/metadata.yaml" <<Y
architecture: x86_64
creation_date: $(date +%s)
properties:
  description: Ubuntu 26.04 LTS minimal LXC template
  os: ubuntu
  release: "26.04"
  codename: resolute
templates:
  /etc/hostname:
    when: [create, copy]
    template: hostname.tpl
  /etc/hosts:
    when: [create, copy]
    template: hosts.tpl
Y
tar --numeric-owner --xattrs --xattrs-include='*' --acls -C "$W/img" -cf - metadata.yaml templates rootfs | zstd -q -T4 -10 -o "$OUT.part"
chown 0:0 "$OUT.part"; chmod 600 "$OUT.part"; mv "$OUT.part" "$OUT"
sha256sum "$OUT"; du -h "$OUT"; du -sh "$W/img/rootfs"; wc -l <"$W/packages.txt"
grep -E '^(PRETTY_NAME|VERSION_CODENAME)' "$W/img/rootfs/usr/lib/os-release"
