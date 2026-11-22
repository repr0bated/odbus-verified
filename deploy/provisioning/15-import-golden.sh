#!/usr/bin/env bash
# Host-owned import only: never creates, starts or replaces an Incus identity.
# Usage: 15-import-golden.sh SOURCE SOURCE_UUID SOURCE_FSID PARENT PARENT_UUID POOL_PARENT POOL_FSID SEED_NAME RECEIPT_DIR
# SOURCE/PARENT are readonly on the OS disk. POOL_PARENT is their retained
# send/receive baseline on the native pool. The output is one subvolume with
# an ORDINARY rootfs directory, so op-container snapshots retain every file.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
[[ $# == 9 && $(id -u) == 0 ]] || { echo 'require root and nine pinned arguments' >&2; exit 2; }
source=$1 source_uuid=$2 source_fsid=$3 parent=$4 parent_uuid=$5 pool_parent=$6 pool_fsid=$7 seed_name=$8 receipt_dir=$9
pool=/var/lib/incus/storage-pools/3tched-pool
[[ $seed_name =~ ^@golden-[a-zA-Z0-9-]+$ && $receipt_dir == /var/lib/opdbus-release/* ]] || exit 2
uuid_of() { btrfs subvolume show "$1" | awk '$1=="UUID:" {print $2}'; }
received_uuid() { btrfs subvolume show "$1" | awk '$1=="Received" && $2=="UUID:" {print $3}'; }
readonly() { [[ $(btrfs property get -ts "$1" ro) == ro=true ]]; }
[[ $(findmnt -rn -M "$pool" -o FSTYPE) == btrfs && $(findmnt -rn -M "$pool" -o UUID) == "$pool_fsid" ]] || exit 1
[[ $(findmnt -rn -T "$source" -o UUID) == "$source_fsid" && $(findmnt -rn -T "$parent" -o UUID) == "$source_fsid" ]] || exit 1
[[ $(uuid_of "$source") == "$source_uuid" && $(uuid_of "$parent") == "$parent_uuid" ]] || exit 1
readonly "$source"; readonly "$parent"; readonly "$pool_parent"
[[ $(received_uuid "$parent") == "$(uuid_of "$pool_parent")" && $(findmnt -rn -T "$pool_parent" -o UUID) == "$pool_fsid" ]] || exit 1
[[ -z $(btrfs subvolume list -o "$source") ]] || { echo 'nested source subvolumes require explicit transport' >&2; exit 1; }
[[ -f $source/etc/os-release && ! -e $source/rootfs && ! -e $source/metadata.yaml ]] || exit 1
install -d -m 0700 "$receipt_dir"
exec 9>"$receipt_dir/lock"
flock -n 9 || { echo 'import already owned' >&2; exit 1; }
staging=$pool/golden-import-$seed_name
seed=$pool/images/$seed_name
receipt=$receipt_dir/receipt.json
phase=prepared
record() {
    jq -n --arg phase "$phase" --arg source "$source" --arg source_uuid "$source_uuid" --arg source_fsid "$source_fsid" \
        --arg pool "$pool" --arg fsid "$pool_fsid" --arg seed "$seed" --arg staging "$staging" \
        --arg at "$(date -u +%FT%TZ)" --arg pid "$$" \
        '{version:1,phase:$phase,source:$source,source_uuid:$source_uuid,source_fsid:$source_fsid,pool:$pool,pool_fsid:$fsid,seed:$seed,staging:$staging,pid:$pid,updated_at:$at}' >"$receipt.new"
    sync -f "$receipt.new"; mv -T "$receipt.new" "$receipt"; sync -f "$receipt_dir"
}
finish() {
    rc=$?
    jq -n --argjson exit "$rc" --arg phase "$phase" --arg at "$(date -u +%FT%TZ)" '{exit:$exit,phase:$phase,finished_at:$at}' >"$receipt_dir/completion.json.new"
    sync -f "$receipt_dir/completion.json.new"; mv -T "$receipt_dir/completion.json.new" "$receipt_dir/completion.json"; sync -f "$receipt_dir"
}
trap finish EXIT
if [[ -e $receipt ]]; then
    jq -e --arg source "$source" --arg uuid "$source_uuid" --arg seed "$seed" --arg fsid "$pool_fsid" \
        '.source==$source and .source_uuid==$uuid and .seed==$seed and .pool_fsid==$fsid' "$receipt" >/dev/null
else
    [[ ! -e $staging && ! -e $seed ]] || { echo 'unowned import path exists' >&2; exit 1; }
    record
fi
install -d -m 0700 "$staging"
if [[ ! -e $staging/os ]]; then
    phase=receiving; record
    btrfs send -p "$parent" "$source" | btrfs receive "$staging"
fi
readonly "$staging/os"
[[ $(received_uuid "$staging/os") == "$source_uuid" ]] || exit 1
phase=received; record
if [[ ! -e $seed ]]; then btrfs subvolume snapshot "$staging/os" "$seed"; fi
[[ $(btrfs subvolume show "$seed" | awk '$1=="Parent" && $2=="UUID:" {print $3}') == "$(uuid_of "$staging/os")" ]] || exit 1
if ! readonly "$seed"; then
    mkdir -p "$seed/rootfs"
    shopt -s dotglob nullglob
    for entry in "$seed"/*; do
        case ${entry##*/} in rootfs|metadata.yaml|golden-provenance.json) continue;; esac
        [[ ! -e $seed/rootfs/${entry##*/} && ! -L $seed/rootfs/${entry##*/} ]] || { echo 'wrapper collision; inspect retained staging' >&2; exit 1; }
        mv -- "$entry" "$seed/rootfs/"
    done
    [[ -f $seed/rootfs/etc/os-release && -f $seed/rootfs/opt/op-build.src/Cargo.toml ]] || exit 1
    printf 'architecture: x86_64\ncreation_date: %s\nproperties:\n  description: op-dbus approved Golden OS %s\n  opdbus.golden.source_uuid: %s\n' "$(date +%s)" "$source_uuid" "$source_uuid" >"$seed/metadata.yaml"
    jq -n --arg source "$source" --arg uuid "$source_uuid" --arg fsid "$source_fsid" --arg parent "$parent" \
        '{version:1,source:$source,source_uuid:$uuid,source_fsid:$fsid,incremental_parent:$parent,rootfs_nested_subvolume:false}' >"$seed/golden-provenance.json"
    sync -f "$seed"
    btrfs property set -ts "$seed" ro true
fi
[[ -z $(btrfs subvolume list -o "$seed") ]] || exit 1
readonly "$seed"
phase=complete; record
jq --arg uuid "$(uuid_of "$seed")" --arg id "$(btrfs inspect-internal rootid "$seed")" --arg seed_name "$seed_name" \
    '. + {seed_uuid:$uuid,seed_id:$id,seed_location:{incus_dir:"/var/lib/incus",pool:"3tched-pool",seed_dir:"images",seed_name:$seed_name},runtime_staged:false}' "$receipt" >"$receipt.new"
sync -f "$receipt.new"; mv -T "$receipt.new" "$receipt"; sync -f "$receipt_dir"
cat "$receipt"
