# Exchange Point — Design (PROPOSED, awaiting owner approval)

## Shape
`GET /exchange` (door, same listener and TLS as `/mcp`):
```
{ "schema_hash": "<sha256>", "mode": {"selected": "...", "allowed": ["project", ...]},
  "model": {"provider": "...", "model": "...", "role_alias": null|"control_plane"|"accountability"},
  "schema": { "plugins": [...gated sealed schemas...], "render_catalog": [...json-render entries...] } }
```
`POST /exchange/mode` and `POST /exchange/model` forward unchanged to `project.select_context` and
the `chat_router` selection method through the MutationEngine with the caller's identity. The
Exchange_Point returns the method's own result or denial.

## Data flow
1. Door authenticates the caller (existing path) → principal, session.
2. Grants: one `load_capability_grants(principal)` read, shared by the `/exchange` and `/mcp` builders.
3. Mode: `project_dispatch::current_chat_context` (existing) gives the selected mode. The allowed
   modes are those whose admission in `resolve_selection` succeeds for these grants, evaluated
   read-only.
4. Model: `chat_mode_routing::resolve` (existing), read-only.
5. Schema: the sealed catalog filtered by `required_capability ∈ grants`; json-render entries by the
   same set. The hash is computed over the canonical JSON.

## Enforcement (backend, not the Exchange_Point)
- `memory_binding::bind_args` and the chat turn path read the persisted mode at execution time.
  Personal → Personal_Store only. Any embedding call (`embedding_model.embed`,
  `ctl_plane_chatbot.vectorize`, `rag_pipeline`, activity Qdrant upsert) refuses input whose source
  namespace is under `session/{sid}/personal`. A unit test per pipeline.
- Writes always go through the sealed methods, so admission and grants are re-checked there.

## Chatbot history (pending D3)
Option A: new sealed plugin `ctl_plane_chat_history` (read: list/get turns for its own principal;
write: append from the chat turn path only), catalog reseal, `$ROOT`-granted, FUSE-projected.
Option B: expose `runtime_chat_conversations` for the chatbot principal through its FUSE mount, with
no new plugin.

## Not in scope until approved
No code, catalog or grant change. The pending golden build does not include the Exchange_Point.
