# Schema-tagged routing — requirements (first pass)

Status: **draft / proposal**. This document turns
`docs/schema-tagged-routing-whitepaper.md` (concept draft, 2026-09-30) into
numbered, testable requirements. It describes a *proposed* planning and
accountability layer over existing OP-DBUS contracts. Nothing here claims a
deployed policy engine, an installed local classifier, a new capability grant,
or active network enforcement.

Related: OD-100 (model selection), OD-108 (threetched lookup surface / catalog
model authority), OD-91/OD-93 (private digests, catalog fields, pre-dispatch
`evaluate_mutation`), OD-62 (EMQX knowledge delivery). Canonical task board is
`WISHLIST.md`; this spec does **not** replace it. Promote any actionable item
here into `WISHLIST.md` as an `OD-##` and link it back.

The whitepaper's September observations were re-verified against current source
on this branch; see `design.md` §1 for the file/line map and the few
corrections (lungs socket location/path, `ChatMode` location, xray
restart/trace retirement). Where this spec says "existing," it means verified in
current source; where it says "proposed," no such method/field exists yet.

2026-10-12 field-contract review: three read-only batches of five plugins inform
R1.1–R1.8 and design §2c. These are source findings, not completed field
migrations or proof of effective runtime coverage. The owner requests that
workers report concrete cases to the contract owner before applying shared
classification rules across more plugins.

---

## Vocabulary (one meaning per term)

- **PluginSchema** — the single typed input/output/state contract a plugin
  owns (`crates/op-state-store/src/plugin_schema.rs`). opblob seals it; D-Bus,
  gRPC, MCP and 3tchedFS shapes derive from the sealed blob. There is no
  parallel schema source.
- **Schema tag** — a string in `PluginSchema.tags` (`plugin_schema.rs:301`).
  Today the only runtime meaning is `"immutable"`. A tag is **not** an OSCAL
  subid, **not** a Gemma taxonomy entry, and **not** a capability grant.
- **OSCAL subid** — an operational-taxonomy key bound to a method/field
  (`PluginSchema.subids`), resolvable through the `oscal_subid_registry` plugin.
- **Taxonomy entry** — a `GemmaTag {id,label,description,keywords,perspectives}`
  in the revisioned `GemmaTaxonomy` used by the deterministic `gemma_brain`
  keyword classifier. Not a model; substring matching only.
- **Capability grant** — a capability id held by a principal in durable
  `/etc/opdbus/capability-grants.json`, enforced per RPC by the interceptor /
  MutationEngine.
- **Verified identity** — principal + container + session + permitted project
  scope, supplied by identity validation (interceptor / lungs kernel-auth),
  **never** derived from model output.
- **Classification proposal** — a draft `{tag, taxonomy_revision,
  classifier_version, uncertainty}` suggestion. Advisory input to the gate,
  never a decision.
- **Deterministic gate** — the proposed non-model decision step that checks
  verified identity, resolved method contract, explicit policy revision and
  available grants, and emits an explicit allow / deny / unresolved outcome.
- **Transform decision** — a decision to run a bounded local transformation
  (e.g. redaction) on scoped content.
- **Residual-disclosure validation** — deterministic checks on a transform's
  output for leftover identifying material. A model's redaction claim is not
  this check.
- **Egress authorization** — a separate decision authorizing content to cross
  a boundary to a destination/provider route. Distinct from the transform
  decision and from residual validation.
- **Evidence source** — a retrieval surface (recent events, operation
  receipts, authoritative SHM state, NotebookLM history, compliance mappings,
  compliance vector scope). Each carries its own identity, access scope and
  provenance.
- **Operation receipt** — a `MethodExecutionReceipt`
  (`crates/op-state-store/src/method_receipt.rs:54`) locating an admitted
  operation by `operation_id` plus audit-chain event ids/hashes. Evidence, not
  a state store.
- **Compliance generation** — the maintained `compliance-index` graph
  generation hash (`38cdcec0…` is a September capture, not live state). Vector scope and mappings are
  pinned to a generation; mixed generations are rejected.
