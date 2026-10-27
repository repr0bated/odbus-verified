# Declarative JEV receipts — op-cozo-store

Actual tool: eval Python `await judge(state, questions)`, supported syntax from `xd://eval/judge`. No routing/model selection. Evidence supplied as exact source excerpts already read. JEV classifications are source evidence, not mutation authority or runtime proof.

## J1 — native storage and provenance terminology

Evidence: `src/bin/migrate-chatbot-root.rs:69-121` complete `prepare()` body; `src/bin/inventory/mod.rs:1819` complete GraphSnapshot expression. Exact excerpts and complete request retained in session tool transcript, title `Judging native storage terminology`.

Question migration_native: Does native root in prepare refer to storage path terminology or implement implicit provider/model selection? Outcomes: storage_descriptive / implicit_routing_override / insufficient.
Result: storage_descriptive; confidence 1; probabilities {storage_descriptive:1, implicit_routing_override:0, insufficient:0}.
Decision: preserve Incus/native_root storage terminology. The command prints guidance, not a provider/model override.

Question inventory_native: Classify UI/native in GraphProvenance.coverage: descriptive inventory label or executable provider/model override? Outcomes: descriptive / runtime_override / insufficient.
Result: descriptive; confidence 1; probabilities {descriptive:1, runtime_override:0, insufficient:0}.
Decision: preserve inventory coverage string. Graph provenance does not set provider/model selection.

## J2 — native persistence and Incus allocation

Evidence: `src/identity_transactions/native_creation.rs:1-3,130-175` complete reserve function plus module purpose; `src/identity_transactions.rs:432-442` complete checked-commit helpers. Exact excerpts/request retained in session tool transcript, title `Judging native persistence semantics`.

Question creation: Classify native_creation and provider-side maximum: Incus backend naming/fence or implicit AI provider/model override? Outcomes: incus_naming / ai_override / insufficient.
Result: incus_naming; confidence 1; probabilities {incus_naming:1, ai_override:0, insufficient:0}.
Decision: preserve container-native terminology and existing ownership/replay fence. JEV does not establish upstream authentication; storage owner checks are not transport admission.

Question boundary: Does commit_checked's native vocabulary describe backend commit error handling or model routing? Outcomes: commit_errors / ai_routing / insufficient.
Result: commit_errors; confidence 1; probabilities {commit_errors:1, ai_routing:0, insufficient:0}.
Decision: preserve library-native error propagation, no rename or selection change.

## J3 — explicit profile authority and incidental source risks

Evidence: `src/model_registry.rs:645-706` complete profile setter; `src/lib.rs:754-774` complete verdict match; `src/model_registry.rs:406-422` complete write helper; `src/chat_response_conversations.rs:792-814,176-178` complete count function and parameter constructor. Exact excerpts/request retained in session tool transcript, title `Judging selection authority and incidental risks`.

Question profile_authority: Does set_model_registry_profile derive a hidden hard-coded override or persist/clear supplied profile for supplied provider/model? Outcomes: explicit_persistence / implicit_override / insufficient.
Result: explicit_persistence; confidence 1; probabilities {explicit_persistence:1, implicit_override:0, insufficient:0}.
Decision: no native-specific setter patch; it stores caller parameters and has no automatic model/provider/profile derivation. Upstream admission was not claimed.

Question compliance: Is a Cozo query error converted into an allow verdict? Outcomes: fail_open / fail_closed / insufficient.
Result: fail_open; confidence 1; probabilities {fail_open:1, fail_closed:0, insufficient:0}.
Decision: report generic security risk, outside native-only repair scope; no live failure or exploit claim.

Question registry_barrier: Can model_registry_write return Ok(value) after sync_runtime_document_store returned Err? Outcomes: suppressed_error / propagated_error / insufficient.
Result: suppressed_error; confidence 1; probabilities {suppressed_error:1, propagated_error:0, insufficient:0}.
Decision: report generic durability-receipt risk; not an implicit native selection override.

Question conversation_count: Does count_principal_conversations use a parameter name not supplied by principal_params? Outcomes: mismatch / consistent / insufficient.
Result: mismatch; confidence 0.97; probabilities {mismatch:0.98, consistent:0.02, insufficient:0}.
Decision: report exact local contract mismatch ($principal_id versus principal); exact backend failure/invocation remains unverified.

## J4 — incidental maintenance guidance and commit inconsistency

Evidence: `src/identity_transactions.rs:432-442`; `src/lib.rs:1555-1557,1975-1977`; `src/bin/get-migration-preflight.rs:41-45`; `src/bin/validate-chatbot-state.rs:97-104`. Exact excerpts/request retained in session tool transcript, title `Judging maintenance and commit inconsistencies`. Criteria include repository provenance/admission/TLS rules.

