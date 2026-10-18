# Schema-tagged routing — design (first pass)

Status: **draft / proposal**. Maps current implementation to proposed design.
Companion to `requirements.md` and `tasks.md`. Where a symbol is cited it was
verified in current source on branch `feat/schema-tagged-routing` (forked from
`feat/od-108-native-lookup`, HEAD `aaa5f32`, dirty tree preserved). Proposed
items are marked **(proposed)** against that review baseline. Implementation
has since started; this inventory is not deployment evidence. See the shared
HANDOFF entry for the implemented, unbuilt first slice and remaining gaps.

Terminology: the autonomous router binary is `threetched` (lowercase); the
`native`/`zeroclaw` names are retired branding. The OP-DBUS plugin that fronts
it is `tched_router`.

Owner clarification, 2026-10-11: OVS forwarding under the existing OpenFlow
controller is required. Lungs supplies identity-bound admission, while the
controller requires authenticated operation attribution and a verified
identity-to-traffic binding. Xray is not the controller for this feature.
Canonical OSCAL mappings in Qdrant are required policy inputs; the new tag
registry references their identifiers, direction, generation and provenance
rather than defining a second mapping authority. Live canonical Qdrant mapping
discovery remains unverified. The earlier empty Incus inventory was superseded
by the authorized 2026-10-11 inspection: the container existed, but its daemon
was crash-looping on a restored WAL. That observation is also historical;
neither it nor the owner's recovery report proves today's collection contents.
See the dated OD-06 entries in `HANDOFF.md` before any further live discovery.

Implementation sequencing: the owner requested builds at the end unless needed
to continue development, and test execution after deployment. Author fixtures
during implementation and retain unrun checks explicitly; do not treat this
sequencing exception as passing validation or authorization to activate the
network.

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
  empty vec.
- **Correction (not in the whitepaper):** the newly registered `threetched`
  plugin (`crates/op-plugins/src/state_plugins/threetched.rs`, untracked on this
  branch) **does** declare tags: `schema.tags = vec!["threetched", "binary",
  "immutable"]` (`:111`). So "no declared schema tags" is too broad — at least
  one sealed plugin already sets tags, and these existing tags are discovery
  provenance, not policy. A *policy-bearing declared-tag picker* is empty today;
  the design must coexist with these existing provenance tags and not overload
  their meaning.
- Sealed blob is the plugin: `/dev/shm/opdbus/plugin-blobs/<id>.<hash16>.blob`;
  opblob (`crates/op-blob/`) is the only writer. gRPC/MCP/3tchedFS shapes derive
  from it (see `.agents/skills/grpc-expert/SKILL.md`).
- Owner clarification (2026-10-11): the shared OSCAL plugin owns the extensible
  metadata inherited by every plugin. The integration must consume that
  effective shared view instead of demanding repeated local annotations.
  Current source has `oscal_subid_registry` lookup/resolve/export/register,
  sealed-catalog binding discovery, and revisioned review annotations
  (`oscal_subid_registry_methods.rs`, `oscal_dispatch.rs`). The shared
  `derive_schema_subid` helper and schemars adapter derive schema identity.
  A local field-attribute census does not verify inheritance or effective
  coverage. Reviewed field export/projection paths still use explicit schema
  annotations; the registry's current annotations carry control references,
  not a direct PII flag. The OSCAL-to-privacy field-selection consumer has not
  been found in those paths. Verify that integration rather than inferring
  it from local attribute counts or from the registry's existence alone.

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
  `["call", "input.schema.json", "metadata.json"]` for a method with no return
  contract, and additionally `"output.schema.json"` when the method has an
  output schema (`namespace.rs:282-302`, `:817-833`; the fixed three-leaf list
  at `:1474` is the no-output case). Mount meta file `.3tchedfs.json`, format
  `"3tchedfs/1"`. `MountIdentity = HostSession{session} | GuestKernel | Unbound`.
  Default mount `/opt/run-mounts/3tched` (env `LUNGS_MOUNT`).
- These are **sealed public projections**: reading a plugin's files through the
  FUSE/3tchedFS surface exposes the sealed schema and public committed state. It
  is **not** a private read channel — private, caller-scoped decision records
  (§5) require their own admitted read methods, not a raw FUSE file read.
- `respond_to_user` is an op-chat forced-execution tool
  (`crates/op-chat/src/forced_execution.rs:200`), **not** a lungs subid.
- `ThreetchedSurface` (`crates/threetched-fs/src/threetched-surface.rs`)
  enumerates method ids discovered from the pinned binary — not a fixed list.
  This is the discovery contract R8 depends on. The registered `threetched`
  plugin (`crates/op-plugins/src/state_plugins/threetched.rs`) owns these
  binary-discovered CLI methods as one sealed contract, **alongside**
  `tched_router` (which owns `ResolveRoute`/`SetOvsRoutingModel`/`Chat`). Both
  front the same `threetched` binary; they are distinct typed plugins.

### 1.8 Modes — CONFIRMED (location correction)

- `ChatMode` is defined in
  `crates/op-plugins/src/state_plugins/project.rs:17-29` (not op-chat /
  op-grpc-bridge): `Personal`(default), `Project`, `ControlPlane`,
  `Accountability`, `NotebookLm`, `UiRender`; `UiRender` has no memory
  (`has_memory()` `:31`). Mode→agent in
  `crates/op-grpc-bridge/src/chat_mode_routing.rs:31-33`.

### 1.9 Compliance material (maintained package + Qdrant)

**Provenance caveat:** the package commit, path, generation and counts below are
**September 2026 capture evidence** (`reports/compliance-scaffold/20260930-discovery/`,
captured 2026-09-30), not freshly verified live state. Treat every value here as
a dated capture to be re-read at integration time (see U6); do not cite it as
today's live generation.

