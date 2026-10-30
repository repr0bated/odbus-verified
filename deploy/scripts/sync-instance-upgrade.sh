#!/usr/bin/env bash
# Upgrade OS only: host -> instance; never replicate user/agent subvolumes.
set -euo pipefail
export LC_ALL=C
fail() { printf 'sync-instance-upgrade: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $1 =~ ^(pair|sync|boot)$ && $2 =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || fail 'usage: sync-instance-upgrade pair|sync|boot NAME'
action=$1 name=$2
[[ $(id -u) == 0 ]] || fail 'host root required'
# Restoration uses the existing native/storage receipts below, not an online
# preparation replay; the bridge may itself depend on this root at boot.
for tool in jq btrfs blkid findmnt losetup mount umount flock incus stat realpath blockdev lsblk sfdisk awk find head timeout sync; do
    type -P "$tool" >/dev/null || fail "missing $tool"
done
# One bounded attempt, never a retry after an uncertain mount/API result.
readback() { timeout --kill-after=5s 15s "$@"; }
incus() { readback incus --force-local --project default "$@"; }
findmnt() { readback findmnt "$@"; }
stat() { readback stat "$@"; }
realpath() { readback realpath "$@"; }
blkid() { readback blkid "$@"; }
mount() { timeout --kill-after=5s 30s mount "$@"; }
umount() { timeout --kill-after=5s 30s umount "$@"; }
mounted() {
    local targets target
    # Query the complete table successfully before deciding a mount is absent;
    # a failed/timed-out filtered findmnt must not authorize another mount.
    targets=$(findmnt -rn -o TARGET) || fail 'cannot read mount table'
    while IFS= read -r target; do
        [[ $target != "$1" ]] || return 0
    done <<<"$targets"
    return 1
}
umask 077
pool=/var/lib/incus/storage-pools/3tched-pool
[[ -d $pool && ! -L $pool && $(realpath -e "$pool") == "$pool" &&
   $(findmnt -rn -M "$pool" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$pool" -o FSROOT) == / &&
   $(findmnt -rn -M "$pool" -o UUID) == a1ab7b42-12a8-437c-8f65-d0469892c8dd &&
   $(findmnt -rn -M / -o UUID) != a1ab7b42-12a8-437c-8f65-d0469892c8dd ]] ||
    fail 'canonical Incus loop pool is not mounted on its pinned filesystem'
receipt=$pool/.provisioning/$name.json
[[ -f $receipt && ! -L $receipt ]] || fail 'preparation receipt required'
jq -e --arg name "$name" --arg image "$pool/$name.img" \
    '.phase=="filesystems-prepared" and .name==$name and .image==$image and .filesystem_prepared==true' "$receipt" >/dev/null || fail 'disk not prepared'
image=$pool/$name.img
[[ -f $image && ! -L $image && $(stat -c %i "$image") == "$(jq -r .image_inode "$receipt")" && $(stat -c %s "$image") == "$(jq -r .image_bytes "$receipt")" ]] || fail 'image pins differ'
exec 8>"$pool/.provisioning/host-upgrade.lock"
flock -n 8 || fail 'host upgrade transfer active'
exec 9>"$pool/.provisioning/$name.lock"
flock -n 9 || fail 'instance preparation active'
instances=$(incus list -f json)
jq -e --arg name "$name" 'type=="array" and all(.[]; .name!=$name or .status=="Stopped")' \
    <<<"$instances" >/dev/null || fail 'instance must be stopped'
binding=$pool/.provisioning/$name.incus.json
# ID-N is only a selector for this existing configured chatbot. In particular,
# a disk filename/regex cannot authorize a new human provisioning path.
if [[ $name == 3tchedCtlChatbot || $name == 3tchedChatbot || $name == ID-* ]]; then
    [[ $name =~ ^(3tchedCtlChatbot|3tchedChatbot|ID-[1-9][0-9]*)$ && -f $binding && ! -L $binding ]] || fail 'configured chatbot preparation receipt required'
    jq -e --arg name "$name" '
        .version==1 and .name==$name and .session_id=="b0c777e3-0955-cd8b-3d01-ca12998cae28" and
        (.phase=="container-defined" or .phase=="root-bound") and
        .instance.name==$name and .instance.type=="container" and
        .instance.config["user.opdbus.session_id"]==.session_id and
        .instance.config["user.opdbus.wireguard_public_key"]=="Env5zPMmM1Zhypu5/DY/yU4PRYW/4AOEq2KZ4OSc+18=" and
        (.instance.config["volatile.uuid"]|type=="string" and test("^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"))
    ' "$binding" >/dev/null || fail 'configured chatbot preparation identity differs'
    instance=$(readback incus --force-local query "/1.0/instances/$name?project=default") || fail 'native chatbot readback failed'
    jq -e --slurpfile binding "$binding" --arg name "$name" '
        .name==$name and .type=="container" and .status=="Stopped" and .status_code==102 and
        .config["volatile.uuid"]==$binding[0].instance.config["volatile.uuid"] and
        .config["user.opdbus.session_id"]==$binding[0].session_id and
        .config["user.opdbus.wireguard_public_key"]==$binding[0].instance.config["user.opdbus.wireguard_public_key"]
    ' <<<"$instance" >/dev/null || fail 'native chatbot UUID/session/key differs'
fi
root=$pool/.mounts/$name/root
replica=$pool/.mounts/$name/upgrade
view=$pool/.mounts/$name/root-view
seed_layer=$pool/.mounts/$name/seed-layer
upgrade_layer=$pool/.mounts/$name/upgrade-layer
container_root=/var/lib/incus/storage-pools/identity-roots/containers/$name/rootfs
[[ $(jq -r .root_mount "$receipt") == "$root" && $(jq -r .upgrade_mount "$receipt") == "$replica" ]] || fail 'mount pins differ'
host_uuid=7de8757c-6b8e-43d0-82fc-47e9da564775
state=$pool/.provisioning/$name.upgrade.json
[[ ! -L $state && ! -e $state.new && ! -L $state.new ]] || fail 'unsafe or retained pair receipt; reconcile manually'
if [[ $action == pair ]]; then
    [[ ! -e $state ]] || fail 'pair exists; never repeat provisioning transfer'
else
    [[ -f $state ]] || fail 'initial pairing required'
    jq -e --arg host "$host_uuid" --arg replica "$(jq -r .upgrade_fsid "$receipt")" \
        '.phase=="paired" and .host_fsid==$host and .replica_fsid==$replica and .direction=="host-to-container" and
         (.generation|type=="string" and test("^[A-Za-z0-9-]{1,128}$"))' "$state" >/dev/null || fail 'incomplete or different pairing; inspect before recovery'
fi
generation=$(date -u +%Y%m%dT%H%M%S)-$$
commit_state() {
    sync -f "$state.new" || fail 'cannot persist pending receipt'
    mv -T "$state.new" "$state" || fail 'cannot publish pending receipt'
    sync -f "$pool/.provisioning" || fail 'cannot persist receipt directory'
}
transition_started=false
on_exit() {
    local result=$?
    if [[ $result != 0 && $transition_started == true ]]; then
        printf 'sync-instance-upgrade: incomplete transition retained at %s; inspect mounts and intent before manual recovery; no automatic retry/rollback.\n' "$state" >&2
    fi
}
trap on_exit EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
begin_transition() {
    [[ $transition_started != true ]] || return 0
    transition_started=true
    if [[ $action == pair ]]; then
        jq -n --arg host "$host_uuid" --arg replica "$(jq -r .upgrade_fsid "$receipt")" --arg next "$generation" \
            '{phase:"send-intent",host_fsid:$host,replica_fsid:$replica,direction:"host-to-container",next_generation:$next}' > "$state.new" || fail 'cannot write pair intent'
    else
        jq --arg next "$generation" '.phase="send-intent"|.next_generation=$next' "$state" > "$state.new" || fail 'cannot write sync intent'
    fi
    commit_state
}
# Restore the existing host source pair before resolving filesystem UUID aliases.
# The LV contains GPT, not a directly mountable Btrfs filesystem. Never map the
# retained staging image: it deliberately has the same filesystem UUIDs.
host_disk=$(realpath -e /dev/3tched-vg/3tched-seed) || fail 'host seed disk unavailable'
[[ -b $host_disk && $(readback blockdev --getsize64 "$host_disk") == 32212254720 &&
   $(readback blockdev --getro "$host_disk") == 0 ]] || fail 'host seed disk size/protection differs'
readback sfdisk --json "$host_disk" | jq -e '
    .partitiontable |
    .label=="gpt" and (.id|ascii_downcase)=="f514d49f-5053-4b2c-b8aa-e4b365a72036" and
    .unit=="sectors" and .sectorsize==512 and (.partitions|length)==2 and
    (.partitions[0] | .start==2048 and .size==20971520 and
        (.uuid|ascii_downcase)=="0dc47848-f43d-46c1-9b39-f633037e3bd4") and
    (.partitions[1] | .start==20973568 and .size==41940959 and
        (.uuid|ascii_downcase)=="8ed63510-2b70-47c7-a437-bcfcd8f5ea86")
' >/dev/null || fail 'host GPT source layout differs'
host_loops=$(readback losetup --json --list --associated "$host_disk" \
    --output NAME,BACK-FILE,OFFSET,SIZELIMIT,RO) || fail 'cannot inspect host source mappings'
host_loop_count=$(jq -er '.loopdevices | length' <<<"$host_loops") || fail 'invalid host loop inventory'
if [[ $host_loop_count == 0 ]]; then
    begin_transition
    # CleanPlan pins the physical, non-symlink seed path /dev/loop1p1.
    # A busy loop1 is a conflict, never permission to detach or choose another.
    host_loop=/dev/loop1
    readback losetup --partscan "$host_loop" "$host_disk" || fail 'pinned host source mapping incomplete'
else
    host_loop=$(jq -er --arg disk "$host_disk" '
        .loopdevices | select(length==1) | .[0] |
        select(.["back-file"]==$disk and .offset==0 and .sizelimit==0 and .ro==false) | .name
    ' <<<"$host_loops") || fail 'ambiguous or incompatible host source mapping'
fi
[[ $host_loop == /dev/loop1 &&
   -b ${host_loop}p1 && -b ${host_loop}p2 &&
   $(blkid -p -s UUID -o value "${host_loop}p1") == 6231f275-effd-4b6d-9ff7-e841531556b7 &&
   $(blkid -p -s UUID -o value "${host_loop}p2") == "$host_uuid" &&
   $(readback blockdev --getro "${host_loop}p2") == 0 ]] || fail 'host source partitions differ'
readback btrfs inspect-internal dump-super "${host_loop}p1" |
    awk '/SEEDING/ { sealed=1 } END { exit !sealed }' || fail 'host base is not sealed'
if [[ $(readback blockdev --getro "${host_loop}p1") != 1 ]]; then
    begin_transition
    readback blockdev --setro "${host_loop}p1" || fail 'cannot protect host seed partition'
fi
# Remount existing members after reboot, never add/remove a member or format.
if ! mounted "$root" || ! mounted "$replica"; then
    [[ $(jq -er .seed_device "$receipt") == /dev/disk/by-uuid/6231f275-effd-4b6d-9ff7-e841531556b7 &&
       $(jq -er .seed_fsid "$receipt") == 6231f275-effd-4b6d-9ff7-e841531556b7 ]] || fail 'seed receipt differs'
    seed=$(realpath -e "${host_loop}p1")
    # A timeout/cancellation of cold restoration must also block blind retries.
    begin_transition
    blockdev --setro "$seed"
    seed_loop=$(losetup --json --list --associated "$seed" |
        jq -r --arg seed "$seed" '[.loopdevices[] |
            select(."back-file"==$seed and .offset==0 and .sizelimit==0 and .ro==true)][0].name // empty')
    [[ -n $seed_loop ]] || seed_loop=$(losetup --find --show --read-only "$seed")
    [[ $(blockdev --getro "$seed_loop") == 1 ]] || fail 'seed loop must be read-only'
    disk_loop=$(losetup -j "$image" -O NAME --noheadings | head -n 1)
    [[ -n $disk_loop ]] || disk_loop=$(losetup --find --show --partscan "$image")
    [[ $(blkid -p -s UUID -o value "${disk_loop}p1") == "$(jq -r .root_fsid "$receipt")" && $(blkid -p -s UUID -o value "${disk_loop}p2") == "$(jq -r .upgrade_fsid "$receipt")" ]] || fail 'filesystem pins differ'
    mkdir -p "$root" "$replica"
    mounted "$root" || mount -o "rw,subvolid=5,device=$seed_loop,device=${disk_loop}p1" "${disk_loop}p1" "$root"
    mounted "$replica" || mount -o rw,subvolid=5 "${disk_loop}p2" "$replica"
fi
assert_fs() {
    local source
    [[ $(findmnt -rn -M "$1" -o FSTYPE) == btrfs && $(findmnt -rn -M "$1" -o FSROOT) == / ]] || fail "not a top-level Btrfs mount: $1"
    source=$(findmnt -rn -M "$1" -o SOURCE)
    [[ $(blkid -p -s UUID -o value "$source") == "$2" ]] || fail "wrong filesystem: $1"
}
assert_fs "$root" "$(jq -r .root_fsid "$receipt")"
assert_fs "$replica" "$(jq -r .upgrade_fsid "$receipt")"
host=$pool/.mounts/host-upgrade
[[ ! -L $host && ! -L $view ]] || fail 'unsafe mount path'
mkdir -p "$host"
if ! mounted "$host"; then
    begin_transition
    mount -o rw,subvolid=5 "${host_loop}p2" "$host"
fi
assert_fs "$host" "$host_uuid"
[[ $(realpath -e "$(findmnt -rn -M "$host" -o SOURCE)") == "$(realpath -e "${host_loop}p2")" ]] ||
    fail 'host upgrade backing differs'
readback btrfs subvolume show "$host/@" >/dev/null
nested=$(readback btrfs subvolume list -o "$host/@") || fail 'cannot inspect host OS subvolumes'
[[ -z $nested ]] || fail 'nested OS subvolumes require explicit transport'
# The pair starts empty. The read-only overlay falls through to the existing
# seed-root view until a patch supplies a replacement or an OverlayFS whiteout.
source_snapshots=$host/.upgrade-sync/$name
received=$replica/.upgrade-sync/$name
[[ ! -L $source_snapshots && ! -L $received ]] || fail 'unsafe transport paths'
mkdir -p "$source_snapshots" "$received"
if [[ $action == pair ]]; then
    entries=$(readback find "$replica/@" -mindepth 1 -print -quit) || fail 'cannot inspect initial upgrade'
    [[ -z $entries ]] || fail 'initial upgrade must be empty'
    parent=
else
    parent=$(jq -er .generation "$state")
    [[ $(readback btrfs property get -ts "$source_snapshots/$parent" ro) == 'ro=true' &&
       $(readback btrfs property get -ts "$received/$parent" ro) == 'ro=true' ]] || fail 'immutable common baseline missing'
fi
assert_dir() {
    [[ $(stat -c %F "$1") == directory && $(realpath -e "$1") == "$1" ]] || fail "unsafe or missing directory: $1"
}
assert_mode() {
    local options
    options=$(findmnt -rn -M "$1" -o VFS-OPTIONS) || fail "mount readback failed: $1"
    [[ ,$options, == *,"$2",* ]] || fail "mount mode differs ($2): $1"
}
assert_overlay() {
    local options
    [[ $(findmnt -rn -M "$1" -o FSTYPE) == overlay ]] || fail "not an overlay: $1"
    options=$(findmnt -rn -M "$1" -o FS-OPTIONS) || fail "overlay readback failed: $1"
    [[ ,$options, == *,"lowerdir=$upgrade_layer:$seed_layer",* &&
       ,$options, != *,upperdir=* && ,$options, != *,workdir=* ]] || fail "unexpected overlay lowers: $1"
    assert_mode "$1" ro
}
assert_bind() {
    local target=$1 source=$2 fsroot=$3 mode=$4 target_pin source_pin
    [[ $(findmnt -rn -M "$target" -o FSTYPE) == btrfs &&
       $(findmnt -rn -M "$target" -o FSROOT) == "$fsroot" ]] || fail "unexpected Btrfs bind: $target"
    target_pin=$(stat -c %d:%i "$target") || fail "cannot read bind: $target"
    source_pin=$(stat -c %d:%i "$source") || fail "cannot read bind source: $source"
    [[ $target_pin == "$source_pin" && $(stat -c %u:%g "$target") == 1000000:1000000 ]] || fail "bind source/idmap differs: $target"
    assert_mode "$target" "$mode"
}
restore_required=false
[[ $action != boot ]] || restore_required=true
if mounted "$container_root"; then restore_required=true; fi
if [[ -e $binding || -L $binding ]]; then
    [[ -f $binding && ! -L $binding ]] || fail 'unsafe native binding receipt'
    jq -e '.root_bound|type=="boolean"' "$binding" >/dev/null || fail 'invalid native binding receipt'
    if jq -e '.root_bound' "$binding" >/dev/null; then restore_required=true; fi
fi
validate_instance() {
    instance=$(readback incus --force-local query "/1.0/instances/$name?project=default") || fail 'native instance readback failed'
    jq -e --arg name "$name" --arg sid "$session_id" --arg uuid "$native_uuid" --arg key "$public_key" '
        .name==$name and .type=="container" and .status=="Stopped" and .status_code==102 and
        .config["volatile.uuid"]==$uuid and .config["user.opdbus.wireguard_public_key"]==$key and
        .config["user.opdbus.session_id"]==$sid and .config["security.privileged"]=="false" and
        .config["security.protection.shift"]=="true" and .devices.root.pool=="identity-roots" and
        .devices.root.type=="disk" and .devices.root.path=="/"' <<<"$instance" >/dev/null || fail 'stopped identity container required for root restoration'
    local current
    current=$(jq -er '
        def mapping:
            fromjson |
            select(type=="array" and length==2 and
                all(.[]; .Hostid==1000000 and .Nsid==0 and .Maprange==1000000000) and
                any(.[]; .Isuid==true and .Isgid==false) and
                any(.[]; .Isgid==true and .Isuid==false)) |
            sort_by(.Isuid, .Isgid);
        .config as $config |
        [$config | to_entries[] |
            select(.key=="volatile.idmap.current" or .key=="volatile.idmap.next") |
            .value] as $maps |
        select(($maps|length)>0) |
        [$maps[] | mapping] as $checked |
        select(($checked|length)==($maps|length) and all($checked[]; .==$checked[0])) |
        if $config|has("volatile.idmap.current")
        then $config["volatile.idmap.current"]
        else $config["volatile.idmap.next"] end
    ' <<<"$instance") || fail 'unsupported or differing current/next container idmap'
    [[ -z ${idmap:-} || $idmap == "$current" ]] || fail 'native idmap changed during synchronization'
    idmap=$current
}
if [[ $restore_required == true ]]; then
    [[ $action != pair && -f $binding && ! -L $binding ]] || fail 'existing bound root receipt required'
    jq -e --arg name "$name" --arg root "$container_root" --arg view "$view" '.name==$name and
        .root_bound==true and .threetched_root==$root and .root_view==$view and .root_view_readonly==true' \
        "$binding" >/dev/null || fail 'native binding pins differ; reconcile before sync'
    session_id=$(jq -er '.session_id' "$binding")
    native_uuid=$(jq -er '.instance.config["volatile.uuid"]' "$binding")
    public_key=$(jq -er '.instance.config["user.opdbus.wireguard_public_key"]|select(type=="string" and length>0)' "$binding")
    jq -e --arg sid "$session_id" --arg uuid "$native_uuid" --arg name "$name" '
        ([$sid,$uuid]|all(test("^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"))) and
        .instance.name==$name and .instance.config["user.opdbus.session_id"]==$sid
    ' "$binding" >/dev/null || fail 'native receipt UUID/session binding differs'
    validate_instance
    pool_source=$(jq -er '.pool_source' "$binding")
    [[ $pool_source == /var/lib/incus-storage/identity-roots ]] || fail 'unexpected native pool source'
    pool_state=$(readback incus --force-local query "/1.0/storage-pools/identity-roots?project=default")
    jq -e --arg source "$pool_source" '.driver=="dir" and .status=="Created" and .config.source==$source' \
        <<<"$pool_state" >/dev/null || fail 'native pool differs'
    mounted /var/lib/incus/storage-pools/identity-roots || fail 'native pool is not mounted'
    source_pin=$(stat -c %d:%i "$pool_source")
    [[ $(stat -c %d:%i /var/lib/incus/storage-pools/identity-roots) == "$source_pin" ]] || fail 'native pool mount source differs'
    assert_dir "$container_root"
    for member in memory rules shared skills soul user; do
        assert_dir "$root/@agent_$member"
        readback btrfs subvolume show "$root/@agent_$member" >/dev/null
        mode=false
        case "$member" in shared|soul) mode=true ;; esac
        [[ $(readback btrfs property get -ts "$root/@agent_$member" ro) == "ro=$mode" ]] || fail "sibling protection differs: $member"
    done
    assert_dir "$root/@agent_user/persist"
    [[ $(stat -c %u:%g:%a "$root/@agent_user/persist") == 0:0:700 ]] || fail 'private persistence ownership/mode differs'
    # Check known restoration inputs while the old root is still attached.
    # Overlay whiteouts/opaque directories still require merged-view readback.
    for relative in agent agent/memory agent/rules agent/shared agent/skills agent/soul agent/user opt opt/run-mounts opt/run-mounts/persist; do
        assert_dir "$root/@/$relative"
        if [[ -e $host/@/$relative || -L $host/@/$relative ]]; then assert_dir "$host/@/$relative"; fi
    done
fi
for target in "$container_root" "$view" "$upgrade_layer" "$seed_layer"; do
    [[ ! -L $target ]] || fail "unsafe mount path: $target"
    if mounted "$target"; then
        [[ $restore_required == true ]] || fail 'unowned retained root mounts; reconcile before sync'
        assert_dir "$target"
        case "$target" in
            "$container_root"|"$view")
                assert_overlay "$target"
                for member in memory rules shared skills soul user; do
                    if mounted "$target/agent/$member"; then
                        mode=rw
                        case "$member" in shared|soul) mode=ro ;; esac
                        assert_bind "$target/agent/$member" "$root/@agent_$member" "/@agent_$member" "$mode"
                    fi
                done
                if mounted "$target/opt/run-mounts/persist"; then
                    assert_bind "$target/opt/run-mounts/persist" "$root/@agent_user/persist" /@agent_user/persist rw
                fi
                ;;
            "$upgrade_layer") assert_bind "$target" "$replica/@" /@ ro ;;
            "$seed_layer") assert_bind "$target" "$root/@" /@ ro ;;
        esac
    fi
