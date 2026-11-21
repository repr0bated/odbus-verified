#!/bin/sh
# Step 3b: refresh the op-dbus binaries inside an existing, STOPPED container's
# writable rootfs (e.g. 3xcontrolchatbot carries the pre-fix 3tched-fs that
# crash-loops on "duplicate namespace path /plugins/threetched").
#   35-refresh-binaries.sh <NAME>
# Old files are kept beside the new ones as <file>.pre-<stamp>.
. "$(dirname -- "$0")/lib.sh"; need_root
N=${1:?container}; R=$POOL_DIR/containers/$N/rootfs; B=$R/usr/local/libexec/op-dbus/bin
[ "$(incus list -f csv -c s "^$N\$")" = STOPPED ] || fail "$N must be stopped"
[ -d "$B" ] || fail "$B absent"
libc=$(chroot "$R" /lib/x86_64-linux-gnu/libc.so.6 | sed -n '1s/.*version \([0-9.]*\)\..*/\1/p')
need=$(objdump -T "$BIN_BRIDGE" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -n1 | cut -d_ -f2)
[ -z "$need" ] || [ "$(printf '%s\n%s\n' "$need" "$libc" | sort -V | head -n1)" = "$need" ] \
  || fail "rootfs glibc $libc < $need needed by $BIN_BRIDGE (build in-chroot: 25-build-in-chroot.sh)"
for pair in "$BIN_3TCHED_FS:3tched-fs" "$BIN_BRIDGE:op-grpc-bridge" "$BIN_TUNNEL_DOOR:op-tunnel-door"; do
  src=${pair%%:*}; dst=$B/${pair#*:}
  if [ -f "$dst" ] && cmp -s "$src" "$dst"; then echo "  ${pair#*:} current"; continue; fi
  [ -f "$dst" ] && cp -a "$dst" "$dst.pre-$STAMP"
  install -m 0755 "$src" "$dst.new" && mv "$dst.new" "$dst"
  echo "  ${pair#*:} -> $(sha256sum "$dst" | cut -c1-8) (old kept as .pre-$STAMP)"
done
install -m 0755 "$BIN_ACCEPT" "$R/usr/local/bin/op-identity-accept"; echo "  op-identity-accept installed"
