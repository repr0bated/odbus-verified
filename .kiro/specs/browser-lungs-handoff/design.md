# Design — `browser-lungs-handoff`

Status: DRAFT for owner review, 2026-11-26. Requirements: `requirements.md` in this directory.

## Path

```
browser ──TCP──▶ 10.0.0.2:8080 on ovsbr0 (host, op-web: login)
                    │ login decides the identity (session_id)
                    ▼
host bridge: the identity's bound lungs connection (/run/opdbus/lungs/lungs.sock,
             peer-to-peer D-Bus, peer already identified and bound)
                    │ D-Bus reply carrying the browser socket as a Unix fd
                    ▼
container bridge (waiter on lungs) ──▶ axum UI door router serves that socket
                    │
                    ▼
the container's own 3tchedFS mount (call files, receipts)
```

After the hand-off the browser's packets go straight to the socket the container now owns. The host process that accepted it has closed its copy of the fd.

## Decisions

### D1. Transport: the existing lungs connection
- The container already connects to `lungs.sock` and binds; the host already knows it from peer credentials. No new socket, path or mount.
- A new method on the lungs bootstrap object (`org.opdbus.v1.LungsV1`), for example `AcceptBrowser() -> h` (a D-Bus Unix fd). The container keeps a few calls pending (waiters, as `op-tunnel-door` does today); the host answers one with the next socket for that identity.
- zbus carries Unix fds on Unix-socket connections; the lungs connection is one.

### D2. Container side: serve the handed socket with the existing router
- The container bridge turns the fd into a `tokio::net::TcpStream` and serves it with the same axum/tonic router as `127.0.0.1:8443` (`container_ui_door::router`). No new listener; `127.0.0.1:8443` stays.
- TLS: the host entry `:8080` is plain HTTP today (inside WireGuard). The handed socket therefore carries plain HTTP; the container serves the router without TLS on handed sockets. **Owner to confirm**, or the host entry becomes TLS terminated in the container (certificate for the entry name).

### D3. The login decides; the identity is the container
- The identity is established at login, and the identity is the container (owner, 2026-11-26). So the login already says which container the browser belongs to. The host hands the browser's socket to that container after the login. Nothing selects again: no address, no port, no second check.

### D4. Host side
- op-web (the login on `10.0.0.2:8080`) hands the accepted socket to the host bridge (same host, a Unix socket with SCM_RIGHTS or an existing in-process path; to be read from op-web's current code before choosing), which answers the identity's pending `AcceptBrowser`.
- If the identity has no pending waiter or no bound lungs connection, the socket is closed.

### D5. Removal after it works
`op-tunnel-door` byte-frame splice, the socat relay, `PortSelector`, `BrowserSelector::select` and the 20000+ ports go once hand-off is live.

## Found while reading (not changed)
- In `kiroween` right now `/run/mounts/opdbus` shows `plugin-blobs` and `tunnel-browser` but no `lungs` (seen with a read-only `lxc exec ls`, 2026-11-26). Provisioning adds the `lungs` disk; to check whether this container has it elsewhere or lacks it.
