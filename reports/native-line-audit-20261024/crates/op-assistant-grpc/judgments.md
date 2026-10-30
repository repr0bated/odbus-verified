# Declarative JEV receipts

Task: `native-line-audit-20261024`; crate: `op-assistant-grpc`; date: 2026-10-24.
Tool: `functions.eval`, Python `await judge(native_audit_state, native_audit_questions)`; successful call titled `Judging explicit selection and admission`. Instructions read from `xd://eval/judge`. One state, four independent typed choice questions; no model routing, reranking, searches, filesystem execution, or delegation. Native absence rests on full-file reading in coverage.tsv, not on a fabricated native occurrence or a JEV keyword scan.

## Evidence supplied

- Scope: this crate only; native-specific implicit model/provider/config overrides; preserve canonical admission; no runtime claim.
- `src/models.rs:49-63`: `switch_model` consumes `SwitchModelRequest`; constructs `json!({ "model_id": req.model_id })`, copies optional `agent_id` and `session_id`, calls `self.client.call("models.switch", params).await?`, converts response. The call state included this function as code.
- `src/agents.rs:66-82,85-111`: create parameters use supplied `req.model`; update changes `params["model"]` only under `if let Some(v) = req.model`. The call state explicitly summarized the surrounding fields and calls rather than claiming a verbatim full-function quotation.
- `src/auth.rs:23-50`: code supplied for entire interceptor: rejects OIA1, missing/duplicate/malformed/invalid SID1, and unconditionally ends in `Err(Status::unavailable("standalone Assistant RPCs disabled until canonical identity and grant admission is shared"))` even after structural SID1 open.
- `src/server.rs:97-129,158-166`: state summarized all eight application `ServiceServer::with_interceptor(..., wireguard_auth_interceptor)` registrations and loopback-only address check; health/reflection explicitly distinguished.
- `src/transport.rs:113-121`: code supplied for `call`: DBus branch returns `self.dbus_call(method, &body).await`; Rpc branch returns disabled transport error. No HTTP execution or catch-and-retry.
- `src/dbus_service.rs:24-28`: code supplied for unconditional `AccessDenied` compatibility call.
- Coverage statement: all 31 inventoried files read to EOF; no native token/native-specific implicit override found. No other-crate handler or deployed runtime claim.

## Questions and results

### 1. Model selection

Question: “Classify the shown model-changing code in THIS crate. Does it force a provider/model without caller input? Do not infer other-crate backend behavior.”

Criteria:
- `explicit_forwarding`: shown methods forward explicit caller model/model_id; no shown native/provider/model override.
- `implicit_override`: shown methods substitute or force model/provider independent of caller selection.
- `insufficient`: evidence insufficient to distinguish caller-forwarding from implicit override.

Result: `explicit_forwarding`; confidence **1.00**. Probabilities: explicit_forwarding 1.00, implicit_override 0.00, insufficient 0.00.
Decision: retain explicit model selection; no native-specific repair evidenced. Upstream handling is not inferred.

### 2. Admission

Question: “Classify standalone application RPC admission in shown server/auth code. Distinguish application RPCs from health/reflection and do not claim live deployment.”

Criteria:
- `fail_closed`: application RPCs use interceptor with no successful return; SID1 structural validity alone never grants access.
- `allows_asserted_identity`: shown application path admits asserted legacy identity or structural SID1 alone.
- `insufficient`: source does not establish application admission behavior.

Result: `fail_closed`; confidence **1.00**. Probabilities: fail_closed 1.00, allows_asserted_identity 0.00, insufficient 0.00.
Decision: preserve guard; do not “repair” a retired standalone path by weakening canonical identity/grant admission.

### 3. Transport fallback

Question: “Does the shown transport call path implement a direct HTTP fallback after D-Bus denial/failure?”

Criteria:
- `no_http_fallback`: D-Bus error propagates; Rpc branch returns disabled error, not HTTP execution.
- `fallback_bypass`: D-Bus denial/failure can execute direct HTTP control call.
- `insufficient`: provided code cannot distinguish.

Result: `no_http_fallback`; confidence **1.00**. Probabilities: no_http_fallback 1.00, fallback_bypass 0.00, insufficient 0.00.
Decision: do not mistake stale crate-level fallback prose or retained config names for an executable fallback.

### 4. Native repair disposition

Question: “Given full-file coverage finding plus exact selection/admission/transport evidence, is a native-specific repair justified in these shown paths? Do not treat this as a full correctness audit or proof that other crates have no defects.”

Criteria:
- `no_repair_evidenced`: no native-specific override/gate defect evidenced in this crate's shown paths; leave source unchanged.
- `repair_evidenced`: shown code establishes native-specific implicit override/gate defect requiring repair.
- `insufficient`: evidence cannot decide native-specific disposition of shown paths.

Result: `no_repair_evidenced`; confidence **0.92**. Probabilities: no_repair_evidenced 0.95, repair_evidenced 0.03, insufficient 0.02.
Decision: zero independent native fixes and zero threetched-overlap proposals. Confidence is judge evidence, not execution proof.

## Raw returned answers

```json
{
  "model_selection": {"type":"choice","choice":"explicit_forwarding","confidence":1,"probabilities":{"explicit_forwarding":1,"implicit_override":0,"insufficient":0}},
  "admission": {"type":"choice","choice":"fail_closed","confidence":1,"probabilities":{"fail_closed":1,"allows_asserted_identity":0,"insufficient":0}},
  "transport_fallback": {"type":"choice","choice":"no_http_fallback","confidence":1,"probabilities":{"no_http_fallback":1,"fallback_bypass":0,"insufficient":0}},
  "native_repair": {"type":"choice","choice":"no_repair_evidenced","confidence":0.92,"probabilities":{"no_repair_evidenced":0.95,"repair_evidenced":0.03,"insufficient":0.02}}
}
```

## Every-occurrence clarification reconciliation

Updated common instructions read completely in the same continuing session. Scope now explicitly includes all case/compound forms and the accuracy of wording/claimed behavior in comments, documentation and fixtures, not just model selection.

Complete prior source reads in [coverage.tsv](coverage.tsv) contain **zero occurrences** under that scope. [occurrences.tsv](occurrences.tsv) records zero occurrence rows with explicit metadata and coverage reference. The four actual successful source-context judgments above are retained unchanged; they are not treated as per-occurrence judgments for contexts they never received, and no nonexistent occurrence was invented to obtain a JEV result.

Supplementary occurrence judgment: **not applicable — no occurrence context exists**. Absence is the reviewer's full sequential reading conclusion, not a new JEV source scan. No additional judge call is claimed in this reconciliation. No coordinator-followup file was shown in the complete assignment-directory listing. No native-specific edit, unresolved overlap, missing source range or pending occurrence remains.
