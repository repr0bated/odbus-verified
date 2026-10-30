# Schema-tagged routing — tasks (first pass)

Status: **draft / proposal**. Dependency-ordered implementation plan for
`requirements.md` + `design.md`. `WISHLIST.md` remains the canonical task board;
when a task here is picked up, promote it to an `OD-##` there and link back.
This document authorizes no build, test, seal, or deployment by itself.

Conventions:
- **R-refs** map each task to requirement IDs.
- Phases keep **implementation**, **validation**, **sealing**, and
  **activation** distinct — a build is not a seal, a seal is not a deployment
  (per `AGENTS.md`). Sealing/activation tasks are owner-gated and never bundled
  into an implementation task.
- "Verify" means a meaningful check (typed test, negative test, or readback),
  not "it compiled."
- All Cargo work is delegated per `AGENTS.md` (`scripts/cargo-dispatch.md`).
- Owner sequencing for this work: build at the end unless development needs
  it sooner; execute tests after deployment. Author the verification fixtures
  below during implementation and record them as unrun. This exception does
  not turn an unrun check into a pass or authorize network activation.

2026-10-12 progress: Luna has returned four read-only batches of five plugin
audits (design §2c.4). Root incorporated the cases into R1.1–R1.8 and §2c.
No fields were migrated by those audits. Shared classification contracts,
their implementation and effective live coverage remain separate deliverables.

Source implementation now covers the T0.2b/T0.2c assessment foundation:
`oscal_field_classification.rs`, pure candidate evaluation and the shared
owner's read-only `assess_fields`. Source declarations are version 2.1.0; they
are not sealed, granted or deployed. Bounded traversal/reporting and service
revision/pin checks are implemented; regression fixtures are authored but
unrun. Scoped formatting and GNU compilation of `op-plugins` and `op-grpc-bridge`
passed; existing unrelated warnings remain. Neither task
is complete: reviewed authoring/first-seal initialization, raw producer
preservation, delegated payload binding and creation/seal integration remain
open. Metadata is held as one opaque document boundary in this slice. See
design §2c for exact implementation limits.

Legend: 🟢 shadow-safe (no grant/flow/proxy change) · 🔒 owner-gated
(seal/activate) · ⛔ depends-on.

---

## Phase 0 — Boundary, minimal gate, honest UI (first vertical slice) 🟢

Goal: a typed boundary that shows evidence and drafts, persists caller-scoped
shadow decisions through an admitted writer, evaluates a minimal deterministic
gate, and changes nothing else. After its identity, schema, policy, and bounded
work prerequisites, prioritize the redaction-first structured visual demo
(R5.8); this priority neither activates egress nor removes either other
workflow (R6/R7).

Internal consistency: shadow decisions are **persisted** (not read-only), so
Phase 0 includes an admitted caller-scoped write method (T0.1b) and a minimal
deterministic evaluator (T0.3a). The alternative — keeping shadow results as
disposable in-memory fixtures with no persistence — is called out in T0.4 as an
explicit owner choice.

- **T0.0 — Concrete limits + audit-reservation config (prerequisite)** 🟢
  - R-refs: R11, R13. ⛔ none.
  - Component: owner-scoped config defaults + a bounded-reservation helper
    reused by all later stages (bytes per-principal/global, queue depth, active
    work, evidence fan-out, pagination page/max, retained-record count,
    expiry); a durable-audit-reservation primitive acquired **before** dispatch.
  - Deliverable: documented default + named owner for each limit; a
    reservation API that fails closed when audit capacity is unavailable.
  - Verify: exceeding each limit returns a typed capacity/timeout; an operation
    flagged durable-audit-required is refused before dispatch when audit
    capacity is unavailable. **Must land before any persistent shadow run.**

- **T0.1 — Decision-record DTO + private store (`routing_decision` skeleton)**
  - R-refs: R1, R2, R9. ⛔ T0.0.
  - Component: `crates/op-plugins/src/state_plugins/routing_decision.rs`
    (proposed), schemars DTOs per design §3.2; private CozoDB store
    (`op-cozo-store`).
  - Deliverable: sealed-shape `PluginSchema` with caller-scoped **read** methods
    (`list_decisions`, `get_decision`, read capability, principal/session-
    scoped). Record round-trips all identity + operation_ref + label-origin
    fields.
  - Verify: unit test round-trips a record; negative test shows a model-proposed
    `principal` is ignored in favor of the verified session; test proves no
    capability/route effect from any `tag` value; a read as a different
    principal returns nothing (scope isolation).