Question commit_inconsistency: Do library callers use convenience txn.commit contrary to this crate's checked-commit rationale? Outcomes: inconsistent / consistent / insufficient. Explicitly do not certify unread Cozo dependency behavior.
Result: inconsistent; confidence 0.88; probabilities {inconsistent:0.92, consistent:0.01, insufficient:0.07}.
Decision: report source inconsistency/risk, not a reproduced dependency failure; no independent native routing fix.

Question maintenance_guidance: Does printed advice conflict with provenance/admission/TLS rules without constituting AI selection override? Outcomes: unsafe_historical_advice / implicit_ai_override / compliant / insufficient.
Result: unsafe_historical_advice; confidence 1; probabilities {unsafe_historical_advice:1, implicit_ai_override:0, compliant:0, insufficient:0}.
Decision: flag historical operator advice for coordinator; do not run it or silently expand edit scope.

## Initial-pass receipt summary (not occurrence-complete)

Four successful `judge()` requests, ten typed questions. These original judgments remain evidence but are too narrow to establish every-occurrence completion. The following contextual requests supplement them. No routing extension/model selection.

## Clarified occurrence reconciliation

Ledger: [occurrences.tsv](occurrences.tsv). Paths there are relative to `crates/op-cozo-store/`; line/position identifies the left-to-right native-containing token ordinal, not a byte column. Every row maps to an actual-context judgment below. Matching group evidence included every mapped occurrence, not a representative subset. Complete requests and successful outputs are retained in this same session's eval transcript under the titles below. Each choice included an explicit adverse-change and insufficient-evidence alternative. The tables record returned choices and calibrated confidence, not runtime results.

### C5 — every identity transaction occurrence

Actual request: `JEV C5 every transaction occurrence`. Evidence: complete sequential reread `src/identity_transactions.rs:1-300;301-600;601-900;901-1200;1201-1500;1501-1802`, snapshot `1322`; exact native lines and helper implementation supplied in the request. All 55 occurrences supplied: 43 helper definitions/calls (including two at441), five module/export tokens at7-8, seven comments at81,432,433,818,849,993,1190.

| Question / explicit evaluation | Choice | Confidence |
| --- | --- | --- |
| helpers: naming and accuracy of each underlying Cozo/IO/nix/channel/decoding error conversion, versus misleading native semantics | keep | 0.88 |
| exports: native_creation module and request/record/validator represent backend container allocation, versus inaccurate naming | keep | 0.67 |
| comments: seven actual storage/Incus/nested-commit contexts match intended behavior, versus inaccurate claim | keep | 0.85 |

Decision: preserve each mapped occurrence. Dependency-specific commit rationale is not independently certified dependency/runtime evidence.

### C6 and C6d — every creation journal occurrence

Actual requests: `JEV C6 every native creation occurrence`; `JEV C6d resolving diagnostic uncertainty`. Evidence: complete `src/identity_transactions/native_creation.rs:1-229`, snapshot `87E7`. All 68 case/compound/string occurrences, including multiple tokens per line, were supplied. C6 included request fields, sequence/journal ownership, exact native-containing lines and phase/UUID guards.

| Question / explicit evaluation | Choice | Confidence |
| --- | --- | --- |
| C6.identifiers: all native-bearing names/fields/relations/markers accurately represent Incus creation and binding, versus inaccurate contract | keep | 0.70 |
| C6.diagnostics: all diagnostic strings accurately describe guards, versus misleading failure claims | keep | 0.35 |
| C6d.journal_sequence: exact strings at62,84,88,104,105,109 with individual journal/overflow guards | keep | 0.86 |
| C6d.reservation: exact strings at135,142,152,156,159,166,170 with request, ownership and name-allocation guards | keep | 0.83 |
| C6d.uuid_update: exact strings at182,202,203,205,214,221 with UUID/CAS/transition guards | keep | 0.74 |

C6.diagnostics confidence below0.6 was not accepted as disposition. C6d examined the individual guards and every diagnostic context, replacing that broad result. Decision: preserve identifiers and locally accurate diagnostics; no claim that an external Incus creation actually ran.

### C7 — runtime durability and storage wording

Actual request: `JEV C7 runtime and storage occurrences`; complete output `artifact://3:raw:1-1`. Evidence: complete runtime document source `1-265` (`8D49`), clean provision `1-384` (`888B`), retained physical root `1-357`, complete migration utility `1-150` (`F146`). Request supplied every mapped actual line, complete error-wrapper meaning, formatting boundary, target CAS order, held-fd syncfs behavior and instance comparison logic.

