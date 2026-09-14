# Host socket & network topology (live snapshot)

**Host:** Artix Linux, runit (not s6), post 2026-07-20/21 provider move  
**Verified:** 2026-07-22 against live `ip`, `ovs-vsctl`, `incus config device show`, `ss`, `/dev/shm/xray_config.json`, `/etc/op-dbus/network.conf`  
**Status:** Read-only investigation. Tree and older docs lag; **this file documents live reality**, then contrasts intent.

If more than a few days old, re-verify with:

```bash
ip -br addr; ip route
sudo ovs-vsctl show
sudo incus list -c nst4
for c in assistant cozo mail-3tched netmaker qdrant xray; do
  echo "=== $c ==="; sudo incus config device show "$c"
done
ss -ltnp; ss -lxnp | grep -E 'opdbus|ghostbridge|openvswitch|dbus'
sudo sv status ovsbr0-uplink ovsbr0-addr ovsbr0-svc-addr uplink-dhcp op-of-controller wg-3tched op-web op-grpc-bridge
```

---

## 1. Mental model (two layers)

| Layer | Role |
| --- | --- |
| **L2/L3 fabric** | Single OVS bridge `ovsbr0`; host L3 on internal ports `pub0` / `svc0`; physical `eth0` enslaved, unaddressed |
| **Service attachment** | Hybrid: shared UDS *intent* vs live **Incus `proxy` + disk mounts + mesh `tcpfwd`** |

There is **no** uniform “every container gets only `/run/ghostbridge/container.sock`” on this host today. Only `assistant` is pure shared-mount (and that directory is currently empty). Most services use **host-side TCP listeners** + **Incus proxy** or **runit tcpfwd** into the mesh / xray.

---

## 2. Host L3 map (live)

```text
Internet
   |
   v
eth0  (physical, enslaved, NO IPv4)
   |
ovsbr0  (OVS bridge, no host L3 on the bridge device itself)
   |
   +-- pub0   188.68.58.237/22   default via 188.68.56.1   (MAC = eth0 MAC)
   +-- svc0   10.200.0.2/24      host end of service segment
   +-- grpc   (internal port, link-local only)
   +-- veth*  (xray container NIC peer)
   |
xray eth0: 10.200.0.1/24  and  10.0.0.2/24

WireGuard mesh (separate L3, NOT on ovsbr0):
   3tched  100.69.0.254/16   listen :51821
```

### Routes (host)

| Destination | Device | Notes |
| --- | --- | --- |
| default | `pub0` via `188.68.56.1` | Public egress |
| `10.200.0.0/24` | `svc0` | Service / xray segment |
| `100.69.0.0/16` | `3tched` | Mesh — **must not** land on `ovsbr0` |
| `1.1.1.1/32` | `3tched` | Mesh-pinned |

### Hard invariants (boot helpers enforce)

- **Never** put `10.200.0.1` on the host (`xray` owns it).
- **Never** install `100.69.0.0/16` on `ovsbr0` (WG owns mesh).
- Bridge device never holds public/service addresses; `pub0`/`svc0` do.
- Static addressing via `/etc/op-dbus/network.conf` (no DHCP fight on enslaved eth0).

Canonical config: `/etc/op-dbus/network.conf`  
Helpers: `/usr/local/libexec/3tched/{ovsbr0-uplink-up,ovsbr0-addr-up,ovsbr0-svc-addr-up,uplink-dhcp-up}`

---

## 3. Runit boot graph (network + control plane)

```text
opdbus-rundirs
  -> uplink-dhcp          # snapshot pub0/network.conf into /run/opdbus/uplink-migration.env
  -> ovsdb-server
  -> ovs-vswitchd         # seed bridge + internal ports
  -> ovsbr0-uplink        # bridge + eth0 up; may call op-ovsbr0-setup
  -> ovsbr0-addr          # pub0 + svc0 L3 + default route
  -> ovsbr0-svc-addr      # reaffirm; strip mesh-on-bridge mistakes
  -> op-of-controller     # OpenFlow listen 127.0.0.1:6653
  -> wg-3tched            # mesh iface after host L3
  -> op-grpc-bridge / op-web / op-cognitive-mcp  (each waits on ovsbr0-addr or rundirs)
```

Ready stamps: `/run/opdbus/runit-ready/{opdbus-rundirs,uplink-dhcp,ovsbr0-uplink,ovsbr0-addr,ovsbr0-svc-addr}`

