# Native contextual audit — op-agents

Task `native-line-audit-20261024`; assigned Codex worker; source scope `crates/op-agents/`.

## Outcome

All 149 supplied inventory entries read sequentially through EOF, including tests, generated templates and documentation. Directory listings reconciled against the inventory; no extra files, nested agent instructions, missing entries, binary files, symlinks, crate-local lockfiles or unreadable entries were encountered. Initial supplied SHA-256 values are recorded in coverage.tsv; they are not newly computed current-state hashes.

Occurrence reconciliation complete under the clarified owner scope: ten native occurrences across seven files, each individually bound to actual contextual JEV evaluation in `occurrences.tsv` and judgments.md J5/J6. Nine retain correct domain/product/platform wording. One source metadata sentence corrected: legacy iOS description no longer claims app building when its implementation only returns advice. No provider/model/config override established. Earlier routing-only conclusions are superseded by this broader wording/intent review. Preserve all other source and OpenCode's separate tched work. Not built, tested or deployed.

## Native references: every occurrence dispositioned

All paths below are relative to `crates/op-agents/`. The authoritative per-occurrence ledger is `occurrences.tsv`: N01–N10 distinguish every case/compound use, including comments, description strings, JSON keys, the React Native product name, and both cloud-native strings. No fixture occurrence was found in the complete sequential reads. J1/J2 are historical routing-only evidence; current wording/intent decisions are J5/J6, not inferred from those earlier judgments.

| Source | Context | Disposition |
| --- | --- | --- |
| `src/agents/architecture/backend_architect.rs:49` | N01; K8s native service discovery recommendation | Accurate Kubernetes option in advice; retain. J5 confidence 0.92. |
| `src/agents/compliance/policy_enforcer.rs:84` | N02; C2P target-provider policy format in external workflow guidance | Retain after closer inspection; explicit workflow template is not an operation receipt. J5 uncertain change judgment 0.30 superseded by J6 retain 0.60 with preserved rationale. |
| `src/agents/mobile/flutter_expert.rs:43` | N03; native features via platform channels | Accurate Flutter advice; retain. J5 confidence 0.95. |
| `src/agents/mobile/ios_developer.rs:1,63` | N04 header topic; N05 capability description | Retain accurate domain header (J6 0.93); correct Build promise to Guidance (J5 change 0.94, J6 exact correction 0.96). |
| `src/agents/mobile/mobile_developer.rs:49-50` | N06 React Native product name; N07 native platform grouping | Both individually judged and retained: J6 product name 0.93, J5 grouping 0.92. General architecture considerations do not implement or authorize a non-Svelte application UI. |
| `src/agents/mobile/mod.rs:4` | N08; native iOS/Swift topic in export documentation | Retain accurate subject-domain label, not an independent build claim. J5 confidence 0.77. |
| `src/unified/persona/operations_experts.rs:16,18` | N09 cloud-native description; N10 cloud-native role prompt | Both individually judged and retained: J5 confidence 1.00 / 0.89. Metadata identifies Kubernetes-oriented expertise; direct persona execution remains fail-closed. |

All other files remain accounted for by complete sequential reads in coverage.tsv; no other native occurrence was encountered. Only the warranted wording correction below was applied; no blanket native rename.

## R1 — correct the native iOS capability description

`src/agents/mobile/ios_developer.rs:63` originally returned `Build native iOS apps with Swift and SwiftUI.` from `description()`. Lines21-51 only select recommendations and serialize advice/framework names; lines80-88 return that advice for every operation. The native platform terminology is accurate, but the capability sentence promised app construction absent from the implementation.

Applied only line63: `Guidance for native iOS apps using Swift and SwiftUI.` No method/operation/schema shape, security profile, task validation, admission or execution logic changed. Header/topic wording remains accurate and unchanged. No generic stub completion or unrelated description cleanup.

Before mutation, full source reread matched snapshot `B9EC`; exact 3303-byte copy saved as `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-agents/ios_developer.rs.BEFORE`, also snapshot `B9EC`. Applied surgical edit receipt `2AFF` shows the new sentence. J6 approves that exact narrow correction at confidence 0.96; J7 supports the applied metadata claim at confidence 0.86. No concurrent content mismatch or tched/cross-crate overlap. Source-only correction; no build/test/deployment claim.

## Coordinator observation A1 — unconditional model metadata, not proven live routing

**Confirmed source behavior:** `src/bin/gen-agent-defs.rs:22-23,99-111` always emits `model: sonnet` into generated definitions under `.agents/generated`, with no caller-selected model input. `src/generator/md_parser.rs:75-86,111-121` fills an omitted frontmatter model with `sonnet`. This metadata is not merely a comment.