done
# Persist a non-complete intent BEFORE the first detach. Failure/cancellation
# never remounts a partially rotated tree or leaves a false paired receipt.
begin_transition
[[ $restore_required != true ]] || validate_instance
# Never change a lower tree under a mounted OverlayFS view. The stopped
# container's bind holds another reference, so detach that view first too.
detach_agent_mounts() {
    local target=$1 member
    for member in user soul skills shared rules memory; do
        if mounted "$target/agent/$member"; then umount "$target/agent/$member"; fi
    done
    if mounted "$target/opt/run-mounts/persist"; then umount "$target/opt/run-mounts/persist"; fi
}
if mounted "$container_root"; then
    assert_overlay "$container_root"
    detach_agent_mounts "$container_root"
    umount "$container_root"
fi
if mounted "$view"; then
    assert_overlay "$view"
    detach_agent_mounts "$view"
    umount "$view"
fi
for layer in "$upgrade_layer" "$seed_layer"; do
    if mounted "$layer"; then umount "$layer"; fi
done
assert_detached() {
    local table fsid
    # Btrfs assigns different st_dev values to its subvolumes; compare FSIDs,
    # not mount device numbers or volatile device aliases, for retained binds.
    # This is the host owner's namespace, not proof about arbitrary namespaces;
    # external lifecycle writers must remain quiescent during maintenance.
    table=$(findmnt --json --list -o TARGET,UUID,FSTYPE,FSROOT,FS-OPTIONS) || fail 'mount inventory unavailable'
    fsid=$(jq -er .upgrade_fsid "$receipt")
    jq -e --arg base "$pool/.mounts/$name" --arg native "$container_root" --arg fsid "$fsid" '
        all(.filesystems[];
            (.target != $native and (.target|startswith($native+"/")|not)) and
            (.target != ($base+"/root-view") and (.target|startswith($base+"/root-view/")|not)) and
            (.target != ($base+"/seed-layer") and (.target|startswith($base+"/seed-layer/")|not)) and
            (.target != ($base+"/upgrade-layer") and (.target|startswith($base+"/upgrade-layer/")|not)) and
            (if .fstype=="overlay" then ((.["fs-options"] // "")|contains($base)|not) else true end) and
            (if .fstype=="btrfs" then
                (.uuid|type=="string" and length>0) and
                (if .uuid==$fsid then (.fsroot!="/@" and (.fsroot|startswith("/@/")|not)) else true end)
             else true end))
    ' <<<"$table" >/dev/null || fail 'retained root/lower reference; refusing to change a mounted tree'
}
assert_detached
btrfs subvolume snapshot -r "$host/@" "$source_snapshots/$generation"
if [[ -n $parent ]]; then
    btrfs send -p "$source_snapshots/$parent" "$source_snapshots/$generation" | btrfs receive "$received"
else
    btrfs send "$source_snapshots/$generation" | btrfs receive "$received"
fi
sent_uuid=$(readback btrfs subvolume show "$source_snapshots/$generation" | awk '$1=="UUID:" {print $2}')
received_uuid=$(readback btrfs subvolume show "$received/$generation" | awk '$1=="Received" && $2=="UUID:" {print $3}')
[[ -n $sent_uuid && $sent_uuid == "$received_uuid" && $(readback btrfs property get -ts "$received/$generation" ro) == 'ro=true' ]] || fail 'receive identity/read-only receipt differs'
# Keep received snapshots immutable for future incremental sends. @ is a
# writable clone; the exposed root view, not this upgrade partition, is RO.
btrfs subvolume snapshot "$received/$generation" "$replica/.next-$generation"
[[ $restore_required != true ]] || validate_instance
assert_detached
mv "$replica/@" "$received/previous-$generation"
mv "$replica/.next-$generation" "$replica/@"
sync -f "$host"
sync -f "$replica"
if [[ $restore_required == true ]]; then
    # Both OS layers are read-only in the guest. The upgrade partition itself
    # remains writable for receive/rotation; user siblings are separate binds.
    # Patches deleting seed files must carry whiteouts in the upgrade layer.
    jq '.phase="restore-intent"' "$state" > "$state.new"
    commit_state
    validate_instance
    assert_dir "$container_root"
    mkdir -p "$view" "$seed_layer" "$upgrade_layer"
    for target in "$view" "$seed_layer" "$upgrade_layer"; do assert_dir "$target"; done
    # This kernel cannot idmap an OverlayFS mount. Map its Btrfs lowers first.
    mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$root/@" "$seed_layer"
    mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$replica/@" "$upgrade_layer"
    assert_bind "$seed_layer" "$root/@" /@ ro
    assert_bind "$upgrade_layer" "$replica/@" /@ ro
    mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
    assert_overlay "$view"
    # Check the effective merged mountpoints before binding any private data.
    # Never create directories or repair a lower tree through a mounted view.
    for relative in agent agent/memory agent/rules agent/shared agent/skills agent/soul agent/user opt opt/run-mounts opt/run-mounts/persist; do
        assert_dir "$view/$relative"
    done
    for member in memory rules shared skills soul user; do
        mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_$member" "$view/agent/$member"
        case "$member" in shared|soul) mount -o remount,bind,ro "$view/agent/$member" ;; esac
    done
    mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_user/persist" "$view/opt/run-mounts/persist"
    mount --rbind "$view" "$container_root"
    incus config set "$name" "volatile.last_state.idmap=$idmap"
    validate_instance
    [[ $(jq -er '.config["volatile.last_state.idmap"]' <<<"$instance") == "$idmap" ]] || fail 'native mapping readback differs'
    assert_bind "$seed_layer" "$root/@" /@ ro
    assert_bind "$upgrade_layer" "$replica/@" /@ ro
    view_pin=$(stat -c %d:%i "$view")
    [[ $(stat -c %d:%i "$container_root") == "$view_pin" &&
       $(stat -c %u:%g "$container_root") == 1000000:1000000 ]] || fail 'native root is not the mapped view'
    for target in "$view" "$container_root"; do
        assert_overlay "$target"
        for member in memory rules shared skills soul user; do
            mode=rw
            case "$member" in shared|soul) mode=ro ;; esac
            assert_bind "$target/agent/$member" "$root/@agent_$member" "/@agent_$member" "$mode"
        done
        assert_bind "$target/opt/run-mounts/persist" "$root/@agent_user/persist" /@agent_user/persist rw
    done
    validate_instance
fi
# Verify the published clone's ancestry even when this is an unbound initial
# pair. Successful mv/mount commands alone are not publication evidence.
published_parent=$(readback btrfs subvolume show "$replica/@" | awk '$1=="Parent" && $2=="UUID:" {print $3}')
baseline_uuid=$(readback btrfs subvolume show "$received/$generation" | awk '$1=="UUID:" {print $2}')
[[ -n $baseline_uuid && $published_parent == "$baseline_uuid" &&
   $(readback btrfs property get -ts "$replica/@" ro) == 'ro=false' ]] || fail 'published upgrade ancestry/mode differs'
# PairReceipt is deny_unknown_fields in the owner: keep its completed shape.
# A send/restore intent remains on every failed or interrupted restoration.
jq --arg generation "$generation" '.phase="paired"|.generation=$generation|del(.next_generation)' "$state" > "$state.new"
commit_state
readback jq -e --arg generation "$generation" '.phase=="paired" and .generation==$generation and
    (has("next_generation")|not)' "$state" >/dev/null || fail 'completion receipt readback differs'
transition_started=false
printf 'Upgrade synchronized host -> %s; user data excluded.\n' "$name"