- Maintained package `/srv/git/repos-bulk/compliance-index` (HEAD
  `c651f896`, *as captured 2026-09-30*), read-only CLI
  `python -m compliance_index {mappings|route|scope}` under exclusive
  `.state/writer.lock`. **Not** a sealed bridge method yet.
- Captured contracts: `reports/compliance-scaffold/20260930-discovery/`
  (`contracts.md`, `registry.json`, `policy-route.json`, `ac2-route.json`,
  `ac2-scope.json`). `mappings`: 3,047 assertions / 557 historical / 1,738
  unresolved; generation `38cdcec0…`; kinds `covered_by`/`maps_to`/
  `merged_into`/`renumbered_to`/`split_into` (all *captured* values).
- **Contract shapes (none performs vector search):**
  - `mappings` → `{generation, changed, schema_version, retrieval_policy,
    assertions, historical_assertions, unresolved_targets, kinds, sources,
    contract}`.
  - `route TAG --depth D --limit L` → `{generation, tag, retrieval_policy?,
    nodes, paths, truncated}`. Depth bound `0..8`, limit `1..10_000`. An unknown
    tag returns empty nodes/paths with `truncated:false`. `paths` is the
    traversal's discovery tree, **not all graph edges** — even a `truncated:
    false` route is still **depth-bounded** and cannot claim completeness
    (`contracts.md:33`).
  - `scope --tag TAG` → `{generation, collection, allowed_groups,
    qdrant_filter}`. It returns **eligible groups + a filter, not search
    results**. Empty `allowed_groups` → `qdrant_filter: null` → zero eligible
    results (never an unfiltered query). The scope CLI internally routes at
    depth 4 / limit 10,000 and **exposes no truncation indicator**
    (`contracts.md:41`), so a consumer must carry an explicit completeness
    limitation rather than assume the scope is complete.
- The actual vector search is a **separate, not-yet-admitted operation** the
  consumer must perform against `collection` using exactly the returned
  `qdrant_filter`, then validate that each returned point's group/context
  membership matches the active generation (`embedding.py:244`, payload shape
  `embedding.py:192`). This search step does not exist as a sealed method today.
- Vector scope: collection `compliance_context_v1`; source corpus references
  `qdrant://compliance_general/<uuid>`.
- Qdrant endpoint `10.200.0.2:6334` reached via UDS relay
  (`/run/ghostbridge/qdrant/grpc.sock`; `deploy/runit/uds-qdrant-grpc/run`).
  This is the "tons of compliance material in Qdrant" the owner referenced; the
  design **retrieves** from it through the maintained package's
  active-generation scope + a bounded validated search, and never rebuilds or
  unfiltered-queries it.
- Captured sealed contracts for routing/UI scaffolding:
  `reports/btrfs-cache-memory/20260930-implementation/routing/` (`NOTES.md`
  plus `*-sealed-contract.json`).

### 1.10 Implementation status and remaining gaps (2026-10-12 source review)

The original baseline had no gate, decision store or shadow surface. Source
work now exists in `routing_decision`, `schema_tag_registry` and the bridge's
`routing_{catalog,dispatch,store,resources}` modules. It includes a shadow
evaluator, private durable records, catalog resolution and resource accounting.
The controller attribution work is also present in source. The shared OSCAL
assessment slice described below is now implemented in source; the GNU compile
check of `op-plugins` and `op-grpc-bridge` passed. This OD-06 work remains untested, unsealed and undeployed;
it is not evidence of enforcement.

- Canonical pre-dispatch tag-policy enforcement and final policy-revision
  rechecks remain to be completed and verified; shadow records authorize no
  effects. Local audit-byte accounting does not prove durable audit capacity.
- Shared OSCAL field/subtree assessment now has typed source contracts and a
  bounded resolver (§2c). Reviewed rule publication/bootstrap, raw producer
  preservation, creation/import/seal enforcement and live coverage remain open.
- No admitted local redaction/transform method; no residual-disclosure validator.
- No sealed redaction method was found. The upstream memory redactor is
  internal to the executable and is not an admitted plugin method; its
  availability, inputs, and guarantees are therefore not assumed here.
- No sealed bridge method exposing the compliance mappings/route/scope
  contracts (live integration unavailable).
- No completed redaction/accountability evidence UI for this feature or exact
  validated-content egress path.
- No verified operation-to-packet binding for shared provider connections;
  controller attribution alone cannot identify one request's traffic.

---

## 2. Proposed plugin ownership (all **proposed**)

Each new surface is one plugin owning one `PluginSchema`, sealed by opblob,
registered via `inventory::submit!{PluginReg::new(...)}`, scaffolded with
`plugin_scaffold_helpers.rs`. No repo-level `schemas/` dir; no new proto service.

| Concern | Proposed plugin id | Owns | Reuses (existing) |
|---|---|---|---|
| Tag declaration + semantics | `schema_tag_registry` **(proposed)** | declared tags, tag→semantics descriptors, versioned vocabulary | `PluginSchema.tags`, `oscal_subid_registry` for subid linkage |
| Field/subtree classification | `oscal_subid_registry` **(existing owner; extension proposed)** | reviewed classification references, scoped inheritance rules, coverage and provenance | existing subid bindings, exact plugin-owned schema roots |
| Decision gate + records | `routing_decision` **(proposed)** | decision records, gate outcomes, shadow-mode comparisons, receipt correlation index | `MethodExecutionReceipt`, MutationEngine commit, `capability-grants` |
| Local transform (redaction) | `content_transform` **(proposed, later phase)** | bounded transform methods, findings, residual-validation results | lungs admission, bounded reservations |
| Compliance read bridge | `compliance_evidence` **(proposed, later phase)** | sealed read methods over the maintained package (mappings/route/scope) | maintained `compliance-index`, `compliance_context_v1` scope |

Classification proposals stay in `gemma_brain`; authoritative field
classification belongs to the shared OSCAL owner. Route resolution stays in
`tched_router`; network stays in
`openflow`/`ovsdb_bridge`/`xray`; identity stays in the interceptor + lungs.

