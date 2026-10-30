# OD-100 · tasks

Status legend: `[x]` already in source (verified against the named file),
`[~]` partially present / deployment-lagged, `[ ]` not yet done. Cargo work is
delegated per `scripts/cargo-dispatch.md`.

## Native profiles

- [x] T1 `model_registry` typed contract for `model-providers.toml`:
  `ProviderConfig`, `ControlPlaneRouting`, `ROUTER_FAMILIES`,
  `parse_providers_toml` collision/fail-closed invariants
  (`op-plugins/src/state_plugins/model_registry.rs`).
- [x] T2 Personal-vs-ControlPlane alias derivation: `derived_native_provider_id`,
  `effective_native_provider_id`, `native_provider_id_for_model`,
  `control_plane_profile`, `valid_profile` (same file).
- [x] T3 Cozo catalog with profile preserved across refresh:
  `set_model_registry_profile`, revisioned relations
  (`op-cozo-store/src/model_registry.rs`).
- [x] T4 `list_models` / `set_model_profile` dispatch; HTTPS `/models` fetch;
  `set_model_profile` is metadata-only, never a route change
  (`op-grpc-bridge/src/model_registry_dispatch.rs`).
- [ ] T5 Reconcile the `opencode_go` / `glm-5.3-flash` recorded profile and
  `inference_url_prefix` against the native `agents.control_plane.model_provider`
  reference; prove via recorded metadata only (no billed probe). Closes the
  HANDOFF 06:07Z binding mismatch.

## Routes

- [x] T6 `ChatMode` + `chat.context` methods and memory namespaces
  (`op-plugins/src/state_plugins/project.rs`).
- [x] T7 `chat_mode_routing`: `dedicated_alias`, `apply_mode_route`
  (role modes clear dashboard selection), `resolve` re-reads persisted mode
  (`op-grpc-bridge/src/chat_mode_routing.rs`).
- [x] T8 `get_own_agent_route` schema (4.3.0) + pure whitelist projection with
  `safe_uri()` (`tched_router.rs`, `tched_own_agent_route.rs`).
- [x] T9 MutationEngine enforcement path for `get_own_agent_route`
  (grants→ctx→observe→commit→readback→project→re-admit→ctx-unchanged)
  (`op-grpc-bridge/src/mutation_engine.rs`).
- [x] T10 Catalog→route binding: unique origin+path match, CP excluded,
  fail-closed; ordinary bindings assert no dedicated alias
  (`op-grpc-bridge/src/tched_catalog_binding.rs`).
- [~] T11 Deploy the native router role-alias / session-adapter so live routing
  matches source `control_plane`/`accountability` aliases (SIGNALS OD-83).
  Source correct; deployed binary lagged.

## Grants & permissions

- [x] T12 Capability + subid declarations for every model-selection method
  (`tched_router.rs`, `project.rs`, `model_registry.rs`,
  `oscal_subid_registry.rs`).
- [x] T13 Durable per-principal grant doc holding the model-selection caps for
  the control-plane chatbot principal (`deploy/security/capability-grants.json`).
- [x] T14 `op-grants-materializer` validate + atomic `0o600` tmpfs projection +
  fail-closed on invalid source
  (`op-grpc-bridge/src/bin/op-grants-materializer.rs`).
- [x] T15 Interceptor runtime loader with wildcard/legacy fail-closed
  (`op-grpc-bridge/src/interceptor.rs`).
- [~] T16 Deploy `cap.software.tched-router.agent.route.self.read@v1` to the
  live principal so `get_own_agent_route` stops returning "identity does not
  hold the capability." Owner approval → atomic grant edit → materialize →
  durable/SHM equality. Owner-approved deltas were mid-rollout in HANDOFF.

## Verification

- [ ] V1 `cargo fmt --all -- --check` + `cargo clippy --workspace
  --all-targets --all-features -- -D warnings` + `cargo test --workspace
  --all-targets --all-features` (the `/verify` gate). Note: HANDOFF records a
  pre-existing ~99-file fmt drift blocking the full gate — do not count it as a
  pass and do not mass-reformat other agents' files to clear it.
- [ ] V2 Focused tests: `tched_own_agent_route` projection whitelist +
  `safe_uri` rejection; `chat_mode_routing` dedicated-alias/dashboard-clear;
  `tched_catalog_binding` ambiguity fail-closed; `model_registry`
  parse/collision; materializer `validate_document` wildcard/legacy rejection.
