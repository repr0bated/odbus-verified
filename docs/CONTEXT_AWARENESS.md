# Context Awareness & Proactive Knowledge Pushing

## Overview

The cognitive-mcp server now includes **context awareness** capabilities that monitor session activity and proactively push relevant knowledge to clients via SSE (Server-Sent Events) streams.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      MCP Client (Droid/Cursor/etc)                     │
│                        Subscribes to SSE stream                          │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │ HTTP/SSE
                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│              cognitive-mcp :3003 (Context-Aware Endpoints)             │
│                                                                          │
│  GET /context/stream/:session_id     → SSE push stream (proactive)     │
│  POST /context/record                → Record activity                 │
│  POST /context/request_push          → On-demand knowledge             │
│  GET /context/status/:session_id     → Session context stats           │
│                                                                          │
└──────────────────────────────┬──────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    Context Awareness Engine                              │
│                                                                          │
│  • Session Activity Monitor (sliding window of events)                 │
│  • Pattern Detection (stuck sessions, new topics, errors)                │
│  • Knowledge Trigger Engine (evaluates when to push)                   │
│  • RAG Retriever (semantic search for relevant content)                │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Trigger Types

The engine detects various patterns and pushes knowledge accordingly:

| Trigger | Description | Example Use Case |
|---------|-------------|------------------|
| `new_topic_detected` | User switched to a new domain/area | "You switched from DB queries to React components - here's the React architecture guide" |
| `stuck_session` | Repeated similar queries detected | "You seem stuck on connection pooling - here are working examples and troubleshooting steps" |
| `context_gap` | Query appears incomplete | "Your query seems incomplete - here's what you might need to know about X" |
| `pattern_match` | Usage suggests need for specific resource | "You're implementing auth - here's the JWT middleware pattern" |
| `session_milestone` | Nth query reached | "You've made 10 queries in this session - here's a summary of your work" |
| `idle_recovery` | User returned after idle | "Welcome back - here's what you were working on" |
| `tool_guidance` | Tool usage patterns detected | "Tip: You can use the memory/store tool to persist this context" |
| `error_assistance` | Multiple errors detected | "I see you're hitting connection errors - here's the troubleshooting guide" |

## Client Integration

### 1. Subscribe to Push Stream

```javascript
const sessionId = 'my-session-' + Date.now();
const eventSource = new EventSource(
  `http://100.90.37.254:3003/context/stream/${sessionId}`
);

eventSource.addEventListener('knowledge_push', (event) => {
  const push = JSON.parse(event.data);
  console.log(`🧠 ${push.trigger}: ${push.trigger_reason}`);
  console.log('Content:', push.content);
  console.log('Suggested action:', push.suggested_action);
});
```

### 2. Record Activities

Report user actions to enable context awareness:

```javascript
await fetch('http://100.90.37.254:3003/context/record', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    session_id: sessionId,
    activity_type: 'query',  // or: tool_call, context_switch, error, return_from_idle
    content: 'How do I configure connection pooling?',
    metadata: { tool_used: 'memory/retrieve' }
  })
});
```

### 3. Request On-Demand Push

```javascript
const response = await fetch('http://100.90.37.254:3003/context/request_push', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    session_id: sessionId,
    query: 'Best practices for async Rust',
    context: { current_file: 'server.rs' }
  })
});

const result = await response.json();
```

## Push Content Types

Each knowledge push contains structured content:

### RAG Results
```json
{
  "type": "rag_results",
  "query": "connection pooling",
  "results": [
    { "score": 0.92, "file_path": "crates/op-db/src/pool.rs", "content": "..." }
  ],
  "sources": ["op-dbus:crates/op-db/src/pool.rs"]
}
```

### Memory Entries
```json
{
  "type": "memory_entries",
  "namespace": "project:op-dbus",
  "entries": [...],
  "context": "Recent work on database configuration"
}
```

### Context Summary
```json
{
  "type": "context_summary",
  "session_overview": "You've been working on database connection pooling",
  "related_namespaces": ["project:op-db", "session:current"],
  "suggested_next_steps": ["Review connection pool limits", "Check async patterns"]
}
```

### Tool Guidance
```json
{
  "type": "tool_guidance",
  "tool_name": "memory/store",
  "usage_tips": ["Use namespaces for organization", "Add tags for searchability"],
  "example_usage": { "namespace": "project:x", "key": "context", "value": {} }
}
```

### Error Assistance
```json
{
  "type": "error_assistance",
  "error_pattern": "database connection timeout",
  "resolution_steps": ["Check pool size", "Verify network connectivity", "Review logs"],
  "related_docs": ["troubleshooting_guide", "connection_pooling_docs"]
}
```

## Configuration

### Engine Configuration

```rust
use op_cognitive_mcp::context_awareness::ContextAwarenessConfig;

