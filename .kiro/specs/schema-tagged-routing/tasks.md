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

Legend: 🟢 shadow-safe (no grant/flow/proxy change) · 🔒 owner-gated
(seal/activate) · ⛔ depends-on.

---

## Phase 0 — Boundary and honest UI (first vertical slice) 🟢

Goal: a typed, read-only boundary that shows evidence and drafts, changes
nothing, and establishes the shadow-mode substrate. This is the recommended
first slice.

- **T0.1 — Decision-record DTO + private store (`routing_decision` plugin skeleton)**
  - R-refs: R1, R2, R9, R13.
  - Component: `crates/op-plugins/src/state_plugins/routing_decision.rs`
    (proposed), schemars DTOs per design §3.2; private CozoDB store
    (`op-cozo-store`).
  - Deliverable: sealed-shape `PluginSchema` with **read-only** methods
    (`list_decisions`, `get_decision`) requiring a read capability; no mutation
    method yet. Decision record round-trips all identity + operation_ref +
    label-origin fields.
  - Verify: unit test round-trips a record; negative test shows a
    model-proposed `principal` is ignored in favor of the verified session;
    test proves no capability/route effect from any `tag` value.

- **T0.2 — Operation-reference resolver (read-only)**
  - R-refs: R9, R1. ⛔ T0.1.
  - Component: resolver that maps a request to `{plugin_id, method_key,
    transport_name, schema_hash, schema_version, catalog_hash, subid}` from the
    sealed catalog, reusing `oscal_subid_registry.resolve`/`lookup`.
  - Deliverable: resolver returns a fully-pinned `operation_ref` or a `stale`
    marker on hash mismatch.
  - Verify: test with a current hash resolves; test with a mutated hash yields
    `stale`, not a silent re-pin.

- **T0.3 — Honest UI panel (captures vs live; label origin)**
  - R-refs: R7.4, R7.5, R9, R10. ⛔ T0.1.
  - Component: json-render typed page (per json-render skill) rendering decision
    records, compliance **captures** from
    `reports/compliance-scaffold/20260930-discovery/`, and unavailable live
    integration.
  - Deliverable: panel preserves mapping direction, endpoint resolution status,
    generation, truncation; shows label origin; marks captures as captures;
    drafts invoke no mutations.
  - Verify: UI validation rejects mixed generations; empty scope renders zero
    results and issues no query; a reversed traversal stays
    `reverse:maps_to`; a historical ASVS edge stays historical.

- **T0.4 — Shadow-mode comparison (dry run)**
  - R-refs: R3, R14, R11. ⛔ T0.1, T0.2.
  - Component: `routing_decision` dry-run path that records a classification
    proposal (`gemma_brain.classify`, advisory) + a deterministic
    *would-be* gate outcome, with `shadow_mode=true`.
  - Deliverable: for a bounded fixture set, paired (proposal, deterministic
    outcome) records; metrics for false negatives, ambiguity, latency, memory,
    cost placeholders.
  - Verify: assertion that **no** grant, provider selection, flow, or proxy rule
    changes during a dry run; model output containing bypass instructions stays
    data and does not alter the outcome.

- **T0.5 — Phase-0 validation gate** 🟢
  - R-refs: all Phase-0. ⛔ T0.1–T0.4.
  - Deliverable: `/verify` (fmt + clippy + tests) green for the new crate code;
    documented coverage of the negative tests above.
  - Verify: delegated Cargo run; results recorded in the task's `OD-##` row.

- **T0.6 — Phase-0 seal + activation (owner-gated)** 🔒
  - R-refs: R1, R4. ⛔ T0.5.
  - Deliverable (separate steps, each owner-approved): `opblob seal-shm` of the
    new read-only plugin; catalog persist; received-release activation per
    `docs/src/getting-started/local-release-activation.md`.
  - Verify: live/persisted catalog hashes match after restart; read-only
    methods callable; no other plugin IDs swept. **Not** bundled with T0.5.

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

