# WARP VIP ports (antix1)

| Service | Installed as | What it does |
|---|---|---|
| `opdbus-vip-door-8090` | `/etc/sv/opdbus-vip-door-8090/{run,log/run}` | `socat` 100.96.0.5:8090 (CloudflareWARP mesh) to 10.0.0.2:8090 |
| `op-warp-vip-ports` | `/etc/sv/op-warp-vip-ports/{run,log/run}`, `/usr/local/sbin/op-warp-vip-ports.sh` | keeps the decoy route 10.0.0.1/32 in table 65743 on CloudflareWARP; removes stale 8090 DNAT |

Both are enabled by the `/etc/service/<name>` symlinks, so runsvdir starts them at boot.
Logs: `/var/log/op-dbus/<name>/current` (svlogd, `s4000000 n10 t86400`).

External MCP on 100.96.0.5:8090 must reach the control-plane chatbot door. On the host,
10.0.0.2:8090 is the op-grpc-bridge door tunnel listener (`OP_TUNNEL_DOOR`). It splices each
connection through `/run/opdbus/tunnel/tunnel.sock` into the chatbot's own door. The socat
service is the only path.

`op-warp-vip-ports.sh` used to DNAT 100.96.0.5:8090 to 127.0.0.1:8090 in PREROUTING and OUTPUT.
That bypassed socat and landed on the host bridge's own listener. It now deletes that rule if
present and adds no 8090 NAT. `/etc/iptables/rules.v4` has no 8090 rule.

Check: `openssl s_client -connect 100.96.0.5:8090` presents the chatbot door certificate, not
the host `tonic-svc0` certificate.

Install: copy `op-warp-vip-ports.sh` to `/usr/local/sbin/` (0755) and `sv/<name>` to `/etc/sv/<name>`,
then `ln -s /etc/sv/<name> /etc/service/<name>`.

## User browser doors

`OP_BROWSER_DOOR` selects the host IP, not a shared port. A provisioned user opens
`https://<host>:<assigned-port>/`; the port selects that identity's waiting
door. The container terminates TLS and serves its own UI. Nothing on the host
parses the ClientHello, and the port does not authorize the user.

Browser waiters use `/run/opdbus/tunnel-browser/browser.sock`, separate from
the MCP tunnel. No waiter for the selected identity means refusal, not a
fallback to the chatbot. `zcall tunnel status` reports aggregate browser counts.
Use the `browser` door argument when closing browser splices; omitting it
continues to select the control-plane door. A guest needs its own running TLS
UI listener and `op-tunnel-door`; exposing the socket alone does not install a UI.

The bridge can replay a tunnel 1.0 observation under the sealed 1.1 contract
without rewriting the ledger. Once a 1.1 observation is committed, a 1.0
bridge cannot replay it; reverting the catalog and binary alone is not a
supported downgrade after that point.
