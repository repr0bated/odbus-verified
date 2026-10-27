# Declarative JEV receipts — op-chat

Successful installed `judge()` call via `functions.eval` (Python), 2026-10-24. No model routing or arbitrary execution.

## J1: native tool-call semantics and caller selection

Evidence: `src/nl_admin.rs:25-44,109-140,409-449`; `src/forced_tool_pipeline.rs:115-190`. Exact source excerpts supplied in eval transcript: enum `ToolCallSource::Native`; parser reads `response.tool_calls`, clones names and arguments, returns nonempty structured calls before text parsing; both process methods accept provider/model arguments and invoke `provider.chat_with_request(model, request)`; pipeline request has required tools and sampling fields.

Question `native_semantics`: does Native select/override provider/model/config, or label API-structured tool calls? Explicit outcomes `representation_only`, `selection_override`, `insufficient_evidence`.

Result: `representation_only`, confidence 1; probabilities `{representation_only:1, selection_override:0, insufficient_evidence:0}`.

Question `pipeline_selection`: does shown invocation preserve passed provider/model or implicitly replace either? Explicit outcomes `caller_preserved`, `implicit_replacement`, `insufficient_evidence`; generation sampling options excluded from provider/model routing criteria.

Result: `caller_preserved`, confidence 0.98; probabilities `{caller_preserved:0.99, implicit_replacement:0, insufficient_evidence:0.01}`.

Decision: retain `Native` enum/comment vocabulary; no provider/model cutover in these methods. Judgment establishes static semantics only, not admission of external callers or runtime correctness.

## J2: contextual selection, protocol wording and admission

Exact-source batch receipt: `jdgb-15b3209963025545`, model `typesafe/jev-1.14.0`, 3/3 judged. The first eval mistakenly awaited synchronous `judge_batch`, raising `TypeError: object JudgmentBatch can't be used in 'await' expression` after submission. Completion notification supplied its durable ID; attached to that original batch and retrieved all results successfully. A retry batch `jdgb-15b320a658825546` also completed (3/3) with consistent classifications; no routing or runtime action.

Shared question: classify the supplied source as `native_override` (native-specific branch replacing authoritative selection/config), `generic_or_descriptive` (caller/configurable default or OS/protocol wording), `fail_closed` (explicit unavailable admission/scoped dispatch without fallback), or `insufficient_evidence`. Do not infer deployment/external callers.

| State and evidence | Original exact-source result | Confidence | Probabilities | Decision |
| --- | --- | --- | --- | --- |
| actor: `src/actor.rs:29-53,557-565,636-646`; configurable public `default_model`, default `"default"`, explicit optional request model wins via `model.as_deref().unwrap_or(&self.config.default_model)`, same provider/model passed to pipeline | generic_or_descriptive | 0.95 | native_override0.03; generic_or_descriptive0.96; fail_closed0; insufficient_evidence0.01 | Generic fallback alias, not a native-specific named product/provider override. Preserve. |
| protocol_prompt: `src/system_prompt.rs:90-94,181-224,482-507`; native rtnetlink/bridge/protocol/D-Bus tools; shell tool only when no native tool exists; minimal prompt asks native tools directly | generic_or_descriptive | 0.92 | native_override0.06; generic_or_descriptive0.94; fail_closed0; insufficient_evidence0 | Protocol/implementation vocabulary, not LLM provider/config selection. Preserve wording. Does not certify the broader shell or topology policy. |
| admission: `src/orchestration/services/mod.rs:155-198`; `src/orchestration/grpc_pool.rs:606-623`; `src/orchestration/workstack_executor.rs:421-442,506-525,838-849`; unconditional unavailable listener interceptor, unavailable scoped agent/tool defaults, required verified scope and scoped phase dispatch | fail_closed | 0.99 | native_override0; generic_or_descriptive0.01; fail_closed0.99; insufficient_evidence0 | Preserve fail-closed boundaries; do not wire a new bypass to make legacy paths available. |

Retry batch results: actor generic_or_descriptive confidence0.93 (probabilities0.05/0.95/0/0); protocol_prompt generic_or_descriptive confidence0.79 (0.15/0.84/0.01/0); admission fail_closed confidence0.99 (0/0.01/0.99/0). Original exact-source receipts are primary; neither batch proves deployed admission.

## Reconciliation supersession

The owner clarified every native occurrence, including wording, behavior claims and admission intent. J1/J2 remain valid evidence for their narrow questions, but J2's “preserve wording” decision is **superseded** for prompt94/202/211/476/496. No-model-routing is insufficient to establish those instructions are accurate or authorized. The completed full-file reads remain evidence.

