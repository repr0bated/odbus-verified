# Tasks — `browser-lungs-handoff`

Branch `hackathon/kiroween`. Every commit ends with the trailer `Hackathon: kiroween`. One commit per task; proof of work per task in the kiroween repo (`docs/proof-of-work/browser-lungs-handoff/`). No tests or cargo checks until after deployment, then only on the owner's request (AGENTS.md). No architecture change. Owner OK before the deploy and before the first live hand-off.

- [ ] 0. Owner decision: plain HTTP or TLS on the handed socket (D2).
- [ ] 1. Read op-web's login path on `10.0.0.2:8080` and the lungs bootstrap (`runtime_lungs.rs`, `lungs_socket.rs`) and record exactly where the accepted socket is and how it reaches the host bridge (D4).
- [ ] 2. Lungs `AcceptBrowser() -> h` on the bootstrap object: host side answers only the identity's own bound connection; no waiter or no binding closes the socket.
- [ ] 3. Container bridge: waiters on lungs; serve each handed fd with `container_ui_door::router` (D2).
- [ ] 4. op-web hands the socket after the login (D3, D4).
- [ ] 5. Check why `kiroween` shows no `/run/mounts/opdbus/lungs` (read-only) and record it.
- [ ] 6. Golden deploy and live check with the owner's OK.
- [ ] 7. Remove the relay path (D5).
- [ ] 8. Proof of work and the `HANDOFF.md` update.
