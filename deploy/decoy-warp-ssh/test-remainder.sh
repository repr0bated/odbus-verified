#!/bin/bash
# Native disposable namespace; never starts the official client.
set -euo pipefail
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" = 0 ]; then exec sudo -n unshare --net "$0" --inside; fi
[ "$1" = --inside ] || exit 1
fixture=$(mktemp -d /tmp/warp-remainder-fixture.XXXXXX)
export OP_WARP_SSH_CONFIG=$fixture/config.json OP_WARP_SSH_STATE_DIR=$fixture/state
mkdir "$fixture/bin"
cat > "$fixture/bin/ip" <<'EOF'
#!/bin/bash
set -euo pipefail
if [ "${FAIL_V6_DEFAULT:-}" = 1 ] && [[ "$*" == '-6 route add unreachable '* ]]; then exit 77; fi
if [ "${FAIL_V6_RULE:-}" = 1 ] && [[ "$*" == '-6 rule add priority 200 '* ]]; then exit 78; fi
if [ "${FAIL_CANCEL:-}" = 1 ] && [[ "$*" == '-6 rule add priority 200 '* ]]; then kill -TERM "$PPID"; exit 143; fi
if [ "${FAIL_V6_ENABLE:-}" = 1 ] && [[ "$*" == '-6 route add default dev '* ]]; then exit 79; fi
if [[ "$*" == *'-j route show '* ]]; then
 /usr/bin/ip "$@" | jq 'map(if .type == "unreachable" then .type="7" else . end)'
else exec /usr/bin/ip "$@"; fi
EOF
chmod 755 "$fixture/bin/ip"
export PATH="$fixture/bin:$PATH"
cat > "$OP_WARP_SSH_CONFIG" <<'EOF'
{"role":"decoy","warp_interface":"warptest","remainder_table":28053,"remainder_priority":200,"remainder_mtu":1300}
EOF
ip link set lo up
ip link add phytest type dummy
ip link set phytest up
ip addr add 192.0.2.1/24 dev phytest
ip -6 addr add 2001:db8:1::1/64 dev phytest nodad
ip route add default dev phytest
ip -6 route add default dev phytest
ip -6 route add fe80::/10 dev phytest
ip -6 route add ff00::/8 dev phytest
for family in 4 6; do
 ip -N -"$family" -j rule show > "$fixture/rules-before-$family.json"
 ip -N -"$family" -j route show table main > "$fixture/main-before-$family.json"
done
helper=$script_dir/remainder.sh
"$helper" prepare
"$helper" prepare
for host in 1.1.1.1 188.68.58.237; do
 if ip -4 route get "$host" uid 995 ipproto tcp dport 2223 > "$fixture/absent4.log" 2>&1; then echo 'IPv4 fell through' >&2; exit 1; fi
done
if ip -6 route get 2606:4700:4700::1111 > "$fixture/absent6.log" 2>&1; then echo 'IPv6 fell through' >&2; exit 1; fi
for host in fe80::2 ff02::1; do ip -6 -j route get "$host" | jq -e '.[0].dev == "phytest"' >/dev/null; done
ip link add warptest type dummy
ip link set warptest up
ip addr add 198.51.100.1/32 dev warptest
ip -6 addr add 2001:db8:2::1/128 dev warptest nodad
"$helper" enable
"$helper" enable
ip -4 -j route get 188.68.58.237 uid 995 ipproto tcp dport 2223 | jq -e '.[0].dev == "warptest"' >/dev/null
ip -6 -j route get 2606:4700:4700::1111 | jq -e '.[0].dev == "warptest"' >/dev/null
nft add table inet fixture
nft 'add chain inet fixture output { type filter hook output priority 0; policy accept; }'
nft 'add rule inet fixture output meta skuid 995 ip daddr 188.68.58.237 tcp dport 2223 oifname "warptest" counter drop comment "remainder-warp"'
timeout 1 setpriv --reuid 995 --regid 995 --clear-groups bash -c ': > /dev/tcp/188.68.58.237/2223' > "$fixture/packet.log" 2>&1 || true
nft -j list table inet fixture | jq -e 'any(.nftables[].rule?;.comment == "remainder-warp" and any(.expr[];.counter.packets? > 0))' >/dev/null
ip rule add priority 200 to 203.0.113.5 lookup main
if "$helper" rollback > "$fixture/foreign.log" 2>&1; then echo 'foreign rule erased' >&2; exit 1; fi
ip rule del priority 200 to 203.0.113.5 lookup main
ip link del warptest
"$helper" check
if ip -4 route get 1.1.1.1 > "$fixture/deleted4.log" 2>&1; then exit 1; fi
if ip -6 route get 2606:4700:4700::1111 > "$fixture/deleted6.log" 2>&1; then exit 1; fi
ip link add warptest type dummy
ip link set warptest up
ip addr add 198.51.100.2/32 dev warptest
ip -6 addr add 2001:db8:2::2/128 dev warptest nodad
"$helper" enable
"$helper" enable
ip -4 -j route get 1.1.1.1 | jq -e '.[0].dev == "warptest"' >/dev/null
ip -6 -j route get 2606:4700:4700::1111 | jq -e '.[0].dev == "warptest"' >/dev/null
ip link del warptest
"$helper" rollback
for failure in FAIL_V6_DEFAULT FAIL_V6_RULE FAIL_CANCEL; do
 if env "$failure=1" "$helper" prepare > "$fixture/$failure.log" 2>&1; then exit 1; fi
 jq -e '.incomplete == true' "$OP_WARP_SSH_STATE_DIR/remainder/receipt.json" >/dev/null
 "$helper" rollback
