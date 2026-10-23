#!/bin/bash
# Official WARP owns only op-warp's networking, never the decoy root namespace.
set -Eeuo pipefail
umask 077
die() { printf 'warp namespace: %s\n' "$*" >&2; exit 1; }
action=${1:-check}
case "$action" in prepare|check|down) ;; *) die 'usage: namespace.sh prepare|check|down' ;; esac
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}/namespace
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
mkdir -p "$state"
exec 9>"$state/lock"
flock -n 9 || die 'another namespace operation is active'
receipt=$state/receipt.json
if [ -f "$receipt" ]; then config=$state/config.json; fi
jq -se 'length == 1 and (.[0]|type == "object")' "$config" >/dev/null || die 'expected one JSON object'
jq -e '.role == "decoy" and (.namespace // "op-warp") == "op-warp" and (.egress.physical_interface|type == "string" and test("^[A-Za-z0-9_.-]{1,15}$"))' "$config" >/dev/null || die 'invalid namespace configuration'
physical=$(jq -r .egress.physical_interface "$config")
ns=op-warp
nsip() { ip netns exec "$ns" ip "$@"; }
nsnft() { ip netns exec "$ns" nft "$@"; }
normalize_nft() { jq '[.nftables[]|del(.metainfo)|select(length > 0)|walk(if type == "object" then del(.handle)|if has("counter") and (.counter|type) == "object" then .counter={} else . end else . end)]'; }
root_links() {
 ip -j -d link show | jq '[.[]|select(.ifname == "warp-uplink" or .ifname == "warp-data")|{ifindex,ifname,ifalias,mtu,kind:.linkinfo.info_kind}]'
}
root_addresses() {
 ip -j addr show | jq '[.[]|select(.ifname == "warp-uplink" or .ifname == "warp-data")|{ifname,addresses:[.addr_info[]|select(.scope == "global")|{family,local,prefixlen}]}]'
}
ns_links() { nsip -j -d link show | jq '[.[]|select(.ifname == "uplink0" or .ifname == "data0")|{ifindex,ifname,ifalias,mtu,kind:.linkinfo.info_kind}]'; }
ns_addresses() { nsip -j addr show | jq '[.[]|select(.ifname == "uplink0" or .ifname == "data0")|{ifname,addresses:[.addr_info[]|select(.scope == "global")|{family,local,prefixlen}]}]'; }
ns_routes() {
 local f out
 for f in 4 6; do
  out=$(nsip -N -"$f" -j route show table main) || die 'namespace route read failed'
  jq --argjson f "$f" '[.[]|select(.metric == 42800)|.family=$f|if has("flags") then .flags -= ["linkdown"] else . end]' <<<"$out"
 done | jq -s 'add'
}
root_nft() {
 local out
 out=$(nft -j list tables) || die 'root nft inventory failed'
 if jq -e 'any(.nftables[].table?;.family == "ip" and .name == "op_warp_namespace")' <<<"$out" >/dev/null; then nft -j list table ip op_warp_namespace | normalize_nft; else echo '[]'; fi
}
ns_nft() {
 local out
 out=$(nsnft -j list tables) || die 'namespace nft inventory failed'
 if jq -e 'any(.nftables[].table?;.family == "inet" and .name == "op_warp_namespace")' <<<"$out" >/dev/null; then nsnft -j list table inet op_warp_namespace | normalize_nft; else echo '[]'; fi
}
save() {
 root_links > "$state/root-links.json"
 root_addresses > "$state/root-addresses.json"
 root_nft > "$state/root-nft.json"
 if [ -e "/run/netns/$ns" ]; then
  stat -Lc '%d:%i' "/run/netns/$ns" > "$state/ns-inode"
  ns_links > "$state/ns-links.json"
  ns_addresses > "$state/ns-addresses.json"
  ns_routes > "$state/ns-routes.json"
  ns_nft > "$state/ns-nft.json"
 fi
}
check() {
 [ -f "$receipt" ] || die 'namespace receipt missing'
 root_links > "$state/current-root-links.json"
 root_addresses > "$state/current-root-addresses.json"
 root_nft > "$state/current-root-nft.json"
 for part in root-links root-addresses root-nft; do cmp -s "$state/$part.json" "$state/current-$part.json" || die "$part ownership differs"; done
 if [ -f "$state/ns-inode" ]; then
  [ -e "/run/netns/$ns" ] || die 'owned namespace disappeared'
  [ "$(stat -Lc '%d:%i' "/run/netns/$ns")" = "$(cat "$state/ns-inode")" ] || die 'namespace identity differs'
  ns_links > "$state/current-ns-links.json"
  ns_addresses > "$state/current-ns-addresses.json"
  ns_routes > "$state/current-ns-routes.json"
  ns_nft > "$state/current-ns-nft.json"
  for part in ns-links ns-addresses ns-routes ns-nft; do cmp -s "$state/$part.json" "$state/current-$part.json" || die "$part ownership differs"; done
  # A live official daemon may own CloudflareWARP and its own tables/rules.
  # Other devices in this exclusively created namespace are foreign work.
  nsip -j link show | jq -e 'all(.[];.ifname == "lo" or .ifname == "uplink0" or .ifname == "data0" or .ifname == "CloudflareWARP" or .ifname == "gre0" or .ifname == "gretap0" or .ifname == "erspan0")' >/dev/null || die 'foreign namespace interface'
  if jq -e '.incomplete != true' "$receipt" >/dev/null; then
   [ "$(ip netns exec "$ns" sysctl -n net.ipv4.ip_forward)" = 1 ] && [ "$(ip netns exec "$ns" sysctl -n net.ipv6.conf.all.forwarding)" = 1 ] || die 'namespace forwarding changed'
  fi
 fi
}
interrupted() {
 trap - ERR INT HUP TERM
 save
 jq -n '{incomplete:true}' > "$receipt"
 die 'partial preparation retained; inspect and down before retry'
}
case "$action" in
 check) check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'incomplete preparation' ;;
 prepare)
  if [ -f "$receipt" ]; then check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'incomplete preparation'; exit 0; fi
  [ ! -e "/run/netns/$ns" ] || die 'namespace already exists without receipt'
  [ "$(root_links|jq length)" = 0 ] || die 'root veth name already exists'
  [ "$(root_nft|jq length)" = 0 ] || die 'root NAT table already exists'
  cp "$config" "$state/config.json"
  # Clear only stale metadata from an earlier successfully removed namespace.
  rm -f "$state/ns-inode"
  save
  jq -n '{incomplete:true}' > "$receipt"
  trap interrupted ERR INT HUP TERM
  ip netns add "$ns"
  ip link add warp-uplink type veth peer name uplink0 netns "$ns"
  ip link set warp-uplink alias op-warp-namespace
  nsip link set uplink0 alias op-warp-namespace
  ip link add warp-data type veth peer name data0 netns "$ns"
  ip link set warp-data alias op-warp-namespace
  nsip link set data0 alias op-warp-namespace
  ip link set warp-data mtu 1300
  nsip link set data0 mtu 1300
  ip addr add 192.168.253.1/30 dev warp-uplink
  nsip addr add 192.168.253.2/30 dev uplink0
  ip addr add 192.168.252.1/30 dev warp-data
  nsip addr add 192.168.252.2/30 dev data0
  ip -6 addr add fd42:7761:7270:1::1/126 dev warp-data nodad
  nsip -6 addr add fd42:7761:7270:1::2/126 dev data0 nodad
  ip link set warp-uplink up
  ip link set warp-data up
  nsip link set lo up
  nsip link set uplink0 up
  nsip link set data0 up
  nsip route add default via 192.168.253.1 dev uplink0 metric 42800
  nsip route add 10.0.0.0/24 via 192.168.252.1 dev data0 metric 42800
  nsip route add 10.10.0.0/24 via 192.168.252.1 dev data0 metric 42800
  ip netns exec "$ns" sysctl -q -w net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1
  cat > "$state/root.nft" <<EOF
