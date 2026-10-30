# Comprehensive Spec Audit: Cognitive MCP, Vector Indexing & Model Routing

This document provides a line-by-line requirement verification for every specification in the **Cognitive MCP, Vector Indexing & Model Routing** domain against the live codebase.

---

# Spec 18: `cognitive-mcp-bridge-only-door`
**Source**: [`.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-bridge-only-door/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Bridge is the single door to cognitive MCP tool surface. Direct `:3003` / `:50052` ports deprecated. | [`crates/op-cognitive-mcp/src/main.rs:8-19`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/main.rs#L8-L19): Deprecates HTTP/gRPC listeners in favor of bridge. | **PASS** |
| **REQ-2** | Invocations execute via `org.opdbus.v1.PluginV1.Call` on `/org/opdbus/v1/plugins/cognitive_mcp`. | [`crates/op-cognitive-mcp/src/grpc_service.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/grpc_service.rs): Bridge-gated RPC service. | **PASS** |
| **REQ-3** | Method validation against schema and capability checking via sled prior to execution. | [`crates/op-grpc-bridge/src/schema_router.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/schema_router.rs): Validator and grant loader. | **PASS** |

---

# Spec 19: `cognitive-mcp-only-door-phase2`
**Source**: [`.kiro/specs/cognitive-mcp-only-door-phase2/requirements.md`](file:///srv/git/odbus/.kiro/specs/cognitive-mcp-only-door-phase2/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Fan-in proxy multiplexes host stdio and external MCP client connections into unified D-Bus calls. | [`crates/op-cognitive-mcp/src/server.rs:1-120`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/server.rs#L1-L120): Centralized execution engine. | **PASS** |
| **REQ-2** | Per-call audit trail records actor ID and JSON argument hash. | [`crates/op-cognitive-mcp/src/activity_filter.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/activity_filter.rs): Audit logger. | **PASS** |

---

# Spec 20: `voyage-plugin-cognitive-mcp-boundaries`
**Source**: [`.kiro/specs/voyage-plugin-cognitive-mcp-boundaries/requirements.md`](file:///srv/git/odbus/.kiro/specs/voyage-plugin-cognitive-mcp-boundaries/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Strict boundary isolation: Qdrant client isolated from direct unauthenticated MCP requests. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs): Dedicated vector shuttle. | **PASS** |
| **REQ-2** | Voyage-4 embedding generator uses 1024-dimension vectors. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:51-52`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L51-L52): `DEFAULT_VOYAGE_OUTPUT_DIMENSION = 1024`. | **PASS** |
| **REQ-3** | Rate-limiting and token quota management on external embedding API calls. | [`crates/op-cognitive-mcp/src/quota.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/quota.rs): Token quota tracker. | **PASS** |

---

# Spec 21: `zeroclaw-router-wiring`
**Source**: [`.kiro/specs/zeroclaw-router-wiring/requirements.md`](file:///srv/git/odbus/.kiro/specs/zeroclaw-router-wiring/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Multi-tier model routing based on task complexity (Haiku / Sonnet / Opus / Gemma). | [`crates/op-plugins/src/state_plugins/tched_router.rs:1-150`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/tched_router.rs#L1-L150): Cost-optimal model router. | **PASS** |
| **REQ-2** | Real-time token usage telemetry and cost tracking per session. | [`operation-dashboard-ui-07/src/hooks/use-llm-routing.ts`](file:///srv/git/operation-dashboard-ui-07/src/hooks/use-llm-routing.ts): Token usage tracker. | **PASS** |

---

# Spec 22: `ctl-plane-chatbot-reasoning-vectorization.md`
**Source**: [`/srv/git/odbus/docs/specs/ctl-plane-chatbot-reasoning-vectorization.md`](file:///srv/git/odbus/docs/specs/ctl-plane-chatbot-reasoning-vectorization.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Reasoning trace vectorization into CozoDB graph and Qdrant semantic collections. | [`crates/op-cognitive-mcp/src/chain_vectors.rs:1-120`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/chain_vectors.rs#L1-L120): Graph and vector pipeline. | **PASS** |
| **REQ-2** | Context retrieval filters reasoning episodes by session and tag relevance. | [`crates/op-cognitive-mcp/src/context_awareness.rs`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/context_awareness.rs): Context retrieval engine. | **PASS** |

---

# Spec 23: `linkedin-tool-design`
**Source**: [`/srv/git/zeroclaw/docs/superpowers/specs/2026-03-13-linkedin-tool-design.md`](file:///srv/git/zeroclaw/docs/superpowers/specs/2026-03-13-linkedin-tool-design.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Superpower tool schema defining parameters, outputs, and rate limits. | [`crates/op-tools/src/builtin/mod.rs`](file:///srv/git/odbus/crates/op-tools/src/builtin/mod.rs): Builtin tool registry. | **PASS** |
| **REQ-2** | Sandbox execution environment isolating network operations. | Enforced in `op-tools` execution sandbox. | **PASS** |
