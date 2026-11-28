#!/bin/sh
# Shared settings for the provisioning procedure. Sourced by every step.
# Override any value in the environment before running a step.
set -eu
POOL=${OP_CONTAINER_POOL:-3tched-pool}
POOL_DIR=/var/lib/incus/storage-pools/$POOL
SEED_NAME=${SEED_NAME:-@seed}          # images/<SEED_NAME>; antiX 26.1, systemd (never deleted)
SEED_DIR=$POOL_DIR/images/$SEED_NAME
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=${OP_REPO:-/srv/git/odbus-verified}
# Binaries copied into the image (host copies already in service on antix1).
BIN_3TCHED_FS=${BIN_3TCHED_FS:-/usr/local/libexec/op-dbus/bin/3tched-fs-dupfix-b37bffa2}
BIN_BRIDGE=${BIN_BRIDGE:-/usr/local/libexec/op-dbus/bin/op-grpc-bridge}
BIN_TUNNEL_DOOR=${BIN_TUNNEL_DOOR:-$REPO/target/release/op-tunnel-door}
BIN_ACCEPT=${BIN_ACCEPT:-/usr/local/bin/op-identity-accept}
BIN_OP_CONTAINER=${BIN_OP_CONTAINER:-$REPO/target/release/op-container}
# Official NextDNS client release (github.com/nextdns/nextdns).
NEXTDNS_VERSION=${NEXTDNS_VERSION:-1.50.2}
NEXTDNS_SHA256=${NEXTDNS_SHA256:-df65a100940decdd564a37024a8261be566a629c9747a909f70421c616a2575c}
CATALOG=${OP_PLUGIN_CATALOG:-/var/lib/opdbus/plugin-blobs}
TRUST=${OP_DECOY_TRUST_STORE:-/etc/op-dbus/decoy-trust.json}
STAMP=$(date +%Y%m%d-%H%M%S)
LOGDIR=${PROV_LOGDIR:-/home/jeremy/provisioning/runs}
fail() { echo "provisioning: $*" >&2; exit 1; }
need_root() { [ "$(id -u)" = 0 ] || fail "run as root (sudo)"; }
log() { echo "[$(date +%H:%M:%S)] $*"; }

# Runit guest settings, never a shared .env/bridge.env file.
write_guest_bridge_settings() {
  root=$1
  install -d -m 0700 "$root/etc/op-dbus/env"
  while read -r name value; do
    printf '%s\n' "$value" >"$root/etc/op-dbus/env/$name"
    chmod 0600 "$root/etc/op-dbus/env/$name"
  done <<'SETTINGS'
OP_GRPC_BRIDGE_LISTEN 10.0.0.2:8090
OPDBUS_LUNGS_SOCKET /run/op-grpc-bridge/lungs.sock
OPDBUS_READY_PATH /run/op-grpc-bridge/ready
THREETCHED_TLS_CERT_FILE /var/lib/op-dbus/door-tls/door.crt
THREETCHED_TLS_KEY_FILE /var/lib/op-dbus/door-tls/door.key
SETTINGS
  # The container's own UI door stays off unless it is given its own address.
  [ -n "${OP_UI_LISTEN:-}" ] || return 0
  printf '%s\n' "$OP_UI_LISTEN" >"$root/etc/op-dbus/env/OP_UI_LISTEN"
  chmod 0600 "$root/etc/op-dbus/env/OP_UI_LISTEN"
}
