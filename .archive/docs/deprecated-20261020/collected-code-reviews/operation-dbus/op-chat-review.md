# op-chat Code Review: Spec/Design vs Implementation

## Summary

Compared the comprehensive SPEC.md (2297 lines) and DESIGN.md (2741 lines) against actual implementation. The spec and design describe a 16-week phased build; the implementation is roughly at week 2. **49 findings total: 5 critical, 10 high, 14 medium, 3 low.**

The spec is aspirational — it describes a system that is perhaps 10-15% implemented. There are 15 orphan source files (not included in lib.rs, never compiled) containing working implementations that are disconnected from the main code path.

---

## Critical Stubs (5 findings)

### 1. ChatActor.handle_chat() returns hardcoded string
**File**: `src/actor.rs` ~line 456
**Spec**: LLM-powered chat with tool execution, anti-hallucination
**Code**: Returns `"Chat not yet implemented"`
**Fix**: Wire in ForcedToolPipeline from orphan `forced_tool_pipeline.rs`

### 2. MCP Server drops params and results
**File**: `src/mcp_server.rs` lines 184, 192
**Spec**: Full MCP-over-gRPC server
**Code**: `params: None` and `result: None` — every call ignores input and discards output
**Fix**: Parse params JSON, serialize result back

### 3. MCP Server list_tools/call_tool are dead
**File**: `src/mcp_server.rs` lines 237-251
**Spec**: Exposes chat tools via MCP
**Code**: list_tools returns empty array, call_tool returns error
**Fix**: Populate from ToolRegistry, delegate to executor

### 4. GrpcAgentPool is entirely simulated
**File**: `src/orchestration/grpc_pool.rs` ~lines 742-777
**Spec**: Real gRPC connections with circuit breaker, health checks
**Code**: Returns `{"agent": id, "success": true, "simulated": true}`
**Fix**: Replace with actual tonic gRPC client calls

### 5. main.rs drops actor handle immediately
**File**: `src/main.rs` line 18
**Spec**: ChatActor runs as long-lived service
**Code**: `let (actor_obj, _handle)` — handle dropped, mpsc sender dies, actor terminates
**Fix**: Store handle in a live variable

---

## High Severity — Spec Divergences (10 findings)

### 1. ForcedExecutionOrchestrator is never wired in
Exists in `forced_execution.rs` with `start_turn()`, `verify_turn()`, `execute_tool_sequence()` — but nothing calls it. The core anti-hallucination system is entirely disconnected.

### 2. NLAdminOrchestrator bypasses TrackedToolExecutor
Calls `tool.execute()` directly, skipping rate limiting and audit trail entirely.

### 3. NLAdminOrchestrator constructor diverges
Spec says `(Arc<dyn LlmProvider>, Arc<TrackedToolExecutor>, Arc<ToolRegistry>)`. Code takes only `Arc<ToolRegistry>`, provider passed per-call.

### 4. system_prompt.rs signature diverges
Spec: `fn generate_system_prompt(tools, custom_additions, repo_info) -> String`. Code: `async fn generate_system_prompt() -> ChatMessage` — no params, wrong return type, tool summary hardcoded.

### 5. TrackedToolExecutor concurrency "limiting" doesn't block
Uses AtomicU64 counter instead of Semaphore. Tracks concurrent count but doesn't actually prevent exceeding the limit.

### 6. get_history() and get_stats() return empty/zeros
Both methods have TODO comments and return stub data.

### 7. Proto definitions compiled but never included
`build.rs` compiles `orchestration.proto`, but `proto/mod.rs` is empty. Generated code is dead. `agents.proto` not compiled at all.

### 8. Only 3 builtin workstacks vs spec's 10
Missing: security_audit, database_migration, deploy_production, debug_network, infra_setup, multi_service_deploy, dr_plan

### 9. Workstack variables use String not Value
`HashMap<String, String>` instead of `HashMap<String, Value>` — no structured data support.

---

## Orphan/Dead Code — 15 Files Not Compiled

