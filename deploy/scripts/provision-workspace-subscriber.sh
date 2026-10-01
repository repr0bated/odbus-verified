#!/usr/bin/env bash
# Prepare a physical root for an EXISTING authoritative identity.
# Usage:
#   sudo ./provision-workspace-subscriber.sh LABEL --session-id UUID --pubkey B64 \
#     --operator-session UUID --leaf-path ABSOLUTE_PATH [--fstorage-gib N] [--no-start]
#   sudo ./provision-workspace-subscriber.sh NAME --system \
#     [--image ALIAS|FINGERPRINT] [--storage POOL] [--no-start]
#   sudo ./provision-workspace-subscriber.sh --system NAME [system options]
#
# Identity mode never signs up an account, generates/copies credentials,
# deletes/recreates a container, or attaches a leaf to an Incus filesystem.
# The session UUID is the native name; LABEL is display metadata.
# Existing native names and storage receipts are refused.
# --no-start prepares storage, reconciles and verifies attach, leaving STOPPED.
# Normal mode requires an inactive target with a canonical stored instance;
# supported touch then starts it after attached readback. --no-start preserves
# prior active metadata. Credential bytes are not requested or inspected;
# supported method results redact them. This wrapper never mints credentials.
# Completion enables root/FUSE restoration on boot; --no-start records a
# stopped boot policy. Incomplete storage never receives a boot-ready marker.
# Failures retain native/storage intent and private sanitized receipts.
# Host helper/catalog defaults select activated release artifacts.
# Selected overrides must be root-controlled. This is host operator maintenance,
# not a guest-accessible host socket or a new identity minting authority.
# --system remains ordinary Raccoon cloud provisioning without identity changes.
set -euo pipefail
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
need_cmd() { command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }
usage() { sed -n '2,20p' "$0"; }
if [[ $# == 0 || ${1:-} == -h || ${1:-} == --help ]]; then usage; exit 0; fi
SYSTEM_MODE=false SYSTEM_NAME="" ROLE=""
if [[ $1 == --system ]]; then
    SYSTEM_MODE=true; shift; SYSTEM_NAME=${1:?container name required}; shift
else ROLE=$1; shift; fi
SYSTEM_IMAGE=${SYSTEM_CONTAINER_IMAGE:-raccoon}
SYSTEM_STORAGE_POOL=${SYSTEM_STORAGE_POOL:-3tched-storage}
IMAGE=${IDENTITY_SEED_IMAGE:-3a639373bb7a} IMAGE_OVERRIDDEN=false
SYSTEM_STORAGE=$SYSTEM_STORAGE_POOL SYSTEM_STORAGE_OVERRIDDEN=false
IDENTITY_ONLY_FLAGS="" PUBKEY="" SESSION_ID="" OPERATOR_SESSION="" LEAF_PATH=""
FSTORAGE_GIB=2 NO_START=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --pubkey|--session-id|--operator-session|--leaf-path|--fstorage-gib)
            option=$1; IDENTITY_ONLY_FLAGS="$IDENTITY_ONLY_FLAGS $option"
            shift; value=${1:?option requires a value}
            case "$option" in
                --pubkey) PUBKEY=$value ;; --session-id) SESSION_ID=$value ;;
                --operator-session) OPERATOR_SESSION=$value ;; --leaf-path) LEAF_PATH=$value ;;
                --fstorage-gib) FSTORAGE_GIB=$value ;;
            esac ;;
        --image) shift; IMAGE=${1:?}; IMAGE_OVERRIDDEN=true ;;
        --storage) shift; SYSTEM_STORAGE=${1:?}; SYSTEM_STORAGE_OVERRIDDEN=true ;;
        --no-start) NO_START=true ;;
        --system)
            [[ $SYSTEM_MODE == false ]] || die '--system specified more than once'
            SYSTEM_MODE=true; SYSTEM_NAME=$ROLE ;;
        --identity-dir|--private-key-file|--email|--mesh-ip)
            IDENTITY_ONLY_FLAGS="$IDENTITY_ONLY_FLAGS $1"; shift; : "${1:?}"
            [[ $SYSTEM_MODE == true ]] || die 'legacy identity options are retired; use an existing authoritative session' ;;
        --ghostbridge|--semantic|--recreate)
            IDENTITY_ONLY_FLAGS="$IDENTITY_ONLY_FLAGS $1"
            [[ $SYSTEM_MODE == true ]] || die 'legacy identity options are retired; no recreate or credential/socket mounts' ;;
        -h|--help) usage; exit 0 ;; *) die "unknown option: $1" ;;
    esac
    shift
