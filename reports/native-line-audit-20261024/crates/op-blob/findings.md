# op-blob — native contextual audit

Task `native-line-audit-20261024`; 2026-10-24; Codex crate worker.

## Outcome

Complete static review of all **19 inventory files**, including Cargo.lock, docs, unit tests and integration fixtures. **5,146 supplied newline-counted lines** covered by sequential reads through EOF; no pending files. Directory listings of crate root, src and tests show no extra entries or nested guidance. No binary/missing/symlink dispositions needed for the displayed inventory.

**No independent native-specific defect found. No crate source edits.** The clarified review covers every native occurrence's wording, intent and claimed behavior—not just provider/model routing. It is not a general security/quality certification or a claim that every upstream/downstream caller is correct.

**Occurrence reconciliation complete:** 18 case-insensitive native substrings on 17 lines in 5 files, including 7 literal `native` uses and 11 incidental substrings in `alternatives`. [occurrences.tsv](occurrences.tsv) records every position separately. The two tokens at descriptor.rs:224 are separate rows. The remaining 14 inventory files have zero occurrences under their retained full-file coverage. No searches or source extraction used.

Earlier full-file reads remain evidence. The five occurrence-bearing files were reread sequentially through EOF; `coverage.tsv` preserves old ranges and separately identifies these rereads. No coordinator-followup.md was present in the complete assignment-directory listing.

## Retained native contexts

| Source | Context | Disposition |
| --- | --- | --- |
| `README.md:43` | Btrfs image mounted read-only, appearing “native local” | Accurate description of ordinary mounted files, not a claim of existing deployment or plugin-operation exposure. Preserve. |
| `src/adapter.rs:128,133,225,230` | Four `alternatives` identifiers | Binding/reference names accurately denote JSON Schema union branches, including constraint-branch selection. Incidental substring; preserve. |
| `src/descriptor.rs:4` | Typed fields retain native types from MethodDecl args/returns | Contextual protobuf typing, not original Rust ABI or arbitrary JSON numeric fidelity; dynamic/bounded fallback limits explicitly considered. Preserve. |
| `src/descriptor.rs:220,224#1,224#2,226` | Four `alternatives` identifiers | Branch array, closure parameter, length-check receiver and single-branch reference. All actual contexts judged; preserve. |
| `src/json_schema_proto.rs:24,31,34` | Three `alternatives` identifiers | Schema branch-array naming accurately reflects wire-type selection. Incidental substring; preserve. |
| `tests/exact_field_contract.rs:18,51,57,58,60` | Five `native` field/key/pointer positions | Coherent internal exact-source-schema fixture declaration, decoded/export lookups, rebased ref and input key. Preserve. Test read, not executed. |

Original routing-only judgments did not satisfy the clarification alone. New batch `jdgb-15b3279b8f2b2faa` evaluated seven exact context states covering all18 positions for **semantic accuracy and intent**, not routing alone. Initial accuracy confidence ranged0.27–0.74; low-confidence cases were re-examined and received four successful `jev_verify` refinements: mounted-file wording supports0.92; typed-protocol wording supports0.89; all five fixture positions supports0.70; five union identifiers supports0.76. Remaining exact function-body judgments support preserve0.69/0.74. Full evidence, questions, probabilities, uncertainty and decisions: [judgments.md#occurrence-reconciliation](judgments.md#occurrence-reconciliation).

## Override and authority evaluation

- `src/adapter.rs:267-291`: `apply_state_defaults` overwrites **FieldSchema.default metadata**, using the caller-supplied typed default state. No live provider/model/config write. JEV: `schema_metadata_only`, confidence 1.00.
- `src/lib.rs:79-100`: full workspace PluginSchema is canonicalized and sealed; reduced descriptor view derives from that same value. No parallel model/provider contract. JEV: `supplied_schema_authority`, confidence 1.00.
- `src/blob.rs:179-251`: manifest methods, capability labels and descriptor names derive from supplied schema declarations. This is contract publication, not runtime admission or a capability grant.
- `src/catalog.rs:23,359-403`: default SHM location is the canonical blob storage boundary; single-plugin contract reader uses the sealer-owned manifest, validates plugin/hash/type/version and rejects manifest changes. Not a model/config override. Existing concurrent edits in this file preserved; supplied initial hash is not a fresh hash verification.
- `src/identity.rs:18-125`: public publisher WG metadata is distinct from selecting a provider/model. No identity/key/grant changes made; no claim about downstream credential admission.

No native-specific hunk requiring tched/OpenCode reconciliation was identified. Existing threetched CLI field-alias fixture in `descriptor.rs:871-919` was read and left unchanged; it tests generic descriptor transport of authored keys, not a routing policy.

## Coverage and verification

- [coverage.tsv](coverage.tsv): all19 inventory entries, actual sequential read ranges, supplied initial SHA-256, per-file native-substring counts and complete status. [occurrences.tsv](occurrences.tsv): N01–N18 exact line/position/context/meaning/JEV/decision/status.
- Successful exhaustive batch `jdgb-15b3279b8f2b2faa` (7/7) and four semantic `jev_verify` refinements supplement the earlier routing-focused receipts. Initial prior-session Python await misuse and recovered completed batch remain documented.
- No searches, recursive delegation, source filtering, model routing, Cargo, tests, formatting, lint, custom smoke fixtures, runtime operations, deployment/sealing, grant/credential changes, staging or commits.
- No behavior changed; no runtime verification claimed. Build/test/deployment coordination remains with the primary under the assignment rules.
- Shared HANDOFF bookkeeping occurred in the earlier pass under higher-priority session instructions. **No shared HANDOFF/SIGNALS edits in this occurrence-reconciliation continuation.** No other agent's claim or source overwritten.

## Outside native scope, intentionally unchanged

README's historical standalone-workspace framing and test-count claims are separate from the native-local mount wording. The actual native phrase was judged for semantic accuracy against the image/mount/catalog implementation; unrelated historical documentation was not rewritten. No broader general cleanup or security refactor attempted.

## Next action

Coordinator: reconcile this complete **18-occurrence, no-source-change** result with other crate audits. Every occurrence is dispositioned; no op-blob native patch, unresolved overlap or follow-up source read remains.

## Self-evaluation

| Axis | Score | Evidence / limit |
| --- | ---: | --- |
| Accuracy | 4/5 | Grounded full-file ranges and actual JEV receipts; runtime deliberately unobserved. |
| Completeness | 4/5 | All19 files and all18 exact positions dispositioned; earlier routing-only gap corrected without inventing new coverage. |
| Clarity | 4/5 | Native semantics separated from substring-only union names; initial low confidence and refinements retained explicitly. |
| Actionability | 4/5 | No patch required; coordinator has explicit coverage and result paths. |
| Conciseness | 4/5 | Findings bounded; judgment receipt retains repetition needed for auditability. |

Overall 4.0/5. Improvement: retain source-snapshot/hash validation in coordinator reconciliation, rather than treating initial inventory hashes as a freshness guarantee. Expected user assessment: full assigned static scope complete; no build/test/deploy claim.
