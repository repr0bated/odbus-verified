# Requirements — `rovs-openflow`

Status: DRAFT for owner review, 2026-11-26. Branch `hackathon/kiroween`; every commit carries the trailer `Hackathon: kiroween`.

## Why

`op-of-controller` is ~2,000 hand-rolled lines (`crates/op-network/src/controller.rs`, `controller_flow_verify.rs`, `bin/op-of-controller.rs`) plus a vendored `rovs-openflow` patch. It is not installed on this host, so every sealed `openflow` method fails ("call unsupported"). The rovs suite (crates.io, 0.2.0) already provides the OpenFlow client, message types, a controller framework and flow templates.

Flows also have no representation of their own: they exist only as arguments to `add_flow` and as rows read back from the switch. A flow must be an object, so it can be seen, rendered and acted on like everything else, and so no flow is ever written into code.

## Glossary

- **Flow child**: one OpenFlow flow, represented as a child object of the `openflow` plugin.
- **Flow id**: the flow's OVS cookie (64-bit, hex), which identifies it on the switch.
- Every flow is created through the plugin's `add_flow` (owner, 2026-11-26). There are no flows made any other way.
- No `NORMAL` (owner, 2026-11-26): the bridge does not run as a learning switch. Every path is an explicit flow child, including the host uplink.

## Requirements

### 1. OpenFlow through rovs, OpenFlow 1.5

1.1 THE bridge's `openflow` plugin SHALL program and read flows with upstream rovs (`rovs-openflow`, `rovs-transport`) over OVS's own bridge socket `unix:/var/run/openvswitch/<bridge>.mgmt`, negotiating OpenFlow 1.5 (`Version::Of15`).
1.2 THE bridge's `protocols` SHALL include `OpenFlow15`, set through the sealed `ovsdb_bridge` plugin (OVSDB `transact`), never an `ovs-*` command.
1.3 WHERE a packet-in controller is needed, THE system SHALL use `rovs-ext`'s `Controller`, connecting to an OVS listener `ptcp:6653:10.200.0.1` (OVS listens; the controller dials).
1.4 THE host uplink SHALL stay up through the change: explicit uplink flows (`eth0` ↔ `LOCAL`, the bridge's own port) SHALL be in place before the `NORMAL` flow is removed, and SHALL be restored first after any wipe. Essential traffic SHALL forward without a controller.

### 2. Each flow is a child of the plugin

2.1 EACH flow on a managed bridge SHALL be a child of the `openflow` plugin at D-Bus path `/org/opdbus/v1/plugins/openflow/bridges/<bridge>/flows/<flow_id>`.
2.2 THE plugin's `Introspect` SHALL list its children as child `<node>` elements, and `GetManagedObjects` SHALL return them.
2.3 THE plugin SHALL emit `InterfacesAdded` when a flow appears and `InterfacesRemoved` when it goes; a flow child SHALL emit `PropertiesChanged` when its counters or state change.
2.4 EACH flow child SHALL expose its fields as properties: bridge, table, priority, cookie (flow id), match, actions, flags, idle/hard timeout, packet and byte counters, duration.
2.5 THE flow children SHALL also appear in 3tchedFS under the plugin at `plugins/openflow/bridges/<bridge>/flows/<flow_id>/` with `value.json`.
2.6 THE switch SHALL be the source of truth: children are read from OVS over rovs. No flow table is copied, cached or stored elsewhere. A flow removed on the switch disappears as a child.
2.7 EVERY flow on a managed bridge SHALL be created by `add_flow` and SHALL carry a plugin-assigned cookie. The existing priority-0 `NORMAL` flow SHALL be removed and SHALL NOT be re-created; its traffic is carried by explicit flow children (1.4).
2.8 A flow found on the switch without a plugin cookie SHALL be reported by `get_datapath_health` as foreign and SHALL NOT be adopted silently; it is an error to resolve, not a child.

### 3. Schema for rendering

3.1 THE flow child schema SHALL be part of the `openflow` plugin's sealed schema (one source), not a separate file.
3.2 EVERY field of the flow child schema SHALL carry an `x-oscal-subid`; every method SHALL keep its subid; `oscal_subid_registry` SHALL list them.
3.3 THE json-render UI SHALL be able to render a flow and the plugin's flow list from that schema alone; capability names SHALL NOT be rendered.
3.4 THE same schema SHALL feed the bridge's gRPC reflection descriptors (`op-blob` descriptor synthesis); no hand-written `.proto`.

### 4. Methods

4.1 `add_flow` SHALL create a flow child; `modify_flow` and `delete_flow` SHALL act on a flow child by flow id; `get_datapath_health` SHALL report the bridge's state (fail mode, controllers, fallback present).
4.2 `ensure_fallback_normal` SHALL be removed from the sealed schema (resealed, subid retired in `oscal_subid_registry`). `attach_controller_safe` SHALL pre-seed the explicit uplink flows instead of `NORMAL`. `set_fail_mode`, `set_controller`, `del_controller` SHALL keep their contracts; fail mode SHALL be `secure` so OVS never falls back to learning-switch behaviour on its own.
4.3 `add_flow` SHALL require a flow id that is absent on the switch; `modify_flow`/`delete_flow` SHALL require the expected current flow (compare-and-swap).

### 5. Admission unchanged

5.1 EVERY flow mutation SHALL be admitted (identity AND grants AND sealed) and SHALL run its final admission immediately before the write, then read back and verify.
5.2 AN unadmitted or unattributed mutation SHALL be refused before any write, with a test.
5.3 EVERY call SHALL produce a receipt in the execution tracker and sled ledger, unchanged.

### 6. Remove the hand-rolled code

6.1 `op-network`'s `controller.rs`, `controller_flow_verify.rs`, the `op-of-controller` binary, its runit service, the D-Bus service `org.opdbus.v1.plugins.openflow` and the OpenFlow permit hop SHALL be removed once 1–5 work.
6.2 THE vendored `vendor/rovs-openflow` patch and its `[patch.crates-io]` entry SHALL be removed; the `packet_type` code SHALL go with it (no L3-only port exists).
6.3 References in `deploy/` (`network.conf`, `managed-services`, golden activation) SHALL be updated.

### 7. Docs

7.1 THE chatbot system prompt network section (`crates/op-chat/src/system_prompt.rs`) SHALL say: OpenFlow address `10.200.0.1` on `ovsbr0` (next to `10.0.0.2` and `100.96.0.5`), controller port 6653, flows through rovs, managed only through sealed plugins.

### 8. Verification

8.1 Unit tests SHALL run against a fake switch (rovs message encode/decode), covering create, modify, delete, CAS conflict, refusal of unadmitted writes, uplink flows present before `NORMAL` is removed, foreign-flow reporting, and child appear/disappear signals.
8.2 ONE live check on `ovsbr0` through zcall SHALL add a flow, see it as a child (D-Bus and 3tchedFS), modify it, delete it, and confirm the uplink flows and host reachability are untouched.
8.3 Builds SHALL be bounded (affected crates only, few jobs) so they cannot exhaust host memory.
8.4 Nothing SHALL be deployed, and the host bridge SHALL NOT be restarted, without the owner's OK.