- **T0.1b — Admitted caller-scoped record writer**
  - R-refs: R1, R2, R4, R13. ⛔ T0.1, T0.2, T0.3a.
  - Component: a `record_decision` mutation on `routing_decision` (its own write
    capability) that writes a shadow decision via the private durable publisher
    `PrivateProjectionDirectory::commit_document_durable[_with_decision]`
    (`crates/op-core/src/private_document_publication.rs:71`) — **not**
    `commit_state_update` (whose shared projection + audit digests give no
    namespace isolation). Ordering: durable storage → private SHM → notification,
    in a cancellation-owned blocking worker; intermediate failure retains the
    pending gate. Inspect its metadata through `pending_document()` and recover
    the exact plugin/operation through `recover_document_with_decision`.
  - Input contract (anti-forgery, R2 extended): the caller submits
    **proposals only** (`classification_proposal`, a candidate operation request,
    `shadow_mode`). The backend authors the trusted fields —
    `verified_identity` (from session/lungs), `operation_ref` (re-resolved via
    T0.2, not trusted from the caller), `gate_outcome` (computed by the
    evaluator), `policy`/revisions, and any `execution_ref` — and validates
    **receipt ownership** (the referenced `operation_id`/event ids must belong
    to this caller's operation). A caller-supplied `gate_outcome` or
    `verified_identity` is rejected/ignored.
  - Deliverable: the only path that persists a decision record; carries no
    credential and grants nothing.
  - Verify: write-then-read round-trip under one principal; a second principal
    cannot read it; the record is committed to the **private** projection (not
    the shared plugin projection and not the receipt store), SHM commit precedes
    notification; a forged `gate_outcome` in the proposal is rejected; a
    decision referencing another principal's receipt is rejected as wrong-owner;
    an intermediate-failure fixture uses the pending operation's metadata to
    recover its authoritative durable document without substituting a different
    operation.

- **T0.1c — Reboot recovery / reconciliation before private reads**
  - R-refs: R4, R13, R10. ⛔ T0.1b.
  - Component: an activation-time recovery step that reconstructs/reconciles
    `routing_decision`'s private projection from validated, owner-scoped
    **durable storage** before read methods are enabled. Follow
    `crates/op-grpc-bridge/src/chatbot_episode_state.rs:505–519`: a surviving
    pending marker selects `recover_document_with_decision` with its exact
    plugin/operation ID; an absent marker selects
    `commit_document_durable_with_decision` with a fresh publication ID. Both
    callbacks load/reconcile the authoritative durable document; neither
    replays the original mutation or its effects. The recovery helper requires
    a surviving manifest and exact pending operation
    (`crates/op-core/src/private_document_publication.rs:205–214`).
  - Deliverable: private reads and notification wait for committed publication;
    an unproven durable outcome keeps reads blocked and publication pending.
  - Verify: separate disposable fixtures for (a) **process restart** after a
    mid-publish failure with the pending marker retained and (b) **cold boot**
    with durable rows retained but an empty SHM namespace. The first recovers
    the exact pending operation; the second publishes a validated durable
    reconstruction under a fresh publication ID. Both keep private reads
    blocked until commit, preserve owner scope, and execute no original effect.
    An unknown durable outcome in either fixture leaves reads blocked;
    `pending_document() == None` alone never enables reads.

- **T0.2 — Operation-reference resolver (read-only)**
  - R-refs: R9, R1. ⛔ T0.1.
  - Component: resolver that maps a request to `{plugin_id, method_key,
    transport_name, schema_hash, schema_version, catalog_hash, subid}` from the
    sealed catalog, reusing `oscal_subid_registry.resolve`/`lookup`.
  - Deliverable: resolver returns a fully-pinned `operation_ref` or a `stale`
    marker on hash mismatch.
  - Verify: test with a current hash resolves; test with a mutated hash yields
    `stale`, not a silent re-pin.

- **T0.2a — Field evidence batches and contract feedback** 🟢
  - R-refs: R1.1–R1.4, R1.6–R1.8, R9. ⛔ none for source review.
  - Component: workers audit five to ten plugins at a time and return findings
    to the shared-contract owner. Include state, input, result and declared
    signal roots, reachable DTO modules, selectors, protection flags,
    projection allowlists, separate document-metadata targets,
    conversion/import paths and unresolved boundaries.
  - Deliverable: source-pinned evidence and proposed contract corrections,
    recorded in this spec and the shared handoff. Effective inherited coverage
    must be resolved through the shared OSCAL authority or explicitly reported
    unavailable. No field-name/public-by-absence classification, runtime values,
    secret examples, or guessed upstream argument types.
  - Verify: contract review accounts for each batch's concrete counterexample;
    source-audited, proposed, migrated, verified and deployed counts remain
    distinct. Initial batches: elevation, ctl_plane_chatbot, chat_response,
    notebooklm, project; then openflow, ovsdb_bridge, qdrant, keypair,
    threetched; then oscal_subid_registry, schema_tag_registry,
    routing_decision, model_registry, json_render. Fifteen source audits,
    zero field migrations; live effective inheritance remains unverified.

- **T0.2b — Shared OSCAL field contract and candidate authoring** 🟢
  - R-refs: R1.1–R1.8, R2, R9. ⛔ T0.2a (initial reviewed batch).
  - Component: extend `oscal_subid_registry` with the typed contract in design
    §2c: distinct roots/source pins, schema pointers and payload selectors,
    exact/subtree/key coverage, reviewed class references and bounded
    assessment outcomes. Classification remains separate from routing tags,
    projection policy, transform actions and egress authorization.
  - Deliverable: candidate review before the subject field's first seal,
    owned by the same shared authority. Existing `register` cannot provide
    this because it requires sealed membership. Drafts grant no membership,
    and caller-authored resolution claims are never trusted. Define the
    source/rule revalidation lifecycle without a self-referential schema hash.
    Include a bounded, owner-reviewed initial authority seed through supported
    release preparation, pinned to its candidate/rules/producer provenance;
    do not call the unsealed authoring method to establish its own trust.
  - Verify: candidate without a sealed subid can be assessed but not resolved
    as a runtime plugin; changed candidate/rule pins invalidate its assessment;
    stale or wrong-owner review cannot publish a rule. No duplicated taxonomy
    or new repository-level schema files. A mismatched/replayed bootstrap seed
    fails; loss of the durable authority during recovery cannot enable a fresh
    bootstrap or exempt the owner's own fields from coverage.

- **T0.2c — Bounded classification resolver and preservation checks** 🟢
  - R-refs: R1.1–R1.7, R5.8, R9–R12. ⛔ T0.0, T0.2, T0.2b.
  - Component: one shared resolver over the exact source roots and reviewed
    OSCAL rules; explicit property/item/map-key/map-value selectors; bounded
    local references and alternatives; whole-value protection for opaque data;
    exact target binding for delegated arguments. Preserve source and rule
    provenance and existing projection restrictions.
  - Deliverable: backend-derived field selection and coverage results reused
    by schema validation and the structured overlay. The UI consumes results;
    it does not implement another resolver. Pure fixtures can support T0.3
    while live inherited coverage is unavailable, clearly labelled as fixtures.
  - Verify: reused DTO under different roots; array/optional/union coverage;
    map keys distinct from values; reference-site private flags; restrictive
    opaque boundary without claiming interior inspection; delegated target
    mismatch; finite-adapter protection loss versus exact/direct method
    schemas; binary provenance change; missing rules and exhausted traversal.
    Include inherited coverage without local markers, and private state
    duplicated into `PluginSchema.example`: unassessed metadata is excluded
    from catalog disclosure/model context and never enters the payload overlay.
    Negative cases reject/hold without broadening a projection or leaking raw
    values; valid inherited coverage resolves with its source and revision.
    Full versus summary DTOs and gallery promotion must evaluate actual copied
    fields under source/destination roots; a summary's coverage or a Canvas
    render-validation result cannot clear a full private record or its prompt.

- **T0.2d — Creation/import/seal coverage integration** 🟢→🔒
  - R-refs: R1.5–R1.8, R9, R10. ⛔ T0.2b, T0.2c.
  - Component: run the shared resolver against producer candidates before and
    after adaptation and at sealing; bind accepted assessments to the final
    sealer manifest. Assessment only during initial migration; enforcing
    rejection requires the reviewed affected-surface migration in T4.1a.
  - Deliverable: missing/conflicting/stale coverage and protection loss are
    validation failures once activated. Include the classification owner's own
    schema; no exemption or default-to-public bootstrap. Binary imports retain
    actual command/schema provenance without semantic argument inference.
  - Verify: new field before first seal; rule change during preparation;
    adapter drops a private extension; importer changes a command; final
    manifest does not match the assessed candidate. Failure cannot publish a
    successful assessment or silently enable runtime authority.

- **T0.3 — Honest UI panel (captures vs live; label origin)**
  - R-refs: R1.1–R1.7, R5.8, R7.4, R7.5, R9, R10, R14.
    ⛔ T0.0, T0.1, T0.2, T0.2c, T0.3a.
  - Component: redaction-first, local-data demo in the existing
    `/srv/git/3tched.com` SvelteKit/Svelte 5/Vite `@json-render/svelte`
    application, through its existing `ui_render` prompt and
    `compileChatCanvas` hydration flow. Render decision records, compliance
    **captures** from `reports/compliance-scaffold/20260930-discovery/`, and
    unavailable live integration. A schema overlay must derive from the exact
    sealed source schema, shared OSCAL inheritance and explicit machine privacy policy; values hydrate
    locally after masking. Use fictional content or a content-free model
    template; do not send private originals to prompts, history, or gallery.
    `ui_render` skips memory but ordinary provider/fallback selection means it
    does not prove local/private inference. No new fixed page or schema source.
  - Deliverable: panel preserves mapping direction, endpoint resolution status,
    generation, truncation; shows label origin; marks captures as captures;
    demonstrates the schema-derived structured overlay and holds unknown,
    unclassified, or stale paths; drafts invoke no mutations. This is a visual
    demonstration, not proof of a deployed redactor or permission to send data.
    Fictional fixtures are labelled; real results retain backend receipts and
    do not use a second UI-owned policy evaluator.
    Use T0.2c's shared resolver; the fixture demo does not depend on the later
    live transform in T4.1 or activation of the coverage gate in T4.1a.
  - Verify: UI validation rejects mixed generations; empty scope renders zero
    results and issues no query; a reversed traversal stays
    `reverse:maps_to`; a historical ASVS edge stays historical; a non-truncated
    route is still labelled depth-bounded (not "complete"); the overlay pins
    the sealed schema hash/version; a fixture proves private originals are
    absent from model input/history/gallery; free-text residual validation
    remains separately required (R5.4).

- **T0.3a — Minimal deterministic evaluator**
  - R-refs: R3, R2, R10. ⛔ T0.2.
  - Component: the Phase-0 subset of the gate (full gate is T1.2): a pure,
    model-free function over `{verified_identity, operation_ref,
    classification_proposal, policy?}` → `authorized_local | denied |
    unresolved | hold`. Phase 0 supports `deny_method` + missing-policy →
    `unresolved`; `tag_constraint` evaluation arrives with T1.2. The
    policy-required set and rules are **fixed fixtures** in Phase 0 (no live
    administration; see T1.1b); the "requires policy" marker is explicit, never
    inferred from rule presence.
  - Deliverable: deterministic outcome usable by T0.4 without the full tag
    semantics; a policy-store error on a policy-required method yields
    `unresolved` (does not inherit the existing allow-on-error behavior).
  - Verify: same inputs → same outcome regardless of classifier availability;
    unknown tag / stale hash → `unresolved`/`hold`, never allow; simulated
    policy-store failure on a policy-required fixture → `unresolved`.

- **T0.4 — Shadow-mode comparison (dry run)**
  - R-refs: R3, R14, R11. ⛔ T0.1b, T0.2, T0.3a.
  - Component: `routing_decision` dry-run path that records a classification
    proposal (`gemma_brain.classify`, advisory) + the T0.3a deterministic
    *would-be* outcome, persisted via T0.1b with `shadow_mode=true`.
  - Deliverable: for a bounded fixture set, paired (proposal, deterministic
    outcome) records; false-negative / ambiguity / latency / memory / cost
    metrics.
  - Owner choice: if persistence is not wanted yet, T0.4 may instead emit
    **disposable in-memory fixtures** and skip T0.1b — but then shadow results
    are not durable evidence. Default recommendation: persist via T0.1b.
  - Verify: assertion that **no** grant, provider selection, flow, or proxy rule
    changes during a dry run; model output containing bypass instructions stays
    data and does not alter the outcome.

- **T0.5 — Phase-0 focused validation** 🟢
  - R-refs: all Phase-0. ⛔ T0.0–T0.4.
  - Deliverable: **focused** checks on the new/changed crates
    (`cargo fmt -p … -- --check`, `cargo clippy -p op-plugins -p op-grpc-bridge
    … -- -D warnings`, targeted `cargo test -p …`). This is NOT the full
    workspace `/verify`; name it as focused and distinct.
  - Verify: delegated Cargo run (per `scripts/cargo-dispatch.md`); results +
    any pre-existing workspace failures reported explicitly in the `OD-##` row.

- **T0.5b — Full workspace `/verify` (explicit, separate)** 🟢
  - R-refs: all Phase-0. ⛔ T0.5.
  - Deliverable: the workspace-wide `/verify` gate (`cargo fmt --all --check` +
    `cargo clippy --workspace --all-targets --all-features -D warnings` +
    `cargo test --workspace --all-targets --all-features`), run only when the
    owner wants the full gate. Report existing unrelated failures explicitly
    rather than folding them into this slice's result (AGENTS.md: CI does not
    gate test/clippy).

