# Requirements — `browser-lungs-handoff`

Status: DRAFT for owner review, 2026-11-26. Branch `hackathon/kiroween`; every commit carries the trailer `Hackathon: kiroween`. Supersedes `.kiro/specs/openflow-browser-route/` (rejected by the owner: the architecture already works and is not changed).

## Why

The browser must reach its own container's UI door (axum, inside the container on `127.0.0.1:8443`) directly: a pass-through, not a relay, not a remote desktop. The login already decides the identity and the container is the identity, so nothing looks the container up again.

The pieces already exist and work, and are not changed:
- **The lungs socket** `/run/opdbus/lungs/lungs.sock` (host), bind-mounted read-only into each container at `/run/mounts/opdbus/lungs` (`deploy/provisioning/30-provision-container.sh`). It is peer-to-peer D-Bus (zbus, `crates/op-grpc-bridge/src/runtime_lungs.rs`). The host identifies the connecting container from the socket's peer credentials and the committed sled, and binds it (`runtime_lungs_identity.rs`, `lungs_socket.rs`): the connection is already the identity's.
- **The pattern** of handing a socket across namespaces over a Unix socket, as `ct-egress-bpf` does (`crates/op-network/bpf/ct_egress_loader.c`: connected fd passed over a socketpair, then `splice(2)`).
- **The container**: only `lo` (`127.0.0.1/8`, `10.0.0.2/32`); the door on `127.0.0.1:8443`; internet by eBPF egress.
- **The host entry**: `10.0.0.2:8080` on `ovsbr0` (op-web; the decoy forwards `10.0.0.1:8080` to it).

## Glossary

- **Hand-off**: the host passes the browser's accepted TCP socket (the file descriptor itself) to the identity's container over its bound lungs connection, as a D-Bus Unix fd.
- **Waiter**: a pending lungs call from the container that receives the next handed-off socket.

## Requirements

### 1. The socket is handed, not relayed
1.1 AFTER the login has decided the identity, THE host SHALL pass the browser's socket to that identity's container over the identity's bound lungs connection, as a D-Bus Unix fd. The host SHALL NOT read, copy or relay the bytes afterwards.
1.2 THE host SHALL hand a socket only to the lungs connection bound to that identity. No bound connection: the socket is closed. No fallback to another container.
1.3 THE container SHALL serve the handed socket with its existing axum UI door (same router as on `127.0.0.1:8443`).

### 2. No second lookup
2.1 THE identity SHALL come from the login only. Nothing the browser sends after the login, and no address or port, SHALL select the container.
2.2 THE identity is established at login and the identity is the container: the login alone decides which container receives the socket.

### 3. Nothing else changes
3.1 The lungs socket, its peer identification and binding, `ovsbr0`, `10.0.0.2`, the container's loopback and its eBPF egress SHALL stay as they are.
3.2 The UI door's admission SHALL stay as it is: every call is a write to the container's own 3tchedFS call file.

### 4. Removal
4.1 ONCE hand-off works live, the relay path (`op-tunnel-door` byte frames, the socat relay, `PortSelector`, `BrowserSelector::select`, the per-identity ports 20000+) SHALL be removed.

### 5. Safety
5.1 No `ovs-*` command, no `lxc exec` data path, no new listener in the container, no new NIC.
5.2 The deploy and the first live hand-off SHALL wait for the owner's OK.
5.3 Tests run after deployment only, on the owner's request (AGENTS.md).
