#!/usr/bin/env bash
# Provision a fresh shared root for the EXISTING configured chatbot key.
# Completed manual storage stage (no identity/backend preflight):
#   sudo ./provision-workspace-subscriber.sh --prepare-disk 3tchedChatbot
#   sudo ./provision-workspace-subscriber.sh --inspect-disk 3tchedChatbot
#   sudo ./provision-workspace-subscriber.sh --prepare-container 3tchedChatbot
#   sudo ./provision-workspace-subscriber.sh --pair-upgrade 3tchedChatbot
#   sudo ./provision-workspace-subscriber.sh --bind-root 3tchedChatbot
#   sudo ./provision-workspace-subscriber.sh --sync-upgrade 3tchedChatbot
#     [--root-gib 20 --upgrade-gib 40]   # prepare-disk only
# Creates /var/lib/3tched-leaf-pool/NAME.img: raw GPT root + upgrade.
# --prepare-container defines the stopped chatbot and socket profile only;
# it does not bind the prepared root, adopt the identity or start the guest.
# This stage ends before formatting, seed attachment, Incus creation or Cozo
# registration. Existing identity mode below remains the older unpartitioned
# leaf workflow; do not pass a partitioned image to that mode.
# Usage:
#   sudo ./provision-workspace-subscriber.sh LABEL --session-id UUID --pubkey B64 \
#     --operator-session UUID --leaf-path ABSOLUTE_PATH --principal-group '$ROOT' \
#     [--fstorage-gib N] [--no-start]
#     [--container-name 3tchedChatbot]   # configured chatbot key only
#   sudo ./provision-workspace-subscriber.sh NAME --system \
#     [--image ALIAS|FINGERPRINT] [--storage POOL] [--no-start]
#   sudo ./provision-workspace-subscriber.sh --system NAME [system options]
#
# Identity mode never signs up an account, generates/copies credentials,
# deletes/recreates a container, or attaches a leaf to an Incus filesystem.
# Clean replacement is currently scoped to the configured chatbot alias
# 3tchedChatbot. LABEL is display metadata, never authority.
# Existing native names and storage receipts are refused.
# --no-start prepares storage, reconciles and verifies attach, leaving STOPPED.
# The old native instance must be stopped. The supported owner operation
# creates a fresh parked current-schema record using the same key and SID;
# supported touch then starts it after attached readback. --no-start leaves
# the new identity parked. Credential bytes are not requested or inspected;
# supported method results redact them. This wrapper never mints credentials.
# Completion enables root/FUSE restoration on boot; --no-start records a
# stopped boot policy. Incomplete storage never receives a boot-ready marker.
# The physical seed is one immutable filesystem shared by all leaves. Storage
# preparation uses its existing seven subvolumes through a private writable
# sprout; it never snapshots, copies, sends or receives a container root.
# Each guest gets its session ID as /etc/hostname and the LXC UTS hostname.
# seed-lungs configures the persistent Incus disk attaching the identity's own
# host FUSE mount at /run/mount/3tchedFS on every start.
# The 3tched-lungs profile provides /run tmpfs and the dedicated host socket
# directory at the same /run/opdbus/lungs path inside each new container.
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
prepare_chatbot_container() (
    # Host preparation only: no identity adoption, root copy, device-add or start.
    [[ $# == 1 && $1 == 3tchedChatbot ]] || die 'usage: --prepare-container 3tchedChatbot'
    [[ $(id -u) == 0 ]] || die 'container preparation requires host root'
    for tool in incus jq findmnt stat flock realpath sync; do need_cmd "$tool"; done
    incus() { command incus --force-local "$@"; }
    umask 077
    pool_root=/var/lib/3tched-leaf-pool
    receipt=$pool_root/.provisioning/$1.json
    container_receipt=$pool_root/.provisioning/$1.incus.json
    [[ $(findmnt -rn -M "$pool_root" -o FSROOT) == /@3tched-leaf-pool &&
       -f $receipt && ! -L $receipt && ! -e $container_receipt && ! -L $container_receipt ]] ||
        die 'prepared disk receipt required; existing container intents must be reconciled manually'
    jq -e --arg name "$1" '.name==$name and .phase=="filesystems-prepared" and
        .layout=="raw-gpt-root-upgrade-v1" and .filesystem_prepared==true and .identity_registered==false' \
        "$receipt" >/dev/null || die 'disk is not ready for container definition'
    image=$(jq -er .image "$receipt")
    [[ $image == "$pool_root/$1.img" && -f $image && ! -L $image &&
       $(stat -c %i "$image") == "$(jq -r .image_inode "$receipt")" &&
       $(stat -c %s "$image") == "$(jq -r .image_bytes "$receipt")" ]] || die 'prepared image changed'
    exec 9>"$pool_root/.provisioning/$1.lock"
    flock -n 9 || die 'instance preparation active'
    instances=$(incus list -f json)
    jq -e --arg name "$1" 'type=="array" and all(.[]; .name!=$name)' <<<"$instances" >/dev/null ||
        die 'container already exists; never recreate it'
    [[ $(findmnt -rn -M /var/lib/incus-storage -o FSROOT) == /@incus ]] || die 'Incus subvolume not mounted'
    pool_source=/var/lib/incus-storage/identity-roots
    pools=$(incus storage list -f json)
    if jq -e 'any(.[]; .name=="identity-roots")' <<<"$pools" >/dev/null; then
        pool_json=$(incus query /1.0/storage-pools/identity-roots)
        jq -e --arg source "$pool_source" '.driver=="dir" and .status=="Created" and .config.source==$source' \
            <<<"$pool_json" >/dev/null || die 'identity-roots pool differs'
    else
        [[ ! -e /var/lib/incus/storage-pools/identity-roots &&
           ! -L /var/lib/incus/storage-pools/identity-roots &&
           ! -e $pool_source && ! -L $pool_source ]] || die 'retained pool paths require manual reconciliation'
        mkdir -m 0711 "$pool_source"
        incus storage create identity-roots dir "source=$pool_source"
    fi
    /usr/local/libexec/3tched/op-identity-root seed-profile
    sid=b0c777e3-0955-cd8b-3d01-ca12998cae28
    public_key=Env5zPMmM1Zhypu5/DY/yU4PRYW/4AOEq2KZ4OSc+18=
    jq -n --arg name "$1" --arg sid "$sid" --arg source "$pool_source" \
        '{version:1,phase:"container-create-intent",name:$name,session_id:$sid,pool_source:$source,
          identity_registered:false,root_bound:false,started:false}' > "$container_receipt"
    sync -f "$container_receipt"
    deny_policy=$(printf '%s\n' \
        'ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]' \
        'ioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]' \
        'ioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]')
    incus init --empty "$1" --storage identity-roots --profile 3tched-lungs \
        --config security.privileged=false --config security.nesting=false \
        --config security.protection.shift=true --config boot.autostart=false \
        --config security.syscalls.deny_default=true --config "security.syscalls.deny=$deny_policy" \
        --config "user.opdbus.session_id=$sid" --config "user.opdbus.wireguard_public_key=$public_key" \
        --config user.display_name=control-plane-chatbot --config "raw.lxc=lxc.uts.name = $sid" </dev/null
    instance=$(incus query "/1.0/instances/$1")
    jq -e --arg name "$1" --arg sid "$sid" '.name==$name and .status=="Stopped" and
        .config["user.opdbus.session_id"]==$sid' <<<"$instance" >/dev/null || die 'stopped definition readback differs'
    jq --argjson instance "$instance" '.phase="container-defined"|.instance=$instance' \
        "$container_receipt" > "$container_receipt.new"
    sync -f "$container_receipt.new"
    mv -T "$container_receipt.new" "$container_receipt"
    sync -f "$pool_root/.provisioning"
    printf 'Stopped container defined; root binding and identity adoption remain pending.\n'
)
case $1 in
    --bind-root)
        shift
        script_dir=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
        bind_helper=$script_dir/bind-instance-root.sh
        [[ -x $bind_helper ]] || bind_helper=$script_dir/bind-instance-root
        [[ -x $bind_helper ]] || die 'root binding helper missing'
        exec "$bind_helper" "$@"
        ;;
    --prepare-container)
        shift
        prepare_chatbot_container "$@"
        exit
        ;;
    --pair-upgrade|--sync-upgrade|--upgrade-boot)
        upgrade_action=pair
        [[ $1 != --sync-upgrade ]] || upgrade_action=sync
        [[ $1 != --upgrade-boot ]] || upgrade_action=boot
        shift
        script_dir=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
        upgrade_helper=$script_dir/sync-instance-upgrade.sh
        [[ -x $upgrade_helper ]] || upgrade_helper=$script_dir/sync-instance-upgrade
        [[ -x $upgrade_helper ]] || die 'upgrade synchronization helper missing'
        exec "$upgrade_helper" "$upgrade_action" "$@"
        ;;
    --prepare-disk|--inspect-disk|--prepare-filesystems)
        disk_action=create
        [[ $1 != --inspect-disk ]] || disk_action=inspect
        [[ $1 != --prepare-filesystems ]] || disk_action=prepare
        shift
        script_dir=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
        disk_helper=$script_dir/prepare-instance-disk.sh
        [[ -x $disk_helper ]] || disk_helper=$script_dir/prepare-instance-disk
        [[ -x $disk_helper ]] || die 'disk preparation helper is missing'
        exec "$disk_helper" "$disk_action" "$@"
        ;;
