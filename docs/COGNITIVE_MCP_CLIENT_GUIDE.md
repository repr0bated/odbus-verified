# Cognitive MCP Client Configuration & Optimization Guide

## Overview

This guide covers how to add **cognitive-mcp** to your MCP configuration and optimize its use by MCP clients.

Per AGENTS.md Section 4b:
- **cognitive-mcp** (`:3003`, Netmaker WG IP `100.90.37.254`) is the **universal gateway for ALL external clients**: NotebookLM, Droid (factory.ai), Cursor, Codex, Junie, Gemini CLI
- **compact-mcp** (`127.0.0.1:11436`) is **loopback/chatbot only** — never expose externally

## Quick Start

### 1. Configuration File

Add to your MCP client configuration (e.g., `~/.config/mcp/config.json`):

```json
{
  "mcpServers": {
    "cognitive-mcp": {
      "type": "http",
      "url": "http://100.90.37.254:3003",
      "auth": {
        "type": "bearer",
        "tokenEnv": "COGNITIVE_MCP_TOKEN"
      },
      "requestTimeout": 30000,
      "connectionPool": {
        "maxIdle": 10,
        "idleTimeoutMs": 90000
      },
      "retry": {
        "maxRetries": 3,
        "baseDelayMs": 100,
        "maxDelayMs": 5000,
        "backoffMultiplier": 2.0,
        "useJitter": true
      },
      "cache": {
        "toolsListTtlSecs": 300,
        "resourcesListTtlSecs": 60
      }
    }
  }
}
```

### 2. Environment Variables

```bash
# Required: WireGuard identity for Ghostbridge auth
export WG_INTERFACE=netmaker
export WG_PUBKEY=$(wg show netmaker public-key)

# Optional: Custom endpoint (defaults to 100.90.37.254:3003)
export COGNITIVE_MCP_URL=http://100.90.37.254:3003

# Optional: Quota tier (affects rate limiting awareness)
export COGNITIVE_MCP_TIER=standard  # Options: free, standard, high_throughput
```

### 3. Rust Client Integration

```rust
use op_cognitive_mcp::client_config::{
    ClientConfig, CognitiveMcpClient, CognitiveMcpClientFactory,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client configuration for external client type
    let config = ClientConfig::for_external_client("my-droid-instance-1");

    // Or use pre-configured factory profiles
    let config = CognitiveMcpClientFactory::low_latency("cursor-session");
    // Options: low_latency(), high_throughput(), resilient(), for_notebooklm()

    // Connect with optimizations
    let client = CognitiveMcpClient::connect(config).await?;

    // Use cached tool discovery (saves round-trips)
    let tools = client.list_tools_cached().await?;

    // Store memory
    client.store_memory(
        "project:my-app",
        "context",
        serde_json::json!({"key": "value"}),
        vec!["important".into()],
    ).await?;

    // RAG search
    let results = client.rag_search("architecture patterns", Some(5)).await?;

    Ok(())
}
```

## Optimization Strategies

### 1. Connection Pooling

**Purpose**: Reuse HTTP/SSE connections to reduce latency

```rust
let mut config = ClientConfig::for_external_client("client-1");
config.pool = PoolConfig {
    max_idle: 20,              // Keep more connections ready
    idle_timeout_secs: 300,    // Keep alive longer for bursty traffic
    max_connections: 100,      // Max concurrent connections
    use_http2: true,         // Enable HTTP/2 multiplexing
};
```

**Client-specific recommendations**:
- **Cursor**: `max_idle: 20` (interactive, low latency)
- **Codex**: `max_connections: 200` (batch operations)
- **NotebookLM**: `max_idle: 5` (lower frequency)

### 2. Capability Caching

**Purpose**: Cache tool/resource discovery to reduce round-trips

```rust
let mut config = ClientConfig::for_external_client("client-1");
config.cache = CacheConfig {
    tools_cache_ttl_secs: 300,      // Cache tools for 5 minutes
    resources_cache_ttl_secs: 60,   // Cache resources for 1 minute
    max_entries: 1000,
    stale_while_revalidate: true,   // Return stale while fetching fresh
};
```

**Usage**:
```rust
// First call hits the server
let tools = client.list_tools_cached().await?;

// Second call returns from cache (microseconds vs milliseconds)
let cached_tools = client.list_tools_cached().await?;

// Force refresh when needed
client.invalidate_cache().await;
```

### 3. Batched Operations

**Purpose**: Parallelize independent tool calls

```rust
let calls = vec![
    ("memory/store", json!({"namespace": "n1", "key": "k1", "value": "v1"})),
    ("memory/store", json!({"namespace": "n1", "key": "k2", "value": "v2"})),
    ("rag/search", json!({"query": "patterns"})),
];

let results = client.call_tools_batch(calls).await?;
// Executes all calls in parallel, not sequentially
```

### 4. Circuit Breaker

**Purpose**: Fail-fast on degraded service, prevent cascading failures

```rust
let mut config = ClientConfig::for_external_client("client-1");
config.circuit_breaker = CircuitBreakerConfig {
    failure_threshold: 5,      // Open after 5 failures
    success_threshold: 3,      // Close after 3 successes (half-open)
    timeout_secs: 30,          // Try reset after 30 seconds
};
```

**Behavior**:
- **Closed**: Normal operation
- **Open**: Fail immediately without calling server
- **Half-Open**: Test with limited traffic

### 5. Exponential Backoff with Jitter

**Purpose**: Graceful retry without thundering herd

```rust
let mut config = ClientConfig::for_external_client("client-1");
config.retry = RetryConfig {
    max_retries: 3,
    base_delay_ms: 100,
    max_delay_ms: 5000,
    backoff_multiplier: 2.0,
    use_jitter: true,  // Add randomness to prevent synchronized retries
};
```