- **Decision record** — the proposed planning/accountability record that
  correlates verified identity, operation reference, classification proposal,
  policy/evidence and execution record for one request.
- **Shadow mode** — a dry-run comparison of proposals against deterministic
  outcomes that changes no grant, provider selection, flow or proxy rule.

---

## R1 · The plugin is the only schema; tags are metadata, not policy

- WHEN any schema-tagged-routing method, input, output, capability, subid, tag
  or state field is defined THEN it is defined inside an owning plugin's
  `PluginSchema` under `crates/op-plugins/src/state_plugins/` via
  `plugin_scaffold_helpers.rs`, never in a repo-level or plugin `schemas/`
  directory and never as a new gRPC proto service package.
- WHEN a schema tag is read THEN it is treated as an annotation with no
  behavioral meaning until the deterministic gate explicitly implements and
  tests that meaning. A label alone MUST NOT establish authorization, execution
  locality, processing requirement, or compliance.
- Acceptance: a review shows every new field in a sealed `PluginSchema`; a
  negative test shows a tag value (including an undeclared one) does not change
  any capability check, route selection, flow, or proxy rule.
- Acceptance: schema tags, OSCAL subids, taxonomy entries and capability grants
  are represented as four distinct objects; no code path converts one into
  another.
- Owner clarification (2026-10-11): OSCAL metadata belongs to its shared
  `oscal_subid_registry` plugin and is inherited by every plugin so it can
  evolve centrally. Routing MUST resolve those shared bindings and applicable
  metadata, preserving their catalog identity and annotation revision. It MUST
  NOT create another OSCAL authority or require copied annotations in each
  plugin. Missing local field attributes alone do not establish missing
  effective coverage; evaluate the resolved inherited view.
- Owner requirement (2026-10-11): every field MUST acquire an effective
  classification when its schema is created or imported, resolved from the
  shared OSCAL authority. Inheritance satisfies this only when the declared
  rule explicitly covers that field or subtree. Cover method inputs, results
  and state, including nested objects, array items and opaque containers;
  neither a schema-level ID alone nor absence of an annotation implies a
  classification. Validate coverage during creation/import and before sealing;
  unresolved classifications fail validation rather than defaulting to public.
  Preserve the classification source and revision without introducing a
  second taxonomy or guessing upstream argument types.
- Acceptance: adding an uncovered field fails classification validation;
  adding a field covered by an explicit inherited rule resolves to that rule
  with provenance. Nested/array coverage is checked, conflicting or stale
  bindings fail, and changing the shared rule invalidates an outdated resolved
  classification. Existing schemas require a coverage assessment before this
  gate is activated; document gaps without claiming them classified.

- **R1.1 — Root identity.** Classification targets MUST distinguish plugin
  state, each method input, each method result, and each declared signal
  payload. Preserve the canonical method map key separately from its transport
  name. Reusing a DTO under multiple roots MUST NOT copy an exposure decision
  between those roots implicitly. Pin the source schema and shared-rule
  revision for every resolution. Schema examples/defaults and captured
  help/config evidence require separate document-metadata targets; assess
  them before disclosure or exclude them. A state allowlist does not authorize
  exposing omitted values through `PluginSchema.example`.
  Bind the actual result DTO/projection: a summary and its full record are
  distinct. Promotion from a private candidate to a shared catalog MUST
  re-evaluate the copied content at that disclosure boundary; render validation
  or an approved summary does not clear the full candidate or its prompt.
- **R1.2 — Coverage selectors.** Bindings MUST distinguish a schema-document
  pointer from a typed payload selector. Support nested properties, array
  items, map keys separately from map values, local references, nullable
  types, and union branches. Inheritance requires explicit exact/subtree
  scope; a value rule does not classify dynamic map keys. Ambiguous branches,
  conflicting rules, unsupported constructs and stale references cannot
  select a permissive result. An empty collection or absent optional field
  does not prove coverage of its possible values.
