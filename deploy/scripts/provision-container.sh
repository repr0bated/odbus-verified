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
#      No NIC. Inside the container, opdbus-lo puts 10.0.0.2/32 on lo and the
#      seed's op-grpc-bridge serves the door on 10.0.0.2:8090; 3tchedfs and
#      threetchedfs mount both views. Lungs stay on the host: the host
#      bridge's lungs.sock is shared into every container, which it binds to
#      the calling container (BindCurrentContainer). An isolated idmap gives
#      each container its own host uid range. Devices (idmap-shifted):
#        /dev/fuse                     for the in-container 3tchedFS mounts
#        /run                          Incus tmpfs (source=tmpfs:), so systemd
#                                      does not cover the binds below it
#        /run/mounts/opdbus            host /run/opdbus/ct/<name> (the seed
#                                      links /run/opdbus here)
#        .../opdbus/lungs              host /run/opdbus/lungs, read-only, so
#                                      nothing in the container can replace
#                                      the host socket
#        /dev/shm/opdbus/plugin-blobs  the host's sealed catalog, read-only
#        .../opdbus/tunnel             chatbot only: the shared door socket
#                                      (op-tunnel-mux) its door connects out on
#      The door's TLS key is generated inside the container on first boot
#      (door-tls-init) and never exists on the host; its public certificate
#      appears at /run/opdbus/ct/<name>/door.crt and its fingerprint is
#      printed for pinning.
#      The chatbot has no internet. ID-N get NIC-less egress from
#      ct-egress-relay (loopback socat to host Unix sockets) after start.
#   4. The decoy minted the identity (IID1, delivered over WARP). The
#      container IS the identity; its sled is the identity volume
#      identity-<session> on the pool, mounted read-only at /agent/user:
#        session_id, issued_identity.iid1, sealed_id   fixed, never cleared
#        ledger/   the snowball session ledger, written by the host bridge,
#                  cleared after end-of-session vectorization
#      op-identity-accept verifies the IID1 against the decoy trust store and
#      installs its SID1 as sealed_id. The host's trust store (public keys)
#      is bound read-only for the container's own startup check. Nothing
#      mints here.
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
[ -S /run/opdbus/lungs/lungs.sock ] || fail "host lungs socket /run/opdbus/lungs/lungs.sock is absent"

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
install -d -m 0755 "$RUNDIR" "$RUNDIR/lungs" "$RUNDIR/tunnel"   # nested idmapped mount targets
incus config device add "$NAME" runtime-run disk source=tmpfs: path=/run initial.mode=0755 >/dev/null
incus config device add "$NAME" opdbus-run disk source="$RUNDIR" path=/run/mounts/opdbus shift=true >/dev/null
incus config device add "$NAME" lungs disk source=/run/opdbus/lungs path=/run/mounts/opdbus/lungs readonly=true shift=true >/dev/null
incus config device add "$NAME" catalog disk source="$CATALOG" path=/dev/shm/opdbus/plugin-blobs readonly=true shift=true >/dev/null
if [ "$NAME" = "$CHATBOT_NAME" ]; then
    incus config device add "$NAME" tunnel disk source=/run/opdbus/tunnel path=/run/mounts/opdbus/tunnel shift=true >/dev/null
fi

"$OP_CONTAINER" from-seed "$NAME"

SLED=identity-$SESSION
incus storage volume create "$POOL" "$SLED" >/dev/null
SLED_DIR=/var/lib/incus/storage-pools/$POOL/custom/default_$SLED
[ -d "$SLED_DIR" ] || fail "identity volume $SLED_DIR is absent"
printf '%s\n' "$SESSION" > "$SLED_DIR/session_id"
install -m 0600 "$ISSUED" "$SLED_DIR/issued_identity.iid1"
"$ACCEPT" --issued "$ISSUED" --pubkey "$KEY" --trust-store "$TRUST" \
    --out "$SLED_DIR/sealed_id" >/dev/null
install -d -m 0700 "$SLED_DIR/ledger"
incus config device add "$NAME" sled disk pool="$POOL" source="$SLED" path=/agent/user readonly=true >/dev/null
incus config device add "$NAME" decoy-trust disk source="$TRUST" path=/etc/op-dbus/decoy-trust.json readonly=true shift=true >/dev/null

incus start "$NAME"
if [ "$NAME" != "$CHATBOT_NAME" ]; then
    "$(dirname -- "$(readlink -f "$0")")/../runit/ct-egress-relay/install.sh" "$NAME"
fi
if [ "$NAME" = "$CHATBOT_NAME" ]; then
    i=0
    until [ -s "$RUNDIR/door.crt" ]; do
        i=$((i + 1)); [ "$i" -ge 60 ] && fail "door certificate did not appear in $RUNDIR"
        sleep 1
    done
    echo "door certificate $RUNDIR/door.crt"
    openssl x509 -in "$RUNDIR/door.crt" -noout -fingerprint -sha256
fi
echo "$NAME $SESSION"
