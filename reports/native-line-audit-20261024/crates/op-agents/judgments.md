# Declarative JEV receipts — op-agents

Only already-read source evidence supplied; no routing/reranking or source skipping.

## J1: descriptive native vocabulary

Successful batch job: `jdgb-15b31e22beaafe8f`, TypeSafe `typesafe/jev-1.14.0`, 2/2 results. Eval transcript title: Judging native advice strings.

Question: Determine whether the native occurrence affects runtime provider/model/config selection or merely describes domain vocabulary in returned advice. Do not treat quoted workflow prose as executed code.

Explicit choices: `descriptive_only` = serialized advice string data without evidenced runtime selection override; `runtime_override` = native causes authoritative provider/model/config selection replacement without explicit admitted request; `insufficient_evidence` = cannot distinguish data from executable override.

| Evidence | Exact native text and context | Answer | Confidence / probabilities | Decision |
| --- | --- | --- | --- | --- |
| `crates/op-agents/src/agents/architecture/backend_architect.rs:29-128,164-185` | `recommendations.push("Implement service discovery (Consul, etcd, or K8s native)");` under service/microservice keyword advice; vectors serialized into returned JSON, execute returns TaskResult | descriptive_only | confidence 1; descriptive_only=1, runtime_override=0, insufficient_evidence=0 | Retain legitimate Kubernetes terminology; no runtime provider/config selection in this file. |
| `crates/op-agents/src/agents/compliance/policy_enforcer.rs:25-117,146-154` | `"2. C2P generates native policy for each provider"` is one string in `c2p_bridge` JSON workflow; all arms return JSON guidance, execute wraps serialized result | descriptive_only | confidence 1; descriptive_only=1, runtime_override=0, insufficient_evidence=0 | Retain domain-specific policy vocabulary; not an executing provider override or evidence of actual C2P deployment. |

First attempt created batch `jdgb-15b31e15e06afe8e` but Python incorrectly awaited synchronous `judge_batch`, returning `TypeError: object JudgmentBatch can't be used in 'await' expression`. No result from that attempt is used as evidence. Corrected call used synchronous batch construction and awaited `drain`; actual successful typed answers shown above. Both completion notices reported 2/2 judged. This was caller syntax correction, not an unavailable generic judge.

Static judgments do not establish build/test/runtime/deployment status.

## J2: mobile platform and cloud-native vocabulary

Successful batch job: `jdgb-15b31f16e9eafe90`, TypeSafe `typesafe/jev-1.14.0`, 4/4 results. Eval transcript: Judging remaining native vocabulary. Full results recovered from `artifact://2:raw:2-2` after display truncation.

Question: Judge the native text's role from the exact quoted source. Does it replace runtime model/provider/config selection, or name platform/domain concepts only? Preserve legitimate framework/product/domain vocabulary; do not treat advice as executing configuration.

Choices: `domain_vocabulary` = platform/product/domain metadata or advice, without implicit selection override; `runtime_override` = implicit runtime model/provider/config replacement; `insufficient_evidence` = cannot distinguish.

| State / source | Exact native evidence and executing context | Answer | Confidence / probabilities | Decision |
| --- | --- | --- | --- | --- |
| flutter; `src/agents/mobile/flutter_expert.rs:21-51,80-88` | `recommendations.push("Use platform channels for native features");` serialized into analysis JSON; execute returns that string | domain_vocabulary | confidence 0.98; domain_vocabulary=0.99, runtime_override=0.01, insufficient_evidence=0 | Retain Flutter platform-channel terminology. |
| ios; `src/agents/mobile/ios_developer.rs:1-89`, `src/agents/mobile/mod.rs:1-13` | `Native iOS/Swift development` in comments, `Build native iOS apps with Swift and SwiftUI.` in description; analyze emits advice JSON, no provider calls | domain_vocabulary | confidence 1; domain_vocabulary=1, other choices=0 | Retain platform-specific iOS naming. |
| mobile; `src/agents/mobile/mobile_developer.rs:21-54,83-91` | `"cross_platform": ["Flutter", "React Native", "Kotlin Multiplatform"]`, `"native": ["Swift/SwiftUI (iOS)", "Kotlin/Jetpack Compose (Android)"]` in serialized considerations object | domain_vocabulary | confidence 1; domain_vocabulary=1, other choices=0 | Retain product name and platform grouping; not a provider config. |
| cloud_native; `src/unified/persona/operations_experts.rs:10-22`, `src/unified/persona/base.rs:42-61,167-186` | `cloud-native patterns` in Kubernetes description and system prompt; constructor copies metadata, persona execute returns failure requiring authenticated bridge consultation | domain_vocabulary | confidence 1; domain_vocabulary=1, other choices=0 | Retain cloud-native domain terminology; no local model selection. |

