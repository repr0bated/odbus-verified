# 3tched resources available to the project

These already exist in 3tched (`/srv/git/odbus-verified`, live on the host)
and are used through their interfaces. Status as of 2026-10-28.

## The identity and its mount

- **Identity container** `3tchedCtlChatbot` — session
  `<subsidiary-session>`, Incus pool `3tched`, profile
  `3tched-lungs`, persist volume, kept up by runit `chatbot-identity`. **Proven.**
- **3tchedFS mount** inside it at `/run/mount/3tchedfs` (and the threetched
  view at `/run/mount/threetched`), served over the lungs Unix socket
  `/run/opdbus/lungs/lungs.sock` (in-guest symlink `/run/3tched-lungs.socket`).
- Layout: `plugins/<id>/` (schema, capabilities, object metadata),
  `operations/<plugin>/<method>/{call,metadata.json,input.schema.json,output.schema.json}`,
  `capabilities/`, `relationships/`, `view.json`.

### Calling an operation (proven)

Open the method's `call` file read-write, write the plain JSON arguments,
flush (fsync), then read the result from the same handle:

```python
import os
fd = os.open("/run/mount/3tchedfs/operations/identity_sled/get_own_identity/call", os.O_RDWR)
os.write(fd, b"{}")
try: os.fsync(fd)
except OSError: pass
os.lseek(fd, 0, 0)
print(os.read(fd, 200000).decode())
```

The result carries `required_capability`, `operation_id`, chain
`event_id`/`event_hash`, `outcome`. A missing grant is refused with the exact
capability name. Live state reads (`plugins/*/value.json`) currently return
EIO — use operations.

## Plugins (sealed catalog, 72 blobs)

Most relevant to this project:

| Area | Plugins |
|---|---|
| Identity | `identity_sled`, `human_principal`, `keypair`, `users` (create/modify/delete/list Linux users), `persona` |
| Compliance & tags | `oscal_subid_registry` (register, lookup, resolve, export, list_by_category, assess_fields), `schema_tag_registry` (tags, owner-scoped policies, protected-method markers), `routing_decision` |
| Network | `openflow` (add/modify/delete flows, controller, fail mode, datapath health), `ovsdb_bridge` (bridges, ports, transact), `rovs_commands`, `rtnetlink`, `privacy_routes`, `wireguard`, `wg_opdbus` |
| Compute & storage | `incus` (create/start/stop/snapshot instances, devices), `btrfs`, `service`, `packagekit` |
| Knowledge | `qdrant`, `cozo`, `cognitive`, `memory`, `embedding_model`, `snowball` |
| Chat & models | `chat_router` (`Chat`, catalog model binding, generation jobs, config), `chat_response`, `ctl_plane_chatbot`, `model_registry`, `large_language_model` |
| Messaging | `emqx` (topics: `opdbus/wake/<owner-key>`, `opdbus/responder-wake/<key>`, `opdbus/toolsets`, `opdbus/ack/<owner-key>`) |
| UI | `json_render`, `schema_renderer`, `web_ui` |
| Workflow | `workflows`, `cron`, `elevation`, `project` |

Full list: `ls /dev/shm/opdbus/plugin-blobs/` on the host, or `plugins/` in
the mount.

## Receipts and the chain

- Every admitted call gets an `operation_id` and chain events with hashes
  (MutationEngine). These are the evidence the Web3 index links to.
- Execution tracker → identity sled recording (every tool call validated and
  written to the sled for vectorization): recovered on branch
  `recover/execution-tracker`, compiles, **not deployed**.

## Knowledge (Qdrant, `127.0.0.1:6333`)

`repos_specs_docs_voyage_4_lite` (specs/docs), `repos_repomix_voyage_context_4`
(whole-repo context), `repos_lsp_*_voyage_4_*` (code), `snowball_footprints`
(receipt chain, ~10k), `blockchain_footprints`, `compliance_*` (compliance
corpora). Voyage-4 embeddings, 1024 dimensions, one fused space.

## Models and chat

- Router: zeroclaw on `127.0.0.1:8084` (`zeroclaw.v1`), agent `dashboard`
  used by chat. Currently Salad `qwen3.5-35b-a3b`. The product calls Meta's Model API directly, not through this router (see `docs/RUNTIME.md`).
- No MCP: it was removed and replaced by the 3tchedFS mount. Do not build on the bridge's `/mcp` endpoint or `3tched-fs mcp`.
- UI: `http://<door-ip>:8080` (door `<door-ip>:8090`).

## The 3tched.com UI repository

`/srv/git/3tched.com` — 3tched's existing web UI (SvelteKit + json-render,
served by op-web). Pre-existing platform, not hackathon code: see
`ui/README.md` for what was taken from it (references only).

## Tools on the host

`trestle` (Compliance Trestle, OSCAL authoring/validation), `bd`/`bv`
(beads task graph), `hf`/`datasets`/`sentence-transformers`, `omp`, `hermes`,
`junie`, `wscat`, `b3sum`, `wg`.
