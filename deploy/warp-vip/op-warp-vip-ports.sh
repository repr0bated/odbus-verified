#!/bin/sh
# 100.96.0.5:8090 (WARP mesh) is the Chatbot door: opdbus-vip-door-8090 forwards it
# to 10.0.0.2:8090, the host bridge tunnel listener that splices into the
# control-plane chatbot door. A DNAT here would bypass that socat and land on the
# host listener (127.0.0.1:8090), so this removes any such rule left behind.
set -eu
PATH=/usr/sbin:/sbin:/usr/bin:/bin
sysctl -w net.ipv4.conf.all.route_localnet=1 >/dev/null
for chain in PREROUTING OUTPUT; do
    while iptables -t nat -C "$chain" -d 100.96.0.5 -p tcp --dport 8090 -j DNAT --to-destination 127.0.0.1:8090 2>/dev/null; do
        iptables -t nat -D "$chain" -d 100.96.0.5 -p tcp --dport 8090 -j DNAT --to-destination 127.0.0.1:8090
    done
done
