#!/bin/bash
# One-shot called only by the existing stage-1 release owner; no service loop.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
[[ $(id -u) == 0 ]] || exit 1
read_setting() {
    local f=/etc/op-dbus/env/$1
    [[ -f $f && ! -L $f && $(stat -c %u:%g:%a "$f") == 0:0:600 ]] || exit 1
    tr -d '\n' <"$f"
}
fsid=$(read_setting GOLDEN_FSID)
work_uuid=$(read_setting GOLDEN_WORK_UUID)
upper_uuid=$(read_setting GOLDEN_UPPER_UUID)
[[ $(read_setting GOLDEN_HOST_MOUNT) == /mnt && $(read_setting GOLDEN_WORK) == /mnt/@opdbus-system-a && $(read_setting GOLDEN_ROOT) == /mnt/@upper ]] || exit 1
[[ $(findmnt -rn -M /mnt -o FSTYPE,FSROOT) == 'btrfs /' && $(findmnt -rn -M /mnt -o UUID) == "$fsid" ]] || exit 1
uuid_of() { btrfs subvolume show "$1" | awk '$1=="UUID:" {print $2}'; }
[[ $(realpath -e /mnt/@upper) == /mnt/@upper && $(realpath -e /mnt/@opdbus-system-a) == /mnt/@opdbus-system-a ]] || exit 1
[[ $(uuid_of /mnt/@upper) == "$upper_uuid" && $(uuid_of /mnt/@opdbus-system-a) == "$work_uuid" ]] || exit 1
[[ $(btrfs property get -ts /mnt/@opdbus-system-a ro) == ro=false ]] || exit 1
state=/mnt/@upper/golden-state.json
[[ -f $state && ! -L $state && $(stat -c %u:%g "$state") == 0:0 && $((0$(stat -c %a "$state") & 0022)) == 0 ]] || exit 1
lower=$(jq -er '.live.path' "$state")
[[ $lower == /mnt/@upper/*/os && ${lower#/mnt/@upper/} != */*/* && $(realpath -e "$lower") == "$lower" ]] || exit 1
[[ $(findmnt -rn -T "$lower" -o UUID) == "$fsid" && $(btrfs property get -ts "$lower" ro) == ro=true ]] || exit 1
[[ $(uuid_of "$lower") == "$(jq -er .live.uuid "$state")" && $(btrfs inspect-internal rootid "$lower") == "$(jq -er .live.id "$state")" ]] || exit 1
upper=${lower%/os}/upper
work=${lower%/os}/work
[[ -d $upper && ! -L $upper && -d $work && ! -L $work && $(realpath -e "$upper") == "$upper" && $(realpath -e "$work") == "$work" ]] || exit 1
options="lowerdir=$lower,upperdir=$upper,workdir=$work"
for target in /run/opdbus-os-golden /run/opdbus-os-live; do
    if mountpoint -q "$target"; then
        [[ $(findmnt -rn -M "$target" -o FSTYPE) == overlay && ,$(findmnt -rn -M "$target" -o OPTIONS), == *",$options,"* ]] || exit 1
        continue
    fi
    install -d -m 0755 "$target"
    [[ -z $(find "$target" -mindepth 1 -maxdepth 1 -print -quit) ]] || exit 1
    if [[ $target == /run/opdbus-os-golden ]]; then
        mount -t overlay overlay -o "$options" "$target"
    else
        mount --bind /run/opdbus-os-golden "$target"
    fi
done
printf 'Golden boot selection restored: %s\n' "$lower"