esac
SYSTEM_MODE=false SYSTEM_NAME="" ROLE=""
if [[ $1 == --system ]]; then
    SYSTEM_MODE=true; shift; SYSTEM_NAME=${1:?container name required}; shift
else ROLE=$1; shift; fi
SYSTEM_IMAGE=${SYSTEM_CONTAINER_IMAGE:-raccoon}
SYSTEM_STORAGE_POOL=${SYSTEM_STORAGE_POOL:-3tched-storage}
IMAGE=${IDENTITY_SEED_IMAGE:-3a639373bb7a} IMAGE_OVERRIDDEN=false
SYSTEM_STORAGE=$SYSTEM_STORAGE_POOL SYSTEM_STORAGE_OVERRIDDEN=false
IDENTITY_ONLY_FLAGS="" PUBKEY="" SESSION_ID="" OPERATOR_SESSION="" LEAF_PATH="" NATIVE_NAME=""
PRINCIPAL_GROUP="" FSTORAGE_GIB=2 NO_START=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --pubkey|--session-id|--operator-session|--leaf-path|--fstorage-gib|--container-name|--principal-group)
            option=$1; IDENTITY_ONLY_FLAGS="$IDENTITY_ONLY_FLAGS $option"
            shift; value=${1:?option requires a value}
            case "$option" in
                --pubkey) PUBKEY=$value ;; --session-id) SESSION_ID=$value ;;
                --operator-session) OPERATOR_SESSION=$value ;; --leaf-path) LEAF_PATH=$value ;;
                --fstorage-gib) FSTORAGE_GIB=$value ;; --container-name) NATIVE_NAME=$value ;;
                --principal-group) PRINCIPAL_GROUP=$value ;;
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
        --config boot.autostart=false </dev/null
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
case "$PRINCIPAL_GROUP" in
    '$ROOT'|'$ADMIN'|'$SUPERADMIN'|'$EXTERNAL-MCP'|'$VENDOR'|'$GUEST'|'$AGENT-L1'|'$AGENT-L2'|'$AGENT-L3'|'$AGENT-L4') ;;
    '') die '--principal-group is required; quote it, for example --principal-group '\''$ROOT'\''' ;;
    *) die "unknown principal group: $PRINCIPAL_GROUP" ;;
