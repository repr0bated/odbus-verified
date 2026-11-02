# Tag taxonomy v1 (draft)

Tags route requests: profile → controls → plugin operations. One tag per
control in `catalogs/etched-catalog/catalog.json`, stored as OSCAL props
(`ns: https://3tched.com/ns/oscal`, `name: tag`). Mirrors live in 3tched's
`schema_tag_registry` (tag semantics + owner-scoped policies); this file is
the draft, the registry is authoritative at runtime.

| Tag | Control | Sealed operations (candidate) | Evidence |
|---|---|---|---|
| `pii` | `etched-pt-1` | `schema_tag_registry/get_policy` | redaction receipt naming policy |
| `audit` | `etched-au-1` | `identity_sled/get_own_identity`, `routing_decision/get_decision`, `routing_decision/list_decisions` | operation receipt (`operation_id`, `event_hash`) |
| `access-control` | `etched-ac-1` | `elevation/authorize_task`, `schema_tag_registry/get_tag` | refusal receipt (missing capability + rule); approval record |
| `retention` | `etched-si-1` | `cron/create_job`, `workflows/start_workflow` | scheduled-operation receipt |
| `integrity` | `etched-in-1` | `identity_sled/get_identity`; anchor needs a sealed `web3` method (none yet — drafts only) | anchor record (index entry + tx) |

Qualifier tags (planned, for the router data model in `routing/DATA-MODEL.md`):
`data:private` / `data:public`, `action:read` / `action:write` /
`action:publish`, `approval:root` (two humans), `surface:team-chat` /
`surface:web`.

Rules:
- Every request carries ≥1 control tag; the router resolves tags → controls → operations.
- Tags never grant authority; grants come from the compiled profile (enforcement). A tag miss is a refusal, not a default-allow.
- New tags are added here first, then registered in `schema_tag_registry`.