**Delay sequence**: 100ms, ~230ms (200ms + 15% jitter), ~460ms, ..., max 5s

## Client-Specific Profiles

### Droid (Factory.ai)

```rust
let config = ClientConfig::for_external_client("droid-instance-1");
// Uses default optimized settings
// - Endpoint: 100.90.37.254:3003
// - Quota: 1000/day (standard tier)
// - Connection pooling: 10 idle
// - Retry: 3 attempts with exponential backoff
```

### Cursor

```rust
let config = CognitiveMcpClientFactory::low_latency("cursor-session-1");
// Optimized for:
// - Low latency (5s timeout)
// - 20 idle connections
// - Short cache TTL (1 min) for frequent updates
// - Minimal retries (1)
```

### Codex

```rust
let config = CognitiveMcpClientFactory::high_throughput("codex-batch");
// Optimized for:
// - High volume (10000/day quota)
// - Batch operations
// - 200 max connections
// - Longer timeouts (30s)
```

### NotebookLM

```rust
let config = CognitiveMcpClientFactory::for_notebooklm("session-abc123");
// Per R11 spec:
// - Endpoint: 100.90.37.254:3003
// - Quota: 50 queries/day (free tier)
// - Graceful quota exceeded handling
```

### Gemini CLI

```rust
let config = ClientConfig::for_external_client("gemini-cli-1");
// Features:
// - Standard tier quota
// - Gemini fallback on cognitive-mcp unavailability
// - HTTP/SSE transport (not stdio)
```

### Local Chatbot (Loopback Only)

```rust
let config = ClientConfig::for_local_chatbot("local-bot-1");
// Constraints:
// - Endpoint: 127.0.0.1:11436 ONLY
// - Loopback validation enforced
// - No WireGuard auth required
// - Higher rate limits (unlimited)
```

## Security Considerations

### Ghostbridge Authentication

All external clients must inject identity headers:

```rust
let config = ClientConfig::for_external_client("client-1")
    .with_wg_pubkey("abcd1234...");

// Headers automatically added:
// - X-Ghostbridge-Footprint: <wg_pubkey>
// - X-Ghostbridge-Trace-ID: <session_uuid>
// - X-Client-ID: <client_id>
// - X-Client-Type: External
```

### Validation Rules

1. **External clients** must use port `:3003` endpoint
2. **Local chatbot** must use `127.0.0.1` loopback
3. **D-Bus native** clients bypass HTTP entirely

```rust
// This will fail validation
let bad_config = ClientConfig {
    client_type: ClientType::LocalChatbot,
    endpoint: "http://192.168.1.1:11436".into(),  // NOT loopback!
    // ...
};
assert!(bad_config.validate().is_err());
```

## Monitoring & Observability

### Client Statistics

```rust
let stats = client.stats().await;
println!("Total requests: {}", stats.total_requests);
println!("Quota remaining: {}/{}", stats.quota_remaining, stats.quota_limit);
```

### Tracing Integration

Enable debug logging:

```rust
let config = ClientConfig::for_external_client("client-1");
config.debug = true;

// Logs:
// - Connection establishment
// - Cache hits/misses
// - Retry attempts
// - Circuit breaker state transitions
// - Quota consumption
```

## Troubleshooting

### Connection Refused

**Cause**: Wrong endpoint
**Solution**: Verify using `100.90.37.254:3003` for external clients

### Quota Exceeded

**Error**: `"Quota exceeded: 50/50 queries used"`
**Solution**: Wait for UTC midnight reset or request tier upgrade

### Circuit Breaker Open

**Error**: `"Circuit breaker is OPEN"`
**Solution**: Check cognitive-mcp server health; auto-recovers after 30s

### Stale Capabilities

**Symptom**: New tools not appearing
**Solution**: `client.invalidate_cache().await;`

## Architecture Reference

```
┌─────────────────────────────────────────────────────────────────┐
│                      External Clients                            │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │
│  │  Droid  │ │ Cursor  │ │  Codex  │ │ Gemini  │ │Notebook │   │
│  │(factory)│ │         │ │         │ │  CLI    │ │   LM    │   │
│  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘   │
│       │           │           │           │           │         │
│       └───────────┴───────────┴───────────┴───────────┘         │
│                         │                                       │
│                         ▼                                       │
│              ┌─────────────────────┐                           │
│              │  cognitive-mcp:3003 │  ← Universal Gateway      │
│              │  (100.90.37.254)    │                           │
│              └──────────┬──────────┘                           │
│                         │                                       │
│       ┌─────────────────┼─────────────────┐                     │
│       ▼                 ▼                 ▼                     │
│  ┌─────────┐       ┌─────────┐       ┌─────────┐                │
│  │ Memory  │       │   RAG   │       │ Session │                │
│  │ (CozoDB)│       │(Qdrant) │       │ Manager │                │
│  └─────────┘       └─────────┘       └─────────┘                │
└─────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Local (Loopback)                           │
│              ┌─────────────────────┐                           │
│              │   compact-mcp:11436 │  ← Chatbot Only           │
│              │   (127.0.0.1)       │    NEVER expose           │
│              └─────────────────────┘                           │
└─────────────────────────────────────────────────────────────────┘
```

## See Also

- [AGENTS.md](../AGENTS.md) - Master configuration and architecture rules
- [client_config.rs](../crates/op-cognitive-mcp/src/client_config.rs) - Implementation
- [examples/external_client.rs](../crates/op-cognitive-mcp/examples/external_client.rs) - Working example
- [deploy/config/cognitive-mcp-clients.json](../deploy/config/cognitive-mcp-clients.json) - Deployment profiles
