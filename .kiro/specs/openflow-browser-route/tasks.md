# Tasks — `openflow-browser-route`

> SUPERSEDED 2026-11-26 by `.kiro/specs/browser-lungs-handoff/`. Rejected by the owner: the architecture already works and is not changed. Do not implement.

Branch `hackathon/kiroween`. Every commit ends with the trailer `Hackathon: kiroween`. One commit per task; proof of work per task in the kiroween repo (`docs/proof-of-work/openflow-browser-route/`). No `ovs-*` commands, no `lxc exec` data path. No tests until after deployment, and then only on the owner's request (AGENTS.md). Nothing is written to the live switch and nothing is deployed without the owner's OK.

- [ ] 0. Owner decision: what the route flow matches (design D3).
- [ ] 1. Prove the route in a throwaway namespace on the host: an internal-port-like device with no address, `route_localnet=1`, packets rewritten to `127.0.0.1:8443` delivered to a listener on loopback, and the replies' return path. Record the commands and output as proof.
- [ ] 2. Update kiroween `docs/handoff/identity-aware-mux` to point here (superseded: no mux) and align `docs/wiki/openflow-ui.md`.
- [ ] 3. Route port per identity: named from `session_id` (D2), created and moved into the namespace through sealed plugins; removed on logout/expiry/revocation.
- [ ] 4. Route flows per identity (D4, D5): translator set-fields if missing; inbound and return flows created at login and deleted at logout through `add_flow`/`delete_flow`; collision refusal (requirement 2.2).
- [ ] 5. Login service writes the route at login (D1); no other lookup.
- [ ] 6. One shared `:8080` entry; remove the socat relay, `PortSelector`, `BrowserSelector::select` browser path and the 20000+ ports (D6).
- [ ] 7. Golden deploy and live check. Owner OK required for the deploy and for the first route flow written to the switch.
- [ ] 8. Proof of work and the `HANDOFF.md` update.
