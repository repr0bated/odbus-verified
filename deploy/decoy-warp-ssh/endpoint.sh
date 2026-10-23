#!/bin/bash
# Explicit transport endpoint maintenance; never run automatically on shutdown.
set -euo pipefail
fail() { printf '%s\n' "warp-ssh endpoint: $*" >&2; exit 1; }
ip_json() {
 local output
 output=$(ip "$@") || fail 'network state read failed'
 [ -n "$output" ] || fail 'network state read returned no JSON'
 printf '%s\n' "$output"
}
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
jq -e -s 'length == 1 and (.[0] | type == "object")' "$config" >/dev/null || fail 'configuration must contain one JSON object'
jq -e '
 def ipv4: type == "string" and (split(".") | length == 4 and all(.[]; test("^(0|[1-9][0-9]{0,2})$") and (tonumber <= 255)));
 (.role == "host" or .role == "decoy") and
 (.tun_id | type == "number" and floor == . and . >= 0 and . <= 255) and
 ((.interface_name // ("tun" + (.tun_id | tostring))) | type == "string" and test("^[a-zA-Z0-9_-]{1,15}$")) and
 (.mtu | type == "number" and floor == . and . >= 1280 and . <= 1500) and
 (.local_address | ipv4) and (.peer_address | ipv4) and
 .local_address != .peer_address and
 ((.aliases // []) | type == "array" and (unique | length) == length and all(.[]; ipv4)) and
 (.local_address as $local | .peer_address as $peer | all((.aliases // [])[]; . != $local and . != $peer)) and
 (.ssh_user | type == "string" and test("^[a-z_][a-z0-9_-]*$"))
' "$config" >/dev/null || fail 'invalid endpoint configuration'
tun_id=$(jq -r .tun_id "$config")
attachment=tun$tun_id
device=$(jq -r '.interface_name // ("tun" + (.tun_id | tostring))' "$config")
local_address=$(jq -r .local_address "$config")
peer_address=$(jq -r .peer_address "$config")
mtu=$(jq -r .mtu "$config")
ssh_user=$(jq -r .ssh_user "$config")
owner=$(id -u "$ssh_user") || fail 'tunnel user does not exist'
[ "$owner" != 0 ] || fail 'tunnel user must be nonroot'
owned() {
 state=$(ip_json -j -d link show dev "$device") || fail 'endpoint missing'
 printf '%s' "$state" | jq -e --arg user "$ssh_user" --arg uid "$owner" '
 length == 1 and .[0].ifalias == "op-warp-ssh" and
 .[0].linkinfo.info_kind == "tun" and
 (.[0].linkinfo.info_data | .type == "tun" and .persist == true and .pi == false and
 ((.user | tostring) == $user or (.user | tostring) == $uid))
 ' >/dev/null || fail 'refusing unmanaged or incorrectly owned endpoint'
}
case ${1:-} in
 up)
  # OpenSSH attaches by numeric tun ID. Preserve that as a kernel alternative
  # name while displaying the transport's descriptive primary interface name.
  if [ "$device" != "$attachment" ] && ! ip link show dev "$device" >/dev/null 2>&1 && ip link show dev "$attachment" >/dev/null 2>&1; then
   desired=$device
   device=$attachment
   owned
   ip -j link show dev "$device" | jq -e 'all(.[]; .flags | index("LOWER_UP") == null)' >/dev/null || fail 'stop the SSH carrier before renaming its endpoint'
   ip link set dev "$device" down
   ip link set dev "$device" name "$desired"
   device=$desired
  fi
  if ip link show dev "$device" >/dev/null 2>&1; then
   owned
  else
   ip tuntap add dev "$device" mode tun user "$owner"
   ip link set dev "$device" alias op-warp-ssh
   owned
  fi
  if [ "$device" != "$attachment" ]; then
   if ! ip -j link show dev "$device" | jq -e --arg name "$attachment" '.[0].altnames // [] | index($name) != null' >/dev/null; then
    ip link property add dev "$device" altname "$attachment"
   fi
  fi
  # Refuse to repurpose an endpoint carrying any other IPv4 address.
  aliases=$(jq -c '.aliases // []' "$config")
  ip_json -j -4 addr show dev "$device" | jq -e --arg local "$local_address" --arg peer "$peer_address" --argjson aliases "$aliases" '
  all(.[].addr_info[]; .prefixlen == 32 and
    ((.local == $local and ((.peer // .address) == $peer or (.peer // .address) == ($peer + "/32"))) or
     (.local as $ip | $aliases | index($ip) != null)))
  ' >/dev/null || fail 'endpoint contains unexpected addresses'
  ip addr replace "$local_address/32" peer "$peer_address/32" dev "$device"
  while IFS= read -r alias; do
   if ! ip_json -j -4 addr show dev "$device" | jq -e --arg ip "$alias" 'any(.[].addr_info[]; .local == $ip)' >/dev/null; then
    ip addr add "$alias/32" dev "$device" noprefixroute
   fi
   ip_json -j -4 addr show | jq -e --arg dev "$device" --arg ip "$alias" '
    [.[] | select(any(.addr_info[]; .local == $ip)) | .ifname] == [$dev]
   ' >/dev/null || fail "alias $alias also exists on another interface"
  done < <(jq -r '.aliases // [] | .[]' "$config")
  ip link set dev "$device" mtu "$mtu" up
  ip_json -j -d link show dev "$device"
  ;;
 check)
  owned
  aliases=$(jq -c '.aliases // []' "$config")
  ip_json -j -4 addr show dev "$device" | jq -e \
    --arg local "$local_address" --arg peer "$peer_address" \
    --argjson mtu "$mtu" --argjson aliases "$aliases" '
    length == 1 and (.[0] as $endpoint |
      $endpoint.mtu == $mtu and ($endpoint.flags | index("UP") != null) and
      any($endpoint.addr_info[]; .local == $local and .prefixlen == 32 and
        ((.peer // .address) == $peer or (.peer // .address) == ($peer + "/32"))) and
      all($aliases[]; . as $alias | any($endpoint.addr_info[]; .local == $alias)))
  ' >/dev/null || fail 'configured local endpoint is not ready'
  ;;
 status) owned; ip_json -j -d link show dev "$device"; ip_json -j addr show dev "$device" ;;
 down) owned; ip tuntap del dev "$device" mode tun ;;
 *) fail 'usage: endpoint.sh up|check|status|down' ;;
esac
