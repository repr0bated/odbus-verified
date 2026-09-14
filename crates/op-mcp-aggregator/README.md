# op-mcp-aggregator

MCP Server Aggregator - proxies and aggregates multiple MCP servers behind a single endpoint, with intelligent tool management to stay under Cursor's 40-tool limit.

## Features

| Feature | Description |
|---------|-------------|
| **Compact Mode** | Reduces 750+ tools to 4 meta-tools (~95% context savings) |
| **Tool Groups** | Organize tools into toggleable sets (systemd, network, etc.) |
| **Auto-Detection** | Automatically detects Gemini CLI, Cursor, Claude → optimal mode |
| **Profiles** | Named configurations for different use cases |
| **Multi-Server** | Aggregate tools from unlimited upstream MCP servers |

## Problem

Cursor IDE has a hard limit of ~40 MCP tools. If you have multiple MCP servers or a server with many tools, you quickly hit this limit.

## Solutions

### 1. Compact Mode (Recommended for LLMs)

Exposes only 4 meta-tools instead of hundreds:
- `list_tools` - Browse available tools
- `search_tools` - Find tools by keyword
- `get_tool_schema` - Get input schema
- `execute_tool` - Run any tool

**Auto-enabled for:** Gemini CLI, Claude, ChatGPT, any LLM client

### 2. Tool Groups (For Full Mode)

Organize tools into toggleable sets:

| Group | Description | ~Tools |
|-------|-------------|--------|
| core | Essential (respond, system_info) | 5 |
| shell | Command execution | 3 |
| filesystem | File operations | 10 |
| systemd | Service management | 12 |
| network | Network config | 10 |
| dbus | D-Bus introspection | 8 |
| packages | Package management | 6 |
| monitoring | System metrics | 8 |
| git | Version control | 10 |

**Example:** Enable `core + shell + systemd + network = 30 tools` (under 40!)

This crate provides an **aggregator** that:

1. **Connects to multiple upstream MCP servers** (SSE, stdio, websocket)
2. **Caches tool schemas** with TTL and LRU eviction
3. **Provides named profiles** that select subsets of tools
4. **Routes tool calls** to the correct upstream server
5. **Stays under Cursor's limits** per-profile

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Cursor IDE                               │
│                         │                                    │
│              ~/.cursor/mcp.json                             │
│              url: "http://localhost:3001/mcp/profile/dev"   │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                 op-mcp-aggregator                           │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Profile Manager                          │  │
│  │  /profile/sysadmin → [systemd, network, dbus]        │  │
│  │  /profile/dev      → [github, filesystem, shell]     │  │
│  │  /profile/minimal  → [respond, system_info]          │  │
│  └───────────────────────────────────────────────────────┘  │
│                           │                                  │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              Tool Cache (LRU + TTL)                   │  │
│  │  Schemas cached, routes tool calls to servers         │  │
│  └───────────────────────────────────────────────────────┘  │
│                           │                                  │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │
│  │ Local   │ │ GitHub  │ │ Postgres│ │ Custom  │           │
│  │ op-dbus │ │ MCP     │ │ MCP     │ │ Server  │           │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘           │
└─────────────────────────────────────────────────────────────┘
```

## Configuration

Create `/etc/op-dbus/aggregator.json`:

```json
{
  "servers": [
    {
      "id": "local",
      "name": "Local op-dbus",
      "url": "http://localhost:3001",
      "transport": "sse",
      "enabled": true,
      "priority": 100
    },
    {
      "id": "github",
      "name": "GitHub MCP",
      "url": "http://localhost:3002",
      "transport": "sse",
      "tool_prefix": "github",
      "include_tools": ["search_repositories", "search_code"],
      "auth": {
        "type": "bearer",
        "token": "${GITHUB_TOKEN}"
      }
    }
  ],
  "profiles": {
    "sysadmin": {
      "description": "System administration tools",
      "servers": ["local"],
      "include_namespaces": ["system", "systemd", "network"],
      "max_tools": 35
    },
    "dev": {
      "description": "Development tools", 
      "servers": ["local", "github"],
      "include_tools": ["github_*", "shell_*", "file_*"],
      "max_tools": 35
    }
  },
  "default_profile": "sysadmin",
  "max_tools_per_profile": 40
}
```

## Usage

### In Cursor

Update `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "op-dbus": {
      "url": "http://localhost:3001/mcp/profile/sysadmin",
      "transport": "sse"
    }
  }
}
```

Switch profiles by changing the URL path:
- `/mcp/profile/sysadmin` - System admin tools
- `/mcp/profile/dev` - Development tools  
- `/mcp/profile/minimal` - Essential tools only

### Programmatic

```rust
use op_mcp_aggregator::{Aggregator, AggregatorConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load config
    let config = AggregatorConfig::load("/etc/op-dbus/aggregator.json")?;
    
    // Create and initialize aggregator
    let aggregator = Aggregator::new(config).await?;
    aggregator.initialize().await?;
    
    // List tools for a profile
    let tools = aggregator.list_tools("sysadmin").await?;
    println!("Profile 'sysadmin' has {} tools", tools.len());
    
    // Call a tool
    let result = aggregator.call_tool("system_info", serde_json::json!({})).await?;
    println!("Result: {:?}", result);
    
    Ok(())
}
```

## Server Configuration

### Server Options

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique identifier |
| `name` | string | Human-readable name |
| `url` | string | Server URL or command |
| `transport` | `sse`/`stdio`/`websocket` | Connection type |
| `enabled` | bool | Whether to use this server |
| `tool_prefix` | string? | Prefix added to tool names |
| `include_tools` | string[] | Only include these tools |
| `exclude_tools` | string[] | Exclude these tools |
| `priority` | int | Higher = preferred when tools conflict |
| `timeout_secs` | int | Connection timeout |
| `auth` | object? | Authentication config |

### Profile Options

| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Human-readable description |
| `servers` | string[] | Which servers to include |
| `include_tools` | string[] | Specific tools (supports `*` wildcard) |
| `exclude_tools` | string[] | Tools to exclude |
| `include_categories` | string[] | Tool categories to include |
| `include_namespaces` | string[] | Namespaces to include |
| `max_tools` | int? | Max tools for this profile |

## Features

- **Multi-server aggregation**: Connect to unlimited upstream MCP servers
- **Profile-based filtering**: Define named profiles with different tool sets
- **Wildcard support**: Use `github_*` to match tool names
- **Tool prefixing**: Avoid name collisions with prefixes like `github_search`
- **LRU caching**: Efficient tool schema caching with TTL
- **Background refresh**: Keep schemas fresh automatically
- **Health checks**: Monitor upstream server status
- **Auth support**: Bearer tokens, basic auth, custom headers
- **Environment variables**: Use `${VAR_NAME}` in config values

## Integration with op-mcp

The aggregator integrates seamlessly with `op-mcp`. Add upstream servers and profiles to your existing setup without changing how Cursor connects.
