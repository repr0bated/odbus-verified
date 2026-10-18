#!/bin/bash
# Bounded native nftables fixture. Everything runs in disposable net namespaces.
set -euo pipefail
export LC_ALL=C

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
HELPER=$SCRIPT_DIR/private-ingress.sh
for tool in unshare ip nsenter nft jq socat timeout; do
    command -v "$tool" >/dev/null 2>&1 || { printf 'missing required tool: %s\n' "$tool" >&2; exit 77; }
done

if [[ ${1:-} != --inside ]]; then
    exec unshare --user --map-root-user --net "$0" --inside
fi
[[ $(readlink /proc/self/ns/net) != "$(readlink /proc/1/ns/net)" ]] || { echo 'not in isolated network namespace' >&2; exit 1; }

fixture=$(mktemp -d /var/tmp/decoy-private-ingress.XXXXXXXX)
chmod 0700 "$fixture"
export OP_DECOY_PRIVATE_INGRESS_STATE_DIR=$fixture/state
peer_pid=
listener_pids=()
cleanup() {
    local pid
    for pid in "${listener_pids[@]:-}"; do kill "$pid" 2>/dev/null || true; done
    if [[ -n $peer_pid ]]; then
        # unshare --fork keeps a supervising parent while the namespace command
        # runs as its child; stop both so the fixture cannot leak a namespace.
        if [[ -r /proc/$peer_pid/task/$peer_pid/children ]]; then
            for pid in $(</proc/$peer_pid/task/$peer_pid/children); do kill "$pid" 2>/dev/null || true; done
        fi
        kill "$peer_pid" 2>/dev/null || true
        wait "$peer_pid" 2>/dev/null || true
    fi
    rm -rf -- "$fixture"
}
trap cleanup EXIT
ip link set lo up

# Create a peer namespace, then move each remote veth endpoint into it.
unshare --net --fork sh -c 'ip link set lo up; exec sleep 600' &
peer_pid=$!
for _ in $(seq 1 50); do kill -0 "$peer_pid" 2>/dev/null && break; sleep 0.05; done
ip link add rtly-lungs type veth peer name private-peer
ip link set rtly-lungs up
ip addr add 198.18.5.1/30 dev rtly-lungs
ip -6 addr add fd42:5::1/64 dev rtly-lungs nodad
ip link set private-peer netns "$peer_pid"
ip link add enp0s6 type veth peer name public-peer
ip link set enp0s6 up
ip addr add 198.18.9.1/30 dev enp0s6
ip -6 addr add fd42:9::1/64 dev enp0s6 nodad
ip link set public-peer netns "$peer_pid"
nsenter --net --target "$peer_pid" ip link set private-peer up
nsenter --net --target "$peer_pid" ip addr add 198.18.5.2/30 dev private-peer
nsenter --net --target "$peer_pid" ip -6 addr add fd42:5::2/64 dev private-peer nodad
nsenter --net --target "$peer_pid" ip link set public-peer up
nsenter --net --target "$peer_pid" ip addr add 198.18.9.2/30 dev public-peer
nsenter --net --target "$peer_pid" ip -6 addr add fd42:9::2/64 dev public-peer nodad

start_listener() {
    local family=$1 port=$2 bind=$3
    if [[ $family == 4 ]]; then
        socat "TCP4-LISTEN:$port,bind=$bind,reuseaddr,fork" SYSTEM:'sleep 3' >/dev/null 2>&1 &
    else
        socat "TCP6-LISTEN:$port,bind=$bind,ipv6only=1,reuseaddr,fork" SYSTEM:'sleep 3' >/dev/null 2>&1 &
    fi
    listener_pids+=("$!")
}
for port in 22 9443 8080; do start_listener 4 "$port" 0.0.0.0; start_listener 6 "$port" ::; done
sleep 0.2

