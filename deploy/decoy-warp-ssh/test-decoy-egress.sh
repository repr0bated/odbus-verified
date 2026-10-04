#!/bin/bash
# Disposable native fixture; no official client or production namespace access.
set -euo pipefail
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" = 0 ]; then exec unshare --user --map-root-user --net "$0" --inside; fi
[ "$1" = --inside ] || exit 1
fixture=$(mktemp -d /tmp/decoy-egress-fixture.XXXXXX)
export OP_WARP_SSH_CONFIG=$fixture/config.json OP_WARP_SSH_STATE_DIR=$fixture/state
fixture_user=$(id -un 1001)
cat > "$OP_WARP_SSH_CONFIG" <<'EOF'
{"role":"decoy","ssh_host":"188.68.58.237","warp_interface":"warptest","egress":{"physical_interface":"phytest","physical_address":"192.0.2.1","gateway":"192.0.2.2","underlay_table":28050,"rule_priority":40,"wan_reply_mark":1073741824,"wgcf_table":51820,"wgcf_interface":"wgfirst","gre_host":"188.68.58.237","metadata_address":"169.254.169.254","wireguard_listen_ports":[51820,51821],"physical_services":[{"address":"169.254.169.254","protocol":"udp","port":123}],"local_routes":[{"prefix":"10.0.0.0/24","dev":"privtest"},{"prefix":"192.0.2.0/24","dev":"phytest"},{"prefix":"192.168.255.0/30","dev":"gretest","temporary_gre":true},{"prefix":"192.168.254.0/30","dev":"tun5"}]}}
EOF
jq --arg user "$fixture_user" '.ssh_user=$user | .ssh_port=2223' "$OP_WARP_SSH_CONFIG" > "$fixture/next-config.json"
mv "$fixture/next-config.json" "$OP_WARP_SSH_CONFIG"
ip link set lo up
for dev in phytest warptest wgfirst privtest gretest; do ip link add "$dev" type dummy; ip link set "$dev" up; done
ip tuntap add dev tun5 mode tun
ip link set tun5 up
ip addr add 192.0.2.1/24 dev phytest
ip route add default via 192.0.2.2 dev phytest
ip route add 10.0.0.0/24 dev privtest
ip route add default dev wgfirst table 51820
ip rule add priority 101 fwmark 0x51820 lookup 51820
ip -N -4 -j route show table main > "$fixture/main-before.json"
ip -N -4 -j rule show > "$fixture/rules-before.json"
ip -N -4 -j route show table 51820 > "$fixture/wg-before.json"
"$script_dir/decoy-egress.sh" prepare
"$script_dir/decoy-egress.sh" prepare
"$script_dir/decoy-egress.sh" check
# Attachment changes only dynamic linkdown flags on the copied TUN route.
ip -N -4 -j route show table 28050 | jq '[.[]|select(.dev == "tun5")]' > "$fixture/tun-detached.json"
socat TUN,tun-name=tun5,iff-no-pi PIPE > "$fixture/tun-attachment.log" 2>&1 &
tunnel_pid=$!
trap 'kill "$tunnel_pid" 2>/dev/null || true' EXIT
for ((attempt=0;attempt<20;attempt++)); do
 if ip -j link show tun5 | jq -e '.[0].operstate == "UP"' >/dev/null; then break; fi
 sleep 0.05
done
ip -j link show tun5 | jq -e '.[0].operstate == "UP"' >/dev/null
"$script_dir/decoy-egress.sh" check
kill "$tunnel_pid"
wait "$tunnel_pid" || true
trap - EXIT
"$script_dir/decoy-egress.sh" check
# Model official WARP default routing without changing the existing main table.
ip route add default dev warptest table 28053
ip rule add priority 200 lookup 28053
for host in 162.159.192.1 162.159.197.3 162.159.137.105 169.254.169.254; do
 ip -4 -j route get "$host" | jq -e '.[0].dev == "phytest"' >/dev/null
 ip -4 -j route get "$host" mark 0x51820 | jq -e '.[0].dev == "wgfirst"' >/dev/null
done
# Official ALWAYS INCLUDE connectivity check and other addresses in these
# ranges must follow remainder WARP, while marked apps retain selected wgcf.
for host in 162.159.197.4 162.159.197.1 162.159.192.2; do
 ip -4 -j route get "$host" | jq -e '.[0].dev == "warptest"' >/dev/null
 ip -4 -j route get "$host" mark 0x51820 | jq -e '.[0].dev == "wgfirst"' >/dev/null
