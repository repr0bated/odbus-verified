#!/bin/bash
# Private mount AND network namespaces keep /run/netns and all packets isolated.
set -euo pipefail
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" = 0 ]; then exec sudo -n unshare --mount --net "$0" --inside; fi
[ "$1" = --inside ] || exit 1
mount --make-rprivate /
mount -t tmpfs tmpfs /run
mkdir /run/netns
fixture=$(mktemp -d /tmp/warp-namespace-fixture.XXXXXX)
export OP_WARP_SSH_CONFIG=$fixture/config.json OP_WARP_SSH_STATE_DIR=$fixture/state
cat > "$OP_WARP_SSH_CONFIG" <<'EOF'
{"role":"decoy","namespace":"op-warp","egress":{"physical_interface":"phytest"}}
EOF
ip link set lo up
ip link add phytest type dummy
ip link set phytest up
ip addr add 192.0.2.1/24 dev phytest
ip route add default dev phytest
sysctl -q -w net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1
ip -N -4 -j rule show > "$fixture/rules4-before.json"
ip -N -6 -j rule show > "$fixture/rules6-before.json"
helper=$script_dir/namespace.sh
"$helper" prepare
"$helper" prepare
"$helper" check
# Root's remainder and carrier return exceptions; owned by other release helper.
ip route add default via 192.168.252.2 dev warp-data src 192.168.252.1 table 28053
ip -6 route add default via fd42:7761:7270:1::2 dev warp-data src fd42:7761:7270:1::1 table 28053
ip rule add priority 100 to 192.168.253.0/30 lookup main
ip rule add priority 200 lookup 28053
ip -6 rule add priority 200 lookup 28053
# No tunnel: data must never use carrier physical default.
ping -c 1 -W 1 203.0.113.1 > "$fixture/no-tunnel.log" 2>&1 || true
ip netns exec op-warp nft -j list table inet op_warp_namespace | jq -e 'any(.nftables[].rule?;.comment == "op-warp-data-no-underlay" and any(.expr[];.counter.packets? > 0))' >/dev/null
# Synthetic provider-side link. This is not an official-client compatibility test.
ip netns add op-test-provider
ip link add warp-internet netns op-test-provider type veth peer name CloudflareWARP netns op-warp
ip netns exec op-test-provider ip link set lo up
ip netns exec op-test-provider ip link set warp-internet up
ip netns exec op-test-provider ip addr add 198.51.100.1/24 dev warp-internet
ip netns exec op-test-provider ip -6 addr add 2001:db8:44::1/64 dev warp-internet nodad
ip netns exec op-warp ip link set CloudflareWARP up
ip netns exec op-warp ip addr add 198.51.100.2/24 dev CloudflareWARP
ip netns exec op-warp ip -6 addr add 2001:db8:44::2/64 dev CloudflareWARP nodad
ping -c 1 -W 1 -I 192.168.252.1 198.51.100.1 > "$fixture/data4.log"
ping -6 -c 1 -W 1 -I fd42:7761:7270:1::1 2001:db8:44::1 > "$fixture/data6.log"
ip netns exec op-warp nft -j list chain inet op_warp_namespace data_nat | jq -e '[.nftables[].rule?.expr[]?.counter?.packets // empty]|length == 2 and all(.[];. > 0)' >/dev/null
"$helper" check
# Carrier UDP is accepted and SNATed, independent of root remainder default.
ip rule add priority 90 to 162.159.198.2 lookup main
nft add table inet fixture
nft 'add chain inet fixture after_nat { type filter hook postrouting priority 110; policy accept; }'
nft 'add rule inet fixture after_nat ip daddr 162.159.198.2 udp dport 443 ip saddr 192.0.2.1 counter comment "carrier-snat"'
ip netns exec op-warp bash -c 'printf carrier > /dev/udp/162.159.198.2/443'
nft -j list table inet fixture | jq -e 'any(.nftables[].rule?;.comment == "carrier-snat" and any(.expr[];.counter.packets? > 0))' >/dev/null
ip netns exec op-warp bash -c 'printf forbidden > /dev/udp/203.0.113.9/443' > "$fixture/forbidden-output.log" 2>&1 || true
ip netns exec op-warp nft -j list table inet op_warp_namespace | jq -e 'any(.nftables[].rule?;.comment == "op-warp-carrier-scoped" and any(.expr[];.counter.packets? > 0))' >/dev/null
"$helper" check
nft add rule ip op_warp_namespace carrier_nat counter
if "$helper" down > "$fixture/foreign-nft.log" 2>&1; then echo 'foreign root NAT erased' >&2; exit 1; fi
handle=$(nft -j list chain ip op_warp_namespace carrier_nat | jq -r '.nftables[].rule?|select(.expr|length == 1)|.handle')
nft delete rule ip op_warp_namespace carrier_nat handle "$handle"
ip link set warp-data alias foreign
if "$helper" down > "$fixture/foreign-link.log" 2>&1; then exit 1; fi
ip link set warp-data alias op-warp-namespace
# A resident process must stop before namespace deletion.
ip netns exec op-warp sleep 20 & resident=$!
if "$helper" down > "$fixture/resident.log" 2>&1; then exit 1; fi
kill "$resident"
wait "$resident" || true
ip netns del op-test-provider
"$helper" check
"$helper" down
[ ! -e /run/netns/op-warp ]
ip -j link show | jq -e 'all(.[];.ifname != "warp-uplink" and .ifname != "warp-data")' >/dev/null
for f in 4 6; do ip -"$f" rule del priority 200 lookup 28053; ip -"$f" route flush table 28053; done
ip rule del priority 100 to 192.168.253.0/30 lookup main
ip rule del priority 90 to 162.159.198.2 lookup main
ip -N -4 -j rule show > "$fixture/rules4-after.json"
ip -N -6 -j rule show > "$fixture/rules6-after.json"
cmp "$fixture/rules4-before.json" "$fixture/rules4-after.json"
cmp "$fixture/rules6-before.json" "$fixture/rules6-after.json"
# Failure during construction remains explicit and reversible.
mkdir "$fixture/bin"
cat > "$fixture/bin/ip" <<'EOF'
#!/bin/bash
if [[ "$*" == 'addr add 192.168.252.1/30 '* ]]; then exit 77; fi
exec /usr/bin/ip "$@"
EOF
chmod 755 "$fixture/bin/ip"
if PATH="$fixture/bin:$PATH" "$helper" prepare > "$fixture/partial.log" 2>&1; then exit 1; fi
jq -e '.incomplete == true' "$OP_WARP_SSH_STATE_DIR/namespace/receipt.json" >/dev/null
"$helper" down
# A cancelled prepare also records ownership for exact down.
cat > "$fixture/bin/ip" <<'EOF'
#!/bin/bash
if [[ "$*" == 'addr add 192.168.252.1/30 '* ]]; then kill -TERM "$PPID"; exit 143; fi
exec /usr/bin/ip "$@"
EOF
if PATH="$fixture/bin:$PATH" "$helper" prepare > "$fixture/cancelled.log" 2>&1; then exit 1; fi
jq -e '.incomplete == true' "$OP_WARP_SSH_STATE_DIR/namespace/receipt.json" >/dev/null
"$helper" down
printf 'PASS: isolated /run/netns, no data-underlay escape, NAT44/NAT66, carrier scoped SNAT, foreign ownership refusal, resident process refusal, partial/SIGTERM rollback, original RPDB preservation; evidence %s\n' "$fixture"
