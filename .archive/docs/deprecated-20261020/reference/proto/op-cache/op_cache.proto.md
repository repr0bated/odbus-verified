# `op_cache.proto`

- **Crate:** `op-cache`
- **Path:** `crates/op-cache/proto/op_cache.proto`
- **Package:** `op_cache`
- **Imports:** `google/protobuf/{any,struct}.proto`

Agent registry, orchestration, workstack step caching, and an MCP passthrough, unified in
one caching service crate.

## Services

### `AgentService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Register` | `RegisterAgentRequest` | `RegisterAgentResponse` | - |
| `Unregister` | `UnregisterAgentRequest` | `UnregisterAgentResponse` | - |
| `Execute` | `ExecuteAgentRequest` | `ExecuteAgentResponse` | - |
| `ExecuteStream` | `ExecuteAgentRequest` | `ExecuteAgentChunk` | server |
| `GetAgent` | `GetAgentRequest` | `Agent` | - |
| `ListAgents` | `ListAgentsRequest` | `ListAgentsResponse` | - |
| `FindByCapability` | `FindByCapabilityRequest` | `FindByCapabilityResponse` | - |
| `ListCapabilities` | `Empty` | `ListCapabilitiesResponse` | - |
| `HealthCheck` | `HealthCheckRequest` | `HealthCheckResponse` | - |

### `OrchestratorService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Execute` | `OrchestratorRequest` | `OrchestratorResponse` | - |
| `ExecuteStream` | `OrchestratorRequest` | `WorkstackStepResult` | server |
| `ExecuteAgents` | `ExecuteAgentsRequest` | `OrchestratorResponse` | - |
| `Resolve` | `ResolveRequest` | `ResolveResponse` | - |
| `GetPatterns` | `Empty` | `GetPatternsResponse` | - |
| `PromotePattern` | `PromotePatternRequest` | `PromotePatternResponse` | - |
| `GetStats` | `Empty` | `OrchestratorStats` | - |

### `CacheService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetStep` | `GetStepRequest` | `GetStepResponse` | - |
| `PutStep` | `PutStepRequest` | `PutStepResponse` | - |
| `InvalidateWorkstack` | `InvalidateWorkstackRequest` | `InvalidateWorkstackResponse` | - |
| `InvalidateStep` | `InvalidateStepRequest` | `InvalidateStepResponse` | - |
| `Cleanup` | `CleanupRequest` | `CleanupResponse` | - |
| `GetStats` | `Empty` | `CacheStats` | - |
| `GetWorkstackStats` | `GetWorkstackStatsRequest` | `WorkstackCacheStats` | - |

### `McpService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `HandleRequest` | `McpRequest` | `McpResponse` | - |
| `ListTools` | `ListToolsRequest` | `ListToolsResponse` | - |

## Notes

- Workstack step results are keyed and cached for reuse across orchestration runs.
- Pattern promotion feeds recurring successful step chains back into the orchestrator.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
