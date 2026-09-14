# Operation-DBus Comprehensive Specification and Design Documentation

**Generated**: 2026-02-16  
**Status**: Complete architectural documentation for all 39 crates

## Overview

This document indexes comprehensive specifications and design documents for every crate in the operation-dbus project. Each crate has:

1. **SPEC.md** - Complete specification (1000+ lines) covering purpose, architecture, API contracts, data models, error handling, testing strategy, and integration points
2. **DESIGN.md** - Detailed design document (1500+ lines) describing implementation from scratch including module structure, phases, data flow, algorithms, concurrency patterns, and deployment

## Documentation Structure

```
docs/
├── specs/           # Comprehensive specifications
│   ├── op-core.md
│   ├── op-dbus-model.md
│   ├── op-state.md
│   └── ... (39 total)
└── designs/         # Implementation designs
    ├── op-core.md
    ├── op-dbus-model.md
    ├── op-state.md
    └── ... (39 total)
```

## Crate Categories

### Foundation Layer (Core Infrastructure)
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-core** | Core types, errors, execution tracking | [spec](specs/op-core.md) | [design](designs/op-core.md) |
| **op-dbus-model** | D-Bus data models and schemas | [spec](specs/op-dbus-model.md) | [design](designs/op-dbus-model.md) |
| **op-execution-tracker** | Execution audit trail and tracking | [spec](specs/op-execution-tracker.md) | [design](designs/op-execution-tracker.md) |
| **op-tools** | Tool registry and execution framework | [spec](specs/op-tools.md) | [design](designs/op-tools.md) |

### State Management Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-state** | State management and plugin framework | [spec](specs/op-state.md) | [design](designs/op-state.md) |
| **op-state-store** | Persistent state storage | [spec](specs/op-state-store.md) | [design](designs/op-state-store.md) |
| **op-plugins** | System state plugins (network, systemd, OVS) | [spec](specs/op-plugins.md) | [design](designs/op-plugins.md) |
| **op-cache** | Caching layer for state and queries | [spec](specs/op-cache.md) | [design](designs/op-cache.md) |

### D-Bus Integration Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-introspection** | D-Bus introspection and discovery | [spec](specs/op-introspection.md) | [design](designs/op-introspection.md) |
| **op-dbus-mirror** | D-Bus service mirroring and proxying | [spec](specs/op-dbus-mirror.md) | [design](designs/op-dbus-mirror.md) |

### Network Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-network** | Network configuration and management | [spec](specs/op-network.md) | [design](designs/op-network.md) |
| **op-services** | Service lifecycle management | [spec](specs/op-services.md) | [design](designs/op-services.md) |

### AI/LLM Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-llm** | LLM provider abstraction | [spec](specs/op-llm.md) | [design](designs/op-llm.md) |
| **op-chat** | Chat interface and tool orchestration | [spec](specs/op-chat.md) | [design](designs/op-chat.md) |
| **op-ml** | Machine learning utilities | [spec](specs/op-ml.md) | [design](designs/op-ml.md) |

### MCP (Model Context Protocol) Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-mcp** | Core MCP protocol implementation | [spec](specs/op-mcp.md) | [design](designs/op-mcp.md) |
| **op-mcp-proxy** | MCP HTTP proxy server | [spec](specs/op-mcp-proxy.md) | [design](designs/op-mcp-proxy.md) |
| **op-mcp-aggregator** | Multi-server MCP aggregation | [spec](specs/op-mcp-aggregator.md) | [design](designs/op-mcp-aggregator.md) |
| **op-cognitive-mcp** | Cognitive processing with MCP | [spec](specs/op-cognitive-mcp.md) | [design](designs/op-cognitive-mcp.md) |

### Agent & Workflow Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-agents** | Agent lifecycle and management | [spec](specs/op-agents.md) | [design](designs/op-agents.md) |
| **op-workflows** | Workflow orchestration engine | [spec](specs/op-workflows.md) | [design](designs/op-workflows.md) |

### API & Gateway Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-gateway** | API gateway with routing | [spec](specs/op-gateway.md) | [design](designs/op-gateway.md) |
| **op-http** | HTTP client/server utilities | [spec](specs/op-http.md) | [design](designs/op-http.md) |
| **op-grpc-bridge** | gRPC-HTTP protocol bridge | [spec](specs/op-grpc-bridge.md) | [design](designs/op-grpc-bridge.md) |
| **op-jsonrpc** | JSON-RPC protocol implementation | [spec](specs/op-jsonrpc.md) | [design](designs/op-jsonrpc.md) |
| **op-web** | Web UI and frontend integration | [spec](specs/op-web.md) | [design](designs/op-web.md) |

### Infrastructure Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-deployment** | Deployment automation | [spec](specs/op-deployment.md) | [design](designs/op-deployment.md) |
| **op-snowball** | Snowball audit trail | [spec](specs/op-snowball.md) | [design](designs/op-snowball.md) |
| **op-identity** | Identity and authentication | [spec](specs/op-identity.md) | [design](designs/op-identity.md) |
| **op-dynamic-loader** | Dynamic plugin loading | [spec](specs/op-dynamic-loader.md) | [design](designs/op-dynamic-loader.md) |
| **op-inspector** | System inspection and diagnostics | [spec](specs/op-inspector.md) | [design](designs/op-inspector.md) |

### CLI & Tooling Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-cli** | Command-line interface | [spec](specs/op-cli.md) | [design](designs/op-cli.md) |
| **op-api** | API client library | [spec](specs/op-api.md) | [design](designs/op-api.md) |
| **op-parser** | Configuration parsing | [spec](specs/op-parser.md) | [design](designs/op-parser.md) |

