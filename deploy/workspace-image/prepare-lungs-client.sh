#!/usr/bin/env bash
# Offline image preparation; never mounts, unseals or starts a guest/service.
set -euo pipefail
fail() { printf 'prepare-lungs-client: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $(id -u) == 0 ]] || fail 'usage (host root): TOP READ_ONLY_RELEASE'
top=$1 release=$2
[[ $top == /* && $(realpath -e "$top") == "$top" && $top != / ]] || fail 'noncanonical image top'
[[ $(realpath -e "$release") == "$release" ]] || fail 'noncanonical release path'
case "$release" in
    /opt/op-dbus/releases/*) release_kind=received-release ;;
    /opt/op-dbus/release-send/*) release_kind=source-snapshot ;;
    *) fail 'expected retained received release or native release-send snapshot' ;;
esac
parent=$release
while :; do
    [[ $(stat -c %u "$parent") == 0 ]] || fail 'release path is not root-owned'
    mode=$(stat -c %a "$parent")
    (( (8#$mode & 022) == 0 )) || fail 'release path is writable by another owner'
    [[ $parent == / ]] && break
    parent=$(dirname -- "$parent")
done
[[ $(btrfs property get "$release" ro) == ro=true ]] || fail 'release must be read-only'
[[ $(findmnt -rn -M "$top" -o FSTYPE) == btrfs ]] || fail 'image top must be an existing Btrfs mount'
case ",$(findmnt -rn -M "$top" -o OPTIONS)," in *,rw,*) ;; *) fail 'image must already be in authorized writable maintenance' ;; esac
guest=$top/@
[[ -d $guest && ! -L $guest ]] || fail 'missing guest root'
assets=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
client=$release/bin/3tched-fs
catalog=$release/usr/share/op-dbus/plugin-blobs
[[ -f $client && ! -L $client && -x $client && -d $catalog && ! -L $catalog ]] || fail 'release client or sealed public catalog absent'
for asset in 3tchedfs.service threetchedfs.service 3tchedfs-mount-preflight 3tched-lungs.conf mount.fuse.3tchedFS 3tchedfs.fstab; do
    [[ -f $assets/$asset ]] || fail "guest mount asset is absent: $asset"
done
[[ -z $(find "$catalog" -type l -print -quit) ]] || fail 'sealed catalog contains a symlink'
for rel in usr usr/sbin usr/local usr/local/bin usr/local/libexec usr/local/libexec/3tched usr/lib usr/lib/systemd usr/lib/systemd/system usr/lib/tmpfiles.d etc etc/systemd etc/systemd/system etc/systemd/system/multi-user.target.wants usr/share usr/share/op-dbus; do
    path=$guest/$rel
    if [[ ! -e $path && ! -L $path ]]; then mkdir -m 0755 "$path"; fi
    [[ -d $path && ! -L $path && $(stat -c %u "$path") == 0 ]] || fail "unsafe image directory: $rel"
done
store=$guest/usr/share/op-dbus/lungs-view
upstream_store=$guest/usr/share/op-dbus/threetched-view
empty=$guest/usr/share/op-dbus/lungs-empty-state
guest_catalog=$guest/usr/share/op-dbus/lungs-catalog
unit=$guest/usr/lib/systemd/system/3tchedfs.service
unit_link=$guest/etc/systemd/system/multi-user.target.wants/3tchedfs.service
upstream_unit=$guest/usr/lib/systemd/system/threetchedfs.service
upstream_unit_link=$guest/etc/systemd/system/multi-user.target.wants/threetchedfs.service
preflight=$guest/usr/local/libexec/3tched/3tchedfs-mount-preflight
socket_link_config=$guest/usr/lib/tmpfiles.d/3tched-lungs.conf
mount_helper=$guest/usr/sbin/mount.fuse.3tchedFS
fstab=$guest/etc/fstab
[[ -f $fstab && ! -L $fstab && $(stat -c %u "$fstab") == 0 ]] || fail 'unsafe or absent guest fstab'
if findmnt --fstab --tab-file "$fstab" -rn -o TARGET | grep -iExq '/run/mount/(3tchedfs|threetched)'; then
    fail 'guest fstab already declares a FUSE view; inspect before replacing'
fi
[[ ! -e $store && ! -L $store && ! -e $empty && ! -L $empty && ! -e $guest_catalog && ! -L $guest_catalog ]] || fail 'lungs client view or catalog already prepared; inspect existing image'
[[ ! -e $upstream_store && ! -L $upstream_store ]] || fail 'threetched view already prepared; inspect existing image'
[[ ! -e $guest/usr/local/bin/3tched-fs && ! -L $guest/usr/local/bin/3tched-fs ]] || fail 'client already exists; inspect before replacing'
for path in "$unit" "$unit_link" "$upstream_unit" "$upstream_unit_link" "$preflight" "$socket_link_config" "$mount_helper"; do
    [[ ! -e $path && ! -L $path ]] || fail "guest mount service already exists: $path"
done
install -m 0755 "$client" "$guest/usr/local/bin/3tched-fs"
install -d -m 0755 "$empty" "$store" "$upstream_store"
# Same selection as the host adapter; only declarations enter the seed, never
# live state or credentials. Both mounts write through the same lungs endpoint.
upstream_plugins=threetched,tched_router
"$client" --store "$store" capture --schema-only --catalog "$catalog" --state "$empty" \
    --exclude-plugin "$upstream_plugins"
"$client" --store "$upstream_store" capture --schema-only --catalog "$catalog" --state "$empty" \
    --include-plugin "$upstream_plugins"
cp -a -- "$catalog" "$guest_catalog"
find "$store" "$upstream_store" -type d -exec chmod 0755 {} +
find "$store" "$upstream_store" -type f -exec chmod 0644 {} +
find "$guest_catalog" -type d -exec chmod 0755 {} +
find "$guest_catalog" -type f -exec chmod 0644 {} +
[[ -z $(find "$empty" -mindepth 1 -print -quit) ]] || fail 'schema capture populated state unexpectedly'
cmp "$client" "$guest/usr/local/bin/3tched-fs"
cmp "$catalog/.manifest.json" "$guest_catalog/.manifest.json"
install -m 0755 "$assets/3tchedfs-mount-preflight" "$preflight"
install -m 0644 "$assets/3tchedfs.service" "$unit"
install -m 0644 "$assets/threetchedfs.service" "$upstream_unit"
install -m 0644 "$assets/3tched-lungs.conf" "$socket_link_config"
install -m 0755 "$assets/mount.fuse.3tchedFS" "$mount_helper"
printf '\n' >> "$fstab"
cat "$assets/3tchedfs.fstab" >> "$fstab"
ln -s /usr/lib/systemd/system/3tchedfs.service "$unit_link"
ln -s /usr/lib/systemd/system/threetchedfs.service "$upstream_unit_link"
jq -n --arg release "${release##*/}" --arg release_kind "$release_kind" \
    --arg client_sha256 "$(sha256sum "$client" | cut -d ' ' -f1)" \
    --arg catalog_sha256 "$(sha256sum "$catalog/.manifest.json" | cut -d ' ' -f1)" \
    '{release:$release,release_kind:$release_kind,client_sha256:$client_sha256,catalog_manifest_sha256:$catalog_sha256,
      socket:"/run/3tched-lungs.socket",binding:"BindCurrentContainer",state:"empty",
      mounts:[{service:"3tchedfs.service",mountpoint:"/run/mount/3tchedfs",store:"/usr/share/op-dbus/lungs-view"},
              {service:"threetchedfs.service",mountpoint:"/run/mount/threetched",store:"/usr/share/op-dbus/threetched-view"}],
      mount_table:"/etc/fstab",
      command:["3tched-fs","--store","/usr/share/op-dbus/lungs-view","mcp","--state",
       "/usr/share/op-dbus/lungs-empty-state","--lungs-socket","/run/3tched-lungs.socket","--all"]}' \
    > "$guest/usr/share/op-dbus/lungs-client.json"
chmod 0644 "$guest/usr/share/op-dbus/lungs-client.json"
printf 'Prepared two guest FUSE mounts using one 3tched-fs binary from %s %s.\n' "$release_kind" "${release##*/}"