connect_from_peer() {
    local family=$1 address=$2 port=$3
    [[ $family != 6 ]] || address="[$address]"
    timeout 2 nsenter --net --target "$peer_pid" socat -T1 -u /dev/null "TCP$family:$address:$port,connect-timeout=1" >/dev/null 2>&1
}
expect_connect() { connect_from_peer "$@" || { printf 'expected allowed connect failed: %s %s:%s\n' "$1" "$2" "$3" >&2; exit 1; }; }
expect_drop() { if connect_from_peer "$@"; then printf 'expected blocked connect succeeded: %s %s:%s\n' "$1" "$2" "$3" >&2; exit 1; fi; }

"$HELPER" install
"$HELPER" install
"$HELPER" check
expect_connect 4 198.18.5.1 22
expect_connect 4 198.18.5.1 9443
expect_connect 6 fd42:5::1 22
expect_connect 6 fd42:5::1 9443
expect_drop 4 198.18.9.1 22
expect_drop 4 198.18.9.1 9443
expect_drop 6 fd42:9::1 22
expect_drop 6 fd42:9::1 9443
expect_connect 4 198.18.9.1 8080
expect_connect 6 fd42:9::1 8080
echo 'PASS: private IPv4/IPv6 TCP/22+9443 allowed; public IPv4/IPv6 TCP/22+9443 denied; unrelated port allowed'

# Adding a foreign rule makes exact readback fail; neither check nor remove may alter it.
nft add rule inet op_decoy_private_ingress input tcp dport 40000 accept comment 'fixture-foreign'
nft -j list table inet op_decoy_private_ingress >"$fixture/before.json"
if "$HELPER" check >/dev/null 2>&1; then echo 'foreign rule unexpectedly passed check' >&2; exit 1; fi
if "$HELPER" remove >/dev/null 2>&1; then echo 'foreign rule unexpectedly allowed removal' >&2; exit 1; fi
nft -j list table inet op_decoy_private_ingress >"$fixture/after.json"
cmp -s "$fixture/before.json" "$fixture/after.json" || { echo 'refusal changed foreign table' >&2; exit 1; }
handle=$(jq -r '.nftables[] | .rule? | select(.comment == "fixture-foreign") | .handle' "$fixture/before.json")
nft delete rule inet op_decoy_private_ingress input handle "$handle"
# Non-rule objects are also foreign state and must prevent whole-table removal.
nft add set inet op_decoy_private_ingress fixture_set '{ type ipv4_addr; }'
nft -j list table inet op_decoy_private_ingress >"$fixture/before-set.json"
if "$HELPER" remove >/dev/null 2>&1; then echo 'foreign set unexpectedly allowed removal' >&2; exit 1; fi
nft -j list table inet op_decoy_private_ingress >"$fixture/after-set.json"
cmp -s "$fixture/before-set.json" "$fixture/after-set.json" || { echo 'refusal changed foreign set' >&2; exit 1; }
nft delete set inet op_decoy_private_ingress fixture_set
"$HELPER" check
"$HELPER" remove
"$HELPER" remove
if nft list table inet op_decoy_private_ingress >/dev/null 2>&1; then echo 'owned table remains after remove' >&2; exit 1; fi
echo 'PASS: foreign table mutation refused; owned cleanup and idempotence verified'

# Same-name preexisting tables without our receipt are never overwritten.
nft add table inet op_decoy_private_ingress
nft -j list table inet op_decoy_private_ingress >"$fixture/before-existing.json"
if "$HELPER" install >/dev/null 2>&1; then echo 'preexisting table unexpectedly allowed install' >&2; exit 1; fi
nft -j list table inet op_decoy_private_ingress >"$fixture/after-existing.json"
cmp -s "$fixture/before-existing.json" "$fixture/after-existing.json" || { echo 'refusal changed preexisting table' >&2; exit 1; }
nft delete table inet op_decoy_private_ingress
echo 'PASS: preexisting unowned table refused unchanged'
