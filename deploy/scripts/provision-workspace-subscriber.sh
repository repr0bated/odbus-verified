#!/bin/sh
# Provision an identity workspace subscriber — OS seed + shared sockets + memory.
#
# TEMPLATE for signup / re-provision. Modernized for live 3tched topology:
#   docs/operations/host-socket-topology-live.md
#   deploy/config/identities/container-template.json
#
# Attachment model (NOT Incus proxy, NOT per-CT NIC):
#   - Container name = session_id derived from WireGuard pubkey (UUID)
#   - Host owns /run/ghostbridge/container.sock (shared fabric)
#   - CT gets disk bind of /run/ghostbridge only
#   - Optional host loopbacks for app listeners; no eth0
#   - btrfs seed on pool `default`; fstorage.img via btrfs device add
#   - Memory namespaces on the single cognitive/cozo leaf (host fabric)
#
# Identity: prefer EXISTING keys (--pubkey / --identity-dir). Generate only
# when none supplied. Session IDs derive from the WireGuard public key.
#
# Email stored ONLY when GhostBridge privacy is off (same rule as before).
#
# Teardown / --recreate order (do not skip):
#   1. btrfs device delete <loop|img> <seed-rootfs>   # remove from array FIRST
#   2. losetup -d <loop>
#   3. incus stop + incus delete
# Deleting the CT while fstorage is still in the btrfs array can D-state the host.
#
# Usage:
#   sudo ./provision-workspace-subscriber.sh <role|username> \
#     [--pubkey B64] [--identity-dir PATH] [--private-key-file PATH] \
#     [--email ADDR] [--mesh-ip CIDR] \
#     [--image FINGERPRINT|alias] [--fstorage-gib N] \
#     [--ghostbridge] [--semantic] [--no-start] [--recreate]
#
# Examples (existing keys):
#   sudo ./provision-workspace-subscriber.sh jeremy \
#     --pubkey 'GEMLT/+I81zs5HDPsOF22ntqGf71OEy6eFKMm7P7Dzk=' \
#     --mesh-ip 100.69.0.2/32 --email jeremy@3tched.com --ghostbridge
#
#   sudo ./provision-workspace-subscriber.sh chatbot \
#     --identity-dir /var/lib/opdbus-runtime/identities/chatbot \
#     --mesh-ip 100.69.0.10/32 --ghostbridge
#
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
TEMPLATE_JSON="${REPO_ROOT}/deploy/config/identities/container-template.json"
PROFILE_YAML="${REPO_ROOT}/deploy/incus/profiles/identity.yaml"
RUNTIME_ROOT="${OPDBUS_IDENTITY_ROOT:-/var/lib/opdbus-runtime/identities}"
SHARED_GHOSTBRIDGE="${GHOSTBRIDGE_DIR:-/run/ghostbridge}"
SHARED_CONTAINER_SOCK="${GHOSTBRIDGE_SOCKET_PATH:-/run/ghostbridge/container.sock}"
HOST_GRPC_SOCK="${ZEROCLAW_UNIX_SOCKET:-/run/opdbus/grpc.sock}"
STORAGE_POOL="${IDENTITY_STORAGE_POOL:-default}"
# Local noble fingerprint when present; else remote alias.
DEFAULT_IMAGE="${IDENTITY_SEED_IMAGE:-3a639373bb7a}"
COGNITIVE_BUS_ADDRESS="${COGNITIVE_MCP_BUS_ADDRESS:-unix:path=/run/opdbus/session-bus.sock}"

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] || [ $# -eq 0 ]; then
    sed -n '2,45p' "$0"
    exit 0
fi

ROLE="${1:?role/username required}"
shift

PUBKEY=""
IDENTITY_DIR=""
PRIVATE_KEY_FILE=""
EMAIL=""
MESH_IP=""
IMAGE="$DEFAULT_IMAGE"
FSTORAGE_GIB=2
GHOSTBRIDGE=false
SEMANTIC=false
NO_START=false
RECREATE=false

while [ $# -gt 0 ]; do
    case "$1" in
        --pubkey)            shift; PUBKEY="${1:?}" ;;
        --identity-dir)      shift; IDENTITY_DIR="${1:?}" ;;
        --private-key-file)  shift; PRIVATE_KEY_FILE="${1:?}" ;;
        --email)             shift; EMAIL="${1:?}" ;;
        --mesh-ip)           shift; MESH_IP="${1:?}" ;;
        --image)             shift; IMAGE="${1:?}" ;;
        --fstorage-gib)      shift; FSTORAGE_GIB="${1:?}" ;;
        --session-id)        shift; SESSION_ID="${1:?}" ;;
        --ghostbridge)       GHOSTBRIDGE=true ;;
        --semantic)          SEMANTIC=true ;;
        --no-start)          NO_START=true ;;
        --recreate)          RECREATE=true ;;
        -h|--help)
            sed -n '2,45p' "$0"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

