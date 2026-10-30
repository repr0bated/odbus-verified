#!/usr/bin/env bash
# Canonical creation reserves 3tchedCtlChatbot for the configured chatbot;
# ordinary containers receive ID-N. This wrapper never supplies a native name.
#   sudo ./provision-workspace-subscriber.sh --prepare-container --operation-id UUID
# Use the returned NAME for the separate external stages, in order:
#   --prepare-disk NAME [--root-gib 20 --upgrade-gib 40]
#   --prepare-filesystems NAME --seed-device /dev/disk/by-uuid/UUID
#   --pair-upgrade NAME
#   --bind-root NAME
#   --prepare-adoption NAME
#   --sync-upgrade NAME
# Existing 3tchedChatbot receipts remain supported; never rename/recreate them.
#   sudo ./provision-workspace-subscriber.sh --system LABEL --operation-id UUID \
#     [--image ALIAS|FINGERPRINT] [--storage POOL] [--no-start]
# LABEL is display metadata; use the server-assigned name from the receipt.
# Reuse the SAME operation UUID after interruption; no automatic fresh ID.
# Preparation requires an existing trusted identity-roots pool/3tched-lungs
# profile, the configured chatbot key and mounted caller identity_sled.write.
# No credential/grant mutation, native-create fallback, storage member change
# or identity adoption occurs during --prepare-container. Root restoration
# validates the existing UUID/session/key/storage receipts for either name.
set -euo pipefail
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
need_cmd() { command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }
usage() { sed -n '2,20p' "$0"; }
if [[ $# == 0 || ${1:-} == -h || ${1:-} == --help ]]; then usage; exit 0; fi
valid_uuid() { [[ $1 =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; }
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
prepare_chatbot_container() (
    # These aliases identify the configured target, never select its native name.
    [[ ${1:-} != 3tchedChatbot && ${1:-} != 3tchedCtlChatbot ]] || shift
    [[ $# == 2 && $1 == --operation-id ]] || die 'usage: --prepare-container [3tchedCtlChatbot|3tchedChatbot] --operation-id UUID'
    operation_id=$2
    valid_uuid "$operation_id" || die 'operation ID must be a canonical UUID'
    [[ $(id -u) == 0 ]] || die 'container preparation requires host root'
    for tool in incus jq findmnt stat flock realpath sync timeout mktemp; do need_cmd "$tool"; done
    umask 077
    pool_root=/var/lib/incus/storage-pools/3tched-pool
    [[ ! -L $pool_root && $(realpath -e -- "$pool_root") == "$pool_root" &&
       $(stat -c %u:%g "$pool_root") == 0:0 &&
       $(findmnt -rn -M "$pool_root" -o FSTYPE) == btrfs &&
       $(findmnt -rn -M "$pool_root" -o FSROOT) == / &&
       $(findmnt -rn -M "$pool_root" -o UUID) == a1ab7b42-12a8-437c-8f65-d0469892c8dd &&
       $(findmnt -rn -M / -o UUID) != a1ab7b42-12a8-437c-8f65-d0469892c8dd ]] ||
        die 'canonical loop pool is not mounted on its pinned filesystem'
    root_path "$pool_root"
    directory=$pool_root/.provisioning
    [[ ! -L $directory ]] || die 'unsafe receipt directory'
    [[ -e $directory ]] || mkdir -m 0700 "$directory"
    root_path "$directory"
    [[ -d $directory && $(stat -c %u:%g:%a "$directory") == 0:0:700 ]] || die 'receipt directory must be private'
    sid=b0c777e3-0955-cd8b-3d01-ca12998cae28
    key=[REDACTED:opaque-base64]
    client=$(realpath -e -- "${THREETCHED_FS:-/usr/local/libexec/op-dbus/bin/3tched-fs}")
    root_path "$client"
    [[ -f $client && -x $client ]] || die 'mounted client unavailable'
    control_mount=${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}
    operator=${OP_3TCHEDFS_EXPECTED_SESSION-414fbc56-03fb-20ab-758a-bcc7a7cbdc0e}
    valid_uuid "$operator" || die 'canonical expected caller session required'
    namespace_pin() {
        [[ $(findmnt -rn -M "$control_mount" -o FSTYPE) == fuse.3tchedFS ]] || return 1
        findmnt -rn -M "$control_mount" -o ID || return "$?"
        timeout --kill-after=5s 30s jq -ce --arg sid "$operator" '
            select(.format=="3tchedfs/1" and .session_id==$sid and
                .mount_identity=={kind:"host_session",session:$sid})
        ' "$control_mount/.3tchedfs.json"
    }
    pin=$(namespace_pin) || die 'caller namespace unavailable or mismatched'
    args=$(jq -cn --arg key "$key" --arg op "$operation_id" '{wireguard_pubkey:$key,operation_id:$op}')
    intent=$directory/$sid.prepare.json
    [[ ! -L $directory/$sid.prepare.lock ]] || die 'unsafe prepare lock'
    exec 8>"$directory/$sid.prepare.lock"
    flock -n 8 || die 'chatbot preparation active'
    if [[ -e $intent || -L $intent ]]; then
        root_path "$intent"
        [[ -f $intent && ! -L $intent && $(stat -c %a "$intent") == 600 ]] || die 'unsafe prepare intent'
        jq -e --argjson args "$args" --arg caller "$operator" '.prepare_args==$args and .caller_session==$caller' \
            "$intent" >/dev/null || die 'retained preparation uses another operation/caller; reconcile it, never allocate a fresh ID'
    else
        tmp=$(mktemp "$directory/.prepare.XXXXXX")
        jq -n --argjson args "$args" --arg caller "$operator" \
            '{version:1,phase:"prepare-intent",prepare_args:$args,caller_session:$caller}' > "$tmp"
        sync -f "$tmp"; ln "$tmp" "$intent"; rm -- "$tmp"; sync -f "$directory"
    fi
    [[ $(namespace_pin) == "$pin" ]] || die 'caller namespace changed before preparation'
    full=$(timeout --kill-after=5s 30s "$client" invoke --mount "$control_mount" --expected-session "$operator" \
        --confirm-side-effects --args "$args" plugins/identity_sled/operations/prepare_container/call) ||
        die "prepare failed or uncertain; retain $intent and reconcile ONLY operation $operation_id"
    [[ $(namespace_pin) == "$pin" ]] || die 'caller namespace changed; retain operation for reconciliation'
    result=$(jq -ce --arg sid "$sid" --arg op "$operation_id" '
        select(.plugin_id=="identity_sled" and .method=="prepare_container") |
        .result | select(.success==true and .outcome=="completed" and .plugin_id=="identity_sled" and .method=="prepare_container") |
        .result | select(.session_id==$sid and .operation_id==$op and .phase=="complete" and
            (.replayed|type=="boolean") and (.name|type=="string" and test("^(3tchedCtlChatbot|ID-[1-9][0-9]*)$") and length<=63) and
            (.native_uuid|type=="string" and test("^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$")))
    ' <<<"$full") || die 'canonical preparation receipt differs; retain intent'
    name=$(jq -er .name <<<"$result")
    uuid=$(jq -er .native_uuid <<<"$result")
    instance=$(timeout --kill-after=5s 30s incus --force-local query "/1.0/instances/$name?project=default") || die 'native readback unavailable; retain intent'
    jq -e --arg name "$name" --arg sid "$sid" --arg key "$key" --arg uuid "$uuid" --arg op "$operation_id" '
        .name==$name and .type=="container" and .config["volatile.uuid"]==$uuid and
        .config["user.opdbus.session_id"]==$sid and .config["user.opdbus.wireguard_public_key"]==$key and
        .config["user.opdbus.creation_operation"]==$op
    ' <<<"$instance" >/dev/null || die 'canonical/native preparation binding differs'
    [[ $(namespace_pin) == "$pin" ]] || die 'caller namespace changed after native readback'
    container_receipt=$directory/$name.incus.json
    [[ ! -L $directory/$name.lock ]] || die 'unsafe instance lock'
    exec 9>"$directory/$name.lock"
    flock -n 9 || die 'instance preparation active'
    if [[ -e $container_receipt || -L $container_receipt ]]; then
        root_path "$container_receipt"
        [[ -f $container_receipt && ! -L $container_receipt && $(stat -c %a "$container_receipt") == 600 ]] || die 'unsafe container receipt'
        jq -e --argjson args "$args" --argjson result "$result" --arg caller "$operator" '
            .prepare_args==$args and .caller_session==$caller and
            (.prepare_result|del(.replayed))==($result|del(.replayed))
        ' "$container_receipt" >/dev/null || die 'retained container receipt differs; no overwrite'
    else
        jq -e '.status=="Stopped" and .status_code==102' <<<"$instance" >/dev/null || die 'new candidate is not stopped'
        tmp=$(mktemp "$directory/.container.XXXXXX")
        jq -n --arg name "$name" --arg sid "$sid" --arg caller "$operator" --argjson args "$args" \
            --argjson result "$result" --argjson instance "$instance" '
            {version:1,phase:"container-defined",name:$name,session_id:$sid,
             pool_source:"/var/lib/incus-storage/identity-roots",identity_registered:false,root_bound:false,started:false,
             caller_session:$caller,prepare_args:$args,prepare_result:$result,instance:$instance}
        ' > "$tmp"
        sync -f "$tmp"; ln "$tmp" "$container_receipt"; rm -- "$tmp"; sync -f "$directory"
    fi
    tmp=$(mktemp "$directory/.prepare.XXXXXX")
    jq --argjson result "$result" '.phase="complete"|.prepare_result=$result' "$intent" > "$tmp"
    sync -f "$tmp"; mv -T "$tmp" "$intent"; sync -f "$directory"
    printf 'Prepared %s; receipt: %s; storage, binding and adoption remain separate.\n' "$name" "$container_receipt" >&2
    printf '%s\n' "$name"
)
case $1 in
    --prepare-adoption)
        shift
        script_dir=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
        adoption_helper=$script_dir/prepare-chatbot-adoption.sh
        [[ -x $adoption_helper ]] || adoption_helper=$script_dir/prepare-chatbot-adoption
        [[ -x $adoption_helper ]] || die 'adoption plan helper missing'
        exec "$adoption_helper" "$@"
        ;;
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
FSTORAGE_GIB=2 NO_START=false OPERATION_ID=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --principal-group) die '--principal-group is retired; manage authorization separately before provisioning; existing grants are preserved' ;;
        --pubkey|--session-id|--operator-session|--leaf-path|--fstorage-gib|--container-name)
            option=$1; IDENTITY_ONLY_FLAGS="$IDENTITY_ONLY_FLAGS $option"
            shift; value=${1:?option requires a value}
            case "$option" in
                --pubkey) PUBKEY=$value ;; --session-id) SESSION_ID=$value ;;
                --operator-session) OPERATOR_SESSION=$value ;; --leaf-path) LEAF_PATH=$value ;;
                --fstorage-gib) FSTORAGE_GIB=$value ;; --container-name) NATIVE_NAME=$value ;;
            esac ;;
        --operation-id) shift; OPERATION_ID=${1:?canonical operation UUID required} ;;
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
provision_system_container() (
    [ -z "$IDENTITY_ONLY_FLAGS" ] || die "--system cannot be combined with identity-only options:$IDENTITY_ONLY_FLAGS"
    [ -n "$SYSTEM_NAME" ] || die '--system requires a display/retry label'
    [ "${#SYSTEM_NAME}" -le 63 ] || die 'system retry label must be at most 63 characters'
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

    [[ $(id -u) == 0 ]] || die 'system preparation requires host root for durable operation receipts'
    valid_uuid "$OPERATION_ID" || die '--operation-id canonical UUID required; reuse it after interruption'
    for tool in jq findmnt realpath stat incus timeout flock sync mktemp; do need_cmd "$tool"; done
    client=$(realpath -e -- "${THREETCHED_FS:-/usr/local/libexec/op-dbus/bin/3tched-fs}")
    [[ -f $client && -x $client ]] || die 'mounted client unavailable'
    root_path "$client"
    lungs_mount=${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}
    # Mutations use this caller's mounted identity. Native observations below
    # are explicit host-maintenance readback, not a fallback after denial.
    namespace_pin() {
        [[ $(findmnt -rn -M "$lungs_mount" -o FSTYPE) == fuse.3tchedFS ]] || return 1
        findmnt -rn -M "$lungs_mount" -o ID || return "$?"
        timeout --kill-after=5s 30s jq -ce 'select(.format=="3tchedfs/1" and .mount_identity.kind=="host_session" and .mount_identity.session==.session_id) |
            select(.session_id|type=="string" and length>0) |
            {instance_uuid,session_id,source_catalog_hash,source_catalog_generation,view_id}' \
            "$lungs_mount/.3tchedfs.json"
    }
    # The client checks the typed mode as well; capture the declared host SID
    # rather than silently choosing a different principal for --system.
    expected_session=${OP_3TCHEDFS_EXPECTED_SESSION:-$(jq -er '.session_id|select(type=="string" and length>0)' "$lungs_mount/.3tchedfs.json")}
    pin=$(namespace_pin) || die 'operator namespace unavailable'
    jq -e --arg sid "$expected_session" '.session_id==$sid' "$lungs_mount/.3tchedfs.json" >/dev/null || die 'operator session differs'
    assert_namespace() {
        local current
        current=$(namespace_pin) || die 'operator namespace lost; do not retry an uncertain operation'
        [[ $current == "$pin" ]] || die 'operator namespace changed; reconcile before retrying'
    }
    read_instances() {
        local state
        assert_namespace
        # The declared public projection intentionally redacts names/config and
        # devices. Never widen it or invent a get/list plugin method. Read the
        # same local default-project provider for this host maintenance tool.
        state=$(timeout --kill-after=5s 30s incus --force-local query '/1.0/instances?recursion=1&project=default') || die 'bounded native instance readback failed'
        jq -e 'type=="array" and all(.[]; type=="object" and (.name|type=="string"))' <<<"$state" >/dev/null || die 'invalid native inventory'
        assert_namespace
        printf '%s\n' "$state"
    }
    incus_call() {
        local method=$1 args=$2 receipt
        assert_namespace
        receipt=$(timeout --kill-after=5s 30s "$client" invoke --mount "$lungs_mount" --expected-session "$expected_session" \
            --confirm-side-effects --args "$args" "plugins/incus/operations/$method/call") || return "$?"
        assert_namespace
        jq -ce --arg method "$method" 'select(.plugin_id=="incus" and .method==$method) |
            .result | select(.success==true and .outcome=="completed" and .plugin_id=="incus" and .method==$method) | .result' <<<"$receipt"
    }
    # Label is only a local durable retry fence and display metadata. It is NOT
    # a requested native name. Preserve the exact request/caller before invoke.
    label=$SYSTEM_NAME
    umask 077
    directory=${OP_SYSTEM_PROVISION_RECEIPTS:-/var/lib/opdbus-runtime/system-provision-receipts}
    [[ $directory == /* && $(realpath -m "$directory") == "$directory" && ! -L $directory ]] || die 'unsafe system receipt directory'
    root_path "$(dirname "$directory")"
    [[ -e $directory ]] || mkdir -m 0700 "$directory"
    root_path "$directory"
    [[ -d $directory && $(stat -c %a "$directory") == 700 ]] || die 'system receipt directory must be private'
    intent=$directory/$label.json
    [[ ! -L $directory/$label.lock ]] || die 'unsafe system creation lock'
    exec 9>"$directory/$label.lock"
    flock -n 9 || die 'system creation active'
    create_args=$(jq -cn --arg op "$OPERATION_ID" --arg label "$label" --arg image "$IMAGE" --arg pool "$SYSTEM_STORAGE" \
        '{operation_id:$op,image:$image,source_mode:"image",instance_type:"container",profiles:[],
          config:{"user.display_name":$label,"security.privileged":"false","security.nesting":"false","boot.autostart":"false"},
          devices:[{name:"root",device:{type:"disk",path:"/",pool:$pool}}]}')
    if [[ -e $intent || -L $intent ]]; then
        root_path "$intent"
        [[ -f $intent && ! -L $intent && $(stat -c %a "$intent") == 600 ]] || die 'unsafe system intent'
        jq -e --argjson args "$create_args" --arg caller "$expected_session" \
            '.args==$args and .caller_session==$caller' "$intent" >/dev/null ||
            die 'retained label uses another operation/request/caller; reconcile it, never allocate a fresh ID'
    else
        tmp=$(mktemp "$directory/.create.XXXXXX")
        jq -n --argjson args "$create_args" --arg caller "$expected_session" \
            '{version:1,phase:"create-intent",args:$args,caller_session:$caller}' > "$tmp"
        sync -f "$tmp"; ln "$tmp" "$intent"; rm -- "$tmp"; sync -f "$directory"
    fi
    created=$(incus_call create_instance "$create_args") || die "create failed or uncertain; retain $intent and reconcile ONLY operation $OPERATION_ID"
    jq -e --arg op "$OPERATION_ID" '.operation_id==$op and .phase=="complete" and .created==true and
        (.replayed|type=="boolean") and (.name|type=="string" and test("^ID-[1-9][0-9]*$") and length<=63) and
        (.native_uuid|type=="string" and test("^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"))
    ' <<<"$created" >/dev/null || die 'create receipt differs'
    SYSTEM_NAME=$(jq -er .name <<<"$created")
    native_uuid=$(jq -er .native_uuid <<<"$created")
    if jq -e 'has("result")' "$intent" >/dev/null; then
        jq -e --argjson result "$created" '(.result|del(.replayed))==($result|del(.replayed))' "$intent" >/dev/null || die 'retained native allocation changed'
    fi
    tmp=$(mktemp "$directory/.create.XXXXXX")
    jq --argjson result "$created" '.phase="complete"|.result=$result' "$intent" > "$tmp"
    sync -f "$tmp"; mv -T "$tmp" "$intent"; sync -f "$directory"
    printf 'Assigned %s; receipt: %s\n' "$SYSTEM_NAME" "$intent"
    verify_instance() {
        local status=$1 instances
        instances=$(read_instances) || return "$?"
        jq -e --arg name "$SYSTEM_NAME" --arg pool "$SYSTEM_STORAGE" --arg status "$status" --arg uuid "$native_uuid" --arg op "$OPERATION_ID" '
            [.[]|select(.name==$name)] | select(length==1) | .[0] |
            .type=="container" and .status==$status and .profiles==[] and
            .config["volatile.uuid"]==$uuid and .config["user.opdbus.creation_operation"]==$op and
            .config["security.privileged"]=="false" and .config["security.nesting"]=="false" and
            .config["boot.autostart"]=="false" and
            .devices.root.type=="disk" and .devices.root.path=="/" and .devices.root.pool==$pool
        ' <<<"$instances" >/dev/null
    }
    instance_list=$(read_instances) || die 'assigned instance readback failed'
    state=$(jq -er --arg name "$SYSTEM_NAME" '[.[]|select(.name==$name)]|select(length==1)|.[0].status' <<<"$instance_list") || die 'assigned instance missing or ambiguous'
    case "$state" in
        Stopped) verify_instance Stopped || die 'created instance state differs; reconcile retained candidate' ;;
        Running)
            [[ $NO_START != true && $(jq -r .replayed <<<"$created") == true ]] || die 'unexpected running candidate; no lifecycle mutation performed'
            verify_instance Running || die 'replayed running instance differs'
            ;;
        *) die 'unsupported assigned instance state; reconcile retained candidate' ;;
    esac
    if [[ $NO_START != true && $state == Stopped ]]; then
        # A retained start intent is uncertain, not permission to issue another
        # start. A Running readback above can finish it without a mutation.
        jq -e '.start_requested!=true' "$intent" >/dev/null || die 'retained start intent is not running; inspect before explicit recovery'
        tmp=$(mktemp "$directory/.start.XXXXXX")
        jq '.start_requested=true' "$intent" > "$tmp"
        sync -f "$tmp"; mv -T "$tmp" "$intent"; sync -f "$directory"
        start_args=$(jq -cn --arg name "$SYSTEM_NAME" '{name:$name}')
        started=$(incus_call start_instance "$start_args") || die 'start failed or outcome uncertain; inspect existing instance before retrying'
        jq -e --arg name "$SYSTEM_NAME" '.name==$name and .state=="Running"' <<<"$started" >/dev/null || die 'start receipt differs'
        verify_instance Running || die 'running instance readback differs'
    fi
    printf 'Standalone container %s: mounted Incus receipts and native state verified; grants unchanged.\n' "$SYSTEM_NAME"
)

if [[ $SYSTEM_MODE == true ]]; then
    if [[ $IMAGE_OVERRIDDEN == false ]]; then IMAGE=$SYSTEM_IMAGE; fi
    provision_system_container; exit 0
fi
die 'legacy unpartitioned provisioning is retired; use --prepare-container --operation-id UUID, then the assigned-name storage/adoption stages'
