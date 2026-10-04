# OD-100 · Model selection — requirements

Related: OD-83 (dashboard chatbot ↔ mode selection repair). Supersedes the
ad-hoc model-selection notes scattered across HANDOFF.md (2026-10-03 rows) and
SIGNALS OD-83. This spec names the three load-bearing surfaces of model
selection — **native profiles**, **routes**, and **grants/permissions** — and
pins the invariants each must preserve.

## Vocabulary (one meaning per term)

- **Provider** — one `[[providers]]` table in `model-providers.toml`
  (`provider_id`, stable Cozo key). Declares how to reach an upstream model API.
- **Native profile** — a `family.alias` string the upstream `tched-router`
  (`zeroclaw`) binary resolves to a concrete `{model, uri}`. `family ∈
  ROUTER_FAMILIES` (anthropic, custom, deepseek, gemini, groq, ollama, openai,
  opencode, openrouter, xai). Two disjoint classes:
  - *ordinary / Personal* profile — the user-selectable inference route.
  - *control-plane* profile — the dedicated route a role mode uses; never
    user-selectable through an ordinary Apply.
- **Mode** — a `ChatMode` (`Personal`, `Project`, `ControlPlane`,
  `Accountability`, `NotebookLm`, `UiRender`). Role modes (`ControlPlane`,
  `Accountability`) have a **dedicated native agent alias**; the rest do not.
- **Route** — the resolved path `mode → dedicated alias → agent.model_provider
  reference → providers.models.<family>.<profile> → {model, uri}`.
- **Catalog** — the per-provider live model list fetched into CozoDB by
  `model_registry`. Carries a model's `recorded_profile` and
  `control_plane_profile`.
- **Grant** — a capability id held by a principal in the durable
  `capability-grants.json`, enforced per RPC.

## R1 · The plugin is the only schema for model selection

- WHEN any model-selection method, input, output, capability, or subid is
  defined THEN it is defined in a plugin schema under
  `crates/op-plugins/src/state_plugins/` (`model_registry.rs`, `tched_router.rs`,
  `project.rs`) — never a parallel proto package, never a repo-level `schemas/`
  directory.
- The `tched_router` on-disk `config.toml` is owned and written by the upstream
  binary. The bridge is the capability/audit boundary, not a second config
  writer. The repo MUST NOT introduce a second authority for native profiles.

## R2 · Native profiles are declared in root-owned TOML, typed by model_registry

- WHEN a provider is declared THEN it lives in `model-providers.toml`
  (shipped from `deploy/config/model-providers.toml`, installed root-owned at
  `/etc/op-dbus/model-providers.toml`, overridable only by
  `OP_MODEL_PROVIDERS_PATH`). The typed contract is `model_registry.rs`
  (`ProviderConfig`, `ControlPlaneRouting`).
- `parse_providers_toml()` MUST reject a file in which an ordinary profile and a
  control-plane profile collide or are owned by more than one provider. An
  ordinary Apply can never mutate a control-plane route (fail closed).
- `native_provider_id_for_model(recorded)` MUST bind the ordinary profile only
  when no control-plane alternative was recorded; an unmapped alternative MUST
  fail rather than silently reuse another API.
- Secrets (api keys) live only in the root-owned key file / `key_env` path.
  They MUST NOT appear in any catalog record, route projection, or UI payload.

## R3 · The catalog records a model's profile without changing any route

- WHEN `model_registry.list_models` returns a model THEN it carries the resolved
  `native_provider_id`, `inference_url_prefix`, `recorded_profile`, and
  `control_plane_profile`.
- WHEN `model_registry.set_model_profile` records (or clears) a model's chosen
  control-plane profile THEN it writes only catalog metadata (Cozo
  `set_model_registry_profile`) and advances a revision. It MUST NOT change a
  live route. Choosing the control-plane *model* is a protected `tched_router`
  task, not a catalog edit.
- A catalog refresh MUST NOT erase a previously recorded profile.

## R4 · A mode deterministically selects its route

- WHEN a caller is in a role mode (`ControlPlane` → `control_plane`,
  `Accountability` → `accountability`) THEN the route is the dedicated native
  agent alias; the caller's mutable dashboard provider **and** model selection
  MUST both be cleared so a role route cannot inherit either.
- WHEN a caller is in an ordinary mode THEN the caller's explicit
  provider/model selection is honored.
- `chat_mode_routing::resolve()` MUST re-read the caller's persisted mode via
  `project_dispatch::current_chat_context` at resolve time — the mode in effect
  is authoritative state, not a request argument.

