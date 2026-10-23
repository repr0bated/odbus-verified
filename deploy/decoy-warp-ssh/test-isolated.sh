#!/bin/bash
# Native kernel fixtures only. Real SSH/WARP/peer health is a deployment gate.
set -euo pipefail
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" = 0 ]; then
    exec unshare --user --map-root-user --net "$0" --inside
fi
[ "$1" = --inside ] && [ "$(readlink /proc/self/ns/net)" != "$(readlink /proc/1/ns/net)" ] || {
    echo 'tests require an isolated network namespace' >&2; exit 1;
}
fixture=$(mktemp -d /tmp/warp-ssh-native.XXXXXX)
export OP_WARP_SSH_STATE_DIR=$fixture/state
export OP_WARP_SSH_CONFIG=$fixture/config.json
jq '.local_address="198.18.0.1" | .peer_address="198.18.0.2" |
    .warp_interface="warptest" | .policy_table=28051 | .rule_priority=100' \
    "$script_dir/config.example.json" > "$OP_WARP_SSH_CONFIG"
ip link set lo up
for device in phytest greold wgfirst warptest; do
    ip link add "$device" type dummy
    ip link set "$device" up
done
ip addr add 192.0.2.1/24 dev phytest
ip route add default dev phytest
ip route add 10.0.0.0/24 dev greold
ip route add default dev wgfirst table 28052
ip rule add priority 500 fwmark 0x51820 lookup 28052
ip route add default dev warptest table 28053
ip rule add priority 800 lookup 28053
ip tuntap add dev tun5 mode tun
ip link set tun5 alias op-warp-ssh mtu 1280 up
ip addr add 198.18.0.1/32 peer 198.18.0.2/32 dev tun5
sysctl -qw net.ipv4.ip_forward=1
ip -4 -j route show table main | jq '[.[] | select(.dev != "tun5")]' > "$fixture/main-before.json"

"$script_dir/guard.sh" install
"$script_dir/guard.sh" install
"$script_dir/guard.sh" check
nft -j list table inet op_warp_ssh > "$fixture/guard.json"
# A physical-route attempt must increment the scoped carrier drop counter.
ip rule add priority 80 to 188.68.58.237/32 lookup main
timeout 1 bash -c ': > /dev/tcp/188.68.58.237/2223' >/dev/null 2>&1 || true
nft -j list table inet op_warp_ssh | jq -e \
    '[.nftables[].rule?.expr[]?.counter?.packets // empty] | add > 0' >/dev/null
ip rule del priority 80 to 188.68.58.237/32 lookup main
# Prove an additional accept causes ownership/readback refusal.
nft insert rule inet op_warp_ssh ssh_carrier accept
if "$script_dir/guard.sh" check >/dev/null 2>&1; then
    echo 'guard accepted a foreign rule' >&2; exit 1;
fi
handle=$(nft -j list chain inet op_warp_ssh ssh_carrier | jq -r '.nftables[].rule? | select(.expr == [{"accept":null}]) | .handle')
nft delete rule inet op_warp_ssh ssh_carrier handle "$handle"
"$script_dir/guard.sh" check

# An unattached TUN fails the mandatory peer probe without touching routes.
if "$script_dir/routing.sh" activate > "$fixture/probe-refusal.log" 2>&1; then
    echo 'detached TUN passed peer health' >&2; exit 1;
fi
[ ! -e "$OP_WARP_SSH_STATE_DIR/routing.json" ]

# Substitute only peer health for the routing fixture, explicitly NOT an E2E test.
mkdir "$fixture/bin"
printf '#!/bin/sh\nexit 0\n' > "$fixture/bin/ping"
chmod 755 "$fixture/bin/ping"
export PATH="$fixture/bin:$PATH"
"$script_dir/routing.sh" activate
"$script_dir/routing.sh" activate
"$script_dir/routing.sh" check
ip -4 -j route show table main | jq '[.[] | select(.dev != "tun5")]' > "$fixture/main-active.json"
cmp "$fixture/main-before.json" "$fixture/main-active.json"
for mark in 0 0x51820; do
    ip -4 -j route get 10.0.0.2 mark "$mark" | jq -e '.[0].dev == "tun5"' >/dev/null
done
ip -4 -j route get 1.1.1.1 mark 0x51820 | jq -e '.[0].dev == "wgfirst"' >/dev/null
ip -4 -j route get 1.1.1.1 | jq -e '.[0].dev == "warptest"' >/dev/null
ip -4 -j route get 188.68.58.237 | jq -e '.[0].dev == "warptest"' >/dev/null

