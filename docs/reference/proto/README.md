# gRPC / Proto Contract Reference

_Generated 2026-07-02._ Per-file documentation for every project-owned `.proto` contract
in the `operation-dbus-proto` workspace (26 files across 8 crates). Vendored protos under
`node_modules/` and `google/protobuf/*` well-known types are excluded.

> **D-Bus first.** Every gRPC service here is a transport over the authoritative D-Bus
> object model (`org.opdbus.v1`). Message and RPC shapes derive from `PluginSchema`.
> See [`../api-reference.md`](../api-reference.md) §4 (gRPC surface) and
> [`../../overview/architecture.md`](../../overview/architecture.md).

## Index

| Crate | Proto | Package | Services |
|---|---|---|---|
| op-grpc-bridge | [operation.proto](./op-grpc-bridge/operation.proto.md) | `operation.v1` | StateSync, PluginService, EventChainService, OvsdbMirror, RuntimeMirror, DbusPassthrough |
| op-grpc-bridge | [registry.proto](./op-grpc-bridge/registry.proto.md) | `operation.registry.v1` | ComponentRegistry |
| op-grpc-bridge | [mail.proto](./op-grpc-bridge/mail.proto.md) | `operation.mail.v1` | MailService |
| op-grpc-bridge | [privacy_network.proto](./op-grpc-bridge/privacy_network.proto.md) | `operation.privacy.v1` | PrivacyNetworkService |
| op-grpc-bridge | [registration.proto](./op-grpc-bridge/registration.proto.md) | `operation.registration.v1` | RegistrationService |
| op-grpc-bridge | [zeroclaw.proto](./op-grpc-bridge/zeroclaw.proto.md) | `zeroclaw` | ZeroclawService |
| op-chat | [agents.proto](./op-chat/agents.proto.md) | `op_chat.agents` | AgentService, MemoryAgent, SequentialThinkingAgent, ContextManagerAgent, RustProAgent, BackendArchitectAgent |
| op-chat | [chat.proto](./op-chat/chat.proto.md) | `op_chat.chat` | ChatService |
| op-chat | [orchestration.proto](./op-chat/orchestration.proto.md) | `op_chat.orchestration` | AgentLifecycle, AgentExecution, MemoryService, SequentialThinkingService, ContextManagerService, RustProService, BackendArchitectService, WorkstackService |
| op-cache | [op_cache.proto](./op-cache/op_cache.proto.md) | `op_cache` | AgentService, OrchestratorService, CacheService, McpService |
| op-mcp | [mcp.proto](./op-mcp/mcp.proto.md) | `op.mcp.v1` | McpService |
| op-mcp | [internal_agents.proto](./op-mcp/internal_agents.proto.md) | `op_agents` | AgentLifecycle, AgentExecution, MemoryService, SequentialThinkingService, ContextManagerService, RustProService |
| op-cognitive-mcp | [cognitive.proto](./op-cognitive-mcp/cognitive.proto.md) | `operation.cognitive.v1` | CognitiveToolService |
| op-grpc-adapters | [adapters.proto](./op-grpc-adapters/adapters.proto.md) | `op.adapters.v1` | MailService, NetmakerService, MqService |
| op-services | [services.proto](./op-services/services.proto.md) | `opdbus.services.v1` | ServiceManager |
| op-assistant-grpc | [agent.proto](./op-assistant-grpc/agent.proto.md) | `assistant.v1` | AgentService |
| op-assistant-grpc | [common.proto](./op-assistant-grpc/common.proto.md) | `assistant.v1` | _(shared messages, no service)_ |
| op-assistant-grpc | [cron.proto](./op-assistant-grpc/cron.proto.md) | `assistant.v1` | CronService |
| op-assistant-grpc | [memory.proto](./op-assistant-grpc/memory.proto.md) | `assistant.v1` | MemoryService |
| op-assistant-grpc | [model.proto](./op-assistant-grpc/model.proto.md) | `assistant.v1` | ModelService |
| op-assistant-grpc | [namespace.proto](./op-assistant-grpc/namespace.proto.md) | `assistant.v1` | NamespaceMemoryService |
| op-assistant-grpc | [session.proto](./op-assistant-grpc/session.proto.md) | `assistant.v1` | SessionService |
| op-assistant-grpc | [soul.proto](./op-assistant-grpc/soul.proto.md) | `assistant.v1` | SoulService |
| op-assistant-grpc | [task.proto](./op-assistant-grpc/task.proto.md) | `assistant.v1` | TaskService |

## Conventions in these docs

- Each RPC row lists request/response message names and a **Stream** column:
  `-` (unary), `server`, `client`, or `bidi`.
- Message field-level shapes are intentionally omitted; the `.proto` source is the
  authority for exact fields. RPC signatures above are extracted verbatim from source.

## Known gaps

- **MailService duplication:** exists in both `op-grpc-bridge/mail.proto`
  (`operation.mail.v1`, bridge projection) and `op-grpc-adapters/adapters.proto`
  (`op.adapters.v1`, adapter transport).
