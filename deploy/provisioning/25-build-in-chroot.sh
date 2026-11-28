#!/bin/sh
# Step 2b (release design): build the binaries with cargo INSIDE a chroot of
# the seed OS, so they link against the seed's own glibc. Needed whenever the
# host-built binaries need a newer glibc than the seed (antix1: host/Ubuntu
# 26.04 = 2.43, @antix-seed Debian trixie = 2.41 -> op-grpc-bridge cannot run).
#   UNSEAL=1 SEED_NAME=@antix-seed 20-seed-overlay.sh --toolchain   (once)
#   25-build-in-chroot.sh                 # then re-run 20-seed-overlay.sh with
#                                         # BIN_*=<seed>/opt/op-build/target/release/...
# Source is a clean `git archive HEAD` export (no worktree state), built as root
# in /opt/op-build of the writable seed. Long-running (bridge is ~150 MB).
. "$(dirname -- "$0")/lib.sh"; need_root
R=$SEED_DIR/rootfs
[ "$(btrfs property get -ts "$SEED_DIR" ro | cut -d= -f2)" = false ] || fail "seed is sealed; UNSEAL it via 20-seed-overlay.sh flow first"
[ -x "$R/root/.cargo/bin/cargo" ] || fail "no toolchain in seed (20-seed-overlay.sh --toolchain)"
rm -rf "$R/opt/op-build.src"; mkdir -p "$R/opt/op-build.src"
git -C "$REPO" archive HEAD | tar -x -C "$R/opt/op-build.src"
t=$(mktemp -d); cp -a "$R/etc/resolv.conf" "$t/resolv" 2>/dev/null || true
rm -f "$R/etc/resolv.conf"; cp -L /etc/resolv.conf "$R/etc/resolv.conf"
mount -t proc proc "$R/proc"
trap 'umount "$R/proc"; rm -f "$R/etc/resolv.conf"; [ -e "$t/resolv" ] || [ -L "$t/resolv" ] && cp -a "$t/resolv" "$R/etc/resolv.conf"; rm -rf "$t"' EXIT
chroot "$R" sh -c 'cd /opt/op-build.src && CARGO_TARGET_DIR=/opt/op-build/target \
  /root/.cargo/bin/cargo build --release -p op-grpc-bridge -p _3tched-fs -p op-tunnel-mux -p op-identity'
ls -la "$R/opt/op-build/target/release/" | grep -E "op-grpc-bridge|3tched-fs|op-tunnel-door|op-identity-accept"