- **R1.3 — Existing protection.** Resolution MUST preserve `x-secret` and
  `x-opdbus-private` protections and existing public-state allowlists. A shared
  rule cannot silently widen those projections. Descriptions (including
  `[PII]`), names such as `public_key`, read-only/immutable markers, exact-JSON
  fidelity markers, hashes and subids alone confer no privacy classification.
  Acceptance: the same DTO can remain private in one root and explicitly
  projected in another without either exposure leaking across roots.
- **R1.4 — Opaque and delegated values.** Arbitrary JSON and free-form text
  MUST remain opaque unless a supported contract identifies their structure.
  An explicit restrictive whole-value rule may cover an opaque boundary for
  hold/removal; it MUST NOT claim its interior fields were inspected or permit
  unclassified bytes or keys to pass through. A wrapper such as
  `elevation.authorize.args` may use a target method's schema only after the
  backend resolves that exact target and validates the wrapped value against
  it. Missing or changed targets hold; resolution grants no target capability.
- **R1.5 — Before the first seal.** The shared OSCAL owner MUST support review
  against a producer-prepared candidate schema before the subject field has
  sealed membership. Draft bindings confer no runtime existence or authority.
  Validate against the exact candidate and reviewed rule revision, then bind
  the accepted result to the sealer's final manifest. A candidate change or
  rule change requires revalidation; no self-referential schema hash or second
  captured-schema authority may be introduced. Existing `register`, which
  requires an already sealed subid, is not this authoring path.
  The authority's initial classification contract requires a bounded,
  owner-reviewed bootstrap seed with exact candidate, rule and producer
  provenance, delivered through supported release preparation. Its trust
  cannot depend on calling that contract before its first seal. Subsequent
  revisions use the same admitted authority; bootstrap is not a recovery
  fallback, grant bypass or exemption from coverage of the owner itself.
- **R1.6 — Preservation through conversion/import.** Effective classification
  and protective annotations MUST survive source generation, ordinary or exact
  schema adaptation, and sealing. Loss/change of an existing producer
  protection or a changed root/selector invalidates its assessment. Absence of
  a local protection marker alone establishes neither failed effective
  coverage nor publicness: resolve the applicable shared OSCAL rules. For
  binary-discovered methods, retain the pinned binary/discovery provenance and
  command identity; use config-schema provenance only where that document
  actually supplies the contract. CLI help/arity/choices do not establish
  semantic types, privacy classes, or read-only execution.
- **R1.7 — Bounded and honest assessment.** Coverage traversal MUST be bounded
  by configurable schema bytes, roots, nodes, reference depth/expansions,
  branches, bindings and report entries. Report exhausted/unsupported work
  explicitly, never as complete coverage. Review schema shape and protection
  metadata without copying runtime values, credentials, defaults, examples or
  concrete dynamic keys into reports/prompts. Include the shared classification
  authority itself in the assessment; it has no blanket exemption.
- **R1.8 — Batch feedback and migration.** Audit existing plugins in batches
  of five to ten. Each report MUST identify roots, source references,
  conversion behavior, effective rules verified (or unavailable), opaque
  boundaries, conflicts and proposed contract changes. The contract owner
  reconciles the findings before migrating the next batch. Keep source audit,
  proposed classification, implemented binding, verified coverage and deployed
  enforcement separate. No batch report activates the new sealing gate.
- Acceptance (R1.1–R1.8): fixtures cover reused DTOs across roots; optional and
  union branches; arrays and sensitive map keys; restrictive opaque values;
  a wrapper target mismatch; protection lost by an adapter; an uncovered field
  before its first seal; a changed binary without classification metadata;
  inherited coverage without local markers; private state duplicated into a
  schema example; full records versus summaries; private-to-catalog promotion;
  invalid/replayed bootstrap seeds; stale rules; and exhausted traversal.
  Each failure holds or rejects
  validation without relaxing existing projections. Explicit restrictive
  opaque coverage remains labelled as such, not as a complete interior census.

