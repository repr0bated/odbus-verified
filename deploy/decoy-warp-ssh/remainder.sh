#!/bin/bash
# Independent remainder routing; official-client split exclusions are external.
set -Eeuo pipefail
umask 077
die() { printf 'warp-ssh remainder: %s\n' "$*" >&2; exit 1; }
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}/remainder
action=${1:-check}
case "$action" in prepare|enable|check|rollback) ;; *) die 'usage: remainder.sh prepare|enable|check|rollback' ;; esac
mkdir -p "$state"
exec 9>"$state/lock"
flock -n 9 || die 'another remainder operation is active'
receipt=$state/receipt.json
if [ -f "$receipt" ]; then config=$state/config.json; fi
jq -se 'length == 1 and (.[0]|type == "object")' "$config" >/dev/null || die 'expected one configuration object'
jq -e '.role == "decoy" and
 (.warp_interface|type == "string" and test("^[A-Za-z0-9_.-]{1,15}$")) and
 (.remainder_table|type == "number" and . == floor and . >= 1000 and . <= 2147483647) and
 (.remainder_priority|type == "number" and . == floor and . >= 103 and . < 32766) and
 ((.remainder_mtu // 1300)|type == "number" and . == floor and . >= 1280 and . <= 1500)
 and ((.warp_gateway4 // "") == "" or (.warp_gateway4 == "192.168.252.2" and .warp_source4 == "192.168.252.1"))
 and ((.warp_gateway6 // "") == "" or (.warp_gateway6 == "fd42:7761:7270:1::2" and .warp_source6 == "fd42:7761:7270:1::1"))
' "$config" >/dev/null || die 'invalid remainder configuration'
warp=$(jq -r .warp_interface "$config")
table=$(jq -r .remainder_table "$config")
priority=$(jq -r .remainder_priority "$config")
mtu=$(jq -r '.remainder_mtu // 1300' "$config")
gateway4=$(jq -r '.warp_gateway4 // ""' "$config")
gateway6=$(jq -r '.warp_gateway6 // ""' "$config")
source4=$(jq -r '.warp_source4 // ""' "$config")
source6=$(jq -r '.warp_source6 // ""' "$config")
json_ip() { local out; out=$(ip -N "$@") || die 'netlink read failed'; [ -n "$out" ] || die 'empty netlink response'; printf '%s\n' "$out"; }
rules() {
 local family=$1
 json_ip -"$family" -j rule show | jq --argjson p "$priority" --argjson t "$table" --argjson f "$family" '[.[]|select(.priority == $p or ($f == 6 and (.priority == ($p-1) or .priority == ($p-2))) or (.table|tostring) == ($t|tostring))]'
}
routes() {
 json_ip -"$1" -j route show table all | jq --argjson t "$table" '[.[]|select((.table|tostring) == ($t|tostring))|if .type == "7" or .type == 7 then .type="unreachable" else . end|if has("flags") then .flags -= ["linkdown"] else . end]'
}
save() {
 local family
 for family in 4 6; do
  rules "$family" > "$state/rules-$family.json"
  routes "$family" > "$state/routes-$family.json"
 done
}
check() {
 local family
 [ -f "$receipt" ] || die 'remainder preparation receipt missing'
 for family in 4 6; do
  rules "$family" > "$state/current-rules-$family.json"
  cmp -s "$state/rules-$family.json" "$state/current-rules-$family.json" || die "IPv$family rule ownership differs"
  routes "$family" > "$state/current-routes-$family.json"
  # Device deletion may remove only its unicast default. Every terminal route
  # must remain, and every surviving route must exactly match its receipt.
  jq -e --slurpfile expected "$state/routes-$family.json" '
   (group_by([.dst,.metric])|all(.[];length == 1)) and
   all(.[];. as $r|any($expected[0][];. == $r)) and
   (($expected[0]|map(select(.type == "unreachable"))) as $guards |
    . as $current | all($guards[];. as $g|any($current[];. == $g)))
  ' "$state/current-routes-$family.json" >/dev/null || die "IPv$family route ownership differs"
 done
}
incomplete() {
 trap - ERR INT TERM
 # Mutations are checkpointed before the next operation. A failed command
 # cannot turn partial preparation into a ready policy.
 jq -n '{incomplete:true,enabled:false}' > "$receipt"
 die 'operation interrupted; rollback required'
}
mutate() { ip "$@"; save; }
add_default() {
 local family=$1 gateway=$2 source=$3
 if [ -n "$gateway" ]; then
  mutate -"$family" route add default via "$gateway" dev "$warp" src "$source" mtu "$mtu" metric 100 table "$table"
 else
  mutate -"$family" route add default dev "$warp" mtu "$mtu" metric 100 table "$table"
 fi
}
case "$action" in
 check) check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'incomplete preparation; rollback required' ;;
 prepare)
  if [ -f "$receipt" ]; then check; jq -e '.incomplete != true' "$receipt" >/dev/null || die 'incomplete preparation; rollback required'; exit 0; fi
  for family in 4 6; do
   [ "$(rules "$family"|jq length)" = 0 ] || die "IPv$family priority/table already used"
   [ "$(routes "$family"|jq length)" = 0 ] || die "IPv$family table already used"
  done
  cp "$config" "$state/config.json"
  save
  jq -n '{incomplete:true,enabled:false}' > "$receipt"
  trap incomplete ERR INT TERM
  mutate -4 route add unreachable default table "$table" metric 32767
  mutate -6 route add unreachable default table "$table" metric 32767
  mutate -6 rule add priority "$((priority-2))" to fe80::/10 lookup main
  mutate -6 rule add priority "$((priority-1))" to ff00::/8 lookup main
  mutate -4 rule add priority "$priority" lookup "$table"
  mutate -6 rule add priority "$priority" lookup "$table"
  jq -n '{incomplete:false,enabled:false}' > "$receipt"
  trap - ERR INT TERM
  check
  ;;
 enable)
  check
  jq -e '.incomplete != true' "$receipt" >/dev/null || die 'incomplete preparation; rollback required'
  addresses=$(json_ip -j addr show dev "$warp")
  global() { jq -e --arg f "$1" 'any(.[].addr_info[];.family == $f and (.scope == "global" or .scope == "0" or .scope == 0))' <<<"$addresses" >/dev/null; }
  global inet || die 'WARP interface has no global IPv4 address'
  trap incomplete ERR INT TERM
  # Interface recreation removes only its unicast routes. Strict check above
  # proves the terminal guards/rules and all survivors before filling holes.
  if ! jq -e --arg dev "$warp" 'any(.[];.dev == $dev and .dst == "default" and .metric == 100)' "$state/current-routes-4.json" >/dev/null; then
   add_default 4 "$gateway4" "$source4"
  fi
  if global inet6 && ! jq -e --arg dev "$warp" 'any(.[];.dev == $dev and .dst == "default" and .metric == 100)' "$state/current-routes-6.json" >/dev/null; then
   add_default 6 "$gateway6" "$source6"
  fi
  check
  jq '.enabled=true' "$receipt" > "$state/next.json"
  mv "$state/next.json" "$receipt"
  trap - ERR INT TERM
  ;;
 rollback)
  check
  jq -n '{incomplete:true,enabled:false}' > "$receipt"
  trap incomplete ERR INT TERM
  # Delete only the exact receipt-confirmed rules; missing rules in a partial
  # initialization are normal. No priority-only delete can erase foreign work.
  for family in 4 6; do
   if jq -e --argjson p "$priority" 'any(.[];.priority == $p)' "$state/rules-$family.json" >/dev/null; then mutate -"$family" rule del priority "$priority" lookup "$table"; fi
  done
  for pair in "$((priority-2)):fe80::/10" "$((priority-1)):ff00::/8"; do
   p=${pair%%:*}; prefix=${pair#*:}
   if jq -e --argjson p "$p" 'any(.[];.priority == $p)' "$state/rules-6.json" >/dev/null; then mutate -6 rule del priority "$p" to "$prefix" lookup main; fi
  done
  for family in 4 6; do
   if [ "$(jq length "$state/routes-$family.json")" != 0 ]; then mutate -"$family" route flush table "$table"; fi
  done
  rm -f "$receipt"
  trap - ERR INT TERM
  ;;
esac
