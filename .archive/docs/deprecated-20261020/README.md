# Documentation

These documents describe the current Operation D-Bus architecture. Historical
handoffs, generated conversations, reviews, and retired deployment designs are
intentionally excluded.

## Start here

- [Architecture](overview/architecture.md) — system layers, crate map, service
  boundaries, and the schema-to-reflection pipeline.
- [API reference](reference/api-reference.md) — D-Bus, gRPC, MCP, schema, and
  routing contracts.
- [User guide](guides/user-guide.md) — building, running, inspecting, and
  extending the system.
- [Protocol reference](reference/proto/README.md) — project-owned protobuf
  services and messages.

## Durable design records

- [State flow](architecture/state-flow.md)
- [Mutation paths](operations/mutation-paths.md)
- [Mirror and projection](operations/mirror-projection.md)
- [Schema contracts](schema/plugin-contracts.md)
- [Schema registry coverage](schema/registry-coverage.md)
- [Plugin system overview](plugins/system-overview.md)
- [Plugin catalog](plugins/plugin-catalog.md)
- [Plugin creation guide](plugins/create-and-register.md)
- [Schema-coupled reflection](schema-coupled-plugin-blob-reflection-whitepaper.md)
- [Ghostbridge identity sled](ghostbridge-identity-sled-snowball-pipeline.md)

## Current deployment contract

The canonical fresh-host installer is
[`install/3tched-artix-s6-install.sh`](../install/3tched-artix-s6-install.sh).
Host services are managed exclusively through `sudo service6`.

The network uses the OVS system datapath with host Xray. The consolidated
`op-grpc-bridge` exposes the complete gRPC/Zeroclaw surface on loopback port
8090, and host Xray publishes that endpoint on the uplink.
There is no host WireGuard, AF_XDP path, port 18789, or separate Zeroclaw bridge
service.
