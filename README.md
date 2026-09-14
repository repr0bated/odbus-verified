# 3tched Control Plane

Native, deterministic control plane for Artix Linux infrastructure.

For local workspace preparation, start with the [verified workspace
guide](docs/src/getting-started/verified-workspace.md). Repository architecture
and agent requirements are in [AGENTS.md](AGENTS.md).

## Architecture

- **Host**: Artix Linux + runit service supervision, controlled with `sv` (NOT systemd, NOT s6)
- **Network**: OVS switching fabric via `rovs` crate suite — native OVSDB JSON-RPC, no CLI subprocess
- **Containers**: Incus — privacy services (Xray, mail) run inside containers with Unix sockets + OpenFlow routing
- **Storage**: CozoDB for embedded data; btrfs send/receive transports prepared release snapshots
- **AI**: Model/provider routing with per-container memory (CozoDB + Qdrant semantic search)

## Workspace

The workspace contains 37 member crates under `crates/` plus the root package
(see AGENTS.md for membership, excluded manifests, and current binary targets):

| Crate              | Role                                                 |
| ------------------ | ---------------------------------------------------- |
| `op-web`           | Unified HTTP server + chat UI                        |
| `op-chat`          | Chat actor with memory loop, forced tool pipeline    |
| `op-llm`           | LLM provider management                              |
| `op-network`       | OVSDB + OpenFlow + rtnetlink (native protocols only) |
| `op-cognitive-mcp` | CozoDB memory store, Qdrant semantic shuttle         |
| `op-cozo-store`    | CozoDB embedded graph database                       |
| `op-grpc-bridge`   | gRPC service bridge                                  |
| `op-identity`      | Identity and SealedId credential formats             |
| `op-state-store`   | Plugin schema engine                                 |

## Quick Start

```bash
# Check a provisioned Linux development environment
sh scripts/check-workspace.sh preflight

# Compile and run backend smoke tests
sh scripts/check-workspace.sh smoke

# Release build (needs the cstdint shim for vendored RocksDB)
CXXFLAGS="-include cstdint" cargo build --locked --workspace --release

# Complete formatting, Clippy and workspace test gate
sh scripts/check-workspace.sh verify
```

The active UI is maintained in a separate repository and served as static
files by op-web. The in-repository UI trees are retired and are not build
prerequisites. Starting the control plane requires provisioned services,
identity state and a sealed plugin catalog; see AGENTS.md for host operations.

## Key Principles

1. **Native protocols only** — OVSDB JSON-RPC, not `ovs-vsctl`; Generic Netlink, not `ip` commands
2. **Scoped identity and memory** — the assistant is the singleton control-plane chatbot. Human identity, login session and workspace container are separate; private state requires verified scope.
3. **Schema and state ownership** — each plugin owns its schema, sealed into its plugin blob. MutationEngine commits state, including children, to SHM before notifying subscribers.
4. **D-Bus control plane** — generated gRPC and MCP calls use the canonical plugin dispatch. External binaries back plugins through their declared adapters.

## Documentation

- `AGENTS.md` — agent guidance, build gotchas, host-service policy
- `SIGNALS.md` — live model observations (append-only)
- `WISHLIST.md` — task board (OD-## ids)
- `docs/` — architecture docs
- `deploy/runit/` — runit service definitions

## License

Apache-2.0