Paths in this table are relative to `crates/op-agents/`.

## J3: separate hardcoded model metadata concern

Successful installed `judge()` call; Eval transcript: Judging hardcoded model metadata. Exact source evidence supplied from:

- `src/bin/gen-agent-defs.rs:22-35,99-111`: `const DEFAULT_MODEL: &str = "sonnet";`, unconditional `out.push_str(&format!("model: {DEFAULT_MODEL}\n"));`, no model input in main/render.
- `src/generator/md_parser.rs:15-25,75-86,100-121`: `pub model: String`, `#[serde(default = "default_model")] model: String`, `fn default_model() -> String { "sonnet".to_string() }`, returned `model: frontmatter.model`.
- `src/dynamic/pieces.rs:178-270`: parsing calls `add_from_definition`; name/description/purpose/traits/knowledge/examples/capabilities become pieces, not model.
- `src/generator/template.rs:62-93`: template uses name, description, category, detected operations; not model.
- `src/lib.rs:6-15`: generator is compiled (`pub mod generator`), despite contrary historical generator-binary comment.
- `src/unified/persona/base.rs:167-186`: local persona execution fails closed, requiring authenticated bridge consultation.

External harness consumers of `.agents/generated` and external accesses to `AgentDefinition.model` are explicitly unknown. Assignment edit scope is independent native-specific crate fixes; generated artifacts outside the crate and broader caller migrations require coordinator reconciliation.

| Question | Explicit contrary choices | Answer | Confidence / probabilities |
| --- | --- | --- | --- |
| Does generator/parser evidence prove unconditional authored model metadata, a live authoritative route override, neither, or insufficient evidence? Do not infer an external consumer. | metadata_only_proven / live_override_proven / no_default / insufficient_evidence | metadata_only_proven | confidence 1; chosen=1, all others=0 |
| Do the shown local piece/template/persona consumers establish that parsed model metadata selects the completion model? | not_established / established / insufficient_evidence | not_established | confidence 1; chosen=1, all others=0 |
| Given native-specific edit scope, is this an independently evidenced native-specific repair, broader metadata/caller reconciliation, or insufficient? Judgment is not permission. | coordinator_item / native_fix / insufficient_evidence | coordinator_item | confidence 1; chosen=1, all others=0 |

Decision: record the hardcoded metadata concern with proposed producer/parser hunks in findings.md; do not claim live routing impact or mutate broader/generated surfaces. No native-specific independent defect established.

## J4: completion-claim boundary

Successful `jev_verify` call, transcript intent `Verifying bounded audit conclusion`.

Claim: complete static file coverage and no established independent native-specific routing defect; hardcoded sonnet metadata separately assigned to coordinator reconciliation; no built/tested/deployed or proven live routing-override claim.

Evidence supplied: the session's 149 sequential EOF read receipts, 149 individual ledger rows, full directory reconciliation, J1/J2 domain-vocabulary results, exact A1 generator/parser source, J3 local-consumer/scope results, no source changes or execution, and explicit unknown external consumers.

Result: `verdict=supports`, `confidence=0.97`, `misleading=0.1/3`. Tool also returned `verbatim=0.09`; no verbatim quote was requested (`quote=""`), so that auxiliary score is not used. This verifies claim/evidence consistency, not independent runtime correctness.

