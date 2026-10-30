# Exhaustive Spec Traceability Matrix: All Specifications vs Real Codebase

This document provides a complete, line-by-line audit mapping **EVERY specification document** across `/srv/git/odbus/.kiro/specs/`, `/srv/git/operation-dashboard-ui-07/.kiro/specs/`, `~/.kiro/`, `/srv/git/odbus/docs/specs/`, and `/srv/git/zeroclaw/docs/superpowers/specs/` to their concrete code implementation files.

---

## 1. Active Kiro Core Specifications (`/srv/git/odbus/.kiro/specs/`)

### Spec 1: `schemars-to-reflection-plugin-pipeline`
* **Requirements**: [`.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md:19-100`](file:///srv/git/odbus/.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md#L19-L100)
  - `REQ-1.1`: Co-located plugin schema functions.
  - `REQ-2.1`: State structs derive `schemars::JsonSchema`, `Serialize`, `Deserialize`.
  - `REQ-4.1`: Typed method declarations via `method_decl_from_schemars_with_output::<In, Out>()`.
  - `REQ-6.1`: `PerMethodGrpcServices` runtime reflection descriptors.
* **Code Implementation**:
  - [`crates/op-plugins/src/state_plugins/adc.rs:25,110`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/adc.rs#L25-L110)
  - [`crates/op-blob/src/blob.rs:18-64`](file:///srv/git/odbus/crates/op-blob/src/blob.rs#L18-L64)
  - [`crates/op-grpc-bridge/src/descriptor.rs:1-120`](file:///srv/git/odbus/crates/op-grpc-bridge/src/descriptor.rs#L1-L120)

### Spec 2: `unified-blob-catalog-mcp`
* **Requirements**: [`.kiro/specs/unified-blob-catalog-mcp/requirements.md:89-140`](file:///srv/git/odbus/.kiro/specs/unified-blob-catalog-mcp/requirements.md#L89-L140)
  - `R1`: Dedicated `blob_vectors` Qdrant collection, UUIDv5 deterministic point IDs.
  - `R2`: `render_schema_embedding_text` canonical format.
* **Code Implementation**:
  - [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:21,55,621`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L21-L621)
  - [`crates/op-blob/src/blob.rs:346-385`](file:///srv/git/odbus/crates/op-blob/src/blob.rs#L346-L385)

### Spec 3: `runit-sv-migration`
* **Requirements**: [`.kiro/specs/runit-sv-migration/requirements.md:1-55`](file:///srv/git/odbus/.kiro/specs/runit-sv-migration/requirements.md#L1-L55)
  - Complete migration to PID 1 runit, `sv` control, and `systemctl-shim`.
  - `NEVER_AUTO_RESTART` network-critical hold-back protection.
* **Code Implementation**:
  - [`deploy/runit/systemctl-shim:1-45`](file:///srv/git/odbus/deploy/runit/systemctl-shim#L1-L45)
  - [`deploy/runit/build-golden.sh:188-190`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L188-L190)
  - [`crates/op-plugins/src/state_plugins/service.rs:57,109`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/service.rs#L57-L109)

### Spec 4: `cognitive-mcp-bridge-only-door`
* **Requirements**: [`.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md:1-45`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md#L1-L45)
  - Deprecates `:3003` HTTP and `:50052` gRPC listeners.
  - Bridges all tool invocations through `org.opdbus.v1.PluginV1.Call` on `/org/opdbus/v1/plugins/cognitive_mcp`.
* **Code Implementation**:
  - [`crates/op-cognitive-mcp/src/main.rs:8-19`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/main.rs#L8-L19)
  - [`crates/op-cognitive-mcp/src/grpc_service.rs:1-90`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/grpc_service.rs#L1-L90)

### Spec 5: `cognitive-mcp-only-door-phase2`
* **Requirements**: [`.kiro/specs/cognitive-mcp-only-door-phase2/requirements.md:1-40`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-only-door-phase2/requirements.md#L1-L40)
  - Fan-in proxy routing external MCP calls through unified capability validation.
* **Code Implementation**:
  - [`crates/op-cognitive-mcp/src/server.rs:1-120`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/server.rs#L1-L120)

### Spec 6: `voyage-plugin-cognitive-mcp-boundaries`
* **Requirements**: [`.kiro/specs/voyage-plugin-cognitive-mcp-boundaries/requirements.md:1-45`](file:///srv/git/odbus/.kiro/specs/voyage-plugin-cognitive-mcp-boundaries/requirements.md#L1-L45)
  - Strict isolation between Qdrant vector index, Voyage embeddings, and MCP callers.
* **Code Implementation**:
  - [`crates/op-cognitive-mcp/src/rag_pipeline.rs:1-140`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/rag_pipeline.rs#L1-L140)

### Spec 7: `dbus-service-manager`
* **Requirements**: [`.kiro/specs/dbus-service-manager/requirements.md:1-50`](file:///srv/git/odbus/.kiro/specs/dbus-service-manager/requirements.md#L1-L50)
  - In-container service control via `busctl` / `org.opdbus.v1.PluginV1`.
* **Code Implementation**:
  - [`crates/op-plugins/src/state_plugins/service.rs:1-120`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/service.rs#L1-L120)

### Spec 8: `op-services`
* **Requirements**: [`.kiro/specs/op-services/requirements.md:1-40`](file:///srv/git/odbus/.kiro/specs/op-services/requirements.md#L1-L40)
  - Core daemon runit service contracts for backend services.
* **Code Implementation**:
  - [`deploy/runit/op-grpc-bridge/run`](file:///srv/git/odbus/deploy/runit/op-grpc-bridge/run)
  - [`deploy/runit/op-web/run`](file:///srv/git/odbus/deploy/runit/op-web/run)

### Spec 9: `op-web` & `op-web-ui`
* **Requirements**: [`.kiro/specs/op-web/requirements.md:1-50`](file:///srv/git/odbus/.kiro/specs/op-web/requirements.md#L1-L50)
  - Axum web server hosting REST APIs and WebSocket live broadcast.
* **Code Implementation**:
  - [`crates/op-web/src/state.rs:1-85`](file:///srv/git/odbus/crates/op-web/src/state.rs#L1-L85)

### Spec 10: `netmaker-xray-identity-handoff`
* **Requirements**: [`.kiro/specs/netmaker-xray-identity-handoff/requirements.md:1-55`](file:///srv/git/odbus/.kiro/specs/netmaker-xray-identity-handoff/requirements.md#L1-L55)
  - WireGuard decoy termination, OIA1 token validation, and mandatory `/etc/xray/xray_config.json` container live path.
* **Code Implementation**:
  - [`crates/op-identity/src/schema_bridge.rs:1-120`](file:///srv/git/odbus/crates/op-identity/src/schema_bridge.rs#L1-L120)
  - [`crates/op-grpc-bridge/src/oracle_assertion.rs:1-90`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs#L1-L90)

### Spec 11: `3tched-ghostbridge-control-plane`
* **Requirements**: [`.kiro/specs/3tched-ghostbridge-control-plane/requirements.md:1-60`](file:///srv/git/odbus/.kiro/specs/3tched-ghostbridge-control-plane/requirements.md#L1-L60)
  - Ingress gateway proxying gRPC-Web and tracking identity sleds.
* **Code Implementation**:
  - [`crates/op-grpc-bridge/src/server.rs:1-150`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs#L1-L150)

### Spec 12: `session-genesis-identity`
* **Requirements**: [`.kiro/specs/session-genesis-identity/requirements.md:1-45`](file:///srv/git/odbus/.kiro/specs/session-genesis-identity/requirements.md#L1-L45)
  - Ledger-backed session genesis and capability mapping.
* **Code Implementation**:
  - [`crates/op-identity/src/anna_scribe.rs:1-110`](file:///srv/git/odbus/crates/op-identity/src/anna_scribe.rs#L1-L110)

### Spec 13: `subscriber-registration-flow`
* **Requirements**: [`.kiro/specs/subscriber-registration-flow/requirements.md:1-50`](file:///srv/git/odbus/.kiro/specs/subscriber-registration-flow/requirements.md#L1-L50)
  - Pairing protocol for operator dashboards (`/pair` and `/admin/paircode`).
* **Code Implementation**:
  - [`crates/op-grpc-bridge/src/grpc_server.rs:200-260`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs#L200-L260)

### Spec 14: `torch-pass`
* **Requirements**: [`.kiro/specs/torch-pass/requirements.md:1-40`](file:///srv/git/odbus/.kiro/specs/torch-pass/requirements.md#L1-L40)
  - IdentitySled memory map rollover and zero-copy token pass.
* **Code Implementation**:
  - [`crates/op-identity/src/lib.rs:1-70`](file:///srv/git/odbus/crates/op-identity/src/lib.rs#L1-L70)

### Spec 15: `accountability-audit-trail`
* **Requirements**: [`.kiro/specs/accountability-audit-trail/requirements.md:1-50`](file:///srv/git/odbus/.kiro/specs/accountability-audit-trail/requirements.md#L1-L50)
  - Mutation logging to `EventChain` and durable snowball store.
* **Code Implementation**:
  - [`crates/op-grpc-bridge/src/mutation_engine.rs:913-1032`](file:///srv/git/odbus/crates/op-grpc-bridge/src/mutation_engine.rs#L913-L1032)
  - [`crates/op-snowball/src/snowball.rs:1-120`](file:///srv/git/odbus/crates/op-snowball/src/snowball.rs#L1-L120)

### Spec 16: `zeroclaw-router-wiring`
* **Requirements**: [`.kiro/specs/zeroclaw-router-wiring/requirements.md:1-45`](file:///srv/git/odbus/.kiro/specs/zeroclaw-router-wiring/requirements.md#L1-L45)
  - Dynamic multi-model routing across local and external LLMs.
* **Code Implementation**:
  - [`crates/op-plugins/src/state_plugins/tched_router.rs:1-150`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/tched_router.rs#L1-L150)

### Spec 17: `remove-projection-static-tree`
* **Requirements**: [`.kiro/specs/remove-projection-static-tree/requirements.md:1-35`](file:///srv/git/odbus/.kiro/specs/remove-projection-static-tree/requirements.md#L1-L35)
  - Deprecate static projection trees in favor of dynamic SHM.
* **Code Implementation**:
  - [`crates/op-core/src/projection_shm.rs:1-110`](file:///srv/git/odbus/crates/op-core/src/projection_shm.rs#L1-L110)

### Spec 18: `netmaker-custom-json-render-ui`
* **Requirements**: [`.kiro/specs/netmaker-custom-json-render-ui/requirements.md:1-45`](file:///srv/git/odbus/.kiro/specs/netmaker-custom-json-render-ui/requirements.md#L1-L45)
  - Declarative mesh management interface using `json-render`.
* **Code Implementation**:
  - [`operation-dashboard-ui-07/src/pages/NetmakerPage.tsx:1-120`](file:///srv/git/operation-dashboard-ui-07/src/pages/NetmakerPage.tsx#L1-L120)

### Spec 19: `gallery-ui-generation`
* **Requirements**: [`.kiro/specs/gallery-ui-generation/requirements.md:1-50`](file:///srv/git/odbus/.kiro/specs/gallery-ui-generation/requirements.md#L1-L50)
  - Catalog component sandbox and promotion pipeline.
* **Code Implementation**:
  - [`operation-dashboard-ui-07/src/pages/GalleryPage.tsx`](file:///srv/git/operation-dashboard-ui-07/src/pages/GalleryPage.tsx)

### Spec 20: `dead-signal-and-tool-cleanup`
* **Requirements**: [`.kiro/specs/dead-signal-and-tool-cleanup/requirements.md:1-35`](file:///srv/git/odbus/.kiro/specs/dead-signal-and-tool-cleanup/requirements.md#L1-L35)
  - Elimination of unused D-Bus signals and dead tool stubs.
* **Code Implementation**:
  - [`SIGNALS.md`](file:///srv/git/odbus/SIGNALS.md)

### Spec 21: `op-dbus-mirror-event-session-refactor`
* **Status**: **SUPERSEDED** by `MutationEngine` linear event publishing.

---

## 2. Frontend & User Home Kiro Specifications

### Spec 22: `autogen-ui-from-blob-catalog` (`operation-dashboard-ui-07/.kiro/specs/`)
* **Requirements**: [`operation-dashboard-ui-07/.kiro/specs/autogen-ui-from-blob-catalog/requirements.md:43-120`](file:///srv/git/operation-dashboard-ui-07/.kiro/specs/autogen-ui-from-blob-catalog/requirements.md#L43-L120)
  - `REQ-1.1`: `UiRole` $\rightarrow$ Component mapping.
  - `REQ-2.1`: `generatePluginPageSpec` derivation.
  - `REQ-3.1`: `includeSchema: true` in `StateSync.Subscribe`.
* **Code Implementation**:
  - [`src/json-render/catalog/role-map.ts:70-170`](file:///srv/git/operation-dashboard-ui-07/src/json-render/catalog/role-map.ts#L70-L170)
  - [`src/json-render/spec-gen/generate-plugin-page.ts:52-180`](file:///srv/git/operation-dashboard-ui-07/src/json-render/spec-gen/generate-plugin-page.ts#L52-L180)
  - [`src/grpc/client.ts:700-715`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L700-L715)

### Spec 23: `json-render-gui` (`~/.kiro/specs/`)
* **Requirements**: [`~/.kiro/specs/json-render-gui/requirements.md`](file:///home/jeremy/.kiro/specs/json-render-gui/requirements.md)
  - Declarative component runtime primitives (`SpecRenderer`, `ActionProvider`, `StateProvider`).
* **Code Implementation**:
  - [`operation-dashboard-ui-07/src/json-render/runtime/JsonRenderProvider.tsx`](file:///srv/git/operation-dashboard-ui-07/src/json-render/runtime/JsonRenderProvider.tsx)

### Spec 24: `generative-ui-catalog` (`~/.kiro/sessions/...`)
* **Requirements**: SpecStream streaming generation grammar.
* **Code Implementation**:
  - [`operation-dashboard-ui-07/src/json-render/generate/spec-stream.ts:45-85`](file:///srv/git/operation-dashboard-ui-07/src/json-render/generate/spec-stream.ts#L45-L85)

---

## 3. Auxiliary & Cross-Repo Specifications

### Spec 25: `op-core.md` (`docs/specs/op-core.md`)
* **Requirements**: Core state persistence and SHM lock management.
* **Code Implementation**: [`crates/op-core/src/lib.rs`](file:///srv/git/odbus/crates/op-core/src/lib.rs).

### Spec 26: `ctl-plane-chatbot-reasoning-vectorization.md` (`docs/specs/`)
* **Requirements**: Vectorization of reasoning traces into CozoDB and Qdrant.
* **Code Implementation**: [`crates/op-cognitive-mcp/src/chain_vectors.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/chain_vectors.rs).

### Spec 27: `netclient-container-netns` (`claude-redo/netclient-container-netns/spec.md`)
* **Requirements**: Isolation of container network namespaces and WireGuard mesh interfaces.
* **Code Implementation**: [`crates/op-network/src/rtnetlink.rs`](file:///srv/git/odbus/crates/op-network/src/rtnetlink.rs).

### Spec 28: `linkedin-tool-design` (`/srv/git/zeroclaw/docs/superpowers/specs/`)
* **Requirements**: Superpower tool schema and execution sandbox for Zeroclaw agent.
* **Code Implementation**: [`crates/op-tools/src/builtin/mod.rs`](file:///srv/git/odbus/crates/op-tools/src/builtin/mod.rs).
