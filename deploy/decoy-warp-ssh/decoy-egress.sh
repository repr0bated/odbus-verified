#!/bin/bash
# Operator-owned decoy transport preparation. Never changes existing defaults.
# prepare precedes WARP connect; enable follows externally verified MASQUE.
set -euo pipefail
umask 077
die() { printf 'decoy-egress: %s\n' "$*" >&2; exit 1; }
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}/egress
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
action=${1:-check}
case "$action" in firewall|prepare|enable|retire-gre|check|rollback) ;; *) die 'usage: decoy-egress.sh firewall|prepare|enable|retire-gre|check|rollback' ;; esac
mkdir -p "$state"
exec 9>"$state/lock"
flock -n 9 || die 'another egress operation is active'
receipt=$state/receipt.json
if [ -f "$receipt" ]; then config=$state/config.json; fi
jq -se 'length == 1 and (.[0] | type == "object")' "$config" >/dev/null || die 'expected one configuration object'
jq -e '
 def ip: type == "string" and (split(".") | length == 4 and all(.[]; test("^(0|[1-9][0-9]{0,2})$") and tonumber <= 255));
 def prefix: type == "string" and (split("/") | length == 2 and (.[0]|ip) and (.[1]|test("^([1-9]|[12][0-9]|3[0-2])$")));
 def dev: type == "string" and test("^[A-Za-z0-9_.-]{1,15}$");
 .role == "decoy" and (.ssh_host|ip) and (.warp_interface|dev) and
 (.egress | (.physical_interface|dev) and (.physical_address|ip) and (.gateway|ip) and
 (.gre_host|ip) and (.metadata_address|ip) and (.wgcf_interface|dev) and
 (.underlay_table|type == "number" and . == floor and . >= 1000 and . <= 2147483647) and
 (.wgcf_table|type == "number" and . == floor and . >= 1000 and . <= 2147483647) and
 .underlay_table != .wgcf_table and .wan_reply_mark == 1073741824 and
 (.rule_priority|type == "number" and . == floor and . > 0) and
 (.wireguard_listen_ports|type == "array" and length > 0 and length <= 8 and (unique|length) == length and all(.[]; type == "number" and . == floor and . > 0 and . <= 65535)) and
 (if has("gre_enabled") then (.gre_enabled|type == "boolean") else true end) and
 ((.physical_services // [])|type == "array" and length <= 16 and all(.[]; (.address|ip) and (.protocol == "tcp" or .protocol == "udp") and (.port|type == "number" and . == floor and . > 0 and . <= 65535))) and
 (.local_routes|type == "array" and length <= 16 and (map(.prefix)|unique|length) == length and all(.[]; (.prefix|prefix) and (.dev|dev) and ((.gateway // "") == "" or (.gateway|ip)) and (if has("temporary_gre") then (.temporary_gre|type == "boolean") else true end))) and
 (.rule_priority + 8 + (.local_routes|length) + (.wireguard_listen_ports|length) < 90))
' "$config" >/dev/null || die 'invalid egress configuration'
physical=$(jq -r .egress.physical_interface "$config")
address=$(jq -r .egress.physical_address "$config")
gateway=$(jq -r .egress.gateway "$config")
table=$(jq -r .egress.underlay_table "$config")
wgtable=$(jq -r .egress.wgcf_table "$config")
wgdev=$(jq -r .egress.wgcf_interface "$config")
first=$(jq -r .egress.rule_priority "$config")
gre=$(jq -r .egress.gre_host "$config")
metadata=$(jq -r .egress.metadata_address "$config")
warp=$(jq -r .warp_interface "$config")
sshhost=$(jq -r .ssh_host "$config")
locals=$(jq '.egress.local_routes|length' "$config")
ports=$(jq -r '.egress.wireguard_listen_ports|map(tostring)|join(", ")' "$config")
portcount=$(jq '.egress.wireguard_listen_ports|length' "$config")
last=$((first + 7 + locals + portcount))
if [ -f "$receipt" ]; then last=$(jq -r --argjson fallback "$last" '.rule_last // $fallback' "$receipt"); fi
json_ip() { local out; out=$(ip -N "$@") || die 'netlink read failed'; [ -n "$out" ] || die 'empty netlink response'; printf '%s\n' "$out"; }
normalize_nft() {
 jq '[.nftables[] | del(.metainfo) | select(length > 0) |
   walk(if type == "object" then del(.handle) |
     if has("counter") and (.counter | type) == "object" then .counter = {} else . end else . end)]'
}
nft_state() { local out; out=$(nft -j list table inet op_decoy_egress) || die 'egress table read failed'; [ -n "$out" ] || die 'empty nft response'; normalize_nft <<<"$out"; }
owned_rules() { json_ip -4 -j rule show | jq --argjson first "$first" --argjson last "$last" --argjson table "$table" '[.[] | select((.priority >= $first and .priority <= $last) or ((.table|tostring) == ($table|tostring)))]'; }
wg_guard() { json_ip -4 -j route show table "$wgtable" | jq '[.[] | select((.type == "unreachable" or .type == "7" or .type == 7) and .dst == "default" and .metric == 32766)]'; }
normalize_routes() { jq 'map(if has("flags") then .flags -= ["linkdown"] else . end)'; }
underlay_routes() { json_ip -4 -j route show table all | jq --arg table "$table" '[.[] | select((.table|tostring) == $table)]' | normalize_routes; }
check() {
 [ -f "$receipt" ] || die 'no prepared egress receipt'
 owned_rules > "$state/current-rules.json"
 underlay_routes > "$state/current-routes.json"
 nft_state > "$state/current-nft.json"
 wg_guard > "$state/current-wg.json"
 normalize_routes < "$state/routes.json" > "$state/expected-routes.json"
 cmp -s "$state/rules.json" "$state/current-rules.json" || die 'owned rules changed; refusing mutation'
 cmp -s "$state/expected-routes.json" "$state/current-routes.json" || die 'owned routes changed; refusing mutation'
 cmp -s "$state/nft.json" "$state/current-nft.json" || die 'owned firewall changed; refusing mutation'
 cmp -s "$state/wg.json" "$state/current-wg.json" || die 'wgcf failure guard changed; refusing mutation'
}
render_nft() {
 local guarded=$1
 local gre_allow='' service_allow
 if jq -e '.egress.gre_enabled != false' "$config" >/dev/null; then gre_allow="  ip daddr $gre ip protocol gre counter accept"; fi
 service_allow=$(jq -r '(.egress.physical_services // [])[] | "  ip daddr \(.address) \(.protocol) dport \(.port) counter accept"' "$config")
 cat <<EOF
table inet op_decoy_egress {
 comment "op-decoy-egress-owned"
 chain ingress_identity {
  type filter hook prerouting priority -150; policy accept;
  iifname "$physical" ct direction original ct mark set ct mark | 0x40000000
  ct direction reply ct mark & 0x40000000 == 0x40000000 meta mark set meta mark | 0x40000000
 }
 chain reply_route {
  type route hook output priority -150; policy accept;
  ct direction reply ct mark & 0x40000000 == 0x40000000 meta mark set meta mark | 0x40000000
 }
EOF
 if [ "$guarded" = 1 ]; then
 cat <<EOF
 chain physical_output {
  type filter hook output priority -5; policy accept;
  oifname "$physical" jump physical_allowed
 }
 chain physical_forward {
  type filter hook forward priority -5; policy accept;
  oifname "$physical" jump physical_allowed
 }
 chain physical_allowed {
  ct direction reply ct mark & 0x40000000 == 0x40000000 counter accept
  meta nfproto ipv4 udp sport { $ports } counter accept
  ip daddr 162.159.192.1 udp dport 2408 counter accept
  ip daddr { 162.159.137.105, 162.159.138.105, 162.159.197.3 } tcp dport 443 counter accept
  ip daddr $metadata tcp dport 80 counter accept
  ip daddr $metadata udp dport 53 counter accept
  ip daddr $metadata tcp dport 53 counter accept
  ip daddr { 255.255.255.255, $gateway } udp sport 68 udp dport 67 counter accept
$gre_allow
$service_allow
  ip6 daddr { fe80::/10, ff02::/16 } icmpv6 type { nd-router-solicit, nd-neighbor-solicit, nd-neighbor-advert } counter accept
  counter drop comment "op-decoy-physical-failclosed"
 }
EOF
 fi
 printf '}\n'
}
save_owned() {
 owned_rules > "$state/rules.json"
 underlay_routes > "$state/routes.json"
 nft_state > "$state/nft.json"
 wg_guard > "$state/wg.json"
}
rollback() {
 check
 # Leave physical protection in place until routing exceptions have gone.
 for ((p=first;p<=last;p++)); do
  if json_ip -4 -j rule show | jq -e --argjson p "$p" 'any(.[]; .priority == $p)' >/dev/null; then ip -4 rule del priority "$p"; fi
 done
 ip -4 route flush table "$table"
 if [ "$(wg_guard|jq length)" = 1 ]; then ip -4 route del unreachable default table "$wgtable" metric 32766; fi
 nft delete table inet op_decoy_egress
 rm -f "$receipt"
}
case "$action" in
 firewall)
  # Boot path: fail closed before networking, independent of current WARP
  # health. Interface-name matching is valid even before the link exists.
  tables=$(nft -j list tables)
  if jq -e 'any(.nftables[].table?; .family == "inet" and .name == "op_decoy_egress")' <<<"$tables" >/dev/null; then
   if [ -f "$receipt" ]; then check;
   else
    [ -f "$state/firewall.json" ] || die 'existing firewall has no ownership receipt'
    nft_state > "$state/current-nft.json"
    cmp -s "$state/firewall.json" "$state/current-nft.json" || die 'existing firewall differs'
   fi
   { printf 'delete table inet op_decoy_egress\n'; render_nft 1; } > "$state/firewall.nft"
  else render_nft 1 > "$state/firewall.nft"; fi
  nft --check -f "$state/firewall.nft"
  nft -f "$state/firewall.nft"
  nft_state > "$state/firewall.json"
  if [ -f "$receipt" ]; then
   cp "$state/firewall.json" "$state/nft.json"
   jq '.guard_enabled=true' "$receipt" > "$state/next-receipt.json"
   mv "$state/next-receipt.json" "$receipt"
  fi
  ;;
 check) check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'preparation is incomplete; rollback required' ;;
 rollback) rollback ;;
 retire-gre)
  check
  jq -e '.incomplete != true and .guard_enabled == true' "$receipt" >/dev/null || die 'retirement requires complete guarded preparation'
  if jq -e '.egress.gre_enabled == false' "$config" >/dev/null; then exit 0; fi
  # Remove only explicitly tagged temporary transport copies. Main GRE routes
  # and the GRE device remain untouched until the operator's later retirement.
  jq '.egress.gre_enabled=false | .egress.local_routes |= map(select(.temporary_gre != true))' "$config" > "$state/retired-config.json"
  oldconfig=$config
  config=$state/retired-config.json
  { printf 'delete table inet op_decoy_egress\n'; render_nft 1; } > "$state/retire.nft"
  nft --check -f "$state/retire.nft"
  config=$oldconfig
  pending=0
  trap 'if [ "$pending" = 1 ]; then save_owned; jq --argjson last "$last" ".incomplete=true | .rule_last=\$last" "$receipt" > "$state/next-receipt.json"; mv "$state/next-receipt.json" "$receipt"; fi' EXIT
  trap 'exit 130' INT
  trap 'exit 143' HUP TERM
  nft -f "$state/retire.nft"
  pending=1
  ip -4 rule del priority "$((first + 7))" to "$gre/32" ipproto gre lookup "$table"
  i=0
  while IFS=$'\t' read -r prefix dev via temporary; do
   if [ "$temporary" = true ]; then
    ip -4 rule del priority "$((first + 8 + portcount + i))" to "$prefix" lookup "$table"
    if [ "$via" != '-' ]; then ip -4 route del "$prefix" via "$via" dev "$dev" table "$table";
    else ip -4 route del "$prefix" dev "$dev" table "$table"; fi
   fi
   i=$((i + 1))
  done < <(jq -r '.egress.local_routes[]|[.prefix,.dev,(.gateway // "-"),(.temporary_gre // false)]|@tsv' "$config")
  mv "$state/retired-config.json" "$state/config.json"
  config=$state/config.json
  save_owned
  jq --argjson last "$last" '.gre_retired=true | .rule_last=$last' "$receipt" > "$state/next-receipt.json"
  mv "$state/next-receipt.json" "$receipt"
  pending=0
  trap - EXIT INT HUP TERM
  check
  ;;
 enable)
  check
  jq -e '.incomplete != true' "$receipt" >/dev/null || die 'preparation is incomplete; rollback required'
  json_ip -4 -j route get 1.1.1.1 | jq -e --arg dev "$warp" 'length == 1 and .[0].dev == $dev' >/dev/null || die 'unmarked Internet is not on MASQUE'
  carrier_uid=$(id -u "$(jq -r .ssh_user "$config")") || die 'SSH carrier user missing'
  [ "$carrier_uid" != 0 ] || die 'SSH carrier user must be nonroot'
  carrier_port=$(jq -r .ssh_port "$config")
  json_ip -4 -j route get "$sshhost" uid "$carrier_uid" ipproto tcp dport "$carrier_port" | jq -e --arg dev "$warp" 'length == 1 and .[0].dev == $dev' >/dev/null || die 'SSH is not on MASQUE'
  json_ip -4 -j route get 1.1.1.1 mark 0x51820 | jq -e --arg dev "$wgdev" 'length == 1 and .[0].dev == $dev' >/dev/null || die 'selected wgcf egress changed'
  { printf 'delete table inet op_decoy_egress\n'; render_nft 1; } > "$state/enable.nft"
  nft --check -f "$state/enable.nft"
  nft -f "$state/enable.nft"
  nft_state > "$state/nft.json"
  jq '.guard_enabled=true' "$receipt" > "$state/next-receipt.json"
  mv "$state/next-receipt.json" "$receipt"
  check
  ;;
 prepare)
  if [ -f "$receipt" ]; then check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'preparation is incomplete; rollback required'; exit 0; fi
  json_ip -4 -j rule show | jq -e --argjson first "$first" --argjson last "$last" --argjson table "$table" 'all(.[]; (.priority < $first or .priority > $last) and ((.table|tostring) != ($table|tostring)))' >/dev/null || die 'reserved rule block/table already used'
  json_ip -4 -j route show table all | jq -e --arg table "$table" 'all(.[]; (.table|tostring) != $table)' >/dev/null || die 'underlay table already contains routes'
  [ "$(wg_guard | jq length)" = 0 ] || die 'wgcf guard slot already occupied'
  json_ip -4 -j route show table "$wgtable" | jq -e --arg dev "$wgdev" 'any(.[]; .dst == "default" and .dev == $dev and (.metric // 0) < 32766)' >/dev/null || die 'existing selected wgcf default missing or guard would override it'
  json_ip -4 -j addr show dev "$physical" | jq -e --arg address "$address" 'any(.[].addr_info[]; .local == $address)' >/dev/null || die 'observed physical address missing'
  boot_guard=0
  tables=$(nft -j list tables)
  if jq -e 'any(.nftables[].table?; .family == "inet" and .name == "op_decoy_egress")' <<<"$tables" >/dev/null; then
   [ -f "$state/firewall.json" ] || die 'egress firewall table already exists without ownership receipt'
   nft_state > "$state/current-nft.json"
   cmp -s "$state/firewall.json" "$state/current-nft.json" || die 'boot firewall changed'
   boot_guard=1
  fi
  cp "$config" "$state/config.json"
  render_nft 0 > "$state/prepare.nft"
  if [ "$boot_guard" = 0 ]; then nft --check -f "$state/prepare.nft"; fi
  # A failed/interrupted prepare records partial state for explicit rollback.
  # It does not tear down bootstrap routing behind an active operator session.
  pending=0
  trap 'if [ "$pending" = 1 ]; then save_owned; jq -n --argjson guarded "$boot_guard" --argjson last "$last" "{version:1,guard_enabled:(\$guarded == 1),incomplete:true,rule_last:\$last}" > "$receipt"; fi' EXIT
  trap 'exit 130' INT
  trap 'exit 143' HUP TERM
  if [ "$boot_guard" = 0 ]; then nft -f "$state/prepare.nft"; fi
  pending=1
  ip -4 route add unreachable default table "$wgtable" metric 32766
  ip -4 route add unreachable default table "$table" metric 32767
  ip -4 route add default via "$gateway" dev "$physical" src "$address" table "$table"
  while IFS=$'\t' read -r prefix dev via; do
   [ -n "$prefix" ] || continue
   if [ -n "$via" ]; then ip -4 route add "$prefix" via "$via" dev "$dev" onlink table "$table";
   else ip -4 route add "$prefix" dev "$dev" table "$table"; fi
  done < <(jq -r '.egress.local_routes[]|[.prefix,.dev,(.gateway // "")]|@tsv' "$config")
  p=$first
  ip -4 rule add priority "$p" fwmark 0x40000000/0x40000000 lookup "$table"; p=$((p+1))
  # RPDB "not" negates the complete rule, not only fwmark. An explicit early
  # selection preserves the existing app mark without stealing other prefixes.
  ip -4 rule add priority "$p" fwmark 0x51820 lookup "$wgtable"; p=$((p+1))
  # The client's ALWAYS INCLUDE health check 162.159.197.4 must stay inside
  # MASQUE. Do not bypass complete Cloudflare /24s. Actual MASQUE endpoints
  # and fallback ports belong in observed local_routes/physical_services.
  for prefix in 162.159.192.1/32 162.159.197.3/32 162.159.137.105/32 162.159.138.105/32 "$metadata/32"; do
   ip -4 rule add priority "$p" to "$prefix" lookup "$table"; p=$((p+1))
  done
  if jq -e '.egress.gre_enabled != false' "$config" >/dev/null; then ip -4 rule add priority "$p" to "$gre/32" ipproto gre lookup "$table"; fi
  p=$((p+1))
  while IFS= read -r port; do ip -4 rule add priority "$p" ipproto udp sport "$port" lookup "$table"; p=$((p+1)); done < <(jq -r '.egress.wireguard_listen_ports[]' "$config")
  while IFS= read -r prefix; do ip -4 rule add priority "$p" to "$prefix" lookup "$table"; p=$((p+1)); done < <(jq -r '.egress.local_routes[].prefix' "$config")
  save_owned
  jq -n --argjson guarded "$boot_guard" --argjson last "$last" '{version:1,guard_enabled:($guarded == 1),rule_last:$last}' > "$receipt"
  pending=0
  trap - EXIT INT HUP TERM
  check
  ;;
esac
