#!/usr/bin/env bash
# Offline image preparation; never mounts, unseals or starts a guest/service.
set -euo pipefail
fail() { printf 'prepare-lungs-client: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $(id -u) == 0 ]] || fail 'usage (host root): TOP RECEIVED_RELEASE'
top=$1 release=$2
[[ $top == /* && $(realpath -e "$top") == "$top" && $top != / ]] || fail 'noncanonical image top'
[[ $release == /opt/op-dbus/releases/* && $(realpath -e "$release") == "$release" ]] || fail 'expected received release'
[[ $(btrfs property get "$release" ro) == ro=true ]] || fail 'release must be read-only'
[[ $(findmnt -rn -M "$top" -o FSTYPE) == btrfs ]] || fail 'image top must be an existing Btrfs mount'
case ",$(findmnt -rn -M "$top" -o OPTIONS)," in *,rw,*) ;; *) fail 'image must already be in authorized writable maintenance' ;; esac
guest=$top/@
[[ -d $guest && ! -L $guest ]] || fail 'missing guest root'
client=$release/bin/threetched-fs
catalog=$release/var/lib/opdbus/plugin-blobs
[[ -f $client && -x $client && -d $catalog ]] || fail 'received client or sealed public catalog absent'
for rel in usr usr/local usr/local/bin usr/share usr/share/op-dbus; do
    path=$guest/$rel
    if [[ ! -e $path && ! -L $path ]]; then mkdir -m 0755 "$path"; fi
    [[ -d $path && ! -L $path && $(stat -c %u "$path") == 0 ]] || fail "unsafe image directory: $rel"
done
store=$guest/usr/share/op-dbus/lungs-view
empty=$guest/usr/share/op-dbus/lungs-empty-state
[[ ! -e $store && ! -L $store && ! -e $empty && ! -L $empty ]] || fail 'lungs client view already prepared; inspect existing image'
[[ ! -e $guest/usr/local/bin/threetched-fs && ! -L $guest/usr/local/bin/threetched-fs ]] || fail 'client already exists; inspect before replacing'
install -m 0755 "$client" "$guest/usr/local/bin/threetched-fs"
install -d -m 0755 "$empty" "$store"
# Only schema/catalog material enters the base; live SHM and SID1 never do.
"$client" --store "$store" capture --catalog "$catalog" --state "$empty"
find "$store" -type d -exec chmod 0755 {} +
find "$store" -type f -exec chmod 0644 {} +
[[ -z $(find "$empty" -mindepth 1 -print -quit) ]] || fail 'schema capture populated state unexpectedly'
cmp "$client" "$guest/usr/local/bin/threetched-fs"
jq -n --arg release "${release##*/}" \
    --arg client_sha256 "$(sha256sum "$client" | cut -d ' ' -f1)" \
    --arg catalog_sha256 "$(sha256sum "$catalog/.manifest.json" | cut -d ' ' -f1)" \
    '{release:$release,client_sha256:$client_sha256,catalog_manifest_sha256:$catalog_sha256,
      socket:"/run/opdbus/lungs/lungs.sock",binding:"BindCurrentContainer",state:"empty",
      command:["threetched-fs","--store","/usr/share/op-dbus/lungs-view","mcp","--state",
       "/usr/share/op-dbus/lungs-empty-state","--lungs-socket","/run/opdbus/lungs/lungs.sock","--all"]}' \
    > "$guest/usr/share/op-dbus/lungs-client.json"
chmod 0644 "$guest/usr/share/op-dbus/lungs-client.json"
printf 'Prepared schema-only guest lungs client from received release %s.\n' "${release##*/}"
