#!/bin/sh
# Adjust the imported Ubuntu 26.04.1 seed (images/@ubuntu-user, writable after
# 10-seed-acquire.sh) to the current systemd seed topology, then derive
# @ubuntu-subsidiary and seal both read-only. Run as root from the repo.
# Never touches @seed, the source seed.img, or any container.
set -eu
REPO=/srv/git/odbus-verified
P=/var/lib/incus/storage-pools/3tched-pool/images
U=$P/@ubuntu-user; S=$P/@ubuntu-subsidiary; SEED=$P/@seed/rootfs
ST=$(date +%Y%m%d-%H%M%S)
log() { echo "[$(date +%H:%M:%S)] $*"; }
fail() { echo "adjust: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
[ -d "$U/rootfs" ] || fail "no $U/rootfs"
[ "$(btrfs property get -ts "$U" ro | cut -d= -f2)" = false ] || fail "$U is sealed"
[ -e "$S" ] && fail "$S exists"
R=$U/rootfs

# 0. rollback point
btrfs subvolume snapshot -r "$U" "$P/@ubuntu-user.as-imported-$ST" >/dev/null
log "rollback snapshot $P/@ubuntu-user.as-imported-$ST"

keep() { [ -e "$1" ] || [ -L "$1" ] && mv "$1" "$1.pre-$ST"; return 0; }

# 1. current topology files, from git HEAD (deploy/seed/rootfs)
git -C "$REPO" ls-files -s deploy/seed/rootfs | while read -r mode _ _ path; do
  dst=$R/${path#deploy/seed/rootfs/}
  install -d -m 0755 "$(dirname "$dst")"
  tmp=$(mktemp); git -C "$REPO" show "HEAD:$path" >"$tmp"
  if [ -e "$dst" ] && cmp -s "$tmp" "$dst"; then rm -f "$tmp"; continue; fi
  keep "$dst"; install -m "${mode#100}" "$tmp" "$dst"; rm -f "$tmp"
  log "file ${dst#$R}"
done

# 2. binaries: byte-identical with the current @seed
for f in usr/local/libexec/op-dbus/bin/3tched-fs usr/local/libexec/op-dbus/bin/op-grpc-bridge \
         usr/local/libexec/op-dbus/bin/op-tunnel-door usr/local/bin/op-identity-accept \
         usr/local/bin/zcall usr/share/bash-completion/completions/zcall usr/bin/nextdns; do
  [ -e "$SEED/$f" ] || fail "@seed lacks $f"
  if [ -e "$R/$f" ] && cmp -s "$SEED/$f" "$R/$f"; then continue; fi
  install -d -m 0755 "$(dirname "$R/$f")"; keep "$R/$f"
  cp -a "$SEED/$f" "$R/$f"; log "bin /$f $(sha256sum "$R/$f" | cut -c1-16)"
done
# the image's old op-grpc-bridge symlink in /usr/bin points into /usr/libexec; drop if dangling
[ -L "$R/usr/bin/op-grpc-bridge" ] && [ ! -e "$R/usr/bin/op-grpc-bridge" ] && { keep "$R/usr/bin/op-grpc-bridge"; log "dangling /usr/bin/op-grpc-bridge kept aside"; }

# 3. enable the same op units as @seed (multi-user.target.wants)
W=$R/etc/systemd/system/multi-user.target.wants; install -d -m 0755 "$W"
for u in $(ls "$SEED/etc/systemd/system/multi-user.target.wants" | grep -E '^(3tchedfs|threetchedfs|opdbus-lo|door-tls-init|op-grpc-bridge|op-tunnel-door|nextdns)\.service$'); do
  ln -sfn "/etc/systemd/system/$u" "$W/$u"; log "enabled $u"
done
# gRPC-ready client config: profile store + defaults (see LXD template).
install -m 0644 "$REPO/deploy/config/profile.d/op-dbus-env.sh" "$R/etc/profile.d/op-dbus-env.sh"
install -d -m 0755 "$R/etc/op-dbus/env"
printf '%s\n' 'localhost,127.0.0.1,::1,10.0.0.2' >"$R/etc/op-dbus/env/no-proxy"
printf '%s\n' '/etc/ssl/certs/ca-certificates.crt' >"$R/etc/op-dbus/env/ssl-cert-file"
rm -f "$R/etc/op-dbus/env/http-proxy" "$R/etc/op-dbus/env/https-proxy"

# 4. graphical login off, desktop kept
ln -sfn /lib/systemd/system/multi-user.target "$R/etc/systemd/system/default.target"
log "default.target -> multi-user.target (display manager not started)"

# 5. container base fixes, only where needed (seed README)
[ -e "$R/etc/initrd-release" ] && { keep "$R/etc/initrd-release"; log "initrd-release set aside"; }
[ -s "$R/etc/hostname" ] && { keep "$R/etc/hostname"; : >"$R/etc/hostname"; log "hostname emptied"; }
if grep -vE '^\s*(#|$)' "$R/etc/fstab" 2>/dev/null | grep -qv '^tmpfs[[:space:]]\+/tmp[[:space:]]'; then
  keep "$R/etc/fstab"; grep -E '^tmpfs[[:space:]]+/tmp[[:space:]]' "$R/etc/fstab.pre-$ST" >"$R/etc/fstab" || : >"$R/etc/fstab"
  log "fstab reduced to /tmp tmpfs"
fi

# 6. subsidiary: same image, no in-container bridge/door
btrfs subvolume snapshot "$U" "$S" >/dev/null
for u in op-grpc-bridge opdbus-lo door-tls-init op-tunnel-door; do
  rm -f "$S/rootfs/etc/systemd/system/multi-user.target.wants/$u.service"
done
sed -i 's/op-dbus identity seed (@ubuntu-user)/op-dbus subsidiary seed (@ubuntu-subsidiary)/' "$S/metadata.yaml"
log "subsidiary: bridge, lo address, door TLS, tunnel door disabled"

# 7. seal both
btrfs property set -ts "$U" ro true; btrfs property set -ts "$S" ro true
for v in "$U" "$S"; do
  echo "$v ro=$(btrfs property get -ts "$v" ro | cut -d= -f2) uuid=$(btrfs subvolume show "$v" | awk '/^[[:space:]]*UUID:/{print $2}')"
done
log "done"
