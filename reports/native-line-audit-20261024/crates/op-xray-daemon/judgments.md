# Declarative JEV evidence — op-xray-daemon

JEV ran through the installed `judge(state, questions)` facility. All submitted states included the exact assigned occurrence and surrounding source text; later calls added relevant caller/implementation evidence. These are source-context judgments, not runtime verification.

## J1 — initial contextual assessment

Source: `crates/op-xray-daemon/proto/VENDORED.md:26-42`.

Exact occurrence: line 42 says “native account/user preflight prevents that through the plugin path.” Context explains typed VLESS/VMess/Trojan users translated into upstream protobuf messages, VLESS UUID identity equivalence checked before duplicate credentials, and Xray's Trojan validator potentially replacing a user-map entry for reused credentials.

Questions: whether “native” accurately describes Xray account/user preflight or should be replaced; whether the term affects/overrides model-provider selection; and audit disposition.

Answers:
- `meaning`: `retain`, confidence 0.27 (probabilities retain 0.51, replace 0.42, insufficient 0.07). Low confidence; not accepted alone.
- `routing`: bool probability 0.03 that it affects model/provider selection (thus 0.97 support for descriptive account/user terminology).
- `wording`: `retain`, confidence 0.79 (retain 0.86, correct 0.11, defer 0.03).

## J2 — caller-contract context

Added exact evidence from `crates/op-plugins/src/state_plugins/xray.rs:490-553,572-578` (outside assigned source/edit scope): typed `XrayHandlerProtocol` is documented as supported native Xray account/inbound protocols; typed `XrayHandlerAccount` supports VLESS/VMess/Trojan; account UUID is validated/normalized before native RPC; `XrayHandlerUser` has the exact unique Xray email/user key; receipts describe accepted HandlerService mutations.

Questions: whether the assigned wording accurately summarizes Xray typed account/user semantics; whether it means CPU/host-native execution or model/provider selection; and disposition.

Answers:
- `meaning`: `retain`, confidence 0.30 (retain 0.53, replace 0.33, insufficient 0.14).
- `authority`: bool probability 0.05 that it denotes host-native execution or model/provider selection (0.95 for Xray protocol/account semantics).
- `decision`: `unresolved`, confidence 0.07 (retain 0.28, correct 0.34, unresolved 0.38). This weak judgment was not accepted as final.

## J3 — implementation-backed disposition

Added exact implementation evidence from `crates/op-grpc-bridge/src/xray_methods.rs:447-473` (outside assigned source/edit scope): `validate_user_target` calls HandlerService `GetInboundUsers`, computes `account_identity`, compares the existing account through `native_account_identity`, and rejects duplicate email keys and duplicate credentials before returning success. The preceding comment explicitly states reused VLESS/Trojan credentials can overwrite a credential map entry and that the checks reject the destructive alias before issuing the native mutation. This confirms what the assigned prose summarizes.

Questions: whether the assigned phrase is accurate or materially misleading; whether it has any model/provider routing relation; and the justified occurrence disposition.

Answers:
- `contextual_meaning`: `accurate_retain`, confidence 0.45 (accurate-retain 0.63, misleading-correct 0.34, insufficient 0.03). Moderate-low confidence; considered alongside the concrete implementation.
- `selection`: bool probability 0.03 that it relates to model/provider routing (0.97 no).
- `disposition`: `retain`, confidence 0.79 (retain 0.86, edit 0.13, unresolved 0.01).

Human disposition: retain the exact term. The assigned usage describes the Xray HandlerService account/user mutation path, not a host ABI or LLM selection. The external implementation verifies the documented preflight behavior. No assigned-crate edit is warranted. J1/J2's low-confidence outputs and J2 unresolved result are retained above rather than hidden.

## Coordinator follow-up — source-display gaps closed

`coordinator-followup.md` identified exact source-display gaps at
`crates/op-xray-daemon/src/commander_client.rs:1-8` and
`crates/op-xray-daemon/src/main.rs:1-39`. Both ranges were re-read completely.
Neither contains `native` or a native compound use. The follow-up therefore
introduced no new occurrence for JEV evaluation; no judgment was fabricated.
The original occurrence-level J1-J3 evidence and disposition above remain
unchanged.