### 2b. Redaction-first structured visual demo (proposed)

The first visual demo uses the existing `/srv/git/3tched.com` SvelteKit,
Svelte 5, Vite, and `@json-render/svelte` application. Its existing
`ui_render` prompt mode and `compileChatCanvas` data-hydration flow remain the
rendering path; this spec adds neither a fixed page nor another schema
authority. The mode skips memory retrieval, but still uses the ordinarily
selected provider or fallback and therefore provides no privacy or locality
guarantee by itself. Safe demo choices are fictional sample content, or a
content-free model-authored template followed by local hydration from already
authorized data. Private originals must not enter model prompts, prompt
history, or gallery storage.

For structured values, a proposed overlay is computed from the exact sealed
source `PluginSchema` (pinned by hash/version), applicable OSCAL metadata
resolved through the shared plugin (catalog identity and annotation revision),
and an explicit machine-readable privacy policy pinned by revision. The runtime's current schema projection
recognizes `x-secret` and `x-opdbus-private`
(`crates/op-state-store/src/schema_projection.rs:36-47`); `[PII]` in a
description is not a runtime flag. Resolve OSCAL identities to their applicable
inherited metadata rather than inferring classification from the identifier
string. Historical local-annotation instructions and field counts in
`docs/schema-from-structs.md` and `docs/subid-projection-catalog.md` do not
establish effective inherited coverage. The owner's shared-plugin correction
supersedes using those counts as evidence of an unfinished migration. The legacy
`privacy_index` uses name heuristics and is not an authoritative masking
transform. Unknown, unclassified, or stale paths hold; no field is automatically
cleared for public use. A structured overlay does not validate free text, so
residual checks remain a distinct requirement.

Field selection is intended to be automatic: walk the payload under its exact
schema, resolve applicable inherited OSCAL metadata and canonical control
mappings, then apply the explicit privacy policy to select paths/actions.
Record the classification source and revisions with each selected field.
Registry schema pointers must be resolved into payload paths, including nested
objects and array items; they are not interchangeable. This consumer remains
to be implemented. Do not infer a privacy class from field names or an OSCAL
string, and hold unresolved bindings. No live PII mapping has been verified
while the owner is restoring Qdrant.

Owner requirement: classify every field at schema creation/import, through the
shared OSCAL authority. Explicit inherited rules may cover a field/subtree;
an identifier alone is not coverage. Schema validation and the sealing boundary
must reject unresolved classifications, including nested inputs/results/state
and array items, with source/revision evidence retained. This gate is proposed,
not currently enforced. Assess existing effective coverage before activation.
The implementation must support authoring new bindings before their first
seal; current registry register only annotates already sealed IDs, so calling
it for a not-yet-sealed field would create a lifecycle dependency error.
Preserve one shared authority when resolving this authoring/sealing lifecycle.

No sealed redaction method was discovered, and the upstream executable's
internal memory redactor is not exposed as an admitted method. Fictional
fixture rendering can develop the presentation, explicitly labelled as such.
Real transform results must come from an admitted backend method and retain
their receipt; the UI is not a second policy evaluator and cannot author
execution evidence. The owner's schema-overlay direction guides the structured
implementation; exposing an upstream binary method remains an alternative
only if required by the actual transform contract. Runtime redaction remains
unavailable until that implementation is sealed and admitted. Neither the UI
nor the transform alone authorizes content egress. This demo priority follows
identity, exact-schema, policy, and bounded
execution prerequisites; it does not defer or remove Workflow 2 (R6), Workflow
3 (R7), OpenFlow admission, residual validation, or exact-content egress gates.

The current source uses two plugins, `schema_tag_registry` and
`routing_decision`, separating reviewed routing policy from per-request
records. Keep that implementation choice; field classification extends the
shared OSCAL owner and does not create a third routing-policy authority.

### 2c. Shared field-classification contract (source slice; R1.1–R1.8)

This contract is informed by Luna's four five-plugin source audits on
2026-10-12, reviewed by Codex root. **Twenty plugins audited; zero fields migrated
in this pass.** The mounted OSCAL live-state read returned EIO, so these reports
do not establish effective deployed inheritance or classify every field.

Source update, 2026-10-12: `oscal_field_classification.rs` now defines candidate
and sealed pins, separate payload/metadata roots, typed selectors, reviewed
class/rule references and bounded assessment outcomes. The existing OSCAL
plugin's 2.1.0 source declares read-only `assess_fields(plugin_id)` with
`cap.standard.oscal.field-classification.read@v1`; no caller rules, classes,
schema bytes or purported success enter that operation. The bridge selects the
subject and admitted method from one pinned catalog and validates its input and
result. The service reads an optional field-rule section of OSCAL's existing
private annotation document and rechecks both document revision and catalog.
Absence reports unavailable; corruption fails. Neither path initializes rules.

This is a bounded assessment foundation, not completion of T0.2b–d. Candidate
evaluation is a pure producer-side API with no runtime membership or verified
producer receipt. The release seed, review/CAS publication, producer raw-schema
comparison, delegated target resolution, payload-time branch validation and
enforcement are not wired. Metadata is conservatively one opaque document
boundary; there is no finer metadata walker or catalog-disclosure filtering
yet. `source_preservation_verified` and `grants_disclosure` are always false.
The protection-loss helper compares two assessable PluginSchema representations
only; it cannot recover markers lost before either input. Unsupported constructs
(including conditional schemas and pattern properties) and exhausted limits
cannot report complete coverage. Public projection and existing admission stay
independent. The new capability is declared, not granted or deployed.

Luna reviewed the slice; root corrected exact map-key opt-in, incrementally
bounded report/evidence serialization, counted ancestry work and stale/unmatched
rule diagnostics. Scoped rustfmt and the GNU check of `op-plugins` and
`op-grpc-bridge` passed (receipt `/var/tmp/odbus-field-contract-final-check.4JNYXO/`); all
authored regression fixtures remain unrun under the owner's post-deployment
test sequencing. No migration, reseal or activation follows from this source.