**Limit:** the read local consumers do not use that field to select a model: `src/dynamic/pieces.rs:178-270` ignores `definition.model` when extracting pieces; `src/generator/template.rs:62-93` ignores it when constructing code templates. `src/unified/persona/base.rs:167-186` does not create a model client and returns the authenticated-bridge requirement. External harness consumers and external `AgentDefinition.model` callsites were not traced outside this assignment; do not describe a live model override as proved. `src/lib.rs:12` declares the generator module, so the binary header's claim that it never compiles is stale, not a reason to skip the parser.

**JEV J3:** `metadata_only_proven`, `not_established` for local execution model selection, `coordinator_item`; confidence 1 for all three questions. Exact criteria and receipts in judgments.md.

**Disposition:** no native connection established; broader model-metadata/caller cutover is outside independent native-specific edit ownership. No producer, parser or generated artifact changed here.

**Concrete proposed reconciliation hunks (not applied):**

1. `src/bin/gen-agent-defs.rs`: remove `const DEFAULT_MODEL: &str = "sonnet";` and the unconditional `model: {DEFAULT_MODEL}` output line. Update its frontmatter/layout documentation to stop promising implicit model selection. Generated definitions should leave model selection to the caller's authoritative selection unless a supported explicit model contract is supplied.
2. `src/generator/md_parser.rs`: preserve explicitly authored model values, but represent omitted frontmatter model as absent rather than invoking `default_model()`. A straightforward candidate is `Option<String>` with `#[serde(default)]` for both parsed frontmatter and `AgentDefinition`, deleting `default_model`; reconcile all external field consumers before applying that API change.
3. Coordinator owns regeneration/removal of stale model lines under `.agents/generated` and any external harness/client consumer adjustments. Those paths are outside this crate's source/edit scope. This is a proposed contract, not an approved or applied cutover, and does not authorize a new model dispatcher or model-family mapping.

No tched/threetched hunks or cross-crate fixes proposed by this worker.

## Admission and authority context

`src/lib.rs:37-56` resolves explicit agent IDs through `GLOBAL_REGISTRY`; `src/unified/registry.rs:24-45,53-83` registers factories and resolves IDs, not model/provider selection. Legacy per-agent D-Bus execution is rejected before dispatch (`src/dbus_service.rs:268-275,304-305,342-348`). Unified execution requires root D-Bus sender credentials, nonempty actor/session/genesis attribution, an allowlisted execution-agent ID and a supported operation (`src/dbus_service.rs:106-173`). These source checks are retained; they do not prove external bridge admission, production bus policy, exact grants or deployment. No general authorization rewrite was added to a native vocabulary audit.

## Deferred as outside native-specific repair scope

Existing general-purpose CLI execution, storage/SQLite debt, canned advice, framework/service-manager recommendations, sandbox boundaries and workflow correctness are not silently certified by this report. They were read for context; no unrelated cleanup, new infrastructure or permission change was performed. A1 is singled out because it is concrete hardcoded model metadata relevant to coordinator reconciliation, without conflating it with the native references.

## Verification and limits

- Evidence: original complete sequential reads for all 149 files; seven full sequential occurrence rereads; complete directory listings; J5 evaluated ten exact occurrence contexts; J6 reassessed three uncertain contexts and the exact warranted correction; J7 checked the applied metadata claim. Earlier J1–J4 receipts remain historical rather than being promoted to the clarified scope.
- No searches, recursive delegation, arbitrary eval execution, Cargo, tests, lint, formatting, custom smoke fixture, application launch, deployment, seal, service action, grant/credential changes, staging or commits.
- Only descriptive metadata changed; no execution/admission behavior changed. No runtime validation claimed. Builds and owner-requested tests remain with the primary after reconciliation.
- Current commit was not independently queried; HANDOFF.md's historical commit is not promoted to a current-HEAD claim. Supplied initial hashes are provenance receipts only.

## Self-evaluation

| Axis | Score | Evidence / limitation |
| --- | --- | --- |
| Accuracy | 4/5 | Complete read receipts and typed JEV evidence; no runtime model-selection claim from unknown external consumers. |
| Completeness | 4/5 | All 149 files and visible directories covered; cross-scope A1 consumers explicitly untraced rather than silently assumed. |
| Clarity | 4/5 | Native vocabulary disposition separated from confirmed model metadata and proposed changes. |
| Actionability | 4/5 | A1 names exact producer/parser hunks and out-of-scope artifact/caller ownership; external model metadata consumer contract remains unknown. |
| Conciseness | 4/5 | Individual occurrence/coverage/judgment receipts are required by assignment; only one warranted metadata line changed. |

Overall 4.0/5. Clarified occurrence review is complete: ten dispositioned, nine retained, one corrected, no unresolved native-specific needed change. A1 remains a separate non-native coordinator observation, not a claimed live route override. Source metadata correction is not built/tested/deployed.
