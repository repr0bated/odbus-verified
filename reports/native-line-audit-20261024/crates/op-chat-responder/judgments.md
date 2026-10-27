# Declarative JEV receipts — op-chat-responder

Task: native-line-audit-20261024/op-chat-responder
Date: 2026-10-24
Facility: installed eval `judge(state, questions)`; instructions read from xd://eval/judge.
Successful call: `Judging responder selection and admission evidence` (Python eval). One shared source-evidence state, three independent typed choice questions. No model override, routing facility or source-search/reranking facility used.

## Evidence state

Exact source excerpts supplied from `crates/op-chat-responder/src/main.rs` snapshot #6F76: 26-34,98-123,150-198,200-222,280-308,318-353. The call contains the code bodies, not only these references. Scope explicitly states static native-specific review and full crate read coverage, and forbids inventing a native occurrence or inferring live deployment. No external implementation authority was assumed.

## Questions, contrary outcomes, answers

### selection_effect

Question: Classify observe_selection/config_get's actual provider/model/config effect. Distinguish hard-coded schema path addressing from assigning a provider/model/config override.

Criteria:
- `read_only_observation`: calls config.get for current control-plane reference and selected model; validates shape; returns observed family/model; no assignment or fallback provider/model value.
- `implicit_override`: writes or supplies a hard-coded provider/model/config value overriding the selection.
- `insufficient`: evidence does not distinguish read-only observation from override.

Answer: `read_only_observation`; confidence 1.0. Probabilities: read_only_observation=1, implicit_override=0, insufficient=0.

Decision: preserve the observation paths; no independent native override established. This does not prove schema compatibility or runtime availability of config.get.

### generation_behavior

Question: Does our_stack_generate execute a fallback model route, or refuse because generation is unimplemented? Evaluate the body and process_turn error branch, not function/comment names alone.

Criteria:
- `explicit_unavailable`: body unconditionally bails; caller uses cannot_perform; no model generation or automatic fallback implemented.
- `fallback_route`: body or caller actually invokes a substitute model/provider generation path.
- `insufficient`: cannot establish behavior from excerpts.

Answer: `explicit_unavailable`; confidence 1.0. Probabilities: explicit_unavailable=1, fallback_route=0, insufficient=0.

Decision: record inherited generation stub as a limitation, not a native override; no source edit or successful generation claim.

### admission_inference

Question: What admission conclusion is supported? Treat mount declaration lookup, pending turn equality and generation presence as local evidence, not proof of delegated implementation or deployed security.

Criteria:
- `local_checks_only`: uses mounted method declarations and designated turn context, checks returned turn and generation presence; downstream enforcement/deployed behavior not established.
- `proven_end_to_end`: source alone proves full live identity/capability enforcement.
- `bypass_in_source`: shown code replaces mounted admission with a direct alternative API or CLI identity.
- `insufficient`: cannot evaluate even local checks.

Answer: `local_checks_only`; confidence 1.0. Probabilities: local_checks_only=1, proven_end_to_end=0, bypass_in_source=0, insufficient=0.

Decision: preserve existing checks; do not claim full live admission verification.

## Raw successful response

```json
{
  "selection_effect": {"type":"choice","choice":"read_only_observation","confidence":1,"probabilities":{"read_only_observation":1,"implicit_override":0,"insufficient":0}},
  "generation_behavior": {"type":"choice","choice":"explicit_unavailable","confidence":1,"probabilities":{"explicit_unavailable":1,"fallback_route":0,"insufficient":0}},
  "admission_inference": {"type":"choice","choice":"local_checks_only","confidence":1,"probabilities":{"local_checks_only":1,"proven_end_to_end":0,"bypass_in_source":0,"insufficient":0}}
}
```

JEV judgments support bounded static decisions, not mutation permission, build success or runtime proof.

## Occurrence reconciliation — 2026-10-24

The owner clarified every case/compound occurrence, including wording and intent.
Earlier selection/admission judgments remain bounded evidence, not a complete
occurrence review. Full sequential rereads of Cargo.toml:1-19 and main.rs:1-448
identified one embedded `native` spelling, in `alternatives`; zero standalone
uses. This corrects the earlier zero-occurrence conclusion. No coordinator-followup.md
entry was present in the assignment directory listing.

### O1 meaning and wording

Successful installed `judge(state, questions)` call:
`Judging every responder occurrence context`. No model override or routing.

Exact occurrence: `crates/op-chat-responder/src/main.rs:273`, snapshot #6F76,
token `alternatives`, characters 6-11 (`native`).

Actual source context supplied, lines 265-278:

```rust
fn refuse(views: &GuestViews, turn_id: &str, reason: &str) -> Result<Value> {
    views.call_response(
        "chat_response",
        "cannot_perform",
        json!({
            "turn_id": turn_id,
            "reason": reason,
            "category": "not_supported",
            "alternatives": [],
        }),
        turn_id,
        Deadline::ordinary(),
    )
}
```

The same evidence state also supplied exact source bodies at 106-123
(`call_response` resolves the mounted declaration before `call_for_response`),
298-306 (no generation admission invokes refuse), and 343-351 (generation
error invokes refuse). No schema acceptance or live success assumed.

Question: Evaluate actual meaning, intent and wording of the exact occurrence
of native embedded inside alternatives at line273, not just whether it selects
a provider. Does alternatives:[] accurately denote absence of suggested
alternative actions in this refusal payload, or make a misleading native
capability/implementation claim? Do not infer downstream schema acceptance or
live success from source alone.

Choice criteria:
- `preserve_accurate_field`: The key alternatives means suggested alternative
  actions; empty array supplies none. Its embedded native letters are incidental,
  not native branding/routing/implementation vocabulary. No native-specific wording
  or behavior change warranted in this evidence.
- `change_native_defect`: The occurrence makes an inaccurate native capability/
  behavior claim or implements an implicit native selection requiring a
  native-specific change.
- `insufficient_context`: Cannot determine even this local key's meaning or
  native-specific disposition without more evidence.

Actual response:

```json
{"O1_meaning_and_wording":{"type":"choice","choice":"preserve_accurate_field","confidence":0.98,"probabilities":{"preserve_accurate_field":0.99,"change_native_defect":0,"insufficient_context":0.01}}}
```

Decision: preserve `alternatives` unchanged. The field supplies no suggested
alternative actions; it makes no native implementation or routing claim.
All observed occurrences are mapped in occurrences.tsv. No unresolved
native-specific change remains in this crate.