`PublicComplianceAnnotation` currently contains revision and control/statement
references (`oscal_subid_registry_methods.rs:223`); it has no field-coverage or
privacy-class relation. `SubidBinding.schema_key` is a method map key or a
schema-document pointer (`:208`), not a payload selector. Extend that same
owner. Classification references must resolve to its reviewed vocabulary;
neither the routing registry nor a worker invents a parallel set of classes.
Canonical control mappings remain separately provenance-checked under R7.

#### 2c.1 Targets, rules and assessment results

Roles below describe the full intended contract. The source slice above exposes
only sealed assessment; the rest remain authoring/integration obligations.
All runtime inputs/results derive from the owning `PluginSchema`.

| Contract part | Required content and meaning |
|---|---|
| Target root | Plugin id plus `state`, `method_input`, `method_result`, or `signal_payload`; canonical method key or declared signal name where applicable. Bind the actual DTO/branch and result projection (full record versus summary). Resolve transport names separately. |
| Document-metadata target | A separate schema-document selector for examples/defaults or captured help/config evidence. It is not a payload root and has no implicit payload-path mapping. Pin it to the same source document; disclosure requires reviewed coverage or omission. |
| Source pin | Candidate producer identity/digest for authoring, or sealer-issued schema hash/version and manifest catalog identity at runtime. These are distinct variants; a candidate is never a runtime catalog entry. |
| Selector | Schema-root-relative JSON pointer for provenance plus typed payload steps: named property, each array item, each map key, each map value. Carry reference/union-branch provenance separately from payload steps. |
| Inheritance rule | Reviewed class reference, rule id/revision, target selector, explicit exact/subtree coverage and whether that scope includes dynamic keys. A map-value rule cannot cover keys implicitly. |
| Resolution | Effective class reference and contributing rule/protection sources, with outcome `resolved`, `restricted_opaque`, `uncovered`, `conflict`, `stale`, or `unsupported`. Missing authority is unavailable, not an empty successful rule set. |
| Assessment | Source and shared-rule pins, assessed roots, bounded findings, traversal completeness, opaque boundaries and limits hit. Separate declared-boundary coverage from inspection of interior values. |

The backend derives resolutions; caller-supplied classes or successful coverage
claims are advisory only. Preserve each root's context when a DTO is reused.
Do not flatten state, method results and signal payloads into a single exposure
decision. Effective disclosure is constrained by both the existing projection
and the reviewed privacy policy; field classification alone grants no read,
mutation, provider route or egress permission.

A lifecycle transition that copies data into another exposure scope is a new
evaluation point. For example, `json_render` gallery promotion must evaluate
the actual candidate spec and destination catalog entry; a validated Canvas
or disclosed summary cannot authorize copying the candidate's prompt or full
record. Free-form fields such as a catalog retirement reason still require
coverage. Retain source/destination roots and policy revisions for the copy.

`x-secret` and `x-opdbus-private` remain protective floors with their existing
projection semantics. Public-state path allowlists remain separate, explicit
limits. A field omitted from public state cannot become public because another
root exposes a similar DTO. `readOnly`, immutable state, exact-JSON opt-in,
subids, descriptions and hash-shaped values do not lower those protections.

#### 2c.2 Traversal, opaque data and imported contracts

- Resolve local `$ref`/`$defs` within the producer's exact schema, retaining
  reference-site protection. Do not fetch remote schemas. Bound cycles,
  expansions, branch counts, nodes and output entries; an unsupported or
  exhausted traversal cannot yield a complete assessment.
- Walk all declared alternatives for schema coverage, including nullable
  branches and additional properties. At payload time validate branch matches;
  combine applicable protections conservatively. A conflicting or ambiguous
  union never picks the first permissive branch. Empty arrays/maps and absent
  optional values do not excuse missing schema coverage.
- Treat dynamic keys as values requiring their own coverage. Reports use
  selector templates, not concrete keys, because identities or document names
  can be stored in map keys. Do not emit runtime data, defaults, examples or
  credential material while collecting schema evidence.
- Opaque JSON and free-form strings have no inferred interior schema. A
  reviewed rule protecting the whole boundary may satisfy boundary coverage
  as `restricted_opaque`, with only hold/removal permitted until a supported
  finer contract exists. The report must retain that interior classification
  is unknown. A Canvas validated for rendering is not cleared for disclosure.
- Delegated envelopes such as `elevation.authorize.args` require a declared
  relationship to the target plugin/method. The backend resolves and validates
  that target under the exact schema/catalog pin, combines wrapper and target
  protections, and rechecks it before use. A target mismatch or missing binding
  holds the envelope; no target invocation or capability grant is implied.
- Imported methods retain the binary digest, discovery producer/provenance,
  command identity and the actual schema source. For `threetched`, CLI-help
  option arity/choices and its `{stdout, stderr, exit}` return envelope are the
  current contract. A separately captured config schema does not define those
  method arguments. Missing privacy annotations remain unresolved, and generic
  output strings remain opaque. Do not invent flag types or interpret help
  text as execution or privacy policy.
- Schema metadata is also a disclosure surface. Review captured help/config
  evidence and examples as document-metadata targets, separately from state
  values. Exclude unassessed metadata from rendered/disclosed catalog outputs
  and model context; do not feed it into the payload overlay. In particular,
  `threetched` stores captured state including opaque `surface` in
  `PluginSchema.example`, while its public-state allowlist omits that field.
  That example must not provide an alternate disclosure path; immutable
  descriptors do not authorize exposure.
  Runtime-only credentials outside `PluginSchema` stay with their secret-store
  owner; do not fabricate schema pointers for nonexistent fields.

#### 2c.3 Candidate review, preservation and activation