## R2 · Identity is verified, never modelled

- WHEN any decision record is created THEN its verified-identity fields come
  from the interceptor-anchored session or the lungs kernel-authenticated
  binding (`LungsV1.Bind` / `BindCurrentContainer`), never from classifier or
  model output.
- WHEN a model proposes a classification, plan, destination or identity label
  THEN that output is untrusted gate input and MUST NOT widen principal,
  session, project scope, or grants.
- Acceptance: a decision-record fixture carrying a model-proposed `principal`
  is rejected or the field is ignored in favor of the verified session; a test
  proves identity-bound 3tchedFS admission is unchanged.
- Acceptance: model output containing instructions ("ignore policy", "you are
  authorized") is retained as data and never alters the gate outcome.

## R3 · Models propose; deterministic checks decide

- WHEN an operation or any subsequent disclosure is permitted or denied THEN
  the decision is made by the deterministic gate against verified identity, the
  resolved sealed method contract, explicit policy revision and available
  grants — not by a model's confidence or claim.
- WHEN classification is unknown, low-confidence, or unavailable THEN the gate
  treats the input under its most restrictive applicable constraint; absence of
  a classification MUST NOT relax a constraint.
- WHEN the gate decides policy THEN it is expressed/evaluated at the same
  pre-dispatch layer as the existing canonical compliance check
  (`evaluate_mutation`), so a **direct** call to a protected method encounters
  identical enforcement; a recorded "allow" MUST NOT become reusable
  authorization (it carries no credential or capability). Deny wins over allow;
  a method flagged as policy-required with no applicable rule is `unresolved`
  (fail closed), never implicit allow.
- WHEN the policy store errors, is uninitialized, or is otherwise unavailable
  for a policy-required method THEN the gate yields a typed hold/deny
  (`unresolved`), explicitly NOT inheriting the existing evaluator's
  allow-on-query-error / skip-when-uninitialized behavior
  (`crates/op-cozo-store/src/lib.rs:767`).
- WHEN a protected operation (local effect or egress) is about to dispatch
  after any queue wait THEN identity, grants, contract hash and policy revision
  are re-verified immediately before dispatch; a revision bump, grant
  revocation, stale hash or policy-store unavailability at that moment aborts to
  `unresolved`/`hold`.
- Acceptance: for a fixed fixture set, gate outcomes are reproducible and
  independent of classifier availability; a disabled classifier yields the same
  deny/unresolved outcomes as a present one that returns low confidence; the
  same operation issued via the gate path and via a direct call yields
  identical admission outcomes; a policy-store failure on a policy-required
  method yields `unresolved` (not allow); a policy change enqueued during a
  queue wait is reflected by the pre-dispatch recheck.

## R4 · Execution uses existing admitted plugin methods only

- WHEN the gate authorizes a local operation THEN execution goes through the
  canonical D-Bus / MutationEngine path to an already-admitted plugin method;
  no new control plane, no `Command::new` subprocess, no direct state write.
- WHEN authoritative state changes THEN MutationEngine commits it to SHM before
  change notifications; a receipt is produced as evidence and is not treated as
  a second state authority.
- Acceptance: the slice invokes only methods that exist in the sealed catalog
  (verified by name); a test proves a receipt is written to the receipt store,
  not to canonical state, and that SHM commit precedes the returned output.

## R5 · Workflow 1 — local transform before possible cloud use

Transformation, residual-disclosure validation, and egress authorization are
three separate decisions.

The first visual demo prioritizes structured redaction using a schema overlay
derived from the exact sealed source `PluginSchema` plus an explicit,
machine-readable privacy policy. This is a demonstrator priority after its
identity, schema, policy, and bounded-work prerequisites; it does not activate
egress or reduce the accountability (R6) and compliance retrieval (R7)
workflows. Resolve the field's OSCAL identity and applicable inherited metadata
through the shared OSCAL plugin; neither the identifier string nor a count of
local attributes establishes its effective privacy classification. The current runtime recognizes `x-secret` and
`x-opdbus-private` schema extensions (`crates/op-state-store/src/schema_projection.rs:36-47`);
`[PII]` in a description is not a runtime privacy flag. The legacy
`privacy_index` name heuristics are not an authoritative transform. Unknown,
unclassified, or stale schema paths hold rather than being cleared by default.
Free-text residual validation remains a separate check and is not proved by a
structured overlay.

- R5.1 WHEN sensitive input is submitted for transformation THEN the system
  first resolves the input owner, container/session/project scope and the
  actual transform method contract, and verifies execution locality from the
  verified runtime — a provider or model name MUST NOT be accepted as proof of
  local execution.
- R5.2 WHEN no supported local transform method exists THEN the capability is
  presented as *unavailable*; the system MUST NOT fabricate a redactor method
  or route the content outward as a substitute.
- R5.3 WHEN a transform runs THEN it is admitted under a bounded reservation
  (input/output byte caps, time, concurrency) using only necessary scoped
  content, and emits transformed output plus findings and unresolved spans.
- R5.4 WHEN a transform completes THEN residual-disclosure validation runs
  deterministic detectors and consistency checks over the output; a model's
  redaction claim MUST NOT substitute for this check, and confidence MUST NOT
  be recorded as proof of anonymity.
- R5.5 WHEN disclosure to a destination is considered THEN egress is authorized
  separately against destination, actual provider route, purpose, policy
  revision and current grants; a successful transform or valid JSON MUST NOT
  grant egress. The approval binds to an **immutable** `validated_content_ref`
  (the exact transformed payload that passed residual validation) plus the
  transform receipt, validation evidence, and route/policy revisions; the
  sending operation MUST verify the bytes it is about to send against that
  validated artifact and re-verify identity/grants/route immediately before
  dispatch after any queue wait.
- Acceptance (R5.5): payload substitution after approval blocks the send; a
  route change after approval blocks the send; a revoked grant after approval
  blocks the send; sending bytes that do not match `validated_content_ref`
  fails closed.
- R5.6 WHEN a transform or egress is denied or unresolved THEN the content stays
  local with an explanation; there MUST be no automatic promotion to an
  `anonymous`/cleared label and no cloud fallback.
- R5.7 WHEN correlation is recorded THEN it uses bounded identifiers,
  revisions and protected references; originals, removed tokens and sensitive
  detector snippets MUST NOT be logged by default, and hash visibility is scoped
  because hashes reveal equality.
- R5.8 WHEN a structured redaction overlay is rendered THEN its paths and
  classifications MUST be derived from the exact sealed source schema,
  resolved shared OSCAL metadata, and an explicit machine privacy policy, with
  source schema/catalog identity, annotation revision and policy revision
  pinned in the result. Schema descriptions, field names, unresolved OSCAL IDs, classifier proposals,
  or historical catalogs alone MUST NOT grant public clearance. Unknown,
  unclassified, and stale paths hold for review; there is no automatic
  pass-through. The renderer MUST hydrate values locally from an authorized
  fixture or source after applying the overlay, and MUST NOT send private
  originals to a model, prompt history, or gallery. Real execution results
  MUST retain the admitted backend receipt; the UI MUST NOT act as a second
  policy evaluator or fabricate evidence. Fictional fixture results MUST be
  visibly identified as fixtures.
- Acceptance: a defensible result states only "the specified transform
  completed; listed checks passed; stated residual risks remain; the separately
  evaluated destination was permitted/denied." Negative tests: redaction-claim
  without validation fails to authorize egress; absent transform method stays
  unavailable; denied egress leaves content local. For the structured demo,
  tests pin the sealed schema hash, prove unknown/stale paths hold, prove
  private originals are absent from model input/history/gallery, and keep
  free-text residual validation independently required.

## R6 · Workflow 2 — accountability against independent evidence

- R6.1 WHEN a conversational claim is checked THEN the system retrieves recent
  events, operation receipts and authoritative SHM state through independently
  authorized evidence queries — each query independently authorizes the
  **verified caller** against its own access scope, time window and provenance;
  adjacent panels MUST NOT merge identities or histories. (Independent evidence
  does not require a second identity; it requires an independent authorization
  check of the same verified caller.)
- R6.2 WHEN evidence is correlated THEN request, acceptance, SHM publication,
  native readback, failure and cancellation are distinguished; a conversation
  passage MUST NOT be treated as a native receipt, and semantic similarity MUST
  NOT be treated as proof of execution or authorization.
- R6.3 WHEN NotebookLM or other memory history is used THEN it is accessed only
  through verified named access, preserving owner, conversation id,
  capture/retrieval time and revision; conversation provenance is kept distinct
  from current SHM, grants and compliance authority.
- R6.4 WHEN an evidence source is unavailable THEN it is reported as a gap /
  unavailable; a missing event MUST NOT be rendered as a confirmed negative or
  a confirmed positive.
- Acceptance: a claim with no matching receipt reports "no receipt found," not
  "did not happen"; an evidence panel labels each item's source identity and
  retrieval time; a test proves the recent-event query and the chat session
  each independently authorize the verified caller (not that they use different
  identities).

## R7 · Workflow 3 — source-backed policy/compliance retrieval

- R7.1 WHEN compliance evidence is retrieved THEN it comes from the maintained
  canonical OSCAL mappings in Qdrant and their maintained `compliance-index`
  provenance/scope contracts (mappings + active-generation vector scope),
  preserving mapping direction, framework versions, ASVS lifecycle relations,
  unresolved references and graph generation; the system MUST NOT build a
  parallel compliance graph.
- R7.1a WHEN a routing policy references an OSCAL mapping THEN the backend
  resolves its canonical identifier, direction, source hash and active
  generation through the admitted evidence surface. The tag registry stores
  references to this authority, never caller-authored claims that a mapping
  resolved. Missing or unverifiable mappings hold the protected operation.
- R7.2 WHEN a retrieval scope is empty THEN zero results are returned; an empty
  `allowed_groups` means `qdrant_filter: null` and the system MUST NOT issue an
  unfiltered Qdrant query. The `scope`/`route`/`mappings` contracts return
  eligibility and relationships, **not** vector-search results; the actual
  vector search is a separate bounded admitted operation that uses exactly the
  returned filter and validates each result's group/context membership against
  the active generation.
- R7.2a WHEN a `route` is returned THEN it is treated as depth-bounded even when
  `truncated` is false; `scope` exposes no truncation indicator, so a consumer
  MUST carry an explicit completeness limitation rather than claim completeness.
- R7.3 WHEN captures or results of different graph generations are present THEN
  the system rejects the mixed set rather than merging it, and labels truncated
  routes as incomplete.
- R7.4 WHEN compliance evidence is displayed THEN mapping direction and
  endpoint resolution status (resolved / unresolved / historical) are preserved
  per edge with source + sha256 evidence; a mapping MUST NOT be presented as
  equivalence, enforcement, or execution authority.
- R7.5 WHEN no sealed bridge method yet exposes these contracts THEN live
  integration is shown as *unavailable*, and only captured review fixtures are
  displayed, marked as captures.
- Acceptance: an empty-scope tag yields zero results and issues no query; a
  mixed-generation fixture fails; a reversed traversal stays labelled
  `reverse:maps_to`; a historical ASVS edge stays historical; nearest-neighbor
  documents do not authorize any operation.

## R8 · Required OVS/OpenFlow routing with separate admission and forwarding

- WHEN an application-level decision is made THEN it is kept distinct from
  OpenFlow (packet matches/actions), OVSDB (bridge/port/interface config) and
  Xray (proxy routing) ownership.
- WHEN an authorized routing decision requires forwarding THEN it uses the
  existing OVS OpenFlow controller through an explicit admitted adapter and
  verified controller readback; a schema tag, model role, or saved
  draft MUST NOT be accepted as a network argument and MUST NOT install a flow
  or change proxy state.
- The reserved OpenFlow controller endpoint on the OVS bridge carries control
  traffic; it does not by itself identify the request's dataplane traffic.
  Controller mutations require authenticated operation attribution and an
  authoritative identity-to-traffic binding. Missing bindings hold forwarding.
- OVS/OpenFlow integration is required for this feature. Lungs owns local
  identity-bound admission; OVS owns packet forwarding. Xray is not the routing
  controller for this feature. Existing controller denial of unattributed
  mutations must remain enforced until attributed admission is implemented.
- WHEN a threetched-backed capability is used THEN supported commands/flags are
  discovered from the pinned binary and its sealed contract; the system MUST
  NOT invent commands, argument types, provider lists, or parallel schemas.
- WHEN routing/model roles are assigned THEN they are model-agnostic and actual
  execution locality is verified; a provider name alone MUST NOT establish
  locality.
- Acceptance: a test proves no schema-tag/model-intent reaches
  `openflow.*`/`ovsdb.*`/`xray.*` inputs; `SetOvsRoutingModel` records an
  assignment and installs no flow; a threetched method absent from the binary's
  discovered surface is reported unavailable rather than fabricated.

## R9 · Provenance and label origin

- WHEN any label is recorded THEN its origin is retained (declaration, user
  selection, keyword match, model proposal, or validated transform) and travels
  with the label.
- WHEN an operation reference is recorded THEN it preserves exact plugin id,
  method key, transport name, schema hash/version, catalog identity, OSCAL
  subid, taxonomy revision, policy revision and source references; a schema hash
  (contract bytes), catalog hash (membership) and taxonomy revision are distinct
  and updating one MUST NOT silently update another.
- WHEN an association can no longer be resolved THEN it is marked stale/visibly
  provisional rather than dropped or silently repaired.
- Acceptance: a decision-record fixture round-trips all identity fields; a stale
  schema hash renders the association as stale; a user-selected tag and a
  keyword-matched tag are distinguishable in the record.

## R10 · Failure outcomes (no silent broadening)

- WHEN any stage fails — unknown tag, stale catalog identity, denied capability,
  unavailable evidence, invalid model output, capacity exhaustion, cancellation,
  or an uncertain execution outcome — THEN the result is an explicit scoped hold
  / diagnostic.
- No failure may (a) broaden identity or scope, (b) disclose content through a
  cloud fallback, or (c) retry an operation whose effects are unknown without
  reconciliation and idempotency rules.
- WHEN raw content would be logged on failure THEN it is suppressed; only the
  diagnostic and minimal protected references needed for review are retained.
- Acceptance: fault-injection tests for each failure class show an explicit
  outcome with no grant/scope change, no outward disclosure, and no blind
  retry; an uncertain native outcome triggers reconciliation, not re-dispatch.

## R11 · Resource ownership and limits

- WHEN inputs are copied THEN a bounded admission reservation is acquired first;
  payload size, taxonomy size, result count, queue depth, active calls and
  receipt/record retention are each independently limited.
- WHEN limits are defined THEN each has a concrete configurable default and a
  named owner, covering at minimum: per-principal and global input/output
  bytes; per-principal and global queue depth; concurrent active work;
  evidence fan-out (max sources/queries per accountability check); pagination
  page size and max page; retained decision-record count; and record expiry.
  Defaults ship in config (owner-scoped), not hard-coded per call site.
- WHEN capacity is exhausted THEN backpressure is visible as a capacity/timeout
  status; a busy local worker MUST NOT trigger a boundary-crossing (cloud)
  fallback.
- Acceptance: each limit has a documented default + owner; exceeding each limit
  returns a typed capacity/timeout outcome; a saturated local transform worker
  yields "capacity" status, not an egress.

## R12 · Cancellation and deadlines

- WHEN a stage is given work THEN it has an owned deadline and cancellation
  owner; on cancellation, queued reservations are released during cleanup and
  effects are reconciled rather than blindly retried. Permits, buffers and locks
  for running work are retained through cleanup; a completed effect or
  disclosure is recorded, not reversed.
- WHEN a native effect outcome is uncertain THEN the uncertain outcome is
  persisted immediately and reconciliation runs afterward within bounded
  attempts; readback may remain unavailable, leaving the outcome `uncertain`
  (it never silently becomes `succeeded`).
- Acceptance (R12), by case:
  - *Queued cancellation* (no effect started): the reservation is released and
    nothing is dispatched or disclosed.
  - *Running cleanup* (effect in flight, outcome not yet known): permits,
    buffers and locks are retained through cleanup; the outcome is recorded as
    `uncertain` and reconciliation is scheduled.
  - *Completed before cancellation* (effect/disclosure already done): the
    completed effect/disclosure is recorded, not reversed; cancellation does not
    claim it was prevented.
  - *Unavailable readback*: the outcome remains `uncertain` after bounded
    reconciliation attempts; it never silently becomes `succeeded`, and the
    unknown-effect operation is not blindly retried.

## R13 · Audit and durable-evidence policy

- WHEN an operation requires durable audit evidence THEN missing audit capacity
  is detected before dispatch (fail closed), not after the effect.
- WHEN a decision is recorded THEN it links request, admitted operation,
  receipt (`operation_id` + event ids/hashes) and committed SHM observation,
  with input/result masking for sensitive fields.
- WHEN a process/container (re)activates THEN private decision-record reads stay
  disabled until its private projection is reconstructed/reconciled from
  validated, owner-scoped durable storage and committed. Recovery MUST NOT
  replay the original mutation or its effects. Volatile SHM markers MUST NOT be
  trusted across a reboot, and an empty pending marker alone MUST NOT enable
  reads without the durable reconciliation pass.
- Acceptance: with audit capacity unavailable, a durable-audit-required
  operation is refused before dispatch; a completed operation's decision record
  resolves to its receipt and the committed state it produced. Separate tests
  cover process restart with a retained pending marker and cold boot with
  durable rows retained but empty SHM; both keep private reads blocked until
  validated publication commits, preserve owner scope, and replay no effects.
  An unproven durable outcome keeps reads blocked.

## R14 · Phasing without scope reduction

- WHEN the feature is delivered THEN it begins as a bounded shadow-mode /
  evidence slice, prioritizes a redaction-first visual demo only after its
  identity/schema/policy/bounds prerequisites, and reaches all three workflows
  (R5–R7); the demo is not a substitute for the complete workflows and does
  not activate live egress. The plan MUST NOT silently reduce the feature to a
  demonstration or discard the transform, accountability, or compliance
  workflow.
- Acceptance: `tasks.md` maps every workflow (R5–R7) to later phases with
  verification and links the redaction-first demo to R5.8 without treating it
  as egress authorization; the first slice asserts "no grant/provider/flow/proxy
  change during a dry run."

---

## Non-goals (explicit)

- NG1 · No deployed policy engine, installed local classifier, new capability
  grant, or active network enforcement in this spec's scope.
- NG2 · No new schema authority: no repo-level `schemas/` dir, no parallel JSON
  contract, no second captured-schema source. Captures remain fixtures.
- NG3 · No automatic `anonymous`/cleared promotion from a transform; no cloud
  fallback on local saturation or failure.
- NG4 · No direct tag/role/draft-driven network changes. Required OVS/OpenFlow
  forwarding uses admitted exact typed operations, authoritative traffic
  binding and readback; it does not transfer controller ownership to Xray.
- NG5 · No invented threetched commands, argument types, provider lists, or
  parallel schemas; discovery is from the pinned binary + sealed contract.
- NG6 · No new SSE/streaming control plane; reads ride committed SHM via the
  single shared lungs socket / 3tchedFS mirror (consistent with OD-108).
- NG7 · This spec does not replace `WISHLIST.md`, does not seal blobs, does not
  deploy, and does not modify identity, grants, seeds, or services.
- NG8 · No model/provider identity is treated as proof of execution locality.
