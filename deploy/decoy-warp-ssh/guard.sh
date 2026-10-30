#!/bin/bash
# The SSH carrier must never fall back to physical egress or into its TUN.
# Only this new connection is covered; the full WARP egress policy is separate.
set -euo pipefail
umask 077
config=${OP_WARP_SSH_CONFIG:-/etc/op-dbus/warp-ssh.json}
die() { printf 'warp-ssh guard: %s\n' "$*" >&2; exit 1; }
command -v jq >/dev/null || die 'jq is required'
jq -se 'length == 1 and (.[0] | type == "object")' "$config" >/dev/null || die 'expected one configuration object'
jq -e '
  def ipv4: type == "string" and
    (split(".") | length == 4 and all(.[];
      test("^(0|[1-9][0-9]{0,2})$") and (tonumber <= 255)));
  .role == "decoy" and (.ssh_host | ipv4) and
  (.ssh_port | type == "number" and . == floor and . > 0 and . <= 65535) and
  (.warp_interface | type == "string" and test("^[a-zA-Z0-9_.-]{1,15}$"))
' "$config" >/dev/null || die 'invalid decoy carrier configuration'
host=$(jq -r .ssh_host "$config")
port=$(jq -r .ssh_port "$config")
warp=$(jq -r .warp_interface "$config")
action=${1:-check}
case "$action" in install|check|remove) ;; *) die 'usage: guard.sh {install|check|remove}' ;; esac
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}
mkdir -p "$state"
exec 8>"$state/guard.lock"
flock -n 8 || die 'another guard operation is running'

expected=$(mktemp)
trap 'rm -f "$expected"' EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM
cat > "$expected" <<EOF
table inet op_warp_ssh {
  comment "op-warp-ssh-owned"
  chain ssh_carrier {
    type filter hook output priority -10; policy accept;
    ip daddr $host tcp dport $port oifname != "$warp" counter drop comment "op-warp-ssh-carrier"
  }
}
EOF

check() {
    nft -j list table inet op_warp_ssh | jq -e \
      --arg host "$host" --argjson port "$port" --arg warp "$warp" '
      [.nftables[] | .table? // empty] as $t |
      [.nftables[] | .chain? // empty] as $c |
      [.nftables[] | .rule? // empty] as $r |
      ($t | length) == 1 and $t[0].comment == "op-warp-ssh-owned" and
      ($c | length) == 1 and $c[0].name == "ssh_carrier" and
      $c[0].type == "filter" and $c[0].hook == "output" and
      $c[0].prio == -10 and $c[0].policy == "accept" and
      ($r | length) == 1 and $r[0].chain == "ssh_carrier" and
      $r[0].comment == "op-warp-ssh-carrier" and
      ($r[0].expr | map(select(has("counter") | not))) == [
        {match:{op:"==",left:{payload:{protocol:"ip",field:"daddr"}},right:$host}},
        {match:{op:"==",left:{payload:{protocol:"tcp",field:"dport"}},right:$port}},
        {match:{op:"!=",left:{meta:{key:"oifname"}},right:$warp}},
        {drop:null}
      ]' >/dev/null
}
case "$action" in
    install)
        tables=$(nft -j list tables)
        if jq -e 'any(.nftables[].table?; .family == "inet" and .name == "op_warp_ssh")' <<<"$tables" >/dev/null; then
            check || die 'existing guard differs; refusing to overwrite it'
        else
            nft --check --file "$expected"
            nft --file "$expected"
            check || die 'guard readback differs'
        fi
        ;;
    check) check || die 'carrier guard is absent or differs' ;;
    remove)
        check || die 'guard differs; refusing to delete it'
        nft delete table inet op_warp_ssh
        ;;
esac
