#!/bin/sh
# provision-container.sh <wireguard_pubkey> <issued_identity.iid1>
#
# Host root. Provision one identity container from the shared seed:
#   1. session id = derived from the key (op-identity-accept derive); it is
#      the container's lifetime identifier and its hostname.
#   2. Incus name: 3tchedChatbot for the control-plane chatbot key
#      (OP_CHATBOT_WIREGUARD_PUBKEY, the decoy WireGuard server key),
#      otherwise the next free ID-N.
#   3. incus create --empty on 3tched-pool, then op-container from-seed: the
#      container volume becomes a writable CoW snapshot of images/@seed.
#      No NIC. The seed runs the identity owner inside the container:
#      opdbus-lo puts 10.0.0.2/32 on lo, op-grpc-bridge serves the door on
#      10.0.0.2:8090 and lungs.sock, and 3tchedfs/threetchedfs mount both
#      views over that lungs.sock. An isolated idmap gives each container its
#      own host uid range. Devices (idmap-shifted):
#        /dev/fuse                     for the in-container 3tchedFS mounts
#        /opt/run-mounts/opdbus        host /run/opdbus/ct/<name>; the seed
#                                      links /run/opdbus here, so the host
#                                      reaches this container's lungs.sock
#        /dev/shm/opdbus/plugin-blobs  the host's sealed catalog, read-only
#        /opt/run-mounts/opdbus/tunnel chatbot only: op-tunnel-mux delivers
#                                      the host door into its door
#      The chatbot has no internet. ID-N get NIC-less egress from
#      ct-egress-relay (loopback socat to host Unix sockets) after start.
#   4. The decoy minted the identity (IID1, delivered over WARP). It is placed
#      in the container and op-identity-accept installs its SID1 as sealed_id
#      after verifying it against the decoy trust store. Nothing mints here.
#   5. Start. The container adopts its decoy-issued identity at startup.
#
# A failure after step 3 leaves the stopped container for inspection; it is
# never deleted or recreated automatically.
set -eu

[ $# -eq 2 ] || { echo "usage: $0 <wireguard_pubkey> <issued_identity.iid1>" >&2; exit 2; }
KEY=$1
ISSUED=$2
. /etc/profile.d/op-dbus-env.sh
POOL=${OP_CONTAINER_POOL:?set /etc/op-dbus/env/op-container-pool}
TRUST=${OP_DECOY_TRUST_STORE:-/etc/op-dbus/decoy-trust.json}
CATALOG=${OP_PLUGIN_CATALOG:-/var/lib/opdbus/plugin-blobs}
ACCEPT=${OP_IDENTITY_ACCEPT:-/usr/local/bin/op-identity-accept}
OP_CONTAINER=${OP_CONTAINER:-/usr/local/bin/op-container}
CHATBOT_NAME=3tchedChatbot

fail() { echo "provision-container: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
[ -r "$ISSUED" ] || fail "issued identity $ISSUED is not readable"
[ -r "$TRUST" ] || fail "decoy trust store $TRUST is absent"
[ -x "$ACCEPT" ] && [ -x "$OP_CONTAINER" ] || fail "op-identity-accept and op-container are required"
[ -d "$CATALOG" ] || fail "sealed catalog $CATALOG is absent"

SESSION=$("$ACCEPT" derive --pubkey "$KEY" | awk '{print $1}')
[ -n "$SESSION" ] || fail "cannot derive the session id for $KEY"

if incus list -f csv -c n "user.opdbus.session_id=$SESSION" | grep -q .; then
    fail "session $SESSION already has a container"
fi

if [ "$KEY" = "${OP_CHATBOT_WIREGUARD_PUBKEY:-}" ]; then
    NAME=$CHATBOT_NAME
else
    last=$(incus list -f csv -c n | sed -n 's/^ID-\([1-9][0-9]*\)$/\1/p' | sort -n | tail -n1)
    NAME=ID-$(( ${last:-0} + 1 ))
fi
incus info "$NAME" >/dev/null 2>&1 && fail "container $NAME already exists"

echo "provisioning $NAME (session $SESSION) on $POOL"
incus create --empty "$NAME" -s "$POOL" --no-profiles \
    -c boot.autostart=false \
    -c security.privileged=false \
    -c security.nesting=false \
    -c security.idmap.isolated=true \
    -c "user.opdbus.session_id=$SESSION" \
    -c "user.opdbus.wireguard_public_key=$KEY" \
    -c "raw.lxc=lxc.uts.name = $SESSION
lxc.mount.auto = cgroup2:force"
incus config device add "$NAME" fuse unix-char source=/dev/fuse path=/dev/fuse mode=0666 >/dev/null
RUNDIR=/run/opdbus/ct/$NAME
install -d -m 0755 "$RUNDIR"
incus config device add "$NAME" opdbus-run disk source="$RUNDIR" path=/opt/run-mounts/opdbus shift=true >/dev/null
incus config device add "$NAME" catalog disk source="$CATALOG" path=/dev/shm/opdbus/plugin-blobs readonly=true shift=true >/dev/null
if [ "$NAME" = "$CHATBOT_NAME" ]; then
    incus config device add "$NAME" tunnel disk source=/run/opdbus/tunnel path=/opt/run-mounts/opdbus/tunnel shift=true >/dev/null
fi

"$OP_CONTAINER" from-seed "$NAME"

ROOT=/var/lib/incus/storage-pools/$POOL/containers/$NAME/rootfs
install -d -m 0700 "$ROOT/agent" "$ROOT/agent/user"
install -m 0600 "$ISSUED" "$ROOT/agent/user/issued_identity.iid1"
"$ACCEPT" --issued "$ISSUED" --pubkey "$KEY" --trust-store "$TRUST" \
    --out "$ROOT/agent/user/sealed_id" >/dev/null

incus start "$NAME"
if [ "$NAME" != "$CHATBOT_NAME" ]; then
    "$(dirname -- "$(readlink -f "$0")")/../runit/ct-egress-relay/install.sh" "$NAME"
fi
echo "$NAME $SESSION"
