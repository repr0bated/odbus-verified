# Declarative JEV receipts — op-cognitive

Date 2026-10-24. Installed `xd://eval/judge` instructions read; supported `await judge(state, questions)` invoked through JavaScript eval. No model routing, source filtering or search used. Successful tool output is in this worker transcript.

## J1 — explicit Gemini operation and JSON native vocabulary

Evidence: `src/cognitive_tools.rs:64-150`, `src/gemini_fallback.rs:21-53`, `src/cognitive_tools.rs:779-796`, all fully read. Submitted source excerpts identify `GeminiQueryTool`, tool name `gemini_query`, capability `cognitive.invoke`, parsed Gemini request, query/depth validation and explicit mode dispatch; provider-specific environment defaults; complete native-values serialization test body. Dispatcher branch summarized in submitted evidence, not claimed verbatim.

Questions and explicit contrary outcomes:

- selection: Does evidence show explicit provider operation, implicit override of authoritative chatbot selection, or insufficient evidence? Answer `explicit_operation`; confidence 1; probabilities explicit_operation 1 / implicit_override 0 / insufficient 0.
- native_word: Is `native` in the test name serialization vocabulary, routing selection, or insufficient? Answer `serialization`; confidence 1; probabilities serialization 1 / routing 0 / insufficient 0.
- admission_limit: Does this demonstrate local capability/input checks only, end-to-end authenticated execution, or insufficient evidence? Answer `local_only`; confidence .95; probabilities local_only .97 / end_to_end 0 / insufficient .03.

Decision: preserve provider-specific Gemini configuration and JSON serialization test vocabulary. These snippets do not establish implicit ordinary-chat native overrides or end-to-end identity enforcement. Remaining actual callers reviewed next; no source mutation authorized by JEV alone.

## J2 — technical native contexts; supported fallback

Attempted installed `judge_batch` with three evidence states (Voyage dimensions,
Qdrant domain sockets, OVS fixture). Exact failure:
`ReferenceError: judge_batch is not defined` in JavaScript eval. No batch
judgment was produced. Reported via `xd://report_issue`; tool acknowledged it.
Used installed `jev_verify` with a combined claim/evidence check instead; did
not invent an endpoint or load a routing extension.

Claim: these native/natively references describe embedding dimensions,
domain-socket functionality and OVS fixture prose, rather than an executable
override of authoritative chatbot provider/model selection. VoyageClient::new
consumes embedding_model's resolved endpoint/key/model parameters; shared
dimension is an embedding compatibility contract.

Submitted evidence (source excerpts with flattened comments, not verbatim
whole-file quotations):

- `src/voyage.rs:23-33`: request carries input, model, input_type, truncation
  and output_dimension; comment contrasts provider “native (differing)
  dimension” with pinned shared-space dimension.
- `src/voyage.rs:65-85`: `let params = voyage_embed_params().context(
  "Voyage credentials are missing or embedding_model's current provider,
  endpoint, dimensions, or vector space is incompatible")?;`
  followed by `Self::from_params(params.endpoint, params.api_key, params.model_id)`.
  Request uses `model: &self.model` and
  `output_dimension: VoyageModel::SHARED_EMBEDDING_DIMS`.
- `src/voyage.rs:124-130`: returned vector must match shared dimension and
  contain only finite numbers.
- `src/rag_pipeline.rs:1223-1237`: comments identify native domain-socket
  `"network": "ds"` outbound; function reads `COGNITIVE_QDRANT_URL` or
  `http://127.0.0.1:6334` and constructs Qdrant client.
- `tests/e2e_voyage_qdrant.rs:112-125`: `ovs_doc` contains “The rovs plugin
  drives Open vSwitch natively over OVSDB JSON-RPC”; that string and a mail
  document are passed to `embed_document`.

Actual result: `verdict=supports`, `verbatim=0.15`,
`misleading=0.6/3`, `confidence=0.85`, `tokens(in/out)=1270/78`.
No claimed verbatim quote was submitted (quote argument empty); the verbatim
score is not used as a quotation guarantee. Decision: preserve all three
contexts. This result neither verifies runtime topology nor re-audits the
external embedding resolver's implementation.

