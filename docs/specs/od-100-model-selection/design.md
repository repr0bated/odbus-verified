# OD-100 · design

Three cooperating surfaces, one authorization spine. Declaration
(`model_registry` + TOML) names what exists; routing (`tched_router` +
`chat_mode_routing`) decides which native profile a caller gets; grants
(`capability-grants.json` → materializer → tmpfs → interceptor/MutationEngine)
decide whether the caller may ask at all.

```
                        model-providers.toml  (root-owned, OP_MODEL_PROVIDERS_PATH)
                                 │  parse_providers_toml()  [model_registry.rs]
                                 ▼
   ┌───────────── DECLARATION ─────────────┐
   │ model_registry plugin (1.1.0)          │   list_models / set_model_profile
   │  ProviderConfig / ControlPlaneRouting  │   (model_registry_dispatch.rs,
   │  Cozo catalog (op-cozo-store/           │    HTTPS /models fetch → Cozo fold,
   │   model_registry.rs: *_models/*_providers)   recorded_profile preserved)
   └────────────────────────────────────────┘
                                 │  recorded_profile, control_plane_profile,
                                 │  native_provider_id, inference_url_prefix
                                 ▼
   ┌───────────────── ROUTING ──────────────────────────────────────────────┐
   │ project.rs   ChatMode + chat.context (resolve/select/get_context)        │
   │      │  persisted mode = authoritative state                            │
   │      ▼                                                                   │
   │ chat_mode_routing.rs  dedicated_alias(mode):                            │
   │      ControlPlane→control_plane · Accountability→accountability · else ∅ │
   │      apply_mode_route(): role modes clear BOTH input.provider and model  │
   │      resolve(): for_agent_alias(alias) | from_env(); attach sealed id    │
   │      ▼                                                                   │
   │ tched_router.rs (4.3.0)  config_* · get_own_agent_route ·                │
   │   resolve/apply_catalog_model_binding                                    │
   │      ▼ (loopback gateway)                                                │
   │ tched_router_runtime.rs → upstream zeroclaw binary owns config.toml      │
   │ tched_own_agent_route.rs::project(mode, config):                         │
   │   agents.<alias>.model_provider → family.profile →                       │
   │   providers.models.<family>.<profile> → {model, uri} ─ safe_uri()        │
   │ tched_catalog_binding.rs::select(): unique origin+path match, no CP ref  │
   └──────────────────────────────────────────────────────────────────────────┘
                                 │  {mode, reference, model, uri}  (whitelist)
                                 ▼
   ┌───────────────── AUTHORIZATION (per RPC, both ends) ─────────────────────┐
   │ deploy/security/capability-grants.json  (durable, per-principal)         │
   │      │  op-grants-materializer: validate → atomic 0o600 project          │
   │      ▼                                                                    │
   │ /dev/shm/opdbus/capability-grants.json  (OP_GRANTS_PATH, tmpfs)          │
   │      │  interceptor.rs load_capability_grants / capabilities_for_principal│
   │      ▼  (fail closed on wildcard/legacy key → whole doc empty)           │
   │ mutation_engine.rs: load grants → ctx → observe → SHM commit → read back │
   │      → project → re-admit → re-read ctx → assert unchanged → return      │
   └──────────────────────────────────────────────────────────────────────────┘
```

## Pieces

