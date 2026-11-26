# Design — `openflow-browser-route`

Status: DRAFT for owner review, 2026-11-26. Requirements: `requirements.md` in this directory.

## Path

```
browser ──▶ host entry :8080 (shared) ──login──▶ login service on the host
                                               │  identity known (sled session_id)
                                               ▼
                               sealed calls: route port + route flows (admitted, receipted)

browser (after login) ──▶ :8080 ──▶ ovsbr0 route flow (match: the browser, see D3)
                                       │ set dst 127.0.0.1:8443, output:<route port>
                                       ▼
                         route port "s<session_id prefix>" in the container namespace
                                       │ route_localnet=1
                                       ▼
                         axum UI door 127.0.0.1:8443 (TLS ends here) ──▶ container's 3tchedFS mount
replies ──▶ route port ──▶ return flow (set src back to the entry) ──▶ browser
```

## Decisions

### D1. The login is the only lookup
The login service already knows the identity and its sled. At login it makes the sealed calls that create the route port (if absent) and the route flows; at logout, expiry or revocation it deletes the flows. After login nothing inspects the traffic: no mux, no relay, no second authentication.

### D2. Route port: an OVS internal port, named from `session_id`, moved into the namespace
- `lo` cannot be an OVS port (the kernel datapath refuses loopback devices), so each identity gets one OVS internal port, moved into its container namespace, with no address.
- Name: `s` + the first 14 hex characters of the sled `session_id` (interface names are at most 15 bytes). A collision between two sessions is refused.
- Created and removed only through sealed plugins (`ovsdb_bridge` for the Interface row, `rtnetlink` to move it into the namespace and bring it up). Never `ovs-*`, never `lxc exec`.
- The container is otherwise unchanged; its internet (BPF egress) does not use this port.

### D3. What the flow matches (OPEN, owner decision)
A flow sees only headers. The match must be stable across one browser's connections and differ between browsers:
- **Candidate A: browser source address** at `ovsbr0` (for example its WireGuard or WARP peer address). Works with one shared `:8080` only if every logged-in browser has its own address; browsers behind one NAT would collide (refused by requirement 2.2).
- Source ports cannot be used (new on every connection).
The spec cannot be finished until this is chosen.

### D4. Into loopback
Inbound flow: `match <D3>, tcp, tp_dst=8080 → set_field ip_dst=127.0.0.1, set_field tcp_dst=8443, set_field eth_dst=<route port MAC>, output:<route port>`. With `route_localnet=1` the kernel accepts a 127.0.0.1 destination arriving on the route port and delivers it to axum on loopback. Return: the namespace needs a route back through the route port for replies sourced from 127.0.0.1; the return flow rewrites the source to the entry address and port. Task 1 proves both directions before any of this is written for real.

### D5. Flows are `openflow` flow children
Route flows are created with `add_flow` and carry derived cookies (`rovs-openflow` D2), so they appear with every other flow, are compare-and-swapped and read back, and need no separate record. Current `rovs-openflow` validation refuses `set_field` targets it cannot translate; the translator gains `ip_dst`/`ip_src`/`tcp_dst`/`tcp_src`/`eth_dst` set-fields if missing.

### D6. One shared entry
`:8080` on the host is the only browser listener. `op-tunnel-mux`'s browser path, `BrowserSelector::select`, the socat relay, `PortSelector` and the 20000+ ports are removed once the route path is live.

## Out of scope
Fonts under `/assets` and Administrative mode (own specs). The host uplink and `NORMAL` (`rovs-openflow` task 12). Container internet (already set up).

## Risks
- D3 is undecided; if browsers do not have distinct addresses, the match needs another header and the design changes.
- Return path from 127.0.0.1 out of a non-loopback interface: the kernel's rules for it are proved in task 1 before relying on them.
- TLS name: the certificate the container's door presents must match the entry name the browser uses.
