#!/usr/bin/env bash
# Upgrade OS only: host -> instance; never replicate user/agent subvolumes.
set -euo pipefail
fail() { printf 'sync-instance-upgrade: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $1 =~ ^(pair|sync|boot)$ && $2 =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || fail 'usage: sync-instance-upgrade pair|sync|boot NAME'
action=$1 name=$2
[[ $(id -u) == 0 ]] || fail 'host root required'
incus() { command incus --force-local "$@"; }
for tool in jq btrfs blkid findmnt losetup mount umount flock incus stat realpath blockdev lsblk awk find head; do
    command -v "$tool" >/dev/null || fail "missing $tool"
done
umask 077
pool=/var/lib/3tched-leaf-pool
[[ $(findmnt -rn -M "$pool" -o FSROOT) == /@3tched-leaf-pool ]] || fail 'leaf pool not mounted'
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
[[ $(jq --arg name "$name" '[.[]|select(.name==$name and .status!="Stopped")]|length' <<<"$instances") == 0 ]] || fail 'instance must be stopped'
root=$pool/.mounts/$name/root
replica=$pool/.mounts/$name/upgrade
view=$pool/.mounts/$name/root-view
seed_layer=$pool/.mounts/$name/seed-layer
upgrade_layer=$pool/.mounts/$name/upgrade-layer
container_root=/var/lib/incus/storage-pools/identity-roots/containers/$name/rootfs
[[ $(jq -r .root_mount "$receipt") == "$root" && $(jq -r .upgrade_mount "$receipt") == "$replica" ]] || fail 'mount pins differ'
# Remount existing members after reboot, never add/remove a member or format.
if ! findmnt -rn -M "$root" >/dev/null || ! findmnt -rn -M "$replica" >/dev/null; then
    seed=$(jq -er .seed_device "$receipt")
    [[ $seed == /dev/disk/by-uuid/"$(jq -r .seed_fsid "$receipt")" && -b $seed ]] || fail 'seed unavailable'
    # UUID aliases can resolve to an already registered RO loop view.
    seed=$(lsblk -J -o PATH,TYPE,UUID | jq -er --arg uuid "$(jq -er .seed_fsid "$receipt")" \
        '[..|objects|select(.uuid==$uuid and .type=="part")|.path]|select(length==1)|.[0]') || fail 'physical seed is not unique'
    blockdev --setro "$seed"
    seed_loop=$(losetup -j "$(realpath -e "$seed")" -O NAME --noheadings | head -n 1)
    [[ -n $seed_loop ]] || seed_loop=$(losetup --find --show --read-only "$seed")
    [[ $(blockdev --getro "$seed_loop") == 1 ]] || fail 'seed loop must be read-only'
    disk_loop=$(losetup -j "$image" -O NAME --noheadings | head -n 1)
    [[ -n $disk_loop ]] || disk_loop=$(losetup --find --show --partscan "$image")
    [[ $(blkid -p -s UUID -o value "${disk_loop}p1") == "$(jq -r .root_fsid "$receipt")" && $(blkid -p -s UUID -o value "${disk_loop}p2") == "$(jq -r .upgrade_fsid "$receipt")" ]] || fail 'filesystem pins differ'
    mkdir -p "$root" "$replica"
    findmnt -rn -M "$root" >/dev/null || mount -o "rw,subvolid=5,device=$seed_loop,device=${disk_loop}p1" "${disk_loop}p1" "$root"
    findmnt -rn -M "$replica" >/dev/null || mount -o rw,subvolid=5 "${disk_loop}p2" "$replica"
fi
assert_fs() {
    local source
    [[ $(findmnt -rn -M "$1" -o FSTYPE) == btrfs && $(findmnt -rn -M "$1" -o FSROOT) == / ]] || fail "not a top-level Btrfs mount: $1"
    source=$(findmnt -rn -M "$1" -o SOURCE)
    [[ $(blkid -p -s UUID -o value "$source") == "$2" ]] || fail "wrong filesystem: $1"
}
assert_fs "$root" "$(jq -r .root_fsid "$receipt")"
assert_fs "$replica" "$(jq -r .upgrade_fsid "$receipt")"
host_uuid=5a88e7cd-9701-40a7-8301-ef076761886c
host=$pool/.mounts/host-upgrade
[[ ! -L $host && ! -L $view ]] || fail 'unsafe mount path'
mkdir -p "$host"
findmnt -rn -M "$host" >/dev/null || mount -o rw,subvolid=5 "/dev/disk/by-uuid/$host_uuid" "$host"
assert_fs "$host" "$host_uuid"
btrfs subvolume show "$host/@" >/dev/null
[[ -z $(btrfs subvolume list -o "$host/@") ]] || fail 'nested OS subvolumes require explicit transport'
# The pair starts empty. The read-only overlay falls through to the existing
# seed-root view until a patch supplies a replacement or an OverlayFS whiteout.
state=$pool/.provisioning/$name.upgrade.json
[[ ! -L $state ]] || fail 'unsafe pair receipt'
source_snapshots=$host/.upgrade-sync/$name
received=$replica/.upgrade-sync/$name
[[ ! -L $source_snapshots && ! -L $received ]] || fail 'unsafe transport paths'
mkdir -p "$source_snapshots" "$received"
if [[ $action == pair ]]; then
    [[ ! -e $state ]] || fail 'pair exists; never repeat provisioning transfer'
    [[ -z $(find "$replica/@" -mindepth 1 -print -quit) ]] || fail 'initial upgrade must be empty'
    jq -n --arg host "$host_uuid" --arg replica "$(jq -r .upgrade_fsid "$receipt")" \
        '{phase:"pair-intent",host_fsid:$host,replica_fsid:$replica,direction:"host-to-container"}' > "$state"
else
    [[ -f $state ]] || fail 'initial pairing required'
    jq -e --arg host "$host_uuid" --arg replica "$(jq -r .upgrade_fsid "$receipt")" \
        '.phase=="paired" and .host_fsid==$host and .replica_fsid==$replica and .direction=="host-to-container"' "$state" >/dev/null || fail 'incomplete or different pairing; inspect before recovery'
fi
# Never change a lower tree under a mounted OverlayFS view. The stopped
# container's bind holds another reference, so detach that view first too.
detach_agent_mounts() {
    local target=$1 member
    for member in user soul skills shared rules memory; do
        if findmnt -rn -M "$target/agent/$member" >/dev/null; then umount "$target/agent/$member"; fi
    done
    if findmnt -rn -M "$target/opt/run-mounts/persist" >/dev/null; then umount "$target/opt/run-mounts/persist"; fi
}
if findmnt -rn -M "$container_root" >/dev/null; then
    [[ $(findmnt -rn -M "$container_root" -o FSTYPE) == overlay &&
       $(findmnt -rn -M "$container_root" -o FS-OPTIONS) == *"lowerdir=$upgrade_layer:$seed_layer"* ]] || fail 'unexpected container root mount; reconcile manually'
    detach_agent_mounts "$container_root"
    umount "$container_root"
fi
if findmnt -rn -M "$view" >/dev/null; then
    [[ $(findmnt -rn -M "$view" -o FSTYPE) == overlay &&
       $(findmnt -rn -M "$view" -o FS-OPTIONS) == *"lowerdir=$upgrade_layer:$seed_layer"* ]] || fail 'unexpected root view mount'
    detach_agent_mounts "$view"
    umount "$view"
fi
for layer in "$upgrade_layer" "$seed_layer"; do
    if findmnt -rn -M "$layer" >/dev/null; then umount "$layer"; fi
done
generation=$(date -u +%Y%m%dT%H%M%S)-$$
parent=$(jq -r '.generation // empty' "$state")
jq --arg next "$generation" '.phase="send-intent"|.next_generation=$next' "$state" > "$state.new"
mv -T "$state.new" "$state"
sync -f "$pool/.provisioning"
btrfs subvolume snapshot -r "$host/@" "$source_snapshots/$generation"
if [[ -n $parent ]]; then
    [[ $(btrfs property get -ts "$source_snapshots/$parent" ro) == 'ro=true' && $(btrfs property get -ts "$received/$parent" ro) == 'ro=true' ]] || fail 'immutable common baseline missing'
    btrfs send -p "$source_snapshots/$parent" "$source_snapshots/$generation" | btrfs receive "$received"
else
    btrfs send "$source_snapshots/$generation" | btrfs receive "$received"
fi
sent_uuid=$(btrfs subvolume show "$source_snapshots/$generation" | awk '$1=="UUID:" {print $2}')
received_uuid=$(btrfs subvolume show "$received/$generation" | awk '$1=="Received" && $2=="UUID:" {print $3}')
[[ -n $sent_uuid && $sent_uuid == "$received_uuid" && $(btrfs property get -ts "$received/$generation" ro) == 'ro=true' ]] || fail 'receive identity/read-only receipt differs'
# Keep received snapshots immutable for future incremental sends. @ is a
# writable clone; the exposed root view, not this upgrade partition, is RO.
btrfs subvolume snapshot "$received/$generation" "$replica/.next-$generation"
mv "$replica/@" "$received/previous-$generation"
mv "$replica/.next-$generation" "$replica/@"
sync -f "$host"
sync -f "$replica"
jq --arg generation "$generation" '.phase="paired"|.generation=$generation|del(.next_generation)' "$state" > "$state.new"
mv -T "$state.new" "$state"
sync -f "$pool/.provisioning"
if [[ $action == boot ]]; then
    # Both OS layers are read-only in the guest. The upgrade partition itself
    # remains writable for receive/rotation; user siblings are separate binds.
    # Patches deleting seed files must carry whiteouts in the upgrade layer.
    instance=$(incus query "/1.0/instances/$name")
    jq -e '.status=="Stopped" and .config["security.privileged"]=="false" and
        .config["security.protection.shift"]=="true" and .devices.root.pool=="identity-roots"' \
        <<<"$instance" >/dev/null || fail 'stopped identity container required for boot mounts'
    idmap=$(jq -er '.config["volatile.idmap.current"]' <<<"$instance")
    jq -e 'length==2 and all(.[]; .Hostid==1000000 and .Nsid==0 and .Maprange==1000000000) and
        any(.[]; .Isuid==true and .Isgid==false) and any(.[]; .Isgid==true and .Isuid==false)' \
        <<<"$idmap" >/dev/null || fail 'unsupported identity idmap'
    [[ -d $container_root && ! -L $container_root &&
       $(realpath -e "$container_root") == "$container_root" ]] || fail 'unsafe Incus root path'
    mkdir -p "$view" "$seed_layer" "$upgrade_layer"
    # This kernel cannot idmap an OverlayFS mount. Map its Btrfs lowers first.
    mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$root/@" "$seed_layer"
    mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$replica/@" "$upgrade_layer"
    mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
    for member in memory rules shared skills soul user; do
        [[ -d $view/agent/$member && ! -L $view/agent/$member ]] || fail "upgrade missing /agent/$member"
        mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_$member" "$view/agent/$member"
        case "$member" in shared|soul) mount -o remount,bind,ro "$view/agent/$member" ;; esac
    done
    mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_user/persist" "$view/opt/run-mounts/persist"
    mount --rbind "$view" "$container_root"
    incus config set "$name" "volatile.last_state.idmap=$idmap"
fi
printf 'Upgrade synchronized host -> %s; user data excluded.\n' "$name"