# ── helpers ──────────────────────────────────────────────────────────────────

die() { echo "ERROR: $*" >&2; exit 1; }

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

# session_id = blake3 derive_key("op-identity session-id v1", pubkey)[:16] as UUID
# Matches op_identity::session::derive_session_id
derive_session_id() {
    _pub="$1"
    # Known existing keys (prefer-existing re-provision without blake3 py module)
    case "$_pub" in
        'GEMLT/+I81zs5HDPsOF22ntqGf71OEy6eFKMm7P7Dzk=')
            echo 'f036f8d8-aabb-c5f2-49c9-18dac19f41ea'
            return 0
            ;;
        'VaRh9EUieQxA3zIoOj3qNiNIqZoPGpqztPU4muyF1zM=')
            echo 'bea37ecb-92be-197c-660f-09e806f1a34f'
            return 0
            ;;
        '/yBYjQkcD997wTd+e6b1rJiALMKLbLVfF2IO413lj1Y=')
            echo '1ce2775a-dd9d-a7bf-fb80-690e4c7bc29f'
            return 0
            ;;
    esac
    if command -v python3 >/dev/null 2>&1 && python3 -c 'import blake3' 2>/dev/null; then
        python3 - "$_pub" <<'PY'
import sys, uuid, blake3
pub = sys.argv[1]
key = blake3.blake3(pub.encode(), derive_key_context="op-identity session-id v1").digest()
print(str(uuid.UUID(bytes=key[:16])))
PY
        return
    fi
    # Optional cargo helper if present
    if [ -x /tmp/sid-tool/target/debug/sid-tool ]; then
        /tmp/sid-tool/target/debug/sid-tool 2>/dev/null | awk -v p="$_pub" '$3==p {print $2; exit}'
        return
    fi
    die "cannot derive session_id for pubkey (install python3-blake3, or pass --session-id)"
}

ensure_identity_profile() {
    if ! incus profile show identity >/dev/null 2>&1; then
        echo "    Creating Incus profile 'identity'..."
        incus profile create identity
    fi
    # Apply devices: root on btrfs default + ghostbridge bind. No nic, no proxy.
    # Mount host socket dir at /opt/run-mounts/ghostbridge — NOT /run/ghostbridge,
    # because container /run is tmpfs and Incus disk mounts there often vanish.
    GHOSTBRIDGE_CT_PATH="${GHOSTBRIDGE_CT_PATH:-/opt/run-mounts/ghostbridge}"
    incus profile device add identity root disk path=/ pool="$STORAGE_POOL" 2>/dev/null || \
        incus profile device set identity root pool="$STORAGE_POOL" 2>/dev/null || true
    if [ ! -d "$SHARED_GHOSTBRIDGE" ]; then
        mkdir -p "$SHARED_GHOSTBRIDGE"
    fi
    incus profile device add identity ghostbridge-socket disk \
        source="$SHARED_GHOSTBRIDGE" path="$GHOSTBRIDGE_CT_PATH" 2>/dev/null || \
        incus profile device set identity ghostbridge-socket \
            source="$SHARED_GHOSTBRIDGE" path="$GHOSTBRIDGE_CT_PATH" 2>/dev/null || true
    # Strip accidental nic/proxy from profile if present
    for d in $(incus profile device list identity 2>/dev/null || true); do
        case "$d" in
            eth0|eth*|proxy*|nic*)
                echo "    Removing forbidden profile device: $d"
                incus profile device remove identity "$d" 2>/dev/null || true
                ;;
        esac
    done
}

ensure_runtime_dir() {
    _dir="$1"
    mkdir -p "$_dir"
    chmod 755 "$_dir"
}

