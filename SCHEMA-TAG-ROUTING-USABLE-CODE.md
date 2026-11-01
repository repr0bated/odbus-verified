# Schema/tag routing — usable code, strongest first → weakest last

Extracted 2026-11-01 from `SCHEMA-TAG-ROUTING-CODE.md` (Codex collection,
`schema-tag-routing-recovery-20261101`) and re-verified against the **live tree
at HEAD `b77882b915590cbddd68d983535a4ab136c900a8`** — after the bridge rewrite
(`server.rs` / `grpc_server.rs` / `lib.rs`) and after chat messaging moved to
EMQX (doorbell + ExHook only, see Tier 4).

**Ranking key:** Tier 1 = byte-identical to live, sealed, callable today.
Descending tiers = still-present but partial/proposed, then context-only, then
stale/archival — do not build on the bottom tiers without re-verification.

**Verification:** Jev `judge_batch jdgb-15bcd9276f1a435b` (jev-1.14.0).
Jev scores `< 0.6` are *inconclusive* per repo rule and are labelled as such;
those verdicts rest on the primary evidence cited (exact SHA-256 matches and
grep call-paths), with Jev only corroborative. No build/test/seal was run.

---

## Tier 1 — Strongest: live, sealed, byte-identical routing core

Evidence: live `sha256sum` matches the collection's `current-20261101` index
exactly, and both plugins are sealed in the live catalog
(72 blobs, incl. `routing_decision.e8c810cd666d3189.blob`,
`schema_tag_registry.efda4ac1ab2e6a22.blob`).

| File | SHA-256 (live = doc index) |
|---|---|
| `crates/op-grpc-bridge/src/routing_dispatch.rs` | `5b4c253b028a39d9aed6c93d9d342ea679dc7a6999a13ae75027f956d322cc79` |
| `crates/op-grpc-bridge/src/routing_catalog.rs` | `73e9ce7a4c4f6f063771af9038c0946e2ca19d1c1d58dc9cb61a454abf85bfe1` |
| `crates/op-grpc-bridge/src/routing_store.rs` | `5ae07a4baf3be2cf30c4b074d042aaac22ad848e5a9fa0faf0974a9454e6dc04` |
| `crates/op-grpc-bridge/src/routing_resources.rs` | `5f93931f781512e03116c10297a644708c68af0a9241fae51f7673ba390e38e6` |
| `crates/op-grpc-bridge/src/mutation_engine.rs` | `406704f8a90f832ce64cc6db85cd21eca3017b5410617666190ef4c43a536b39` |
| `crates/op-plugins/src/state_plugins/schema_tag_registry.rs` | `3405bdde390787bde5e396eb22dda95a85559a7946f933c537c3f6a17698d762` |
| `crates/op-plugins/src/state_plugins/routing_decision.rs` | `b789f779d7430322b47b7eb3f585c8e15f131742a9a6df9e6fa61c72f0a6b9cf` |

### 1.1 How you call it (the only usage path)

No public Rust API, no chat/EMQX entry point. A caller uses the sealed plugin
methods through `PluginService.CallMethod` (door `:8090`) or 3tchedFS
`operations/<plugin>/<method>/call`, with the exact capability held.
`MutationEngine` reserves routing capacity *before* parsing untrusted args,
then dispatches:

```rust
// crates/op-grpc-bridge/src/mutation_engine.rs:3592-3594
let early_routing = crate::routing_dispatch::reserve_before_parse(
    self, plugin_id, method, capability_id, actor_id, session_id, session_genesis,
).await?;

// crates/op-grpc-bridge/src/mutation_engine.rs:3800-3813
"routing_decision" | "schema_tag_registry" => {
    crate::routing_dispatch::dispatch(
        self, plugin_id, method, &serde_json::to_value(&parsed_value)?,
        actor_id, capability_id, session_id, session_genesis,
        &admitted_schema, &change.change_id,
        routing_admission.context("routing owner reservation missing")?,
    ).await?
}
```

