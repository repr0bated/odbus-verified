# Exchange Point — Requirements (PROPOSED, awaiting owner approval)

Status: proposal only. Nothing here is implemented. Owner term (2026-11-22): **Exchange_Point**.

## What exists today (split across several places)

| Concern | Where it lives now |
|---|---|
| Chat mode the UI selects | `project.select_context` → Cozo `chat_context_selection` (`op-plugins/src/state_plugins/project.rs:497`, `op-cozo-store/src/lib.rs:325`) |
| Mode admission | per call in `project_dispatch.rs:490-560` (`chat.control_plane` grant gates ControlPlane; Personal is the default for every session) |
| Model / provider | `chat_router` selection plus dedicated role aliases (`chat_mode_routing.rs:29-46`) |
| Memory routing by mode | `memory_namespace_for` (`project.rs:52-77`) → namespaces `session/{sid}/personal[/name]`, `project/{id}`, `session/{sid}/{control_plane,accountability,notebooklm/…}`; all in the one cognitive CozoDB (RocksDB) with `soul_memories` |
| UI mode list | hard-coded `json_render_gallery::MODES` (`json_render_gallery.rs:32-39`) |
| UI-renderable schema | json-render catalog + sealed plugin schemas (FUSE / door) |
| MCP tools | `/mcp` on the door, built from caller grants (`mcp_frontend.rs:2551`) |
| Session_Mode (spec) | `.kiro/specs/identity-chat-interface` uses standard/control_plane/…, not `ChatMode` |

Gaps: no single endpoint returns mode + model + grant-gated schema together. The UI mode list is a
constant, not derived from grants. `Session_Mode` and `ChatMode` are two vocabularies. No
code separates personal-mode memory from vectorized storage. Personal and the other modes share one
CozoDB, and no documented "personal RocksDB" location or API exists. The chatbot has no dedicated
chat-history plugin. `runtime_chat_conversations` (principal-private conversations) and
`ctl_plane_chatbot` (reasoning episodes) are the closest.

## Glossary
- **Exchange_Point**: a read-mostly, versioned endpoint on the door, alongside `/mcp`, where the UI
  reads its current chat mode and model and the grant-gated schema it may render, and submits mode
  and model changes.
- **Gated_Schema**: the subset of sealed plugin schemas and json-render catalog entries the caller's
  grants allow, identified by a schema hash.
- **Personal_Store**: the storage personal-mode chat routes to: soul plus the personal namespace,
  never vectorized.

## Requirements

### R1 One docking point
1. THE Exchange_Point SHALL return, for the verified caller, in one response: the selected ChatMode,
   the modes the caller may enter, the selected provider/model, and the Gated_Schema with its hash.
2. THE Exchange_Point SHALL be served on the door next to `/mcp` and SHALL use the door's caller
   authentication. It SHALL NOT have its own authentication.
3. THE Exchange_Point SHALL be versioned by the Gated_Schema hash. A client presenting a stale hash
   SHALL get the current hash and schema.

### R2 No logic of its own
1. THE Exchange_Point SHALL only read from the identity owner (caller identity, grants) and the
   sealed plugin schemas/catalog. It SHALL contain no routing, grant or admission logic.
2. Mode or model changes submitted through the Exchange_Point SHALL be forwarded to the existing
   sealed methods (`project.select_context`, `chat_router` selection), which re-validate every write.

### R3 Same grants for UI and tools
1. THE Gated_Schema and the `/mcp` tool list SHALL derive from the same grant read for the same
   caller, so a capability is either rendered and offered as a tool, or neither.
2. THE UI SHALL render only from the Gated_Schema; hard-coded mode lists (`MODES`) SHALL be replaced
   by the Exchange_Point's allowed modes.

### R4 Personal routing enforced server-side
1. WHEN the persisted mode is Personal, THE backend SHALL route chat and memory to the Personal_Store
   and SHALL NOT write it to the sled or to any embedding/vector pipeline, regardless of UI flags.
2. WHEN the persisted mode is any other memory-bearing mode, THE backend SHALL write to the sled and
   the vector pipeline.
3. THE backend SHALL decide routing from the persisted mode read at execution time, never from a
   client-supplied field.

### R5 Chatbot chat history
1. THE control-plane chatbot SHALL have one dedicated plugin holding its own chat history, exposed
   in its Gated_Schema and projected through its FUSE mount, persisted durably.

## Open decisions for the owner
- D1 Personal_Store location and API. Options: (a) a separate RocksDB per principal (path?), or
  (b) the existing cognitive CozoDB, keeping `session/{sid}/personal` and `soul_memories` out of
  every vector pipeline.
- D2 What "the sled" means for vectorization: which store and which pipeline embeds non-personal
  modes. Today memory entries are not embedded automatically.
- D3 Chatbot history plugin: a new sealed plugin (catalog reseal) or reuse
  `runtime_chat_conversations` for the chatbot principal.
- D4 Converge `Session_Mode` (identity-chat-interface spec) onto `ChatMode`.