write_identity_meta() {
    _dir="$1"
    cat > "$_dir/identity.json" <<EOF
{
  "role": "$ROLE",
  "session_id": "$SESSION_ID",
  "pubkey": "$PEER_PUBKEY",
  "mesh_ip": "${MESH_IP:-}",
  "email": "${EMAIL:-}",
  "ghostbridge": $GHOSTBRIDGE,
  "shared_socket": "$SHARED_CONTAINER_SOCK",
  "fstorage": "$FSTORAGE_IMG",
  "template": "deploy/config/identities/container-template.json"
}
EOF
    printf '%s\n' "$PEER_PUBKEY" > "$_dir/public.key"
    chmod 644 "$_dir/public.key" "$_dir/identity.json"
}

ensure_fstorage() {
    _img="$1"
    _gib="$2"
    if [ -f "$_img" ]; then
        echo "    fstorage exists: $_img"
        return 0
    fi
    echo "    Creating fstorage ${_gib}GiB → $_img"
    truncate -s "${_gib}G" "$_img"
    chmod 600 "$_img"
}

# Resolve rootfs path for an Incus CT on STORAGE_POOL (host view).
container_rootfs() {
    echo "/var/lib/incus/storage-pools/${STORAGE_POOL}/containers/${1}/rootfs"
}

# Loop device currently backing fstorage image, if any.
fstorage_loop_for() {
    _img="$1"
    losetup -j "$_img" 2>/dev/null | head -1 | cut -d: -f1 || true
}

# Remove fstorage from the CT seed btrfs array BEFORE deleting the container.
# Order matters: delete-from-array → detach loop → stop CT → delete CT.
# Skipping step 1 is how we D-state the host (remove CT while device still in array).
detach_fstorage_before_delete() {
    _name="$1"
    _img="$2"
    _rootfs=$(container_rootfs "$_name")
    _timeout="${DETACH_FSTORAGE_TIMEOUT:-120}"

    echo "    [teardown] detach fstorage from btrfs array before deleting $_name"

    if [ ! -f "$_img" ]; then
        echo "    no fstorage image — nothing to remove from array"
        return 0
    fi

    _loop=$(fstorage_loop_for "$_img")

    # If rootfs is gone already, only free the loop
    if [ ! -d "$_rootfs" ]; then
        echo "    rootfs missing at $_rootfs — skip btrfs device delete"
        if [ -n "$_loop" ]; then
            losetup -d "$_loop" 2>/dev/null || true
            echo "    detached orphan loop $_loop"
        fi
        return 0
    fi

    # Ensure we have a loop if the image is still a FS member (path may show as file)
    if [ -z "$_loop" ]; then
        if btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_img"; then
            _loop=$(losetup -f --show "$_img" 2>/dev/null) || true
            echo "    re-looped $_img as ${_loop:-failed} for device delete"
        fi
    fi

    _dev="${_loop:-}"
    if [ -z "$_dev" ]; then
        # Last resort: try image path if btrfs lists it that way
        if btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_img"; then
            _dev="$_img"
        fi
    fi

    if [ -n "$_dev" ] && btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qE "$(basename "$_dev")|$_dev|$_img"; then
        echo "    btrfs device delete $_dev → $_rootfs (timeout ${_timeout}s)"
        # Prefer stop CT first so rootfs is quiet (still mounted on host for pool)
        incus stop "$_name" --timeout 30 2>/dev/null || incus stop "$_name" --force 2>/dev/null || true
        sleep 1
        if command -v timeout >/dev/null 2>&1; then
            if timeout "$_timeout" btrfs device delete "$_dev" "$_rootfs"; then
                echo "    ok: removed $_dev from btrfs array"
            else
                echo "    WARN: btrfs device delete failed/timeout — abort CT delete to avoid host hang"
                echo "    Fix manually: btrfs device delete $_dev $_rootfs && losetup -d <loop>"
                return 1
            fi
        else
            if btrfs device delete "$_dev" "$_rootfs"; then
                echo "    ok: removed $_dev from btrfs array"
            else
                echo "    WARN: btrfs device delete failed — abort CT delete"
                return 1
            fi
        fi
    else
        echo "    fstorage not listed on FS for $_name — skip device delete"
        # Still stop for clean delete
        incus stop "$_name" --timeout 30 2>/dev/null || incus stop "$_name" --force 2>/dev/null || true
    fi

    # Detach loop after it is no longer a FS member
    _loop=$(fstorage_loop_for "$_img")
    if [ -n "$_loop" ]; then
        if losetup -d "$_loop" 2>/dev/null; then
            echo "    detached loop $_loop"
        else
            echo "    WARN: could not detach $_loop (may still be busy)"
        fi
    fi
    return 0
}

