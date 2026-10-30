# Spec 18: `cognitive-mcp-bridge-only-door`

**Spec Path**: [`.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md)  
**Domain**: Cognitive MCP Ingress & Single-Door Policy  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | `op-grpc-bridge` is the ONLY door to cognitive MCP tool surface. Direct `:3003`/`:50052` ports deprecated. | [`crates/op-cognitive-mcp/src/main.rs:8-19`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/main.rs#L8-L19): Deprecates HTTP/gRPC listeners. | **PASS** |
| **REQ-2** | Invocations execute exclusively via `org.opdbus.v1.PluginV1.Call` on `/org/opdbus/v1/plugins/cognitive_mcp`. | [`crates/op-cognitive-mcp/src/grpc_service.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/grpc_service.rs): Bridge-gated RPC service. | **PASS** |
| **REQ-3** | Method validation against schema and capability checking via sled prior to execution. | [`crates/op-grpc-bridge/src/schema_router.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/schema_router.rs): Validator and grant loader. | **PASS** |
| **REQ-4** | Every tool execution appends to `EventChain` accountability ledger. | Enforced by `MutationEngine` during tool invocation. | **PASS** |
