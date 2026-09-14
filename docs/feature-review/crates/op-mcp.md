# op-mcp Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-mcp` passed
- Tests in tree: 19
- Static incompleteness markers: 8
- Patch / backup artifacts in tree: 3
- Purpose: Unified MCP Protocol Server with multiple transport and mode support
- Assessment: op-mcp builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-mcp/SPEC.md`
- `crates/crates/SPECS/22-op-mcp.md`

## Coded Features
- Public/module surface: agents_server, compact, protocol, resources, server, transport, tool_registry, grpc
- Source files under `src/` recursively: 40

## Alignment Review
- Compared against `crates/crates/op-mcp/SPEC.md` and `crates/crates/SPECS/22-op-mcp.md` plus the crate source tree.

## Missing Or Risky Areas
- The crate provides a wide protocol surface, but resource serving and some tool integrations are still placeholder-level, and the gRPC service still reports streaming tool-call support as unimplemented.
- Static scan found 8 TODO/stub/placeholder markers in this crate.
- Static scan found 3 patch/backup artifact files checked into the crate tree.

## Verification Notes
- `cargo check -p op-mcp` passed
- Static scan counted 19 test markers and 8 TODO/stub markers in this crate.
- Static scan also found 3 patch/backup artifacts in the crate tree.

