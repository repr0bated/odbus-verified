#!/usr/bin/env bash
# Prepare an offline image or deliver to a stopped, UUID-pinned private Incus root.
# Never starts a guest/service, changes storage, or copies identity credentials.
set -euo pipefail
fail() { printf 'prepare-lungs-client: %s\n' "$*" >&2; exit 1; }
usage='usage (host root): TOP READ_ONLY_RELEASE | --container NAME NATIVE_UUID ROOT_SUBVOLUME_UUID OLD_CLIENT_SHA256_OR_absent READ_ONLY_RELEASE'
[[ $(id -u) == 0 ]] || fail "$usage"
mode=image
container= native_uuid= root_uuid= old_client= private_root= stage=
case "${1:-}" in
    --container)
        [[ $# == 6 ]] || fail "$usage"
        mode=container
        container=$2 native_uuid=$3 root_uuid=$4 old_client=$5 release=$6
        [[ $container =~ ^[A-Za-z0-9][A-Za-z0-9-]*$ ]] || fail 'invalid local container name'
        for value in "$native_uuid" "$root_uuid"; do
            [[ $value =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] || fail 'expected canonical UUID pins'
        done
        [[ $old_client == absent || $old_client =~ ^[0-9a-f]{64}$ ]] || fail 'expected old client SHA256 or absent'
        ;;
    *)
        [[ $# == 2 ]] || fail "$usage"
        top=$1 release=$2
        [[ $top == /* && $(realpath -e "$top") == "$top" && $top != / ]] || fail 'noncanonical image top'
        ;;
esac
[[ $release == /* && $(realpath -e "$release") == "$release" ]] || fail 'noncanonical release path'
case "$release" in
    /opt/op-dbus/releases/*) release_kind=received-release ;;
    /opt/op-dbus/release-send/*) release_kind=source-snapshot ;;
    *) fail 'expected retained received release or native release-send snapshot' ;;
esac
parent=$release
while :; do
    [[ $(stat -c %u "$parent") == 0 ]] || fail 'release path is not root-owned'
    permissions=$(stat -c %a "$parent")
    (( (8#$permissions & 022) == 0 )) || fail 'release path is writable by another owner'
    [[ $parent == / ]] && break
    parent=$(dirname -- "$parent")
done
[[ $(btrfs property get "$release" ro) == ro=true ]] || fail 'release must be read-only'
container_metadata() {
    incus --force-local query "/1.0/instances/$container?project=default"
}
metadata_pin() {
    jq -Sc '{name,type,status,status_code,location,profiles,config,expanded_config,devices,expanded_devices}' | sha256sum | cut -d ' ' -f1
}
assert_container() {
    [[ $(container_metadata | metadata_pin) == "$instance_pin" ]] || fail 'container identity/configuration/state changed; leave stopped'
    [[ $(realpath -e "$volume/rootfs") == "$private_root" ]] || fail 'private root changed'
    [[ $(btrfs subvolume show "$volume" | while read -r key value rest; do
        [[ $key != UUID: ]] || printf '%s\n' "$value"
    done) == "$root_uuid" ]] || fail 'private root UUID changed'
    [[ $(btrfs property get "$volume" ro) == ro=false ]] || fail 'private root is not writable'
}
check_private_path() {
    local path=$private_root/$1 permissions
    if [[ -e $path || -L $path ]]; then
        [[ ! -L $path && $(stat -c %u:%g "$path") == 0:0 ]] || fail "unsafe private target: $1"
        permissions=$(stat -c %a "$path")
        (( (8#$permissions & 022) == 0 )) || fail "writable by another owner: $1"
        [[ $(btrfs inspect-internal rootid "$path") == "$root_id" ]] || fail "target leaves private root: $1"
        [[ ! -f $path || $(stat -c %h "$path") == 1 ]] || fail "hard-linked private target: $1"
    fi
}
directories=(usr usr/sbin usr/local usr/local/bin usr/local/libexec usr/local/libexec/3tched
    usr/lib usr/lib/systemd usr/lib/systemd/system usr/lib/tmpfiles.d
    etc etc/systemd etc/systemd/system etc/systemd/system/multi-user.target.wants
    usr/share usr/share/op-dbus)
targets=(usr/share/op-dbus/lungs-view usr/share/op-dbus/threetched-view
    usr/share/op-dbus/lungs-empty-state usr/share/op-dbus/lungs-catalog
    usr/share/op-dbus/lungs-client.json usr/lib/systemd/system/3tchedfs.service
    usr/lib/systemd/system/threetchedfs.service
    etc/systemd/system/multi-user.target.wants/3tchedfs.service
    etc/systemd/system/multi-user.target.wants/threetchedfs.service
    usr/local/libexec/3tched/3tchedfs-mount-preflight usr/lib/tmpfiles.d/3tched-lungs.conf
    usr/sbin/mount.fuse.3tchedFS)
if [[ $mode == image ]]; then
    [[ $(findmnt -rn -M "$top" -o FSTYPE) == btrfs ]] || fail 'image top must be an existing Btrfs mount'
    case ",$(findmnt -rn -M "$top" -o OPTIONS)," in *,rw,*) ;; *) fail 'image must already be in authorized writable maintenance' ;; esac
    guest=$top/@
    [[ -d $guest && ! -L $guest ]] || fail 'missing guest root'
    [[ ! -e $guest/usr/share/op-dbus/image && ! -L $guest/usr/share/op-dbus/image ]] || fail 'base metadata already exists; never mutate a retained seed'
else
    # Native file delivery owns writes. Host access below is inspection only;
    # no chroot, root copy, device change or Incus-owned directory write.
    lock_dir=/run/prepare-lungs-client
    if [[ ! -e $lock_dir && ! -L $lock_dir ]]; then mkdir -m 0700 "$lock_dir"; fi
    [[ -d $lock_dir && ! -L $lock_dir && $(stat -c %u:%g:%a "$lock_dir") == 0:0:700 ]] || fail 'unsafe preparation lock directory'
    exec 9>"$lock_dir/$native_uuid.lock"
    flock -n 9 || fail 'another guest preparation is active'
    metadata=$(container_metadata)
    jq -e --arg name "$container" --arg uuid "$native_uuid" '
        .name == $name and .type == "container" and .status_code == 102 and
        .location == "none" and .config["volatile.uuid"] == $uuid and
        (.config["user.opdbus.session_id"] | type == "string" and length > 0) and
        (.config["user.opdbus.wireguard_public_key"] | type == "string" and length > 0) and
        ((.expanded_config["raw.lxc"] // "") == "") and
        ([.expanded_devices[] | select(.type == "disk" and .path == "/")] | length == 1)
    ' <<<"$metadata" >/dev/null || fail 'expected stopped local identity container with no raw root overrides'
    root_device=$(jq -c '.expanded_devices[] | select(.type == "disk" and .path == "/")' <<<"$metadata")
    pool=$(jq -er '.pool' <<<"$root_device")
    [[ $pool =~ ^[A-Za-z0-9][A-Za-z0-9_-]*$ ]] || fail 'invalid native root pool'
    jq -e '(.source // "") == "" and (.readonly // "false") == "false"' <<<"$root_device" >/dev/null || fail 'root must be the private native volume'
    incus --force-local query "/1.0/storage-pools/$pool" |
        jq -e '.driver == "btrfs"' >/dev/null || fail 'maintenance requires a native Btrfs container root'
    incus --force-local query "/1.0/storage-pools/$pool/volumes/container/$container?project=default" |
        jq -e --arg instance "/1.0/instances/$container" '
            .type == "container" and .content_type == "filesystem" and .used_by == [$instance]
        ' >/dev/null || fail 'native root volume is not exclusive to this container'
    volume=/var/lib/incus/storage-pools/$pool/containers/$container
    [[ $(realpath -e "/var/lib/incus/containers/$container") == "$volume" ]] || fail 'unexpected local Incus root layout'
    private_root=$volume/rootfs
    [[ -d $private_root && ! -L $private_root && $(realpath -e "$private_root") == "$private_root" ]] || fail 'unsafe private root'
    instance_pin=$(metadata_pin <<<"$metadata")
    assert_container
    root_id=$(btrfs inspect-internal rootid "$volume")
    check_private_path .
    [[ $(findmnt -rn -T "$private_root" -o FSTYPE) == btrfs ]] || fail 'private root filesystem is not Btrfs'
    case ",$(findmnt -rn -T "$private_root" -o OPTIONS)," in *,rw,*) ;; *) fail 'private root filesystem is not writable' ;; esac
    while IFS= read -r mounted; do
        case "$mounted" in "$private_root"|"$private_root"/*) fail 'private root has a live mount; leave stopped' ;; esac
    done < <(findmnt -rn -o TARGET)
    # An attached host/custom disk must not cover any installation target.
    jq -e '
        [.expanded_devices[] | select(.type == "disk" and .path != "/") | .path] |
        all(.[]; . as $path |
            ["/usr", "/etc"] | all(.[];
                . as $target | ($path != $target and
                ($path | startswith($target + "/") | not) and
                ($target | startswith($path + "/") | not))))
    ' <<<"$metadata" >/dev/null || fail 'an attached disk overlaps guest installation paths'
    for rel in "${directories[@]}" etc/fstab usr/local/bin/3tched-fs; do
        check_private_path "$rel"
    done
    for rel in "${directories[@]}"; do
        [[ ! -e $private_root/$rel || -d $private_root/$rel ]] || fail "not a private directory: $rel"
    done
    for rel in "${targets[@]}" etc/systemd/system/3tchedfs.service etc/systemd/system/threetchedfs.service \
        etc/systemd/system/3tchedfs.service.d etc/systemd/system/threetchedfs.service.d; do
        [[ ! -e $private_root/$rel && ! -L $private_root/$rel ]] || fail "guest target already exists; inspect before replacing: $rel"
    done
    if [[ $old_client == absent ]]; then
        [[ ! -e $private_root/usr/local/bin/3tched-fs && ! -L $private_root/usr/local/bin/3tched-fs ]] || fail 'unexpected existing client'
    else
        [[ -f $private_root/usr/local/bin/3tched-fs ]] || fail 'pinned old client is absent'
        [[ $(sha256sum "$private_root/usr/local/bin/3tched-fs" | cut -d ' ' -f1) == "$old_client" ]] || fail 'old client digest differs'
    fi
    [[ -f $private_root/etc/fstab ]] || fail 'missing private fstab'
    fstab_pin=$(sha256sum "$private_root/etc/fstab" | cut -d ' ' -f1)
    stage=$(mktemp -d /var/tmp/prepare-lungs-client.XXXXXXXX)
    trap 'rm -rf -- "$stage"' EXIT
    guest=$stage/rootfs
    install -d -m 0755 "$guest" "$guest/etc"
    cp -p -- "$private_root/etc/fstab" "$guest/etc/fstab"
fi
assets=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
client=$release/bin/3tched-fs
catalog=$release/usr/share/op-dbus/plugin-blobs
[[ -f $client && ! -L $client && -x $client && -d $catalog && ! -L $catalog ]] || fail 'release client or sealed public catalog absent'
for asset in 3tchedfs.service threetchedfs.service 3tchedfs-mount-preflight 3tched-lungs.conf mount.fuse.3tchedFS 3tchedfs.fstab; do
    [[ -f $assets/$asset ]] || fail "guest mount asset is absent: $asset"
done
[[ -z $(find "$catalog" -type l -print -quit) ]] || fail 'sealed catalog contains a symlink'
for rel in "${directories[@]}"; do
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
if findmnt --fstab --tab-file "$fstab" -rn -o TARGET | grep -iExq '/run/mounts?/(3tchedfs|threetched)'; then
    fail 'guest fstab already declares a FUSE view; inspect before replacing'
fi
[[ ! -e $store && ! -L $store && ! -e $empty && ! -L $empty && ! -e $guest_catalog && ! -L $guest_catalog ]] || fail 'lungs client view or catalog already prepared; inspect existing image'
[[ ! -e $upstream_store && ! -L $upstream_store ]] || fail 'threetched view already prepared; inspect existing image'
[[ ! -e $guest/usr/share/op-dbus/lungs-client.json && ! -L $guest/usr/share/op-dbus/lungs-client.json ]] || fail 'preparation receipt already exists'
[[ ! -e $guest/usr/local/bin/3tched-fs && ! -L $guest/usr/local/bin/3tched-fs ]] || fail 'client already exists; inspect before replacing'
for path in "$unit" "$unit_link" "$upstream_unit" "$upstream_unit_link" "$preflight" "$socket_link_config" "$mount_helper"; do
    [[ ! -e $path && ! -L $path ]] || fail "guest mount service already exists: $path"
done
install -m 0755 "$client" "$guest/usr/local/bin/3tched-fs"
install -d -m 0755 "$empty" "$store" "$upstream_store"
# Same selection as the host adapter; only declarations are delivered, never
# live state or credentials. Both mounts write through the same lungs endpoint.
upstream_plugins=threetched
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
    --arg delivery "$mode" --arg container "$container" --arg native_uuid "$native_uuid" --arg root_uuid "$root_uuid" \
    --arg old_client_sha256 "$old_client" --arg client_sha256 "$(sha256sum "$client" | cut -d ' ' -f1)" \
    --arg catalog_sha256 "$(sha256sum "$catalog/.manifest.json" | cut -d ' ' -f1)" \
    '{release:$release,release_kind:$release_kind,delivery:$delivery,
      container:(if $delivery == "container" then {name:$container,native_uuid:$native_uuid,root_subvolume_uuid:$root_uuid,previous_client_sha256:$old_client_sha256} else null end),
      client_sha256:$client_sha256,catalog_manifest_sha256:$catalog_sha256,
      socket:"/run/3tched-lungs.socket",binding:"BindCurrentContainer",state:"empty",
      mounts:[{service:"3tchedfs.service",mountpoint:"/run/mounts/3tchedFS",store:"/usr/share/op-dbus/lungs-view"},
              {service:"threetchedfs.service",mountpoint:"/run/mounts/threetched",store:"/usr/share/op-dbus/threetched-view"}],
      mount_table:"/etc/fstab"}' \
    > "$guest/usr/share/op-dbus/lungs-client.json"
chmod 0644 "$guest/usr/share/op-dbus/lungs-client.json"
if [[ $mode == container ]]; then
    assert_container
    [[ $(sha256sum "$private_root/etc/fstab" | cut -d ' ' -f1) == "$fstab_pin" ]] || fail 'guest fstab changed during preparation'
    # All payload generation precedes native writes. On failure leave the
    # container stopped; existing-target guards deliberately prevent blind retry.
    for rel in "${directories[@]}"; do
        if [[ ! -e $private_root/$rel ]]; then
            incus --force-local --project default file create --type directory \
                --mode 0755 --uid 0 --gid 0 "$container/$rel"
        fi
    done
    for rel in lungs-view threetched-view lungs-empty-state lungs-catalog; do
        incus --force-local --project default file push --archive --uid 0 --gid 0 \
            "$guest/usr/share/op-dbus/$rel" "$container/usr/share/op-dbus/"
    done
    for rel in usr/local/bin/3tched-fs usr/local/libexec/3tched/3tchedfs-mount-preflight \
        usr/sbin/mount.fuse.3tchedFS usr/lib/systemd/system/3tchedfs.service \
        usr/lib/systemd/system/threetchedfs.service usr/lib/tmpfiles.d/3tched-lungs.conf etc/fstab; do
        incus --force-local --project default file push --uid 0 --gid 0 \
            --mode "$(stat -c %a "$guest/$rel")" "$guest/$rel" "$container/$rel"
    done
    for service in 3tchedfs threetchedfs; do
        incus --force-local --project default file create --type symlink --uid 0 --gid 0 \
            "$container/etc/systemd/system/multi-user.target.wants/$service.service" \
            "/usr/lib/systemd/system/$service.service"
    done
    assert_container
    while IFS= read -r -d '' path; do
        rel=${path#"$guest/"}
        [[ $rel != usr/share/op-dbus/lungs-client.json ]] || continue
        check_private_path "$rel"
        cmp -- "$path" "$private_root/$rel"
    done < <(find "$guest" -type f -print0)
    for service in 3tchedfs threetchedfs; do
        [[ $(readlink "$private_root/etc/systemd/system/multi-user.target.wants/$service.service") == "/usr/lib/systemd/system/$service.service" ]] || fail 'guest service enablement differs'
    done
    incus --force-local --project default file push --uid 0 --gid 0 --mode 0644 \
        "$guest/usr/share/op-dbus/lungs-client.json" "$container/usr/share/op-dbus/lungs-client.json"
    cmp -- "$guest/usr/share/op-dbus/lungs-client.json" "$private_root/usr/share/op-dbus/lungs-client.json"
    assert_container
fi
printf 'Prepared two guest FUSE mounts using one 3tched-fs binary from %s %s.\n' "$release_kind" "${release##*/}"