### Mesh port forwarders (`fwd-*`)

After `3tched` has `100.69.0.254`, each enabled `fwd-*` runs `tcpfwd.py`:

| Service | Listen (mesh) | Connect |
| --- | --- | --- |
| `fwd-8444` | `100.69.0.254:8444` | `10.200.0.1:8444` (xray VLESS/xhttp) |
| `fwd-8091` | `100.69.0.254:8091` | `10.200.0.1:8091` |
| `fwd-8090` | *defined under `/etc/runit/sv` but not always enabled* | `10.200.0.1:8090` |
| `fwd-8081` | `100.69.0.254:8081` | `127.0.0.1:8081` (netmaker API via host loopback proxy) |
| `fwd-6333` / `fwd-6334` | mesh | `10.200.0.1:6333/6334` (xray dokodemo → qdrant host proxies) |
| `fwd-3003` | mesh | `10.200.0.1:3003` → cognitive MCP |
| `fwd-28082` | mesh | `10.200.0.1:28082` |

`uds-assistant`: `10.200.0.2:8091` ← `udsfwd.py` ← `/var/lib/assistant-controlplane/http.sock`

---

## 4. Host TCP / UDS listeners (what actually binds)

### Control plane (host processes)

| Bind | Process | Role |
| --- | --- | --- |
| `0.0.0.0:8080` | `op-web-server` | HTTP GUI + API (public path via xray SNI → `10.200.0.2:8080`) |
| `0.0.0.0:8090` | `op-grpc-bridge` | gRPC binary + gRPC-Web |
| `127.0.0.1:6653` | `op-of-controller` | OpenFlow controller |
| `10.200.0.2:3003` | `op-cognitive-mcp` | MCP HTTP/SSE |
| `10.200.0.2:50052` | `op-cognitive-mcp` | Cognitive gRPC |
| `/run/opdbus/grpc.sock` | `op-grpc-bridge` | Unix gRPC (live path; see §6) |
| `/run/opdbus/session-bus.sock` | session bus | SESSION D-Bus |
| `/run/dbus/system_bus_socket` | dbus | SYSTEM bus |
| `/run/openvswitch/db.sock` | ovsdb-server | OVSDB |

### Incus proxy–backed host binds (see §5)

| Bind | Backend container |
| --- | --- |
| `188.68.58.237:{25,80,143,465,587,993}` | `mail-3tched` |
| `127.0.0.1:18080` | `mail-3tched` web local |
| `10.200.0.2:8081`, `127.0.0.1:8081` | `netmaker` API |
| `10.200.0.2:8083`, `127.0.0.1:8083` | `netmaker` broker |
| `188.68.58.237:8081`, `188.68.58.237:8083` | also observed on public IP (extra bind path) |
| `10.200.0.2:6333`, `10.200.0.2:6334` | `qdrant` |
| `127.0.0.1:50053` | `cozo` → host `127.0.0.1:50052` (chat/cozo chain) |

### Not on host (yet)

| Service | Where it lives |
| --- | --- |
| Prometheus `:9090` | **inside** `netmaker` only (`*:9090`) |
| Grafana `:3000` | **inside** `netmaker` only (`*:3000`) |

No host Incus proxy or `fwd-*` for prom/graf as of this snapshot. Do not expose until this topology doc is accepted.

---

## 5. Containers: attachment mode (live devices)

| Container | NIC | Shared UDS mount | Incus TCP proxy | Notes |
| --- | --- | --- | --- | --- |
| **xray** | **yes** — `eth0` bridged on `ovsbr0` (`10.200.0.1`, `10.0.0.2`) | `/run/opdbus` disk; config bind of `/dev/shm/xray_config.json` | none | Sole public SNI terminator on fabric; only container with a real NIC |
| **assistant** | no | `/run/ghostbridge` → host `/run/ghostbridge` (**empty dir**) | none | Controlplane disk + identity mounts; pure shared-socket *shape*, not active sock |
| **qdrant** | no | `/run/ghostbridge` (empty) | `6333`/`6334` host←container loopback | Hybrid |
| **netmaker** | no | none | `8081`/`8083` on `svc0` + loopback; egress proxy to `10.0.0.2:10809` | systemd inside CT (prom/graf too) |
| **mail-3tched** | no | none | public mail/web ports on `pub0` IP; SMTP egress via mesh `100.69.0.1:2525` | Host-side mail, not Oracle decoy |
| **cozo** | no | none | egress via xray `10.200.0.1:10809`; host proxy `50053→50052` | data volume for op-dbus |

