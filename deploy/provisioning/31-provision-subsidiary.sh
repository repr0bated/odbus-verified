#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'subsidiary: %s\n' "$*" >&2; exit 1; }
usage() {
    printf '%s\n' 'Usage: 30-provision-container.sh --subsidiary PARENT --name NAME --lxd-pool POOL --image SHA256 [--dry-run]'
}

if [[ ${1:-} == --help ]]; then usage; exit 0; fi
parent=${1:?parent identity container required}
shift
name= pool= fingerprint= dry_run=${DRY_RUN:-0}
while (( $# )); do
    case "$1" in
        --name|--lxd-pool|--image)
            (( $# >= 2 )) || fail "missing value for $1"
            case "$1" in
                --name) name=$2 ;;
                --lxd-pool) pool=$2 ;;
                --image) fingerprint=$2 ;;
            esac
            shift 2
            ;;
        --dry-run) dry_run=1; shift ;;
        *) fail "unknown option: $1" ;;
    esac
done
[[ ${START:-0} == 0 ]] || fail 'a subsidiary must stay stopped until its own identity is admitted'
[[ $parent =~ ^[A-Za-z0-9][A-Za-z0-9-]{0,62}$ ]] || fail 'invalid Incus parent name'
[[ $name =~ ^[a-z][a-z0-9-]{0,62}$ && $name != "$parent" ]] || fail 'choose a distinct LXD subsidiary name'
[[ $pool =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || fail 'invalid or missing LXD pool'
[[ $fingerprint =~ ^[0-9a-f]{64}$ ]] || fail 'a full LXD image fingerprint is required'

here=$(cd -- "$(dirname -- "$0")" && pwd)
mount=${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}
state=${GOLDEN_ROOT:-/var/lib/incus/storage-pools/${OP_CONTAINER_POOL:-3tched-pool}/golden-os}/state.json
release=$(jq -ec '.live | select(.readonly == true and (.commit | test("^[0-9a-f]{40}$")) and (.id | test("^[0-9]+$")))' "$state") ||
    fail 'no identified read-only golden live snapshot'
commit=$(jq -r .commit <<<"$release")
subvolume=$(jq -r .id <<<"$release")
isolation=$(jq -nc '{
    "boot.autostart":"false",
    "security.protection.start":"true",
    "security.privileged":"false",
    "security.nesting":"false",
    "security.idmap.isolated":"true",
    "security.devlxd":"false",
    "security.syscalls.deny_default":"true",
    "security.syscalls.deny_compat":"true",
    "security.syscalls.intercept.mknod":"false",
    "security.syscalls.intercept.mount":"false",
    "security.syscalls.intercept.bpf":"false",
    "security.syscalls.intercept.setxattr":"false",
    "security.syscalls.intercept.sysinfo":"false",
    "limits.cpu":"2",
    "limits.memory":"2GiB",
    "limits.memory.enforce":"hard",
    "limits.processes":"512"
}')

plan() {
    local method=$1 metadata capability
    metadata=$mount/operations/lxd/$method/metadata.json
    capability=$(jq -er .required_capability "$metadata") ||
        fail "sealed LXD operation unavailable: $method"
    printf 'lxd.%s [%s]\n' "$method" "$capability"
}

if [[ $dry_run == 1 ]]; then
    printf 'Parent: %s\nSubsidiary: %s\nGolden snapshot: %s (%s)\nLXD pool: %s\nImage fingerprint: %s\n' \
        "$parent" "$name" "$commit" "$subvolume" "$pool" "$fingerprint"
    printf 'Pending isolation: %s\n' "$isolation"
    plan get_image
    plan create_instance
    plan get_operation_wait
    plan get_instance
    exit 0
fi

host_read=()
(( EUID == 0 )) || host_read=(sudo -n)
instances=$("${host_read[@]}" incus list --format json)
binding=$(jq -er --arg parent "$parent" '
    . as $all | .[] | select(.name == $parent and .type == "container")
    | .config["user.opdbus.session_id"] as $session
    | select(($session | type) == "string" and ($session | length) > 0)
    | select(.devices.sled.source == ("identity-" + $session))
    | select(((.config["user.opdbus.mode"] // "") == ""))
    | select([$all[] | select(.config["user.opdbus.session_id"] == $session)] | length == 1)
    | [$session, .devices.sled.pool] | @tsv
' <<<"$instances") || fail 'parent must be the unique owner of its own Incus identity sled'
IFS=$'\t' read -r session sled_pool <<<"$binding"
[[ $session =~ ^[0-9a-f-]{36}$ && $sled_pool =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] ||
    fail 'invalid parent identity sled binding'
sled=/var/lib/incus/storage-pools/$sled_pool/custom/default_identity-$session
[[ $("${host_read[@]}" cat -- "$sled/session_id") == "$session" ]] ||
    fail 'parent session does not match the identity sled'

invoke=$here/../scripts/3tchedfs-invoke.sh
image_args=$(jq -nc --arg fingerprint "$fingerprint" '{fingerprint:$fingerprint}')
image=$("$invoke" lxd get_image "$image_args")
jq -e --arg fingerprint "$fingerprint" --arg commit "$commit" --arg subvolume "$subvolume" '
    .result.result.result | select(.fingerprint == $fingerprint and .type == "container" and .public == false)
    | select(.properties["opdbus.golden.commit"] == $commit
        and .properties["opdbus.golden.subvolume"] == $subvolume)
' <<<"$image" >/dev/null || fail 'the private LXD image does not identify the live golden snapshot'

payload=$(jq -nc --arg name "$name" --arg parent "$parent" --arg session "$session" \
    --arg pool "$pool" --arg fingerprint "$fingerprint" --argjson isolation "$isolation" '
    {instance:{name:$name,type:"container",start:false,profiles:[],
      source:{type:"image",fingerprint:$fingerprint},
      devices:[{name:"root",device:{type:"disk",path:"/",pool:$pool,readonly:"true"}}],
      config:($isolation + {
        "user.opdbus.mode":"subsidiary","user.opdbus.admission":"pending",
        "user.opdbus.parent_instance":$parent,"user.opdbus.parent_session_id":$session})}}
')
creation=$("$invoke" lxd create_instance "$payload" --confirm-side-effects)
printf '%s\n' "$creation"
operation=$(jq -er '.result.result.operation.id | select(type == "string" and length > 0)' <<<"$creation") ||
    fail 'creation receipt has no LXD operation ID; inspect it before retrying'
wait_args=$(jq -nc --arg uuid "$operation" '{uuid:$uuid,timeout:30}')
outcome=$("$invoke" lxd get_operation_wait "$wait_args")
jq -e '.result.result.result | .status_code == 200' <<<"$outcome" >/dev/null ||
    fail "LXD operation $operation did not finish successfully; inspect it before retrying"
instance_args=$(jq -nc --arg name "$name" '{name:$name}')
instance=$("$invoke" lxd get_instance "$instance_args")
jq -e --arg name "$name" --arg fingerprint "$fingerprint" --arg session "$session" \
    --arg pool "$pool" --argjson isolation "$isolation" '
    .result.result.result | select(.name == $name and .status == "Stopped")
    | .expanded_config as $config
    | select(all($isolation | to_entries[]; $config[.key] == .value))
    | select(($config | keys | map(select(startswith("raw."))) | length) == 0)
    | select(.config["volatile.base_image"] == $fingerprint
        and .config["user.opdbus.mode"] == "subsidiary"
        and .config["user.opdbus.admission"] == "pending"
        and .config["user.opdbus.parent_session_id"] == $session
        and .config["user.opdbus.session_id"] == null
        and .config["user.opdbus.wireguard_public_key"] == null
        and .profiles == []
        and (.expanded_devices | length) == 1
        and (.expanded_devices[0] | .name == "root"
            and (.device | .type == "disk" and .path == "/"
                and .pool == $pool and .readonly == "true" and (.source // "") == "")))
    | {name, status, admission:.config["user.opdbus.admission"]}
' <<<"$instance" || fail "creation was receipted, but $name could not be verified; inspect it through LXD before retrying"
