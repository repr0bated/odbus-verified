# op-chat Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-chat` passed
- Tests in tree: 56
- Static incompleteness markers: 28
- Patch / backup artifacts in tree: 2
- Purpose: Chat functionality and LLM integration for op-dbus-v2
- Assessment: op-chat builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-chat/SPEC.md`
- `crates/crates/SPECS/04-op-chat.md`

## Coded Features
- Public/module surface: actor, agent_tools, forced_execution, forced_tool_pipeline, mcp_server, nl_admin, orchestration, session, system_prompt, tool_executor
- Source files under `src/` recursively: 32

## Alignment Review
- Compared against `crates/crates/op-chat/SPEC.md` and `crates/crates/SPECS/04-op-chat.md` plus the crate source tree.

## Missing Or Risky Areas
- The chat/orchestration surface is broad, but multiple paths are still stubbed: SSE streaming is TODO, agent execution is stubbed, and several gRPC pool calls are placeholders.
- The MCP server implementation still returns `Status::unimplemented` for subscribe/streaming/tool-call streaming flows.
- Static scan found 28 TODO/stub/placeholder markers in this crate.
- Static scan found 2 patch/backup artifact files checked into the crate tree.

## Verification Notes
- `cargo check -p op-chat` passed
- Static scan counted 56 test markers and 28 TODO/stub markers in this crate.
- Static scan also found 2 patch/backup artifacts in the crate tree.

