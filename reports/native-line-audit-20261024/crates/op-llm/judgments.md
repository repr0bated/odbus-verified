# Declarative JEV receipts — op-llm

Task native-line-audit-20261024/op-llm; Codex crate worker; 2026-10-24.
Installed documentation read: `xd://eval/judge`. Actual successful calls used `await judge(state, questions)` in Python Eval. No routing extension, model choice, endpoint/client invention, search, extraction, filesystem/process/network code or recursive delegation. Each call supplied one combined evidence state with independent typed choice questions. Confidence is the returned `confidence`, not the winning probability. Source excerpts were already read in full sequential coverage; absence/coverage are reader observations, not excerpt-based JEV certification.

## J1 — tool label `Judging exact native contexts`

### State/evidence

Scope: static native contextual audit of op-llm only, every case/compound meaning and accuracy; no runtime/deployment assertion; outside callers unexamined.

`sdk_and_direct` supplied these actual contexts and the surrounding source behavior:
- `antigravity.rs:1`: `//! Native Google/Antigravity provider and its durable control-plane owner.`
- `antigravity.rs:3–7`: revisioned RuntimeDocuments, durable returned-token ledger; `//! uses the native Google Cloud auth SDK; this module never invokes gcloud.` (`gcloud` has backticks in source).
- Import `google_cloud_auth::credentials::{AccessTokenCredentials, Builder as GoogleCredentialsBuilder}`; owner builds access-token credentials with scopes and calls SDK `access_token().await`; empty tokens rejected.
- `authenticated_post` uses origin-bound bridge key only for configured `openai_compatible`; Automatic Gemini mode prefers nonempty GEMINI_API_KEY before SDK Google access token; explicit Google/ApiKey variants also shown.
- `chat_with_request`: `let actual_model = if model.is_empty() || model == "auto" { runtime.selected_model.as_str() } else { model };`; validated model, Gemini endpoint `/models/{actual_model}:generateContent`; safetySettings/systemInstruction/tools/functionCallingConfig/generationConfig.
- `antigravity.rs:974`: `debug!(url = %url, "sending native Antigravity Gemini request");`, followed by authenticated POST/send.
- `antigravity.rs:1110`: `google_sdk_authorized_user_refresh_is_sanitized_and_uses_native_http`; actual SDK user_account Builder with fixture token URI, refresh call, POST/token grant JSON and redacted status assertions supplied.
- `antigravity.rs:1162`: `gemini_request_applies_safety_and_parses_native_usage`; fixture vendor usageMetadata prompt7/completion3/total10, result total10 assertion and safety/tool/generation request assertions supplied.

`bridge_status` supplied ALL seven `NativeBridgeStatus` contexts:
- Import from canonical `op_plugins::state_plugins::antigravity_chat` at18.
- Return type498, unreachable result constructor516, observed result constructor546, wrapper return555, persisted-observation return563, stored-observation optional return576.
- Entire behavior of496–585: requires configured compatible provider; validated origin; GET `/v1/models`, no redirects, bound key; failure reports reachable false/http0/empty models; response bounded2MiB, <=512 models, validated identifiers; successful observation stores URL/status/models/time; RuntimeDocuments read/write observation, not model route.
- Fixture1242 `bridge_status_reads_bounded_native_models_without_fabricated_version`; model-one/model-two response, HTTP200/reachable/model IDs/GET path assertions and no fabricated serialized version.

`chat_words` supplied ALL four occurrences with actual implementation context:
-6 `//! 2. **Antigravity** (native Google ADC refresh or configured bridge)`.
-126 `// Antigravity - native ADC/API-key/OpenAI-compatible owner. Provider`, with127–128 durable config/usage and no-gcloud description.
-132 `info!("Antigravity native provider initialized")` (source includes leading checkmark), initialization/insertion/default-provider conditional130–136.
-627 `"Native Google ADC or OpenAI-compatible bridge"` in626–629 auth_type/features tuple.
- Actual Antigravity Automatic mode uses nonempty GEMINI_API_KEY before SDK; explicit API-key constructor supported; auth status separately reports API key. Priority docs not asserted exhaustive; detailed auth label names only ADC/bridge.

`fixture_words` supplied all of `graph_turn_reminder.rs` behavior pertinent to59/61/63:
- Common caller has no verified principal; prompt directs identity-bound discovery with grants/provenance.
- Existing exact reminder or reminder+newline is reused; otherwise prepend to system content; insert system if absent. Existing caller/tool messages preserved.
-59–60 `// The native factory emits a line without a trailing newline when` / `// there are no other system instructions. Reuse that exact form too.`
-61 `let native =`;62 exact SYSTEM_GRAPH_TURN_REMINDER passed as system content;63 `assert_eq!(native[0].content, SYSTEM_GRAPH_TURN_REMINDER);`.
- External emitter unexamined; supplied code proves exact no-newline-input idempotence, no provider/model choice.

`model_key` supplied `gemini.rs:392–418` static metadata context:
-409 `"gemini-live-2.5-flash-native-audio"`, `LiveApi`, rates0/4000000/0.
- Model-info/list/search/get functions copy ID/category. Full GeminiClient has no native-audio special branch. Caller model only changed for explicit auto/gemini-auto. Exact vendor identifier should not be casually renamed; static list is not current availability proof.

### Questions, explicit alternatives, actual answers

