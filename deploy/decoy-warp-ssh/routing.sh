#!/bin/bash
# Add a private-prefix policy table without overwriting GRE/main-table routes.
# Losing the TUN cannot fall through to GRE or the public default.
set -euo pipefail
umask 077
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}
die() { printf 'warp-ssh routing: %s\n' "$*" >&2; exit 1; }
action=${1:-check}
case "$action" in activate|check|rollback) ;; *) die 'usage: routing.sh {activate|check|rollback}' ;; esac

command -v jq >/dev/null || die 'jq is required'
command -v flock >/dev/null || die 'flock is required'
mkdir -p "$state"
exec 9>"$state/routing.lock"
flock -n 9 || die 'another route operation is running'
manifest=$state/routing.json
if [ "$action" != activate ]; then
    [ -f "$manifest" ] || die 'no owned routing manifest'
    config=$manifest
fi
jq -se 'length == 1 and (.[0] | type == "object")' "$config" >/dev/null || die 'expected one configuration object'
jq -e '
  def ipv4: type == "string" and
    (split(".") | length == 4 and all(.[];
      test("^(0|[1-9][0-9]{0,2})$") and (tonumber <= 255)));
  def prefix: type == "string" and
    (split("/") | length == 2 and (.[0] | ipv4) and
      (.[1] | test("^([1-9]|[12][0-9]|3[0-2])$")));
  (.role == "host" or .role == "decoy") and
  (.tun_id | type == "number" and . == floor and . >= 0 and . <= 255) and
  ((.interface_name // ("tun" + (.tun_id | tostring))) | type == "string" and test("^[a-zA-Z0-9_-]{1,15}$")) and
  (.mtu | type == "number" and . == floor and . >= 1280 and . <= 1500) and
  (.local_address | ipv4) and (.peer_address | ipv4) and
  .local_address != .peer_address and
  (.policy_table | type == "number" and . == floor and . >= 1000 and . <= 2147483647) and
  (.routes | type == "array" and length > 0 and length <= 64 and
    all(.[]; prefix) and (unique | length) == length) and
  (.rule_priority | type == "number" and . == floor and . > 0) and
  (.rule_priority + (.routes | length) < 32766)
' "$config" >/dev/null || die 'incomplete or invalid route configuration'
tun=$(jq -r '.interface_name // ("tun" + (.tun_id | tostring))' "$config")
peer=$(jq -r .peer_address "$config")
local_address=$(jq -r .local_address "$config")
table=$(jq -r .policy_table "$config")
first=$(jq -r .rule_priority "$config")
count=$(jq '.routes | length' "$config")
mtu=$(jq -r .mtu "$config")
prefixes=$(jq -r '.routes[]' "$config")

# These helpers inspect the live kernel only when an operator invokes this file.
rules_owned() {
    ip -4 -j rule show | jq -e --argjson cfg "$(cat "$config")" '
      map(if has("dstlen") then
        .dst = (.dst + (if .dstlen == 32 then "" else "/" + (.dstlen|tostring) end)) |
        del(.dstlen) else . end) |
      all(.[] | select(.priority >= $cfg.rule_priority and
                       .priority < ($cfg.rule_priority + ($cfg.routes | length)));
        (.table |= tostring) == {
          priority: .priority, src: "all",
          dst: ($cfg.routes[.priority - $cfg.rule_priority] | sub("/32$"; "")),
          table: ($cfg.policy_table | tostring)
        }) and
      ([.[] | select(.priority >= $cfg.rule_priority and
                      .priority < ($cfg.rule_priority + ($cfg.routes | length)))] |
        group_by(.priority) | all(.[]; length == 1)) and
      all(.[] | select((.table | tostring) == ($cfg.policy_table | tostring));
        .priority >= $cfg.rule_priority and
        .priority < ($cfg.rule_priority + ($cfg.routes | length)))' >/dev/null
}
routes_owned() {
    ip -4 -j route show table "$table" | jq -e \
      --arg tun "$tun" --arg peer "$peer" --argjson cfg "$(cat "$config")" '
      all(.[];
        . == {type:"unreachable", dst:"default", metric:32767, flags:[]} or
        ((del(.dst, .flags)) == {dev:$tun, gateway:$peer, metrics:[{mtu:$cfg.mtu}]} and
         (.flags | index("onlink") != null and ((. - ["onlink", "linkdown"]) | length == 0)) and
         (.dst as $d | any($cfg.routes[]; (sub("/32$"; "")) == $d)))) and
      (group_by(.dst) | all(.[]; length == 1))' >/dev/null
}
rollback() {
    rules_owned && routes_owned || die 'owned routing state was changed externally; preserve it for review'
    p=$first
    for prefix in $prefixes; do
        if ip -4 -j rule show | jq -e --argjson p "$p" 'any(.[]; .priority == $p)' >/dev/null; then
            ip -4 rule del priority "$p" to "$prefix" lookup "$table"
        fi
        p=$((p + 1))
    done
    ip -4 route flush table "$table"
    rm -f "$manifest"
}
check() {
    rules_owned && routes_owned || die 'route ownership differs'
    [ "$(ip -4 -j route show table "$table" | jq length)" -eq "$((count + 1))" ] || die 'route set incomplete'
    [ "$(ip -4 -j rule show | jq --argjson first "$first" --argjson count "$count" \
        '[.[] | select(.priority >= $first and .priority < ($first + $count))] | length')" -eq "$count" ] || die 'rule set incomplete'
}
case "$action" in
    check) check ;;
    rollback) rollback ;;
    activate)
        if [ -f "$manifest" ]; then
            projection='{role,tun_id,interface_name,mtu,local_address,peer_address,policy_table,rule_priority,routes}'
            cmp -s <(jq -S "$projection" "$config") <(jq -S "$projection" "$manifest") || die 'routing configuration changed while active'
            check
            exit 0
        fi
        ip -d -j link show dev "$tun" | jq -e 'length == 1 and .[0].ifalias == "op-warp-ssh" and .[0].linkinfo.info_kind == "tun"' >/dev/null || die 'owned TUN missing'
        ip -4 -j addr show dev "$tun" | jq -e --arg address "$local_address" \
            'any(.[].addr_info[]; .local == $address)' >/dev/null || die 'transport address absent'
        [ "$(sysctl -n net.ipv4.ip_forward)" = 1 ] || die 'IPv4 forwarding is not enabled'
        # This is the actual new transport peer, never a service alias or GRE peer.
        ping -4 -n -I "$tun" -c 2 -W 2 "$peer" >/dev/null || die 'SSH transport peer has not passed a live probe'
        ip -4 -j rule show | jq -e --argjson first "$first" --argjson count "$count" --argjson table "$table" '
            all(.[]; (.priority < $first or .priority >= ($first + $count)) and
                     ((.table | tostring) != ($table | tostring)))' >/dev/null || die 'policy table or priority block is already used'
        # Querying all tables allows an unused table without swallowing netlink errors.
        ip -4 -j route show table all | jq -e --arg table "$table" \
            'all(.[]; (.table | tostring) != $table)' >/dev/null || die 'policy table already contains routes'
        cp "$config" "$manifest"
        failed=1
        trap 'if [ "$failed" = 1 ]; then rollback; fi' EXIT
        trap 'exit 130' INT
        trap 'exit 143' HUP TERM
        ip -4 route add unreachable default table "$table" metric 32767
        for prefix in $prefixes; do
            ip -4 route add "$prefix" via "$peer" dev "$tun" onlink table "$table" mtu "$mtu"
        done
        p=$first
        for prefix in $prefixes; do
            ip -4 rule add priority "$p" to "$prefix" lookup "$table"
            p=$((p + 1))
        done
        check
        failed=0
        trap - EXIT HUP INT TERM
        ;;
esac