**Incus networks:** only `ovsbr0` is used by an instance (`xray`). No managed Incus bridge for the app set.

### Device detail (summary)

**assistant** — disk only: `ghostbridge-socket`, identity, controlplane runtime, wayland/x11.

**xray** — `nic` eth0/`ovsbr0`; disks: opdbus runtime, `/run/opdbus`, read-only xray conf from SHM.

**qdrant** — proxies:

- host `10.200.0.2:6333` → container `127.0.0.1:6333`
- host `10.200.0.2:6334` → container `127.0.0.1:6334`

**netmaker** — proxies:

- host `10.200.0.2:8081` / `127.0.0.1:8081` → container `127.0.0.1:8081`
- host `10.200.0.2:8083` / `127.0.0.1:8083` → container `127.0.0.1:8083`
- container egress `127.0.0.1:3128` → `10.0.0.2:10809` (xray HTTP egress)

**mail-3tched** — proxies on `188.68.58.237` for 25/80/143/465/587/993; local web `127.0.0.1:18080`; egress SMTP proxy to mesh.

**cozo** — egress proxy to xray; `op-chat-host` host bind `127.0.0.1:50053` → `127.0.0.1:50052`.

---

## 6. Shared socket (fixed 2026-07-22)

### Model

- **Host-local UDS:** `/run/opdbus/grpc.sock` (`ZEROCLAW_UNIX_SOCKET`) — op-web, zbusctl, host tools.
- **Shared container UDS:** `/run/ghostbridge/container.sock` (`GHOSTBRIDGE_SOCKET_PATH` / `SHARED_CONTAINER_SOCKET`) — bind-mounted into NIC-less CTs (`assistant`, `qdrant`).
- `op-grpc-bridge` serves the **same** gRPC route set on both paths (dual listen). `createunixsocket` registers `(name, ports)` metadata against the shared path; it does not open per-container sockets.
- Identity on the UDS path: peer cred + identity sled headers (same interceptor as xray-injected path).

### Live after fix

| Path | Role |
| --- | --- |
| `/run/ghostbridge/container.sock` | Shared fabric for containers |
| `/run/opdbus/grpc.sock` | Host-local gRPC |
| `/run/opdbus/session-bus.sock` | SESSION bus |

Env (canonical):

```text
ZEROCLAW_UNIX_SOCKET=/run/opdbus/grpc.sock
GHOSTBRIDGE_SOCKET_PATH=/run/ghostbridge/container.sock
```

`opdbus-rundirs` creates `/run/ghostbridge` at boot. Re-verify with:

```bash
ss -lxnp | grep -E 'ghostbridge|opdbus/grpc'
sudo sv status op-grpc-bridge
```

### Still hybrid (proxies remain — non-identity workloads only)

Incus TCP proxies and mesh `fwd-*` still exist for **mail / netmaker / qdrant / cozo side channels**. Those are **not** the identity/control-plane attachment model.

**Do not confuse Incus `proxy` devices with identity attachment.** Registration / identity_sled containers use:

| Mechanism | Role | Status |
| --- | --- | --- |
| **Shared host UDS** `/run/ghostbridge/container.sock` | Single control surface (host binds; CT gets the dir bind-mounted) | **Listen live** (op-grpc-bridge dual-bind) |
| **Host / CT loopbacks** | Services bind `127.0.0.1` (loopback), not a CT NIC; not an Incus `proxy` device | Pattern for NIC-less CTs |
| **Disk bind of identity dir** | `/var/lib/opdbus-runtime/identities/<role>` → CT `/opt/run-mounts/identity` | Dir layout present; not full provision |
| **btrfs fstorage** | Per-identity sealed image; `btrfs device add` onto seed rootfs (preferred cleaner persistence) | Keep this model. **Delete order:** `btrfs device delete` fstorage from seed **before** `incus delete` (provision `--recreate` does this). Deleting the CT while the loop is still in the array is how the host D-states. |
| **No NIC** | Only **xray** has a real NIC on `ovsbr0` | Live |
| **No Incus proxy for identity** | Proxy devices remain hybrid hangover for mail/netmaker/qdrant/cozo only | Live hybrid |

