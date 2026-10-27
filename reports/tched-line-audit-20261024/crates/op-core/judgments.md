# op-core declarative JEV receipts

Task: `tched-line-audit-20261024/op-core`; auditor: Codex; date: 2026-10-24.

Installed interface read: `xd://eval/judge`. Actual calls used `functions.eval` JavaScript `await judge(state, questions)` only; no routing, model allocation, extraction, traversal or search. Two successful calls, four typed questions. The full source had already been read sequentially before judgments. Confidence below is returned confidence, not a runtime guarantee.

## J1 — Judging two plugin identifier occurrences

Source references: `crates/op-core/src/private_projection_directory_tests.rs:181-247`, `src/private_document_publication.rs:85-89`, `src/private_projection_directory.rs:117-119`; naming authority `AGENTS.md:45-52` and common assignment:56-75.

Exact contextual source supplied to JEV (line-number formatting/indentation normalized in input; source tokens unchanged):

```text
crates/op-core/src/private_projection_directory_tests.rs
181: #[test]
182: fn private_identity_lock_order_rejects_aliases_and_finishes_concurrent_revocation() {
219: let writer = namespace.clone();
220: let publication = std::thread::spawn(move || {
221: let result = writer.commit_document_durable_with_decision("tched_router", "output", |_| {
222: reading_tx.send(()).unwrap();
223: // Approved one-way acquisition only: private job file lock ->
224: // canonical identity reader. Identity writers never acquire jobs.
225: let credential = read_credential_in(&identity_owned, "identity_sled")?;
226: ensure!(credential.is_none(), "revocation did not become visible");
227: Ok(PrivateDocumentCommit::RetainPending {
228: durable: (),
229: error: anyhow::anyhow!("admission withdrawn"),
230: })
231: });
232: done_tx.send(()).unwrap();
233: result.unwrap()
234: });
235: reading_rx.recv_timeout(Duration::from_secs(3)).unwrap();
236: release_tx.send(()).unwrap();
237: done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
238: assert!(matches!(
239: revocation.join().unwrap(),
240: CredentialPublication::Committed { .. }
241: ));
242: assert!(matches!(
243: publication.join().unwrap(),
244: PrivateDocumentPublication::Pending { .. }
245: ));
246: assert!(namespace.read_snapshot(&["tched_router".into()]).is_err());
247: }

crates/op-core/src/private_document_publication.rs:85-89:
pub fn commit_document_durable_with_decision<T>(&self, plugin_id: &str,
operation_id: &str, commit: impl FnOnce(Option<&[u8]>) -> Result<PrivateDocumentCommit<T>>)

crates/op-core/src/private_projection_directory.rs:117-119:
Exact committed bytes for caller-selected sealed IDs;
pub fn read_snapshot(&self, plugin_ids: &[String]) -> Result<ProjectionSnapshot>
```

Authority supplied: filesystem `3tched-fs`, package `tched-fs`, plugin `tched_fs`, retained directory `crates/threetched-fs`; external executable/surface `threetched`; authoritative mount/plugin distinction explicitly names `tched_router`. Preserve sealed/contract plugin identifiers absent an explicit migration; no blanket rename.

Shared contrary criteria:

- `leave_as_is`: Exact declared plugin identifier tched_router used as a plugin ID, not ambiguous branding; preserve its spelling under the authoritative naming distinction.
- `replace_with_threetched`: External executable/surface prose or executable invocation currently incorrectly abbreviated; replace this occurrence with qualified threetched.
- `replace_with_3tched`: Our filesystem branding/projection name incorrectly abbreviated or confused with external surface; replace with qualified 3tched.
- `insufficient_evidence`: Cannot determine the intended referent or whether identifier preservation applies from supplied exact context.

Questions:

1. `occurrence_221`: Choose the naming disposition for the literal tched_router at private_projection_directory_tests.rs:221, considering actual API parameter intent and authoritative plugin identifier rule. Evaluate contrary replacement outcomes explicitly.
2. `occurrence_246`: Choose the naming disposition for the literal tched_router at private_projection_directory_tests.rs:246, considering its use in read_snapshot plugin IDs and relation to the pending publication above. Evaluate contrary replacement outcomes explicitly.

Returned receipt:

```json
{
  "occurrence_221": {"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}},
  "occurrence_246": {"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}}
}
```

Reader decision: both occurrences unchanged. They select the same fixture document key for a pending-publication/credential-revocation lock-order scenario, not a binary invocation or human-facing product label. No current/live catalog membership follows from the fixture name.

## J2 — Reconciling migration signal against fixture intent

Additional source: complete `SIGNALS.md:915-917` read using raw ranges after the sequential crate review. J2 state included the complete J1 state plus the following explicitly qualified evidence summary:

- SIGNALS:915, 2026-10-24, SchemaOwnership-3/Astra reports an isolated seal with71 entries, no tched_router, and a threetched schema; legacy caller/test migration gaps explicit; runtime acceptance unknown; STOP for owner review.
- SIGNALS:916 reports caller-migration ownership transfer and unfinished Chat/UI/router callers; earlier isolated seal does not verify current source; resume only after director release.
- These are historical agent claims, not independently verified by this worker. Source/runtime expansion is prohibited by this assignment. Contract identifiers remain protected absent an explicit migration in scope.
- Both actual op-core literals occur in a disposable lock-order fixture; no sealed catalog is loaded and no executable invoked. The API treats the literal as a flat plugin identifier. Full22-file source reading observed only these two tched substrings.

Question `signal_reconciliation`: Given BOTH the original exact source and the conflicting newer signal, choose the justified disposition of the two tched_router fixture plugin-ID occurrences for this limited naming audit. Preserve caller-migration ownership and avoid asserting current/live catalog facts from signals. Do these require renaming here, or retaining/reporting the conflict?

Contrary criteria:

- `leave_as_is`: Preserve both exact fixture plugin-ID literals under assignment contract exception; record newer migration claim and its unverified/current-source/ownership limits for coordinator, without asserting tched_router remains in current catalog.
- `replace_with_threetched`: Exact evidence independently establishes these fixture IDs must migrate to external threetched as an in-scope naming repair, despite shared migration ownership.
- `replace_with_3tched`: The fixture explicitly means our filesystem branding rather than a plugin-ID reference.
- `insufficient_evidence`: No justified disposition even with scoped preservation and conflict reporting is possible.

Question `static_limit`: Does full sequential source review and naming disposition without executing commands justify a tested/deployed or runtime-catalog correctness claim?

Criteria: `static_audit_only` = Justifies static coverage and source naming dispositions only; no test/build/deploy/live-catalog claim. `runtime_verified` = Justifies tested/deployed/live-catalog correctness.

Returned receipt:

```json
{
  "signal_reconciliation": {"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}},
  "static_limit": {"type":"choice","choice":"static_audit_only","confidence":1,"probabilities":{"static_audit_only":1,"runtime_verified":0}}
}
```

Reader decision: retain fixture IDs and report migration-signal disagreement for coordinator context. This is not a blocked naming repair and not a claim that tched_router remains present in a current catalog. No migration patch or cross-crate hunk is proposed from this generic fixture. No source change or behavioral verification performed.