# Safe delete: array teardown then incus delete.
safe_delete_container() {
    _name="$1"
    _img="$2"
    if ! incus info "$_name" >/dev/null 2>&1; then
        echo "    $_name does not exist — nothing to delete"
        # Still free any orphan loop for this image
        _loop=$(fstorage_loop_for "$_img")
        if [ -n "$_loop" ]; then
            losetup -d "$_loop" 2>/dev/null || true
        fi
        return 0
    fi
    if ! detach_fstorage_before_delete "$_name" "$_img"; then
        die "refusing to delete $_name while fstorage may still be in the btrfs array"
    fi
    echo "    incus delete $_name --force"
    incus delete "$_name" --force
}

# Persistent storage: btrfs device add of a host loop image onto the CT seed
# rootfs (cleaner multi-device model). Preferred path — ATTACH_FSTORAGE=1 by
# default. 2026-07-22 hang may have been remove/recreate contention (D-state
# on btrfs mutex while pool was busy), not the model itself — monitor.
#
# Safety:
#   - never attach if the loop is already a device of this FS
#   - hard timeout so a wedged pool cannot block signup forever
#   - ATTACH_FSTORAGE=0 to skip and leave fstorage.img as file-only
attach_fstorage_best_effort() {
    _img="$1"
    _name="$2"
    _rootfs="/var/lib/incus/storage-pools/${STORAGE_POOL}/containers/${_name}/rootfs"
    if [ "${ATTACH_FSTORAGE:-1}" = "0" ]; then
        echo "    fstorage image at $_img (ATTACH_FSTORAGE=0 — skip device add)"
        return 0
    fi
    if [ ! -f "$_img" ]; then
        return 0
    fi
    if [ ! -d "$_rootfs" ]; then
        echo "    fstorage attach skipped (rootfs not at $_rootfs)"
        return 0
    fi
    if ! command -v btrfs >/dev/null 2>&1; then
        echo "    fstorage attach skipped (no btrfs tool)"
        return 0
    fi

    # Already attached? (device path or image path listed on this FS)
    if btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_img"; then
        echo "    fstorage already on FS for $_name"
        return 0
    fi

    # Prefer a free loop; reuse if this image is already loop-backed
    _loop=""
    _existing=$(losetup -j "$_img" 2>/dev/null | head -1 | cut -d: -f1 || true)
    if [ -n "$_existing" ]; then
        _loop="$_existing"
        echo "    reusing loop $_loop for $_img"
    else
        _loop=$(losetup -f --show "$_img" 2>/dev/null) || {
            echo "    fstorage losetup failed"
            return 0
        }
    fi

    # If this loop is already a device of the FS, done
    if btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_loop"; then
        echo "    fstorage already attached as $_loop"
        return 0
    fi

    _timeout="${ATTACH_FSTORAGE_TIMEOUT:-60}"
    echo "    btrfs device add $_loop → $_rootfs (timeout ${_timeout}s)"
    if command -v timeout >/dev/null 2>&1; then
        if timeout "$_timeout" btrfs device add "$_loop" "$_rootfs"; then
            echo "    ok: persistent device added"
            return 0
        fi
        _rc=$?
    else
        if btrfs device add "$_loop" "$_rootfs"; then
            echo "    ok: persistent device added"
            return 0
        fi
        _rc=$?
    fi
    echo "    WARN: btrfs device add failed/timeout (rc=${_rc:-?}) — image kept at $_img"
    echo "    (do NOT force-remove mid-add; if hung, reboot — avoid concurrent remove+add)"
    # Only detach loop if add clearly failed and loop was created this run
    if [ -z "${_existing:-}" ]; then
        losetup -d "$_loop" 2>/dev/null || true
    fi
}