```rust
// crates/op-grpc-bridge/src/routing_dispatch.rs:1-4,88,239
//! Admitted dispatch for private schema-tag and shadow-decision documents.
//! This first slice records proposals only. It never installs forwarding,
//! claims an execution receipt, or treats a recorded allow as a credential.
pub(crate) async fn dispatch(
    engine: &MutationEngine, plugin: &str, method: &str,
    args: &serde_json::Value, actor: &str, capability: Option<&str>,
    session: Option<&str>, genesis: Option<&str>,
    admitted: &PluginSchema, operation_id: &str,
    reservation: RoutingAdmission,
) -> Result<serde_json::Value>
pub(crate) async fn reserve_before_parse(/* … */)
    -> Result<Option<(PluginSchema, RoutingAdmission)>>
```

Final admission inside dispatch re-checks the caller and the catalog after
any queueing (`routing_dispatch.rs:37-81`): `chat_router_admission::readmit_call`
(an admission helper — *not* chat transport), then
`routing_catalog::recheck(operation)` and association re-resolution.

### 1.2 Methods you can call today

From `routing_decision_schema()` (`routing_decision.rs:99-119`) and
`schema_tag_registry_schema()` (`schema_tag_registry.rs:114-183`):

| Plugin.method | Effect | Required capability | Subid |
|---|---|---|---|
| `routing_decision.list_decisions` | Read | `cap.governance.routing-decision.read@v1` | `obs.software.routing-decision.list@v1` |
| `routing_decision.get_decision` | Read | `cap.governance.routing-decision.read@v1` | `obs.software.routing-decision.get@v1` |
| `routing_decision.record_shadow_decision` | Mutation | `cap.governance.routing-decision.record@v1` | `mut.software.routing-decision.record-shadow@v1` |
| `schema_tag_registry.list_tags` / `get_tag` / `get_protected_method_marker` / `list_policies` / `get_policy` | Read | `cap.governance.schema-tag-registry.read@v1` | `obs.software.schema-tag-registry.*@v1` |
| `schema_tag_registry.register_tag_semantics` | Mutation | `cap.governance.schema-tag-registry.tag.register@v1` | `mut.software.schema-tag-registry.register-tag-semantics@v1` |
| `schema_tag_registry.set_protected_method_marker` | Mutation | `cap.governance.schema-tag-registry.marker.set@v1` | `mut.software.schema-tag-registry.set-protected-marker@v1` |
| `schema_tag_registry.author_policy` / `update_policy` / `revoke_policy` | Mutation | `cap.governance.schema-tag-registry.policy.{author,update,revoke}@v1` | `mut.software.schema-tag-registry.*-policy@v1` |

Typed contracts (input/output structs, `deny_unknown_fields`):
`crates/op-plugins/src/state_plugins/routing_decision_methods.rs` and
`schema_tag_registry_methods.rs` — both byte-identical in the collection
(hashes in `SCHEMA-TAG-ROUTING-CODE.md` source index, `current-20261101` rows).

### 1.3 Catalog resolution (complete, self-contained)

```rust
// crates/op-grpc-bridge/src/routing_catalog.rs:14-48 (abridged; full file is 81 lines)
pub(crate) fn resolve_in(
    catalog: &ManifestPinnedCatalog, plugin_id: &str, method_key: &str,
) -> Result<ResolvedOperationRef> {
    let pinned = catalog.plugins.get(plugin_id)
        .context("routing target plugin is not sealed")?;
    let declaration = pinned.schema.methods.get(method_key)
        .context("routing target method key is not declared in its sealed plugin")?;
    crate::method_contract_admission::admit_compiled_state_contract(pinned, plugin_id)?;
    crate::method_contract_admission::admit_pinned_compiled_method(
        pinned, plugin_id, method_key, declaration.required_capability.as_deref())?;
    let operation = ResolvedOperationRef { /* plugin_id, method_key, transport_name,
        schema_hash, schema_version, catalog_hash, subid, required_capability */ };
    operation.validate()?;
    Ok(operation)
}
pub(crate) async fn resolve_candidate(candidate: CandidateOperation) -> Result<ResolvedOperationRef>
pub(crate) async fn resolve_associations(requests: Vec<SchemaTagAssociationRequest>, tag_id: u32)
    -> Result<Vec<SchemaTagAssociation>>
pub(crate) async fn recheck(operation: ResolvedOperationRef) -> Result<()>
```