### Terminology: Heartbeat (do not over-claim)

| Term | What it means | Status |
| --- | --- | --- |
| **ComponentRegistry `Heartbeat`** | gRPC RPC on `operation.registry.v1` (`registry.proto`): lease token + liveness; server marks **stale** after missed windows; can demand re-register | **Spec + server handler in tree** (`op-grpc-bridge` `ComponentRegistry.Heartbeat`). **Not** an identity-CT client loop, **not** a runit service, **not** deployed as the live identity control path |
| **Chat stream heartbeat** | Unrelated `ChatFrame` keepalive on chat streams | Separate concept — do not conflate with registry Heartbeat |
| **identity_sled `last_seen_at` / `touch_session`** | Sled bookkeeping when a session is touched | Different from ComponentRegistry Heartbeat |

**Intended (not implemented/deployed):** NIC-less identity CT (or host-side agent) would periodically call ComponentRegistry **`Heartbeat`** over the **shared UDS / host gRPC**, presenting its lease. Host is the lease authority. Until that client path exists, say **“Heartbeat (registry lease RPC — planned)”**, not “host runs heartbeat for containers” as live fact.

### Identity containers (registration / identity_sled)

Correct provision (existing WG keys preferred) — **target sequence**, not all live:

1. `session_id = derive_session_id(wg_pubkey)` (or PSK-aware Argon2 form) — **container name IS the UUID**.
2. `identity_sled.provision_container` with that pubkey → Incus create on **btrfs seed** (default pool), profiles without NIC, register sled.
3. Create per-session **fstorage** image under `/var/lib/opdbus-runtime/identities/<role>/fstorage.img`; `attach_btrfs_device` → `btrfs device add` onto seed mount.
4. Register on shared socket: `createunixsocket` / `shared_unix_socket` with `name = session_id` (metadata only; host already owns `container.sock`).
5. **Later:** ComponentRegistry **Register** + **Heartbeat** over shared UDS (lease / stale) — API present; **CT client not implemented/deployed**.

**Two identities on this host (existing keys, not rotated):**

| Role | WG pubkey (existing) | Mesh IP | session_id (blake3 derive) |
| --- | --- | --- | --- |
| jeremy | `GEMLT/+I…` | `100.69.0.2` | `f036f8d8-aabb-c5f2-49c9-18dac19f41ea` |
| chatbot | `VaRh9EU…` | `100.69.0.10` | `bea37ecb-92be-197c-660f-09e806f1a34f` |

When provisioned correctly, `incus list` shows those **two UUID names** (plus infra CTs). **Live today:** only named infra CTs (`assistant`, `cozo`, …) — UUID identity CTs **not** created yet.

**Memory leaf:** one Cozo-backed store owns durable memory (Cozo `:put` leaf ops / cognitive memory). Identity CTs do **not** each run a separate memory server — they use the shared control plane toward that single memory leaf.

**Registration surface:** `op-identity` registration helpers + gRPC `RegistrationService` / privacy verify should end in `identity_sled.provision_container` + shared-socket register — not vault JSON alone and not Incus proxy plumbing.

---

## 7. Public / identity path (Oracle decoy → host)

```text
Client
  |  DNS (NextDNS profile) → Oracle decoy A 129.153.134.63
  v
Oracle / mesh peer (WG endpoint :443, AllowedIPs 100.69.0.0/16)
  |
  v  WireGuard mesh
Host 3tched 100.69.0.254
  |
  v  fwd-8444: 100.69.0.254:8444 → 10.200.0.1:8444
xray (VLESS xhttp-in on 10.200.0.1:8444)
  |  SNI / domain route from /dev/shm/xray_config.json
  +-- dashboard|registration.{3tched,ghostbridge}.tech → 10.200.0.2:8080  (op-web)
  +-- assistant.*                                      → 10.200.0.2:8090  (grpc-bridge)
  +-- api.*                                            → 10.200.0.2:8081  (netmaker API proxy)
  +-- broker.*                                         → 10.200.0.2:8083
  +-- qdrant.*                                         → 10.200.0.2:6333
  +-- default                                          → blackhole
```

**Xray live config location (mandatory):** `/dev/shm/xray_config.json` only (bind-mounted into the container). Never disk-backed live path under `/etc/xray`.