done
"$helper" prepare
ip link add warptest type dummy
ip link set warptest up
ip addr add 198.51.100.1/32 dev warptest
ip -6 addr add 2001:db8:2::1/128 dev warptest nodad
if FAIL_V6_ENABLE=1 "$helper" enable > "$fixture/enable-failure.log" 2>&1; then exit 1; fi
jq -e '.incomplete == true' "$OP_WARP_SSH_STATE_DIR/remainder/receipt.json" >/dev/null
"$helper" rollback
# IPv4-only WARP must leave IPv6 Internet terminally unreachable.
ip -6 addr del 2001:db8:2::1/128 dev warptest
"$helper" prepare
"$helper" enable
if ip -6 route get 2606:4700:4700::1111 > "$fixture/v4-only.log" 2>&1; then exit 1; fi
"$helper" rollback
ip link del warptest
for family in 4 6; do
 ip -N -"$family" -j rule show > "$fixture/rules-after-$family.json"
 ip -N -"$family" -j route show table main > "$fixture/main-after-$family.json"
 cmp "$fixture/rules-before-$family.json" "$fixture/rules-after-$family.json"
 cmp "$fixture/main-before-$family.json" "$fixture/main-after-$family.json"
 ip -N -"$family" -j route show table all | jq -e 'all(.[];(.table|tostring) != "28053")' >/dev/null
done
# Routed namespace-data topology: test real next-hop packets, source selection,
# and loss/recreation, without mounting or modifying host /run/netns.
jq '.warp_interface="warp-data" | .warp_gateway4="192.168.252.2" | .warp_source4="192.168.252.1" | .warp_gateway6="fd42:7761:7270:1::2" | .warp_source6="fd42:7761:7270:1::1"' "$OP_WARP_SSH_CONFIG" > "$fixture/gateway-config.json"
mv "$fixture/gateway-config.json" "$OP_WARP_SSH_CONFIG"
unshare --net sh -c 'touch "$1"; exec sleep 30' sh "$fixture/gateway-ready" & gateway_pid=$!
trap 'kill "$gateway_pid" 2>/dev/null || true' EXIT
for ((attempt=0;attempt<40;attempt++)); do [ ! -e "$fixture/gateway-ready" ] || break; sleep 0.05; done
[ -e "$fixture/gateway-ready" ]
gateway_ip() { nsenter -t "$gateway_pid" -n ip "$@"; }
gateway_ip link set lo up
gateway_ip addr add 203.0.113.77/32 dev lo
gateway_ip -6 addr add 2001:db8:55::77/128 dev lo nodad
gateway_link() {
 ip link add warp-data type veth peer name data0
 ip link set data0 netns "$gateway_pid"
 ip link set warp-data mtu 1300
 gateway_ip link set data0 mtu 1300
 ip addr add 192.168.252.1/30 dev warp-data
 ip -6 addr add fd42:7761:7270:1::1/126 dev warp-data nodad
 gateway_ip addr add 192.168.252.2/30 dev data0
 gateway_ip -6 addr add fd42:7761:7270:1::2/126 dev data0 nodad
 ip link set warp-data up
 gateway_ip link set data0 up
}
gateway_link
"$helper" prepare
if ip route get 203.0.113.77 > "$fixture/gateway-before.log" 2>&1; then echo 'gateway prepare fell through' >&2; exit 1; fi
"$helper" enable
ip -4 -j route get 203.0.113.77 uid 995 ipproto tcp dport 2223 | jq -e '.[0].dev == "warp-data" and .[0].gateway == "192.168.252.2" and .[0].prefsrc == "192.168.252.1"' >/dev/null
ip -6 -j route get 2001:db8:55::77 | jq -e '.[0].dev == "warp-data" and .[0].gateway == "fd42:7761:7270:1::2" and .[0].prefsrc == "fd42:7761:7270:1::1"' >/dev/null
ping -c 1 -W 1 203.0.113.77 > "$fixture/gateway4-packet.log"
ping -6 -c 1 -W 1 2001:db8:55::77 > "$fixture/gateway6-packet.log"
ip link del warp-data
"$helper" check
if ip route get 203.0.113.77 > "$fixture/gateway-deleted4.log" 2>&1; then exit 1; fi
if ip -6 route get 2001:db8:55::77 > "$fixture/gateway-deleted6.log" 2>&1; then exit 1; fi
gateway_link
"$helper" enable
ping -c 1 -W 1 203.0.113.77 > "$fixture/gateway4-recreated.log"
ping -6 -c 1 -W 1 2001:db8:55::77 > "$fixture/gateway6-recreated.log"
"$helper" rollback
ip link del warp-data
kill "$gateway_pid"
wait "$gateway_pid" || true
trap - EXIT
for family in 4 6; do
 ip -N -"$family" -j rule show > "$fixture/gateway-rules-after-$family.json"
 ip -N -"$family" -j route show table main > "$fixture/gateway-main-after-$family.json"
 cmp "$fixture/rules-before-$family.json" "$fixture/gateway-rules-after-$family.json"
 cmp "$fixture/main-before-$family.json" "$fixture/gateway-main-after-$family.json"
 ip -N -"$family" -j route show table all | jq -e 'all(.[];(.table|tostring) != "28053")' >/dev/null
done
printf 'PASS: dual-stack terminal defaults, ND exceptions, UID995 SSH packet counter, numeric7, interface-loss failclosed, foreign refusal, partial prepare/enable and SIGTERM rollback, IPv4-only carrier, routed-veth gateways/source selection/recreation; evidence %s\n' "$fixture"
