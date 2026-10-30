# Cognitive Client Guide

## Source naming and release boundary

The current source package is `op-cognitive`, imported as `op_cognitive`.
Its canonical plugin ID is `cognitive`; MCP remains the name of the transport
protocol, not part of the cognitive component name.

These names describe the replacement source contract, not the currently
installed application release. Applications are not being rebuilt as part of
the deployment-helper update. Keep existing release binaries, catalog, grants
and runtime configuration together until an authorized application rebuild
and matching catalog/configuration cutover. This guide is not a deployment
receipt or authorization to change a live service.

## Client connection

External MCP clients use the single public door:

```text
https://<host>:8090/mcp
```

`op-grpc-bridge` owns this endpoint and the in-process cognitive runtime.
Use the configured host and trusted CA, and forward the existing OIA1 or SID1
credential through the supported client transport. A client label, environment
variable, WireGuard address or toolset selection does not mint an identity or
grant a capability. Do not start a standalone cognitive service or point
clients at the assistant service. Compact chatbot mode is in process in
op-web, not a second public listener.

Configure the endpoint using the client's supported MCP configuration format;
this repository does not define a universal client JSON authentication schema.
See [MCP Gateway](src/reference/mcp-gateway.md) for admission and discovery.

## Discovery and calls

- Follow `tools/list` pagination until `nextCursor` is absent.
- Discover the admitted toolsets and select a warm set before using its tools.
- The cognitive canonical tool prefix is `plugin.cognitive.`, for example
  `plugin.cognitive.code_search`. Use the actual sealed schema for arguments,
  results and the exact required capability; do not copy a historical input.
- Cognitive grants use `cognitive.*`. Renaming a caller does not translate an
  old grant or authorize broader access.
- Provider readiness is separate from catalog membership. An advertised schema
  is not evidence that a provider, memory store or retrieval backend is ready.

Local clients start at their identity-bound 3tchedFS namespace, under
`plugins/cognitive/operations/<method>`. Discover the operation contract and
check its receipt there. When explicitly maintaining the D-Bus infrastructure,
the canonical object is `/org/opdbus/v1/plugins/cognitive`; the generic
interface remains `org.opdbus.v1.PluginV1`.

## Embedded Rust integration

The runtime is `op_cognitive::CognitiveServer`; production bridge initialization
uses `CognitiveServer::new_durable`. It owns no listener. Cozo is the persistent
memory owner, and the durable constructor requires the Qdrant/Voyage semantic
shuttle rather than acknowledging writes to an in-memory fallback.

The public modules and types are documented in
[`crates/op-cognitive/src/lib.rs`](../crates/op-cognitive/src/lib.rs).
The former client-factory and context HTTP-server examples are not current
exported APIs; do not recreate them as compatibility wrappers.

## Configuration and troubleshooting

Replacement source consumes `COGNITIVE_*` environment settings without old-name
aliases. Examples include `COGNITIVE_QDRANT_URL`, `COGNITIVE_RAG_COLLECTION` and
`COGNITIVE_BUS_ADDRESS` (the existing NotebookLM synchronization helper's bus
override). Configure matching replacement applications and catalog together,
not the old application release being deployed now.

For missing tools, inspect identity admission, exact grants, selected toolset,
provider readiness and the sealed catalog before assuming an endpoint problem.
For unavailable memory, inspect durable Cozo and semantic-shuttle readiness;
do not launch another database writer or substitute a separate listener.

## References

- [MCP Gateway](src/reference/mcp-gateway.md)
- [Context Awareness](CONTEXT_AWARENESS.md)
- [Cognitive protocol](reference/proto/op-cognitive/cognitive.proto.md)
- [Runtime implementation](../crates/op-cognitive/src/server.rs)
- [Local release activation](src/getting-started/local-release-activation.md)
