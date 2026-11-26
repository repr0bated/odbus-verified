# Design — `rovs-openflow`

Status: DRAFT for owner review, 2026-11-26. Requirements: `requirements.md` in this directory.

## Overview

```
zcall / UI ─▶ sealed openflow method ─▶ Runtime::call (identity, grants, sealed; receipt)
                                           │
                                           ▼
                              runtime_network.rs  (in the bridge process)
                                           │  per-bridge lock: read ▸ compare ▸ final admission ▸ write ▸ read back
                                           ▼
                         rovs VConn, OpenFlow 1.5 ─▶ unix:/var/run/openvswitch/ovsbr0.mgmt ─▶ OVS
                                           │
                                           ▼
                flow children: D-Bus objects + 3tchedFS entries, projected from the switch's flow table
```

The bridge talks to OVS directly. There is no controller process between them, no D-Bus hop, no permit.

## Decisions

### D1. Transport: VConn over the bridge's `.mgmt` socket
- `rovs_openflow::VConn::connect(&rovs_transport::Address::Unix(".../ovsbr0.mgmt"))`, hello at `Version::Of15`.
- One long-lived VConn per managed bridge, owned by the network runtime in the bridge, reconnecting with `rovs_transport::Reconnect`. Echo replies keep it alive.
- `op-network`'s `OpenFlowClient::connect` gains a Unix address; it stays the one wrapper around VConn.
- To verify in task 1: `rovs_transport::Address` has a Unix variant that VConn accepts (docs list Unix, TCP, TLS streams).

### D2. Flow identity: plugin-assigned cookies, derived, not stored
- Cookie = `0x33444348_xxxxxxxx`: the high 32 bits are the fixed 3tched prefix (as in the archived fabric's NORMAL cookie `0x3344434800000001`); the low 32 bits are the first 32 bits of SHA-256 over the canonical `(bridge, table, priority, match)`.
- The cookie is a function of the flow's identity, so no counter or registry is stored: re-creating the same flow gives the same cookie, and the switch remains the only record.
- A collision (two different identities, same low bits) is refused at `add_flow` with a clear error.
- Deletes and modifies use cookie plus `cookie_mask = 0xffffffffffffffff` and `OFPFC_*_STRICT` on the identity.

### D3. Flow children are projections of the switch
- On connect, and after every committed mutation, the runtime reads the bridge's flows (OF1.5 flow-description multipart, or flow-stats) and projects each into:
  - a D-Bus child object `/org/opdbus/v1/plugins/openflow/bridges/<bridge>/flows/<cookie>`;
  - a 3tchedFS entry `plugins/openflow/bridges/<bridge>/flows/<cookie>/value.json`.
- Changes are signalled, not polled: flow mutations go through this runtime, so it emits `InterfacesAdded`/`InterfacesRemoved` and `PropertiesChanged` on commit. Counters come from OF1.5 flow monitoring (`FlowMonitorRequest` in rovs) where OVS reports them, otherwise they refresh on each read.
- Nothing is cached beyond the live projection: a reconnect drops and rebuilds the children from the switch.

### D4. Schema: one sealed child schema
- The `openflow` plugin's sealed schema gains a `flow` child type whose JSON Schema is generated from the Rust `FlowEntry` (schemars), every field annotated `x-oscal-subid` (`obs.network.openflow.flow.<field>@v1`).
- The introspection XML for the plugin lists child `<node>`s; each child's introspection carries `org.opdbus.v1.PluginSchema` for the `flow` type, like plugins do today.
- `op-blob` descriptor synthesis covers the child type, so gRPC reflection shows it; json-render renders from it.

### D5. Mutation = admitted compare-and-swap in one place
1. Take the per-bridge lock.
2. Read the target flow by cookie (strict identity).
3. Compare with `expected` (absent for `add_flow`). Mismatch: refuse, nothing written.
4. Run the call's `FinalAdmission` (the same closure the permit carries today), immediately before the write.
5. Send the FLOW_MOD inside an OF1.5 **bundle** with `OFPBF_ATOMIC` where rovs supports it (verify in task 1; otherwise a single FLOW_MOD plus barrier).
6. Barrier, read back, verify the switch matches the request; project children; emit signals; return the receipt data.
- Steps 2–6 run under the lock, so this process is the only writer. `ovs-*` commands are banned, so nothing else writes.

### D6. No `NORMAL`: explicit uplink flows
- Today `ovsbr0` carries only `priority=0 actions=NORMAL` (cookie `0x0`), and it carries all host traffic (`eth0` and the bridge's own `LOCAL` port). Removing it with nothing in its place cuts the host off.
- Order, each step through `add_flow` / `delete_flow` with receipts:
  1. Add `in_port=eth0 → output:LOCAL` and `in_port=LOCAL → output:eth0` (priority 10, derived cookies). With two ports these two flows carry everything `NORMAL` did.
  2. Read back both; check the host still reaches its gateway (`188.68.56.1`).
  3. Delete the cookie-`0x0` `NORMAL` flow (strict match on priority 0, empty match).
  4. Set fail mode `secure` so OVS does not become a learning switch by itself if the connection drops.
- After any wipe the uplink pair is restored first. When container or namespace ports are added later, each gets its own explicit flows; there is never a catch-all.
- A flow without the 3tched cookie prefix on a managed bridge is reported by `get_datapath_health` under `foreign` and is not projected as a child.

### D7. Packet-in controller (only if needed)
- `rovs-ext::Controller` in the bridge runtime, connecting to an OVS listener `ptcp:6653:10.200.0.1` set via `set_controller`, connection mode `out-of-band` disabled per the in-band rule (D8). No flow in this spec needs packet-ins, so this is wiring only, not on by default.

### D8. Safety rules carried over
- Uplink flows restored first after any wipe; fail mode `secure`; essential traffic forwards with no controller.
- Bounds kept from `op-of-controller`: bridge name ≤ 15 bytes, flow JSON ≤ 64 KiB.

## Removed

- `crates/op-network/src/controller.rs`, `controller_flow_verify.rs`, `bin/op-of-controller.rs`; the `org.opdbus.v1.plugins.openflow` D-Bus service; `crates/op-grpc-bridge/src/openflow_permit.rs` and the permit path in `openflow_methods.rs` (also used by `obfuscation_methods.rs` and `decoy_network.rs`, which move to the same in-process admission).
- `deploy/runit/op-of-controller/`, its entries in `deploy/runit/managed-services`, `deploy/lib/golden/activate-services.tsv` and its test, and `OF_CONTROLLER_*` in `deploy/config/network.conf`.
- `vendor/rovs-openflow` and its `[patch.crates-io]` entry; `packet_type` parsing in `openflow_translate.rs`.

## Risks

- rovs 0.2.0 is young (one version). Mitigation: fake-switch tests on encode/decode; any upstream fix goes to rovs, not a new vendored copy.
- OVS OpenFlow 1.5 support has gaps; any message OVS rejects at 1.5 falls back per message to the 1.3 form rovs also encodes, recorded in the code.
- Removing the permit path touches `obfuscation_methods.rs` and `decoy_network.rs`; their tests must pass unchanged in behaviour.
