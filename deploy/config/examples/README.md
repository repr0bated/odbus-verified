# Unified op-dbus MCP client examples

Every example in this directory targets the same bridge-owned Streamable HTTP
endpoint:

```text
https://10.0.0.3:8090/mcp
```

The listener also serves native gRPC, gRPC-Web, and authenticated reflection on
port `8090`. Cognitive memory, coding context, agent tools, and blob-schema
resources are projections of the same sealed catalog; they are not separate MCP
servers.

## Authentication

The client-side identity broker must attach a fresh
`x-oracle-identity-assertion-bin` value to every request. Static bearer tokens,
`X-Ghostbridge-Footprint`, `X-Ghostbridge-Trace-ID`, and MCP session identifiers
are not authentication.

## Files

The files preserve the configuration shape expected by each named client while
declaring only `op-dbus-unified`. Copy the matching example into the client's MCP
configuration and configure its OIA broker and the op-dbus TLS CA.

## Smoke test

Use the repository acceptance client so it can mint a valid assertion and verify
TLS. An unauthenticated `curl` request is expected to return `401` and is a useful
negative test; it is not a connectivity failure.
