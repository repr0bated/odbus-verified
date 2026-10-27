# op-cozo-store — native contextual audit

Task: `native-line-audit-20261024/op-cozo-store`
Auditor: Codex crate worker; date: 2026-10-24

## Outcome

Complete static review of all 29 inventory files: 18,952 supplied newline-counted lines, sequential reads through each EOF, including tests. Clarified every-occurrence review is complete: 178 semantic case/compound native occurrences plus two conservatively included incidental substrings in alternatives, each individually dispositioned in [occurrences.tsv](occurrences.tsv). No warranted native-specific change established; no crate source edits. Preserve accurate storage/Incus/Cozo vocabulary, not merely absence of model-routing overrides.

Evidence: [coverage.tsv](coverage.tsv) retains original complete-file reads; [judgments.md](judgments.md) records 12 successful declarative requests/36 typed questions, including eight supplemental actual-context requests. Four low-confidence intermediate results were resolved using closer source evidence. JEV is contextual evidence, not runtime proof.

## Every-occurrence accounting

Ledger paths are crate-relative; line/position uses left-to-right native-bearing token ordinal so multiple tokens per line are distinct. Every row includes actual context, meaning, judgment reference/result/confidence, decision and status. Shared judgment groups received all their actual contexts.

| File | Ledger rows | Final contextual judgments |
| --- | ---: | --- |
| src/identity_transactions.rs | 55 | C5.helpers/exports/comments |
| src/identity_transactions/native_creation.rs | 68 | C6.identifiers; C6d diagnostic groups |
| src/bin/migrate-chatbot-root.rs | 4 | C7.migration |
| src/runtime_document_transactions.rs | 15 | C7.runtime_helpers/runtime_comments |
| src/identity_transactions/clean_provision.rs | 4 | C7.clean |
| src/identity_transactions/physical_root.rs | 1 | C7.physical |
| src/lib.rs | 7 | C8.lib |
| src/identity_inventory.rs | 1 | C8.inventory_disclaimer |
| src/system_graph.rs | 1 | C8.graph |
| src/identity_transactions_tests.rs | 11 | C8.identity_tests |
| src/runtime_document_transactions_tests.rs | 9 | C8r.privacy/coercion; C8b.postcommit/lifetime; C8p.prewrite |
| src/system_graph_tests.rs | 1 | C8.graph |
| src/bin/inventory/mod.rs | 3 | C8.inventory; two alternatives substrings are incidental, not semantic native |
| Total | 180 | All unchanged/dispositioned |

No native occurrences in the other 16 fully read files: Cargo.toml; both examples; src/bin/get-migration-preflight.rs, identity_sled_bind.rs, inventory/tests.rs, list-genesis.rs, system_graph_inventory.rs, update-chatbot-fsid.rs, validate-chatbot-state.rs; src/chat_response_conversations.rs, ctl_plane_chatbot_episodes.rs, external_mcp_sessions.rs, model_registry.rs, ui_gallery.rs, workflow_executions.rs. Their exact full sequential coverage remains in coverage.tsv.

No coordinator-followup.md was present in the complete assignment-directory listing. No needed independent native correction, tched overlap or cross-crate native change remains unresolved. The printed migration example's current leaf provenance and deployed root remain unverified: preserving correct native terminology does not endorse historical operator guidance. G5 remains a separate generic maintenance risk.

Original representative/routing judgments below remain historical evidence; the complete per-occurrence ledger and supplemental C5–C8p judgments supersede their narrow completeness claim.

## Native context dispositions

| Context | Source evidence | Disposition |
| --- | --- | --- |
| Incus native root path in printed migration guidance | `src/bin/migrate-chatbot-root.rs:69-121` | Filesystem terminology, not AI model/config selection. Preserve. J1 confidence 1. |
| UI/native artifact inventory label | `src/bin/inventory/mod.rs:1819`; complete file reviewed through 2095 | Descriptive provenance metadata. Offline inventory derives paths/dependencies from explicitly supplied prepared bytes, not deployed state. Preserve. J1 confidence 1. |
| Native runtime state disclaimer | `src/identity_inventory.rs:360` | Explicit limitation of metadata inventory, not runtime authority. Preserve. |
| Native error helper, checked commit result, repair/provision effects and receipts | `src/identity_transactions.rs:81,222-224,432-442,816-818,849-850,993-1012,1182-1229`; complete file through 1802 | Database/library errors and fenced storage/Incus operations. Do not weaken checked commits, revision checks, ownership or revocation fences. J2 checked-commit classification confidence 1. |
| Native container name/UUID and provider-side naming | Complete `src/identity_transactions/native_creation.rs:1-229` | Incus backend allocation, durable POST fencing, exact operation ownership and UUID binding. Provider means container backend here, not AI provider. Preserve. J2 confidence 1. |
| Native UUID/power/config security observations | `src/identity_transactions/clean_provision.rs:177-191,286-350`; `physical_root.rs:25-58` | Native container identity and allowed power observations. No AI override; preserve. Singleton tched labels are not independently edited. |
| Native recursive rules | `src/lib.rs:972-991` | Cozo Datalog traversal vocabulary. Preserve. |
| Native database/diagnostic/barrier | Complete `src/runtime_document_transactions.rs:1-265`; `src/system_graph.rs:512-637` | Database commit results and filesystem durability barriers. Typed CAS/import is not a second plugin schema or model selector. Preserve. |
| Native failure/security/store fixtures | Complete `src/identity_transactions_tests.rs`, `src/runtime_document_transactions_tests.rs`, `src/system_graph_tests.rs` | Library/storage semantics in tests; read only, not executed. Preserve. |
| Model registry/profile persistence | Complete `src/model_registry.rs:1-922`, specifically 437-601 and 645-706 | Retains supplied availability/profile values; setter explicitly stores/clears supplied profile for supplied provider/model. No hard-coded AI provider/profile derivation or automatic execution selector in this owner. J3 confidence 1. Upstream admission is not established by a storage API. |
| Episode embedding metadata | `src/ctl_plane_chatbot_episodes.rs:24-32,82-98,804-851` | Provider/model/config revision are supplied metadata on a scoped embedding CAS, not a default provider/model selector. Preserve. |

