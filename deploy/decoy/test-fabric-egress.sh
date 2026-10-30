#!/bin/bash
# Exercise real policy routing in a disposable namespace, never the host network.
set -euo pipefail
if [ "$#" = 0 ]; then
    exec unshare --user --map-root-user --net bash "$0" --inside
fi
[ "$#" = 1 ] && [ "$1" = --inside ] &&
    [ "$(readlink /proc/self/ns/net)" != "$(readlink /proc/1/ns/net)" ] || exit 2
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ip link set lo up
for device in physical wg-opdbus wgcf-ingress; do
    ip link add "$device" type dummy
    ip link set "$device" up
done
ip addr add 192.0.2.1/24 dev physical
ip addr add 10.0.0.1/24 dev wg-opdbus
ip addr add 172.16.0.2/32 dev wgcf-ingress
ip route add default dev physical
ip route add default dev wgcf-ingress table 51820

sh "$script_dir/decoy-fabric-egress" --policy-only
ip -j -4 route get 188.68.58.237 mark 0x51820 |
    jq -e 'length == 1 and .[0].dev == "wgcf-ingress"' >/dev/null
ip -j -4 route get 188.68.58.237 |
    jq -e 'length == 1 and .[0].dev == "physical"' >/dev/null
ip -j -4 route get 188.68.58.237 mark 0x40000000 |
    jq -e 'length == 1 and .[0].dev == "physical"' >/dev/null
ip -j -4 route get 10.0.0.9 |
    jq -e 'length == 1 and .[0].dev == "wg-opdbus"' >/dev/null

# Reapplying policy must not create a second selector.
sh "$script_dir/decoy-fabric-egress" --policy-only
ip -N -j -4 rule show |
    jq -e '[.[] | select(.priority == 41)] | length == 1' >/dev/null

# A selector with an extra source restriction is foreign, not equivalent.
ip rule del priority 41 fwmark 0x51820 lookup 51820
ip rule add priority 41 from 192.0.2.1 fwmark 0x51820 lookup 51820
before=$(ip -N -j -4 rule show)
if sh "$script_dir/decoy-fabric-egress" --policy-only; then
    echo 'FAIL: foreign selector was accepted' >&2
    exit 1
fi
[ "$(ip -N -j -4 rule show)" = "$before" ]
ip rule del priority 41 from 192.0.2.1 fwmark 0x51820 lookup 51820
sh "$script_dir/decoy-fabric-egress" --policy-only

# Losing the WARP interface must not send marked traffic through main.
ip link del wgcf-ingress
if ip -j -4 route get 188.68.58.237 mark 0x51820; then
    echo 'FAIL: selected traffic escaped after WARP loss' >&2
    exit 1
fi
ip -j -4 route get 188.68.58.237 |
    jq -e 'length == 1 and .[0].dev == "physical"' >/dev/null
printf '%s\n' 'PASS: marked WARP selection, unchanged management/peer return, idempotency, foreign-selector refusal, fail-closed link loss'