| File | Lines | What it does | Value |
|------|-------|-------------|-------|
| `tool_orchestrator.rs` | 195 | Full LLM-tool chat loop, uses TrackedToolExecutor correctly | **HIGH — integrate** |
| `forced_tool_pipeline.rs` | 295 | Missing glue connecting LLM to forced execution | **HIGH — integrate** |
| `tool_loader.rs` | 2318 | 30+ tool implementations (response, fs, shell, systemd, OVS) | **HIGH — integrate** |
| `agent_tools.rs` | 656 | Agent tool registration with keyword matching | MEDIUM |
| `router.rs` | 210 | Axum HTTP router with chat/session endpoints | MEDIUM |
| `intent_executor.rs` | 668 | Regex-based NL intent parsing, well-tested | MEDIUM |
| `chat_loop.rs` | 402 | Another forced-tool chat loop with CLI validation | MEDIUM |
| `orchestrated_executor.rs` | 649 | Unified execution routing | MEDIUM |
| `hybrid_executor.rs` | 221 | Intent-first with LLM fallback (has compile error) | LOW |
| `grpc_client.rs` | 491 | gRPC agent client (simulated, duplicates grpc_pool) | LOW |
| `orchestration/workstacks.rs` | 614 | Full workstack executor (overlaps workstack_executor.rs) | MEDIUM |
| `orchestration/workflows.rs` | 491 | Workflow engine with conditional branching | MEDIUM |
| `orchestration/executor.rs` | 547 | Another orchestrated executor | MEDIUM |
| `orchestration/dbus_orchestrator.rs` | ~200 | D-Bus agent lifecycle management | MEDIUM |
| `orchestration/coordinator.rs` | ~200 | Multi-agent coordinator (sequential/parallel/race/voting) | MEDIUM |

**Key insight**: There are 3-4 independent implementations of the "chat with tools" loop across these files. Significant iteration happened without cleanup.

---

## Bugs (6 findings)

### Medium
1. **hybrid_executor.rs** — unresolvable variable reference (line 124 uses `args` never bound)
2. **tool_loader.rs** — `register_tool` function defined twice (lines 28-41 and 48-60)
3. **tool_loader.rs** — OVS tools call `ovs-vsctl`/`ovs-ofctl` CLI commands, violating "native protocols only" philosophy

### Low
4. **system_prompt.rs** — test asserts `contains("ANTI-HALLUCINATION")` but prompt uses `"FORCED TOOL EXECUTION"` header
5. **error.rs** — `#[cfg(feature = "grpc")]` used but no `grpc` feature defined in Cargo.toml
6. **mcp_server.rs** — `handle_read_resource` returns hardcoded fake session data

---

## Top 5 Recommended Actions

1. **Wire ForcedToolPipeline into ChatActor.handle_chat()** — connects LLM, tools, and verification. Code exists in orphan file.
2. **Fix MCP server params/results** — two lines (184, 192) make the entire gRPC interface non-functional.
3. **Fix main.rs handle lifetime** — actor dies immediately because handle is dropped.
4. **Integrate tool_loader.rs** — contains the actual tool implementations the system needs.
5. **Clean up orphan files** — decide which chat loop to keep, integrate it, delete the rest.

---

## What's Actually Good

- **ForcedExecutionOrchestrator** (`forced_execution.rs`) — well-designed hallucination detection with 5 types, severity levels, and verification rules. Just needs to be connected.
- **SessionManager** (`session.rs`) — fully implemented CRUD with tests, eviction, and clean API.
- **TrackedToolExecutor** (`tool_executor.rs`) — rate limiting logic and metrics tracking are solid, just needs Semaphore fix.
- **Skills system** (`orchestration/skills.rs`) — clean registry with constraint types (RequireArgument, ForbidArgument, MaxExecutions).
- **GrpcAgentPool architecture** (`orchestration/grpc_pool.rs`) — circuit breaker state machine is correctly designed, just returns fake data.
- **WorkstackExecutor** (`orchestration/workstack_executor.rs`) — dependency resolution and phase execution flow is sound.
- **NLAdminOrchestrator** (`nl_admin.rs`) — tool call extraction from 4 formats (native, XML tags, code blocks, JSON-in-text) is thorough.

---

*Generated 2026-02-16 from full spec/design vs implementation review*