| Question / explicit evaluation | Choice | Confidence |
| --- | --- | --- |
| runtime_helpers: ten helper-definition/call occurrences at96,116,132,149,167,199,209,218,243,257 accurately wrap underlying errors | keep | 0.80 |
| runtime_comments: barrier/privacy/nested-result/native-OS wording at4,11,212,213,252 versus actual boundary semantics | keep | 0.88 |
| clean: decoder helper159, name validator184, mutable power286, pinned backend UUID287 versus closed volatile allowance | keep | 0.77 |
| physical: native UUID26 versus complete residual JSON comparison after allowed power changes | keep | 0.94 |
| migration: all four tokens76,108,109,111 denote Incus filesystem root in printed historical guidance; distinguish terminology from unverified plan provenance | keep_terminology_unverified_plan | 0.98 |

Decision: preserve terminology. Migration examples and leaf field provenance are unverified historical advice, not a supported current plan or deployment receipt. No concrete native-specific correction was established. Existing generic maintenance-risk finding G5 remains separate.

### C8 — remaining library, inventory, graph and fixture contexts

Actual request: `JEV C8 remaining occurrence contexts`; complete output `artifact://4:raw:1-1`. Evidence: retained complete reads of lib, inventory, identity inventory, identity tests, runtime tests, graph source/tests; each native-bearing line supplied, together with actual surrounding algorithm/fixture behavior. This included all three tokens at lib24, both tokens at identity test1446 and the two incidental substrings in alternatives.

| Question / explicit evaluation | Choice | Confidence |
| --- | --- | --- |
| lib: seven export/relation/provision/recursive-rule tokens at24,300,301,539,972 versus backend/Datalog implementation | keep | 0.83 |
| inventory: UI/native artifact label1819 and incidental alternatives789,1417 versus prepared-only conservative inventory | keep | 0.88 |
| inventory_disclaimer: metadata_is_not_native_runtime_state360 versus metadata-only read transaction | keep | 1.00 |
| identity_tests: all eleven native tokens in names, errors, comments and opaque UUID fixture versus actual intended invariants | keep | 0.91 |
| runtime_tests: all nine native tokens in confidentiality and persistence fixtures versus claimed test purpose | keep | 0.52 |
| graph: checked native transaction512 and aborted native-write test172 versus single Cozo namespace transaction | keep | 0.98 |

C8.runtime_tests below0.6 was not accepted as final disposition. The focused reads and requests below resolve it. Graph test is a precommit-abort write-transaction test, not reproduction of a dependency commit error.

### C8r, C8b and C8p — resolve runtime-test uncertainty

Actual requests: `JEV C8r resolving runtime context uncertainty`, `JEV C8b exact native boundary ordering`, `JEV C8p deciding final write occurrence`. Sequential runtime test reread `1-220;221-450` through EOF444, snapshot `BE96`; exact production runtime source171-265 (`8D49`); exact directory guard source200-370;374-417 (`1322`). Original complete-file coverage is retained, not replaced by these narrower rechecks.

| Question / added evidence and evaluation | Choice | Confidence |
| --- | --- | --- |
| C8r.privacy: exact fixture/assertion contexts67,84,93, wrapped diagnostics versus standard formatting boundary | keep | 0.94 |
| C8r.coercion: actual schema changed to Int and typed CAS supplies encoded String; native wording136,139,140 versus real intended Cozo rejection | keep | 0.94 |
| C8r.boundaries: comments/name335,402,433 versus summarized commit/preflight/lifetime behavior | keep | 0.56 |
| C8b.postcommit: exact successful commit_checked before commit_reply/barrier checkpoints; native comment335 | keep | 0.89 |
| C8b.prewrite: exact preflight-before-transaction ordering and malformed-layout fixture; native_write402 | keep | 0.53 |
| C8b.lifetime: exact path restoration before store scope end; native handle433, not synchronous worker-destruction claim | keep | 0.91 |
| C8p.prewrite: actual O_NOFOLLOW/dev/inode/options guards reject each corruption before target CAS transaction/write; distinguish target write from fixture initialization | keep | 0.94 |

Low-confidence C8r.boundaries and C8b.prewrite were not final decisions. Each refinement added exact implementation guards/ordering or separated distinct assertions; no unchanged check was repeated to force an outcome. Final ledger uses only C8r.privacy/coercion, C8b.postcommit/lifetime and C8p.prewrite. Decision: preserve all nine runtime test native terms; tests were read, not run.

## Final receipt summary

12 successful declarative judge requests, 36 typed questions, including eight supplemental requests/26 questions. Four low-confidence broad/intermediate results (C6.diagnostics, C8.runtime_tests, C8r.boundaries, C8b.prewrite) were explicitly resolved with closer evidence. Every final ledger judgment confidence is at least0.67. All 180 ledger rows dispositioned: 178 semantic case/compound native occurrences plus two conservatively included incidental alternatives substrings. No native-specific repair established; no source changes. No build/test/runtime/deployment proof, model routing or source search.