The initial classification authority needs an explicit trust starting point.
Prepare one owner-reviewed bootstrap seed using the shared owner's typed
rules, bound to the exact initial OSCAL candidate, producer provenance and
rule/vocabulary revision. Its scope enumerates the owner and any required
bootstrap dependencies, with their reviewed coverage; it is not a blanket
public rule. Supply it through the existing release-preparation/sealer path,
not through a method that is still unsealed. Keep candidate digests outside
self-hashed schema content. Persist the accepted initialization provenance
with the same authority so later changes use its normal admission/revision
checks. A mismatched or replayed seed is rejected; missing durable authority
during recovery holds rather than re-enabling bootstrap. This creates neither
a second schema source nor a grant exception.

1. Obtain the candidate from the owning Rust schema producer or the verified
   binary importer. The shared OSCAL owner reviews bindings against that
   candidate's roots without requiring the subject subid to exist in a sealed
   catalog. Use the same authority and revision store; drafts are not a second
   schema source and confer no runtime membership.
2. Pin the producer's candidate identity and the reviewed shared-rule revision
   outside the candidate's self-hashed content. Validate shape, coverage and
   protective metadata before and after ordinary/exact schema adaptation.
   Source-to-adapter loss is a failure, even if a method using the same DTO
   happens to preserve its annotations. An absent local marker is different:
   resolve its effective inherited rules rather than declaring coverage
   missing or public merely from the absence.
3. At sealing, revalidate the actual producer output and reviewed revision;
   bind the accepted assessment to the sealer's final manifest. A changed
   candidate or rule requires revalidation. Consumers use manifest pins; they
   do not re-hash schema contents or consult drafts for plugin existence.
4. Runtime resolution uses the exact sealed roots and current authorized
   shared-rule revision. A stale cached resolution holds. Activation first
   requires assessment and reviewed migration of the affected existing
   schemas, including `oscal_subid_registry` itself. No universal public
   fallback or blanket exemption for the classification owner is allowed.

Existing runtime `register` requires a sealed subid and therefore cannot be
reused unchanged for step 1. Define candidate review in the shared owner's
typed contract before adding importer/sealer enforcement. Reports and review
drafts can proceed while live state is unavailable; publishing reviewed rules,
validating live effective coverage and activating enforcement require the
admitted authority. This spec revision performs none of those live actions.

#### 2c.4 Source evidence feeding the contract

Paths in this table are under `crates/op-plugins/src/state_plugins/` unless
specified. References identify audited source shapes, not successful runtime
coverage. Traverse reachable DTO modules when implementing the full census.

| Batch / plugin | Evidence and implication |
|---|---|
| 1 / `elevation` | `elevation.rs:160` wraps plugin/method plus opaque args; password has `x-secret`. Resolve the target contract without losing the wrapper's protection. |
| 1 / `ctl_plane_chatbot` | `ctl_plane_chatbot.rs:245` episodes and `:446` knowledge pushes contain identity fields, reused DTOs and opaque content; some text has explicit private flags while other descriptions only say `[PII]`. Exact state fields preserve nested schemas; neither names nor prose fill coverage gaps. |
| 1 / `chat_response` | `chat_response.rs:63` response union and `:227` content are private; Canvas is an opaque map. Preserve whole-content protection and distinguish method, state and signal roots. |
| 1 / `notebooklm` | `notebooklm.rs`, `notebooklm/outputs.rs`, `notebooklm/provider_outputs.rs` expose method DTOs with opaque recipients, citations, transcripts and references. Follow cross-module roots; absence of local privacy markers does not prove public or inherited classification. |
| 1 / `project` | `project.rs:202` explicitly limits public state to availability; method DTOs include member/invitation scope and document content. That state projection does not classify method results. |
| 2 / `openflow` | `openflow.rs:108` flow entries contain arbitrary match maps and action variants; exact-JSON opt-in preserves shape. Neither that marker nor reserved OSCAL tag comments classifies network strings or map keys. |
| 2 / `ovsdb_bridge` | `ovsdb_bridge.rs:22` transaction/monitor DTOs and `:215` state include opaque values and maps; `:662` explicitly allowlists public topology paths. Preserve the allowlist and caller-scoped monitor boundaries. |
| 2 / `qdrant` | `qdrant.rs:145` methods include raw point/result JSON and vectors; nested state also contains opaque config/telemetry. Point IDs/payloads need actual schema authority, not a guessed field layout or collection-name heuristic. |
| 2 / `keypair` | `keypair.rs:35`/`:104` state/result inventory has public keys, fingerprints and names; no private-key field is declared. Review exposure without inventing private-key or runtime socket fields. |
| 2 / `threetched` | `threetched.rs:104` seals discovered methods and restricts public state; `crates/threetched-fs/src/threetched-surface.rs:433` supplies CLI inputs and the generic output envelope. Retain binary provenance; help does not supply missing semantic classification. |
| 3 / `oscal_subid_registry` | `oscal_subid_registry_methods.rs:63` resolves pinned sealed documents; `:221` defines compliance annotations and `:445` requires existing sealed membership for registration. Cover the authority's own schema and initial trust seed without a self-dependent first-seal method. |
| 3 / `schema_tag_registry` | `schema_tag_registry_methods.rs:352` marks registry state private; tag/policy method DTOs separately return records (`:97`, `:312`, `:419`). State protection and canonical mapping references do not supply those results' field classification. |
| 3 / `routing_decision` | `routing_decision_methods.rs:177` marks state private; `:237`/`:282` expose decision records through admitted method results. Preserve their independent identity, policy and execution-evidence coverage. |
| 3 / `model_registry` | `model_registry.rs:45` places the model-visible guide in schema description; `:703`/`:785` define selected provider/model output projections. Root-owned provider configuration is a separate contract (`:151`), not the full method output. Credential references are not credential values; bounded provider error text is not automatically safe. |
| 3 / `json_render` | `json_render_gallery.rs:124`/`:144` distinguish full private candidates and summaries; `:191`/`:215` distinguish full catalog entries and summaries. Promotion crosses exposure scopes. Authoring docs, returned specs/schema strings and prompt text also require the correct payload or metadata scope (`json_render.rs:467`/`:562`). |
| 4 / `btrfs` | `btrfs_plugin.rs:207` state and typed methods at `:896` expose filesystem/device paths and structured subvolume, scrub, balance and usage results. Ordinary string/number types do not establish publicness; paths need reviewed classification. The unrelated withdrawn five-subvolume cap is owned by OD-119. |
| 4 / `cozo` | `cozo.rs:172`, input DTOs at `:32`, and `cozo_methods.rs:37` include query/trigger text, arbitrary parameter maps, relation-keyed import/export and rows with arbitrary JSON cells. Classify dynamic keys separately from values and full records separately from summaries; query text is an opaque boundary. |
| 4 / `memory` | `memory_plugin.rs:11` and DTOs at `:104` include arbitrary config/value data; update results can return `previous_value` (`:193`). Principal/session scope and the credential guard (`memory_methods.rs:504`) do not classify all stored content or full/previous-value results. |
| 4 / `mcp` | `mcp.rs:195` state uses imported exact config schema (`:225`) with dynamic server/environment maps and optional tool groups. `get_config` and `configure_server` reuse the `tched_router` declarations; `configure_tool_groups` is a local DTO. Preserve the imported state provenance and delegated method roots rather than treating full configuration as a summary. |
| 4 / `cognitive_mcp` | `cognitive_mcp.rs:1099` state and `CodeIndexInput` at `:772` expose code content/collection selection; `code_index` (`:1238`) is a method, not a separate plugin. Generic tool JSON, schema documents, retrieved memory and search/context content require their own roots; result counters and existing subids do not cover those payloads. |

