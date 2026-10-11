#!/bin/bash
# Decoy-originated foreground SSH TUN; route and firewall owners are separate.
set -euo pipefail
fail() { printf '%s\n' "warp-ssh connect: $*" >&2; exit 1; }
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
 def tunid: type == "number" and floor == . and . >= 0 and . <= 255;
 .role == "decoy" and (.tun_id | tunid) and (.remote_tun_id | tunid) and
 ((.interface_name // ("tun" + (.tun_id | tostring))) | type == "string" and test("^[a-zA-Z0-9_-]{1,15}$")) and
 (.ssh_host | ipv4) and
 (.ssh_port | type == "number" and floor == . and . >= 1 and . <= 65535) and
 (.ssh_user | type == "string" and test("^[a-z_][a-z0-9_-]*$")) and
 (.warp_interface | type == "string" and test("^[a-zA-Z0-9_.-]{1,15}$")) and
 all(.key_file, .known_hosts_file; type == "string" and startswith("/") and (explode | all(.[]; . > 32 and . != 127)))
' "$config" >/dev/null || fail 'invalid client configuration'
tun_id=$(jq -r .tun_id "$config")
remote_tun_id=$(jq -r .remote_tun_id "$config")
ssh_host=$(jq -r .ssh_host "$config")
ssh_port=$(jq -r .ssh_port "$config")
ssh_user=$(jq -r .ssh_user "$config")
warp_interface=$(jq -r .warp_interface "$config")
key_file=$(jq -r .key_file "$config")
known_hosts_file=$(jq -r .known_hosts_file "$config")
owner=$(id -u "$ssh_user") || fail 'tunnel user does not exist'
[ "$owner" != 0 ] || fail 'tunnel user must be nonroot'
caller=$(id -u)
[ "$caller" = 0 ] || [ "$caller" = "$owner" ] || fail 'preflight must run as service owner or dedicated tunnel user'
[ -f "$key_file" ] && [ -r "$key_file" ] || fail 'private key unavailable'
[ -s "$known_hosts_file" ] && [ -r "$known_hosts_file" ] || fail 'pinned host trust unavailable'
device=$(jq -r '.interface_name // ("tun" + (.tun_id | tostring))' "$config")
ip_json -j -d link show dev "tun$tun_id" | jq -e --arg device "$device" --arg user "$ssh_user" --arg uid "$owner" '
 length == 1 and .[0].ifalias == "op-warp-ssh" and
 .[0].ifname == $device and
 .[0].linkinfo.info_kind == "tun" and
 (.[0].linkinfo.info_data | .type == "tun" and .persist == true and .pi == false and
 ((.user | tostring) == $user or (.user | tostring) == $uid))
' >/dev/null || fail 'managed persistent endpoint unavailable'
ip_json -j -4 route get "$ssh_host" uid "$owner" ipproto tcp dport "$ssh_port" | jq -e --arg device "$warp_interface" '
 length == 1 and .[0].dev == $device
' >/dev/null || fail 'SSH outer destination is not routed through MASQUE'
# Inspect authoritative rules through the guard owner's exact readback check,
# including refusal of additional rules that could bypass the intended drop.
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
"$script_dir/guard.sh" check || fail 'exact SSH carrier leak guard cannot be verified'
set -- ssh -F /dev/null -4 -N -T -w "$tun_id:$remote_tun_id" \
 -o Tunnel=point-to-point -o BatchMode=yes -o IdentitiesOnly=yes \
 -o StrictHostKeyChecking=yes -o UpdateHostKeys=no \
 -o UserKnownHostsFile="$known_hosts_file" -o GlobalKnownHostsFile=/dev/null \
 -o IdentityAgent=none -o ControlMaster=no -o ControlPath=none \
 -o ExitOnForwardFailure=yes -o ConnectTimeout=15 \
 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 \
 -o ForwardAgent=no -o ForwardX11=no \
 -i "$key_file" -p "$ssh_port" "$ssh_user@$ssh_host"
if [ "$caller" = 0 ]; then
 exec setpriv --reuid "$ssh_user" --regid "$ssh_user" --init-groups -- "$@"
fi
exec "$@"
