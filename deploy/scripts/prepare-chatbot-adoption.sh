#!/usr/bin/env bash
# Pin the manually prepared GPT/overlay instance for identity-owner adoption.
# No format, member changes, start, credentials, grants or identity-store writes.
set -euo pipefail
fail() { printf 'prepare-chatbot-adoption: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 && $1 == 3tchedChatbot && $(id -u) == 0 ]] || fail 'usage (host root): prepare-chatbot-adoption 3tchedChatbot'
umask 077
name=$1
sid=b0c777e3-0955-cd8b-3d01-ca12998cae28
key=Env5zPMmM1Zhypu5/DY/yU4PRYW/4AOEq2KZ4OSc+18=
operator=414fbc56-03fb-20ab-758a-bcc7a7cbdc0e
pool=/var/lib/3tched-leaf-pool
disk=$pool/.provisioning/$name.json
container=$pool/.provisioning/$name.incus.json
plans=/var/lib/op-dbus/identity-provision-plans
[[ -f $disk && ! -L $disk && -f $container && ! -L $container ]] || fail 'operator preparation receipts absent'
jq -e '.phase=="filesystems-prepared" and .layout=="raw-gpt-root-upgrade-v1"' "$disk" >/dev/null || fail 'disk preparation incomplete'
jq -e '.phase=="root-bound" and .root_bound and .upgrade_paired and .root_view_readonly and (.identity_registered|not)' "$container" >/dev/null || fail 'root preparation incomplete or already adopted'
exec 9>"$pool/.provisioning/$name.lock"
flock -n 9 || fail 'instance preparation active'
if [[ ! -e $plans && ! -L $plans ]]; then mkdir -m 0700 "$plans"; fi
[[ $(realpath -e "$plans") == "$plans" && $(stat -c %u:%g:%a "$plans") == 0:0:700 ]] || fail 'unsafe private plan directory'
[[ ! -e $plans/$sid.json && ! -L $plans/$sid.json ]] || fail 'existing plan must be reconciled; never overwrite it'
work=$(mktemp -d "$pool/.provisioning/$name.adoption.XXXXXX")
printf 'Private preparation evidence: %s\n' "$work"
client=/opt/op-dbus/current/bin/threetched-fs
args=$(jq -cn --arg key "$key" '{wireguard_pubkey:$key}')
"$client" invoke --mount /run/mount/3tchedFS --expected-session "$operator" --args "$args" \
    plugins/identity_sled/operations/inspect_provision_preflight/call > "$work/preflight-receipt.json"
# Keep only revision/native metadata; no SID1 is read or copied.
jq -ce --arg sid "$sid" --arg key "$key" '
    [.. | objects | select(.session_id==$sid and .wireguard_pubkey==$key and has("expected_revision_operation_id"))] |
    select(length==1) | .[0] |
    select(.expected_revision==0 and .expected_revision_operation_id=="" and .previous_instance==null) |
    {expected_revision,expected_revision_operation_id,previous_instance,previous_instance_etag}
' "$work/preflight-receipt.json" > "$work/preflight.json" || fail 'owner did not admit fresh configured-key preflight'
socket=/var/lib/incus/unix.socket
curl --silent --show-error --fail --noproxy '*' --max-time 30 --unix-socket "$socket" \
    --dump-header "$work/instance.headers" --output "$work/instance-response.json" \
    "http://unix.socket/1.0/instances/$name"
jq -ce --arg name "$name" 'select(.type=="sync" and .status_code==200)|.metadata|
    select(.name==$name and .status=="Stopped" and .status_code==102)' \
    "$work/instance-response.json" > "$work/instance.json" || fail 'stopped Incus definition absent'
etag=$(awk 'tolower($1)=="etag:" {sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print}' "$work/instance.headers")
[[ -n $etag && $etag != *$'\n'* ]] || fail 'ambiguous Incus ETag'
incus --force-local query /1.0/storage-pools/identity-roots > "$work/pool.json"
root=$(jq -er .root_mount "$disk")
[[ $root == "$pool/.mounts/$name/root" && $(findmnt -rn -M "$root" -o FSTYPE) == btrfs ]] || fail 'prepared root mount absent'
members='{}'
for member in @ @agent_memory @agent_rules @agent_shared @agent_skills @agent_soul @agent_user; do
    show=$(btrfs subvolume show "$root/$member")
    id=$(awk '$1=="Subvolume" && $2=="ID:" {print $3}' <<<"$show")
    uuid=$(awk '$1=="UUID:" {print $2}' <<<"$show")
    members=$(jq -cn --argjson members "$members" --arg name "$member" --argjson id "$id" --arg uuid "$uuid" '$members+{($name):{id:$id,uuid:$uuid}}')
done
# The same seed UUID is exposed by both the physical partition and RO loop.
# Resolve the unique physical partition, rather than udev's current UUID alias.
seed=$(lsblk -J -o PATH,TYPE,UUID | jq -er --arg uuid "$(jq -er .seed_fsid "$disk")" \
    '[..|objects|select(.uuid==$uuid and .type=="part")|.path]|select(length==1)|.[0]')
generation=$(btrfs inspect-internal dump-super "$seed" | awk '$1=="generation" {print $2; exit}')
image=$(jq -er .image "$disk")
jq -n --arg sid "$sid" --arg key "$key" --arg seed "$seed" --arg etag "$etag" \
    --argjson generation "$generation" --argjson members "$members" \
    --argjson inode "$(stat -c %i "$image")" --argjson size "$(stat -c %s "$image")" \
    --argjson persist "$(stat -c %i "$root/@agent_user/persist")" \
    --slurpfile disk "$disk" --slurpfile container "$container" --slurpfile pool "$work/pool.json" \
    --slurpfile instance "$work/instance.json" --slurpfile old "$work/preflight.json" '
    $disk[0] as $d | $d.partition_table.partitions as $p |
    {version:4,session_id:$sid,wireguard_pubkey:$key,
     expected_revision:$old[0].expected_revision,expected_revision_operation_id:$old[0].expected_revision_operation_id,
     binding:{device_path:$d.image,mount_point:$d.root_mount,btrfs_uuid:$d.root_fsid,cozo_id:$sid,attached:true},
     seed_device:$seed,seed_fsid:$d.seed_fsid,seed_generation:$generation,leaf_inode:$inode,leaf_size:$size,
     members:$members,persist_inode:$persist,native_root:$container[0].threetched_root,
     pool_name:$pool[0].name,pool_source:$pool[0].config.source,instance_etag:$etag,instance:$instance[0],
     previous_instance_etag:"",previous_instance:null,
     partitioned_overlay:{disk_guid:($d.partition_table.id|ascii_downcase),
      root_partition:{partuuid:($p[0].uuid|ascii_downcase),start_sector:$p[0].start,sectors:$p[0].size},
      upgrade_partition:{partuuid:($p[1].uuid|ascii_downcase),start_sector:$p[1].start,sectors:$p[1].size},
      upgrade_fsid:$d.upgrade_fsid,host_upgrade_fsid:"5a88e7cd-9701-40a7-8301-ef076761886c"}}
' > "$work/plan.json"
[[ $(stat -c %s "$work/plan.json") -le 32768 ]] || fail 'plan exceeds owner bound'
# Stage on the plan directory's filesystem before exclusive publication.
plan_tmp=$(mktemp "$plans/.$sid.XXXXXX")
cat "$work/plan.json" > "$plan_tmp"
sync -f "$plan_tmp"
ln "$plan_tmp" "$plans/$sid.json"
rm -- "$plan_tmp"
sync -f "$plans/$sid.json"
sync -f "$plans"
digest=$(sha256sum "$plans/$sid.json"); digest=${digest%% *}
jq -cn --arg key "$key" --arg digest "$digest" '{wireguard_pubkey:$key,plan_sha256:$digest}' > "$work/provision-args.json"
printf 'Plan prepared, not adopted: %s\n' "$plans/$sid.json"
printf 'Declared operation: plugins/identity_sled/operations/provision_container/call\n'
printf 'Arguments: %s\n' "$work/provision-args.json"