# Similar-looking rules/routes are not owned if selectors/metrics differ, and
# duplicate rules must never survive a destructive rollback.
ip rule del priority 100 to 10.0.0.2/32 lookup 28051
ip rule add priority 100 to 10.0.0.2/32 lookup 28051 ipproto tcp
if "$script_dir/routing.sh" rollback > "$fixture/selector-refusal.log" 2>&1; then
    echo 'rollback accepted an extra selector' >&2; exit 1;
fi
ip rule del priority 100 to 10.0.0.2/32 lookup 28051 ipproto tcp
ip rule add priority 100 to 10.0.0.2/32 lookup 28051
ip -4 -j rule show > "$fixture/rules-before-duplicate.json"
if ip rule add priority 100 to 10.0.0.2/32 lookup 28051 > "$fixture/duplicate-insertion.log" 2>&1; then
    ip -4 -j rule show > "$fixture/rules-with-duplicate.json"
    if "$script_dir/routing.sh" rollback > "$fixture/duplicate-refusal.log" 2>&1; then
        echo 'rollback accepted a duplicate rule' >&2; exit 1;
    fi
    ip -4 -j rule show > "$fixture/rules-after-duplicate-refusal.json"
    cmp "$fixture/rules-with-duplicate.json" "$fixture/rules-after-duplicate-refusal.json"
    ip rule del priority 100 to 10.0.0.2/32 lookup 28051
else
    # Some kernels reject identical RPDB entries at insertion. Verify that the
    # rejected insertion preserved the original rules and complete owned set.
    ip -4 -j rule show > "$fixture/rules-after-duplicate-rejection.json"
    cmp "$fixture/rules-before-duplicate.json" "$fixture/rules-after-duplicate-rejection.json"
    "$script_dir/routing.sh" check
fi
"$script_dir/routing.sh" check
ip route add 10.0.0.2/32 via 198.18.0.2 dev tun5 onlink table 28051 mtu 1280 metric 99
if "$script_dir/routing.sh" rollback > "$fixture/metric-refusal.log" 2>&1; then
    echo 'rollback accepted a foreign route metric' >&2; exit 1;
fi
ip route del 10.0.0.2/32 via 198.18.0.2 dev tun5 table 28051 metric 99
"$script_dir/routing.sh" check

# A foreign table entry must never be flushed by rollback.
ip route add 203.0.113.0/24 dev phytest table 28051
if "$script_dir/routing.sh" rollback > "$fixture/foreign-refusal.log" 2>&1; then
    echo 'rollback erased an external route' >&2; exit 1;
fi
ip route del 203.0.113.0/24 dev phytest table 28051
# Link loss removes attached routes but the unreachable default stops fallback.
ip tuntap del dev tun5 mode tun
if ip -4 route get 10.0.0.2 > "$fixture/link-loss.log" 2>&1; then
    echo 'private traffic escaped after TUN loss' >&2; exit 1;
fi
"$script_dir/routing.sh" rollback
ip -4 -j route show table main | jq '[.[] | select(.dev != "tun5")]' > "$fixture/main-after.json"
cmp "$fixture/main-before.json" "$fixture/main-after.json"
[ ! -e "$OP_WARP_SSH_STATE_DIR/routing.json" ]

# Cancel after the first private route has been inserted. The EXIT handler must
# remove the partial owned table without changing unrelated routing.
ip tuntap add dev tun5 mode tun
ip link set tun5 alias op-warp-ssh mtu 1280 up
ip addr add 198.18.0.1/32 peer 198.18.0.2/32 dev tun5
cat > "$fixture/bin/ip" <<'EOF'
#!/bin/bash
set -euo pipefail
/usr/bin/ip "$@"
if [[ "$*" == '-4 route add 10.0.0.2/32 '* ]]; then
    kill -TERM "$PPID"
fi
EOF
chmod 755 "$fixture/bin/ip"
if "$script_dir/routing.sh" activate > "$fixture/cancellation.log" 2>&1; then
    echo 'cancelled activation returned success' >&2; exit 1;
fi
[ ! -e "$OP_WARP_SSH_STATE_DIR/routing.json" ]
ip -4 -j route show table all | jq -e 'all(.[]; (.table | tostring) != "28051")' >/dev/null
ip -4 -j route show table main | jq '[.[] | select(.dev != "tun5")]' > "$fixture/main-cancelled.json"
cmp "$fixture/main-before.json" "$fixture/main-cancelled.json"
"$script_dir/guard.sh" remove
printf 'PASS: isolated carrier drop, health refusal, policy selection, link-loss fail-closed, rollback, cancellation; evidence %s\n' "$fixture"