Supporting, same tier: `routing_store.rs` (private Cozo-backed registry/decision
documents, owner-scoped), `routing_resources.rs` (bounded queue/byte/audit
reservations, `deploy/config/routing-limits.json`), and
`build_shadow_record` (`routing_decision.rs:126`) — the trusted envelope where
candidate strings, identity, policy and outcome are never taken from caller input.

**Limit that stays true:** this tier *records shadow proposals only*. It never
installs forwarding, and a recorded allow is not a credential
(`routing_dispatch.rs:3-4`). Sealed ≠ enforced.

---

## Tier 2 — Strong but partial: OSCAL field classification + OpenFlow permit

Byte-identical in the collection and present live; implemented in source but
with the open gaps the spec itself lists (design §1.10): reviewed rule
publication/bootstrap and live coverage remain open; controller attribution
is not operation-to-packet enforcement.

- `crates/op-plugins/src/state_plugins/oscal_field_classification.rs` (+ `_tests.rs`):
  bounded schema-field assessment, inherited reviewed rules, privacy floors.
  SHA-256 `e74d656fca835e54cd714f3d5a3510704477e78d615d51276844b7a51ba84dd2`.
- `crates/op-plugins/src/state_plugins/oscal_subid_registry.rs` /
  `oscal_subid_registry_methods.rs`: `resolve`/`lookup`/`list_by_category`/`export`
  (`oscal.read`), `register` (`oscal.invoke`); bridge side `oscal_dispatch.rs`.
- `crates/op-grpc-bridge/src/openflow_permit.rs` /
  `openflow_methods.rs` + `crates/op-network/src/controller.rs`:
  one-use attributed callbacks, last-moment caller/grant/schema re-admission
  for controller writes. Explicitly *not* complete tag-to-packet enforcement.

---

## Tier 3 — Reference only: the spec (proposed contracts, not code to call)

`.kiro/specs/schema-tagged-routing/{requirements,design,tasks}.md` and
`docs/schema-tagged-routing-whitepaper.md` — byte-identical in the collection
and live, but large parts are marked **proposed** (design §2–§5: `schema_tag_registry`
/ `routing_decision` ownership as originally proposed, decision flow, storage).
Use for intent and vocabulary (R1–R14); do not copy the illustrative contracts
in design §3 as if they were the sealed types — the sealed types are Tier 1.2.

Also in this tier: `gemma_brain.rs` / `gemma_brain_methods.rs` keyword taxonomy
(`matches()` = deterministic substring scoring, not a model call) — present and
sealed (`gemma_brain.107ffbac3b403dd1.blob`), but it is a classification helper,
not a tag→route executor. **A tag→OpenFlow/Xray router does not exist.**

---

## Tier 4 — Current architecture, but *not* routing code (context)

Chat messaging after the EMQX move — included so nobody wires routing into it:

```rust
// crates/op-grpc-bridge/src/chat_response_state.rs:54-65 — content stays in
// the private SHM document; EMQX carries only a content-free wake:
pub(crate) fn emqx_doorbell() -> Doorbell {
    Arc::new(|wake: DoorbellWake| {
        // … dispatch_emqx_method("publish_wake", &args) on topic
        // opdbus/wake/<owner-key>, best effort
    })
}
```

