# op-mcp: Minimal MCP Protocol Adapter

## Architecture

This is a **thin protocol adapter** that exposes op-dbus-v2 functionality via the Model Context Protocol (MCP). It delegates all intelligence to existing crates:

- **op-chat** - Orchestration and LLM integration
- **op-tools** - Tool registry and execution
- **op-introspection** - D-Bus discovery and scanning

## Design Principle

**op-mcp = Protocol Adapter ONLY**

All complex functionality already exists in other crates. This crate just translates between:
- MCP JSON-RPC protocol (stdin/stdout)
- op-chat RPC calls

## Protocol Flow

```
stdin → MCP JSON-RPC → ChatActorHandle → stdout
```

### Supported Methods

- `initialize` - MCP handshake and capabilities
- `tools/list` → `chat.list_tools()` - List available tools
- `tools/call` → `chat.execute_tool()` - Execute a tool
- `resources/list` - List documentation resources (placeholder)
- `resources/read` - Read documentation (placeholder)

## Code Size

- **Before**: ~20,000 lines (95% duplication)
- **After**: ~350 lines (protocol adapter only)
- **Reduction**: 98% smaller!

## Building and Running

```bash
# Build the MCP server
cargo build --package op-mcp

# Run as MCP server
./target/debug/op-mcp-server

# Or install and run
cargo install --package op-mcp
op-mcp-server
```

## Dependencies

Minimal dependency set:
- `op-chat` - For orchestration
- `op-core` - For core types
- `tokio` - Async runtime
- `serde` - JSON serialization
- Standard logging/tracing crates

**No duplicate implementations** of:
- Tool registries
- Introspection systems
- Orchestrators
- Chat systems

## Integration

The MCP server integrates seamlessly with:

1. **Claude Desktop** - Add to MCP config:
   ```json
   {
     "mcpServers": {
       "op-dbus-v2": {
         "command": "op-mcp-server",
         "args": []
       }
     }
   }
   ```

2. **Other MCP Clients** - Any client that supports stdio-based MCP servers

## Error Handling

- Proper JSON-RPC 2.0 error responses
- Graceful handling of malformed requests
- Detailed error messages for debugging
- Protocol version compliance

## Testing

The minimal design makes testing straightforward:

- Unit tests for protocol translation
- Integration tests with op-chat
- Protocol compliance tests

## Future Extensions

If needed, this can be extended with:
- Resource registry with embedded documentation
- Additional MCP protocol features
- Health checking and monitoring
- Configuration management

## Migration from Old Implementation

The old implementation (`op-mcp.backup`) had massive duplication:

❌ **Removed** (now handled by other crates):
- Tool registry (use `op-tools`)
- Introspection system (use `op-introspection`)
- Chat orchestration (use `op-chat`)
- Agent management (use `op-agents`)
- Multiple web bridges
- Workflow systems

✅ **Kept** (minimal protocol adapter):
- MCP JSON-RPC protocol handling
- Request/response translation
- Resource serving (placeholder)

## Benefits

1. **Maintainable**: 350 lines vs 20,000 lines
2. **No Duplication**: Each feature exists in ONE place
3. **Clear Architecture**: Single responsibility principle
4. **Easy Testing**: Simple, focused components
5. **Protocol Compliant**: Proper MCP implementation

## Contributing

Keep it simple:
1. If you need new functionality, add it to the appropriate base crate
2. If you need MCP protocol features, add them here
3. Always delegate to existing crates - never duplicate