### Storage & Data Layer
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-storage** | Storage abstraction layer | [spec](specs/op-storage.md) | [design](designs/op-storage.md) |
| **op-worker** | Background worker system | [spec](specs/op-worker.md) | [design](designs/op-worker.md) |

### Benchmarking & Testing
| Crate | Purpose | Spec | Design |
|-------|---------|------|--------|
| **op-benchmark** | Performance benchmarking | [spec](specs/op-benchmark.md) | [design](designs/op-benchmark.md) |
| **op-json-benchmark** | JSON serialization benchmarks | [spec](specs/op-json-benchmark.md) | [design](designs/op-json-benchmark.md) |

## Key Architectural Principles

### 1. gRPC-First Communication
All internal communication uses gRPC where possible for type safety and performance.

### 2. SIMD JSON Serialization
Uses `simd-json` instead of `serde_json` for 2-3x faster JSON processing.

### 3. D-Bus Native Integration
Direct D-Bus protocol integration without CLI wrappers for systemd, NetworkManager, etc.

### 4. Plugin Architecture
Extensible plugin system for state management with automatic discovery and validation.

### 5. Execution Tracking
Comprehensive audit trail for all tool and agent executions with snowball anchoring.

### 6. MCP Integration
Full Model Context Protocol support for LLM tool integration and agent communication.

## Documentation Standards

Each specification includes:
- **Purpose & Scope**: What the crate does and doesn't do
- **Architecture**: High-level design and component relationships
- **API Contracts**: Public interfaces with examples
- **Data Models**: Core types and their relationships
- **Error Handling**: Error types and recovery strategies
- **Testing Strategy**: Unit, integration, and system test approaches
- **Integration Points**: How it connects to other crates
- **Performance Considerations**: Scalability and optimization notes
- **Security Model**: Authentication, authorization, and data protection

Each design document includes:
- **Module Structure**: File organization and module hierarchy
- **Implementation Phases**: Step-by-step build plan (typically 4-6 phases)
- **Data Flow Diagrams**: Request/response flows and state transitions
- **Algorithm Details**: Core algorithms with pseudocode
- **Concurrency Patterns**: Threading, async, and synchronization strategies
- **Testing Approach**: Test structure and coverage strategy
- **Build & Deployment**: Compilation, packaging, and deployment steps
- **Migration Path**: How to transition from current implementation

## Usage

### For New Development
1. Read the SPEC.md to understand requirements and contracts
2. Follow the DESIGN.md phase-by-phase implementation plan
3. Reference integration points for cross-crate dependencies

### For Maintenance
1. Check SPEC.md for intended behavior and contracts
2. Use DESIGN.md to understand implementation details
3. Verify changes don't break documented contracts

### For Architecture Review
1. Review SPEC.md files for system-wide understanding
2. Check integration points for dependency analysis
3. Use this index to navigate the crate hierarchy

## Generation Methodology

These documents were generated through:
1. **Code Analysis**: Deep inspection of Cargo.toml, source files, and tests
2. **Pattern Recognition**: Identifying common patterns and architectural decisions
3. **Gap Analysis**: Comparing intended vs actual implementation
4. **Best Practices**: Applying Rust and distributed systems best practices
5. **Completeness**: Ensuring every aspect is documented thoroughly

## Maintenance

This documentation should be updated when:
- New crates are added to the workspace
- Major architectural changes occur
- API contracts change significantly
- New integration patterns emerge

---

*Generated as part of comprehensive architecture documentation initiative*
*Reference: docs/planning/op-chat-review.md for quality standards*

---

## Consolidated documentation added 2026-07-20

The following documents were consolidated from the inspect copy under `/mnt/opt-inspect/home/git/operation-dbus-proto/` and corrected against the current codebase.

### Operational runbooks
- [Artix runit service recovery](operations/artix-runit-recovery.md) — diagnosis order, stuck `supervise/` recovery, runlevel switching, and the systemd-unit converter
- [OVS native JSON-RPC guide](guides/ovs-native-jsonrpc.md) — using `op_network::OvsdbClient` over the D-Bus rovs service

### Architecture and design
- [Blob architecture appendix](schema-coupled-plugin-blob-reflection-whitepaper-appendix.md) — current state of `PluginObjectBlob`, `ActiveReflectionCatalog`, and remaining blob-deployment work
- [Privacy network architecture](architecture/privacy-network-architecture.md) — wgcf/WARP privacy routing design
- WireGuard identity principles — not promoted; the extract contained major inaccuracies against current `op-identity` and remains in `.consolidation-staging/docs-stale-excerpts/major-rewrite/`

### Historical review and planning
- [Feature review matrix](feature-review/README.md) — per-crate build/feature review from 2026-02-16
- Collected code reviews — historical review transcripts under `collected-code-reviews/`
- [Factory MCP setup](FACTORY_MCP_SETUP.md) — Factory/Droid MCP configuration guide
- [Kiro spec workflow](kiro-spec-workflow.md) — Kiro spec workflow guide
- [Code-assist escalation](operations/code-assist-escalation-2026-02-11.md) — historical escalation note
- Planning notes — historical planning and review docs under `planning/`
- [dbus-mirror session refactor prompt](prompts/dbus-mirror-event-session-refactor.md) — one-time task prompt

### Stale excerpts archived
The following extracted documents were found to contain major inaccuracies against the current codebase and were moved to `.consolidation-staging/docs-stale-excerpts/major-rewrite/` rather than promoted:
- `xdp-debugging-patterns.md` — references forbidden host AF_XDP binaries that no longer exist
- `wireguard-identity-principles.md` — crypto model and several identity claims do not match current `op-identity` implementation
- `deploy-readme-evolution.md` — describes a deployment structure that no longer exists
