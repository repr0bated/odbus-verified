#!/usr/bin/env bash
# Bind the already prepared chatbot sprout. No format, member change or start.
set -euo pipefail
fail() { printf 'bind-instance-root: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 && $1 == 3tchedChatbot && $(id -u) == 0 ]] || fail 'usage: bind-instance-root 3tchedChatbot (host root)'
incus() { command incus --force-local "$@"; }
umask 077
name=$1
pool=/var/lib/3tched-leaf-pool
disk=$pool/.provisioning/$name.json
receipt=$pool/.provisioning/$name.incus.json
[[ ! -L $disk && -f $disk && ! -L $receipt && -f $receipt ]] || fail 'operator preparation receipts required'
exec 9>"$pool/.provisioning/$name.lock"
flock -n 9 || fail 'instance preparation active'
jq -e '.phase=="filesystems-prepared" and .filesystem_prepared==true' "$disk" >/dev/null || fail 'filesystems not prepared'
jq -e '.phase=="container-defined" and .identity_registered==false and .root_bound==false' "$receipt" >/dev/null || fail 'existing root intent requires manual reconciliation'
root=$pool/.mounts/$name/root
replica=$pool/.mounts/$name/upgrade
view=$pool/.mounts/$name/root-view
seed_layer=$pool/.mounts/$name/seed-layer
upgrade_layer=$pool/.mounts/$name/upgrade-layer
container_root=/var/lib/incus/storage-pools/identity-roots/containers/$name/rootfs
sid=b0c777e3-0955-cd8b-3d01-ca12998cae28
image=$pool/$name.img
[[ $(findmnt -rn -M "$pool" -o FSROOT) == /@3tched-leaf-pool &&
   $(jq -r .image "$disk") == "$image" && -f $image && ! -L $image &&
   $(stat -c %i "$image") == "$(jq -r .image_inode "$disk")" &&
   $(stat -c %s "$image") == "$(jq -r .image_bytes "$disk")" &&
   $(jq -r .root_mount "$disk") == "$root" &&
   $(findmnt -rn -M "$root" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$root" -o FSROOT) == / ]] || fail 'prepared root/image pins differ'
root_source=$(findmnt -rn -M "$root" -o SOURCE)
[[ $(blkid -p -s UUID -o value "$root_source") == "$(jq -r .root_fsid "$disk")" ]] || fail 'wrong sprout filesystem'
pair=$pool/.provisioning/$name.upgrade.json
[[ -f $pair && ! -L $pair ]] || fail 'upgrade pair must be established during provisioning'
jq -e --arg replica "$(jq -r .upgrade_fsid "$disk")" '.phase=="paired" and
    .host_fsid=="5a88e7cd-9701-40a7-8301-ef076761886c" and .replica_fsid==$replica and
    .direction=="host-to-container"' "$pair" >/dev/null || fail 'upgrade pair differs'
[[ $(findmnt -rn -M "$replica" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$replica" -o FSROOT) == / ]] || fail 'upgrade filesystem not mounted'
for layer in "$view" "$seed_layer" "$upgrade_layer"; do
    [[ ! -L $layer && -z $(findmnt -rn -M "$layer") ]] || fail 'retained OS view requires reconciliation'
done
instance=$(incus query "/1.0/instances/$name")
jq -e --arg sid "$sid" '.status=="Stopped" and .config["user.opdbus.session_id"]==$sid and
   .config["security.privileged"]=="false" and .config["security.protection.shift"]=="true" and
   .devices.root.pool=="identity-roots" and .profiles==["3tched-lungs"]' <<<"$instance" >/dev/null || fail 'stopped container binding differs'
idmap=$(jq -er '.config["volatile.idmap.current"]' <<<"$instance")
jq -e 'length==2 and all(.[]; .Hostid==1000000 and .Nsid==0 and .Maprange==1000000000) and
    any(.[]; .Isuid==true and .Isgid==false) and any(.[]; .Isgid==true and .Isuid==false)' <<<"$idmap" >/dev/null || fail 'unsupported container idmap'
[[ ! -L $container_root && -d $container_root && $(realpath -e "$container_root") == "$container_root" &&
   -z $(findmnt -rn -M "$container_root") && ! -L $root/@/etc/fstab && -f $root/@/etc/fstab &&
   ! -L $root/@/etc/hostname && -f $root/@/etc/hostname ]] || fail 'unexpected root mount or guest configuration'
for member in memory rules shared skills soul user; do
    [[ -d $root/@/agent/$member && ! -L $root/@/agent/$member &&
       -d $root/@agent_$member && ! -L $root/@agent_$member ]] || fail 'missing ordinary mountpoint or sibling'
    btrfs subvolume show "$root/@agent_$member" >/dev/null
done
# Keep the Incus empty-root scaffolding underneath the bind; never delete it.
[[ ! -e $pool/.provisioning/$name.before-root.json ]] || fail 'retained container backup requires reconciliation'
printf '%s\n' "$instance" > "$pool/.provisioning/$name.before-root.json"
cp -a "$root/@/etc/fstab" "$pool/.provisioning/$name.fstab.before"
jq '.phase="root-bind-intent"' "$receipt" > "$receipt.new"
sync -f "$receipt.new"
mv -T "$receipt.new" "$receipt"
for device in 3tchedfs system-bus; do
    if jq -e --arg device "$device" '.devices|has($device)' <<<"$instance" >/dev/null; then
        incus config device remove "$name" "$device"
    fi
done
if jq -e '.devices|has("fuse")' <<<"$instance" >/dev/null; then
    jq -e '.devices.fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"}' <<<"$instance" >/dev/null || fail 'different fuse device'
else
    incus config device add "$name" fuse unix-char source=/dev/fuse path=/dev/fuse
fi
printf '%s\n' '# Container root and agent subvolumes are mounted by the external identity mount owner.' \
    '# Do not mount the physical seed or host boot partitions inside this container.' > "$root/@/etc/fstab"
printf '%s\n' "$sid" > "$root/@/etc/hostname"
incus config set "$name" "volatile.last_state.idmap=$idmap"
sync -f "$root"
mkdir -p "$view" "$seed_layer" "$upgrade_layer"
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$root/@" "$seed_layer"
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$replica/@" "$upgrade_layer"
mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
mount --bind "$view" "$container_root"
for member in memory rules shared skills soul user; do
    mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_$member" "$container_root/agent/$member"
    case "$member" in shared|soul) mount -o remount,bind,ro "$container_root/agent/$member" ;; esac
done
mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_user/persist" "$container_root/opt/run-mounts/persist"
instance=$(incus query "/1.0/instances/$name")
jq --arg root "$container_root" --arg view "$view" --argjson instance "$instance" \
   '.phase="root-bound"|.root_bound=true|.threetched_root=$root|.instance=$instance|
    .root_view=$view|.root_view_readonly=true|.upgrade_paired=true|
    .guest_fuse_configured=true|.guest_fuse_mounted=false|.host_fuse_mirrored=false' "$receipt" > "$receipt.new"
sync -f "$receipt.new"
mv -T "$receipt.new" "$receipt"
sync -f "$pool/.provisioning"
printf 'Existing root bound; container stopped, identity adoption and live guest FUSE remain pending.\n'
