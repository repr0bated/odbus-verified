#!/bin/sh
# Step 3: provision ONE identity container (the container IS the identity).
#   30-provision-container.sh new <wireguard_pubkey> <issued_identity.iid1> [NAME]
#       fresh container from the sealed seed + new identity sled volume
#   30-provision-container.sh complete <NAME>
#       an existing container that already carries its identity sled
#       (e.g. 3xcontrolchatbot): verify the sled, set the session labels and
#       add the missing devices. Idempotent; prints every change.
# Nothing is started unless START=1. Nothing is deleted. A failure leaves the
# stopped container for inspection (same contract as deploy/scripts/provision-container.sh).
if [ "${1:-}" = --subsidiary ]; then
  shift
  exec "$(dirname -- "$0")/31-provision-subsidiary.sh" "$@"
fi
. "$(dirname -- "$0")/lib.sh"; need_root
. /etc/profile.d/op-dbus-env.sh 2>/dev/null || true
CHATBOT_KEY=${OP_CHATBOT_WIREGUARD_PUBKEY:-}
mkdir -p "$LOGDIR"
[ -S /run/opdbus/lungs/lungs.sock ] || fail "host lungs socket absent (host op-grpc-bridge must be running)"
[ -r "$CATALOG/.manifest.json" ] || fail "sealed catalog $CATALOG absent"
[ -r "$TRUST" ] || fail "decoy trust store $TRUST absent"
# Unprivileged (isolated idmap) containers must traverse the pool containers
# dir to mount their credentials; Incus creates it 0711. 0700 makes every start
# fail with "Permission denied ... .incus-systemd-credentials".
[ "$(stat -c %a "$POOL_DIR/containers")" = 711 ] || fail "$POOL_DIR/containers is mode $(stat -c %a "$POOL_DIR/containers"), Incus needs 0711"

session_of() { "$BIN_ACCEPT" derive --pubkey "$1" | awk '{print $1}'; }
dev_has() { incus config device get "$1" "$2" type >/dev/null 2>&1; }
dev_add() { n=$1; d=$2; shift 2
  if dev_has "$n" "$d"; then echo "  device $d present"; else incus config device add "$n" "$d" "$@" >/dev/null; echo "  + device $d $*"; fi; }
labelled() { incus list -f csv -c n "user.opdbus.session_id=$1"; }
verify_sled() { # dir session key
  [ "$(tr -d '\n' <"$1/session_id")" = "$2" ] || fail "sled session_id differs from $2"
  t=$(mktemp); rm -f "$t"
  "$BIN_ACCEPT" --issued "$1/issued_identity.iid1" --pubkey "$3" --trust-store "$TRUST" --out "$t" >/dev/null \
    || { rm -f "$t"; fail "issued identity does not verify for this key"; }
  cmp -s "$t" "$1/sealed_id" || { rm -f "$t"; fail "sled sealed_id differs from the decoy-issued identity"; }
  rm -f "$t"; echo "  sled verified (IID1 vs trust store, sealed_id matches)"; }
wire_devices() { # name
  n=$1; RUNDIR=/run/opdbus/ct/$n
  # lungs and tunnel are mounted inside the opdbus-run disk; with idmapped
  # mounts Incus does not create nested targets, so start fails without them.
  install -d -m 0755 "$RUNDIR" "$RUNDIR/lungs" "$RUNDIR/tunnel"
  dev_add "$n" fuse unix-char source=/dev/fuse path=/dev/fuse mode=0666
  # /run is its own Incus tmpfs disk (not a nested bind under systemd's tmpfs): systemd finds /run
  # already mounted and does not cover the nested /run/mounts/opdbus binds with
  # its own tmpfs. Incus orders disks by path, so /run comes first.
  # Exactly the trusted shape {type:disk, source:tmpfs:, path:/run}
  # (crates/op-plugins/src/state_plugins/incus.rs:1612): the sealed typed Incus
  # device contract cannot carry initial.mode, and the host bridge refuses to
  # adopt an identity whose device has it (incus.rs:142).
  dev_add "$n" runtime-run disk source=tmpfs: path=/run
  dev_add "$n" opdbus-run disk source="$RUNDIR" path=/run/mounts/opdbus shift=true
  dev_add "$n" lungs disk source=/run/opdbus/lungs path=/run/mounts/opdbus/lungs readonly=true shift=true
  # Outside /dev/shm: the guest init mounts its own tmpfs there. This source/path
  # pair is the one the host bridge admits (runtime_identity_native.rs).
  dev_add "$n" catalog disk source="$CATALOG" path=/var/lib/opdbus/plugin-blobs readonly=true shift=true
  dev_add "$n" decoy-trust disk source="$TRUST" path=/etc/op-dbus/decoy-trust.json readonly=true shift=true
  if [ "$(incus config get "$n" user.opdbus.wireguard_public_key)" = "$CHATBOT_KEY" ] && [ -n "$CHATBOT_KEY" ]; then
    dev_add "$n" tunnel disk source=/run/opdbus/tunnel path=/run/mounts/opdbus/tunnel shift=true
  else
    dev_has "$n" tunnel && fail "refusing browser provisioning for a container with a control tunnel"
    [ ! -L "$RUNDIR/tunnel-browser" ] || fail "unsafe browser mount target"
    mkdir -p "$RUNDIR/tunnel-browser"
    dev_add "$n" tunnel-browser disk source=/run/opdbus/tunnel-browser path=/run/mounts/opdbus/tunnel-browser readonly=true shift=true
    python3 "$HERE/browser-port.py" "$SESSION" "${OP_UI_LISTEN:-10.0.0.2:8090}" "$DIR"
  fi; }

