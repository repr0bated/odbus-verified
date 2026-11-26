# Tasks — `rovs-openflow`

Branch `hackathon/kiroween`. Every commit ends with the trailer `Hackathon: kiroween`. One commit per task. Builds: affected crates only (`-p op-network -p op-grpc-bridge -p op-plugins`), few jobs; never a whole-workspace release build. No `ovs-*` commands anywhere; live steps go through zcall. Nothing is deployed and the host bridge is not restarted without the owner's OK.

- [x] 1. Confirm the rovs surface (no code change)
  - [x] 1.1 `rovs_transport::Address` Unix variant accepted by `VConn::connect`; hello at `Version::Of15` against `ovsbr0.mgmt` (read-only probe).
  - [x] 1.2 rovs 0.2.0 support for: OF1.5 bundles (`OFPBF_ATOMIC`), flow-description / flow-stats multipart, flow monitor, barrier, cookie + cookie_mask on strict modify/delete. Record gaps in `design.md`.
- [x] 2. Transport: `op-network` `OpenFlowClient` connects over a Unix address at OF1.5; reconnect via `rovs_transport::Reconnect`; echo handling. Fake-switch tests for hello, echo, reconnect.
- [x] 3. Cookies: derived cookie (prefix `0x33444348`, low 32 bits of SHA-256 over canonical bridge/table/priority/match); collision refusal; tests.
- [x] 4. Mutation path in the bridge (`runtime_network.rs`)
  - [x] 4.1 Per-bridge lock; read ▸ compare ▸ `FinalAdmission` ▸ write (bundle where supported) ▸ barrier ▸ read back ▸ verify.
  - [x] 4.2 `add_flow`, `modify_flow`, `delete_flow` by cookie; bounds (bridge ≤ 15 bytes, flow JSON ≤ 64 KiB).
  - [x] 4.3 Tests: CAS conflict writes nothing; unadmitted/unattributed mutation refused before any write; read-back mismatch reported.
- [ ] 5. Flow children
  - [ ] 5.1 Sealed `flow` child type in the `openflow` plugin schema from `FlowEntry` (schemars), `x-oscal-subid` on every field, entries in `oscal_subid_registry`.
  - [ ] 5.2 D-Bus children at `/org/opdbus/v1/plugins/openflow/bridges/<bridge>/flows/<cookie>`: child `<node>`s in `Introspect`, `GetManagedObjects`, `InterfacesAdded`/`InterfacesRemoved`, `PropertiesChanged`.
  - [ ] 5.3 3tchedFS entries `plugins/openflow/bridges/<bridge>/flows/<cookie>/value.json`, projected from the switch; rebuilt on reconnect.
  - [ ] 5.4 Counters from flow monitor where OVS reports them, else on read.
  - [ ] 5.5 `op-blob` descriptors include the child type (gRPC reflection); json-render renders a flow and the list from the schema.
  - [ ] 5.6 Tests: child appears on add, disappears on delete, rebuilt after reconnect, foreign flow reported not projected.
- [ ] 6. Bridge methods
  - [ ] 6.1 `get_datapath_health` over rovs/OVSDB (fail mode, controllers, uplink flows present, foreign flows).
  - [ ] 6.2 `set_fail_mode`, `set_controller`, `del_controller` over OVSDB (`ovsdb_bridge` path); `attach_controller_safe` pre-seeds the uplink flows.
  - [ ] 6.3 Remove `ensure_fallback_normal` from the sealed schema; retire its subid.
  - [ ] 6.4 `ovsbr0` `protocols` includes `OpenFlow15` (OVSDB transact).
- [ ] 7. Packet-in controller wiring: `rovs-ext::Controller` against `ptcp:6653:10.200.0.1`, off by default; test the connect path with a fake switch.
- [ ] 8. Move the other permit users (`obfuscation_methods.rs`, `decoy_network.rs`) to the in-process admitted path; their tests unchanged in behaviour.
- [ ] 9. Remove the hand-rolled code: `controller.rs`, `controller_flow_verify.rs`, `bin/op-of-controller.rs`, the `org.opdbus.v1.plugins.openflow` D-Bus service, `openflow_permit.rs`, `vendor/rovs-openflow` and its `[patch.crates-io]` entry, `packet_type` in `openflow_translate.rs`; deploy references (`deploy/runit/op-of-controller/`, `managed-services`, golden `activate-services.tsv` and its test, `OF_CONTROLLER_*` in `network.conf`).
- [ ] 10. Docs: `crates/op-chat/src/system_prompt.rs` network section (OpenFlow `10.200.0.1` on `ovsbr0` with `10.0.0.2` and `100.96.0.5`, port 6653, rovs, sealed plugins only); `deploy/` README for the network runtime.
- [ ] 11. Reseal the `openflow` plugin; check the mounted schema matches the sealed one.
- [ ] 12. Live, **owner OK required before each step**
  - [ ] 12.1 Deploy the bridge build; restart the host bridge.
  - [ ] 12.2 Add the two uplink flows (`eth0` ↔ `LOCAL`); read back; check gateway reachability.
  - [ ] 12.3 Delete the cookie-`0x0` `NORMAL` flow; set fail mode `secure`; re-check reachability (SSH, WARP doors, `https://10.0.0.2:20000/`).
  - [ ] 12.4 Add, modify and delete a test flow through zcall; see it appear and go as a child on D-Bus and in 3tchedFS.
- [ ] 13. Merge `hackathon/kiroween` back to `dev` cleanly.