No local signal declaration was found in the fourth batch. That source observation
does not prove absence of inherited protection or complete runtime coverage.

The conversion boundary is concrete: `schemars_adapter.rs:258` constructs a
finite `FieldSchema`, while `:156` has an explicit exact-schema path.
`plugin_scaffold_helpers.rs:335` serializes method inputs/results directly.
Tests must compare effective protection across these paths, rather than count
attributes in Rust and assume the sealed result retained them.

---

## 2a. Policy contract and enforcement point (proposed)

The gate is a *planning/record* layer; it is **not** a new authorization
boundary. Authorization stays where it already is: the canonical admission path
in MutationEngine. This section pins how the two relate.

### 2a.1 Where enforcement actually happens (existing)

The real pre-dispatch checks, in order, inside the canonical path
(`crates/op-grpc-bridge/src/mutation_engine.rs`):
1. method-contract admission + argument validation
   (`validate_admitted_arguments`, ~`:3548`);
2. integer normalization and args-schema build;
3. **compliance deny check** — `cognitive_mcp().cozo_shuttle()
   .evaluate_mutation(plugin_id, method)` (~`:3568`): fail-closed only when the
   cognitive Cozo store has `Deny` rows; a no-op otherwise;
4. per-RPC capability-grant enforcement (interceptor / grants).

**The gate joins at step 3's layer, not around it.** Any schema-tag policy the
gate decides is expressed as, or evaluated alongside, `evaluate_mutation` so a
**direct** call to a protected method hits the identical enforcement. A
`routing_decision` record of "authorized_local" is a *log of a decision*, never
a token a later call can present to skip steps 1–4. There is no "allow cache."

**Correction — the existing evaluator is permissive on failure.** Two gaps mean
step 3 cannot be relied on as fail-closed as written:
- `evaluate_mutation` returns `allow: true` when the Cozo query itself fails
  (`crates/op-cozo-store/src/lib.rs:767`, `Err(error) => PolicyVerdict { allow:
  true, reason: "compliance query failed: …" }`);
- the check is skipped entirely when the cognitive store is uninitialized
  (`if let Ok(mcp) = self.cognitive_mcp().await { … }`).
Therefore, for a method the gate marks **policy-required**, the gate MUST NOT
inherit this allow-on-error behavior: a policy-store query error, an
uninitialized store, or a missing applicable rule yields a typed **hold/deny**
(`unresolved`), not an allow. Allow-on-error remains acceptable only for methods
that are *not* policy-required (the gate adds nothing there).

### 2a.1a Final recheck for all protected operations (not only egress)

The immediate-before-dispatch recheck (identity, grants, contract hash,
policy/`rule_revision`, and — for egress — the exact validated bytes) applies to
**every** protected operation, including purely local effects, not just egress.
Approval obtained before a queue wait is re-verified immediately before the
admitted dispatch; any revision bump, grant revocation, stale schema/catalog
hash, or policy-store unavailability at that moment aborts to
`unresolved`/`hold`. Egress additionally rechecks the immutable
`validated_content_ref` (§3.2).

### 2a.2 Supported rule types (proposed, minimal first)

| Rule type | Shape | Decision |
|---|---|---|
| `deny_method` | `{plugin_id, method_key, reason}` | hard deny at admission (mirrors existing `evaluate_mutation` Deny rows) |
| `tag_constraint` | `{tag, constraint, applies_to: [subid|method_key]}` | gate maps tag→constraint (e.g. `requires_residual_validation_before_egress`); evaluated deterministically |
| `egress_rule` | `{destination_class, allowed_provider_routes, required_policy_revision}` | egress authorization input (Workflow 1) |

Each rule is **data** until the deterministic gate implements and tests its
meaning (R1). No rule type infers a network effect (R8).

### 2a.3 Ownership, revisions, conflict, missing-policy

- **Ownership:** `deny_method`/`tag_constraint` rules are owner-authored policy,
  revisioned by `rule_revision`; tag→method associations live in
  `schema_tag_registry` with `expected_revision`; the compliance Deny rows
  remain owned by the cognitive Cozo store (unchanged). The gate **reads** all
  three; it authors none of them implicitly.