- **T0.6 — Phase-0 preparation + seal + activation (owner-gated)** 🔒
  - R-refs: R1, R4. ⛔ T0.5b.
  - Note: `opblob seal-shm` reseals the **live** catalog and sweeps plugin IDs
    absent from the build — it is not a per-plugin seal. Do each step
    separately, each owner-approved:
    1. isolated `seal-plugins` preparation into a release catalog
       (`OP_PERSISTED_CATALOG_SOURCE`), using the service schema-shaping env;
    2. catalog-difference review (confirm only the new plugin IDs added, none
       swept) with a recoverable copy kept;
    3. golden preparation (`build-golden.sh`, validate-and-copy, no reseal);
    4. snapshot creation + `btrfs send/receive` + received-release activation
       per `docs/src/getting-started/local-release-activation.md`.
  - Verify: live/persisted catalog hashes match after restart; new read/write
    methods callable; catalog diff shows no unintended sweeps. **Not** bundled
    with T0.5/T0.5b.

---

## Phase 1 — Deterministic gate + tag semantics 🟢→🔒

Goal: a real gate object and a first declared tag vocabulary; still no transform
or network effect.

- **T1.1 — `schema_tag_registry` plugin (declared vocabulary + semantics)**
  - R-refs: R1, R9. ⛔ T0.6.
  - Component: `crates/op-plugins/src/state_plugins/schema_tag_registry.rs`
    (proposed). Tag descriptors with `expected_revision`, optional subid
    linkage, source refs.
  - Deliverable: read methods (`list_tags`, `get_tag`) + a review mutation
    (`register_tag_semantics`, its own capability, `oscal`-style). Semantics are
    data (no behavioral meaning yet).
  - Verify: register/read round-trip; a tag with no gate implementation has no
    runtime effect (negative test).

