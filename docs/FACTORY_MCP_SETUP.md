# Factory/Droid MCP Configuration Guide

## Overview

This guide configures **Factory/Droid** (the AI agent you're currently using) to connect to the **cognitive-mcp** gateway for enhanced capabilities including memory, RAG, and session management.

## Architecture

Per AGENTS.md Section 4b:

```
┌─────────────────────────────────────────────────────────────┐
│                    Factory/Droid (External)                  │
│                    Your AI Agent Interface                   │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│           cognitive-mcp :3003 (100.90.37.254)              │
│        ★ UNIVERSAL GATEWAY - ALL external clients          │
│              Droid ✓ | Cursor ✓ | Codex ✓ | etc.           │
│  Memory · RAG · Session Mgmt · Quota · Ghostbridge Auth      │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│              compact-mcp :11436 (127.0.0.1)                 │
│              ✗ Loopback/Chatbot ONLY - NEVER expose         │
└─────────────────────────────────────────────────────────────┘
```

## Configuration Files Created

### 1. `~/.factory/mcp.json` (ACTIVE)
Factory's MCP server configuration:

```json
{
  "mcpServers": {
    "cognitive-mcp": {
      "url": "http://100.90.37.254:3003",
      "transport": "sse",
      "enabled": true
    }
  }
}
```

### 2. `~/.factory/settings.json` (UPDATED)
Factory settings now include MCP configuration:

```json
{
  "enabledPlugins": {
    "core@factory-plugins": true,
    "mcp@factory-plugins": true
  },
  "mcp": {
    "enabled": true,
    "cognitiveMcp": {
      "url": "http://100.90.37.254:3003",
      "clientType": "droid"
    }
  }
}
```

### 3. `deploy/config/factory-mcp.json` (TEMPLATE)
Project deployment template with full optimization settings.

## Verification

Run the verification script:

```bash
# Make executable
chmod +x scripts/verify-factory-cognitive-mcp.sh

# Run verification
./scripts/verify-factory-cognitive-mcp.sh
```

Expected output:
```
==========================================
Factory/Droid Cognitive-MCP Verifier
==========================================

[1/4] Checking WireGuard interface (netmaker)...
  ✅ WireGuard interface active (IP: 100.90.x.x)

[2/4] Checking cognitive-mcp gateway health...
  ✅ Cognitive-MCP gateway is reachable

[3/4] Fetching available capabilities...
  ✅ Found X tools available

[4/4] Checking Factory configuration...
  ✅ Factory MCP config found: ~/.factory/mcp.json
  ✅ Cognitive-MCP configured in Factory
```

## Available Tools

Once connected, Factory/Droid gains access to:

| Tool | Description |
|------|-------------|
| `memory/store` | Store context across sessions |
| `memory/retrieve` | Retrieve stored memories |
| `memory/query` | Search memory by tags/patterns |
| `rag/search` | Semantic search via Qdrant |
| `session/start` | Start a new tracked session |
| `session/save_context` | Save session checkpoint |
| `namespace/create` | Create memory namespaces |
| `namespace/list` | List available namespaces |
| `quota/status` | Check rate limit status |

## Usage Examples

### Store Project Context
```
Hey Droid, store this context in memory:
Namespace: "project:op-dbus"
Key: "architecture"
Value: "Using D-Bus first architecture with cognitive-mcp gateway"
Tags: ["architecture", "mcp"]
```

### RAG Search
```
Search the knowledge base for:
"How does the Ghostbridge interceptor work?"
```

### Session Management
```
Start a new session for implementing the feature X.
Save checkpoint after we complete the database schema.
```

## Troubleshooting

### Connection Refused
```bash
# Check if cognitive-mcp is running
systemctl status op-cognitive-mcp

# Check if port 3003 is listening
ss -tlnp | grep 3003
```

### WireGuard Auth Failure
```bash
# Verify WireGuard interface
wg show netmaker

# Check IP
ip addr show netmaker
```

### Factory Not Picking Up Config
1. Restart Factory/Droid application
2. Check: `~/.factory/mcp.json` exists
3. Verify JSON syntax: `jq . ~/.factory/mcp.json`

## Client Optimization

The configuration includes optimization settings for Factory/Droid:

```json
{
  "optimization": {
    "connectionPool": {
      "maxIdle": 10,
      "idleTimeoutMs": 90000,
      "useHttp2": true
    },
    "retry": {
      "maxRetries": 3,
      "backoffMultiplier": 2.0,
      "useJitter": true
    },
    "cache": {
      "toolsListTtlSecs": 300
    }
  }
}
```

This ensures:
- **Low latency** for interactive use
- **Connection reuse** for multiple tool calls
- **Graceful retries** on transient failures
- **Capability caching** to reduce round-trips

## Compliance

✅ **AGENTS.md Section 4b Compliant:**
- External client (Factory/Droid) → `:3003` ✓
- Never uses `127.0.0.1:11436` (loopback only) ✓
- WireGuard identity for Ghostbridge auth ✓

## See Also

- [COGNITIVE_MCP_CLIENT_GUIDE.md](./COGNITIVE_MCP_CLIENT_GUIDE.md) - Full client optimization guide
- [AGENTS.md](../AGENTS.md) - Architecture rules and constraints
- `scripts/verify-factory-cognitive-mcp.sh` - Verification script
