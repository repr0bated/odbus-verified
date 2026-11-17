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
ACCEPT=${OP_IDENTITY_ACCEPT:-/usr/local/bin/op-identity-accept}
OP_CONTAINER=${OP_CONTAINER:-/usr/local/bin/op-container}
CHATBOT_NAME=3tchedChatbot

fail() { echo "provision-container: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
[ -r "$ISSUED" ] || fail "issued identity $ISSUED is not readable"
[ -r "$TRUST" ] || fail "decoy trust store $TRUST is absent"
[ -x "$ACCEPT" ] && [ -x "$OP_CONTAINER" ] || fail "op-identity-accept and op-container are required"

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
    -c "user.opdbus.session_id=$SESSION" \
    -c "user.opdbus.wireguard_public_key=$KEY" \
    -c "raw.lxc=lxc.uts.name = $SESSION
lxc.mount.auto = cgroup2:force"
incus config device add "$NAME" fuse unix-char source=/dev/fuse path=/dev/fuse mode=0666 >/dev/null

"$OP_CONTAINER" from-seed "$NAME"

ROOT=/var/lib/incus/storage-pools/$POOL/containers/$NAME/rootfs
install -d -m 0700 "$ROOT/agent" "$ROOT/agent/user"
install -m 0600 "$ISSUED" "$ROOT/agent/user/issued_identity.iid1"
"$ACCEPT" --issued "$ISSUED" --pubkey "$KEY" --trust-store "$TRUST" \
    --out "$ROOT/agent/user/sealed_id" >/dev/null

incus start "$NAME"
echo "$NAME $SESSION"
