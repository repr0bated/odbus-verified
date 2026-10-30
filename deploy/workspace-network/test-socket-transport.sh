#!/usr/bin/env bash
# Disposable rootless proof. No Incus, live configuration, or external traffic.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
xray=${XRAY_BINARY:-/usr/local/bin/xray}
relay=${UDS_RELAY_BINARY:-/usr/local/libexec/3tched/op-uds-relay}
fixture=$(mktemp -d /tmp/workspace-xray-fixture.XXXXXX)
pids=()
cleanup() {
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
    for pid in "${pids[@]}"; do wait "$pid" 2>/dev/null || true; done
    rm -rf -- "$fixture"
}
trap cleanup EXIT
rustc --edition=2021 "$here/fixture-probe.rs" -o "$fixture/probe"
# Public fixture identifier, never an identity/session credential.
uuid=11111111-1111-4111-8111-111111111111
jq --arg id "$uuid" '.outbounds[0].settings.vnext[0].users[0].id=$id' "$here/guest-xray.json" > "$fixture/guest.json"
jq --arg id "$uuid" --arg socket "$fixture/vless.sock" '
    .inbounds[0].settings.clients[0].id=$id | .inbounds[0].listen=$socket |
    .outbounds[0]={tag:"approved-upstream",protocol:"freedom",settings:{}} |
    .routing.rules=[{type:"field",ip:["127.0.0.1/32"],outboundTag:"approved-upstream"}] |
    .outbounds += [{tag:"block",protocol:"blackhole",settings:{}}] |
    .routing.rules += [{type:"field",network:"tcp,udp",outboundTag:"block"}]
' "$here/host-xray.json" > "$fixture/host.json"
"$xray" run -test -config "$fixture/host.json"
"$xray" run -test -config "$fixture/guest.json"
"$fixture/probe" echo "$fixture/ports.json" > "$fixture/echo.log" 2>&1 & pids+=("$!")
"$xray" run -config "$fixture/host.json" > "$fixture/host.log" 2>&1 & pids+=("$!")
for attempt in {1..50}; do
    [[ -S $fixture/vless.sock && -f $fixture/ports.json ]] && break
    sleep .1
done
[[ -S $fixture/vless.sock && -f $fixture/ports.json ]]
tcp=$(jq -er .tcp "$fixture/ports.json") udp=$(jq -er .udp "$fixture/ports.json")
export fixture xray relay tcp udp
unshare --user --map-root-user --net bash -euo pipefail <<'GUEST'
ip link set lo up
[[ $(ip -o link | wc -l) == 1 ]]
[[ -z $(ip route show) ]]
echo 'PASS isolation: guest namespace contains only lo and no route'
"$relay" --tcp-to-unix "127.0.0.1:19443=$fixture/vless.sock" > "$fixture/relay.log" 2>&1 & relay_pid=$!
"$xray" run -config "$fixture/guest.json" > "$fixture/guest.log" 2>&1 & xray_pid=$!
trap 'kill "$relay_pid" "$xray_pid" 2>/dev/null || true; wait "$relay_pid" "$xray_pid" 2>/dev/null || true' EXIT
for attempt in {1..50}; do
    if ss -lnt | grep -q '127.0.0.1:1080 ' && ss -lnt | grep -q '127.0.0.1:19443 '; then break; fi
    sleep .1
done
"$fixture/probe" check 1080 "$tcp" "$udp"
GUEST