**Runit supervises the mount:** `xray-config-mount` (`/etc/runit/sv/xray-config-mount`, helper `/usr/local/libexec/3tched/xray-config-mount-up`). File bind mounts pin an *inode*; host `cp`/`mv` replaces leave the CT stale. The service polls (default 5s), compares host vs CT md5, re-attaches Incus device `xrayconf`, and restarts xray in the CT.

Additional xray inbounds on `10.200.0.1` (dokodemo/http) feed host `10.200.0.2` ports for mesh/side channels: 8090, 8091, 6333, 6334, 8081, 3003, 8443, 10809 (HTTP egress).

**Mail** is **not** on the decoy path for ingress: Postfix/Dovecot ports are Incus-proxied on **host public** `188.68.58.237`. Registration SMTP from `op-web` uses host-reachable `mail.3tched.com:587` (env in `/etc/op-dbus/environment`).

**BASE_URL** for magic links: `https://dashboard.3tched.com` (SNI → op-web).

---

## 8. Common exit (xray) + optional WARP (`wgcf`)

**Xray is the single CT internet exit.** Inbound `egress-proxy` on `10.200.0.1:10809` routes to outbound `warp` (freedom + `sockopt.mark = 0x51820`). Unmarked host/mesh/SNI paths are untouched.

| Tag | Role |
| --- | --- |
| inbound `egress-proxy` | Common exit door for containers |
| outbound `warp` | Marked freedom → host policy table when `wgcf` is up |
| outbound `direct` | Unmarked; main table / `pub0` |

### WARP names (role-encoded — do not mix)

| Host | Interface | Conf | Role |
| --- | --- | --- | --- |
| **Hypervisor** (this Artix box) | **`wgcf-egress`** | `/etc/wireguard/wgcf-egress.conf` | CT privacy **egress** (xray common exit → mark `0x51821` → table 51820) |
| **Oracle decoy** | **`wgcf-ingress`** | `/etc/wireguard/wgcf-ingress.conf` | Decoy edge **ingress** role (public path); not the hypervisor exit |

Bare `wgcf` is a **stub** on both machines pointing at the role name — do not `wg-quick up wgcf`.

- **IP is not ours** — Cloudflare assigns Address; conf may be shared/reused from decoy.
- **Trick:** interface `FwMark = 0x51820` = tunnel **underlay** on main/`pub0`; payload exit mark **`0x51821`** = table 51820 → `wgcf-egress`. Different marks on purpose.
- **Do not enslave** into `ovsbr0` (L3, like `3tched`).

```text
# hypervisor
wg-quick up wgcf-egress
ip rule  # fwmark 0x51821 lookup 51820
```

## 9. OpenFlow (future — tag routing)

- Controller: `op-of-controller` → `127.0.0.1:6653`.
- Bridge `fail_mode: standalone`.
- Live flows today are effectively **`priority=0 actions=NORMAL`** — L2 learning only.
- **Do not treat OpenFlow as broken for the shared-socket work.** OpenFlow is reserved for **future identity/tag routing** (governed flows keyed by registration / uid / service tag once model-generated xray + OF policy land). Socket fabric correctness does not depend on OF policy today.

---

## 9. WireGuard mesh (`3tched`)

- Host address: `100.69.0.254/16`, listen `51821`.
- Notable peer: endpoint `129.153.134.63:443` (Oracle decoy side), AllowedIPs `100.69.0.0/16`.
- Other peer example: `100.69.0.2/32` (remote endpoint observed).
- Service `wg-3tched` explicitly deletes any `100.69.0.0/16` route that appears on `ovsbr0`.

---

## 10. Inside netmaker (observability island)

| Process | Listen | Host exposure |
| --- | --- | --- |
| netmaker API | `*:8081` | Yes — Incus proxy + mesh fwd |
| netmaker MQTT/broker stack | 8083/8084/… | Partial (8083 proxied) |
| **prometheus** | `*:9090` | **None on host** |
| **grafana** | `*:3000` | **None on host** |
| node-ish metrics | `127.0.0.1:9100` | none |

Prom/graf were installed in-rootfs under systemd **inside** the CT. Topology understanding is a prerequisite before adding host proxies or SNI routes.

### Break-glass Netmaker ACL recovery