register_shared_socket_metadata() {
    _name="$1"
    # Metadata-only registration path: host already binds container.sock.
    # Prefer zcall/createunixsocket when available; else record intent file.
    _regdir="/var/lib/opdbus-runtime/socket-registrations"
    mkdir -p "$_regdir"
    cat > "$_regdir/${_name}.json" <<EOF
{
  "name": "$_name",
  "shared_socket": "$SHARED_CONTAINER_SOCK",
  "ports": [],
  "protocol": "grpc",
  "registered_by": "provision-workspace-subscriber.sh",
  "note": "Host owns the bind; this is demux metadata only. Heartbeat client not deployed."
}
EOF
    echo "    shared-socket registration metadata → $_regdir/${_name}.json"
    if command -v zcall >/dev/null 2>&1; then
        zcall unix_socket createunixsocket "{\"name\":\"$_name\",\"ports\":[]}" 2>/dev/null \
            && echo "    zcall createunixsocket ok" \
            || echo "    (zcall createunixsocket unavailable — metadata file only)"
    fi
}

remember() {
    NS="$1"; KEY="$2"; VAL="$3"
    # Host-local provisioning uses the bridge-owned session-bus door. Identity
    # is derived from bus policy; never replay a bearer token or self-asserted
    # footprint into the cognitive path. Keep the seed file as a recoverable
    # fallback when the bridge/session bus is unavailable.
    _call_args=$(jq -cn \
        --arg tool_name "cognitive_memory" \
        --arg namespace "$NS" \
        --arg key "$KEY" \
        --argjson value "$VAL" \
        '{tool_name:$tool_name,arguments:{operation:"store",namespace:$namespace,key:$key,value:$value}}' \
        2>/dev/null || true)
    if [ -n "$_call_args" ] && \
        busctl --address="$COGNITIVE_BUS_ADDRESS" call \
            org.opdbus.v1.plugins \
            /org/opdbus/v1/plugins/cognitive_mcp \
            org.opdbus.v1.PluginV1 Call ss invoke_tool "$_call_args" \
            >/dev/null 2>&1; then
        echo "    ok: $NS/$KEY"
    else
        _seed="$IDENTITY_DIR/memory-seed.jsonl"
        printf '%s\n' "{\"namespace\":\"$NS\",\"key\":\"$KEY\",\"value\":$VAL}" >> "$_seed"
        echo "    seed-file: $NS/$KEY → $_seed (bridge session-bus path unavailable)"
    fi
}

# ── resolve keys ─────────────────────────────────────────────────────────────

need_cmd incus
need_cmd wg

if [ -z "$IDENTITY_DIR" ]; then
    IDENTITY_DIR="${RUNTIME_ROOT}/${ROLE}"
fi
ensure_runtime_dir "$IDENTITY_DIR"

if [ -z "$PUBKEY" ] && [ -f "$IDENTITY_DIR/public.key" ]; then
    PUBKEY=$(tr -d ' \n' < "$IDENTITY_DIR/public.key")
fi
if [ -z "$PRIVATE_KEY_FILE" ] && [ -f "$IDENTITY_DIR/private.key" ]; then
    PRIVATE_KEY_FILE="$IDENTITY_DIR/private.key"
fi

if [ -z "$PUBKEY" ]; then
    echo "[keys] No --pubkey / public.key — generating new WireGuard keypair in $IDENTITY_DIR"
    wg genkey | tee "$IDENTITY_DIR/private.key" | wg pubkey > "$IDENTITY_DIR/public.key"
    chmod 600 "$IDENTITY_DIR/private.key"
    chmod 644 "$IDENTITY_DIR/public.key"
    PUBKEY=$(tr -d ' \n' < "$IDENTITY_DIR/public.key")
else
    echo "[keys] Using existing pubkey"
    printf '%s\n' "$PUBKEY" > "$IDENTITY_DIR/public.key"
    chmod 644 "$IDENTITY_DIR/public.key"
    if [ -n "$PRIVATE_KEY_FILE" ] && [ -f "$PRIVATE_KEY_FILE" ]; then
        # Avoid cp same-file when --identity-dir already holds private.key
        case "$PRIVATE_KEY_FILE" in
            "$IDENTITY_DIR/private.key"|"$IDENTITY_DIR"/private.key) ;;
            *)
                cp "$PRIVATE_KEY_FILE" "$IDENTITY_DIR/private.key"
                ;;
        esac
        chmod 600 "$IDENTITY_DIR/private.key" 2>/dev/null || true
    fi
fi

PEER_PUBKEY="$PUBKEY"

if [ -n "${SESSION_ID:-}" ]; then
    :