table ip op_warp_namespace {
 comment "op-warp-namespace-owned"
 chain carrier_nat {
  type nat hook postrouting priority srcnat; policy accept;
  iifname "warp-uplink" oifname "$physical" ip saddr 192.168.253.2 counter masquerade
 }
}
EOF
  cat > "$state/namespace.nft" <<'EOF'
table inet op_warp_namespace {
 comment "op-warp-namespace-owned"
 chain data_forward {
  type filter hook forward priority -20; policy accept;
  iifname "data0" oifname "CloudflareWARP" counter accept
  iifname "data0" counter drop comment "op-warp-data-no-underlay"
  iifname "CloudflareWARP" oifname "data0" ct state established,related counter accept
  oifname "data0" counter drop
 }
 chain carrier_output {
  type filter hook output priority -20; policy accept;
  oifname "uplink0" ip daddr 162.159.198.2 udp dport { 443,500,1701,4443,4500,8095,8443 } counter accept
  oifname "uplink0" ip daddr { 162.159.198.2,162.159.137.105,162.159.138.105,162.159.197.3 } tcp dport 443 counter accept
  oifname "uplink0" ip daddr 169.254.169.254 udp dport 53 counter accept
  oifname "uplink0" ip daddr 169.254.169.254 tcp dport 53 counter accept
  oifname "uplink0" counter drop comment "op-warp-carrier-scoped"
 }
 chain data_nat {
  type nat hook postrouting priority srcnat; policy accept;
  iifname "data0" oifname "CloudflareWARP" meta nfproto ipv4 counter masquerade
  iifname "data0" oifname "CloudflareWARP" meta nfproto ipv6 counter masquerade
 }
}
EOF
  nft --check -f "$state/root.nft"
  nsnft --check -f "$state/namespace.nft"
  nft -f "$state/root.nft"
  nsnft -f "$state/namespace.nft"
  save
  jq -n '{incomplete:false}' > "$receipt"
  trap - ERR INT HUP TERM
  check
  ;;
 down)
  check
  if [ -f "$state/ns-inode" ]; then
   residents=$(ip netns pids "$ns") || die 'namespace process inventory failed'
   [ -z "$residents" ] || die 'stop namespace daemon/processes before down'
  fi
  trap interrupted ERR INT HUP TERM
  if [ "$(jq length "$state/root-nft.json")" != 0 ]; then nft delete table ip op_warp_namespace; save; fi
  if [ -f "$state/ns-inode" ]; then
   ip netns del "$ns"
   rm -f "$state/ns-inode"
  fi
  for dev in warp-uplink warp-data; do
   if root_links | jq -e --arg d "$dev" 'any(.[];.ifname == $d)' >/dev/null; then ip link del "$dev"; fi
  done
  save
  rm -f "$receipt"
  trap - ERR INT HUP TERM
  ;;
esac