`deploy/netmaker/reenable-all-nodes-acl.sh` restores the default
`3tched.all-nodes` ACL through the host-loopback API. This is an emergency
direct-REST helper, not the normal D-Bus control path. It sends a `PUT` for a
wildcard source-to-destination, all-protocol policy and marks it enabled and
default. Use it only after confirming that broad all-node connectivity is the
intended recovery state.

Preconditions and constraints:

- Run from the host and first confirm that `127.0.0.1:8081` still maps to the
  Netmaker API; re-verify the Incus devices because this topology is dated.
- Supply `NETMAKER_MASTER_KEY` (or `MASTER_KEY`) through a protected
  environment. Do not paste or log the key.
- `NETMAKER_API_BASE` overrides the default `http://127.0.0.1:8081`.
- The script's automatic key lookup currently names the container `NetMaker`,
  while the live inventory in this document uses `netmaker`. Do not rely on
  that fallback until the name is verified or the helper is corrected.
- The helper prints the API response but performs no read-back verification or
  rollback. Inspect the response and independently confirm the resulting ACL.

With those preconditions satisfied:

```bash
./deploy/netmaker/reenable-all-nodes-acl.sh
```

Do not copy this direct API/master-key pattern into routine automation. Add
future ACL lifecycle support to the schema-driven `netmaker` plugin instead.

---

## 11. Intent vs live (checklist)

| Topic | Intent (code/docs) | Live |
| --- | --- | --- |
| Supervisor | runit on new host | runit (`sv`) ✓ |
| Single OVS bridge | `ovsbr0` | ✓ |
| Host L3 on internal ports | pub0/svc0 | ✓ |
| xray only NIC on fabric | yes | ✓ |
| Shared `container.sock` for all CTs | yes | **listen live**; identity CTs still not on it as UUID sessions |
| Identity CTs = UUID session_id, no NIC | yes | **not provisioned yet** (vault/mesh only) |
| Identity attachment = UDS + loopback, not Incus proxy | yes | model; do not add proxy for registration |
| ComponentRegistry **Heartbeat** (lease) | planned identity liveness | **RPC in tree**; **no identity client deployed** |
| Only xray has NIC | yes | ✓ |
| Memory = one leaf (Cozo / cognitive) | yes | `cozo` CT + host cognitive path |
| Incus proxy | bulk services only | mail/netmaker/qdrant/cozo still hybrid |
| OpenFlow policy fabric | future tag routing | standalone + NORMAL (by design until tag routing) |
| Public GUI | decoy SNI → xray → op-web | ✓ (dashboard/registration) |
| gRPC public | SNI assistant → 8090 | ✓ |
| Mail | host CT | ✓ public IP proxies |
| Prometheus/Grafana | TBD host path | **CT-local only** |

---

## 12. Related tree / stale docs

| Path | Note |
| --- | --- |
| `docs/operations/ghostbridge-incus-ovs-architecture.md` | Older dinit/ens3/socket-port design; historical intent |
| `docs/network-address-table.md` | **Stale** (old IPs, incusbr0, 10.88.88.x) — prefer this file |
| `docs/architecture/privacy-network-architecture.md` | Intent / privacy chain |
| `docs/src/operations/network.md` | Book ops page — keep aligned with this snapshot |
| CLAUDE.md “Transport & identity” | Matches hybrid live note |

---

## 13. Safe change policy (from this investigation)

1. **Re-read live devices** before adding any proxy, `fwd-*`, or SNI route.
2. Prefer **documenting + minimal** exposure for prom/graf only after deciding: mesh-only vs SNI vs loopback.
3. Do not put addresses on `ovsbr0` bridge device; do not steal mesh routes onto OVS.
4. Do not point xray at disk config; regenerate `/dev/shm/xray_config.json` and reload via approved control path.
5. Shared sock **listen is live** (`/run/ghostbridge/container.sock`). New identity registrations attach via **UDS + loopback**, never Incus proxy.
6. Provision identities via `identity_sled.provision_container` (existing WG keys); expect **UUID container names** in `incus list`. Attach fstorage with `btrfs device add` onto the RO seed, not subvolume layers.
7. Do not put a NIC on anything except **xray**.
8. **Heartbeat** = ComponentRegistry lease RPC (terminology + server handler in tree). Do not document it as live identity control until a client path is implemented and deployed. Do not confuse with chat-stream heartbeats.