## J3: every occurrence, actual context and intent

Installed `judge_batch` receipt **`jdgb-15b32b095b4c308a`**, `typesafe/jev-1.14.0`; final status33/33, failed0, cost$0.000978348. First result print was a partial settled snapshot; after completion, N126/F44 were retrieved from the same batch, not inferred. Full transcript/source-state payload retained by the assignment worker; printed results `artifact://0:raw:2-2` plus the final settled N126/F44 eval receipt.

Each state is the correspondingly named `occurrences.tsv` row. The actual call supplied that occurrence's source path/range, contextual text and intent independently. It covered18 native/Native/natively occurrences plus15 conservative incidental substrings inside alternative(s)/alternative_thought. No occurrence was judged merely by membership in a representative group. Multiple positions on SPEC1386 were separate states.

Question `disposition`: “Evaluate this particular occurrence with its actual context and intent, including correctness of wording and claimed behavior, NOT merely whether it routes a model. Native direct-protocol/API vocabulary can be correct. A downstream native implementation is not permission to bypass identity-bound declared plugin admission. A normative draft requirement/principle differs from a claim of deployed support. Alternatives contains a literal native substring but is not the word native; assess its actual meaning. Choose change_needed for a concrete misleading/unsafe native-related instruction, keep_accurate for accurate contextual usage, incidental_substring for the unrelated word alternative(s), or insufficient_evidence if needed authority/implementation is missing. Do not infer live availability or let unrelated generic debt force a native change.”

Explicit criteria:
- `keep_accurate`: occurrence and native-related intent accurate in shown context; preserve vocabulary/identifier.
- `change_needed`: specific native-related wording or claimed action contradicts implementation or stated authority; scoped correction warranted.
- `incidental_substring`: embedded in alternative(s)/alternative_thought, unrelated to native terminology; no native-specific change.
- `insufficient_evidence`: missing authority/implementation prevents decision.

| State | Result | Confidence |
| --- | --- | --- |
| D170 | keep_accurate | 0.97 |
| D605 | keep_accurate | 0.49 |
| D1335 | keep_accurate | 0.44 |
| S45 | keep_accurate | 0.48 |
| S974 | keep_accurate | 0.40 |
| S1386a | keep_accurate | 0.96 |
| S1386b | keep_accurate | 0.79 |
| S1442 | change_needed | 0.19 |
| S1466 | keep_accurate | 0.83 |
| N36 | keep_accurate | 0.97 |
| N37 | keep_accurate | 0.95 |
| N115 | keep_accurate | 0.92 |
| N121 | keep_accurate | 0.96 |
| N126 | keep_accurate | 0.93 |
| P90 | keep_accurate | 0.84 |
| P94 | change_needed | 0.57 |
| P185 | change_needed | 0.23 |
| P202 | change_needed | 0.68 |
| P211 | change_needed | 0.61 |
| P476 | keep_accurate | 0.39 |
| P496 | change_needed | 0.28 |
| O357 | incidental_substring | 0.39 |
| O463 | incidental_substring | 0.96 |
| G586 | incidental_substring | 0.93 |
| G2715 | incidental_substring | 0.71 |
| G5225 | incidental_substring | 0.51 |
| T353 | incidental_substring | 0.96 |
| T391 | incidental_substring | 0.79 |
| T392 | incidental_substring | 0.96 |
| T396 | incidental_substring | 0.95 |
| K46 | incidental_substring | 0.79 |
| E44 | incidental_substring | 0.52 |
| F44 | keep_accurate | 0.37 |

Low-confidence classifications are not adopted as answers. Manual closer review: Alternative/alternatives/alternative_thought are plainly unrelated words/identifiers; their local examples/branch logic support the intended alternative meaning regardless of JEV's keep-vs-incidental category uncertainty. S45/S1442/native prompt ambiguities received focused evidence in J4. S1442 reread exact parser107-135: nonempty structured calls return before text parsing, establishing accurate preference. No fabricated native uses were added.

## J4: focused ambiguities and concrete candidate corrections

Installed `judge_batch` receipt **`jdgb-15b32b363f8c308b`**, `typesafe/jev-1.14.0`,8/8. Actual result `artifact://1:raw:2-2`; source states and proposed text in eval transcript. Each state received actual old wording, relevant implementation or repo identity boundary, and its candidate correction, not only a routing label.

Question `review`: “Judge the concrete native-related wording with exact evidence and proposed correction if any. Choose keep when wording is accurate in context and no correction needed; change when a minimal proposed correction is justified by evidence and preserves native terminology and admission; insufficient when evidence cannot decide. Do not judge merely provider/model routing, infer deployment, or expand to unrelated generic defects.”

