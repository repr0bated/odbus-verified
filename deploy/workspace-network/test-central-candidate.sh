#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d /tmp/workspace-central-candidate.XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
uuid=11111111-1111-4111-8111-111111111111
printf '%s\n' "$uuid" > "$fixture/uuid"
chmod 600 "$fixture/uuid"
# Synthetic credential fields exercise preservation; none are live credentials.
jq -n --arg id "$uuid" '{log:{loglevel:"warning"},
    inbounds:[{tag:"original",listen:"127.0.0.1",port:18444,protocol:"socks",settings:{auth:"noauth"}}],
    outbounds:[{tag:"od106-reality-egress",protocol:"vless",settings:{vnext:[{address:"127.0.0.1",port:19444,users:[{id:$id,encryption:"none"}]}]},streamSettings:{network:"tcp",security:"none"}}],
    routing:{domainStrategy:"AsIs",rules:[{type:"field",inboundTag:["original"],outboundTag:"od106-reality-egress"}]}}
' > "$fixture/input.json"
bash "$here/prepare-central-xray.sh" "$fixture/input.json" "$fixture/uuid" "$fixture/vless.sock" "$fixture/candidate.json"
[[ $(stat -c %a "$fixture/candidate.json") == 600 ]]
jq '.inbounds |= .[:-1] | .routing.rules |= .[:-1]' "$fixture/candidate.json" > "$fixture/restored.json"
diff <(jq -S . "$fixture/input.json") <(jq -S . "$fixture/restored.json")
"${XRAY_BINARY:-/usr/local/bin/xray}" run -test -config "$fixture/candidate.json"
reject() {
    if bash "$here/prepare-central-xray.sh" "$fixture/invalid.json" "$fixture/uuid" "$fixture/vless.sock" "$fixture/rejected.json" 2>/dev/null; then
        echo 'invalid candidate accepted' >&2; exit 1
    fi
    [[ ! -e $fixture/rejected.json ]]
}
jq '.outbounds[0].tag="missing"' "$fixture/input.json" > "$fixture/invalid.json"; reject
jq '.inbounds[0].tag="workspace-egress"' "$fixture/input.json" > "$fixture/invalid.json"; reject
jq --arg path "$fixture/vless.sock" '.inbounds[0].listen=$path' "$fixture/input.json" > "$fixture/invalid.json"; reject
jq 'del(.routing.rules[0].inboundTag)' "$fixture/input.json" > "$fixture/invalid.json"; reject
if bash "$here/prepare-central-xray.sh" "$fixture/input.json" "$fixture/uuid" "$fixture/vless.sock" "$fixture/candidate.json" 2>/dev/null; then
    echo 'existing output overwritten' >&2; exit 1
fi
printf 'PASS: central candidate preserves all existing fields; collision/missing upstream/shadowing/output guards\n'