1. `J1.sdk_direct`: Are the exact native documentation, direct-request log and two fixture names accurate SDK/API/returned-usage meanings rather than implicit authority/routing overrides?
   - Criteria: `preserve_sdk_api_terms` = shown SDK/direct API/vendor usage supports wording; `fix_inaccurate_words` = false SDK/direct API/usage claim; `insufficient` = unresolved semantics.
   - Actual: **preserve_sdk_api_terms**, confidence **0.69**; probabilities preserve0.79/fix0.19/insufficient0.02.
   - Decision: preserve five occurrences. SDK import/build/access_token and request body/response parser substantiate the meaning. Does not certify runtime credentials or deployment.
2. `J1.bridge_terms`: Do all seven type occurrences and bounded_native_models fixture describe observed bridge data, not privileged model selection; is local renaming needed?
   - Criteria: `preserve_observation_type` = canonical observation/fixture, not routing/billing proof; `fix_false_claim` = shown behavior misrepresented; `insufficient` = need type/caller evidence.
   - Actual: **preserve_observation_type**, confidence **0.98**; probabilities0.99/0.01/0.
   - Decision: preserve eight occurrences; no independent schema/type authority introduced.
3. `J1.chat_descriptions`: Evaluate all four native descriptions for accuracy, distinguish valid SDK wording from materially incomplete auth_type label.
   - Criteria: `preserve_all`; `fix_auth_description` = SDK wording legitimate but ADC-or-bridge descriptions omit API-key mode; `remove_native_routing` = words cause routing mutation; `insufficient`.
   - Actual: **fix_auth_description**, confidence **0.95**; probabilities0.04/0.96/0/0.
   - Decision F1: retain126/132; add supported Gemini API key to6/627, preserve accurate native adjective.
4. `J1.fixture_description`: Evaluate comment and both local native identifiers; exact reminder handling shown, external emitter not examined.
   - Criteria: `neutralize_unverified_emitter_claim` = describe test input, preserve behavior/identifier; `preserve_all` = external emission established; `routing_defect`; `insufficient`.
   - Actual: **neutralize_unverified_emitter_claim**, confidence **1**; probabilities1/0/0/0.
   - Decision F2: replace comment only with exact-input description, retain both fixture identifier occurrences.
5. `J1.model_identifier`: Does native-audio identify modality rather than implicit routing; what can static evidence justify?
   - Criteria: `preserve_vendor_identifier` = exact ID, no native-specific routing; availability unverified; `rename_or_remove` = token incorrectly forces route/meaning; `insufficient`.
   - Actual: **preserve_vendor_identifier**, confidence **1**; probabilities1/0/0.
   - Decision: preserve exact ID, no current availability assertion.

Full answer output was individually printed by successful Eval `Recording full judgment answers` because the initial combined one-line output was truncated. No missing answer accepted on plausibility.

## J2 — tool label `Judging applied wording disposition`

### State/evidence

Actual successful edit receipts: `chat.rs#6712` line6 now `//! 2. **Antigravity** (native Google ADC refresh, Gemini API key, or configured bridge)`; line627 now `"Native Google ADC, Gemini API key, or OpenAI-compatible bridge",`. `graph_turn_reminder.rs#9587` line59 now `// Reuse an exact reminder supplied without a trailing newline too.` Existing fixture binding/assertion now60/62. Only those two descriptions and fixture comment changed; identity-bound instruction, selection/auth/schema/capability logic and test behavior unchanged.

Exact full BEFORE copy receipts match freshly reread source tags/byte counts: `chat.rs.BEFORE#402C`28997bytes; `graph_turn_reminder.rs.BEFORE#239A`3145bytes. Edits anchored402C/239A.

J1 SDK/direct and original chat descriptions supplied again. Additional already-read generic source contexts: ChatManager invalid/unavailable LLM_PROVIDER default fallback247–266, absent-current first-provider fallback371–384, known-list/model-family/first-provider helper539–587; Gemini chat and request implementations add BALANCED auto-routing for Gemini3+VertexAI. None controlled by native/native_config flags. External consumers unexamined. Generic fallback/routing stays report-only; do not certify overall routing or runtime admission. schema.rs reuses canonical plugin types/current_state and declares no schema.

### Questions and answers

1. `J2.patch_correctness`: Do returned wording changes resolve both semantic issues without altering selection/auth/capabilities/schema authority/test behavior?
   - Criteria `scoped_wording_fix`, `incomplete_or_regression`, `insufficient`.
   - Actual **scoped_wording_fix**, confidence **0.85**; probabilities0.90/0.08/0.02.
   - Decision: source-level wording correction complete. No runtime correctness inferred.
2. `J2.generic_disposition`: Distinguish narrow native scope from broad routing/security certification.
   - Criteria `report_separately` = generic branches not native-controlled; `expand_native_patch`; `insufficient`.
   - Actual **report_separately**, confidence **1**; probabilities1/0/0.
   - Decision: record generic observations, no unrelated patch.
3. `J2.completion_boundary`: What verification claim follows from wording-only patch and static audit, with no execution authorized/performed?
   - Criteria `static_only`, `runtime_verified`, `insufficient`.
   - Actual **static_only**, confidence **1**; probabilities1/0/0.
   - Decision: report static coverage/native disposition and source edits only; no built/tested/deployed claim.

## Accounting

Eight actual successful typed answers across two installed declarative calls; confidence0.69–1. All21 initial occurrences explicitly mapped in occurrences.tsv: thirteen antigravity, four chat, one Gemini model ID, three reminder fixture tokens.18 unchanged, two auth descriptions corrected retaining native, one unsupported native-emitter comment removed.20 native tokens remain after edit. No grouped unexamined occurrence claimed judged.
