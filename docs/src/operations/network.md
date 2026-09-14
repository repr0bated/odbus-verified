# Network Operations

This page describes **how networking is supposed to work on the current Artix/runit host**, and points at the live inventory when the tree and the machine disagree.

For a dated, command-verified snapshot of interfaces, Incus devices, listeners, xray SNI, and mesh forwarders, see:

**[Host socket & network topology (live)](host-socket-topology-live.md)**

Re-verify that file against the host before topology-changing work (new proxies, prom/graf exposure, socket migrations).

## Model in one paragraph

One OVS bridge (`ovsbr0`) carries the private fabric. Physical uplink (`eth0`) is enslaved and unaddressed; host L3 lives on OVS internal ports **`pub0`** (public) and **`svc0`** (`10.200.0.2/24`). The **xray** container is the only instance with a NIC on the bridge (`10.200.0.1`). WireGuard mesh interface **`3tched`** (`100.69.0.254/16`) is separate L3 and must never be routed via `ovsbr0`.

**Shared socket fabric:** `op-grpc-bridge` dual-listens on host UDS `/run/opdbus/grpc.sock` and shared container UDS `/run/ghostbridge/container.sock` (same gRPC routes). NIC-less containers bind-mount `/run/ghostbridge`. Some services still use Incus TCP proxies / mesh `fwd-*` (migration is incremental).

**OpenFlow** (`op-of-controller` on `127.0.0.1:6653`) is intentionally NORMAL/standalone today — reserved for **future tag routing**, not a current blocker for the socket fabric.

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
- **Do not** put `10.200.0.1` on the host.
- **Do not** point live xray at `/etc/xray/config.json`.
- **Do not** expose Prometheus/Grafana from the netmaker container until the live topology doc is current and the path (mesh vs SNI vs loopback) is chosen.

## Stale references

Older address tables and dinit/ens3 socket-port writeups under `docs/network-address-table.md` and `docs/operations/ghostbridge-incus-ovs-architecture.md` describe previous hosts or intent. Prefer the live topology snapshot and this page for the 2026-07 Artix/runit host.
