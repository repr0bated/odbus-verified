# `mcp.proto`

- **Crate:** `op-mcp`
- **Path:** `crates/op-mcp/proto/mcp.proto`
- **Package:** `op.mcp.v1`
- **Imports:** `google/protobuf/{struct,empty}.proto`

Model Context Protocol (MCP) service surface: generic call/subscribe/stream plus
explicit tool discovery and invocation (including streaming tool output).

## Services

### `McpService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Call` | `McpRequest` | `McpResponse` | - |
| `Subscribe` | `SubscribeRequest` | `McpEvent` | server |
| `Stream` | `McpRequest` | `McpResponse` | bidi |
| `Health` | `google.protobuf.Empty` | `HealthResponse` | - |
| `Initialize` | `InitializeRequest` | `InitializeResponse` | - |
| `ListTools` | `ListToolsRequest` | `ListToolsResponse` | - |
| `GetToolSchema` | `GetToolSchemaRequest` | `GetToolSchemaResponse` | - |
| `CallTool` | `CallToolRequest` | `CallToolResponse` | - |
| `CallToolStreaming` | `CallToolRequest` | `ToolOutput` | server |

## Notes

- Tool schemas are schema-driven; `GetToolSchema` should reflect the underlying
  `PluginSchema` for the target tool.
- External MCP clients use `op-grpc-bridge` at `https://<host>:8090/mcp`, not this
  crate directly. See [`cognitive.proto`](../op-cognitive/cognitive.proto.md).

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