| Piece | Where | Role |
|---|---|---|
| `ProviderConfig`, `ControlPlaneRouting`, `ROUTER_FAMILIES`, `parse_providers_toml`, `derived_native_provider_id`, `effective_native_provider_id`, `native_provider_id_for_model`, `control_plane_profile`, `valid_profile` | `op-plugins/src/state_plugins/model_registry.rs` | Typed contract for `model-providers.toml`; Personal-vs-ControlPlane alias derivation; collision/fail-closed invariants (R2) |
| `model_registry_models` / `model_registry_providers` relations, `set_model_registry_profile`, revisioned refresh | `op-cozo-store/src/model_registry.rs` | Durable catalog; profile preserved across refresh (R3) |
| `list_models`, `set_model_profile`, `validate_set_model_profile`, `FETCH_TIMEOUT`, HTTPS `/models` fetch + fold | `op-grpc-bridge/src/model_registry_dispatch.rs` | Catalog read + profile record; no route change (R3) |
| `ChatMode` (+ `memory_namespace_for`), `resolve_context`/`select_context`/`get_context`, caps `chat.context`/`chat.control_plane` | `op-plugins/src/state_plugins/project.rs` | Persisted mode = authoritative state (R4) |
| `dedicated_alias`, `apply_mode_route`, `resolve`, `CONTROL_PLANE_AGENT`/`ACCOUNTABILITY_AGENT` | `op-grpc-bridge/src/chat_mode_routing.rs` | Mode → dedicated native agent alias; role modes drop dashboard selection (R4) |
| schema (4.3.0): `GetOwnAgentRouteInput{}`/`Output{mode,reference,model,uri}`, `config_*`, catalog-binding methods, all caps + subids | `op-plugins/src/state_plugins/tched_router.rs` | Routing contract + capability/subid declarations (R5, R6, R7, R8) |
| imported upstream config contract | `tched_router_contract.rs` + `tched_router_config_surface.rs` | Single schema authority for native config; re-import via `gen_tched_router_config_surface --import-json` only |
| `TchedRouterRuntimeClient` (`for_agent_alias`, `from_env`), loopback gateway | `op-grpc-bridge/src/tched_router_runtime.rs` | Forward `config_*`/chat to upstream; upstream owns `config.toml` |
| `project(mode, config)`, `safe_uri()` | `op-grpc-bridge/src/tched_own_agent_route.rs` | Pure whitelist projection for `get_own_agent_route` (R5) |
| `BindingTarget`, `ProfileCandidate`, `select()`, `resolve_catalog_model_binding`, `apply_catalog_model_binding` | `op-grpc-bridge/src/tched_catalog_binding.rs` | Unambiguous ordinary-profile binding; CP excluded; fail-closed (R6) |
| `get_own_agent_route` enforcement (~`mutation_engine.rs:4902`): grants→ctx→observe→commit→readback→project→re-admit→ctx-unchanged | `op-grpc-bridge/src/mutation_engine.rs` | Identity/mode/seal/grant boundary (R5, R7) |
| durable grant doc; chatbot principal `1262e982-14c1-44d4-0771-7ce74213211b` holds the model-selection caps | `deploy/security/capability-grants.json` | Authoritative policy (R7) |
| `validate_document`, atomic `0o600` tmpfs projection, ready marker, `run`/`once`/`check`/`validate` | `op-grpc-bridge/src/bin/op-grants-materializer.rs` | Fail-closed materialization (R7) |
| `load_capability_grants`, `capabilities_for_principal`, wildcard/legacy fail-closed | `op-grpc-bridge/src/interceptor.rs` | Runtime grant loading (R7) |
| subid grammar `verb.component_type.subject.facet@vN`, `x-oscal-subid` extensions | `op-plugins/src/state_plugins/oscal_subid_registry.rs` (+ `_methods.rs`) | Operational taxonomy; annotate-only (R8) |

## Trust boundaries

- **Native profile declaration** is root-owned TOML. The bridge reads it; it
  never writes it. Secrets stay in the root key file and never cross into a
  catalog record, route, or UI payload.
- **Route resolution** reads the caller's persisted mode (authoritative state),
  not a request field. Role modes get a fixed dedicated alias and have their
  dashboard selection cleared, so a privileged route cannot be steered by a
  mutable user preference.
- **`get_own_agent_route`** is a self-read: the caller sees only its own route,
  only the four safe fields, only via a committed-SHM read-back under its own
  identity, with the context asserted unchanged. No cross-principal disclosure.
- **Catalog metadata** (`set_model_profile`) is strictly non-routing. The only
  path that changes a live route is a protected `tched_router` config write or
  catalog binding, each with its own write capability and the no-dedicated-alias
  assertion for ordinary bindings.
- **Authorization** is enforced twice per sensitive native observation (before
  and after), from a tmpfs projection that fails closed, materialized from a
  durable per-principal document that fails closed on any invalid key. Model
  output and request arguments never widen the grant set.

## Deployment coherence (closes the recorded drift)

1. A changed `tched_router` schema version (e.g. 4.2.0→4.3.0) requires: build →
   explicit `opblob seal-shm` with the service's Qdrant-shaping env → golden
   validation → btrfs send/receive → activation. A method shipping in source is
   not callable until its plugin is sealed and the release is received.
2. A new capability (e.g. `cap.software.tched-router.agent.route.self.read@v1`)
   requires owner approval, then an atomic edit to the durable grants doc,
   materializer re-projection, and durable/SHM equality verification. Source
   holding the cap while the SHM projection lags is the exact failure the owner
   saw; the release/materialize step is what closes it.
3. The `glm-5.3-flash` catalog↔native mismatch is resolved by reconciling the
   recorded `control_plane_profile` / `inference_url_prefix` with the native
   `agents.control_plane.model_provider` reference — observed through recorded
   metadata and `get_own_agent_route`, never a billed probe.
