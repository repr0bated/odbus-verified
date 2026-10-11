# Schema-tagged routing — design (first pass)

Status: **draft / proposal**. Maps current implementation to proposed design.
Companion to `requirements.md` and `tasks.md`. Where a symbol is cited it was
verified in current source on branch `feat/schema-tagged-routing` (forked from
`feat/od-108-native-lookup`, HEAD `aaa5f32`, dirty tree preserved). Proposed
items are marked **(proposed)** and do not exist yet.

Terminology: the autonomous router binary is `threetched` (lowercase); the
`native`/`zeroclaw` names are retired branding. The OP-DBUS plugin that fronts
it is `tched_router`.

---

## 1. Current implementation map (verified)

Re-verification of the whitepaper's 2026-09-30 observations against current
source. "CONFIRMED" = still true; "CHANGED" = corrected below.

### 1.1 Schema and tags — CONFIRMED

- `PluginSchema` owns typed method argument/result/state, required capability,
  side effect and subid: `crates/op-state-store/src/plugin_schema.rs`
  (`subids` map ~`:271`, `tags: Vec<String>` `:301`).
- `tags` has exactly one runtime meaning today: an `"immutable"` check in
  `to_json_schema()` (~`:650`). It is distinct from `subids` (OSCAL) and from
  the Gemma keyword taxonomy.
- The five schemas the whitepaper cites (`gemma_brain`, `tched_router`,
  `openflow`, `ovsdb_bridge`, `xray`) all leave `tags` empty — they build via
  `schemars_adapter::plugin_schema_from_json(...)`, which defaults `tags` to an
  empty vec. **A declared-tag picker is therefore empty today.**
- Sealed blob is the plugin: `/dev/shm/opdbus/plugin-blobs/<id>.<hash16>.blob`;
  opblob (`crates/op-blob/`) is the only writer. gRPC/MCP/3tchedFS shapes derive
  from it (see `.agents/skills/grpc-expert/SKILL.md`).

### 1.2 Classification (gemma_brain) — CONFIRMED

- Types: `GemmaTaxonomy {revision, tags: BTreeMap<String,GemmaTag>}` and
  `GemmaTag {id,label,description,keywords,perspectives}` —
  `crates/op-plugins/src/state_plugins/gemma_brain_methods.rs:14-30`.
- `fn matches()` (`gemma_brain_methods.rs:197-237`) lowercases input and does
  `contains(keyword)`, scores by match count, sorts by score then tag id,
  truncates to `limit`. **Deterministic substring matching, not a model call.**
- Bounds: input `1..=65536` bytes; `MAX_TAGS = 256`; keywords `1..=64`; result
  `1..=64` (`:10-12`, `validate_*`).
- Capabilities (`gemma_brain.rs` schema): `classify` / `analyze_intent` /
  `register_tag` → `gemma.invoke`; `list_tags` / `list_perspectives` →
  `gemma.read`.
- `gemma_brain.route` is a **facade** over `tched_router.ResolveRoute`:
  `crates/op-grpc-bridge/src/gemma_brain_dispatch.rs` (`TARGET_PLUGIN =
  "tched_router"`, `TARGET_METHOD = "ResolveRoute"`, `:16-17`). It rechecks the
  outer `route` grant and the nested exact grant
  `cap.software.3tched-router.route.resolve@v1` (`:56-80`, test `:112-120`).
- Dead config: `GemmaRouting {subid_classification, tag_routing,
  xray_config_path}` (`gemma_brain.rs:35-50`) has no business-logic callers.
  **A tag→OpenFlow/Xray router does not exist.**

### 1.3 Routing (tched_router) — CONFIRMED

- `ResolveRouteInput {hint: String}` `#[serde(deny_unknown_fields)]`
  (`crates/op-plugins/src/state_plugins/tched_router.rs:439-445`);
  `ResolveRouteOutput {route: ModelRoute}` (`:778-784`). Handler
  `resolve_route` (`:2531-2547`) matches `route.hint == hint || route.model ==
  hint` exactly, else `RouteNotDeclared`. It does **not** accept a schema tag,
  provider/model candidate pair, or execution role.
- `SetOvsRoutingModel {model_id}` → `{ovs_routing}` (`:840-848`), decl `:1999`
  with capability `cap.software.3tched-router.model-assignments.ovs-routing.set@v1`
  and subid `mut.service.3tched-router.model-assignments.ovs-routing.set@v1`;
  handler `set_role_model_handler` (`:2642-2674`) records the assignment and
  emits `ModelRoleChanged` — **installs no flows.**