- **Revisions:** every decision record pins the `rule_revision`,
  `taxonomy_revision`, schema/catalog hashes and `compliance_generation` it
  evaluated against. A revision change invalidates reuse (there is no reuse).
- **Conflict resolution:** deny wins. Order = explicit `deny_method` /
  compliance Deny → `tag_constraint` unsatisfied → missing required grant →
  otherwise allow. Any `unresolved` input (unknown tag, stale hash, missing
  policy where one is required) yields `unresolved`/`hold`, never allow.
- **Missing-policy behavior:** for a method flagged as requiring policy,
  absence of an applicable rule is `unresolved` (fail closed), not implicit
  allow. For a method not flagged, absence of a `tag_constraint` leaves the
  existing admission checks (steps 1–4) as the sole authority — the gate adds
  nothing and removes nothing.

### 2a.4 Non-bypassability (test obligations)

- A direct `PluginService/CallMethod` to a protected method encounters steps
  1–4 regardless of whether a `routing_decision` record exists.
- A recorded "authorized_local" cannot be replayed to authorize a later call;
  the record carries no credential and no capability.
- Fixture: issue the operation twice — once via the gate path, once directly —
  and assert identical admission outcomes.

### 2a.5 Policy administration (ownership, CRUD, marker source)

Policy must be administrable, or "missing policy" is indistinguishable from "not
a protected method." **(proposed)**

- **Owning plugin:** `schema_tag_registry` owns tag→method associations,
  `tag_constraint` rules and the proposed `deny_method` rules. Its admitted,
  revision-checked methods administer these rules (T1.1b). Existing compliance
  `Deny` rows remain a separate veto owned by the cognitive Cozo store;
  `store_compliance_rule` is its internal unversioned DB helper, not the
  administration surface for the proposed rules. The gate evaluates both at
  the canonical pre-dispatch layer. `egress_rule` ownership is U2 (owner decision).
- **Methods (proposed):** `author_policy` / `get_policy` / `update_policy` /
  `revoke_policy` on the owning plugin, each a typed method with its own
  capability (author/update/revoke are `invoke`/mutation; get is `read`), under
  the private durable publisher. Each mutation takes `expected_revision`
  (optimistic concurrency) and persists the new `rule_revision`.
- **Persistence:** durable in the owning plugin's private CozoDB; revisioned;
  no second authority.
