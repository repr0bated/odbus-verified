# Network Operations

This page describes **how networking is supposed to work on the current Artix/runit host**, and points at the live inventory when the tree and the machine disagree.

For a dated, command-verified snapshot of interfaces, Incus devices, listeners, xray SNI, and mesh forwarders, see:

**[Host socket & network topology (live)](host-socket-topology-live.md)**

Re-verify that file against the host before topology-changing work (new proxies, prom/graf exposure, socket migrations).

## Do not change this back (owner, 2026-11-02)

Containers attach by **socket, not by a NIC**. A container with no interface is correct. One looks missing because it was removed. Do not put a NIC, a veth, or an address back to fill that gap. The host link list this morning had one veth, `mesh-ovs`. That is the one container NIC. It stays. The others do not come back.

This replaced per-container NICs when the container became the identity. gRPC for those containers is the shared socket the bridge already serves. A NIC would make the container a second network hop again. WireGuard for identity ends at the decoy. This host has no WireGuard interface. Do not add one. OpenFlow tag routing is the later path that puts a container on the fabric. Nothing is listening on `6653` today. Do not install NICs while waiting for it, and do not start the controller just because the socket already works.

Checked on the host the morning of 2026-11-02. Incus itself was not readable from this user, so container names were not re-listed.

| What | Live, do not revert |
| --- | --- |
| gRPC door | `10.0.0.2:8090` |
| UI | `10.0.0.2:8080` |
| `ovsbr0` addresses | `10.0.0.2/32`, `10.200.0.1/24`, `100.96.0.3/32` |
| Public | `pub0` `188.68.58.237/22`. `eth0` is up and unaddressed |
| Shared container socket | `/run/ghostbridge/container.sock` |
| Host gRPC socket | `/run/opdbus/grpc.sock` |
| Lungs | `/run/opdbus/lungs/lungs.sock` |
| Container egress | `/run/ghostbridge/xray/egress-http.sock` |
| Qdrant | `127.0.0.1:6333` and `:6334` only |
| WireGuard | no interface on this host |
| OpenFlow | `ovs-vswitchd` is up. No listener on `6653` |

Do not restore `svc0` at `10.200.0.2`, a host `3tched` WireGuard interface, or a NIC on a socket-attached container. `10.200.0.2:6334` does not answer. The older paragraphs below that still name those are the layout that was changed. This section wins until a new check replaces it.

## Model in one paragraph

One OVS bridge (`ovsbr0`) carries the private fabric. Physical uplink (`eth0`) is enslaved and unaddressed. Host addresses observed on 2026-11-02 are on `ovsbr0` and `pub0`, as in the table above. Container attachment for identity and gRPC is the shared socket, not a bridge port.

**Shared socket fabric:** `op-grpc-bridge` listens on host UDS `/run/opdbus/grpc.sock` and shared container UDS `/run/ghostbridge/container.sock` (same gRPC routes). NIC-less containers bind-mount `/run/ghostbridge`. Some services still use Incus TCP proxies / mesh `fwd-*` (migration is incremental). Do not delete those proxies as if they were the missing NIC, and do not add a NIC beside them.

**OpenFlow** is reserved for **future tag routing**. It is not how a container gets onto the network today, and it is not a reason to give the container an address.

## Boot order (runit)

```text
opdbus-rundirs
  → uplink-dhcp
  → ovsdb-server → ovs-vswitchd
  → ovsbr0-uplink → ovsbr0-addr → ovsbr0-svc-addr
  → op-of-controller
  → wg-3tched
  → op-grpc-bridge / op-web / op-cognitive-mcp
```

Config: `/etc/op-dbus/network.conf`  
Helpers: `/usr/local/libexec/3tched/*-up`

## Public path

Client DNS (NextDNS) → Oracle decoy → WireGuard mesh → host `fwd-8444` → xray SNI on `10.200.0.1:8444` → host services on `10.200.0.2` (dashboard/registration → `:8080`, assistant gRPC → `:8090`, etc.). Live rules: `/dev/shm/xray_config.json` only.

## Attachment modes (summary)

| Pattern | Used by (live) |
| --- | --- |
| Bridged NIC on `ovsbr0` | `xray` only |
| Shared UDS `/run/ghostbridge/container.sock` | bridge + CT mounts (`assistant`, `qdrant`) |
| Incus `proxy` TCP | `mail-3tched`, `netmaker`, `qdrant`, `cozo` |
| Mesh `tcpfwd.py` (`fwd-*`) | side channels into xray/host |
| Host UDS `/run/opdbus/grpc.sock` | host gRPC clients, session bus |

## Do / don't

- **Do** check `sudo incus config device show <name>` before assuming UDS-only or proxy-only.
- **Do** keep mesh routes on `3tched`; strip accidental `100.69.0.0/16` via `ovsbr0`.
- **Do** treat the [Netmaker ACL helper](host-socket-topology-live.md#break-glass-netmaker-acl-recovery) as break-glass recovery; it enables wildcard all-node traffic and bypasses the normal D-Bus path.
- **Do not** add a NIC to a container that has none. The missing interface is the socket attachment. See the 2026-11-02 section above.
- **Do not** add a WireGuard interface on this host. Identity WireGuard ends at the decoy.
- **Do not** point live xray at `/etc/xray/config.json`.
- **Do not** expose Prometheus/Grafana from the netmaker container until the live topology doc is current and the path (mesh vs SNI vs loopback) is chosen.

## Stale references

Older address tables and dinit/ens3 socket-port writeups under `docs/network-address-table.md` and `docs/operations/ghostbridge-incus-ovs-architecture.md` describe previous hosts or intent. Prefer the live topology snapshot and this page for the 2026-07 Artix/runit host.