case ${1:-} in
complete)
  NAME=${2:?container name}
  incus info "$NAME" >/dev/null 2>&1 || fail "no container $NAME"
  [ "$(incus list -f csv -c s "^$NAME\$")" = STOPPED ] || fail "$NAME must be stopped to rewire"
  incus config show "$NAME" >"$LOGDIR/$NAME.config.before-$STAMP.yaml"
  SLED=$(incus config device get "$NAME" sled source 2>/dev/null) || fail "$NAME has no sled device"
  SESSION=${SLED#identity-}; DIR=$POOL_DIR/custom/default_$SLED
  KEY=$(incus config get "$NAME" user.opdbus.wireguard_public_key)
  [ -n "$KEY" ] || KEY=$(sed -n 's/.*"wireguard_pubkey"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$DIR/identity_sled.json" | head -n1)
  [ -n "$KEY" ] || fail "no key for $NAME (config or sled identity_sled.json)"
  [ "$(session_of "$KEY")" = "$SESSION" ] || fail "key does not derive session $SESSION"
  verify_sled "$DIR" "$SESSION" "$KEY"
  others=$(labelled "$SESSION" | grep -vx "$NAME" || true)
  [ -z "$others" ] || fail "session $SESSION already labelled on: $others (must be exactly one)"
  incus config set "$NAME" user.opdbus.session_id="$SESSION" user.opdbus.wireguard_public_key="$KEY"
  echo "  labels user.opdbus.session_id / wireguard_public_key set"
  wire_devices "$NAME"
  ;;
new)
  KEY=${2:?pubkey}; ISSUED=${3:?issued_identity.iid1}
  SESSION=$(session_of "$KEY"); [ -n "$SESSION" ] || fail "cannot derive session"
  [ -z "$(labelled "$SESSION")" ] || fail "session $SESSION already has a container"
  NAME=${4:-}
  if [ -z "$NAME" ]; then
    last=$(incus list -f csv -c n | sed -n 's/^ID-\([1-9][0-9]*\)$/\1/p' | sort -n | tail -n1); NAME=ID-$(( ${last:-0} + 1 ))
  fi
  incus info "$NAME" >/dev/null 2>&1 && fail "container $NAME already exists"
  [ "$(btrfs property get -ts "$SEED_DIR" ro | cut -d= -f2)" = true ] || fail "seed $SEED_NAME is not sealed (run 20-seed-overlay.sh)"
  INIT=; case $(readlink "$SEED_DIR/rootfs/sbin/init") in *runit*) INIT="
lxc.init.cmd = /sbin/runit-init";; esac
  log "provisioning $NAME (session $SESSION) from $SEED_NAME on $POOL"
  incus create --empty "$NAME" -s "$POOL" --no-profiles -c boot.autostart=false \
    -c security.privileged=false -c security.nesting=false -c security.idmap.isolated=true \
    -c "user.opdbus.session_id=$SESSION" -c "user.opdbus.wireguard_public_key=$KEY" \
    -c "raw.lxc=lxc.uts.name = $SESSION
lxc.mount.auto = cgroup2:force$INIT"
  if [ "$SEED_NAME" = @seed ]; then
    "$BIN_OP_CONTAINER" from-seed "$NAME"
  else
    # op-container only knows images/@seed: same CoW-snapshot swap by hand.
    C=$POOL_DIR/containers/$NAME; cp -a "$C/backup.yaml" "/tmp/$NAME.backup.yaml" 2>/dev/null || true
    [ -z "$(ls -A "$C" | grep -vx -e backup.yaml -e rootfs -e templates)" ] || fail "$C is not an empty placeholder"
    btrfs subvolume delete "$C" >/dev/null           # the empty placeholder incus just created
    btrfs subvolume snapshot "$SEED_DIR" "$C" >/dev/null
    [ -f "/tmp/$NAME.backup.yaml" ] && cp -a "/tmp/$NAME.backup.yaml" "$C/backup.yaml"
    echo "$SESSION" >"$C/rootfs/etc/hostname"
  fi
  SLED=identity-$SESSION
  incus storage volume create "$POOL" "$SLED" >/dev/null
  DIR=$POOL_DIR/custom/default_$SLED; [ -d "$DIR" ] || fail "sled $DIR absent"
  printf '%s\n' "$SESSION" >"$DIR/session_id"
  printf '%s\n' "$KEY" >"$DIR/wireguard_pubkey"        # public; read by the nextdns service
  install -m 0600 "$ISSUED" "$DIR/issued_identity.iid1"
  "$BIN_ACCEPT" --issued "$ISSUED" --pubkey "$KEY" --trust-store "$TRUST" --out "$DIR/sealed_id" >/dev/null
  install -d -m 0700 "$DIR/ledger"
  # Personal memory + soul store (memory/cozo), opened only by the host bridge
  # (cognitive-memory-rocksdb); nocow before any file exists.
  install -d -m 0700 "$DIR/memory"; chattr +C "$DIR/memory" 2>/dev/null || true
  incus config device add "$NAME" sled disk pool="$POOL" source="$SLED" path=/agent/user readonly=true >/dev/null
  wire_devices "$NAME"
  ;;
*) fail "usage: $0 new <pubkey> <iid1> [NAME] | complete <NAME>" ;;
esac

if [ "${START:-0}" = 1 ]; then
  incus start "$NAME"; log "started $NAME"
  # NIC-less BPF egress for every container, not only ID-* (owner 2026-11-23).
  "$REPO/deploy/runit/ct-egress-bpf/install.sh" "$NAME"
else
  log "$NAME provisioned, NOT started (START=1 to start)"
fi
echo "$NAME $SESSION"