- **T1.2 — Deterministic gate**
  - R-refs: R3, R2, R10. ⛔ T0.2, T1.1.
  - Component: gate function in `routing_decision` taking `{verified_identity,
    operation_ref, classification_proposal, policy, grants}` → explicit
    `authorized_local | denied | unresolved | hold`.
  - Deliverable: pure, model-free decision; most-restrictive-on-unknown; grant
    check reuses `capability-grants` evaluation.
  - Verify: reproducibility test (same inputs → same outcome regardless of
    classifier availability); each failure class (unknown tag, stale hash,
    denied capability) yields its explicit outcome with no scope change.

- **T1.3 — Receipt correlation index**
  - R-refs: R4, R6.2, R13. ⛔ T0.1.
  - Component: correlation linking `operation_id` + event ids/hashes
    (`MethodExecutionReceipt`) and `observed_state_ref` to a decision record.
  - Deliverable: given a completed admitted operation, the decision record
    resolves to its receipt and the committed SHM observation.
  - Verify: test proves the index points at the receipt store and the committed
    state path, never writing state into the receipt store.

- **T1.4 — Phase-1 validation / seal / activation** 🔒
  - R-refs: Phase-1. ⛔ T1.1–T1.3. Distinct validate → seal → activate steps,
    each owner-gated, as in T0.5/T0.6.

---

## Phase 2 — Workflow 2: accountability against independent evidence 🟢→🔒

- **T2.1 — Independent recent-event / receipt / state evidence sources**
  - R-refs: R6.1, R6.2, R6.4. ⛔ T1.3.
  - Component: evidence adapters that query recent events, receipts and
    authoritative SHM under their own identity/scope; distinguish request /
    acceptance / SHM publication / native readback / failure / cancellation.
  - Deliverable: evidence panel entries each tagged with source identity and
    retrieval time; missing evidence shown as gap.
  - Verify: a claim with no matching receipt reports "no receipt found" (not
    "did not happen"); chat session and event query use separate identities.

- **T2.2 — NotebookLM / memory provenance access**
  - R-refs: R6.3. ⛔ T2.1.
  - Component: verified named access preserving owner, conversation id,
    capture/retrieval time, revision; kept distinct from SHM/grants/compliance.
  - Verify: unavailable source stays unavailable; provenance fields preserved;
    semantic similarity is not rendered as proof of execution.

- **T2.3 — Phase-2 validation / seal / activation** 🔒 ⛔ T2.1–T2.2.

---

## Phase 3 — Workflow 3: source-backed compliance retrieval 🟢→🔒

- **T3.1 — Sealed `compliance_evidence` read methods**
  - R-refs: R7.1, R7.2, R7.3, R7.4, R7.5. ⛔ T0.3.
  - Component: `crates/op-plugins/src/state_plugins/compliance_evidence.rs`
    (proposed) wrapping the maintained `compliance-index` package contracts
    (`mappings`/`route`/`scope`); re-read the live generation at integration
    time (design U6).
  - Deliverable: read methods preserving direction, versions, unresolved refs,
    generation; empty scope → zero results, no query; mixed generations
    rejected; truncation labelled.
  - Verify: empty-scope tag issues no Qdrant query; mixed-generation fixture
    fails; nearest-neighbor documents authorize nothing; mapping shown as
    evidence, not equivalence/enforcement.

- **T3.2 — Phase-3 validation / seal / activation** 🔒 ⛔ T3.1.

---

## Phase 4 — Workflow 1: local transform + residual validation + egress 🟢→🔒

Highest-risk workflow; last so correlation, cancellation, residual checks and
observability already exist.

- **T4.1 — Transform method discovery / definition**
  - R-refs: R5.1, R5.2, R8, NG5. ⛔ T1.2.
  - Component: determine whether a local transform (redaction) is a
    threetched-discovered method (`ThreetchedSurface`) or a new bounded in-repo
    method (design U5). Resolve execution locality from the verified runtime.
  - Deliverable: a transform contract backed by a real, discovered/declared
    method — or an explicit *unavailable* capability if none exists.
  - Verify: absent method stays unavailable (no fabrication); provider name
    rejected as locality proof.