- **"Method requires policy" marker:** comes from an explicit declared marker
  (a declared schema tag such as `policy.required`, or an entry in the owning
  plugin's policy-required set), **never** inferred from the presence/absence of
  a rule. Absence of a rule for a marked method = `unresolved` (fail closed).
  Only a successful lookup confirming marker absence leaves the method
  unprotected by this layer; a failed or unavailable marker lookup produces
  `hold`. The existing admission checks still apply in every case.
- **Phasing:** Phase 0 uses **fixed fixtures** for the policy-required set and
  rules (no live administration); live administration is a later task
  (`T1.1b`) depending on the registry plugin.

### 2a.6 Which decision fields the backend authors (anti-forgery)

Callers submit **proposals only**; the backend authors the trusted fields. This
extends the identity protection (R2) to outcomes and evidence.

| Field | Author | Rule |
|---|---|---|
| `verified_identity` | backend | from session/lungs; a caller-supplied value is ignored |
| `classification_proposal` | caller (advisory) | retained as data; never a decision |
| `operation_ref` | backend | resolved from the sealed catalog (T0.2); a caller-supplied ref is re-resolved, not trusted |
| `policy` / revisions | backend | the revisions the gate actually evaluated |
| `gate_outcome` | backend | computed by the deterministic evaluator; a caller-supplied outcome is rejected |
| `execution_ref` (receipt/event/observed_state) | backend | validated for **ownership** — the receipt's `operation_id`/event ids must belong to this caller's operation; a wrong-owner receipt is rejected |
| `egress.validated_content_ref` + evidence | backend | bound to the exact validated artifact (§3.2) |

Test obligations: a forged `gate_outcome` in a submitted proposal is rejected;
a decision referencing another principal's receipt (`execution_ref`) is
rejected as wrong-owner.

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
  "egress": {                              // Workflow 1 only; bound to exact content
    "decision": "n/a|permitted|denied|unresolved",
    "destination": "…", "provider_route": "…",
    "validated_content_ref": "…",          // protected IMMUTABLE ref to the exact
                                           // transformed payload that passed validation
    "transform_receipt_ref": "…",          // operation_id of the transform
    "validation_evidence_ref": "…",        // residual-validation result id (checks that passed)
    "route_revision": "…", "policy_revision": "…"
  },
  "shadow_mode": true
}
```

Correlation (R13): `execution_ref.operation_id` + event ids/hashes resolve to
the existing `MethodExecutionReceipt`; `observed_state_ref` points at the
committed SHM path the operation produced (e.g. OpenFlow
`observed_flows/{bridge}/{sha256}`), never into the receipt store.

**Egress binds to exact content (R5.5/R5.7).** The egress decision names a
destination *and* a protected immutable `validated_content_ref` — the precise
transformed payload that passed residual validation — plus the transform and
validation evidence refs and the route/policy revisions in force. The content
ref is content-addressed (immutable); it is not a mutable pointer that could be
swapped after approval.

**Recheck immediately before dispatch.** Approval obtained before a queue wait
is not sufficient. Immediately before the egress dispatch, the system re-reads
and re-verifies: current verified identity, current grants, the method/route
contract (schema/catalog hash), the destination/provider route, and that the
`validated_content_ref` still equals the payload about to be sent. Any mismatch
(payload substitution, route change, grant revocation, revision bump) aborts to
`unresolved`/`hold` and discloses nothing. Test obligations: payload
substitution after approval, route change after approval, and grant revocation
after approval each block dispatch.

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
state.

**Record uncertainty first, then reconcile (R12).** `uncertain` is persisted
**immediately** as the observed outcome — the record is not held open waiting
for a successful readback. Reconciliation runs afterward within bounded
attempts/time; readback may stay unavailable, in which case the outcome remains
`uncertain` (it never silently flips to `succeeded`). An `uncertain`
unknown-effect operation is **not** blindly retried.

**Cancellation cannot undo completed effects or disclosure.** If an effect or a
disclosure already completed, cancellation records it, not reverses it. Running
work keeps its admission permit, buffers and locks **through** cleanup so a
cancel cannot strand a half-released reservation; release happens in cleanup,
not mid-effect.

**Historical observations need durable evidence, not a mutable path.** A
recorded past outcome references the receipt (`operation_id` + event
ids/hashes) and the generation/revision in force at the time; a bare mutable
SHM path cannot prove past state (SHM is overwritten). `observed_state_ref` is
therefore paired with the immutable event-chain reference that witnessed it.

---

## 5. Storage (proposed)

- Decision records and shadow-mode comparisons: owner-scoped private CozoDB
  (project convention — no SQLite; `op-cozo-store`). **Authoritative writer:**
  `routing_decision` through the private durable publisher
  `PrivateProjectionDirectory::commit_document_durable[_with_decision]`
  (`crates/op-core/src/private_document_publication.rs:71`), **not**
  `commit_state_update`. `commit_state_update` uses the shared plugin
  projection and its audit digests do **not** provide namespace isolation; the
  private publisher gives per-plugin private namespace isolation with the
  required ordering: **durable storage → private SHM projection →
  notification**, run inside a cancellation-owned blocking worker.
- **Intermediate-failure recovery:** once the durable callback starts, any
  error/panic retains the pending gate; recovery reads
  `PrivateProjectionDirectory::pending_document()` for its metadata and uses
  `recover_document_with_decision` for that exact plugin/operation to load the
  authoritative durable document. The original durable-write callback may
  return `Abort` only when its domain owner proves no durable effect was
  submitted. Post-durable contract checks use the `RetainPending`
  decision (keeps the completed receipt while leaving the pending gate intact).
  `routing_decision` is a distinct plugin id (never `identity_sled`, which the
  publisher refuses — it uses its own paired projection).
- **Recovery on activation:** private reads stay disabled until the projection
  is reconstructed/reconciled from validated, owner-scoped **durable storage**.
  Follow the two branches in `chatbot_episode_state.rs:505–519`: if a pending
  marker survives, call `recover_document_with_decision` with its exact plugin
  and operation ID; if no marker survives, call
  `commit_document_durable_with_decision` with a fresh publication ID and a
  callback that reconstructs the document from its durable owner. This is
  projection publication only; it MUST NOT replay the original mutation or its
  effects. `recover_document` requires an existing manifest and matching
  pending operation (`private_document_publication.rs:205–214`), so it cannot
  restore an empty SHM namespace by itself. A missing marker is not evidence of
  a clean durable outcome. Unknown outcomes keep reads blocked and publication
  pending; only a committed publication enables reads and notification. Test
  retained-marker process restart and empty-SHM cold boot separately (T0.1c).
- **Reading private records is not a raw FUSE file read.** The FUSE/3tchedFS
  surface exposes sealed public projections (§1.7). Private, caller-scoped
  decision records require **explicit admitted read methods** on
  `routing_decision` (`list_decisions`/`get_decision`, read capability,
  principal/session-scoped against the private projection), or a separately
  specified private-projection task if a FUSE view is ever wanted. Do not assume
  the public mirror exposes them.
- Quarantine / scoped hold: owner-scoped hold with bounded retention; stores
  only diagnostic + minimal protected references, never raw content (R10).
- Receipts: unchanged — existing `MethodExecutionReceipt` store; the decision
  record carries only the locator.
- Tag vocabulary: sealed into `schema_tag_registry`'s `PluginSchema`/state;
  versioned by `expected_revision`.
- Compliance: no local copy; reads go to the maintained package + its
  active-generation Qdrant scope. The repo stores only review fixtures under
  `reports/compliance-scaffold/`.

Public reads ride committed SHM through the single shared lungs socket /
3tchedFS mirror (NG6, consistent with OD-108's "no SSE/streaming" direction);
private decision-record reads go through the admitted read methods above.

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
| Cancellation | Permits/buffers/locks retained through cleanup; queued reservation released in cleanup; completed effects/disclosures are recorded, not reversed; nothing new disclosed. |
| Uncertain execution outcome | `uncertain` persisted immediately; reconcile by readback within bounded attempts (readback may stay unavailable → stays `uncertain`); **no blind retry** of unknown-effect operations. |

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
- **A4 (implemented source choice):** separate tag registry and decision
  plugins (§2). Shared field classification extends `oscal_subid_registry`;
  it does not create another taxonomy authority.
- **U2 (unresolved, owner):** where egress authorization for Workflow 1 is
  owned — a method on `routing_decision`, or a dedicated egress plugin — and
  what the authoritative destination/provider-route catalog is.
- **U3 (unresolved, owner):** whether tag semantics may ever reference OSCAL
  subids as *constraints* or must stay purely descriptive (affects R1 blast
  radius).
- **U4 (unresolved, owner):** the first concrete declared tag vocabulary and
  which workflow seeds it.
- **U5 (implementation decision):** resolve the bounded backend contract for
  the owner's structured schema-overlay direction. Preserve plugin-owned
  schema authority and discovered contracts for any upstream-backed method
  (R8/NG5). No exposed binary redactor has been found; fixture rendering does
  not supply that capability. Report runtime redaction as unavailable until
  the actual backend implementation is sealed and admitted.
- **U6 (open from source):** the `compliance_general` corpus and
  `compliance_context_v1` scope were captured 2026-09-30; the live generation
  must be re-read at integration time (generations may have advanced).

Proposed contracts are distinct from the unverified source implementation in
§1.10 and from deployed capabilities. Source references establish code shapes,
not live state or completed enforcement.
