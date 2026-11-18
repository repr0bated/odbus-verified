#!/bin/sh
# Step 2: add the op-dbus extras to a runit seed, then seal it read-only.
#   20-seed-overlay.sh [--toolchain]
# Takes a read-only backup snapshot first: images/<SEED_NAME>.pre-overlay-<stamp>.
# Refuses a sealed (ro) seed unless UNSEAL=1, and refuses while a RUNNING
# container is a snapshot of it (seed README: never while a container depends on it).
# The Ubuntu/systemd seed (@seed) uses deploy/seed/rootfs instead (see README).
. "$(dirname -- "$0")/lib.sh"; need_root
TOOLCHAIN=0; [ "${1:-}" = --toolchain ] && TOOLCHAIN=1
R=$SEED_DIR/rootfs
[ -d "$R" ] || fail "no seed rootfs at $R (run 10-seed-acquire.sh)"
case $(readlink "$R/sbin/init" 2>/dev/null) in *runit*) ;; *) fail "$SEED_NAME is not a runit seed";; esac
ro=$(btrfs property get -ts "$SEED_DIR" ro | cut -d= -f2)
if [ "$ro" = true ]; then
  [ "${UNSEAL:-0}" = 1 ] || fail "$SEED_NAME is sealed; set UNSEAL=1 to reopen it"
fi
for b in "$BIN_3TCHED_FS" "$BIN_BRIDGE" "$BIN_TUNNEL_DOOR" "$BIN_ACCEPT"; do [ -x "$b" ] || fail "binary $b missing"; done
[ -f "$TRUST" ] || fail "decoy trust store $TRUST missing"
seed_uuid=$(btrfs subvolume show "$SEED_DIR" | awk '/^[[:space:]]*UUID:/{print $2}')
for c in $(incus list -f csv -c n status=running); do
  pu=$(btrfs subvolume show "$POOL_DIR/containers/$c" 2>/dev/null | awk '/Parent UUID:/{print $3}')
  [ "$pu" = "$seed_uuid" ] && fail "running container $c is a snapshot of $SEED_NAME; stop it first"
done

BK=$POOL_DIR/images/$SEED_NAME.pre-overlay-$STAMP
btrfs subvolume snapshot -r "$SEED_DIR" "$BK" >/dev/null; log "backup snapshot $BK"
[ "$ro" = true ] && btrfs property set -ts "$SEED_DIR" ro false

# 1. binaries (gRPC libraries are compiled into op-grpc-bridge; nothing to add)
install -d -m 0755 "$R/usr/local/libexec/op-dbus/bin" "$R/usr/local/bin" "$R/etc/op-dbus" "$R/etc/op-dbus/secrets"
install -m 0755 "$BIN_3TCHED_FS"   "$R/usr/local/libexec/op-dbus/bin/3tched-fs"
install -m 0755 "$BIN_BRIDGE"      "$R/usr/local/libexec/op-dbus/bin/op-grpc-bridge"
install -m 0755 "$BIN_TUNNEL_DOOR" "$R/usr/local/libexec/op-dbus/bin/op-tunnel-door"
install -m 0755 "$BIN_ACCEPT"      "$R/usr/local/bin/op-identity-accept"
chmod 0700 "$R/etc/op-dbus/secrets"
# 2. seed overlay files from the repo (committed source of truth, read via git show)
git -C "$REPO" show HEAD:deploy/seed/rootfs/usr/local/libexec/op-dbus/door-tls-init >"$R/usr/local/libexec/op-dbus/door-tls-init"
chmod 0755 "$R/usr/local/libexec/op-dbus/door-tls-init"
git -C "$REPO" show HEAD:deploy/seed/rootfs/etc/op-dbus/bridge.env >"$R/etc/op-dbus/bridge.env"
# 3. NextDNS: official release tarball, checksum pinned
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
curl -fsSL -o "$t/n.tgz" "https://github.com/nextdns/nextdns/releases/download/v$NEXTDNS_VERSION/nextdns_${NEXTDNS_VERSION}_linux_amd64.tar.gz"
echo "$NEXTDNS_SHA256  $t/n.tgz" | sha256sum -c - >/dev/null || fail "nextdns checksum mismatch"
tar -xzf "$t/n.tgz" -C "$t" nextdns; install -m 0755 "$t/nextdns" "$R/usr/bin/nextdns"
log "nextdns $("$R/usr/bin/nextdns" version 2>/dev/null | head -n1) installed"
# 4. base packages the run scripts use
missing=
for p in ip:iproute2 openssl:openssl fusermount3:fuse3 mountpoint:util-linux svlogd:runit curl:curl; do
  chroot "$R" sh -c "command -v ${p%%:*}" >/dev/null 2>&1 || missing="$missing ${p#*:}"
done
pkgs=$missing
[ $TOOLCHAIN = 1 ] && pkgs="$pkgs build-essential pkg-config protobuf-compiler libprotobuf-dev libssl-dev clang cmake git"
if [ -n "$(echo $pkgs)" ]; then
  log "installing in seed chroot:$pkgs"
  cp -a "$R/etc/resolv.conf" "$t/resolv.saved" 2>/dev/null || true
  rm -f "$R/etc/resolv.conf"; cp -L /etc/resolv.conf "$R/etc/resolv.conf"
  mount -t proc proc "$R/proc"
  rc=0; chroot "$R" sh -c "export DEBIAN_FRONTEND=noninteractive; apt-get update -q && apt-get install -y -q --no-install-recommends $pkgs" || rc=$?
  umount "$R/proc"; rm -f "$R/etc/resolv.conf"
  [ -e "$t/resolv.saved" ] || [ -L "$t/resolv.saved" ] && cp -a "$t/resolv.saved" "$R/etc/resolv.conf"
  [ $rc = 0 ] || fail "apt in chroot failed"
fi
if [ $TOOLCHAIN = 1 ] && [ ! -x "$R/root/.cargo/bin/cargo" ]; then
  chroot "$R" sh -c 'curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal' || fail "rustup in chroot failed"
fi
# 5. runit services + stage-1 layout; container DNS through the local nextdns
install -d -m 0755 "$R/etc/runit/core-services" "$R/etc/runit/runsvdir/default"
install -m 0644 "$HERE/runit/core-services/90-opdbus.sh" "$R/etc/runit/core-services/90-opdbus.sh"
for s in opdbus-lo door-tls-init 3tchedfs threetchedfs op-grpc-bridge op-tunnel-door nextdns; do
  rm -rf "$R/etc/sv/$s"; cp -a "$HERE/runit/$s" "$R/etc/sv/$s"
  ln -sfn "/etc/sv/$s" "$R/etc/runit/runsvdir/default/$s"
done
printf 'nameserver 127.0.0.1\noptions edns0\n' >"$R/etc/resolv.conf.opdbus"
# 6. seal
btrfs property set -ts "$SEED_DIR" ro true
log "seed $SEED_NAME sealed read-only; rollback: $BK"
