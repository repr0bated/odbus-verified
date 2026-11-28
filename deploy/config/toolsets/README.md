# Toolset manifests (interim data, not installed)

`cognitive.toolsets.json` records the owner's 2026-11-19 decision for the
cognitive surface. It is data only: nothing reads it at runtime yet, and no
service installs it. `op-mcp-door` enforces the same layout in code
(`catalog::TIER_MEMORY`, `catalog::TIER_HOT`, `toolsets::COGNITIVE_SET`,
`readiness::default_gates`), and `crates/op-mcp-door/tests/manifest.rs` fails if
this file and the door drift apart.

## Layout

| Tier / set | Members | Loading |
|---|---|---|
| `memory` | the 7 `cognitive.memory_*` tools | always loaded, per identity, **never grouped** with anything else |
| `hot` | `code_search`, `code_context`, `code_index`, `search_own_events` (knowledge), `sequential_thinking`, `context_manager`, `specialist`, `schema_read`, `notebook_query`, `workflow_query` | always loaded |
| `cognitive` | every other `cognitive` operation (the 644 `agent_*` methods, `rust_pro`/`python_pro`/`typescript_pro`, `code_review_orchestrator`, `gemini_query`, `workflow_run`, `oscal_subids`, `get_health`, `list_tools`, `invoke_tool`) | **lazy**: projected as `toolsets/cognitive/` in the identity's mount only when requested, grant-gated |

Coding and knowledge are always loaded but answer
`{"status":"pending_revectorization", ...}` until every collection listed under
`readiness` exists in Qdrant with `points_count > 0`. The check only reads
(`GET /collections/<name>`); it never re-vectorizes. A Netcup snapshot restore
that brings the collections back flips them to ready with no code change.

## What Wonder's FUSE toolset projection must do

The shared FUSE contract (identity binding, capability checks, toolset
projection, receipts) belongs to Wonder (Muse) and is not on `origin/dev` yet.
When it lands, the projection must read this file (or an equivalent the
contract defines) and:

1. **Resolve members** from the identity's sealed catalog view:
   `tiers.*.tools` are `<plugin>.<method>` keys; a `members` rule means every
   operation of `plugin` minus the keys in `exclude_tiers`. Keys absent from
   the view are skipped, not errors.
2. **Memory**: always present for the bound identity, served from that
   identity's own store (per `.kiro/specs/cognitive-memory-rocksdb`); never
   appear inside `toolsets/<id>/` for any id.
3. **Hot**: always present in the identity's view (no request needed),
   still subject to the per-operation capability check.
4. **Lazy `cognitive`**: absent from the mount until the identity requests it;
   then appear as `toolsets/cognitive/` with one entry per member that
   resolves to the existing `plugins/<plugin>/operations/<method>/` call files
   (same `call` / receipt contract). Members the identity has no grant for are
   omitted (not listed, indistinguishable from unknown). Unloading removes the
   directory.
5. **Readiness**: for a tool listed under `readiness`, while the group is
   pending, answer the call (or expose a `status.json` next to it) with the
   `pending_status` body instead of dispatching. The door already does this
   for MCP callers.
6. **Receipts**: loading/unloading a lazy set and every call through
   `toolsets/<id>/` produce the contract's normal receipt.

Until then the door (`/mcp/full`, `/mcp/loader`) is the only consumer of this
layout. Loading all of `cognitive` on `/mcp/full` exceeds the 50-tool cap and is
refused with both counts; use `/mcp/loader` (`search_tools` / `call_tool`).
