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
  generation hash (currently `38cdcec0…`). Vector scope and mappings are
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
- Acceptance: for a fixed fixture set, gate outcomes are reproducible and
  independent of classifier availability; a disabled classifier yields the same
  deny/unresolved outcomes as a present one that returns low confidence.

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
  grant egress.
- R5.6 WHEN a transform or egress is denied or unresolved THEN the content stays
  local with an explanation; there MUST be no automatic promotion to an
  `anonymous`/cleared label and no cloud fallback.
- R5.7 WHEN correlation is recorded THEN it uses bounded identifiers,
  revisions and protected references; originals, removed tokens and sensitive
  detector snippets MUST NOT be logged by default, and hash visibility is scoped
  because hashes reveal equality.
- Acceptance: a defensible result states only "the specified transform
  completed; listed checks passed; stated residual risks remain; the separately
  evaluated destination was permitted/denied." Negative tests: redaction-claim
  without validation fails to authorize egress; absent transform method stays
  unavailable; denied egress leaves content local.

## R6 · Workflow 2 — accountability against independent evidence

- R6.1 WHEN a conversational claim is checked THEN the system retrieves recent
  events, operation receipts and authoritative SHM state through independently
  scoped evidence sources, each preserving its own identity, access scope and
  provenance; adjacent panels MUST NOT merge identities or histories.
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
  retrieval time; a test proves the recent-event query and the chat session use
  separate identities/scopes.

## R7 · Workflow 3 — source-backed policy/compliance retrieval

- R7.1 WHEN compliance evidence is retrieved THEN it comes from the maintained
  `compliance-index` package (mappings + active-generation vector scope),
  preserving mapping direction, framework versions, ASVS lifecycle relations,
  unresolved references and graph generation; the system MUST NOT build a
  parallel compliance graph.
- R7.2 WHEN a retrieval scope is empty THEN zero results are returned; an empty
  `allowed_groups` means `qdrant_filter: null` and the system MUST NOT issue an
  unfiltered Qdrant query.
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

## R8 · Ownership separation (application vs network)

- WHEN an application-level decision is made THEN it is kept distinct from
  OpenFlow (packet matches/actions), OVSDB (bridge/port/interface config) and
  Xray (proxy routing) ownership.
- WHEN network enforcement is desired THEN it requires an explicit admitted
  adapter and a verified native readback; a schema tag, model role, or saved
  draft MUST NOT be accepted as a network argument and MUST NOT install a flow
  or change proxy state.
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
  receipt retention are each independently limited.
- WHEN capacity is exhausted THEN backpressure is visible as a capacity/timeout
  status; a busy local worker MUST NOT trigger a boundary-crossing (cloud)
  fallback.
- Acceptance: exceeding each limit returns a typed capacity/timeout outcome; a
  saturated local transform worker yields "capacity" status, not an egress.

## R12 · Cancellation and deadlines

- WHEN a stage is given work THEN it has an owned deadline and cancellation
  owner; on cancellation, queued reservations are released and effects are
  reconciled rather than blindly retried.
- WHEN a cancellation races a native effect THEN the system reconciles the
  observed state via readback before reporting outcome.
- Acceptance: a cancellation test releases the reservation and leaves no
  half-applied effect unreconciled; a cancelled transform discloses nothing.

## R13 · Audit and durable-evidence policy

- WHEN an operation requires durable audit evidence THEN missing audit capacity
  is detected before dispatch (fail closed), not after the effect.
- WHEN a decision is recorded THEN it links request, admitted operation,
  receipt (`operation_id` + event ids/hashes) and committed SHM observation,
  with input/result masking for sensitive fields.
- Acceptance: with audit capacity unavailable, a durable-audit-required
  operation is refused before dispatch; a completed operation's decision record
  resolves to its receipt and the committed state it produced.

## R14 · Phasing without scope reduction

- WHEN the feature is delivered THEN it begins as a bounded shadow-mode /
  evidence slice and reaches all three workflows across later phases; the plan
  MUST NOT silently reduce the feature to a demonstration or discard the
  transform, accountability, or compliance workflow.
- Acceptance: `tasks.md` maps every workflow (R5–R7) to later phases with
  verification; the first slice asserts "no grant/provider/flow/proxy change
  during a dry run."

---

## Non-goals (explicit)

- NG1 · No deployed policy engine, installed local classifier, new capability
  grant, or active network enforcement in this spec's scope.
- NG2 · No new schema authority: no repo-level `schemas/` dir, no parallel JSON
  contract, no second captured-schema source. Captures remain fixtures.
- NG3 · No automatic `anonymous`/cleared promotion from a transform; no cloud
  fallback on local saturation or failure.
- NG4 · No tag/role/draft-driven network changes; OpenFlow/OVSDB/Xray ownership
  is unchanged and requires explicit admitted adapters + readback.
- NG5 · No invented threetched commands, argument types, provider lists, or
  parallel schemas; discovery is from the pinned binary + sealed contract.
- NG6 · No new SSE/streaming control plane; reads ride committed SHM via the
  single shared lungs socket / 3tchedFS mirror (consistent with OD-108).
- NG7 · This spec does not replace `WISHLIST.md`, does not seal blobs, does not
  deploy, and does not modify identity, grants, seeds, or services.
- NG8 · No model/provider identity is treated as proof of execution locality.
