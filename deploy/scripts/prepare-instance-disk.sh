#!/usr/bin/env bash
# Completed manual provisioning step: one sparse disk, root + upgrade partitions.
# This prepares storage only. It does not register identity, format a filesystem,
# attach loops, add Btrfs members, populate a root, or start an Incus container.
set -euo pipefail
fail() { printf 'prepare-instance-disk: %s\n' "$*" >&2; exit 1; }
usage() {
    cat <<'USAGE'
Usage:
  prepare-instance-disk create NAME [--root-gib N] [--upgrade-gib N]
  prepare-instance-disk inspect NAME

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
[[ $action == create || $action == inspect ]] || fail 'action must be create or inspect'
[[ $name =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || fail 'invalid instance name'
root_gib=20 upgrade_gib=40
while [[ $# -gt 0 ]]; do
    [[ $action == create ]] || fail 'inspect takes no size options'
    [[ $# -ge 2 ]] || fail "missing value for $1"
    case $1 in
        --root-gib) root_gib=$2 ;;
        --upgrade-gib) upgrade_gib=$2 ;;
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
