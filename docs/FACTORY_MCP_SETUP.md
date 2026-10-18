# Factory / Droid MCP Setup

Factory/Droid is an external MCP client of the bridge-owned cognitive runtime.
MCP is the protocol; the current source component is `op-cognitive`, with plugin
ID `cognitive` and canonical tool prefix `plugin.cognitive.`.

## Connection contract

Configure Factory's supported MCP client integration for the authenticated
`https://<host>:8090/mcp` bridge door using the configured host and trusted CA.
Forward the existing OIA1 or SID1 credential through the supported transport.
Do not invent client-specific authentication fields, derive credentials from
an address, or substitute a standalone cognitive listener.

The bridge validates identity, exact grants, selected toolset and provider
readiness. Discovery uses `tools/list`; follow `nextCursor` to the last page.
Select an available warm toolset before calling its tools and read each tool's
sealed input/output contract. A client entry or toolset does not grant access.

For cognitive operations, use the discovered `plugin.cognitive.*` names and
matching `cognitive.*` grants. NotebookLM remains a genuine provider/protocol
integration, not a replacement name for the cognitive component.

## Release boundary

This is the replacement source contract, not proof of a deployed Factory
integration. The deployment-helper update does not rebuild applications.
Existing application binaries, sealed catalog, grants and runtime settings
must remain matched until the authorized application rebuild and cutover.
Do not change live grants or services merely to match this guide.

There is no standalone cognitive runit service to start and no current
Factory-specific verifier script supplied by this guide. Operational work
uses the supported identity-bound surfaces and, only when authorized, runit
maintenance of `op-grpc-bridge`; never a foreign service-manager shim.

## References

- [Cognitive Client Guide](COGNITIVE_CLIENT_GUIDE.md)
- [MCP Gateway](src/reference/mcp-gateway.md)
- [Cognitive protocol](reference/proto/op-cognitive/cognitive.proto.md)
- [Local release activation](src/getting-started/local-release-activation.md)