- **T1.1b — Live policy administration (author/read/update/revoke)**
  - R-refs: R1, R9, R3. ⛔ T1.1.
  - Component: `author_policy`/`get_policy`/`update_policy`/`revoke_policy` on
    the owning plugin. Ownership: `schema_tag_registry` owns **both**
    `tag_constraint` + tag→method associations **and** the proposed
    `deny_method` rules — authored through its admitted, `expected_revision`-
    checked, durably-persisted methods, **not** the internal unversioned
    `store_compliance_rule` DB helper (which stays an implementation detail of
    the cognitive Cozo store and is not the admitted administration surface).
    The gate still evaluates `deny_method` at the same pre-dispatch layer as the
    existing compliance check. `egress_rule` ownership = design U2.
  - Deliverable: live replacement for Phase-0 fixed fixtures; revision checks
    enforced; the explicit "requires policy" marker is read from its declared
    source, and **failure to read the marker produces `hold`** (never a silent
    "unprotected" interpretation).
  - Verify: author→read→update(revision bump)→revoke round-trip for both
    `deny_method` and `tag_constraint`; a stale `expected_revision` is rejected;
    a marked method with revoked policy yields `unresolved`, not allow; a
    marker-read failure yields `hold`; an unmarked method is unaffected.

- **T1.2 — Deterministic gate (full)**
  - R-refs: R3, R2, R10. ⛔ T0.3a, T1.1.
    Pure/shadow evaluation may use fixed captured fixtures. Live policy
    authorization additionally requires T5.2; fixtures never authorize effects.
  - Component: extends the T0.3a evaluator with `tag_constraint` evaluation;
    gate function in `routing_decision` taking `{verified_identity,
    operation_ref, classification_proposal, policy, grants}` → explicit
    `authorized_local | denied | unresolved | hold`. Expresses/evaluates policy
    at the same layer as the existing pre-dispatch compliance check
    (`cognitive_mcp().cozo_shuttle().evaluate_mutation(plugin_id, method)`,
    `mutation_engine.rs:~3568`), so a **direct** call to a protected method hits
    identical enforcement (design §2a).
  - Deliverable: pure, model-free decision; most-restrictive-on-unknown; deny
    wins; grant check reuses `capability-grants` evaluation; a recorded "allow"
    is not reusable authorization. Implements the immediate-before-dispatch
    recheck (R3) for **local** protected operations: re-verify identity, grants,
    schema/catalog hash, and policy revision after any queue wait, aborting to
    `unresolved`/`hold` on change. (Egress's content-byte recheck is T4.4.)
  - Verify: reproducibility test (same inputs → same outcome regardless of
    classifier availability); each failure class (unknown tag, stale hash,
    denied capability, missing-required-policy) yields its explicit outcome with
    no scope change; **non-bypass fixture** — same operation via the gate path
    and via a direct `CallMethod` yields identical admission outcomes;
    **local-operation recheck fixtures** — a policy change, a revoked grant, a
    stale contract hash, and an unavailable policy store each injected during a
    queue wait abort the local dispatch to `unresolved`/`hold`.

- **T1.3 — Receipt correlation index**
  - R-refs: R4, R6.2, R13. ⛔ T0.1.
  - Component: correlation linking `operation_id` + event ids/hashes
    (`MethodExecutionReceipt`) and `observed_state_ref` to a decision record.
  - Deliverable: given a completed admitted operation, the decision record
    resolves to its receipt and the committed SHM observation.
  - Verify: test proves the index points at the receipt store and the committed
    state path, never writing state into the receipt store.

- **T1.4 — Phase-1 validation / seal / activation** 🔒
  - R-refs: Phase-1. ⛔ T1.1–T1.3 and T5.2 for live policy enforcement.
    Distinct validate → seal → activate steps,
    each owner-gated, as in T0.5/T0.6.

---

## Phase 2 — Workflow 2: accountability against independent evidence 🟢→🔒

- **T2.1 — Independent recent-event / receipt / state evidence sources**
  - R-refs: R6.1, R6.2, R6.4. ⛔ T1.3.
  - Component: evidence adapters that query recent events, receipts and
    authoritative SHM; each query **independently authorizes the verified
    caller** against its own access scope and provenance (not a second
    identity); distinguish request / acceptance / SHM publication / native
    readback / failure / cancellation.
  - Deliverable: evidence panel entries each tagged with source identity and
    retrieval time; missing evidence shown as gap.
  - Verify: a claim with no matching receipt reports "no receipt found" (not
    "did not happen"); the recent-event query and the chat session each
    independently authorize the verified caller.

- **T2.2 — NotebookLM / memory provenance access**
  - R-refs: R6.3. ⛔ T2.1.
  - Component: verified named access preserving owner, conversation id,
    capture/retrieval time, revision; kept distinct from SHM/grants/compliance.
  - Verify: unavailable source stays unavailable; provenance fields preserved;
    semantic similarity is not rendered as proof of execution.

- **T2.3 — Phase-2 validation / seal / activation** 🔒 ⛔ T2.1–T2.2.

---

## Phase 3 — Workflow 3: source-backed compliance retrieval 🟢→🔒

- **T3.1 — Sealed `compliance_evidence` read methods (mappings/route/scope)**
  - R-refs: R7.1, R7.2, R7.3, R7.4, R7.5. ⛔ T0.3.
  - Component: `crates/op-plugins/src/state_plugins/compliance_evidence.rs`
    (proposed) wrapping the maintained `compliance-index` package contracts.
    These three contracts **do not perform vector search**: `mappings` returns
    assertions; `route` returns a depth-bounded `{nodes, paths, truncated}`
    discovery tree; `scope` returns `{generation, collection, allowed_groups,
    qdrant_filter}` (eligible groups + filter only). Re-read the live generation
    at integration time (design U6).
  - Deliverable: read methods preserving direction, versions, unresolved refs,
    generation; empty `allowed_groups` → `qdrant_filter: null` → zero results,
    no query; mixed generations rejected; a non-truncated `route` still labelled
    depth-bounded; `scope` carries an explicit completeness limitation because
    it exposes no truncation indicator (`contracts.md:41`).
  - Verify: empty-scope tag issues no Qdrant query; mixed-generation fixture
    fails; a non-truncated route is not presented as complete; mapping shown as
    evidence, not equivalence/enforcement.

- **T3.1b — Admitted bounded compliance vector search + membership validation**
  - R-refs: R7.1, R7.2, R7.3, R11. ⛔ T3.1.
  - Component: the actual search step `scope` does not do — a bounded admitted
    operation that queries the verified live `collection` using exactly
    the `qdrant_filter` returned by `scope`, over its supported Qdrant transport,
    then validates that each returned
    point's group/context-hash and document membership match the active
    generation (`embedding.py:244`, payload shape `embedding.py:192`).
    `compliance_context_v1` and `/run/ghostbridge/qdrant/grpc.sock` are captured
    historical inputs, pending verification after container restoration.
  - Deliverable: bounded (result-count/time) search; rejects any point whose
    membership/generation does not match; never issues an unfiltered query.
  - Verify: a `null` filter path performs no query; a point from a different
    generation or non-member group is rejected; nearest-neighbor documents
    authorize nothing.

- **T3.2 — Phase-3 validation / seal / activation** 🔒 ⛔ T3.1, T3.1b.

---

## Phase 4 — Workflow 1: local transform + residual validation + egress 🟢→🔒

The structured visual demo in T0.3 is the first demonstration priority after
its prerequisites. This phase delivers the complete transform, residual
validation, and separately authorized egress workflow; priority does not waive
the T5 controller/Qdrant prerequisites or grant live-egress activation.

- **T4.1 — Transform method discovery / definition**
  - R-refs: R1.1–R1.7, R5.1, R5.2, R5.8, R8, R9, R14, NG5.
    ⛔ T1.2, T0.3, T0.2c, T0.2d.
  - Component: determine whether a local transform (redaction) is a
    threetched-discovered method (`ThreetchedSurface`), an admitted upstream
    binary method, or a plugin-owned structured overlay (design U5). The
    overlay must use the exact sealed schema, resolved shared OSCAL metadata
    and explicit machine privacy policy. Trace the shared registry/helper path
    before assessing coverage; missing local field annotations are insufficient
    evidence. Pin catalog identity and annotation revision, without introducing
    a second OSCAL authority or copying its metadata into each plugin.
    Reuse T0.2c's backend field selection and T0.2d's candidate/sealing
    validation; do not create a second resolver inside the transform or UI.
    Retain per-field reasons and revisions, distinguish restricted opaque
    boundaries from inspected interiors, and hold unresolved bindings.
    Registry availability alone does not prove effective coverage.
    Resolve execution locality from the verified runtime. The existing
    `ui_render` path is not locality proof: it skips memory but selects the
    ordinary provider/fallback. Demo inputs are fictional or model templates
    without private originals, followed by local data hydration.
  - Deliverable: a transform contract backed by a real, discovered/declared
    method, with unknown/stale paths holding, or an explicit *unavailable*
    runtime-transform capability while the local-only visual overlay is
    demonstrated. No egress authorization is implied.
  - Verify: absent runtime method stays unavailable (no fabrication); provider
    name rejected as locality proof; exact source schema hash/version is
    recorded; private originals are absent from model prompts/history/gallery;
    unknown and stale paths hold; free-text residual validation remains a
    separate T4.3 check.
    Shared classification and first-seal fixtures belong to T0.2b–T0.2d;
    integration proves the transform uses their results without relaxing them.

- **T4.1a — Reviewed field migration, five to ten plugins per batch** 🟢→🔒
  - R-refs: R1.1–R1.8, R5.8, R9, R10. ⛔ T0.2a–T0.2d.
  - Component: apply reviewed shared bindings to the audited schema roots,
    with explicit plugin file ownership where protection-preserving producer
    changes are necessary. Workers report unresolved cases to the contract
    owner before broadening a rule or moving to the next migration batch.
  - Deliverable: effective coverage report under exact candidate/sealed pins,
    shared-rule revision and existing projection policy. Preserve restrictive
    opaque boundaries. Missing live authority remains an unverified gap, not
    a source-only pass. Assess `oscal_subid_registry` itself before activation.
  - Verify: compare producer, adapted and sealed roots; no lost secret/private
    flag; keys and values both covered; stale rules rejected. Enforce coverage
    only for the reviewed activation scope after its checks and recoverable
    catalog preparation. Enforcement activation belongs to T4.6; this task
    prepares migration evidence and does not itself enable the gate. Report
    migrated/verified/deployed counts separately.

- **T4.2 — Bounded transform execution**
  - R-refs: R5.3, R11, R12. ⛔ T4.1.
  - Component: admit under bounded reservation (byte/time/concurrency caps);
    emit transformed output + findings + unresolved spans; preserve originals
    only under owner retention.
  - Verify: limit-exceed returns typed capacity/timeout; cancellation follows
    the four R12 cases — queued cancellation releases the reservation; running
    cleanup retains permits/buffers/locks through cleanup and records
    `uncertain`; a transform completed before cancellation is recorded, not
    reversed; unavailable readback leaves the outcome `uncertain` without blind
    retry.

- **T4.3 — Residual-disclosure validator**
  - R-refs: R5.4, R10. ⛔ T4.2.
  - Component: deterministic detectors + consistency checks over transform
    output; records removal categories + unknown risks without exposing secrets.
  - Verify: a model redaction claim without validation does not pass; confidence
    is never recorded as proof of anonymity.

- **T4.4 — Separate egress authorization (bound to exact content)**
  - R-refs: R5.5, R5.6, R5.7, R10, U2. ⛔ T4.3, T1.2, T5.1, T5.2.
  - Component: egress decision against destination, actual provider route,
    purpose, policy revision, grants (owner decides owning plugin — design U2).
    The approval binds an **immutable** `validated_content_ref` (the exact
    payload that passed T4.3) plus `transform_receipt_ref`,
    `validation_evidence_ref`, `route_revision`, `policy_revision`. Immediately
    before dispatch (after any queue wait) it re-verifies identity, grants,
    route/contract hash, policy revision, and that the bytes to send equal
    `validated_content_ref`.
  - Deliverable: a sending path that refuses to transmit bytes not matching the
    validated artifact; no `anonymous` auto-promotion; no cloud fallback.
  - Verify: successful transform / valid JSON does not grant egress; denied or
    unresolved egress keeps content local with explanation; **payload
    substitution** after approval blocks the send; **route change** after
    approval blocks the send; **grant revocation** after approval blocks the
    send; sending bytes != `validated_content_ref` fails closed.

- **T4.5 — Correlation + masked audit for Workflow 1**
  - R-refs: R5.7, R13. ⛔ T4.4.
  - Component: correlate transform receipt + egress outcome with bounded
    identifiers; mask sensitive fields; scope hash visibility.
  - Verify: originals/removed tokens/detector snippets absent from logs by
    default; durable-audit-required egress refused when audit capacity is
    unavailable (fail closed, pre-dispatch).

- **T4.6 — Phase-4 validation / seal / activation** 🔒 ⛔ T4.1–T4.5, T4.1a, T5.1–T5.2. Activate
  only a narrowly reviewed scenario after receipt correlation, cancellation,
  residual checks and observability are demonstrated.

---

## Phase 5 — Required OVS/OpenFlow integration (activation owner-scoped) 🔒

These task identifiers are retained for traceability. Their prerequisites must
land before live policy/egress activation; this section is not a later optional
network phase.

- **T5.1 — Explicit admitted OVS/OpenFlow adapter (required)**
  - R-refs: R8, NG4. ⛔ T1.2 source evaluator and T5.2.
    Prerequisite of T4.4/T4.6, not dependent on completed egress activation.
  - Component: translate an *authorized application decision* into an existing
    admitted controller method (`openflow`, with `ovsdb_bridge` topology reads) via its exact typed
    members + verified readback; owner-scoped native comparison.
  - Verify: no schema-tag/model-intent reaches a network input; partial
    completion is not treated as a cross-system transaction; readback confirms
    the exact member.
  - Preserve the controller's denial of unattributed mutations. Carry verified
    principal/session and the exact admitted operation to the controller's
    final queue boundary; recheck seal, grants and policy there. A bus UID or
    caller-supplied identity label is insufficient.
  - Resolve an authoritative identity-to-packet binding before installing a
    flow. The reserved controller endpoint is not a dataplane selector. Missing
    attachment/selector evidence holds the operation; never invent VLAN or
    register allocations. Xray is not this feature's controller.

- **T5.2 — Canonical Qdrant mapping prerequisites (required)**
  - R-refs: R7.1a, R8, R9. Depends on T3.1/T3.1b; required by policy admission
    and T5.1, not an optional follow-up.
  - Resolve existing canonical mapping identifiers with their direction,
    generation and source hashes through the admitted compliance evidence
    surface. Use `oscal_subid_registry` to link the exact sealed operation;
    its bindings/annotations do not verify external mapping provenance.
    Persist references only. Reject caller
    supplied resolution claims, mixed generations and unscoped search.
  - Verify live collection/payload contracts before writing an adapter. The
    September captures are provenance evidence, not live endpoint/schema proof.

---

## Cross-cutting tasks

- **TX.1 — Fault-injection suite** (R10, R11, R12): one negative test per
  failure class proving no identity broadening, no cloud disclosure, no blind
  retry. Cancellation is tested as four distinct cases — queued cancellation,
  running cleanup, completed-before-cancellation, and unavailable readback —
  each with its own assertion (per R12). Policy-store failure on a
  policy-required method is tested to yield `unresolved`, not allow. Runs each
  phase.
- **TX.2 — Provenance round-trip** (R9): every decision record preserves and
  distinguishes schema hash vs catalog hash vs taxonomy revision vs policy
  revision; stale associations stay stale.
- **TX.3 — WISHLIST promotion** (R14): open the `OD-##` rows and link this spec;
  record phase status there, not in a new TODO doc.

---

## Dependency summary

```
Phase 0 (T0.0→T0.1→T0.2→T0.3a→T0.3/T0.1b→T0.1c→T0.4→T0.5→T0.5b→🔒T0.6)
   ├→ Phase 1 source/shadow (T1.1→T1.1b, T1.2, T1.3)
   │     ├→ Phase 2 (T2.1→T2.2→🔒T2.3)
   │     └→ Local transform (T4.1→T4.2→T4.3)
   └→ Phase 3 (T3.1→T3.1b→🔒T3.2) → T5.2 canonical live mappings
T1.1–T1.3 + T5.2 → 🔒T1.4 live policy enforcement
T1.2 + T5.2 → T5.1 attributed controller + verified traffic binding
T4.3 + T1.2 + T5.1 + T5.2 → T4.4→T4.5→🔒T4.6 live egress
TX.1–TX.3 run across all phases.
```

Field-contract prerequisites run alongside the initial gate work:
`T0.2a (batch reports) → T0.2b (shared contract) → T0.2c (resolver) → T0.3
(fixture demo)`. T0.2c also depends on T0.0/T0.2. Creation/import/seal
integration is `T0.2b/T0.2c → T0.2d`; reviewed migration is
`T0.2a–T0.2d → T4.1a`. Both `T0.2c` and `T0.2d` also precede `T4.1`,
alongside its `T0.3` and `T1.2` prerequisites. This removes the earlier implicit dependency on T4.1
for the demo's field resolver. Runtime transform and egress remain later,
separately verified work; source review does not activate either.

The first vertical slice is **Phase 0** (concrete limits/audit prerequisite +
bounded shadow-mode decisions persisted through an admitted caller-scoped
writer + a minimal deterministic evaluator + honest evidence UI). T0.3
prioritizes a redaction-first structured visual demo only after its prerequisites
and without live egress activation. Later work still delivers all three full
workflows (R5/R6/R7); none is dropped to a demonstration (R14, NG-section of
requirements).
