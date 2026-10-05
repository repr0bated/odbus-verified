#!/usr/bin/env bash
# Native config stubs only; no Incus, FUSE mount, processes or guest probes.
set -euo pipefail
umask 077
repo=$(cd "$(dirname "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
seed_fail() { printf '%s\n' "$*" >&2; exit 1; }
seed_secure_path() { [[ ! -L $1 ]] || seed_fail symlink; }
sync() { :; }
for name in seed_verify_lungs_hook seed_select_lungs_hook seed_lungs_disk_expected seed_lungs_disk_validate seed_lungs_profile_expected seed_lungs_profile_validate; do
    eval "$(sed -n "/^$name() {/,/^}/p" "$helper")"
done
seed_dir=$fixture
seed_name=3tchedChatbot
seed_session=b0c777e3-0955-cd8b-3d01-ca12998cae28
native_uuid=00000000-0000-4000-8000-000000000001
OP_IDENTITY_SEED_LUNGS_DIR=/private/session-lungs
native_file=$fixture/native.json
events=$fixture/events
reset() {
    rm -f "$fixture/lungs-disk.json" "$fixture/lungs-hook"
    : > "$events"
    jq -n '{status:"Stopped",config:{},profiles:["3tched-lungs"],devices:{root:{type:"disk",path:"/",pool:"identity-roots"}},expanded_devices:{root:{type:"disk",path:"/",pool:"identity-roots"},"runtime-run":{type:"disk",source:"tmpfs:",path:"/run","initial.mode":"0755"},"lungs-socket-dir":{type:"disk",source:"/run/opdbus/lungs",path:"/run/opdbus/lungs",readonly:"true"}}}' > "$native_file"
    seed_instance
}
seed_instance() { native=$(cat "$native_file"); }
incus() {
    printf '%s\n' "$*" >> "$events"
    [[ $1 == config ]] || exit 91
    if [[ $2 == set ]]; then
        jq --arg value "${4#raw.lxc=}" '.config["raw.lxc"]=$value' "$native_file" > "$fixture/tmp"
    elif [[ $2 == device && $3 == add && $5 == 3tched && $6 == disk ]]; then
        jq --arg source "${7#source=}" '{type:"disk",source:$source,path:"/run/mount/3tchedFS",readonly:"false"} as $d | .devices["3tched"]=$d | .expanded_devices["3tched"]=$d' "$native_file" > "$fixture/tmp"
    else exit 92; fi
    mv "$fixture/tmp" "$native_file"
}
reset
seed_select_lungs_hook
jq -e --arg sid "$seed_session" '.config["raw.lxc"]==("lxc.uts.name = "+$sid) and .devices["3tched"]==.expanded_devices["3tched"] and .devices["3tched"].path=="/run/mount/3tchedFS" and .devices["3tched"].readonly=="false" and (.devices["3tched"]|has("shift")|not)' "$native_file" >/dev/null
[[ ! -e $fixture/lungs-hook && $(stat -c %a "$fixture/lungs-disk.json") == 600 ]]
before=$(sha256sum "$events")
seed_select_lungs_hook
[[ $(sha256sum "$events") == "$before" ]]
# Every conflict must fail before config writes, mount or start.
for failure in source inherited readonly shift ancestor descendant alias raw running; do
    reset
    seed_lungs_disk_expected
    case "$failure" in
        source|readonly|shift|inherited)
            jq --argjson d "$lungs_device" '.devices["3tched"]=$d | .expanded_devices["3tched"]=$d' "$native_file" > "$fixture/tmp"; mv "$fixture/tmp" "$native_file"
            case "$failure" in
                source) filter='.devices["3tched"].source="/foreign" | .expanded_devices["3tched"].source="/foreign"';;
                readonly) filter='.devices["3tched"].readonly="true" | .expanded_devices["3tched"].readonly="true"';;
                shift) filter='.devices["3tched"].shift="true" | .expanded_devices["3tched"].shift="true"';;
                inherited) filter='del(.devices["3tched"])';;
            esac;;
        ancestor) filter='.expanded_devices.other={type:"disk",path:"/run",source:"/foreign"}';;
        descendant) filter='.expanded_devices.other={type:"disk",path:"/run/mount/3tchedFS/private",source:"/foreign"}';;
        alias) filter='.expanded_devices.other={type:"disk",path:"/run/./mount",source:"/foreign"}';;
        raw) filter='.config["raw.lxc"]="lxc.hook.start-host = /foreign"';;
        running) filter='.status="Running"';;
    esac
    jq "$filter" "$native_file" > "$fixture/tmp"; mv "$fixture/tmp" "$native_file"
    seed_instance
    if (seed_select_lungs_hook) >/dev/null 2>&1; then printf 'accepted %s\n' "$failure" >&2; exit 1; fi
    [[ ! -s $events ]]
done
printf 'PASS: native session-scoped lungs disk, SID hostname, idempotence and conflict guards; no live effects\n'
