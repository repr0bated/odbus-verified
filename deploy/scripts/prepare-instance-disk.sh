#!/usr/bin/env bash
# Completed manual provisioning step: one sparse disk, root + upgrade partitions.
# Storage preparation only; identity registration and container startup are separate.
set -euo pipefail
fail() { printf 'prepare-instance-disk: %s\n' "$*" >&2; exit 1; }
usage() {
    cat <<'USAGE'
Usage:
  prepare-instance-disk create NAME [--root-gib N] [--upgrade-gib N]
  prepare-instance-disk inspect NAME
  prepare-instance-disk prepare NAME --seed-device /dev/disk/by-uuid/UUID

Pool: /var/lib/3tched-leaf-pool (mounted @3tched-leaf-pool)
Image: NAME.img, sparse raw, GPT root + upgrade partitions
Defaults: 20 GiB root, remaining approximately 40 GiB upgrade.
Existing images are never adopted, repartitioned or overwritten by create.
inspect reads the image and reports its partition table; it does not modify it.
USAGE
}
[[ $# -gt 0 ]] || { usage; exit 2; }
case $1 in -h|--help) usage; exit 0 ;; esac
[[ $# -ge 2 ]] || fail 'action and instance name required'
action=$1 name=$2
shift 2
[[ $action == create || $action == inspect || $action == prepare ]] || fail 'unknown action'
[[ $name =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || fail 'invalid instance name'
root_gib=20 upgrade_gib=40 seed_device=
while [[ $# -gt 0 ]]; do
    [[ $action != inspect ]] || fail 'inspect takes no options'
    [[ $# -ge 2 ]] || fail "missing value for $1"
    case $1 in
        --root-gib) [[ $action == create ]] || fail 'size applies to create'; root_gib=$2 ;;
        --upgrade-gib) [[ $action == create ]] || fail 'size applies to create'; upgrade_gib=$2 ;;
        --seed-device) [[ $action == prepare ]] || fail 'seed applies to prepare'; seed_device=$2 ;;
        *) fail "unknown option: $1" ;;
    esac
    shift 2
done
[[ $(id -u) == 0 ]] || fail 'host root owns disk preparation'
for tool in qemu-img sfdisk jq findmnt realpath stat flock date sync; do
    command -v "$tool" >/dev/null || fail "missing $tool"
done
for size in "$root_gib" "$upgrade_gib"; do
    [[ $size =~ ^[1-9][0-9]{0,3}$ ]] && (( size <= 1024 )) || fail 'partition size must be 1 to 1024 GiB'
done
pool=/var/lib/3tched-leaf-pool
[[ -d $pool && ! -L $pool && $(realpath -e "$pool") == "$pool" ]] || fail 'leaf pool is absent or noncanonical'
[[ $(findmnt -rn -M "$pool" -o FSTYPE) == btrfs && $(findmnt -rn -M "$pool" -o FSROOT) == /@3tched-leaf-pool ]] || fail 'expected mounted @3tched-leaf-pool; refusing a root-directory fallback'
[[ $(stat -c '%u %a' "$pool") == '0 700' ]] || fail 'leaf pool must be root-owned mode 0700'
image=$pool/$name.img
if [[ $action == inspect ]]; then
    [[ -f $image && ! -L $image ]] || fail 'image is absent or not a regular file'
    qemu-img info -f raw --output=json "$image"
    sfdisk --json "$image"
    exit 0
fi
umask 077
receipt_root=$pool/.provisioning
[[ ! -L $receipt_root ]] || fail 'receipt directory is a symlink'
mkdir -p -- "$receipt_root"
[[ $(stat -c '%u %a' "$receipt_root") == '0 700' ]] || fail 'receipt directory must be root-owned mode 0700'
lock=$receipt_root/$name.lock
[[ ! -L $lock && ( ! -e $lock || -f $lock ) ]] || fail 'invalid instance lock'
exec 9>"$lock"
flock -n 9 || fail 'another preparation owns this instance'
receipt=$receipt_root/$name.json
if [[ $action == prepare ]]; then
    for tool in losetup blkid blockdev btrfs mkfs.btrfs mount umount rg sed install; do
        command -v "$tool" >/dev/null || fail "missing $tool"
    done
    [[ $seed_device == /dev/disk/by-uuid/* && -b $seed_device ]] || fail 'stable seed UUID path required'
    [[ -f $image && ! -L $image && -f $receipt && ! -L $receipt ]] || fail 'disk and creation receipt required'
    jq -e --arg image "$image" --arg name "$name" --arg inode "$(stat -c %i "$image")" \
        --argjson bytes "$(stat -c %s "$image")" \
        '.phase=="disk-partitioned" and .image==$image and .name==$name and
         .image_inode==$inode and .image_bytes==$bytes and .identity_registered==false' "$receipt" >/dev/null || fail 'not a fresh partitioned disk; never repeat device-add'
    table=$(sfdisk --json "$image")
    jq -e --argjson actual "$table" '.partition_table==$actual.partitiontable and
        (.partition_table.partitions|length)==2' "$receipt" >/dev/null || fail 'partition table differs from receipt'
    [[ -z $(losetup -j "$image") ]] || fail 'image already attached'
    seed_fsid=$(blkid -p -s UUID -o value "$seed_device")
    [[ $seed_device == /dev/disk/by-uuid/"$seed_fsid" ]] || fail 'seed UUID mismatch'
    btrfs inspect-internal dump-super "$seed_device" | sed -n '/flags/,/fsid/p' | rg -q SEEDING || fail 'seed is not sealed'
    root_mount=$pool/.mounts/$name/root
    upgrade_mount=$pool/.mounts/$name/upgrade
    [[ ! -e $pool/.mounts/$name ]] || fail 'mount directory already exists'
    phase() {
        jq --arg phase "$1" '.phase=$phase' "$receipt" > "$receipt.new"
        mv -T "$receipt.new" "$receipt"
        sync -f "$receipt_root"
    }
    phase filesystem-prepare-intent
    mkdir -p "$root_mount" "$upgrade_mount"
    blockdev --setro "$seed_device"
    seed_loop=$(losetup --find --show --read-only "$seed_device")
    disk_loop=$(losetup --find --show --partscan "$image")
    root_partition=${disk_loop}p1 upgrade_partition=${disk_loop}p2
    jq --arg seed "$seed_device" --arg sf "$seed_fsid" --arg sl "$seed_loop" --arg dl "$disk_loop" \
        --arg rp "$root_partition" --arg up "$upgrade_partition" --arg rm "$root_mount" --arg um "$upgrade_mount" \
        '.seed_device=$seed|.seed_fsid=$sf|.seed_loop=$sl|.disk_loop=$dl|
         .root_partition=$rp|.upgrade_partition=$up|.root_mount=$rm|.upgrade_mount=$um' "$receipt" > "$receipt.new"
    mv -T "$receipt.new" "$receipt"
    for partition in "$root_partition" "$upgrade_partition"; do
        [[ -b $partition && -z $(blkid -p -s TYPE -o value "$partition" || true) ]] || fail 'partition is not blank'
    done
    mount -o ro,subvolid=5 "$seed_loop" "$root_mount"
    for member in @ @agent_memory @agent_rules @agent_shared @agent_skills @agent_soul @agent_user; do
        btrfs subvolume show "$root_mount/$member" >/dev/null || fail "seed missing $member"
    done
    phase root-device-add-intent
    btrfs device add "$root_partition" "$root_mount"
    sync -f "$root_mount"
    umount "$root_mount"
    mount -o "rw,subvolid=5,device=$seed_loop,device=$root_partition" "$root_partition" "$root_mount"
    phase upgrade-format-intent
    mkfs.btrfs -L 3tched-upgrade "$upgrade_partition"
    mount -o rw,subvolid=5 "$upgrade_partition" "$upgrade_mount"
    btrfs subvolume create "$upgrade_mount/@"
    for member in @agent_memory @agent_rules @agent_skills @agent_user; do
        btrfs property set -ts "$root_mount/$member" ro false
    done
    for member in @agent_shared @agent_soul; do
        btrfs property set -ts "$root_mount/$member" ro true
    done
    install -d -m 0700 "$root_mount/@agent_user/persist"
    install -d -m 0755 "$root_mount/@/opt/run-mounts/persist"
    sync -f "$root_mount"
    sync -f "$upgrade_mount"
    jq --arg rf "$(blkid -p -s UUID -o value "$root_partition")" \
        --arg uf "$(blkid -p -s UUID -o value "$upgrade_partition")" \
        '.root_fsid=$rf|.upgrade_fsid=$uf|.filesystem_prepared=true|.phase="filesystems-prepared"' "$receipt" > "$receipt.new"
    mv -T "$receipt.new" "$receipt"
    sync -f "$receipt_root"
    printf 'Filesystems prepared; mounts retained for next manual step. Receipt: %s\n' "$receipt"
    exit 0
fi
[[ ! -e $image && ! -L $image ]] || fail 'image already exists; use inspect, never repeat creation'
receipt=$receipt_root/$name.json
[[ ! -e $receipt && ! -L $receipt ]] || fail 'retained preparation intent exists; inspect before any retry'
total_gib=$((root_gib + upgrade_gib))
created_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
jq -n --arg name "$name" --arg image "$image" --arg pool "$pool" --arg time "$created_at" \
    --argjson root "$root_gib" --argjson upgrade "$upgrade_gib" --argjson total "$total_gib" \
    '{version:1,layout:"raw-gpt-root-upgrade-v1",phase:"disk-create-intent",
      name:$name,image:$image,pool:$pool,created_at:$time,
      root_gib:$root,upgrade_gib_requested:$upgrade,total_gib:$total,
      identity_registered:false,filesystem_prepared:false}' > "$receipt"
# qemu-img creates a sparse raw disk; GPT/alignment overhead reduces partition 2
# slightly. All commands address only the newly created image, never a host disk.
qemu-img create -f raw "$image" "${total_gib}G"
chmod 0600 "$image"
sfdisk "$image" <<PARTITIONS
label: gpt
unit: sectors

size=${root_gib}G, type=L, name="3tched-root"
type=L, name="3tched-upgrade"
PARTITIONS
partition_table=$(sfdisk --json "$image")
# Pin partition identities now; loop numbers are runtime attachments, not owners.
jq --arg inode "$(stat -c %i "$image")" --argjson bytes "$(stat -c %s "$image")" \
    --argjson partition_table "$partition_table" \
    '.phase="disk-partitioned" | .image_inode=$inode | .image_bytes=$bytes |
     .partition_table=$partition_table.partitiontable' "$receipt" > "$receipt.new"
mv -T -- "$receipt.new" "$receipt"
sync -f "$pool"
printf 'Prepared disk: %s\nReceipt: %s\n' "$image" "$receipt"
printf '%s\n' 'No filesystems formatted, container created, seed attached, or Cozo binding committed.'