esac
[[ $(id -u) == 0 ]] || die 'identity physical-root preparation requires host root'
[[ $SYSTEM_STORAGE_OVERRIDDEN == false && $IMAGE_OVERRIDDEN == false ]] ||
    die '--image and --storage are system-only; identity roots use the pinned physical seed'
valid_uuid "$SESSION_ID" || die 'explicit canonical --session-id is required'
valid_uuid "$OPERATOR_SESSION" || die 'explicit authorized --operator-session is required'
NATIVE_NAME=${NATIVE_NAME:-3tchedChatbot}
[[ $NATIVE_NAME == 3tchedChatbot && $SESSION_ID == b0c777e3-0955-cd8b-3d01-ca12998cae28 ]] ||
    die 'clean replacement is scoped to the configured 3tchedChatbot identity'
[[ $PUBKEY =~ ^[A-Za-z0-9+/]{43}=$ ]] || die 'explicit canonical 32-byte Base64 --pubkey is required'
[[ ${#ROLE} -ge 1 && ${#ROLE} -le 63 && $ROLE =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || die 'invalid display label'
[[ $FSTORAGE_GIB =~ ^[1-9][0-9]{0,3}$ ]] && (( FSTORAGE_GIB <= 1024 )) || die 'leaf size must be 1 to 1024 GiB'
[[ $LEAF_PATH == /* && $LEAF_PATH != *$'\n'* && $LEAF_PATH != *$'\r'* ]] || die 'explicit absolute --leaf-path is required'
for command in incus jq realpath stat timeout mktemp find findmnt cmp base64 flock curl sha256sum sync; do need_cmd "$command"; done
# Identity plans and the bridge bind the local host, never the CLI's selected
# remote. Standalone --system provisioning keeps its existing remote behavior.
incus() { command incus --force-local "$@"; }
[[ $(printf '%s' "$PUBKEY" | base64 -d | base64 -w0) == "$PUBKEY" ]] || die 'public key encoding is not canonical'
[[ $(realpath -m -- "$LEAF_PATH") == "$LEAF_PATH" ]] || die 'leaf path must be normalized'
[[ ! -e $LEAF_PATH && ! -L $LEAF_PATH ]] || die 'leaf already exists; inspect retained intent, never retry creation'
ROOT_HELPER=$(realpath -e -- "${OP_IDENTITY_ROOT_HELPER:-/usr/local/libexec/3tched/op-identity-root}")
HOST_FOLDERS_HELPER=$(realpath -e -- "${OP_IDENTITY_HOST_FOLDERS_HELPER:-/usr/local/libexec/3tched/attach-host-workspace-folders}")
FS_BINARY=$(realpath -e -- "${THREETCHED_FS:-/usr/local/bin/threetched-fs}")
CATALOG=$(realpath -e -- "${OP_IDENTITY_SEED_CATALOG:-/var/lib/opdbus/plugin-blobs}")
STATE_ROOT=${OP_IDENTITY_SEED_STATE:-/var/lib/opdbus-runtime/seed-identities}
RECEIPT_ROOT=${OP_IDENTITY_PROVISION_RECEIPTS:-/var/lib/opdbus-runtime/identity-provision-receipts}
PLAN_ROOT=${OP_IDENTITY_PROVISION_PLAN_ROOT:-/var/lib/op-dbus/identity-provision-plans}
INCUS_SOCKET=/var/lib/incus/unix.socket
root_path "$ROOT_HELPER"; root_path "$FS_BINARY"; root_path "$CATALOG"
root_path "$HOST_FOLDERS_HELPER"
[[ $PLAN_ROOT == /* && $(realpath -m -- "$PLAN_ROOT") == "$PLAN_ROOT" ]] || die 'invalid provision plan directory'
[[ -f $HOST_FOLDERS_HELPER && -x $HOST_FOLDERS_HELPER ]] || die 'invalid selected host workspace folders helper'
[[ -f $ROOT_HELPER && -x $ROOT_HELPER && -f $FS_BINARY && -x $FS_BINARY && -d $CATALOG ]] || die 'invalid selected artifacts'
# Helpers must use the same selected artifacts, including received layouts.
export THREETCHED_FS="$FS_BINARY" OP_IDENTITY_SEED_CATALOG="$CATALOG"
export OP_IDENTITY_SEED_STATE="$STATE_ROOT"
[[ $(stat -c %a -- "$(dirname -- "$LEAF_PATH")") == 700 ]] || die 'leaf parent must already be private mode 0700'
root_path "$(dirname -- "$LEAF_PATH")"
[[ $STATE_ROOT == /* && $(realpath -m -- "$STATE_ROOT") == "$STATE_ROOT" ]] || die 'invalid seed state root'
root_path "$(dirname -- "$STATE_ROOT")"
[[ ! -e $STATE_ROOT/$NATIVE_NAME && ! -L $STATE_ROOT/$NATIVE_NAME ]] || die 'seed receipt exists; use seed-inspect on the retained intent'
[[ $RECEIPT_ROOT == /* && $(realpath -m -- "$RECEIPT_ROOT") == "$RECEIPT_ROOT" ]] || die 'invalid operator receipt root'
root_path "$(dirname -- "$RECEIPT_ROOT")"
umask 077
mkdir -p -- "$RECEIPT_ROOT"; root_path "$RECEIPT_ROOT"
[[ $(stat -c %a -- "$RECEIPT_ROOT") == 700 ]] || die 'operator receipt root must be mode 0700'
lock_path=$RECEIPT_ROOT/$SESSION_ID.lock
[[ ! -L $lock_path && ( ! -e $lock_path || -f $lock_path ) ]] || die 'unsafe provisioning lock'
exec 9>"$lock_path"
root_path "$lock_path"
[[ $(stat -c %a -- "$lock_path") == 600 ]] || die 'provisioning lock must be mode 0600'
flock -n 9 || die 'this identity is already being provisioned'
WORK=$(mktemp -d "$RECEIPT_ROOT/$SESSION_ID.XXXXXX")
mkdir -- "$WORK/empty-state" "$WORK/store"
printf 'Private sanitized receipts: %s\n' "$WORK"
trap 'printf "Failure; retained private receipts at %s; no automatic cleanup.\n" "$WORK" >&2' ERR
LUNGS_MOUNT=/run/mount/3tchedFS
[[ $(findmnt -rn -M "$LUNGS_MOUNT" -o FSTYPE) == fuse.3tchedFS ]] || die 'operator lungs namespace is not mounted'
jq -e --arg session "$OPERATOR_SESSION" --slurpfile catalog "$CATALOG/.manifest.json" '
    .session_id==$session and .source_catalog_hash==$catalog[0].catalog_hash
' "$LUNGS_MOUNT/.3tchedfs.json" >/dev/null || die 'operator lungs identity or sealed catalog differs'

# No credential input. Every response passes a metadata allowlist before storage.
lungs_call() (
    set -euo pipefail
    tool=$1 args=$2 kind=$3
    [[ $tool == identity_sled__* ]] || exit 1
    method=${tool#identity_sled__}
    [[ $method =~ ^[a-z_]+$ ]] || exit 1
    operation=$LUNGS_MOUNT/plugins/identity_sled/operations/$method
    [[ -f $operation/input.schema.json && -f $operation/call ]] || exit 1
    # FUSE validates the sealed method and dispatches synchronously on close.
    # Never clear read-only receipts or treat a successful write as completion.
    printf '%s' "$args" > "$operation/call"
    [[ ! -s $operation/error ]] || { printf 'Lungs operation %s failed; inspect its protected error receipt.\n' "$method" >&2; exit 1; }
    line=$(jq -ce --arg method "$method" 'select(.plugin_id=="identity_sled" and .method==$method) | {result:{structuredContent:.result}}' "$operation/result")
    jq -ce --arg sid "$SESSION_ID" --arg name "$NATIVE_NAME" --arg key "$PUBKEY" --arg kind "$kind" '
        select(.error==null and .result.isError!=true) |
        (.result.structuredContent // ([.result.content[]? | select(.type=="text") | .text | fromjson?] | select(length==1) | .[0])) as $body |
        if $kind=="device-add" then
            ($body.result // $body) |
            select(.operation=="device_add" and (.device|type=="string" and length>0) and
                (.filesystem_uuid|type=="string" and length>0) and (.devices|type=="array")) |
            {operation,filesystem_uuid,device,devices:[.devices[]|{id,uuid,path,total_bytes,bytes_used}]}
        else $body | select(.success==true) |
        if $kind=="inventory" then
            [.. | objects | select(has("entries") and has("counts"))] | select(length==1) | .[0] |
            select(.entries|type=="array") | select(.counts|type=="object") | {verified:true}
        elif $kind=="preflight" then
            [.. | objects | select(has("wireguard_pubkey") and has("session_id") and has("expected_revision_operation_id"))] |
            select(length==1) | .[0] |             select(.session_id==$sid and .wireguard_pubkey==$key) |
            select(.expected_revision|type=="number" and .>0 and floor==.) |
            select(.expected_revision_operation_id|type=="string" and length>0) |
            {session_id,wireguard_pubkey,expected_revision,expected_revision_operation_id,previous_instance,previous_instance_etag,previous_native_instance}
        else
            (if $kind=="provision" then
                [.. | objects | select(has("committed_revision") and has("plan_sha256") and has("identity"))] |
                select(length==1) | .[0] |
                select(.plan_sha256|type=="string" and test("^[0-9a-f]{64}$")) |
                select(.operation_id|type=="string" and length>0) |
                select(.committed_revision|type=="number" and .>0 and floor==.) |
                select(.replayed|type=="boolean")
             else {identity:.} end) as $receipt |
            $receipt.identity |
            [.. | objects | select(has("wireguard_pubkey") and has("session_id"))] |
            select(length==1) | .[0] | select(.session_id==$sid and .wireguard_pubkey==$key) |
            select(.genesis|type=="string" and length>0) |
            select((.arrival_timestamp|type=="number" and .>0 and floor==.) and
                (.chain_head_at_arrival|type=="string" and length>0)) |
            select(.instance|type=="object" and .name==$name and .type=="container") |
            select($name==$sid or .principal_kind=="service") |
            select((.interface|type=="string") and ((.peer_ip==null) or (.peer_ip|type=="string"))) |
            select(.btrfs_device|type=="object") | select(.btrfs_device.cozo_id|type=="string" and length>0) |
            select(.btrfs_device | (.device_path|type=="string") and (.mount_point|type=="string") and
                (.btrfs_uuid|type=="string") and (.attached|type=="boolean")) |
            select((.active|type=="boolean") and (.mutation_index|type=="number" and .>=0 and floor==.)) |
            {session_id,principal_kind,instance:(.instance|{name,type}),interface,peer_ip,active,mutation_index,
             btrfs_device:(.btrfs_device|{device_path,mount_point,btrfs_uuid,cozo_id,attached})} as $identity |
            if $kind=="provision" then $receipt | .identity=$identity else $identity end
        end
        end' <<<"$line" >"$WORK/operation-filter.json" 2>/dev/null || {
        if [[ $kind == device-add ]]; then
            jq -r '.error.message // ([.result.content[]? | select(.type=="text") | .text] | first) // "device_add failed"' <<<"$line" >&2
        fi
        exit 1
    }
    cat "$WORK/operation-filter.json"
)
lungs_call identity_sled__inspect_identity_inventory '{"limit":1}' inventory > "$WORK/inventory.json" ||
    die 'corrected backend/inventory authorization unavailable; no native mutation performed'
get_args=$(jq -cn --arg sid "$SESSION_ID" '{session_id:$sid}')
preflight_args=$(jq -cn --arg key "$PUBKEY" '{wireguard_pubkey:$key}')
lungs_call identity_sled__inspect_provision_preflight "$preflight_args" preflight > "$WORK/before.json" ||
    die 'sealed owner preflight unavailable; no native mutation performed'
if jq -e '.previous_instance==null' "$WORK/before.json" >/dev/null; then
    plan_version=3
else
    plan_version=2
    jq -e --arg name "$NATIVE_NAME" '.previous_instance.name!=$name' "$WORK/before.json" >/dev/null ||
        die 'previous instance uses the replacement name; explicit retirement is required'
fi
instance_list=$(incus query '/1.0/instances?recursion=1') || die 'cannot list native instances'
jq -e 'type=="array" and all(.[]; type=="object" and (.name|type=="string"))' >/dev/null <<<"$instance_list" || die 'invalid native inventory'
if jq -e --arg name "$NATIVE_NAME" 'any(.[]; .name==$name)' >/dev/null <<<"$instance_list"; then die 'selected instance exists; manual retirement required'; fi
"$ROOT_HELPER" seed-preflight || die 'seed preflight failed; no container or leaf was created'
"$ROOT_HELPER" seed-profile || die 'shared socket profile preparation failed; no container or leaf was created'
pool=$(incus query /1.0/storage-pools/identity-roots) || die 'identity-roots pool unavailable'
jq -e '.name=="identity-roots" and .driver=="dir" and .status=="Created"' >/dev/null <<<"$pool" || die 'identity-roots must be an existing dir pool'
deny_policy=$(printf '%s\n' \
    'ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]' \
    'ioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]' \
    'ioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]')
incus init --empty "$NATIVE_NAME" --storage identity-roots --profile 3tched-lungs \
    --config security.privileged=false --config security.nesting=false \
    --config security.protection.shift=true \
    --config boot.autostart=false --config security.syscalls.deny_default=true \
    --config "security.syscalls.deny=$deny_policy" \
    --config "user.opdbus.session_id=$SESSION_ID" --config "user.display_name=$ROLE" \
    --config "user.opdbus.wireguard_public_key=$PUBKEY" </dev/null
"$ROOT_HELPER" seed-create "$NATIVE_NAME" "$SESSION_ID" "$LEAF_PATH" "$((FSTORAGE_GIB*1073741824))" >/dev/null 2>/dev/null || die "leaf creation failed; run op-identity-root seed-inspect $NATIVE_NAME; do not repeat device-add"
"$ROOT_HELPER" seed-check "$NATIVE_NAME" >/dev/null 2>/dev/null || die 'physical/native readback failed'
"$HOST_FOLDERS_HELPER" "$NATIVE_NAME" || die 'host workspace folders attachment failed; retain stopped candidate'
"$ROOT_HELPER" seed-lungs "$NATIVE_NAME" >/dev/null 2>/dev/null || die 'identity-bound host FUSE and Incus disk preparation failed'
"$ROOT_HELPER" seed-check "$NATIVE_NAME" >/dev/null 2>/dev/null || die 'root and FUSE disk readback failed'
descriptor="$STATE_ROOT/$NATIVE_NAME/state.json"
root_path "$descriptor"
[[ -f $descriptor && ! -L $descriptor && $(stat -c %a -- "$descriptor") == 600 ]] || die 'unsafe storage receipt'
jq -e --arg sid "$SESSION_ID" --arg name "$NATIVE_NAME" --arg leaf "$LEAF_PATH" '
    .version==1 and .layout=="shared-partition-siblings-v2" and
    .phase=="ready" and .session==$sid and .name==$name and .leaf==$leaf and
    (.sprout_fsid|type=="string" and test("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"))
' "$descriptor" >/dev/null || die 'storage receipt differs from selected identity/leaf'
# Pin the official native definition and conditional ETag after all disk/profile
# edits. The owner independently re-reads both; the CLI query omits ETag headers.
[[ -S $INCUS_SOCKET ]] || die 'local Incus socket unavailable'
root_path "$(dirname -- "$INCUS_SOCKET")"
curl --silent --show-error --fail --noproxy '*' --max-time 30 \
    --unix-socket "$INCUS_SOCKET" --dump-header "$WORK/instance.headers" \
    --output "$WORK/instance-response.json" "http://unix.socket/1.0/instances/$NATIVE_NAME" ||
    die 'native instance and ETag readback failed'
jq -ce --arg name "$NATIVE_NAME" '
    select(.type=="sync" and .status_code==200) | .metadata |
    select(.name==$name and .type=="container" and .status=="Stopped")
' "$WORK/instance-response.json" > "$WORK/instance.json" || die 'native stopped definition unavailable'
instance_etag=$(awk 'tolower($1)=="etag:" {sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print}' "$WORK/instance.headers")
[[ -n $instance_etag && $instance_etag != *$'\n'* ]] || die 'missing or ambiguous native ETag'
lungs_call identity_sled__inspect_provision_preflight "$preflight_args" preflight > "$WORK/preflight-recheck.json" ||
    die 'owner preflight recheck failed; new leaf retained without adoption'
cmp -s "$WORK/before.json" "$WORK/preflight-recheck.json" ||
    die 'prior identity/native binding changed; new leaf retained without adoption'
seed_device=$(readlink -f -- "${OP_IDENTITY_SEED_DEVICE:-/dev/disk/by-partuuid/${OP_IDENTITY_SEED_PARTUUID:-911b270a-a88f-48cd-bf9c-9dcac4cdefe8}}")
jq -n --argjson version "$plan_version" --arg sid "$SESSION_ID" --arg key "$PUBKEY" --arg leaf "$LEAF_PATH" \
    --arg mount "$STATE_ROOT/$NATIVE_NAME/volume" --arg seed_device "$seed_device" --arg etag "$instance_etag" \
    --arg group "$PRINCIPAL_GROUP" \
    --argjson pool "$pool" --slurpfile old "$WORK/before.json" --slurpfile storage "$descriptor" \
    --slurpfile instance "$WORK/instance.json" '
    {version:$version,session_id:$sid,wireguard_pubkey:$key,principal_group:$group,
     expected_revision:$old[0].expected_revision,
     expected_revision_operation_id:$old[0].expected_revision_operation_id,
     binding:{device_path:$leaf,mount_point:$mount,btrfs_uuid:$storage[0].sprout_fsid,cozo_id:$sid,attached:true},
     seed_device:$seed_device,seed_fsid:$storage[0].seed_fsid,seed_generation:($storage[0].seed_generation|tonumber),
     leaf_inode:($storage[0].leaf_inode|tonumber),leaf_size:($storage[0].leaf_size|tonumber),
     members:($storage[0].members|with_entries(.value.id|=tonumber)),persist_inode:($storage[0].persist.inode|tonumber),
     native_root:$storage[0].root,pool_name:$pool.name,pool_source:$pool.config.source,
     instance_etag:$etag,instance:$instance[0],
     previous_instance_etag:$old[0].previous_instance_etag,previous_instance:$old[0].previous_native_instance}
' > "$WORK/plan.json" || die 'cannot assemble exact clean-provision plan'
[[ $(stat -c %s "$WORK/plan.json") -le 32768 ]] || die 'provision plan exceeds the owner bound'
root_path "$(dirname -- "$PLAN_ROOT")"
if [[ ! -e $PLAN_ROOT && ! -L $PLAN_ROOT ]]; then mkdir -m 0700 -- "$PLAN_ROOT"; fi
root_path "$PLAN_ROOT"
[[ -d $PLAN_ROOT && ! -L $PLAN_ROOT && $(stat -c %a "$PLAN_ROOT") == 700 ]] || die 'unsafe provision plan directory'
plan_path=$PLAN_ROOT/$SESSION_ID.json
if [[ -e $plan_path || -L $plan_path ]]; then
    root_path "$plan_path"
    [[ -f $plan_path && ! -L $plan_path && $(stat -c %a "$plan_path") == 600 ]] || die 'unsafe previous provision plan'
    cp -- "$plan_path" "$WORK/previous-plan.json"
fi
plan_tmp=$(mktemp "$PLAN_ROOT/.$SESSION_ID.XXXXXX")
cat "$WORK/plan.json" > "$plan_tmp"
sync -f "$plan_tmp"
mv -T -- "$plan_tmp" "$plan_path"
sync -f "$PLAN_ROOT"
plan_sha256=$(sha256sum "$plan_path"); plan_sha256=${plan_sha256%% *}
provision_args=$(jq -cn --arg key "$PUBKEY" --arg digest "$plan_sha256" '{wireguard_pubkey:$key,plan_sha256:$digest}')
lungs_call identity_sled__provision_container "$provision_args" provision > "$WORK/provision.json" ||
    die 'clean owner adoption rejected; preserve the plan, leaf, native definition and receipts for reconciliation'
jq -e --arg digest "$plan_sha256" --slurpfile plan "$WORK/plan.json" '
    .plan_sha256==$digest and .identity.active==false and .identity.btrfs_device==$plan[0].binding
' "$WORK/provision.json" >/dev/null || die 'clean owner receipt differs from pinned plan'
lungs_call identity_sled__get_identity "$get_args" identity > "$WORK/after.json" || die 'authoritative final readback failed'
jq -e --slurpfile plan "$WORK/plan.json" '
    .active==false and .btrfs_device==$plan[0].binding
' "$WORK/after.json" >/dev/null || die 'authoritative attached binding differs'
"$ROOT_HELPER" seed-check "$NATIVE_NAME" >/dev/null 2>/dev/null || die 'final physical readback failed'
if [[ $NO_START == false ]]; then
    # Canonical lifecycle admission verifies storage, starts the native guest
    # and commits its active session state; native start alone is insufficient.
    lungs_call identity_sled__touch_session "$get_args" identity > "$WORK/active.json" || die 'supported identity activation failed'
    lungs_call identity_sled__get_identity "$get_args" identity > "$WORK/running.json" || die 'active identity readback failed'
    jq -e --slurpfile plan "$WORK/plan.json" '
        .active==true and .btrfs_device==$plan[0].binding
    ' "$WORK/running.json" >/dev/null || die 'active identity binding differs'
    expected_status=Running
else
    jq -e '.active==false' "$WORK/after.json" >/dev/null || die '--no-start did not leave the fresh identity parked'
    expected_status=Stopped
fi
native=$(incus query "/1.0/instances/$NATIVE_NAME")
jq -e --arg sid "$SESSION_ID" --arg name "$NATIVE_NAME" --arg status "$expected_status" '
    .name==$name and .status==$status and .config["user.opdbus.session_id"]==$sid
' >/dev/null <<<"$native" || die 'guest power-state readback failed'
if [[ $NO_START == true ]]; then start_on_boot=false; else start_on_boot=true; fi
"$ROOT_HELPER" seed-enable-boot "$NATIVE_NAME" "$start_on_boot" >/dev/null 2>/dev/null || die 'boot completion publication failed; inspect retained receipts'
# This script still admits only the configured chatbot session. The flag
# chooses that identity's group. Containers are not new grant keys.
CHATBOT_PRINCIPAL=1262e982-14c1-44d4-0771-7ce74213211b
GRANTS_FILE=${OP_CAPABILITY_GRANTS:-/etc/opdbus/capability-grants.json}
[[ -f $GRANTS_FILE && ! -L $GRANTS_FILE ]] || die 'capability grant document is absent'
python3 - "$GRANTS_FILE" "$CHATBOT_PRINCIPAL" "$PRINCIPAL_GROUP" <<'PY'
import json, os, sys, tempfile
path, principal, group = sys.argv[1:]
with open(path) as handle:
    document = json.load(handle)
entry = document.get(principal)
if not isinstance(entry, dict):
    entry = {}
entry["groups"] = [group]
entry.setdefault("capabilities", [])
document[principal] = entry
fd, temporary = tempfile.mkstemp(prefix=".capability-grants.", dir=os.path.dirname(path) or ".")
try:
    with os.fdopen(fd, "w") as handle:
        json.dump(document, handle, indent=2)
        handle.write("\n")
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
finally:
    if os.path.exists(temporary):
        os.remove(temporary)
PY
printf 'Fresh physical root and current-schema binding verified for existing key identity %s; container %s is %s; principal group %s.\n' \
    "$SESSION_ID" "$NATIVE_NAME" "$expected_status" "$PRINCIPAL_GROUP"
