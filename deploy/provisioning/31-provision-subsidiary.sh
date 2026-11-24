#!/usr/bin/env bash
# Create one stopped, admission-pending LXD subsidiary through the sealed lxd
# plugin. The caller mounted at OP_3TCHEDFS_MOUNT is its parent and must be a
# live human identity. Start/admission is not part of this script.
set -euo pipefail

fail() { printf 'subsidiary: %s\n' "$*" >&2; exit 1; }
usage() {
    printf '%s\n' 'Usage: 30-provision-container.sh --subsidiary PARENT_PRINCIPAL_ID --name NAME --pubkey WG_PUBKEY --image SHA256 (--seed-uuid UUID | --template NAME) [--dry-run]'
}

if [[ ${1:-} == --help ]]; then usage; exit 0; fi
parent=${1:?parent principal id required}
shift
name= pubkey= fingerprint= seed= template= dry_run=${DRY_RUN:-0}
while (( $# )); do
    case "$1" in
        --name|--pubkey|--image|--seed-uuid|--template)
            (( $# >= 2 )) || fail "missing value for $1"
            case "$1" in
                --name) name=$2 ;;
                --pubkey) pubkey=$2 ;;
                --image) fingerprint=$2 ;;
                --seed-uuid) seed=$2 ;;
                --template) template=$2 ;;
            esac
            shift 2
            ;;
        --dry-run) dry_run=1; shift ;;
        *) fail "unknown option: $1" ;;
    esac
done
uuid='^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
[[ ${START:-0} == 0 ]] || fail 'a subsidiary must stay stopped until its own identity is admitted'
[[ $parent =~ $uuid ]] || fail 'invalid parent principal id'
[[ $name =~ ^[a-z][a-z0-9-]{0,62}$ ]] || fail 'invalid subsidiary name'
[[ $pubkey =~ ^[A-Za-z0-9+/]{43}=$ ]] || fail 'invalid WireGuard public key'
[[ $fingerprint =~ ^[0-9a-f]{64}$ ]] || fail 'a full LXD image fingerprint is required'
# Exactly one provenance property the private image must carry.
if [[ -n $template && -z $seed ]]; then
    [[ $template =~ ^[a-z0-9][a-z0-9.-]{0,62}$ ]] || fail 'invalid template name'
    property=opdbus.template expected=$template
elif [[ -z $template && $seed =~ $uuid ]]; then
    property=opdbus.seed.uuid expected=$seed
else
    fail 'give either the seed subvolume UUID or the template name'
fi

here=$(cd -- "$(dirname -- "$0")" && pwd)
mount=${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}
accept=${OP_IDENTITY_ACCEPT:-/usr/local/bin/op-identity-accept}
pool=lxd-pool
read -r session _ < <("$accept" derive --pubkey "$pubkey")
[[ $session =~ $uuid ]] || fail 'the key does not derive a session'

payload=$(jq -nc --arg name "$name" --arg parent "$parent" --arg session "$session" \
    --arg key "$pubkey" --arg pool "$pool" --arg fingerprint "$fingerprint" '
    {instance:{name:$name,type:"container",start:false,profiles:[],
      source:{type:"image",fingerprint:$fingerprint},
      devices:[
        {name:"root",device:{type:"disk",path:"/",pool:$pool}},
        {name:"sled",device:{type:"disk",path:"/agent/user",pool:$pool,
          source:("identity-" + $session),readonly:"true"}},
        {name:"run",device:{type:"disk",path:"/run",recursive:"true",
          source:("/run/opdbus/subsidiary/" + $session + "/run")}}],
      config:{
        "boot.autostart":"false",
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
        "limits.cpu":"2",
        "limits.memory":"2GiB",
        "limits.memory.enforce":"hard",
        "limits.processes":"512",
        "user.opdbus.mode":"subsidiary",
        "user.opdbus.admission":"pending",
        "user.opdbus.principal_kind":"service",
        "user.opdbus.session_id":$session,
        "user.opdbus.wireguard_public_key":$key,
        "user.opdbus.parent_principal_id":$parent}}}')

plan() {
    local capability
    capability=$(jq -er .required_capability "$mount/operations/lxd/$1/metadata.json") ||
        fail "sealed LXD operation unavailable: $1"
    printf 'lxd.%s [%s]\n' "$1" "$capability"
}
if [[ $dry_run == 1 ]]; then
    jq . <<<"$payload"
    for method in get_image get_storage_pool_volume create_instance get_operation_wait get_instance; do
        plan "$method"
    done
    exit 0
fi

invoke=$here/../scripts/3tchedfs-invoke.sh
image=$("$invoke" lxd get_image "$(jq -nc --arg f "$fingerprint" '{fingerprint:$f}')")
jq -e --arg f "$fingerprint" --arg p "$property" --arg v "$expected" '.result.result.result
    | select(.fingerprint == $f and .type == "container" and .public == false
        and .properties[$p] == $v)' <<<"$image" >/dev/null ||
    fail 'the private LXD image does not carry the expected seed or template'
"$invoke" lxd get_storage_pool_volume "$(jq -nc --arg pool "$pool" --arg n "identity-$session" \
    '{pool:$pool,vol_type:"custom",name:$n}')" >/dev/null ||
    fail "identity sled identity-$session is absent; install the Decoy-issued identity first"
creation=$("$invoke" lxd create_instance "$payload" --confirm-side-effects)
printf '%s\n' "$creation"
operation=$(jq -er '.result.result.operation.id | select(type == "string" and length > 0)' <<<"$creation") ||
    fail 'creation receipt has no LXD operation ID; inspect it before retrying'
outcome=$("$invoke" lxd get_operation_wait "$(jq -nc --arg uuid "$operation" '{uuid:$uuid,timeout:30}')")
jq -e '.result.result.result | .status_code == 200' <<<"$outcome" >/dev/null ||
    fail "LXD operation $operation did not finish successfully; inspect it before retrying"
instance=$("$invoke" lxd get_instance "$(jq -nc --arg name "$name" '{name:$name}')")
jq -e --argjson want "$payload" '.result.result.result | . as $got
    | select(.name == $want.instance.name and .status == "Stopped" and .profiles == []
        and (.expanded_devices | length) == 3)
    | select(all($want.instance.config | to_entries[]; $got.expanded_config[.key] == .value))
    | select(([$got.expanded_config | keys[] | select(startswith("raw."))] | length) == 0)
    | {name, status, admission:.config["user.opdbus.admission"]}' <<<"$instance" ||
    fail "creation was receipted, but $name could not be verified; inspect it through LXD before retrying"
# NIC-less BPF egress is a host service, installed when the subsidiary is
# started (this script creates it stopped): sudo deploy/runit/ct-egress-bpf/install.sh <name>
# The service discovers LXD containers through /var/lib/lxd/unix.socket.