All inventory files are accounted for above and in coverage.tsv. Conversation, episode, gallery and workflow records retain existing principal/session/genesis ownership contracts. This source review does not independently certify transport admission in callers outside the assigned crate.

## Incidental generic risks — outside native-only edit ownership

These were encountered during full reads, not searched for. No runtime reproduction, current deployment claim or repair from this worker. They do not justify broadening the clarified native-only repair assignment.

### G1 — Compliance evaluation fails open on query error

`src/lib.rs:724-774`, specifically `Err(error) => PolicyVerdict { allow: true, ... }` at 770-773. Query failure becomes an allow verdict instead of a denial/error. Consumer impact depends on the caller actually using this verdict; no caller/deployment trace claimed. J3: fail_open, confidence 1.

Coordinator action: separately scope fail-closed error semantics and caller handling; keep missing deny matches distinct from failed evaluation.

### G2 — Model registry suppresses persistence failure

`src/model_registry.rs:406-422`: after checked commit, `sync_runtime_document_store()` errors are logged and execution returns `Ok(value)`. Success therefore does not establish durable registry/profile persistence, unlike adjacent typed owners that propagate/report uncertainty. J3: suppressed_error, confidence 1.

Coordinator action: separately scope a truthful post-commit persistence outcome; never imply rollback or replay blindly after an ambiguous commit.

### G3 — Conversation-count parameter/relation contract differs

`src/chat_response_conversations.rs:792-814` references `$principal_id`; `principal_params` at 176-178 supplies only `principal`. The query also names `chat_conversation`, while this owner initializes `chat_response_conversations` at 496-522. The parameter mismatch is directly established; exact Cozo syntax error and caller reachability remain unverified. J3: mismatch, confidence 0.97.

Coordinator action: separately reconcile this count API with the existing scoped count/relation contract; no new storage authority.

### G4 — Remaining convenience commits contradict checked-commit rationale

`src/lib.rs:1555-1557` (`put_wg_session`) and 1975-1977 (`put_human_principal`) call `txn.commit()`. `src/identity_transactions.rs:432-442` documents that Cozo 0.7.6 convenience commit ignores an inner native failure and provides `commit_checked`. Source inconsistency established; dependency implementation was not opened and an actual failed commit was not reproduced. J4: inconsistent, confidence 0.88.

Coordinator action: separately inspect the actual dependency behavior and reconcile these identity-bearing writers with existing checked-commit semantics. This is not an AI-routing override.

### G5 — Historical maintenance advice conflicts with current boundaries

`src/bin/get-migration-preflight.rs:41-45` advises manual migration without bridge, mutation_index as expected_revision, and placeholder hashes. `src/bin/validate-chatbot-state.rs:97-104` prints direct restart advice and plaintext grpcurl against the TLS door. Other maintenance commands contain fixed historical session/device bindings; they were not executed. These files are historical tooling, not authority to bypass identity/provenance/TLS requirements. J4: unsafe_historical_advice, confidence 1.

Coordinator action: separately retire or correct the maintenance commands under their actual supported operation contract. Do not run printed instructions as an audit probe.

## Coverage reconciliation and limitations

Directory reads: crate root, complete `src/`, `src/bin/inventory/`; root/child listings displayed `examples/`, `src/bin/`, `src/identity_transactions/` completely. They agree with the 29-entry inventory. No additional nested guidance, unread binary, missing file, symlink or generated/lockfile entry was encountered in this crate. All supplied entries were text and read in full, including final lines.

Mandatory 28 skill/reference files read completely (truncated default reads continued explicitly), plus using-superpowers skill URI. Root AGENTS was supplied in session context; shared HANDOFF and recent SIGNALS were read. No new source schemas or configuration copies were created. Shared handoff bookkeeping follows the higher-priority session contract; other agents' claimed source was untouched.

Initial hashes/line counts are supplied pre-audit inventory receipts, not current hash verification or an immutable snapshot. No searches, filesystem extraction, commands, Git mutation, delegation, Cargo/build/test/lint/format/smoke fixtures, runtime/deploy/seal/grant/credential actions. No built, tested, installed or deployed outcome claimed. No source change requires a build from this worker; coordinator owns reconciliation and any subsequently authorized checks.