else
    SESSION_ID=$(derive_session_id "$PEER_PUBKEY") || die "session_id derivation failed"
fi
CONTAINER_ID="$SESSION_ID"

TOKEN=$(uuidgen -v5 "6ba7b812-9dad-11d1-80b4-00c04fd430c8" "$PEER_PUBKEY" 2>/dev/null \
    || python3 -c "import uuid; print(uuid.uuid5(uuid.NAMESPACE_OID, '''$PEER_PUBKEY'''))")

FSTORAGE_IMG="${IDENTITY_DIR}/fstorage.img"

echo "=== Provision identity workspace ==="
echo "    Role / user   : $ROLE"
echo "    Session ID    : $SESSION_ID  (= container name)"
echo "    WG pubkey     : $PEER_PUBKEY"
echo "    Identity dir  : $IDENTITY_DIR"
echo "    Shared UDS    : $SHARED_CONTAINER_SOCK"
echo "    Host gRPC UDS : $HOST_GRPC_SOCK (host tools; not CT mount)"
echo "    Seed image    : $IMAGE"
echo "    Storage pool  : $STORAGE_POOL (btrfs seed)"
echo "    fstorage      : $FSTORAGE_IMG (${FSTORAGE_GIB}GiB)"
echo "    Mesh IP       : ${MESH_IP:-<none>}"
echo "    Email         : ${EMAIL:-<none>}"
echo "    GhostBridge   : $GHOSTBRIDGE"
echo "    Semantic      : $SEMANTIC"
echo "    Template      : $TEMPLATE_JSON"
echo ""

if [ ! -S "$SHARED_CONTAINER_SOCK" ]; then
    echo "WARN: $SHARED_CONTAINER_SOCK not present — start op-grpc-bridge / opdbus-rundirs first"
fi

# ── 0. Profile (sockets) ─────────────────────────────────────────────────────
echo "[0] Ensuring Incus profile 'identity' (shared socket mount, no NIC/proxy)..."
ensure_identity_profile

# ── 1. OS seed container ─────────────────────────────────────────────────────
echo "[1] OS seed container ($CONTAINER_ID)..."
if incus info "$CONTAINER_ID" >/dev/null 2>&1; then
    if [ "$RECREATE" = "true" ]; then
        echo "    --recreate: remove fstorage from btrfs array, then delete $CONTAINER_ID"
        safe_delete_container "$CONTAINER_ID" "$FSTORAGE_IMG"
    else
        echo "    Already exists — reusing (pass --recreate to wipe)"
    fi
fi

if ! incus info "$CONTAINER_ID" >/dev/null 2>&1; then
    # Prefer profile identity only (includes root on default pool + ghostbridge).
    # Do not attach default profile if it ever gains a NIC.
    if ! incus init "$IMAGE" "$CONTAINER_ID" --storage "$STORAGE_POOL" --profile identity 2>/dev/null; then
        # Fallback: empty profiles + explicit devices
        incus init "$IMAGE" "$CONTAINER_ID" --storage "$STORAGE_POOL" --no-profiles
        GHOSTBRIDGE_CT_PATH="${GHOSTBRIDGE_CT_PATH:-/opt/run-mounts/ghostbridge}"
        incus config device add "$CONTAINER_ID" root disk path=/ pool="$STORAGE_POOL"
        incus config device add "$CONTAINER_ID" ghostbridge-socket disk \
            source="$SHARED_GHOSTBRIDGE" path="$GHOSTBRIDGE_CT_PATH"
    fi
    # Identity dir mount (per-role keys)
    GHOSTBRIDGE_CT_PATH="${GHOSTBRIDGE_CT_PATH:-/opt/run-mounts/ghostbridge}"
    incus config device add "$CONTAINER_ID" identity disk \
        source="$IDENTITY_DIR" path=/opt/run-mounts/identity readonly=true 2>/dev/null || \
        incus config device set "$CONTAINER_ID" identity source="$IDENTITY_DIR" 2>/dev/null || true

    incus config set "$CONTAINER_ID" \
        user.opdbus.role="$ROLE" \
        user.opdbus.session_id="$SESSION_ID" \
        user.opdbus.wireguard_public_key="$PEER_PUBKEY" \
        user.opdbus.shared_socket="$SHARED_CONTAINER_SOCK" \
        boot.autostart=false

    # Hard rule: strip any NIC or proxy that slipped in
    for d in $(incus config device list "$CONTAINER_ID" 2>/dev/null || true); do
        _type=$(incus config device get "$CONTAINER_ID" "$d" type 2>/dev/null || true)
        case "$_type" in
            nic|proxy)
                echo "    Removing forbidden device $d (type=$_type)"
                incus config device remove "$CONTAINER_ID" "$d" || true
                ;;
        esac
    done
    echo "    Created $CONTAINER_ID (NIC-less, shared UDS mount)"
