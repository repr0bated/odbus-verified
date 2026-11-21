# Model selection: host receiving side and GUI contract

Recorded 2026-11-19 (America/New_York), odbus-verified origin/dev `0b43e43`,
live host view `/run/mounts/3tchedFS` observed read-only the same day (only
`ls` and `metadata.json` / `*.schema.json` reads; no call file was written).

Scope (owner, 2026-11-19): we own the **receiving side**: the host method that
takes the GUI's model choice and applies it system-wide. The GUI picker is not
ours; this file fixes the request/response it sends. The GUI gets valid
choices straight from threetched (its providers), so the receiver does **not**
validate the choice again. It applies it behind the grant gate and records a
ledger event.

## 1. The receiver already exists and is deployed

| Choice from GUI | Sealed method (host view `plugins/chat_router/operations/<m>/call`) | Required grant | Side effect | Applies via |
|---|---|---|---|---|
| default provider + model (system-wide) | `SetSelection` `{provider_id, model_id}` -> `{selected_provider, selected_model}` | `cap.software.threetched.selection.set@v1` | mutation, not idempotent | `mutation_engine.rs:5832-5905` -> `chat_router_runtime.rs:1788` `set_selection` -> threetched `PATCH /api/config` (agent provider path + provider model path), then authoritative readback and SHM publish |
| provider only | `SetProvider` `{provider_id}` | `cap.software.threetched.provider.set@v1` | mutation | same path, `set_provider` (`chat_router_runtime.rs:1712`) |
| model only | `SetModel` `{model_id}` | `cap.software.threetched.model.set@v1` | mutation | same path, `set_model` (`chat_router_runtime.rs:1743`) |
| role assignments | `SetOvsRoutingModel`, `SetObfuscationModel`, `SetVectorizationModel`, `SetQdrantRetrievalModel`, `SetCozoRetrievalModel` `{model_id}` | `cap.software.threetched.model-assignments.<role>.set@v1` | mutation | `mutation_engine.rs:5792-5830` -> `chat_router_legacy_dispatch` durable role CAS + readback |
| catalog row -> native profile (ordinary chat route) | `apply_catalog_model_binding` `{provider_id, model_id}` | `cap.software.threetched.catalog.selection.set@v1` | mutation | `mutation_engine.rs:5329-5420` (model_registry catalog target, `ensure_ordinary_profile`) |
| raw threetched config | `config_set` `{path, value}`, `config_patch` `{operations[]}` | `cap.software.threetched.config.write@v1` / router write | mutation | `chat_router_config_dispatch.rs:53-80` |

Declarations: `op-plugins/src/state_plugins/chat_router.rs:1951-2040` (method
table), `:810-819` (`SetSelectionInput`/`Output`). The live host view lists
all of the above under `plugins/chat_router/operations/`, and
`SetSelection/metadata.json` reads `side_effect: mutation`,
`required_capability: cap.software.threetched.selection.set@v1`,
`subid: mut.service.threetched.selection.set@v1`.

"System-wide": there is one threetched gateway per host. `SetSelection`
patches its global agent provider/model defaults, so every identity that
generates through threetched gets the new default. Per-agent dedicated routes
(control plane) use `config_patch` on `agents.<alias>.*` instead.

### Grant gate, ledger, execution tracker: already on this path

Every Operation_File write goes through `Runtime::call`
(`op-grpc-bridge/src/runtime.rs:707-790`):

- the sealed `required_capability` must be in the caller's grants;
- an execution-tracker record is reserved before admission, keyed by the
  verified principal and session, `tool_name = "chat_router.SetSelection"`;
- the ledger writes an attempt event and a terminal event, whose ids and
  hashes go back to the caller in the receipt and into the tracker's
  `ExecutionProof`;
- the bridge broadcasts a `SelectionChanged` / `ProviderChanged` /
  `ModelChanged` signal (`mutation_engine.rs:5893-5905`).

No new ledger event type is needed. The audit record for "who changed the
system model, when, from what to what" is the method receipt plus the
`chat_router` state change.

## 2. GUI -> host contract (what the GUI sends)

The GUI runs in the identity container. It never calls the host directly. Its
Chat_Route (or other in-container process) does one Operation_File call on the
identity's own mount:

```
path:   <mount>/plugins/chat_router/operations/SetSelection/call
open:   O_RDWR on that one handle
write:  {"provider_id":"<threetched provider id>","model_id":"<model id>"}
fsync:  executes the call (never re-write on the same or a new handle)
read:   receipt from offset 0 on the same handle
```

Request rules:
- `provider_id` and `model_id` are exactly the ids the GUI got from
  threetched (`chat_router/ListProviders`, `chat_router/ListModels`; grants
  `cap.software.threetched.providers.read@v1`, `...models.read@v1`). No extra
  fields: the input schema is closed.
