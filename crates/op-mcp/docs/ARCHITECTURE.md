# op-mcp Architecture

## Overview

op-mcp is a clean MCP (Model Context Protocol) server that provides:

1. **MCP JSON-RPC 2.0 Protocol** - Standard MCP protocol over stdio
2. **Lazy Tool Loading** - Tools loaded on-demand with LRU caching
3. **Discovery System** - Multiple sources for tool discovery
4. **External MCP Aggregation** - Connect to other MCP servers

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                        MCP Client                                │
│                    (Claude Desktop, etc.)                        │
└───────────────────────────┬─────────────────────────────────────┘
                            │ stdio (JSON-RPC 2.0)
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                        op-mcp Server                             │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    McpServer                             │   │
│  │  - JSON-RPC protocol handling                            │   │
│  │  - Request routing                                       │   │
│  │  - Response formatting                                   │   │
│  └────────────────────────┬────────────────────────────────┘   │
│                           │                                      │
│  ┌────────────────────────▼────────────────────────────────┐   │
│  │                 LazyToolManager                          │   │
│  │  - On-demand tool loading                                │   │
│  │  - Context-based filtering                               │   │
│  │  - LRU cache management                                  │   │
│  └────────────────────────┬────────────────────────────────┘   │
│                           │                                      │
│           ┌───────────────┼───────────────┐                     │
│           ▼               ▼               ▼                     │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ ToolRegistry │ │  Discovery   │ │   External   │            │
│  │  (LRU Cache) │ │   System     │ │  MCP Clients │            │
│  └──────────────┘ └──────────────┘ └──────────────┘            │
└─────────────────────────────────────────────────────────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        ▼                   ▼                   ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│   D-Bus      │   │   Plugins    │   │   Agents     │
│   Services   │   │   (op-state) │   │  (op-agents) │
└──────────────┘   └──────────────┘   └──────────────┘
```

## Key Components

### 1. McpServer

The main server component that:
- Handles MCP JSON-RPC 2.0 protocol
- Routes requests to appropriate handlers
- Manages server lifecycle

### 2. LazyToolManager

Manages tool loading with:
- **On-demand loading**: Tools loaded when first requested
- **LRU caching**: Evicts unused tools to save memory
- **Context filtering**: Returns relevant tools based on context
- **Multiple sources**: D-Bus, plugins, agents, external MCP

### 3. ToolRegistry (from op-tools)

Provides:
- Tool storage with usage tracking
- LRU eviction policy
- Factory-based lazy loading
- Statistics and monitoring

### 4. ToolDiscoverySystem (from op-tools)

Manages tool discovery from:
- **BuiltinToolSource**: Compiled-in tools
- **DbusDiscoverySource**: Runtime D-Bus introspection
- **PluginDiscoverySource**: State management plugins
- **AgentDiscoverySource**: Agent-based tools

## Data Flow

### Tool Listing

```
Client → tools/list → LazyToolManager → DiscoverySystem → Definitions
                                      ↓
                              Apply context filter
                                      ↓
                              Return paginated list
```

### Tool Execution

```
Client → tools/call → LazyToolManager → Registry.get()
                                       ↓
                            ┌──────────┴──────────┐
                            │ Tool loaded?        │
                            ├─────────────────────┤
                            │ Yes: Return cached  │
                            │ No: Load via factory│
                            └─────────────────────┘
                                       ↓
                              Tool.execute(args)
                                       ↓
                              Return result
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MCP_MAX_TOOLS` | 50 | Max tools to keep loaded |
| `MCP_IDLE_SECS` | 300 | Idle time before eviction |
| `MCP_DBUS_DISCOVERY` | true | Enable D-Bus discovery |
| `MCP_PLUGIN_DISCOVERY` | true | Enable plugin discovery |
| `MCP_AGENT_DISCOVERY` | true | Enable agent discovery |
| `MCP_PRELOAD` | true | Preload essential tools |

## Benefits

1. **Memory Efficient**: Only loads tools when needed
2. **Fast Startup**: No upfront tool loading
3. **Scalable**: Supports thousands of tools
4. **Context-Aware**: Returns relevant tools based on context
5. **Extensible**: Easy to add new discovery sources