## R5 · A principal can read its own route without reading anyone else's

- WHEN `tched_router.get_own_agent_route {}` is called THEN it returns exactly
  `{mode, reference, model, uri}` for the caller's current mode and nothing
  else.
- It MUST error when the caller's mode has no dedicated alias (ordinary modes
  have no dedicated route to disclose).
- The returned `uri` MUST pass `safe_uri()`: HTTPS only, no embedded
  credentials, no query, no fragment, no escaped path separators. `api_key`
  and unrelated provider fields MUST NEVER be projected.
- The observation MUST be taken under the caller's verified identity, committed
  to SHM, and read back from committed state; the caller's chat context MUST be
  unchanged across the call (`ensure_own_route_context_unchanged`).

## R6 · Catalog→route binding is unambiguous or it fails

- WHEN `apply_catalog_model_binding` runs THEN it selects a unique live ordinary
  profile whose endpoint origin **and** path match the catalog model's
  `inference_url_prefix` (this is what keeps `.../zen/go/v1` distinct from
  `.../zen/v1`), excludes control-plane references, and fails closed on
  ambiguity.
- Applying a binding MUST assert the caller's mode has NO dedicated alias.
  Changing a dedicated-agent model is a separate protected operation.

## R7 · Every model-selection operation is authorized per principal, per RPC

- WHEN any model-selection method is dispatched THEN the caller's grants are
  loaded from the tmpfs projection and intersected with the method's declared
  capability before and after any sensitive native observation.
- The required capabilities are exactly:
  | Operation | Capability |
  |---|---|
  | `get_own_agent_route` | `cap.software.tched-router.agent.route.self.read@v1` |
  | `chat` | `cap.software.3tched-router.chat@v1` |
  | `apply_catalog_model_binding` | `cap.software.tched-router.catalog.selection.set@v1` |
  | `resolve_catalog_model_binding` | `cap.software.tched-router.model.binding.read@v1` |
  | `config_list` / `config_get` | `cap.software.tched-router.router.read@v1` |
  | `config_set` / `config_patch` / `config_init` / `config_migrate` | `cap.software.tched-router.router.write@v1` |
  | `resolve_context` / `select_context` / `get_context` | `chat.context` (+ `chat.control_plane` for control-plane partition) |
  | `list_models` | `model_registry.read` |
  | refresh / maintain | `model_registry.refresh` / `model_registry.maintain` |
- The durable policy is `/etc/opdbus/capability-grants.json` (shipped from
  `deploy/security/capability-grants.json`), keyed by principal UUID.
- `op-grants-materializer` MUST validate the durable document (reject `*`
  wildcards and legacy 64-hex footprint keys, require a `capabilities` array per
  principal, reject duplicates) and atomically project a root-only `0o600`
  copy to `/dev/shm/opdbus/capability-grants.json`. On any invalid source it
  MUST fail closed: publish `{}` and remove the ready marker.
- The runtime loader MUST invalidate the **entire** document (return empty,
  fail closed) if any wildcard/legacy-footprint key is present.
- A new capability requires owner approval before it is added to any principal
  (AGENTS.md identity/permissions contract). The spec does not authorize
  granting; it records what each operation requires.

## R8 · Subids follow the OSCAL taxonomy; membership comes from the seal

- Every model-selection method and state field carries an `x-oscal-subid`
  (`verb.component_type.subject.facet@vN`) registered in
  `oscal_subid_registry.rs`. The registry annotates; it never mints runtime
  membership. A method is callable ⟺ its plugin is sealed in the blob catalog
  AND its `schema.methods` entry is present.

## Known drift this spec must close (evidence: HANDOFF.md)

- `get_own_agent_route` ships in `tched_router` source (4.2.0→4.3.0) and the cap
  is in the durable grants doc, but the running SHM projection / native router
  lagged — a caller saw "This identity does not hold the capability." The route
  read is correct in source; the gap is deployment of the grant + native
  role-alias/session-adapter.
- `opencode_go` model `glm-5.3-flash` is in the catalog at a revision whose
  recorded profile does not match the native router's configured profile /
  `inference_url_prefix` — an open catalog↔native binding mismatch for the
  control-plane model.

## Out of scope (v1)

- Writing `tched-router`'s on-disk `config.toml` from the bridge (upstream owns
  it).
- Mounting shared `HOME`/config or the bridge-private model-registry Cozo into
  any guest.
- UI for authoring providers (operator edits root-owned TOML).
- Paid upstream inference probes to resolve the Zen-vs-Go binding (verify
  through recorded metadata, not billed calls).