- [ ] V3 End-to-end on a received release: in `ControlPlane` mode,
  `get_own_agent_route` returns `{mode, reference, model, uri}` with `safe_uri`
  satisfied; a bounded Chat reports `routing.native_alias=control_plane`,
  `routing.source=native_agent` for the reconciled model (confirms T5/T11/T16
  together).

## Sequencing

T1–T4, T6–T10, T12–T15 are landed and only need verification (V1/V2). The open
work is the deployment/reconciliation triad **T5 ↔ T11 ↔ T16**, which must land
together: the native binding (T5), the live role alias (T11), and the live
grant (T16) are the three legs of the single HANDOFF-recorded "model selection
works end to end" outcome, proven by V3.

## Build-blocker fixes (landed this session)

The branch `feat/catalog-model-selection` validation commits (`be9d1b0`,
`0950df7`, `5536eda`) did not compile against the real `ProviderConfig`
(`enabled: bool`, `native_profile_overrides: BTreeMap`, required
`name`/`models_url`/`format`/`auth`, `#[serde(deny_unknown_fields)]`). Fixed so
`op-grpc-bridge` and the `model_registry` tests build:

- [x] F1 `model_registry_dispatch.rs`: `p.enabled.unwrap_or(true)` → `p.enabled`
  (bool, not Option); `RefreshCounts` → `RegistryRefreshCounts` (the real
  export); dropped redundant `.map(str::to_string)` on an `Option<String>`;
  `let failure` → `let mut failure` (needed by `get_or_insert_with`).
- [x] F2 `model_providers_validation.rs`: rewrote the module against the real
  `ProviderConfig`; preserved the routing-readiness gate (an enabled provider
  needs `inference_url_prefix`, and `key_file` unless `auth = "none"`); fixed
  all five tests.
- [x] F3 `tests/integration_model_routing.rs`: rewrote to construct a real
  `ProviderConfig`/`ControlPlaneRouting` and exercise
  `native_provider_id_for_model`/`control_plane_profile`.
- [x] F4 `model_registry.rs` test `native_binding_requires_explicit_ordinary_profiles`:
  compare `native_provider_id_for_model` (now `Option<String>`) against owned
  strings, not `Option<&str>`.
- [x] F5 `chat_response_state.rs`: the agent reply path is part of this
  workflow. Fixed the `owner_key` local shadowing the module function and the
  `tokio::sync::Mutex` misuse (`.lock().unwrap_or_else(..)` → `.lock().await`).
- [x] F6 `chat_response_dispatch.rs`: annotated a `spawn_blocking` closure
  return type (`anyhow::Result<usize>`) so `Into::into` resolves.

## Verification (this session)

- [x] V2 Focused tests pass (native `cargo`, `CXXFLAGS=-include cstdint`):
  `model_providers_validation` 5/5 · `chat_mode_routing` 4/4 ·
  `tched_own_agent_route` 4/4 · `tched_catalog_binding` 4/4 ·
  `integration_model_routing` 3/3 · `op-plugins model_registry` 12/12 ·
  `op-grants-materializer` 3/3. `cargo check -p op-grpc-bridge` is clean.
- [ ] V1 Full `/verify` gate not run: HANDOFF records a pre-existing ~99-file
  fmt drift that blocks it independently of this work; a remaining pre-existing
  `unused variable: snapshot` warning at `chat_response_dispatch.rs:476` is
  outside model-selection scope and was left untouched.

## The reply-to-user path (diagnosis, no code change)

Reported symptom: "the human id isn't getting to responses." Traced end to end.
The human message turn is written with the full caller identity, and the
`tched_router.Chat` arm **reserves** an answer turn designating the
control-plane chatbot as responder — but the reply is returned to the caller and
stored only as an episodic-memory episode; the reserved turn is never filled.

This is **by design**, not a bridge bug. Per the code comments and
`DELEGATION_CONTRACT.md`, the reply is the chatbot identity's own
`respond_to_user` write. Verified live: the chatbot principal's 3tchedFS **lungs**
mount (`/run/opdbus-lungs/<b0>/…`, bound into the guest as
`/opt/run-mounts/3tched` via the Incus `3tched` disk device) exposes a callable
`plugins/chat_response/operations/respond_to_user/{call,error,result,…}`. The
transport already exists and is callable through the filesystem. The gap is that
the control-plane chatbot is not performing that `respond_to_user` write after
producing a reply — a chatbot-orchestration gap, not a Rust change.
Auto-committing the reply inside the `Chat` arm was explicitly rejected: it would
bypass the identity/delegation boundary. No code changed for this item.