fi

# Always ensure ghostbridge + identity devices on re-run
GHOSTBRIDGE_CT_PATH="${GHOSTBRIDGE_CT_PATH:-/opt/run-mounts/ghostbridge}"
incus config device add "$CONTAINER_ID" ghostbridge-socket disk \
    source="$SHARED_GHOSTBRIDGE" path="$GHOSTBRIDGE_CT_PATH" 2>/dev/null || \
    incus config device set "$CONTAINER_ID" ghostbridge-socket \
        path="$GHOSTBRIDGE_CT_PATH" source="$SHARED_GHOSTBRIDGE" 2>/dev/null || true
incus config device add "$CONTAINER_ID" identity disk \
    source="$IDENTITY_DIR" path=/opt/run-mounts/identity readonly=true 2>/dev/null || true

if [ "$NO_START" != "true" ]; then
    incus start "$CONTAINER_ID" 2>/dev/null || true
    echo "    Waiting for init..."
    sleep 2
    incus exec "$CONTAINER_ID" -- sh -c \
        'for i in 1 2 3 4 5 6 7 8 9 10; do [ -d /proc/1 ] && exit 0; sleep 1; done' \
        2>/dev/null || true
fi

# ── 2. Base OS packages (inside CT) ──────────────────────────────────────────
if [ "$NO_START" != "true" ] && incus info "$CONTAINER_ID" 2>/dev/null | grep -q 'Status: RUNNING'; then
    echo "[2] Base OS packages..."
    if incus exec "$CONTAINER_ID" -- test -x /usr/bin/apt-get 2>/dev/null; then
        incus exec "$CONTAINER_ID" -- apt-get update -qq 2>/dev/null || true
        incus exec "$CONTAINER_ID" -- apt-get install -y --no-install-recommends \
            curl ca-certificates wireguard-tools iproute2 2>/dev/null || true
    elif incus exec "$CONTAINER_ID" -- test -x /usr/bin/pacman 2>/dev/null; then
        incus exec "$CONTAINER_ID" -- pacman -Sy --noconfirm curl wireguard-tools iproute2 2>/dev/null || true
    else
        echo "    (unknown package manager — skip)"
    fi

    # Materialize keys inside CT from host identity dir (already bind-mounted RO)
    incus exec "$CONTAINER_ID" -- sh -c '
        mkdir -p /etc/wireguard
        if [ -f /opt/run-mounts/identity/private.key ]; then
            cp /opt/run-mounts/identity/private.key /etc/wireguard/privkey
            chmod 600 /etc/wireguard/privkey
        fi
        if [ -f /opt/run-mounts/identity/public.key ]; then
            cp /opt/run-mounts/identity/public.key /etc/wireguard/pubkey
        fi
        # Prove shared socket is visible (host-owned file)
        ls -la /run/ghostbridge/ 2>/dev/null || true
    ' 2>/dev/null || true
else
    echo "[2] Base OS packages skipped (not running / --no-start)"
fi

# ── 3. Identity metadata + mesh notes ────────────────────────────────────────
echo "[3] Identity metadata..."
write_identity_meta "$IDENTITY_DIR"
if [ -n "$MESH_IP" ]; then
    echo "    Mesh IP $MESH_IP — ensure host 3tched peer AllowedIPs includes it"
    echo "    (wg set 3tched peer <pubkey> allowed-ips $MESH_IP)"
fi
if [ "$GHOSTBRIDGE" = "true" ]; then
    echo "    GhostBridge: shared UDS path (no Netmaker CT NIC registration required for control plane)"
fi

# ── 4. fstorage (btrfs device add onto seed) ─────────────────────────────────
echo "[4] fstorage..."
ensure_fstorage "$FSTORAGE_IMG" "$FSTORAGE_GIB"
if [ "$NO_START" != "true" ]; then
    attach_fstorage_best_effort "$FSTORAGE_IMG" "$CONTAINER_ID"