done
provision_system_container() {
    [ -z "$IDENTITY_ONLY_FLAGS" ] || die "--system cannot be combined with identity-only options:$IDENTITY_ONLY_FLAGS"
    [ -n "$SYSTEM_NAME" ] || die '--system requires a container name'
    [ "${#SYSTEM_NAME}" -le 63 ] || die 'system container name must be at most 63 characters'
    case "$SYSTEM_NAME" in
        [!A-Za-z0-9]*|*[!A-Za-z0-9-]*) die 'system container name must begin with a letter or digit and contain only letters, digits, and hyphens' ;;
        *-) die 'system container name must end with a letter or digit' ;;
    esac
    [ -n "$IMAGE" ] || die 'system image must not be empty'
    case "$IMAGE" in -*) die 'system image must not begin with an option prefix' ;; esac
    [ -n "$SYSTEM_STORAGE" ] || die 'system storage pool must not be empty'
    case "$SYSTEM_STORAGE" in
        [!A-Za-z0-9]*|*[!A-Za-z0-9_.-]*) die 'system storage pool must begin with a letter or digit and contain only letters, digits, dot, underscore, and hyphen' ;;
    esac

    need_cmd incus
    need_cmd jq
    instance_list=$(incus query '/1.0/instances?recursion=1') || die 'cannot list Incus instances; refusing to create'
    printf '%s\n' "$instance_list" | jq -e 'type == "array" and all(.[]; type == "object" and (.name | type == "string"))' >/dev/null ||
        die 'Incus instance inventory response is invalid; refusing to create'
    if printf '%s\n' "$instance_list" | jq -e --arg name "$SYSTEM_NAME" 'any(.[]; .name == $name)' >/dev/null; then
        die "Incus instance '$SYSTEM_NAME' already exists; system provisioning never replaces existing instances"
    fi

    echo "=== Provision standalone system container ==="
    echo "    Name          : $SYSTEM_NAME"
    echo "    Image         : $IMAGE"
    echo "    Storage pool  : $SYSTEM_STORAGE"
    echo "    Start         : $([ "$NO_START" = true ] && echo no || echo yes)"
    incus init "$IMAGE" "$SYSTEM_NAME" --storage "$SYSTEM_STORAGE" --no-profiles \
        --config security.privileged=false \
        --config security.nesting=false \
        --config boot.autostart=false
    if [ "$NO_START" != true ]; then
        incus start "$SYSTEM_NAME"
    fi
}

if [[ $SYSTEM_MODE == true ]]; then
    if [[ $IMAGE_OVERRIDDEN == false ]]; then IMAGE=$SYSTEM_IMAGE; fi
    provision_system_container; exit 0
