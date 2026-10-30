#!/bin/bash
# Isolated recovery test for the staged host iptables rules. No live rules touched.
set -euo pipefail
export LC_ALL=C

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
RULES=$(cd -- "$SCRIPT_DIR/../config" && pwd)/iptables.rules
for tool in unshare ip nsenter nft iptables-restore socat timeout awk; do
    command -v "$tool" >/dev/null 2>&1 || { printf 'missing required tool: %s\n' "$tool" >&2; exit 77; }
done
[[ -r $RULES ]] || { echo "missing rules file: $RULES" >&2; exit 1; }

private_accept_count=$(awk '/--comment op-warp-ssh\/private-ssh/ { count++ } END { print count+0 }' "$RULES")
[[ $private_accept_count == 3 ]] || { echo "expected exactly 3 staged private SSH accepts, found $private_accept_count" >&2; exit 1; }

if [[ ${1:-} != --inside ]]; then
    export DECOY_TEST_PARENT_NETNS=$(readlink /proc/self/ns/net)
    exec unshare --user --map-root-user --net "$(realpath -- "$0")" --inside
fi
[[ -n ${DECOY_TEST_PARENT_NETNS:-} && $(readlink /proc/self/ns/net) != "$DECOY_TEST_PARENT_NETNS" ]] || { echo 'not in isolated network namespace' >&2; exit 1; }

fixture=$(mktemp -d /var/tmp/decoy-host-ssh.XXXXXXXX)
chmod 0700 "$fixture"
peer_pid=
listener_pids=()
cleanup() {
    local child pid
    for pid in "${listener_pids[@]:-}"; do kill "$pid" 2>/dev/null || true; done
    if [[ -n $peer_pid ]]; then
        if [[ -r /proc/$peer_pid/task/$peer_pid/children ]]; then
            for child in $(</proc/$peer_pid/task/$peer_pid/children); do kill "$child" 2>/dev/null || true; done
        fi
        kill "$peer_pid" 2>/dev/null || true
        wait "$peer_pid" 2>/dev/null || true
    fi
    rm -rf -- "$fixture"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM
ip link set lo up

unshare --net --fork sh -c 'ip link set lo up; exec sleep 600' &
peer_pid=$!
for _ in $(seq 1 50); do kill -0 "$peer_pid" 2>/dev/null && break; sleep 0.05; done

# Target namespace models the host: TUN address plus a separate public NIC.
ip link add tun5 type veth peer name private-peer
ip link set tun5 up
ip addr add 10.0.0.2/24 dev tun5
ip addr add 10.0.0.3/32 dev tun5
ip route add 192.168.254.2/32 dev tun5
ip route add 192.168.255.2/32 dev tun5
ip link set private-peer netns "$peer_pid"
ip link add enp0s6 type veth peer name public-peer
ip link set enp0s6 up
ip addr add 198.18.8.1/30 dev enp0s6
ip link set public-peer netns "$peer_pid"

# Peer sources reproduce all three approved host paths and one public path.
nsenter --net --target "$peer_pid" ip link set private-peer up
nsenter --net --target "$peer_pid" ip addr add 10.0.0.5/24 dev private-peer
nsenter --net --target "$peer_pid" ip addr add 192.168.254.2/32 dev private-peer
nsenter --net --target "$peer_pid" ip addr add 192.168.255.2/32 dev private-peer
nsenter --net --target "$peer_pid" ip link set public-peer up
nsenter --net --target "$peer_pid" ip addr add 198.18.8.2/30 dev public-peer
nsenter --net --target "$peer_pid" ip rule add from 198.18.8.2/32 table 100
nsenter --net --target "$peer_pid" ip route add table 100 10.0.0.0/24 via 198.18.8.1 dev public-peer

socat TCP4-LISTEN:22,bind=0.0.0.0,reuseaddr,fork SYSTEM:'sleep 5' >/dev/null 2>&1 &
listener_pids+=("$!")
socat TCP4-LISTEN:8444,bind=0.0.0.0,reuseaddr,fork SYSTEM:'sleep 5' >/dev/null 2>&1 &
listener_pids+=("$!")
sleep 0.2

connect_from_peer() {
    local src=$1 dst=$2 port=$3
    timeout 2 nsenter --net --target "$peer_pid" socat -T1 -u /dev/null "TCP4:$dst:$port,bind=$src,connect-timeout=1" >/dev/null 2>&1
}
expect_connect() { connect_from_peer "$@" || { printf 'expected allowed connect failed: source=%s destination=%s port=%s\n' "$1" "$2" "$3" >&2; exit 1; }; }
expect_drop() { if connect_from_peer "$@"; then printf 'expected blocked connect succeeded: source=%s destination=%s port=%s\n' "$1" "$2" "$3" >&2; exit 1; fi; }

# Keep the older early nft accept present to demonstrate that a later ip
# family DROP still wins when it is the only SSH exception.
nft add table inet fixture_early
nft 'add chain inet fixture_early input { type filter hook input priority -20; policy accept; }'
nft add rule inet fixture_early input iifname tun5 ip saddr 10.0.0.0/24 tcp dport 22 accept
awk '!/--comment op-warp-ssh\/private-ssh/' "$RULES" >"$fixture/old.rules"
iptables-restore --wait <"$fixture/old.rules"
expect_drop 10.0.0.5 10.0.0.2 22
echo 'PASS: old stacked inet accept still loses to later iptables SSH DROP'

iptables-restore --wait <"$RULES"
expect_connect 10.0.0.5 10.0.0.2 22
expect_connect 192.168.254.2 10.0.0.2 22
expect_connect 192.168.255.2 10.0.0.2 22
expect_drop 198.18.8.2 10.0.0.2 22
expect_drop 10.0.0.5 10.0.0.3 22
expect_connect 198.18.8.2 10.0.0.2 8444
echo 'PASS: revised source allows three private SSH sources only to 10.0.0.2; public/wrong destination drop; 8444 remains reachable'