- `server.rs:840-847` installs this doorbell; `server.rs:1120+` serves the EMQX
  ExHook `HookProvider` on loopback mTLS `127.0.0.1:9000` (default disabled,
  `OP_EMQX_EXHOOK_ENABLE`); `mqtt_doorbell_relay.rs:1-5` is a browser byte relay
  on `:8090` that cannot select broker/topic/credential.
- `ChatService` gRPC still exists (`grpc_server.rs:1395`,
  `chat_service.rs:125 dispatch_chat_method` → `chat_router.Chat`).
- Routing has **zero** EMQX/MQTT/chat coupling: grep for
  `emqx|mqtt|ChatService` in `routing_dispatch.rs`, `routing_catalog.rs`,
  `routing_store.rs`, `routing_decision.rs`, `schema_tag_registry.rs` = no hits.
  Its only chat-named dependency is `chat_router_admission::readmit_call`.

Files for this tier are *not* embedded in the collection (except via
`mutation_engine.rs`/`server.rs`): `chat_router_admission.rs`,
`chat_service.rs`, `chat_response_state.rs`, `emqx_hook_provider.rs`,
`mqtt_doorbell_relay.rs`, `grpc_server.rs`.

---

## Tier 5 — Weak: stale prose inside an otherwise current collection

Jev `current` ≤ 0.19 on each (decisively stale); fix the prose, keep the code.

| Item | Location | What's wrong |
|---|---|---|
| MCP framing | requirements (“gRPC, MCP and 3tchedFS shapes derive…”), design §1.1; embedded `transport_scope="dbus,grpc,mcp,mqtt"`, “MCP gateway policy owner” | MCP retired 2026-10-28 (AGENTS.md): replaced by 3tchedFS. Do not document as current or re-enable `/mcp`. Live `mcp_frontend` modules existing ≠ current architecture. |
| `gemma_brain_dispatch.rs` facade | design §1.2 “CONFIRMED” | File is `absent-at-revision` at current (collection index) and absent live. The `gemma_brain.route → tched_router.ResolveRoute` facade description no longer holds. |
| “Untested, unsealed, undeployed” | intro + design §1.10 | “Unsealed” is now false (Tier 1 blobs exist). Shadow-only/no-enforcement part remains true. |
| Design §1 line numbers | e.g. `mutation_engine.rs:4762-4769` for OpenFlow CAS | Drifted within the same-hash file (that range is now `openflow_obfuscation` re-admission). Re-grep before citing; `chat_service.rs:125` still matches. |

---

## Tier 6 — Weakest: archival, do not reuse

- `foundation-ui-20260930` (3tched.com `999651e`): React workbench files
  (`Workbench*.tsx`, `CompliancePanel.tsx`, `AccountabilityEvidence.tsx`).
  Historical evidence only. Current UI is Svelte/SvelteKit + json-render
  Svelte from `/srv/git/3tched.com`; React/JSX is not authorized.
- `recovered-20261018` (`7766577e`) full blocks that differ from current
  (e.g. `routing_decision.rs`, `gemma_brain.rs`, `plugin_scaffold_helpers.rs`,
  `state_plugins/mod.rs`, `openflow.rs`): provenance/recovery value only —
  superseded by the Tier 1 current versions.
- `pre-checkpoint-to-recovered.patch` and `recovered-to-current.patch`:
  scoped historical patches (collection §“Changes and history”); they include
  other work in shared files and are not directly applicable to every checkout.
- September-2026 captured compliance/Qdrant values (design §1.9): dated capture
  (`2026-09-30`, generation `38cdcec0…`, Qdrant `10.200.0.2:6334` via
  `/run/ghostbridge/qdrant/grpc.sock`). Re-read at integration time; no sealed
  bridge method exposes those contracts today.

---

## What was *not* verified

No Cargo build/test, seal, or runtime call was run for this extraction.
“Callable today” means: present at HEAD, registered, and sealed in the live
catalog observed 2026-11-01 — not an exercised end-to-end invocation.