fi

# ── 5. Shared socket registration (metadata) ─────────────────────────────────
echo "[5] Shared socket registration..."
register_shared_socket_metadata "$CONTAINER_ID"

# ── 6. Semantic flag ─────────────────────────────────────────────────────────
if [ "$SEMANTIC" = "true" ]; then
    echo "[6] Semantic search: enabled (Qdrant collection on first use via control plane)"
else
    echo "[6] Semantic search: skipped"
fi

# ── 7. Memory namespaces (single leaf) ───────────────────────────────────────
echo "[7] Memory namespaces (single cognitive leaf)..."
: > "${IDENTITY_DIR}/memory-seed.jsonl" 2>/dev/null || true
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

remember "container:${CONTAINER_ID}:identity" "wireguard_pubkey" "\"$PEER_PUBKEY\""
remember "container:${CONTAINER_ID}:identity" "mcp_token" "\"$TOKEN\""
remember "container:${CONTAINER_ID}:identity" "session_id" "\"$SESSION_ID\""
remember "container:${CONTAINER_ID}:identity" "shared_socket" "\"$SHARED_CONTAINER_SOCK\""
if [ -n "$MESH_IP" ]; then
    remember "container:${CONTAINER_ID}:identity" "mesh_ip" "\"$MESH_IP\""
fi
if [ -n "$EMAIL" ] && [ "$GHOSTBRIDGE" = "false" ]; then
    remember "container:${CONTAINER_ID}:identity" "email" "\"$EMAIL\""
fi

remember "container:${CONTAINER_ID}:soul" "profile" \
    "{\"kind\":\"soul\",\"container_id\":\"$CONTAINER_ID\",\"role\":\"$ROLE\",\"created_at\":\"$NOW\"}"

for DOMAIN in work personal home; do
    remember "container:${CONTAINER_ID}:domain:${DOMAIN}" "MEMORY_INDEX" \
        "{\"kind\":\"domain\",\"domain\":\"$DOMAIN\",\"container_id\":\"$CONTAINER_ID\",\"entries\":[]}"
done

remember "container:${CONTAINER_ID}:index" "MEMORY_INDEX" \
    "{\"kind\":\"index\",\"container_id\":\"$CONTAINER_ID\",\"namespaces\":[\"identity\",\"soul\",\"domain:work\",\"domain:personal\",\"domain:home\"]}"

remember "container:${CONTAINER_ID}:features" "ghostbridge" "{\"enabled\":$GHOSTBRIDGE}"
remember "container:${CONTAINER_ID}:features" "semantic_search" "{\"enabled\":$SEMANTIC}"
remember "container:${CONTAINER_ID}:features" "shared_socket" \
    "{\"path\":\"$SHARED_CONTAINER_SOCK\",\"proxy\":false,\"nic\":false}"

# ── 8. Heartbeat note (not deployed) ─────────────────────────────────────────
echo "[8] Heartbeat: ComponentRegistry.Heartbeat is API-only — identity client not deployed"

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Done ==="
incus list "$CONTAINER_ID" 2>/dev/null || true
echo ""
echo "Identity:"
echo "  session_id  : $SESSION_ID"
echo "  WG pubkey   : $PEER_PUBKEY"
echo "  MCP token   : $TOKEN (UUID v5 of pubkey)"
echo "  runtime dir : $IDENTITY_DIR"
echo "  fstorage    : $FSTORAGE_IMG"
echo "  shared UDS  : $SHARED_CONTAINER_SOCK (host bind; CT mount /run/ghostbridge)"
echo ""
echo "Sockets (correct model):"
echo "  YES  disk bind  host:$SHARED_GHOSTBRIDGE → CT:/run/ghostbridge"
echo "  YES  host owns  $SHARED_CONTAINER_SOCK"
echo "  NO   Incus proxy devices for identity"
echo "  NO   container NIC (xray only)"
echo ""
echo "Memory namespaces:"
echo "  container:${CONTAINER_ID}:identity|soul|domain:*|index|features"
echo ""
echo "Enter:  incus exec $CONTAINER_ID -- bash"
echo "Re-run: sudo $0 $ROLE --pubkey '$PEER_PUBKEY' --ghostbridge"
