# Zen Review: Network Fabric — End-to-End Integrated Architecture

**Audit Target**: Complete Network Fabric Integration (Oracle Decoy, NetMaker, OVS, OpenFlow, gRPC Bridge, TLS, & Grants)  
**Document Path**: [`/srv/git/odbus/docs/architecture/zen-review-net-fabric/06-integrated-network-fabric-end-to-end.md`](file:///srv/git/odbus/docs/architecture/zen-review-net-fabric/06-integrated-network-fabric-end-to-end.md)  
**Governing Specs**:
- [`.kiro/specs/3tched-ghostbridge-control-plane/requirements.md`](file:///srv/git/odbus/.kiro/specs/3tched-ghostbridge-control-plane/requirements.md)
- [`.kiro/specs/netmaker-xray-identity-handoff/requirements.md`](file:///srv/git/odbus/.kiro/specs/netmaker-xray-identity-handoff/requirements.md)
- [`.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md`](file:///srv/git/odbus/.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md)
- [`.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md)  
**Status**: **PASS (Fully Integrated & Hardened)**

---

## 1. End-to-End System Topology

```mermaid
graph TD
    subgraph 1. Edge Decoy & Identity Handoff
        HUMAN[Human Operator / Browser] -->|WireGuard Outer Tunnel| DECOY[Oracle Decoy Edge]
        DECOY -->|Mints 300s Ed25519 OIA1 Assertion| OIA1[x-oracle-identity-assertion-bin]
    end

    subgraph 2. Encrypted Overlay Transit
        DECOY -->|NetMaker Mesh wg0 100.69.0.0/16| HOST_IFACE[Host Physical Ingress]
    end

    subgraph 3. Open vSwitch & OpenFlow Datapath (ovsbr0)
        HOST_IFACE --> OVSBR[ovsbr0 Bridge Device]
        OVSBR --> PORT_PUB[pub0: Pinned Uplink MAC & Public IP]
        OVSBR --> PORT_SVC[svc0: 10.200.0.1 gRPC / OF Controller]
        OVSBR --> PORT_WG[wg0: NetMaker Overlay Interface]
        OVSBR --> PORT_CT[veth*: Container Sockets]
        
        OVSBR -->|Cookie 0x3344434800000001 priority=0| NORMAL[actions=NORMAL Host Survival]
        OVSBR -->|Cookie 0x3344434800000002| OF_DEMUX[L3/L4 OpenFlow Forwarding]
        OF_CTRL[op-of-controller :6653] <-->|Passive OF1.3| OVSBR
    end

    subgraph 4. Zero-Trust gRPC Bridge & Gateway
        OF_DEMUX -->|Tonic TLS :8090| BRIDGE_TCP[op-grpc-bridge TCP Door]
        OPWEB[op-web :8080/:8443] -->|Loopback TLS :8090 Proxy| BRIDGE_TCP
        
        LOCAL_CLI[op-cli / Daemons] -->|UDS /run/opdbus/grpc.sock| BRIDGE_UDS[Unix Domain Socket Door]
        CONTAINERS[Guest Containers] -->|UDS /run/ghostbridge/container.sock| BRIDGE_SHM[Shared Container Door]
    end

    subgraph 5. Security Gate & Authoritative Storage
        BRIDGE_TCP --> INTERCEPTOR[Oracle Assertion Validator]
        INTERCEPTOR -->|Verify Signature, TTL, Nonce, IP| GATE[Capability & Grant Gate]
        GATE -->|Check Footprint & Permissions| MUTATION[authoritative MutationEngine]
        MUTATION --> SHM_BLOBS[/dev/shm/opdbus/plugin-blobs/]
        MUTATION --> SLED[(Identity Sled / Scribe)]
        MUTATION --> DBUS[D-Bus /org/opdbus/v1/plugins/*]
    end
```

---

## 2. Integrated Architectural Invariants

| Layer | Component | Invariant Guarantee | Code Enforcement |
|---|---|---|---|
| **L7 Security** | **Zero-Trust TLS** | All TCP listeners enforce TLS 1.3/1.2; plaintext TCP is eliminated across all interfaces. Rustls 0.23 `aws-lc-rs` provider initialized at boot. | [`crates/op-grpc-bridge/src/server.rs:465-494`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs#L465-L494) |
| **L7 Auth** | **Assertion Handoff** | Human WireGuard terminates at Oracle Decoy; Decoy mints Ed25519 assertions (`OIA1`) carried as gRPC metadata validated solely at `op-grpc-bridge`. | [`crates/op-grpc-bridge/src/oracle_assertion.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs) |
| **L7 Gate** | **Capability Engine** | Calls are gated by footprint capability grants against sealed `PluginSchema`. Implicit method capability resolution eliminates double-gating traps. | [`crates/op-grpc-bridge/src/grpc_server.rs:160-220`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs#L160-L220) |
| **L4/L3 Transport**| **NetMaker Mesh** | Single NetMaker tunnel (`100.69.0.0/16`) provides encrypted transit without host-level human WireGuard exposure. | [`crates/op-plugins/src/state_plugins/netmaker.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/netmaker.rs) |
| **L2 Datapath** | **OVS Bridge (`ovsbr0`)** | Atomic single-transaction bridge/uplink creation; physical NIC MAC pinned strictly on `pub0` internal port, never `ovsbr0`. | [`crates/op-network/src/bin/op-ovsbr0-setup.rs`](file:///srv/git/odbus/crates/op-network/src/bin/op-ovsbr0-setup.rs) |
| **L2 Control** | **OpenFlow Safety** | `fail_mode=standalone`, cookied flows (`FALLBACK_COOKIE = 0x3344434800000001`, `MANAGED_COOKIE = 0x3344434800000002`), automatic rollback on attach failure. | [`crates/op-network/src/datapath_safe.rs`](file:///srv/git/odbus/crates/op-network/src/datapath_safe.rs) |
| **IPC Isolation** | **UDS Separation** | Unencrypted IPC is restricted strictly to local Unix sockets (`/run/opdbus/grpc.sock`, `/run/ghostbridge/container.sock`) protected by filesystem DAC (`0660`). | [`crates/op-grpc-bridge/src/server.rs:427-449`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs#L427-L449) |

---

## 3. End-to-End Traceability Matrix

| Flow Step | Trigger / Ingress | Processing Layer | Verification & State Change | Status |
|---|---|---|---|:---:|
| **Step 1** | Operator connects via WireGuard. | Oracle Decoy Edge | Decoy authenticates peer pubkey and mints 300s TTL Ed25519 `OIA1` assertion. | **PASS** |
| **Step 2** | Traffic traverses NetMaker overlay (`100.69.0.0/16`). | `netmaker` / `wg0` iface on `ovsbr0` | L3 packet arrives at host over single encrypted overlay tunnel without NAT. | **PASS** |
| **Step 3** | Packet enters `ovsbr0` bridge. | OpenFlow Flow Table | Match cookie `0x3344434800000002` forwards TCP 8090 to `svc0` / host listener; host SSH preserved via `NORMAL`. | **PASS** |
| **Step 4** | TLS handshake on `:8090`. | `op-grpc-bridge` Server | `ServerTlsConfig` verifies TLS identity; `TlsConnectInfo` extracts socket address and verifies inner IP. | **PASS** |
| **Step 5** | Interceptor inspects metadata. | `oracle_assertion` module | Verifies Ed25519 signature, structural lifetime, clock leeway (30s), replay nonce, and resolves `HumanPrincipal`. | **PASS** |
| **Step 6** | Capability authorization. | `enforce_bridge_capability` | Compares resolved principal footprint against method's `required_capability` in sealed `PluginSchema`. | **PASS** |
| **Step 7** | Authoritative state mutation. | `MutationEngine` → D-Bus | Appends linear `StateChange` to `EventChain`, persists to Cozo/Sled, updates SHM blob, and notifies D-Bus. | **PASS** |

---

## 4. Final Verdict

- **End-to-End Fabric Cohesion**: **PASS**
- **Zero-Trust Boundary Enforcement**: **PASS**
- **Cross-Layer Invariant Integrity**: **PASS**