Criteria: `keep` accurate/sufficiently clear design principle; `change` concrete inaccurate claim/unsafe fallback/authority ambiguity warrants shown correction; `insufficient` missing evidence prevents settling.

| State / exact evidence | Result | Confidence | Probabilities keep/change/insufficient |
| --- | --- | --- | --- |
| S45: SPEC28-48 principle vs actor generic D-Bus unavailable; qualify admitted registered tools | change | 0.89 | 0.06/0.93/0.01 |
| S1442: SPEC1441-1445 and exact nl_admin107-135 precedence | keep | 0.88 | 0.92/0.06/0.02 |
| P94: native rovs backend→shell setup instruction vs declared-operation authority; forbid fallback | change | 0.72 | 0.17/0.81/0.02 |
| P185: generic native bridge phrase with proposed common rule211 restricting all native tools | keep | 0.27 | 0.51/0.32/0.17 |
| P202: projection OR native tools vs identity-bound projection/backend admission | change | 0.66 | 0.20/0.77/0.03 |
| P211: native protocol list including sv vs explicit declared-plugin/capability-only instruction | change | 0.77 | 0.10/0.85/0.05 |
| P476: absent native tool→shell fallback vs absent/denied operation cannot_perform | change | 0.94 | 0.02/0.96/0.02 |
| P496: minimal native tools directly vs declared-plugin identity/capability boundary | change | 0.83 | 0.06/0.89/0.05 |

Decision: six minimal hunks in SPEC45 and prompt94/202/211/476/496. Preserve accurate representation vocabulary and native bridge/rtnetlink terms. P185's low-confidence keep was not treated as sufficient: manually compare generic wording with explicit common rule and final J5 context. No specific parallel endpoint/transport is instructed by P185.

## J5: final applied wording against authority

Installed `judge_batch` receipt **`jdgb-15b32b762d0c308c`**, `typesafe/jev-1.14.0`,7/7; actual results `artifact://2:raw:2-2`. Evidence is the applied edit response `SPEC.md#6EA9` and `src/system_prompt.rs#7E10`, not an assumed or unexecuted patch. Every changed native occurrence was supplied in its exact final wording. Unchanged P185 was judged with final rule211.

Question `static_resolution`: “Evaluate exact final native wording and intent against evidence/authority. Does this settle the specific native wording/admission defect without claiming live tool availability or weakening identity checks? This is static source evidence only, not build/runtime proof. Keep legitimate native vocabulary. Do not infer unrelated topology/tool-list defects have been fixed.”

Criteria: `resolved` final wording accurate/appropriately scoped; `unresolved` concretely misleading or authorizes bypass; `insufficient` missing evidence to decide exact statement.

| State / final context | Result | Confidence | Probabilities resolved/unresolved/insufficient |
| --- | --- | --- | --- |
| S45: native protocol principle; generic calls unavailable; admitted registered tools | resolved | 0.73 | 0.82/0.14/0.04 |
| P94: native rovs backend does not authorize shell/setup fallback; missing/denied requires cannot_perform | resolved | 0.79 | 0.86/0.05/0.09 |
| P202: identity-bound projection; native backends behind declared operation/capabilities | resolved | 0.62 | 0.75/0.11/0.14 |
| P211: native tools only through declared identity-bound admitted plugins; no shell/transport fallback | resolved | 0.37 | 0.58/0.04/0.38 |
| P476: absent native tool not shell permission; cannot_perform for absent/denied operation | resolved | 0.91 | 0.94/0.02/0.04 |
| P496: minimal prompt explicitly preserves declared identity/capability admission and no fallback | resolved | 0.55 | 0.70/0.04/0.26 |
| P185: native bridge tools with explicit common admitted-plugin-only rule211 | resolved | 0.59 | 0.73/0.12/0.15 |

Manual closure of low-confidence final P211/P496/P185: these are normative prompt instructions, not claims that runtime admission is implemented. The edited strings explicitly require the existing declared identity/capability boundary and explicitly prohibit fallback; P185 falls under common rule211. No missing external implementation evidence is needed to verify what these instructions now say. Runtime enforcement and deployed tool availability remain unverified. JEV confidence is retained, not upgraded or represented as proof.

Completion evidence: all54 full-file reads in coverage.tsv;33 individually contextualized occurrence states; six source-hunk edit receipts; complete per-occurrence disposition ledger. No build/tests/runtime actions authorized or run; coordinator owns later authorized build. No searches, routing, delegation or arbitrary eval were used.