let config = ContextAwarenessConfig {
    proactive_enabled: true,
    pattern_detection_enabled: true,
    min_relevance_score: 0.75,      // Only push if relevance >= 75%
    push_rate_limit_secs: 30,        // Max 1 push per 30 seconds per session
    idle_threshold_secs: 300,        // Consider idle after 5 minutes
    max_pushes_per_hour: 20,         // Max 20 proactive pushes per session per hour
};
```

### Server Integration

The context awareness is integrated into the HTTP server:

```rust
use op_cognitive_mcp::context_server::create_context_aware_server;

let (router, engine) = create_context_aware_server(
    memory_store,
    session_manager,
    rag_pipeline,
).await?;

// Engine runs background monitoring
// Router provides SSE endpoints
```

## Example: Rust Client

See full example: `crates/op-cognitive-mcp/examples/context_aware_client.rs`

```bash
# Run the context-aware client demo
cargo run --example context_aware_client -- --session-id demo-1 --simulate-stuck
```

```rust
use op_cognitive_mcp::context_awareness::{
    ActivityEvent, ActivityType, ContextAwarenessEngine
};

// Create engine
let engine = Arc::new(ContextAwarenessEngine::new(
    config,
    memory_store,
    rag_pipeline,
));

// Subscribe to pushes
let mut push_rx = engine.subscribe_pushes();

// Record activity
engine.record_activity(
    "session-1",
    ActivityType::Query,
    "How to implement auth?",
    json!({}),
).await;

// Receive proactive push
while let Ok(push) = push_rx.recv().await {
    println!("Received: {:?}", push.trigger);
}
```

## Rate Limiting & Anti-Spam

The engine implements several safeguards:

1. **Push Cooldown**: Minimum 30 seconds between pushes to same session
2. **Hourly Limit**: Max 20 proactive pushes per session per hour
3. **Relevance Threshold**: Only push if score >= 75%
4. **Trigger Suppression**: Same trigger type won't fire repeatedly
5. **Client ACK**: Clients can acknowledge pushes to suppress similar future ones

## Session Status

Query current context status:

```bash
curl http://100.90.37.254:3003/context/status/my-session-1
```

Response:
```json
{
  "session_id": "my-session-1",
  "activity_count": 15,
  "recent_error_count": 0,
  "is_idle": false,
  "can_push": true,
  "current_topics": ["database", "async"],
  "suppressed_triggers": []
}
```

## Health Check

```bash
curl http://100.90.37.254:3003/context/health
```

## Factory/Droid Integration

The context awareness automatically integrates with Factory when using the cognitive-mcp gateway:

1. Factory subscribes to SSE stream on connection
2. User activities (tool calls, queries) are automatically recorded
3. Knowledge pushes appear in the conversation context
4. Suggested actions can trigger follow-up prompts

```
User: How do I fix this error?
[Context engine detects stuck pattern]
Droid: 🧠 It looks like you're stuck on this error pattern. 
      Here's a guide that might help: [Knowledge Push]
      Would you like me to try these resolution steps?
```

## Testing

Run the demo client with simulation flags:

```bash
# Simulate stuck session pattern
cargo run --example context_aware_client -- --simulate-stuck

# Simulate error pattern
cargo run --example context_aware_client -- --simulate-errors

# Full demo with all patterns
cargo run --example context_aware_client -- \
  --session-id test-123 \
  --duration 120 \
  --simulate-stuck \
  --simulate-errors \
  --verbose
```

## Troubleshooting

### No pushes received
- Verify SSE connection: Check `eventSource.onopen` fires
- Check session ID matches between record calls and stream
- Review server logs for trigger evaluations

### Too many pushes
- Adjust `push_rate_limit_secs` in config
- Increase `min_relevance_score`
- Check for trigger suppression working

### Wrong knowledge pushed
- Tune RAG query generation in `retrieve_relevant_knowledge()`
- Adjust relevance scoring thresholds
- Review memory content quality

## API Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/context/stream/:session_id` | GET | SSE stream for proactive pushes |
| `/context/record` | POST | Record activity for context tracking |
| `/context/request_push` | POST | Request on-demand knowledge push |
| `/context/status/:session_id` | GET | Get session context statistics |
| `/context/health` | GET | Health check for context system |

## See Also

- [COGNITIVE_MCP_CLIENT_GUIDE.md](./COGNITIVE_MCP_CLIENT_GUIDE.md) - Client configuration
- [AGENTS.md](../AGENTS.md) - Architecture rules
- `examples/context_aware_client.rs` - Working client example