- Role assignments use the role method with `{"model_id": ...}`.

Response (receipt on the same handle):
```json
{"success": true, "operation_id": "<uuid>", "outcome": "completed",
 "event_id": 0, "event_hash": "...", "terminal_event_id": 0, "terminal_event_hash": "...",
 "plugin_id": "chat_router", "method": "SetSelection",
 "result": {"selected_provider": "...", "selected_model": "..."}}
```
- Missing grant: denial (`PERMISSION_DENIED`). Show it and do not retry.
- Lost reply or unknown outcome: do **not** repeat the write. Recover with the
  sealed receipt-read method for the `operation_id` where the mount has one
  (design D-13; `chat_fs.get_receipt` is not in the host view today), or
  re-read `chat_router/GetState`.
- `result` is the authoritative readback from threetched after the patch, not
  an echo of the request.

## 3. Gaps (bridge code: request for Muse/Wonder)

These need changes in `crates/op-grpc-bridge` (not edited here):

- **R1. Drop the double validation on SetSelection/SetModel/SetProvider.**
  `chat_router_runtime.rs:1795-1802` (`set_selection`) calls
  `resolve_provider`, `ensure_router_default_model` and fetches
  `model_catalog(...)`, then refuses a model that isn't in it. The plugin-side
  handler also checks the catalog (`chat_router.rs:2558-2570`, used only when
  native dispatch is not admitted). The owner decision is that the GUI's
  choices already come from threetched, so the receiver must not re-check.
  Request: in `set_selection` (and the same check in `set_model`, if
  present), keep `resolve_provider` (needed to map the id to the native
  reference/path) but remove the `model_catalog(...)` fetch and its
  "not present in the observed catalog" refusal. That also removes one
  threetched round trip per change. `ensure_router_default_model`
  (`chat_router_runtime.rs:2514-2525`) is a router-default rule rather than a
  catalog check, so the owners decide whether it stays. Keep the readback
  comparison: it proves the write took effect and isn't choice validation.
  The input schema doesn't change, so this needs no reseal.
- **R2. Turn correlation for the execution tracker.** Let an Operation_File
  call carry an optional caller trace id (chat turn id) that becomes
  `ExecutionContext.trace_id`. Today `trace_id` comes only from
  `X-Ghostbridge-Trace-ID` (`tracing.rs:39-72`). Needs a 3tchedFS envelope
  field or side file plus bridge mapping. Untrusted, correlation only, never
  authority (matches `execution_context.rs:4-5`).
- **R3. Own Execution_Record read.** Expose
  `Runtime::execution_checkpoint_page` (`runtime_checkpoint.rs:235`, no
  callers today) as one sealed Read method, private scope only (caller's own
  principal/session). This closes D-12 and spec task 19.3. Adding a method
  changes a sealed schema, so it needs a reseal, which needs Jeremy.
- **R4. One Chat_Route.** Bridge ChatLink (`chat_link.rs:1-20`) spawns a Node
  worker per principal. The spec (`3tched.com` `op-chat-interface/src/lib.rs:3-5`)
  puts the server side in Rust. Decide whether `IDENTITY_CHAT_WORKERS_FILE`
  entries exec the Rust `chat-route` binary over the same NDJSON stdin/stdout
  protocol, or whether ChatLink is retired.

## 4. Gaps in 3tched.com (container repo; not touched here)

- The UI still names pre-rename capabilities: `cap.software.tched-router.*` /
  `cap.software.3tched-router.*` (`src/chat/door-chat-adapter.ts:65-71,363`,
  `native-routing.ts:3-4,98,150`, `control-plane-model.ts:8,63`,
  `router-committed-route.ts:12`). The sealed names are
  `cap.software.threetched.*`. Every call the UI makes with those names asks
  for a grant that no longer exists.
- `control-plane-model.ts:56-75` calls `apply_own_agent_route`, which its
  own comment (`:12-15`) says is no longer sealed. The live host view has no
  `apply_own_agent_route` (only `get_own_agent_route`).
- The UI pre-checks the selection against its catalog
  (`control-plane-model.ts:47-50`) and validates the applied result
  (`native-routing.ts:105-131`). That's fine on the GUI side. Once R1 lands,
  the receiver stops repeating the pre-check.
- Rust `chat-route` has no FUSE runner, so it cannot submit the selection
  itself yet (`chat-route.rs:547,636`). That is spec task 10.1.

## 5. Needs Jeremy

- Grants: the container identity that runs the picker needs
  `cap.software.threetched.selection.set@v1` (plus `providers.read`,
  `models.read`, and the role `*.set` grants it should be allowed). Not
  applied.
- Reseal for R3 (new sealed Read method). Not done.
- R4 decision (Rust worker vs Node worker).