- **T4.2 — Bounded transform execution**
  - R-refs: R5.3, R11, R12. ⛔ T4.1.
  - Component: admit under bounded reservation (byte/time/concurrency caps);
    emit transformed output + findings + unresolved spans; preserve originals
    only under owner retention.
  - Verify: limit-exceed returns typed capacity/timeout; cancellation releases
    reservation and discloses nothing.

- **T4.3 — Residual-disclosure validator**
  - R-refs: R5.4, R10. ⛔ T4.2.
  - Component: deterministic detectors + consistency checks over transform
    output; records removal categories + unknown risks without exposing secrets.
  - Verify: a model redaction claim without validation does not pass; confidence
    is never recorded as proof of anonymity.

- **T4.4 — Separate egress authorization**
  - R-refs: R5.5, R5.6, R10, U2. ⛔ T4.3, T1.2.
  - Component: egress decision against destination, actual provider route,
    purpose, policy revision, grants (owner decides owning plugin — design U2).
  - Verify: successful transform / valid JSON does not grant egress; denied or
    unresolved egress keeps content local with explanation; no `anonymous`
    auto-promotion; no cloud fallback.

- **T4.5 — Correlation + masked audit for Workflow 1**
  - R-refs: R5.7, R13. ⛔ T4.4.
  - Component: correlate transform receipt + egress outcome with bounded
    identifiers; mask sensitive fields; scope hash visibility.
  - Verify: originals/removed tokens/detector snippets absent from logs by
    default; durable-audit-required egress refused when audit capacity is
    unavailable (fail closed, pre-dispatch).

- **T4.6 — Phase-4 validation / seal / activation** 🔒 ⛔ T4.1–T4.5. Activate
  only a narrowly reviewed scenario after receipt correlation, cancellation,
  residual checks and observability are demonstrated.

---

## Phase 5 — Network integration (last, owner-scoped) 🔒

- **T5.1 — Explicit admitted network adapter (if/when desired)**
  - R-refs: R8, NG4. ⛔ Phase 4 complete.
  - Component: translate an *authorized application decision* into an existing
    admitted network method (`openflow`/`ovsdb`/`xray`) via its exact typed
    members + verified readback; owner-scoped native comparison.
  - Verify: no schema-tag/model-intent reaches a network input; partial
    completion is not treated as a cross-system transaction; readback confirms
    the exact member.

---

## Cross-cutting tasks

- **TX.1 — Fault-injection suite** (R10, R11, R12): one negative test per
  failure class proving no identity broadening, no cloud disclosure, no blind
  retry. Runs each phase.
- **TX.2 — Provenance round-trip** (R9): every decision record preserves and
  distinguishes schema hash vs catalog hash vs taxonomy revision vs policy
  revision; stale associations stay stale.
- **TX.3 — WISHLIST promotion** (R14): open the `OD-##` rows and link this spec;
  record phase status there, not in a new TODO doc.

---

## Dependency summary

```
Phase 0 (T0.1→T0.2→T0.3/T0.4→T0.5→🔒T0.6)
   └→ Phase 1 (T1.1→T1.2, T1.3→🔒T1.4)
         ├→ Phase 2 (T2.1→T2.2→🔒T2.3)
         ├→ Phase 3 (T3.1→🔒T3.2)
         └→ Phase 4 (T4.1→T4.2→T4.3→T4.4→T4.5→🔒T4.6)
               └→ Phase 5 (🔒T5.1)
TX.1–TX.3 run across all phases.
```

The first vertical slice is **Phase 0** (bounded shadow-mode decisions +
honest evidence UI). Later phases reach all three workflows (R5/R6/R7); none is
dropped to a demonstration (R14, NG-section of requirements).
