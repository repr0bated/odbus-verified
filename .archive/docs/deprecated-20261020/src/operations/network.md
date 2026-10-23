# Network Operations

This page describes **how networking is supposed to work on the current Artix/runit host**, and points at the live inventory when the tree and the machine disagree.

For a dated, command-verified snapshot of interfaces, Incus devices, listeners, xray SNI, and mesh forwarders, see:

**[Host socket & network topology (live)](host-socket-topology-live.md)**

Re-verify that file against the host before topology-changing work (new proxies, prom/graf exposure, socket migrations).

## Agent rules and owner decisions (moved from AGENTS.md, 2026-10-14)

Read this section before any network, Xray, WARP, decoy, OVS or firewall work.
AGENTS.md now only points here.

**Direct-demo exception (owner, 2026-10-19; not activated yet).** For the demo,
the owner explicitly requests WireGuard termination on the main host and the
door at `10.0.0.1:8090`, setting aside the decoy requirement for this path.
Prepared base-network configuration is in `deploy/runit/wg-demo`; it preserves
existing peer keys and identity admission, adds no fwmark or default-route
takeover, and maps the door to the unchanged `.2:8090` backend. The dashboard
can remain at `.2:8080`; no UI redesign is required. Installation/activation
is blocked by the existing OS upgrade's Cursor package dialog holding dpkg.
No direct tunnel or authenticated demo readiness has been established.
See task `direct-demo-gate-20261019` in `HANDOFF.md` for the exact staged state.


**Partial restoration (2026-10-16).** The native `decoy-admin-reverse-ssh`
runit service is restored on the Devuan host using the existing
`/home/jeremy/.ssh/decoy_key` and strict host-key checking. The decoy publishes
`10.0.0.1:2222` bound to `wg-opdbus`, forwarding to its loopback reverse endpoint;
the tunnel terminates at the host's loopback SSH listener. WireGuard terminates
only on the decoy, never on this host. A fresh authenticated reverse connection
returned `jeremy` on `3tched` and matched the host's authoritative ED25519 key.
The external WireGuard-client leg was not exercised from this session.

The target layout is eth0 as OVS uplink, pub0 for public addressing (not
administrative SSH), two IPv4 addresses on `rtly-lungs` including 10.0.0.2,
and three service aliases on ovsbr0 for EMQX, OpenFlow and Qdrant. OVS packages
are installed, but no bridge/address/route ownership cutover has occurred.
NetworkManager remains active until a protected transfer; afterward it must
remain installed but disabled, for emergency recovery only.

Owner requests stopping at the application-bridge prerequisite. The existing
uplink service waits for `opdbus-rundirs`, which requires `/usr/local/bin/opblob`
and a prepared persisted catalog; that executable is absent. This is an
application-catalog bootstrap dependency, not proof that the bridge daemon must
run to create OVS. Do not bypass it or claim the network cutover is deployed.
Public recovery SSH is still enabled: a proposed private-only cutover was
reverted when its rollback helper could not execute on the `noexec` `/run`
mount. Move forward only with an approved, functioning rollback mechanism.

**Current status (2026-10-14).** The host moved from Artix to Devuan 6 (runit:
`/etc/sv/<svc>`, enabled via `/etc/runit/runsvdir/default`). Only `eth0`
(`188.68.58.237/22`, DHCP) is up; OVS, Incus, Xray and WARP are not installed yet.
Owner decision: the network lives on the **base system**, not in a golden release,
installed on a new machine by a network preinstall script that pulls these
sources from git. The rest of this page, including the GRE description below,
predates that move: GRE (`gre-decoy`) and `wgcf` were retired on 2026-10-04 (see
`deploy/runit/retired-services`), and `deploy/config/nftables.conf` says SSH
transport now uses `tun5`. Xray config/keys and the Xray Incus container are not
in git; the recovery disk keeps them under `received/271/root-271/`
(`etc/xray`, `var/lib/incus`, `etc/openvswitch`, `etc/nftables.conf`).

**Network-critical services.** OVS, uplink, DHCP and the session bus require
deliberate operator action; a learning or inventory pass never restarts them.

**Xray configuration policy — mandatory.** Xray's live config must exist only at
`/etc/xray/xray_config.json` inside the container. Never point Xray at
`/dev/shm/xray_config.json`, `/usr/local/etc/xray/config.json`, or any other
disk-backed live path. The static bootstrap config is materialized at boot; the
future model-generated generator replaces the same file atomically and reloads
through D-Bus. Models must not write or reload Xray directly.

**Owner-confirmed network topology (2026-09-13).** There is no WireGuard on this
server. WireGuard terminates at the decoy; identity is transported by WARP.
WireGuard types, plugin declarations, credentials or historical configuration
paths in source do not establish a server WireGuard interface or a requirement to
install one. Classify those surfaces by their decoy/server role before proposing
implementation or retirement. An unwired `wgcf` method is not evidence that the
actual WARP identity transport is broken.

**Owner decisions: decoy GRE, TLS and identity transport (2026-09-25; GRE since
retired).** The door stays `10.0.0.2:8090`: tonic with the internal `svc0`
certificate from the op-dbus local CA. The bridge no longer listens on
`127.0.0.1:8090` or `10.0.0.3:8090`, and it no longer serves a second browser
certificate. Browser-facing TLS terminates on the decoy at `10.0.0.1`. The decoy
mints the sealed identity blob, and WARP delivers it to the 8090 interceptor. The
oracle (OIA1) is the decoy's signed proof of the WireGuard peer, because
WireGuard exists only on the decoy. At the time, the decoy routed browser
traffic for `10.0.0.2` over `gre-host` with no WARP route in either main routing
table. The `gre-decoy` tunnel joined decoy and host: outer `188.68.58.237` ↔
`129.80.64.127`, inner `192.168.255.1/30` plus `10.0.0.2/32`, with `10.0.0.1` and
the WireGuard clients `10.0.0.5`–`10.0.0.7` routed via `192.168.255.2`. GRE
routing replaced NAT on that path: nftables is the firewall only, never a router,
so do not add DNAT, SNAT or masquerade for it. GRE is not encrypted, so anything
crossing it that is not TLS is readable on the path. MQTT stays on `10.0.0.3`,
aliased on `ovsbr0` beside the OpenFlow address `10.200.0.1`. Xray runs in its
container. op-web's page address is `http://10.0.0.4:8080`: plain HTTP outside
the TLS zone, with `10.0.0.4/32` on `gre-decoy` and routed from the decoy over
`gre-host`. The decoy issuer
(`/etc/systemd/system/op-decoy-issuer.service.d/browser-auth.conf` on the decoy,
not in this repo) hands out the IP door `https://10.0.0.2:8090` and allows the
`10.0.0.4` and `10.0.0.2` page origins; no domain name points browsers at the
decoy. Open on 2026-09-25: how the decoy-minted oracle reaches the interceptor
(the owner's direction then was over GRE). Until that lands, the deployed page
fetches OIA1 from `http://10.0.0.1:51888/identity/self`, and the door rejects a
request that carries no credential.

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
  → op-grpc-bridge (cognitive in process) / op-web
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