fi
root_path() {
    local path mode
    path=$(realpath -e -- "$1") || die 'selected root-controlled path is absent'
    while :; do
        [[ $(stat -c %u -- "$path") == 0 ]] || die 'selected path is not root-owned'
        mode=$(stat -c %a -- "$path")
        (( (8#$mode & 022) == 0 )) || die 'selected path is writable by another owner'
        [[ $path == / ]] && break
        path=$(dirname -- "$path")
    done
}
valid_uuid() { [[ $1 =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; }
[[ $(id -u) == 0 ]] || die 'identity physical-root preparation requires host root'
[[ $SYSTEM_STORAGE_OVERRIDDEN == false && $IMAGE_OVERRIDDEN == false ]] ||
    die '--image and --storage are system-only; identity roots use the pinned physical seed'
valid_uuid "$SESSION_ID" || die 'explicit canonical --session-id is required'
valid_uuid "$OPERATOR_SESSION" || die 'explicit authorized --operator-session is required'
[[ $PUBKEY =~ ^[A-Za-z0-9+/]{43}=$ ]] || die 'explicit canonical 32-byte Base64 --pubkey is required'
[[ ${#ROLE} -ge 1 && ${#ROLE} -le 63 && $ROLE =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || die 'invalid display label'
[[ $FSTORAGE_GIB =~ ^[1-9][0-9]{0,3}$ ]] && (( FSTORAGE_GIB <= 1024 )) || die 'leaf size must be 1 to 1024 GiB'
[[ $LEAF_PATH == /* && $LEAF_PATH != *$'\n'* && $LEAF_PATH != *$'\r'* ]] || die 'explicit absolute --leaf-path is required'
for command in incus jq realpath stat timeout mktemp find cmp base64; do need_cmd "$command"; done
[[ $(printf '%s' "$PUBKEY" | base64 -d | base64 -w0) == "$PUBKEY" ]] || die 'public key encoding is not canonical'
[[ $(realpath -m -- "$LEAF_PATH") == "$LEAF_PATH" ]] || die 'leaf path must be normalized'
[[ ! -e $LEAF_PATH && ! -L $LEAF_PATH ]] || die 'leaf already exists; never recreate'
ROOT_HELPER=$(realpath -e -- "${OP_IDENTITY_ROOT_HELPER:-/usr/local/libexec/3tched/op-identity-root}")
FS_BINARY=$(realpath -e -- "${THREETCHED_FS:-/usr/local/bin/threetched-fs}")
CATALOG=$(realpath -e -- "${OP_IDENTITY_SEED_CATALOG:-/var/lib/opdbus/plugin-blobs}")
LUNGS_HOOK=$(realpath -e -- "${OP_IDENTITY_SEED_LUNGS_HOOK:-$(dirname -- "$ROOT_HELPER")/op-identity-lungs-hook}")
STATE_ROOT=${OP_IDENTITY_SEED_STATE:-/var/lib/opdbus-runtime/seed-identities}
RECEIPT_ROOT=${OP_IDENTITY_PROVISION_RECEIPTS:-/var/lib/opdbus-runtime/identity-provision-receipts}
root_path "$ROOT_HELPER"; root_path "$FS_BINARY"; root_path "$CATALOG"; root_path "$LUNGS_HOOK"
[[ -f $ROOT_HELPER && -x $ROOT_HELPER && -f $FS_BINARY && -x $FS_BINARY && -d $CATALOG ]] || die 'invalid selected artifacts'
[[ -f $LUNGS_HOOK && -x $LUNGS_HOOK ]] || die 'invalid selected lungs hook'
# Helpers must use the same selected artifacts, including received layouts.
export THREETCHED_FS="$FS_BINARY" OP_IDENTITY_SEED_CATALOG="$CATALOG"
export OP_IDENTITY_SEED_LUNGS_HOOK="$LUNGS_HOOK" OP_IDENTITY_SEED_STATE="$STATE_ROOT"
[[ $(stat -c %a -- "$(dirname -- "$LEAF_PATH")") == 700 ]] || die 'leaf parent must already be private mode 0700'
root_path "$(dirname -- "$LEAF_PATH")"
[[ $STATE_ROOT == /* && $(realpath -m -- "$STATE_ROOT") == "$STATE_ROOT" ]] || die 'invalid seed state root'
root_path "$(dirname -- "$STATE_ROOT")"
[[ ! -e $STATE_ROOT/$SESSION_ID && ! -L $STATE_ROOT/$SESSION_ID ]] || die 'seed receipt exists; inspect retained intent'
[[ $RECEIPT_ROOT == /* && $(realpath -m -- "$RECEIPT_ROOT") == "$RECEIPT_ROOT" ]] || die 'invalid operator receipt root'
root_path "$(dirname -- "$RECEIPT_ROOT")"
umask 077
mkdir -p -- "$RECEIPT_ROOT"; root_path "$RECEIPT_ROOT"
[[ $(stat -c %a -- "$RECEIPT_ROOT") == 700 ]] || die 'operator receipt root must be mode 0700'
WORK=$(mktemp -d "$RECEIPT_ROOT/$SESSION_ID.XXXXXX")
mkdir -- "$WORK/empty-state" "$WORK/store"
printf 'Private sanitized receipts: %s\n' "$WORK"
trap 'printf "Failure; retained private receipts at %s; no automatic cleanup.\n" "$WORK" >&2' ERR
"$FS_BINARY" --store "$WORK/store" capture --catalog "$CATALOG" --state "$WORK/empty-state" >/dev/null 2>/dev/null ||
    die 'schema-only capture failed; no native mutation performed'

# No credential input. Every response passes a metadata allowlist before storage.
mcp_call() (
    set -euo pipefail
    tool=$1 args=$2 kind=$3
    coproc MCP { timeout -k 5 90 "$FS_BINARY" --store "$WORK/store" mcp \
        --state "$WORK/empty-state" --lungs-socket /run/opdbus/lungs.sock \
        --lungs-session "$OPERATOR_SESSION" --all 2>/dev/null; }
    pid=$MCP_PID old_read=${MCP[0]} old_write=${MCP[1]}
    exec {rfd}<&"$old_read"; exec {wfd}>&"$old_write"
    exec {old_read}<&-; exec {old_write}>&-
    trap 'exec {wfd}>&-; exec {rfd}<&-; wait "$pid" 2>/dev/null || true' EXIT
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"existing-identity-root-operator","version":"1"}}}' >&"$wfd"
    ready=false matched=false
    while IFS= read -r -t 30 -u "$rfd" line; do
        ((${#line} <= 2097152)) || exit 1
        if jq -e '.id==1 and has("result")' >/dev/null 2>&1 <<<"$line"; then ready=true; break; fi
    done
    [[ $ready == true ]] || exit 1
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&"$wfd"
    jq -cn --arg tool "$tool" --argjson args "$args" '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:$tool,arguments:$args}}' >&"$wfd"
    while IFS= read -r -t 60 -u "$rfd" line; do
        ((${#line} <= 2097152)) || exit 1
        if jq -e '.id==2' >/dev/null 2>&1 <<<"$line"; then matched=true; break; fi
    done
    [[ $matched == true ]] || exit 1
    jq -ce --arg sid "$SESSION_ID" --arg key "$PUBKEY" --arg kind "$kind" '
        select(.error==null and .result.isError!=true) |
        (.result.structuredContent // ([.result.content[]? | select(.type=="text") | .text | fromjson?] | select(length==1) | .[0])) |
        select(.success==true) |
        if $kind=="inventory" then
            [.. | objects | select(has("entries") and has("counts"))] | select(length==1) | .[0] |
            select(.entries|type=="array") | select(.counts|type=="object") | {verified:true}
        else
            [.. | objects | select(has("wireguard_pubkey") and has("session_id"))] |
            select(length==1) | .[0] | select(.session_id==$sid and .wireguard_pubkey==$key) |
            select(.genesis|type=="string" and length>0) |
            select((.arrival_timestamp|type=="number" and .>0 and floor==.) and
                (.chain_head_at_arrival|type=="string" and length>0)) |
            select(.instance|type=="object" and .name==$sid and .type=="container") |
            select((.interface|type=="string") and ((.peer_ip==null) or (.peer_ip|type=="string"))) |
            select(.btrfs_device|type=="object") | select(.btrfs_device.cozo_id|type=="string" and length>0) |
            select(.btrfs_device | (.device_path|type=="string") and (.mount_point|type=="string") and
                (.btrfs_uuid|type=="string") and (.attached|type=="boolean")) |
            select((.active|type=="boolean") and (.mutation_index|type=="number" and .>=0 and floor==.)) |
            {session_id,interface,peer_ip,active,mutation_index,
             btrfs_device:(.btrfs_device|{device_path,mount_point,btrfs_uuid,cozo_id,attached})}
        end' <<<"$line" 2>/dev/null
)
mcp_call identity_sled__inspect_identity_inventory '{"limit":1}' inventory > "$WORK/inventory.json" ||
    die 'corrected backend/inventory authorization unavailable; no native mutation performed'
get_args=$(jq -cn --arg sid "$SESSION_ID" '{session_id:$sid}')
mcp_call identity_sled__get_identity "$get_args" identity > "$WORK/before.json" ||
    die 'existing anchored identity/public-key verification failed; new signup unsupported'
if [[ $NO_START == false ]]; then
    jq -e '.active==false' "$WORK/before.json" >/dev/null || die 'normal mode requires a parked inactive target; use --no-start for active maintenance'
fi
instance_list=$(incus query '/1.0/instances?recursion=1') || die 'cannot list native instances'
jq -e 'type=="array" and all(.[]; type=="object" and (.name|type=="string"))' >/dev/null <<<"$instance_list" || die 'invalid native inventory'
if jq -e --arg sid "$SESSION_ID" 'any(.[]; .name==$sid)' >/dev/null <<<"$instance_list"; then die 'canonical instance exists; manual retirement required'; fi
pool=$(incus query /1.0/storage-pools/identity-roots) || die 'identity-roots pool unavailable'
jq -e '.name=="identity-roots" and .driver=="dir" and .status=="Created"' >/dev/null <<<"$pool" || die 'identity-roots must be an existing dir pool'
deny_policy=$(printf '%s\n' \
    'ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]' \
    'ioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]' \
    'ioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]')
incus init --empty "$SESSION_ID" --storage identity-roots --no-profiles \
    --config security.privileged=false --config security.nesting=false \
    --config boot.autostart=false --config security.syscalls.deny_default=true \
    --config "security.syscalls.deny=$deny_policy" \
    --config "user.opdbus.session_id=$SESSION_ID" --config "user.display_name=$ROLE" \
    --config "user.opdbus.wireguard_public_key=$PUBKEY"
"$ROOT_HELPER" seed-create "$SESSION_ID" "$SESSION_ID" "$LEAF_PATH" "$((FSTORAGE_GIB*1073741824))" >/dev/null 2>/dev/null || die 'seed creation failed; retain intent'
"$ROOT_HELPER" seed-check "$SESSION_ID" >/dev/null 2>/dev/null || die 'physical/native readback failed'
"$ROOT_HELPER" seed-lungs "$SESSION_ID" >/dev/null 2>/dev/null || die 'identity-bound lungs/hook preparation failed'
"$ROOT_HELPER" seed-check "$SESSION_ID" >/dev/null 2>/dev/null || die 'root/hook readback failed'
descriptor="$STATE_ROOT/$SESSION_ID/state.json"
root_path "$descriptor"
[[ -f $descriptor && ! -L $descriptor && $(stat -c %a -- "$descriptor") == 600 ]] || die 'unsafe storage receipt'
jq -e --arg sid "$SESSION_ID" --arg leaf "$LEAF_PATH" '
    .version==1 and .phase=="ready" and .session==$sid and .name==$sid and .leaf==$leaf and
    (.sprout_fsid|type=="string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))
' "$descriptor" >/dev/null || die 'storage receipt differs from selected identity/leaf'
write_args=$(jq -cn --arg key "$PUBKEY" --arg leaf "$LEAF_PATH" --arg mount "$STATE_ROOT/$SESSION_ID/volume" \
    --slurpfile old "$WORK/before.json" --slurpfile storage "$descriptor" \
    '{wireguard_pubkey:$key,btrfs_device:{device_path:$leaf,mount_point:$mount,
      btrfs_uuid:$storage[0].sprout_fsid,cozo_id:$old[0].btrfs_device.cozo_id,attached:false}}')
mcp_call identity_sled__get_identity "$get_args" identity > "$WORK/recheck.json" || die 'identity recheck failed'
cmp -s "$WORK/before.json" "$WORK/recheck.json" || die 'identity changed; binding left untouched'
mcp_call identity_sled__write_identity "$write_args" identity > "$WORK/write.json" || die 'binding update rejected; preserve journal and old receipt'
jq -e --argjson args "$write_args" --slurpfile old "$WORK/before.json" '
    .btrfs_device==$args.btrfs_device and .interface==$old[0].interface and .peer_ip==$old[0].peer_ip
' "$WORK/write.json" >/dev/null || die 'binding update readback differs'
mcp_call identity_sled__attach_btrfs_device "$get_args" identity > "$WORK/attach.json" || die 'attach verification failed; no guest start'
mcp_call identity_sled__get_identity "$get_args" identity > "$WORK/after.json" || die 'authoritative final readback failed'
jq -e --argjson args "$write_args" --slurpfile old "$WORK/before.json" '
    .btrfs_device==($args.btrfs_device|.attached=true) and .interface==$old[0].interface and .peer_ip==$old[0].peer_ip
' "$WORK/after.json" >/dev/null || die 'authoritative attached binding differs'
"$ROOT_HELPER" seed-check "$SESSION_ID" >/dev/null 2>/dev/null || die 'final physical readback failed'
if [[ $NO_START == false ]]; then
    # Canonical lifecycle admission verifies storage, starts the native guest
    # and commits its active session state; native start alone is insufficient.
    mcp_call identity_sled__touch_session "$get_args" identity > "$WORK/active.json" || die 'supported identity activation failed'
    mcp_call identity_sled__get_identity "$get_args" identity > "$WORK/running.json" || die 'active identity readback failed'
    jq -e --argjson args "$write_args" --slurpfile old "$WORK/before.json" '
        .active==true and .btrfs_device==($args.btrfs_device|.attached=true) and
        .interface==$old[0].interface and .peer_ip==$old[0].peer_ip
    ' "$WORK/running.json" >/dev/null || die 'active identity binding differs'
    expected_status=Running
else
    jq -e --slurpfile old "$WORK/before.json" '.active==$old[0].active' "$WORK/after.json" >/dev/null || die '--no-start changed active state'
    expected_status=Stopped
fi
native=$(incus query "/1.0/instances/$SESSION_ID")
jq -e --arg sid "$SESSION_ID" --arg status "$expected_status" '
    .name==$sid and .status==$status and .config["user.opdbus.session_id"]==$sid
' >/dev/null <<<"$native" || die 'guest power-state readback failed'
if [[ $NO_START == true ]]; then start_on_boot=false; else start_on_boot=true; fi
"$ROOT_HELPER" seed-enable-boot "$SESSION_ID" "$start_on_boot" >/dev/null 2>/dev/null || die 'boot completion publication failed; inspect retained receipts'
printf 'Existing identity %s physical root/binding verified; guest %s. No account minted.\n' "$SESSION_ID" "$expected_status"
