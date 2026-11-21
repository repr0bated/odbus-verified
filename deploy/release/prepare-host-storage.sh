#!/bin/sh
# Prepare native Btrfs storage for current/bumped system releases.
# The kit and writable controller state stay on the control rootfs.
# Never format a device, import old system slots, or activate an overlay here.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
fail() { printf 'prepare-host-storage: %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail 'run as root'
[ "$#" = 2 ] && [ "$1" = --uuid ] || fail 'usage: prepare-host-storage.sh --uuid BTRFS-UUID'
uuid=$2
printf '%s\n' "$uuid" | grep -Eq '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' || fail 'canonical filesystem UUID required'
[ "$(blkid -U "$uuid" | wc -l)" = 1 ] || fail 'filesystem UUID must resolve uniquely'
device=$(blkid -U "$uuid")
[ "$(blkid -s TYPE -o value "$device")" = btrfs ] || fail 'selected filesystem is not Btrfs'
state=/var/lib/opdbus-release
root=/opt/op-dbus
secure_directory() {
    [ -d "$1" ] && [ ! -L "$1" ] && [ "$(stat -c %u:%g "$1")" = 0:0 ] &&
        [ "$((0$(stat -c %a "$1") & 07022))" = 0 ] || fail "insecure directory: $1"
}
for path in / /var /var/lib /opt /etc /run; do secure_directory "$path"; done
[ -e "$state" ] || install -d -m 0755 "$state"
secure_directory "$state"
[ "$(stat -c %d "$state")" = "$(stat -c %d /)" ] || fail 'controller state must stay on rootfs'
create_lock() {
    path=$1
    if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        (set -C; : > "$path") 2>/dev/null || [ -f "$path" ] || fail "cannot create $path"
    fi
    [ -f "$path" ] && [ ! -L "$path" ] && [ "$(stat -c %u:%g:%a:%h "$path")" = 0:0:600:1 ] || fail "insecure lock: $path"
}
create_lock "$state/kit.lock"
exec 8<"$state/kit.lock"
flock -n -x 8 || fail 'release kit is in use'
create_lock "$state/lock"
exec 9<"$state/lock"
flock -n -x 9 || fail 'release transaction is active'
[ ! -e "$state/slots.json" ] || fail 'existing release state requires an explicit storage migration'
[ ! -e "$state/storage-preparation.json" ] && [ ! -L "$state/storage-preparation.json" ] || fail 'previous preparation receipt exists; reconcile its exact recorded subvolumes before retrying' 
# Do not reinterpret an existing authority or replace a previous storage binding.
[ ! -e "$state/storage.json" ] && [ ! -L "$state/storage.json" ] || fail 'storage is already configured; inspect its binding instead of replacing it'
[ ! -L "$root" ] || fail 'release storage mountpoint is a symlink'
if [ ! -e "$root" ]; then install -d -m 0755 "$root"; fi
secure_directory "$root"
mountpoint -q "$root" && fail 'release storage mountpoint is already mounted'
[ -z "$(find "$root" -mindepth 1 -maxdepth 1 -print -quit)" ] || fail 'release storage mountpoint is not empty'
[ -f /etc/fstab ] && [ ! -L /etc/fstab ] && [ "$(stat -c %u:%g:%h /etc/fstab)" = 0:0:1 ] || fail 'insecure fstab'
[ "$((0$(stat -c %a /etc/fstab) & 022))" = 0 ] || fail 'fstab is writable by non-root'
awk '$1 !~ /^#/ && $2 == "/opt/op-dbus" {found=1} END {exit found ? 1 : 0}' /etc/fstab || fail 'fstab already declares release storage'
top=$(mktemp -d /run/opdbus-storage.XXXXXX)
cleanup() { mountpoint -q "$top" && umount "$top"; rmdir "$top"; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
mount -t btrfs -o rw,noatime,nodev,nosuid,noexec,subvolid=5 "UUID=$uuid" "$top"
[ "$(findmnt -n -o UUID --target "$top")" = "$uuid" ] || fail 'mounted filesystem UUID differs'
# Dedicated namespace preserves every pre-existing root, seed and release.
[ ! -e "$top/@opdbus-host" ] && [ ! -L "$top/@opdbus-host" ] || fail '@opdbus-host already exists; reconcile the previous preparation'
for name in @opdbus-system-a @opdbus-system-b; do
    [ ! -e "$top/$name" ] && [ ! -L "$top/$name" ] || fail "preexisting unregistered system slot: $name"
done
# Durable step receipt: interrupted preparation is reconciled from recorded IDs;
# never automatically delete or adopt a tree after a failed attempt.
jq -n --arg fsid "$uuid" '{fsid:$fsid,phase:"before-create",subvolumes:{},status:"preparing"}' > "$state/storage-preparation.json"
chmod 0600 "$state/storage-preparation.json"
sync -f "$state"
record() {
    phase=$1 name=${2:-} id=${3:-0}
    temporary=$(mktemp "$state/.storage-preparation.XXXXXX")
    jq --arg phase "$phase" --arg name "$name" --argjson id "$id" \
        '.phase=$phase | if $phase == "complete" then .status="complete" else . end | if $name != "" then .subvolumes[$name]=$id else . end' \
        "$state/storage-preparation.json" > "$temporary"
    mv -T "$temporary" "$state/storage-preparation.json"
    sync -f "$state"
}
btrfs subvolume create "$top/@opdbus-host"
chmod 0755 "$top/@opdbus-host"
parent_id=$(btrfs inspect-internal rootid "$top/@opdbus-host")
record preparation-created @opdbus-host "$parent_id"
record before-golden
btrfs subvolume create "$top/@opdbus-host/golden"
record golden-created golden "$(btrfs inspect-internal rootid "$top/@opdbus-host/golden")"
chmod 0755 "$top/@opdbus-host/golden"
record before-overlay-base
btrfs subvolume create "$top/@opdbus-host/overlay-base"
record overlay-base-created overlay-base "$(btrfs inspect-internal rootid "$top/@opdbus-host/overlay-base")"
chmod 0755 "$top/@opdbus-host/overlay-base"
btrfs property set -ts "$top/@opdbus-host/overlay-base" ro true
install -d -m 0700 "$top/@opdbus-host/release-send"
mount -t btrfs -o rw,noatime,nodev,nosuid,subvolid="$parent_id" "UUID=$uuid" "$root"
[ "$(btrfs inspect-internal rootid "$root")" = "$parent_id" ] || fail 'mounted preparation subvolume differs'
[ "$(findmnt -n -o UUID --target "$root")" = "$uuid" ] || fail 'mounted release filesystem differs'
config=$(mktemp "$state/.storage.XXXXXX")
jq -n --arg fsid "$uuid" --argjson parent "$parent_id" \
    '{fsid:$fsid,parent_subvolume:$parent,slot_a:"@opdbus-system-a",slot_b:"@opdbus-system-b"}' > "$config"
chmod 0600 "$config"
mv -T "$config" "$state/storage.json"
record binding-published
backup=$(mktemp "$state/fstab-before-storage.XXXXXX")
cp -p /etc/fstab "$backup"
record fstab-backup-ready
fstab=$(mktemp /etc/.fstab-opdbus.XXXXXX)
cat /etc/fstab > "$fstab"
printf '\n# Native Btrfs backing for current/bumped system release layers.\nUUID=%s /opt/op-dbus btrfs defaults,noatime,nodev,nosuid,subvolid=%s 0 0\n' "$uuid" "$parent_id" >> "$fstab"
chmod --reference=/etc/fstab "$fstab"
mv -T "$fstab" /etc/fstab
sync -f "$root"
sync -f "$state"
sync -f /etc
record complete
printf 'Native Btrfs preparation mounted at %s (subvolume %s).\n' "$root" "$parent_id"
printf 'System slots: @opdbus-system-a and @opdbus-system-b (created only by release transfer).\n'
printf 'No release activated; existing system/seed/identity subvolumes preserved. Fstab backup: %s\n' "$backup"
