# Context Awareness and Proactive Knowledge

## Current source boundary

Context awareness lives in `op_cognitive::context_awareness`, inside the
bridge-owned `op-cognitive` runtime. The canonical plugin is `cognitive`;
external tools use `plugin.cognitive.*` through the authenticated MCP door in
`op-grpc-bridge`. Cognitive does not own a standalone HTTP listener.

This describes replacement source, not installed application behavior. The
current deployment-helper rebuild does not rebuild applications or authorize
changing their catalog, runtime environment or grants.

## Engine integration

[`ContextAwarenessEngine`](../crates/op-cognitive/src/context_awareness.rs)
tracks activity and produces knowledge pushes. Its public integration surface
includes:

- `ContextAwarenessConfig` for proactive pushing and pattern detection.
- `ActivityEvent` and `ActivityType` for session activity.
- `activity_sender()` and `record_activity()` to submit activity.
- `start_monitoring()` to process arriving activity; idle sessions do not
  require a polling loop.
- `get_session_signals()` for context signals used by the code-context tool.
- `subscribe_pushes()` for in-process knowledge-push receivers.
- `request_push()` for an explicit retrieval request.
- `get_session_stats()` and `reset_suppressions()` for engine state.

The runtime owns the engine alongside Cozo-backed memory and the optional
code-RAG pipeline. Its production `CognitiveServer::new_durable` constructor
requires the semantic shuttle and reports unavailable durable dependencies
rather than silently serving ephemeral memory.

## Client and identity boundary

External clients use the bridge's `https://<host>:8090/mcp` endpoint and their
existing OIA1 or SID1 credential. Local clients use their identity-bound
3tchedFS operation surfaces. Discover the sealed operation contract and exact
grants before invoking cognitive methods; neither an engine session string
nor an activity event grants access to memory.

An in-process broadcast subscription is not proof of an admitted per-user
remote delivery channel. Private delivery must retain principal, session and
project scoping. See the [identity boundaries](src/architecture/identity-boundaries.md)
for the documented isolation constraints and unresolved evidence.

The former standalone context HTTP-server and client-factory examples are not
current exported modules. Do not recreate a `/context/*` listener or assume
that Factory automatically subscribes to knowledge pushes. Library APIs and
source declarations alone do not establish deployed end-to-end delivery.

## References

- [Cognitive Client Guide](COGNITIVE_CLIENT_GUIDE.md)
- [MCP Gateway](src/reference/mcp-gateway.md)
- [Runtime implementation](../crates/op-cognitive/src/server.rs)
- [Code-context tools](../crates/op-cognitive/src/code_tools.rs)