## J5: every-occurrence wording and intent

Owner clarification supersedes the earlier routing-only completion boundary. Prior 149-file sequential reads remain coverage evidence; all seven files containing the ten occurrences were reread completely and sequentially. `occurrences.tsv` binds each actual line/occurrence to the states below. No source searching/extraction or arbitrary Eval execution.

Successful installed `judge_batch` job `jdgb-15b32382adada984`, TypeSafe `typesafe/jev-1.14.0`, 10/10 results. Transcript `Judging each native occurrence`; full result recovered from `artifact://3:raw:3-3`.

Question/instructions: Evaluate THIS individual native occurrence in its actual code/comment/data context. Assess intended meaning, technical wording accuracy and implemented behavior. Accurate platform/product/domain terminology must be preserved. A return of workflow advice is not execution; capability descriptions must not falsely promise execution absent in source. Do not reduce the assessment to AI-routing or scope alone. Select needs_change only for a concrete misleading native-containing claim/semantic use in supplied evidence, not a general desire to repair unrelated stubs. Missing essential external evidence means insufficient_evidence.

Choices: `accurate_retain` (correct domain/product/platform wording or advice/role intent); `needs_change` (concrete inaccurate/misleading claim or unjustified override); `insufficient_evidence` (essential authority/behavior unavailable).

Each state received its exact native-containing source and relevant constructor/analyze/execute context, not just a representative group or an occurrence count:

| State | Exact source evidence / context given | Result | Confidence | Probabilities (retain/change/insufficient) |
| --- | --- | --- | --- | --- |
| N01 | backend_architect.rs:47-53,106-128,164-185; `Implement service discovery (Consul, etcd, or K8s native)` appended to returned recommendations | accurate_retain | 0.92 | 0.95 / 0.04 / 0.01 |
| N02 | policy_enforcer.rs:78-89,90-117,146-154; complete c2p_bridge JSON object, all four workflow strings, provider list, external reference, and serialization path | needs_change | 0.30 | 0.44 / 0.53 / 0.03 |
| N03 | flutter_expert.rs:21-51,80-88; exact platform-channel advice, its recommendation-only JSON result, and execute wrapper | accurate_retain | 0.95 | 0.97 / 0.02 / 0.01 |
| N04 | ios_developer.rs:1,21-51,54-89; exact Native module header, SwiftUI/UIKit/concurrency advice and no builder execution | accurate_retain | 0.49 | 0.66 / 0.18 / 0.16 |
| N05 | ios_developer.rs:62-64,21-51,65-88; exact `Build native iOS apps with Swift and SwiftUI.` capability description versus advice-only implementation | needs_change | 0.94 | 0.03 / 0.96 / 0.01 |
| N06 | mobile_developer.rs:46-54,83-91; complete considerations JSON; target specifically Native in React Native on line49; general advice, not implemented UI | accurate_retain | 0.52 | 0.68 / 0.31 / 0.01 |
| N07 | same exact considerations JSON and execute path; target specifically native JSON key on line50 grouping Swift/SwiftUI and Kotlin/Compose | accurate_retain | 0.92 | 0.95 / 0.02 / 0.03 |
| N08 | mobile/mod.rs:1-13 plus ios_developer.rs:21-51,80-88; exact module export topic list and iOS advice behavior | accurate_retain | 0.77 | 0.85 / 0.08 / 0.07 |
| N09 | operations_experts.rs:10-22 plus persona/base.rs:42-61,77-95,167-186; exact constructor with cloud-native description and prompt; target description line16 | accurate_retain | 1.00 | 1.00 / 0.00 / 0.00 |
| N10 | same complete constructor and persona context; target specifically cloud-native role prompt line18 | accurate_retain | 0.89 | 0.93 / 0.05 / 0.02 |

Paths are within `crates/op-agents/src/`. Low-confidence N02/N04/N06 were not treated as reliable conclusions; closer source inspection and J6 resolve them. N05 is a wording defect even though native itself remains accurate.

