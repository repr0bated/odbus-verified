#!/usr/bin/env bash
# Offline LXD responses for provisioning development. Uses fixture files only.
set -euo pipefail

[[ ${1:-} == lxd ]] || { echo 'mock-lxd: expected the lxd plugin' >&2; exit 2; }
method=${2:?method required}
args=${3:-'{}'}
fixture=${LXD_MOCK_FIXTURE:?set LXD_MOCK_FIXTURE to a disposable fixture directory}
operation=00000000-0000-4000-8000-000000000001
[[ -d $fixture && -f $fixture/image.json ]] || {
    echo 'mock-lxd: fixture must contain image.json' >&2
    exit 2
}

case "$method" in
    get_image)
        result=$(jq -ec --argjson args "$args" '
            select(.fingerprint == $args.fingerprint) | {result:.}
        ' "$fixture/image.json")
        ;;
    create_instance)
        [[ ${4:-} == --confirm-side-effects && ! -e $fixture/instance.json ]] || exit 2
        fingerprint=$(jq -er .fingerprint "$fixture/image.json")
        instance=$(jq -ec --arg fp "$fingerprint" '
            .instance
            | select(.start == false and .profiles == [] and .source.fingerprint == $fp)
            | select(.config["user.opdbus.admission"] == "pending"
                and .config["security.protection.start"] == "true")
            | select([.devices[].name] == ["root"])
            | .status = "Stopped"
            | .config["volatile.base_image"] = $fp
            | .expanded_config = .config
            | .expanded_devices = .devices
        ' <<<"$args")
        (set -o noclobber; printf '%s\n' "$instance" >"$fixture/instance.json")
        result=$(jq -nc --arg id "$operation" '{operation:{id:$id,status_code:103}}')
        ;;
    get_operation_wait)
        [[ -f $fixture/instance.json && $(jq -r .uuid <<<"$args") == "$operation" ]] || exit 2
        result=$(jq -nc --arg id "$operation" '{result:{id:$id,status_code:200}}')
        ;;
    get_instance)
        result=$(jq -ec --argjson args "$args" '
            select(.name == $args.name) | {result:.}
        ' "$fixture/instance.json")
        ;;
    *) echo "mock-lxd: unsupported operation $method" >&2; exit 2 ;;
esac

jq -nc --arg method "$method" --argjson result "$result" '
    {mock:true,plugin_id:"lxd",method:$method,result:{success:true,result:$result}}
'