- `tched_router.Chat` is declared (`:1949-1956`) but executed by the bridge
  runtime, not the sync dispatcher (dispatch test skips it `:2918-2921`); the
  bridge forwards the prompt via `dispatch_chat_method`
  (`crates/op-grpc-bridge/src/chat_service.rs:125-145`); `ChatOutput`
  records the selected upstream `provider`/`model` (`:788-803`).

### 1.4 OSCAL subid registry — CONFIRMED

- `crates/op-plugins/src/state_plugins/oscal_subid_registry.rs` (schema) +
  `oscal_subid_registry_methods.rs` (types). `resolve` / `lookup` /
  `list_by_category` / `export` → `oscal.read`; `register` → `oscal.invoke`
  (`:412-457`; methods `READ_CAPABILITY`/`WRITE_CAPABILITY` `:21-22`).
- `SubidBinding {plugin_id, schema_hash, schema_version, kind: BindingKind,
  schema_key, transport_name: Option<String>}` (`methods.rs:208-219`);
  `BindingKind = Method | Signal | SchemaAnnotation`.
- Configured compiler input: `deploy/config/subid-registry.json` (distinct from
  the runtime annotation document; not auto-synchronized).

### 1.5 Network execution ownership — CONFIRMED (xray correction)

- OpenFlow: `add_flow`/`modify_flow`/`delete_flow` require `openflow.write`;
  the **sync** plugin dispatcher refuses them
  (`crates/op-plugins/src/state_plugins/openflow.rs:1671` — "must use the
  canonical compare/exchange bridge adapter"). They run in the bridge via
  `openflow_methods::dispatch_openflow_flow_method[_on]`
  (`crates/op-grpc-bridge/src/mutation_engine.rs:4762-4769`) using exact
  `FlowEntry` members and a native compare/exchange
  (`CasRequest {bridge, desired, expected, delete, operation}`) that requires a
  `verified == true` readback (`openflow_methods.rs:128-131`).
- OVSDB: `list_dbs`/`get_schema`/`monitor`/`monitor_cond` → `ovsdb.read`;
  writes → `ovsdb.write` (`ovsdb_bridge.rs:473-521`). Monitor is constrained to
  non-secret topology tables/columns.
- Xray: `get_stats` → `xray.read`; `add_user`/`remove_user`/`add_inbound` →
  `xray.write` (`xray.rs:307-348`). **Correction:** `restart` and the trace ops
  are **retired/not declared** (comment `xray.rs:316-318`; test asserts absence
  `:459-464`); the "unavailable" response is only a defensive arm in the bridge
  dispatcher (`mutation_engine.rs:6956-6957`).
- **No schema tag or model intent is accepted as a network argument on any of
  these surfaces.**

### 1.6 Receipts and MutationEngine — CONFIRMED

- `MethodExecutionReceipt {success, operation_id, outcome, event_id,
  event_hash, terminal_event_id, terminal_event_hash, plugin_id, method,
  result}` (`crates/op-state-store/src/method_receipt.rs:54-66`). Caller-private
  recovery via `GetMethodReceiptInput {operation_id}` + `MethodReceiptRecovery`
  (`:27-51`). The receipt is an audit locator carrying the plugin's own
  `result`; it is separate from state storage.
- Commit-before-notify: the OpenFlow mutation path
  (`mutation_engine.rs:4737-4797`) performs the native CAS, derives
  `openflow_methods::projection_change`, then `commit_state_update(...)` to SHM;
  only after a successful SHM commit does it return `receipt.output`
  (failure → "SHM publication failed; reconcile the exact member before retry").
  The privacy_routes arm (`:4807+`) documents the same ordering invariant.
- `OpenFlowMethodReceipt {output, before: Option<FlowEntry>, after:
  Option<FlowEntry>, bridge}` (`openflow_methods.rs:18-26`); CAS records
  `before`/`after`/`verified:true`/`observed_at_unix` (`:148-167`);
  `FlowObservation {flow, installed}` written under
  `.../observed_flows/{bridge}/{sha256 flow identity}` (`:34-57`).

### 1.7 Lungs / 3tchedFS — CHANGED (corrections)

- Transport socket lives in the bridge, **not** in `threetched-fs`:
  `crates/op-grpc-bridge/src/lungs_socket.rs`, zbus interface
  `org.opdbus.v1.LungsV1` on `LungsBootstrap` (`:436`), methods
  `bind(session_id)` (`:438`) and `bind_current_container()` (`:446`).
- **Socket path correction:** `DEFAULT_LUNGS_SOCKET =
  "/run/opdbus/lungs/lungs.sock"` (`:27`) — a nested `lungs/` dir, not
  `/run/opdbus/lungs.sock`. Object path `/org/opdbus/v1/lungs` (`:28`). One
  shared kernel-authenticated socket; host root uses `Bind(session_id)`, guests
  use `BindCurrentContainer()`.
- FUSE surface in `crates/threetched-fs/src/namespace.rs`: each method exposes a
  **single `call` file** (`NodeKind::CallFile {plugin_id, method}` `:1026-1037`),
  not separate `call`/`result`/`error` files. A method directory's leaves are
  `["call", "input.schema.json", "metadata.json"]`. Mount meta file
  `.3tchedfs.json`, format `"3tchedfs/1"`. `MountIdentity = HostSession{session}
  | GuestKernel | Unbound`. Default mount `/opt/run-mounts/3tched` (env
  `LUNGS_MOUNT`).
- `respond_to_user` is an op-chat forced-execution tool
  (`crates/op-chat/src/forced_execution.rs:200`), **not** a lungs subid.
- `ThreetchedSurface` (`crates/threetched-fs/src/threetched-surface.rs`)
  enumerates method ids discovered from the pinned binary — not a fixed list.
  This is the discovery contract R8 depends on.

### 1.8 Modes — CONFIRMED (location correction)

- `ChatMode` is defined in
  `crates/op-plugins/src/state_plugins/project.rs:17-29` (not op-chat /
  op-grpc-bridge): `Personal`(default), `Project`, `ControlPlane`,
  `Accountability`, `NotebookLm`, `UiRender`; `UiRender` has no memory
  (`has_memory()` `:31`). Mode→agent in
  `crates/op-grpc-bridge/src/chat_mode_routing.rs:31-33`.

### 1.9 Compliance material (maintained package + Qdrant)

- Maintained package `/srv/git/repos-bulk/compliance-index` (HEAD
  `c651f896`), read-only CLI `python -m compliance_index {mappings|route|scope}`
  under exclusive `.state/writer.lock`. **Not** a sealed bridge method yet.
- Captured contracts: `reports/compliance-scaffold/20260930-discovery/`
  (`contracts.md`, `registry.json`, `policy-route.json`, `ac2-route.json`,
  `ac2-scope.json`). `mappings`: 3,047 assertions / 557 historical / 1,738
  unresolved; generation `38cdcec0…`; kinds `covered_by`/`maps_to`/
  `merged_into`/`renumbered_to`/`split_into`.
- Vector scope: collection `compliance_context_v1`; empty `allowed_groups` →
  `qdrant_filter: null` → **zero** eligible results (never an unfiltered query).
  Source corpus references `qdrant://compliance_general/<uuid>`.
- Qdrant endpoint `10.200.0.2:6334` reached via UDS relay
  (`/run/ghostbridge/qdrant/grpc.sock`; `deploy/runit/uds-qdrant-grpc/run`).
  This is the "tons of compliance material in Qdrant" the owner referenced; the
  design **retrieves** from it through the maintained package's
  active-generation scope and never rebuilds or unfiltered-queries it.
- Captured sealed contracts for routing/UI scaffolding:
  `reports/btrfs-cache-memory/20260930-implementation/routing/` (`NOTES.md`
  plus `*-sealed-contract.json`).

### 1.10 What is missing today (gaps the feature must fill)

- No deterministic gate object correlating identity + method + classification +
  policy + grants.
- No decision-record store or receipt-correlation surface beyond per-method
  `MethodExecutionReceipt`.
- No declared schema tags; no tag semantics in the gate.
- No local redaction/transform method; no residual-disclosure validator.
- No sealed bridge method exposing the compliance mappings/route/scope
  contracts (live integration unavailable).
- No shadow-mode comparison surface.

---

## 2. Proposed plugin ownership (all **proposed**)

Each new surface is one plugin owning one `PluginSchema`, sealed by opblob,
registered via `inventory::submit!{PluginReg::new(...)}`, scaffolded with
`plugin_scaffold_helpers.rs`. No repo-level `schemas/` dir; no new proto service.

| Concern | Proposed plugin id | Owns | Reuses (existing) |
|---|---|---|---|
| Tag declaration + semantics | `schema_tag_registry` **(proposed)** | declared tags, tag→semantics descriptors, versioned vocabulary | `PluginSchema.tags`, `oscal_subid_registry` for subid linkage |
| Decision gate + records | `routing_decision` **(proposed)** | decision records, gate outcomes, shadow-mode comparisons, receipt correlation index | `MethodExecutionReceipt`, MutationEngine commit, `capability-grants` |
| Local transform (redaction) | `content_transform` **(proposed, later phase)** | bounded transform methods, findings, residual-validation results | lungs admission, bounded reservations |
| Compliance read bridge | `compliance_evidence` **(proposed, later phase)** | sealed read methods over the maintained package (mappings/route/scope) | maintained `compliance-index`, `compliance_context_v1` scope |

Explicitly **not** new plugins: classification stays in `gemma_brain`; route
resolution stays in `tched_router`; network stays in
`openflow`/`ovsdb_bridge`/`xray`; identity stays in the interceptor + lungs.

Open ownership question (needs owner input): whether `schema_tag_registry` and
`routing_decision` should be one plugin or two. First-pass recommendation: two,
so tag vocabulary (slow-changing, declaration-reviewed) is separable from
decision records (fast-changing, per-request).

---

## 3. Proposed typed contracts (illustrative, **proposed**)

Derived from Rust DTOs via schemars; shown as JSON shape for review. These are
not final method signatures.

### 3.1 Tag descriptor (`schema_tag_registry`)

```jsonc
// register_tag_semantics (oscal-style review mutation; its own capability)
{
  "tag": "sensitive.pii",                 // declared vocabulary member
  "expected_revision": 7,                 // optimistic concurrency
  "semantics": {
    "constraint": "requires_residual_validation_before_egress",
    "default_disposition": "local_only"
  },
  "linked_subids": ["mut.content.redact@v1"],   // optional subid linkage
  "source_refs": [{"source": "...", "sha256": "..."}]
}
```

Tag **semantics are data** until the gate implements and tests them (R1).

### 3.2 Decision record (`routing_decision`)

```jsonc
{
  "decision_id": "…",
  "verified_identity": {                   // from session/lungs, never model
    "principal_id": "…", "session_id": "…",
    "container": "…", "project_scope": "…"
  },
  "operation_ref": {                       // resolved from sealed catalog
    "plugin_id": "…", "method_key": "…", "transport_name": "…",
    "schema_hash": "…", "schema_version": "…", "catalog_hash": "…",
    "subid": "…"
  },
  "classification_proposal": {             // advisory; label origin retained
    "tag": "…", "origin": "keyword_match|model_proposal|user_selection|declaration",
    "taxonomy_revision": 12, "classifier_version": "…", "uncertainty": 0.0
  },
  "policy": {"rule_revision": "…", "source_refs": [{"source":"…","sha256":"…"}],
             "compliance_generation": "38cdcec0…"},
  "gate_outcome": "authorized_local|denied|unresolved|hold",
  "execution_ref": {                       // filled only after dispatch
    "operation_id": "…", "event_id": "…", "event_hash": "…",
    "terminal_event_id": "…", "observed_state_ref": "…", "outcome": "…"
  },
  "egress": {"decision": "n/a|permitted|denied|unresolved",
             "destination": "…", "provider_route": "…"},
  "shadow_mode": true
}
```

Correlation (R13): `execution_ref.operation_id` + event ids/hashes resolve to
the existing `MethodExecutionReceipt`; `observed_state_ref` points at the
committed SHM path the operation produced (e.g. OpenFlow
`observed_flows/{bridge}/{sha256}`), never into the receipt store.

---

## 4. Decision flow (proposed)

Mirrors the whitepaper pipeline; stages marked undeployed are gated behind
later phases. Model output is untrusted gate input throughout (R2/R3).

```
Verified identity + scoped request
  → resolve sealed schema + method (operation_ref)
  → bounded classification proposal (gemma_brain, advisory)   ─┐
  → explicit policy + source evidence (compliance_evidence)   ─┤
  → deterministic gate (identity + contract + policy + grants) ┘
       ├─ deny / unresolved → scoped hold + diagnostic (R10)
       └─ authorized local → canonical plugin dispatch (MutationEngine)
             → validate result + residual constraints (R5.4)
             → separate egress decision when needed (R5.5)
                   ├─ denied → scoped hold (stays local)
                   └─ permitted → declared execution/network surface (R8)
             → receipt + committed SHM observation (R4/R6)
             → scoped accountability evidence (R6)
```

### 4.1 State transitions (decision record)

`draft → classified → gated{authorized_local|denied|unresolved|hold}`
→ (if authorized) `dispatched → observed{succeeded|failed|uncertain}`
→ (if egress) `egress_pending → egress{permitted|denied}` → `recorded`.

Terminal `denied`/`unresolved`/`hold` never transition to a more permissive
state. `uncertain` requires reconciliation (readback) before `recorded` (R12).

---

## 5. Storage (proposed)

- Decision records and shadow-mode comparisons: owner-scoped private CozoDB
  (project convention — no SQLite; `op-cozo-store`), projected to private SHM
  for read via committed state (R4). Not a second state authority.
- Quarantine / scoped hold: owner-scoped hold with bounded retention; stores
  only diagnostic + minimal protected references, never raw content (R10).
- Receipts: unchanged — existing `MethodExecutionReceipt` store; the decision
  record carries only the locator.
- Tag vocabulary: sealed into `schema_tag_registry`'s `PluginSchema`/state;
  versioned by `expected_revision`.
- Compliance: no local copy; reads go to the maintained package + its
  active-generation Qdrant scope. The repo stores only review fixtures under
  `reports/compliance-scaffold/`.

All reads ride committed SHM through the single shared lungs socket /
3tchedFS mirror (NG6, consistent with OD-108's "no SSE/streaming" direction).

---

## 6. Integration points (existing surfaces to reuse)

- Identity: interceptor anchoring (`crates/op-grpc-bridge/src/interceptor.rs`),
  lungs `LungsV1.Bind` (`lungs_socket.rs`).
- Dispatch/commit: `MutationEngine` (`mutation_engine.rs`), commit-before-notify.
- Classification: `gemma_brain` `classify`/`analyze_intent` (advisory only).
- Route resolution: `tched_router.ResolveRoute` (unchanged signature).
- Subid linkage: `oscal_subid_registry` `resolve`/`lookup`.
- Compliance: maintained `compliance-index` package CLI contracts; captured
  shapes in `reports/compliance-scaffold/20260930-discovery/contracts.md`.
- UI: json-render typed surface (see `.agents/skills/json-render/SKILL.md`),
  rendering captures-vs-live and label origin; drafts invoke no mutations.
- threetched discovery: `ThreetchedSurface` for any threetched-backed command.

---

## 7. Defined behavior for edge cases (R10 detail)

| Condition | Behavior |
|---|---|
| Unknown / undeclared tag | Treated as data; gate applies most-restrictive applicable constraint; record marks tag `origin` and `provisional=true`. No capability/route/flow effect. |
| Stale contract (schema/catalog hash mismatch) | Association rendered `stale`; gate refuses to act on the stale reference; no silent re-pin. |
| Denied capability | Explicit deny recorded with principal, exact missing capability, error; no alternate identity/transport; no grant broadening. |
| Unavailable evidence (receipt/event/compliance/NotebookLM) | Reported as gap/unavailable; never rendered as confirmed negative or positive; empty compliance scope → zero results, no query. |
| Invalid model output | Rejected as gate input; retained as data; gate outcome unchanged; no raw-content logging. |
| Capacity exhaustion | Typed capacity/timeout status (backpressure visible); no cloud fallback from local saturation. |
| Cancellation | Queued reservation released; effects reconciled via readback; nothing disclosed. |
| Uncertain execution outcome | `uncertain` state; reconcile by readback before recording; **no blind retry** of unknown-effect operations. |

Three invariants hold across every row: no failure broadens identity/scope; no
failure discloses content via cloud fallback; no failure retries an
unknown-effect operation without reconciliation + idempotency.

---

## 8. Assumptions and unresolved decisions

- **A1 (assumption):** `routing_decision` can read committed SHM for evidence
  without a new streaming plane, consistent with OD-108's no-SSE direction.
- **A2 (assumption):** the maintained `compliance-index` package remains the
  compliance authority; a future sealed `compliance_evidence` read method wraps
  it rather than importing its graph.
- **A3 (assumption):** `gemma_brain` keyword classification is the only
  classifier for the first phases; any local model proposal is a later,
  separately measured addition and remains advisory.
- **U1 (unresolved, owner):** one plugin vs two for tag registry + decision
  gate (§2). Recommendation: two.
- **U2 (unresolved, owner):** where egress authorization for Workflow 1 is
  owned — a method on `routing_decision`, or a dedicated egress plugin — and
  what the authoritative destination/provider-route catalog is.
- **U3 (unresolved, owner):** whether tag semantics may ever reference OSCAL
  subids as *constraints* or must stay purely descriptive (affects R1 blast
  radius).
- **U4 (unresolved, owner):** the first concrete declared tag vocabulary and
  which workflow seeds it.
- **U5 (unresolved):** whether the first transform (redaction) runs as a
  threetched-discovered method or a new in-repo bounded method; depends on what
  the pinned binary's `ThreetchedSurface` actually exposes (must be discovered,
  not assumed — R8/NG5).
- **U6 (open from source):** the `compliance_general` corpus and
  `compliance_context_v1` scope were captured 2026-09-30; the live generation
  must be re-read at integration time (generations may have advanced).

No proposed method is presented as existing. Every "existing" reference above
carries a verified file/symbol citation.