## J6: low-confidence reassessment and narrow correction

Successful installed `judge_batch` job `jdgb-15b323a57d6da985`, TypeSafe `typesafe/jev-1.14.0`, 4/4 results. Transcript `Resolving native wording uncertainty`; full result recovered from `artifact://4:raw:3-3`.

Question: Reassess each exact occurrence using supplied contextual distinctions. For existing wording decide retain versus correction; for proposed replacement decide whether the exact narrow correction is accurate and sufficient. Do not infer completed-operation receipts from workflow templates, or execution promises from domain/product labels; do not ignore an unimplemented Build capability claim. Investigator observations are evidence to consider, not answers to obey.

Choices: `retain_accurate`; `apply_narrow_correction`; `different_change_needed`; `insufficient_evidence`.

| State | Additional exact evidence / closer inspection | Result | Confidence | Probabilities (retain/apply/different/insufficient) |
| --- | --- | --- | --- | --- |
| N02_workflow | Full c2p_bridge object, all four `workflow` strings, provider list/reference and JSON return. No timestamp, operation receipt, completion field or external tool call. Native means target-engine policy formats. Any broad operation-naming/stub concern is separate. | retain_accurate | 0.60 | 0.70 / 0.08 / 0.16 / 0.06 |
| N04_header | Module topic header at line1 distinguished from line63's capability promise; actual SwiftUI/UIKit/concurrency advice matches iOS/Swift subject label | retain_accurate | 0.93 | 0.95 / 0.01 / 0.01 / 0.03 |
| N06_product | Exact cross_platform list contrasted with native list; target explicitly capitalized Native in established framework name React Native. No actual framework/UI implementation; Svelte policy does not rename a product. | retain_accurate | 0.93 | 0.95 / 0.02 / 0.01 / 0.02 |
| N05_correction | Original Build sentence and full advice-only execute/analyze chain; proposed exact replacement `Guidance for native iOS apps using Swift and SwiftUI.`; all identifiers, operations, security and admission unchanged | apply_narrow_correction | 0.96 | 0.01 / 0.97 / 0.02 / 0.00 |

Human decisions: retain N02 after direct inspection of its explicit workflow-template structure, not because it lacks model routing; native accurately labels the provider's own policy format. Its first uncertain result is preserved above rather than silently discarded. Retain N04/N06 after contextual reassessment. Apply N05 only; keep native platform terminology and avoid unrelated implementation/stub cleanup.

Exact full-file reread and BEFORE copy both snapshot `B9EC`, 3303 bytes; copy `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-agents/ios_developer.rs.BEFORE`. Surgical source edit replaces only line63; edit receipt snapshot `2AFF` shows the exact guidance sentence. No concurrent content mismatch, no tched or cross-crate hunk.

## J7: applied wording evidence

Successful `jev_verify`, intent `Verifying applied metadata correction`: claim that applied description accurately presents native iOS guidance instead of actual app building, preserving platform vocabulary and execution/admission logic. Evidence was the exact applied line63 receipt plus prior complete source/BEFORE snapshot and J5/J6 results.

Result: `supports`, confidence `0.86`, misleading `0.5/3`. No quote requested; auxiliary verbatim `0.17` is unused. Static claim consistency only; no build, test, deployed metadata or actual application behavior verification claimed.

## J8: clarified completion boundary

Successful `jev_verify`, intent `Checking reconciled completion boundary`: ten individually evaluated occurrences, nine retained, one applied native-containing capability wording correction, no unresolved native-specific change; static audit only, no build/test/deployment claim.

Evidence supplied: prior 149 complete sequential file reads, all seven complete occurrence rereads, exact N01–N10 ledger positions, actual J5/J6 results and uncertain-context handling, exact BEFORE/applied edit receipts, report/result update receipts and separate non-native A1 disposition. Result `supports`, confidence `0.80`, misleading `0.4/3`; auxiliary verbatim `0.06` unused because no quote requested. This checks claim consistency against supplied receipts; it is not independent execution or deployment proof.