done
# This iproute2 build rejects GRE in route-get; assert the protocol-scoped RPDB
# entry instead. The TCP lookup below proves the SSH destination is not stolen.
ip -N -4 -j rule show | jq -e 'any(.[]; .dst == "188.68.58.237" and .ipproto == "ipproto-47" and (.table|tostring) == "28050")' >/dev/null
ip -4 -j route get 188.68.58.237 ipproto 6 dport 2223 | jq -e '.[0].dev == "warptest"' >/dev/null
for port in 51820 51821; do ip -4 -j route get 203.0.113.1 ipproto 17 sport "$port" | jq -e '.[0].dev == "phytest"' >/dev/null; done
ip -4 -j route get 203.0.113.1 mark 0x40051820 | jq -e '.[0].dev == "phytest"' >/dev/null
"$script_dir/decoy-egress.sh" enable
"$script_dir/decoy-egress.sh" check
# Existing outbound conntrack state must not get a broad established exemption.
ip rule add priority 30 to 203.0.113.1/32 lookup main
timeout 1 bash -c ': > /dev/tcp/203.0.113.1/443' >/dev/null 2>&1 || true
nft -j list table inet op_decoy_egress | jq -e '[.nftables[].rule? | select(.comment == "op-decoy-physical-failclosed") | .expr[].counter?.packets // empty]|add > 0' >/dev/null
ip rule del priority 30 to 203.0.113.1/32 lookup main
ip -4 route del default dev wgfirst table 51820
if ip -4 route get 1.1.1.1 mark 0x51820 > "$fixture/wg-loss.log" 2>&1; then echo 'wgcf fell through to remainder' >&2; exit 1; fi
ip route add default dev wgfirst table 51820
# Counter changes normalize away, while a foreign rule must refuse rollback.
nft insert rule inet op_decoy_egress physical_allowed accept
if "$script_dir/decoy-egress.sh" rollback > "$fixture/foreign-refusal.log" 2>&1; then echo 'foreign firewall erased' >&2; exit 1; fi
handle=$(nft -j list chain inet op_decoy_egress physical_allowed | jq -r '.nftables[].rule?|select(.expr == [{"accept":null}])|.handle')
nft delete rule inet op_decoy_egress physical_allowed handle "$handle"
# Retire before the device: receipts retain priority holes as owned scope.
"$script_dir/decoy-egress.sh" retire-gre
"$script_dir/decoy-egress.sh" retire-gre
"$script_dir/decoy-egress.sh" check
ip -N -4 -j rule show | jq -e 'all(.[]; .ipproto != "ipproto-47")' >/dev/null
ip -N -4 -j route show table 28050 | jq -e 'all(.[]; .dev != "gretest") and any(.[]; .dev == "tun5")' >/dev/null
nft -j list table inet op_decoy_egress | jq -e 'all(.nftables[].rule?; all(.expr[]?.match?; .left != {payload:{protocol:"ip",field:"protocol"}} or (.right != "gre" and .right != 47)))' >/dev/null
ip rule add priority 47 to 203.0.113.77/32 lookup 28050
if "$script_dir/decoy-egress.sh" rollback > "$fixture/retired-hole-refusal.log" 2>&1; then echo 'retired priority hole admitted foreign state' >&2; exit 1; fi
ip rule del priority 47 to 203.0.113.77/32 lookup 28050
"$script_dir/decoy-egress.sh" rollback
ip rule del priority 200 lookup 28053
ip route flush table 28053
ip -N -4 -j route show table main > "$fixture/main-after.json"
ip -N -4 -j rule show > "$fixture/rules-after.json"
cmp "$fixture/main-before.json" "$fixture/main-after.json"
cmp "$fixture/rules-before.json" "$fixture/rules-after.json"
ip -N -4 -j route show table 51820 > "$fixture/wg-after.json"
cmp "$fixture/wg-before.json" "$fixture/wg-after.json"
# Boot guard does not require WARP connectivity and is retained by prepare.
"$script_dir/decoy-egress.sh" firewall
"$script_dir/decoy-egress.sh" firewall
mkdir "$fixture/bin"
cat > "$fixture/bin/ip" <<'EOF'
#!/bin/bash
set -euo pipefail
if [[ "$*" == '-4 route add default via '* ]]; then exit 77; fi
exec /usr/bin/ip "$@"
EOF
chmod 755 "$fixture/bin/ip"
if PATH="$fixture/bin:$PATH" "$script_dir/decoy-egress.sh" prepare > "$fixture/partial-refusal.log" 2>&1; then echo 'interrupted prepare returned success' >&2; exit 1; fi
jq -e '.incomplete == true and .guard_enabled == true' "$OP_WARP_SSH_STATE_DIR/egress/receipt.json" >/dev/null
if "$script_dir/decoy-egress.sh" enable > "$fixture/incomplete-refusal.log" 2>&1; then echo 'incomplete preparation enabled' >&2; exit 1; fi
"$script_dir/decoy-egress.sh" rollback
ip -N -4 -j route show table main > "$fixture/main-after-partial.json"
ip -N -4 -j rule show > "$fixture/rules-after-partial.json"
cmp "$fixture/main-before.json" "$fixture/main-after-partial.json"
cmp "$fixture/rules-before.json" "$fixture/rules-after-partial.json"
ip -N -4 -j route show table 51820 > "$fixture/wg-after-partial.json"
cmp "$fixture/wg-before.json" "$fixture/wg-after-partial.json"
printf 'PASS: carrier/control routes, selected wgcf preservation, WG listen carriers, MASQUE SSH, physical failclosed, selected-path loss, foreign-state refusal, TUN attachment/detachment, GRE retirement, boot guard, partial-prepare refusal/rollback; evidence %s\n' "$fixture"