## J3 — generic inherited risks and fixed compatibility implementation

One `judge` state contained the following exact behavioral source:

- `src/doctor.rs:158-170`: query-history preview uses
  `if turn.answer.len() > 200 { format!("{}...", &turn.answer[..200]) }
  else { turn.answer.clone() }`.
- `src/semantic_search.rs:60-69`: `index_file(repo,file_path,content)` has
  `// TODO: Call RagPipeline::index_file when available` and immediately
  returns JSON with repo, file_path, `chunks_indexed: 1`,
  `tokens_used: content.len()/4`, `status: "indexed"`.
- `src/grpc_service.rs:1-4,698-750`: module describes embedded/internal
  compatibility contract; `generate_data_table` formats requested columns
  and prompt, then calls `self.gemini_fallback.gemini_query(&prompt,
  Some(&context))`. Memory reads/output parsing were explicitly summarized,
  not represented as complete source quotations.

Questions, explicit alternatives, results:

1. Can arbitrary valid UTF-8 answers over200 bytes panic if byte200 falls
   inside a character? Alternatives `risk`, `safe`, `insufficient`.
   Answer `risk`, confidence .91; probabilities .94/.05/.01.
2. Does the index_file body actually index? Alternatives `false_receipt`,
   `real_index`, `insufficient`. Answer `false_receipt`, confidence1;
   probabilities1/0/0.
3. Does the table body establish overwriting a caller-authoritative native
   chatbot route? Alternatives `not_established` (fixed Gemini call, no
   caller selection/overwrite shown), `override`, `insufficient`.
   Answer `not_established`, confidence1; probabilities1/0/0.

Decision: generic source risks reported separately, not repaired under native
scope. Fixed compatibility table engine is not evidence of authoritative native
route override. No runtime reproduction or external reachability claimed.

## J4 — clarified every-occurrence meaning and accuracy

Supersedes J1/J2's narrow preservation decisions for wording at O2/O3.
Continuation used actual installed `await judge(state, questions)` successfully.
Full supplied evidence, proposed minimal corrections, explicit contrary outcomes
and actual probabilities are saved in `occurrence-judgment-evidence.md`.
One shared crate-occurrence evidence document supported five independent
questions; no source filtering, model selection/routing, arbitrary execution
or repeated unavailable batch call.

| Ledger ID | Source before edit | Question | Actual outcome | Confidence |
| --- | --- | --- | --- | --- |
| O1 | cognitive_tools.rs:779-797 | Is native_values accurate against the ordinary JSON payload assertion? | preserve | .70 |
| O2 | voyage.rs:23-33,65-85,124-130 plus shared1024 owner contract | Does differing-native-width wording misstate supported variants; does proposed comment correct it without behavior change? | correct_as_proposed | .97 |
| O3 | rag_pipeline.rs:1223-1238 | Does whole native-socket comment overstate helper localhost/proxy responsibility; should forwarding be qualified? | correct_as_proposed | .98 |
| O4 | tests/e2e_voyage_qdrant.rs:112-125,163-193 | Is natively intelligible semantic fixture prose rather than live OVS proof? | preserve_fixture | .98 |
| O5 | gemini_fallback.rs:277-303 | Is native inside alternative only the ordinary English request for other approaches? | preserve_alternative | 1 |

Decisions implemented: O2 corrected documentation and attached shared-width
comment to output_dimension; O3 preserves native domain-socket wording while
requiring separate forwarding configuration and documenting loopback as default.
O1/O4/O5 preserved. Original exact hunk excerpts saved before edits;
`before-excerpts.json` records their scope and read/edit tags. Edit tool accepted
both refreshed source snapshots. No executable statement, model selection,
credential handling, identity or admission condition changed. No test/build or
runtime operation run or claimed. Every ledger occurrence has a final disposition.
