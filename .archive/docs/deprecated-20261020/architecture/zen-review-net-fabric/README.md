# Master Context Bundle: Network Fabric Architecture & Forensic Spec Audit

**Bundle Directory**: [`/srv/git/odbus/docs/architecture/zen-review-net-fabric/`](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/)  
**Primary Repositories**:
- Core Daemons & Plugins: [`/srv/git/odbus`](file:///srv/git/odbus)
- Operator Console UI: [`/srv/git/operation-dashboard-ui-07`](file:///srv/git/operation-dashboard-ui-07)
- User Steering: [`~/.kiro`](file:///home/jeremy/.kiro)

---

## 1. System Invariants (The Grand Non-Negotiables)

1. **Zero Plaintext on Wire**: All TCP traffic on `:8090` and mesh endpoints requires Tonic TLS 1.3/1.2 (`aws-lc-rs` CryptoProvider). Plaintext IPC is strictly confined to local Unix Domain Sockets (`/run/opdbus/grpc.sock`, `/run/ghostbridge/container.sock`, DAC `0660`).
2. **The Plugin IS the Schema**: Proto files and D-Bus interfaces are generated from Rust structs deriving `schemars::JsonSchema` and sealed into content-addressed `OPBLOB01` SHM blobs (`/dev/shm/opdbus/plugin-blobs/`). Hand-writing `.proto` for plugins is forbidden.
3. **Decoy Perimeter & Assertion Handoff**: Human WireGuard terminates exclusively at the external Oracle Decoy node. The decoy mints Ed25519 `OIA1` assertions carried as gRPC metadata (`x-oracle-identity-assertion-bin`) inside TLS across the NetMaker tunnel (`100.69.0.0/16` on `wg0`). The main host never runs human WireGuard (`no wg-lan`).
4. **OpenFlow In-Band Datapath Safety**: OVS `ovsbr0` runs `fail_mode=standalone` with in-band control. `priority=0,actions=NORMAL` (cookie `0x3344434800000001`) is pre-seeded before controller connection to prevent host blackholing.
5. **Physical MAC Pinning**: Uplink hardware MAC is cloned strictly onto internal port `pub0`, never on `ovsbr0`.
6. **Strict Separation of Planes**: Network Fabric (L1–L7 Transport) moves packets and secures TLS channels; Application Policy Layer (L7 Authorization) evaluates capability grants, identity assertions, mutation engine state, and snowball logs.

---

## 2. Master Document Index

### A. System Boundaries & Forensic Audits
* [**`00-fabric-vs-application-boundaries.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/00-fabric-vs-application-boundaries.md): Clean taxonomy separating Network Fabric from Application Policy and Capability Grants.
* [**`00-master-unified-system-interlock.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/00-master-unified-system-interlock.md): The 9-layer interlocking causal chain and cross-layer failure cascades.
* [**`DETAILED-KIRO-SPECS-CODE-AUDIT-AND-DRIFT-REPORT.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/DETAILED-KIRO-SPECS-CODE-AUDIT-AND-DRIFT-REPORT.md): Complete forensic audit of 24 spec packages categorizing Active (PASS), Refactored, Superseded, and Incomplete specifications.
* [**`MASTER-NETWORK-FABRIC-AND-SPECS-REQUIREMENT-AUDIT.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/MASTER-NETWORK-FABRIC-AND-SPECS-REQUIREMENT-AUDIT.md): Requirement-by-requirement verification matrices across all 5 functional domains.

### B. Deep Fabric Domain Audits
* [**`01-tls-zero-trust-fabric.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/01-tls-zero-trust-fabric.md) & [**`01-tls-zen-review.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/01-tls-zen-review.md): TLS 1.3/1.2, `aws-lc-rs`, fail-closed cert loading, and commit `ffcb4796`.
* [**`02-grpc-pipeline-and-bridge.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/02-grpc-pipeline-and-bridge.md): gRPC bridge architecture, dual reflection (static vs SHM dynamic), and schema pipeline.
* [**`03-openflow-datapath-controller.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/03-openflow-datapath-controller.md): Passive OpenFlow 1.3 controller, cookied flows, and safe attach rollback.
* [**`04-ovs-openvswitch-bridge.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/04-ovs-openvswitch-bridge.md): OVS `ovsbr0` setup, atomic OVSDB transactions, and `pub0` MAC pinning.
* [**`05-netmaker-overlay-transport.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/05-netmaker-overlay-transport.md): Single NetMaker overlay mesh, `netmaker-ovs-attach`, and decoy handoff.
* [**`06-integrated-network-fabric-end-to-end.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/06-integrated-network-fabric-end-to-end.md): End-to-end integration across all 5 network layers.
* [**`07-network-fabric-without-tls-risk-audit.md`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/07-network-fabric-without-tls-risk-audit.md): Plaintext attack surface analysis and fail-closed protection breakdown.

### C. Specifications Archive
* [**`all-specs-archive/`**](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/all-specs-archive/): Complete archive containing all 24 Kiro spec packages from `odbus`, `operation-dashboard-ui-07`, and `~/.kiro/`.
