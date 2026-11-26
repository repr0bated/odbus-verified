# Requirements — `openflow-browser-route`

> SUPERSEDED 2026-11-26 by `.kiro/specs/browser-lungs-handoff/`. Rejected by the owner: the architecture already works and is not changed. Do not implement.

Status: DRAFT for owner review, 2026-11-26. Branch `hackathon/kiroween`; every commit carries the trailer `Hackathon: kiroween`. Builds on `rovs-openflow` (release 348, `9bc3aaf`): the bridge creates, changes and deletes flows on `ovsbr0` itself, admitted and verified.

## Why

The browser has to reach its own container's UI door. The login already decides the identity, and the container is the identity, so the login already knows which container it is. Nothing needs to find the container again afterwards: no mux, no relay, no per-container listener port, no remote desktop. OpenFlow carries the browser's packets straight to the container's UI door, which is axum on `127.0.0.1:8443` inside the container.

This replaces the current tunnel path (`op-tunnel-mux`, `op-tunnel-door`, `BrowserSelector::select` picking by listener port, the socat relay, `PortSelector` and the per-identity browser ports 20000+).

## Facts this spec starts from (read in kiroween with `lxc exec`, read-only, 2026-11-26)

- The container's namespace has only `lo`, with `127.0.0.1/8` and `10.0.0.2/32`. It has no NIC and no routes.
- `net.ipv4.conf.all.route_localnet = 1`.
- `op-grpc-bridge` listens on `127.0.0.1:8443` (axum UI door) and `10.0.0.2:8090` (identity door).
- The container's internet access is already set up (BPF direct egress, HANDOFF 2026-11-23). This spec does not touch it.

## Glossary

- **Door address**: `10.0.0.2` is on the bridge `ovsbr0` on the host (an alias; `ovsbr0` is the LXD-visible bridge), not in the container (`docs/src/operations/network.md`). The container's `lo` also carries `10.0.0.2/32` and `127.0.0.1/8`; the bridge side is what connects to that loopback.
- **Route port**: one port on `ovsbr0` per identity, named from its `session_id`, that connects the bridge to that identity's container loopback. It has no address of its own.
- **Route flow**: a flow child of the `openflow` plugin that sends one identity's browser packets to its route port, rewritten to `127.0.0.1:8443`. A return flow carries the replies back.
- **Entry**: `10.0.0.2:8080` on `ovsbr0`, shared by every browser (the decoy forwards `10.0.0.1:8080` to it today).

## Requirements

### 1. The login writes the route
1.1 WHEN a login completes, THE system SHALL create that identity's route flows with `add_flow` through the sealed `openflow` plugin (admitted, compare-and-swap, read back, receipted). Nothing else creates route flows.
1.2 THE route SHALL be found from the identity's sled (`session_id`) at login. It SHALL NOT be looked up again from anything the browser sends afterwards.
1.3 WHEN the identity logs out, expires or is revoked, THE system SHALL delete its route flows with `delete_flow`. With no route flow, the identity's packets reach nothing. There is no fallback to another container.

### 2. Telling identities apart
2.1 EACH route flow SHALL match only header fields that are the same on every connection from one logged-in browser and differ between browsers. **Open for the owner:** which field. The candidate is the browser's source address as it reaches `ovsbr0`; source ports change with every connection and cannot be used.
2.2 THE match SHALL be refused at `add_flow` if it would also match another identity's existing route (collision refused, as for cookies).

### 3. Into the container's loopback
3.1 EACH identity SHALL have one route port on `ovsbr0`, named from its `session_id`, connecting the bridge to its container's loopback, with no address. Created and removed through the sealed plugins (`ovsdb_bridge`, `rovs_commands`, `rtnetlink`) only; never `ovs-*` and never `lxc exec`.
3.2 THE inbound route flow SHALL rewrite the destination to `127.0.0.1:8443` and output to the identity's route port; `route_localnet=1` in the namespace SHALL let the kernel deliver it to axum on loopback.
3.3 THE return path SHALL be proved as well as the inbound one: replies from `127.0.0.1:8443` leave through the route port and are rewritten back to the entry address the browser connected to.
3.4 TLS SHALL end at the container's UI door. Its certificate SHALL match the name the browser used for the entry.
3.5 THE container SHALL need no change beyond its route port: the UI door stays on `127.0.0.1:8443`; its internet access stays as it is.

### 4. One shared entry
4.1 THE host SHALL have one browser entry, port `8080`, for every identity. The socat relay, `PortSelector` and the per-identity ports 20000+ SHALL be removed.
4.2 THE login connection itself ends at the host's login service; only connections opened after the login follow the route.

### 5. Admission and records
5.1 EVERY route port and route flow change SHALL be an admitted call with a receipt, unchanged from `rovs-openflow` (identity AND grants AND sealed; final admission immediately before the write).
5.2 THE route flows SHALL be flow children of the `openflow` plugin, with derived cookies. No route is stored anywhere else: the switch and the sled are the only records.

### 6. Safety
6.1 NO change SHALL touch the host uplink flows or `NORMAL` on `ovsbr0` (those are `rovs-openflow` task 12).
6.2 THE first route flow written to the live switch, and the deploy that carries this feature, SHALL wait for the owner's OK.
6.3 No `ovs-*` command and no `lxc exec` data path.

### 7. Verification
7.1 Task 1's proof runs in a throwaway network namespace on the host, not in a live container, and is recorded as proof of work.
7.2 Tests run after deployment only, when the owner asks (AGENTS.md).
