# Every-occurrence context-and-intent JEV supplement

Task `native-line-audit-20261024/op-core`; 2026-10-24; same crate worker/session.

Actual successful call: installed `judge(occurrence_state, occurrence_questions)` through Python eval, transcript title **Judging every op-core occurrence**. `xd://eval/judge` read again after runtime-loss notice; no reliance on prior eval variables. One shared state containing four separately identified occurrence contexts, four independent typed-choice questions. No routing/model selection, search, source extraction, filesystem operation or arbitrary execution through eval.

This supplement supersedes using the earlier routing-only `native_usage` result as sufficient occurrence disposition. Every row of [occurrences.tsv](occurrences.tsv) maps to its own question here. Counts conservatively include native embedded in Alternative/alternative: four lexical occurrences, of which two are actual standalone/compound native vocabulary and two are ordinary-English substring matches. Full 22-file coverage is retained in [coverage.tsv](coverage.tsv); complete sequential rereads of the four occurrence-bearing files confirmed these contexts, without claiming another full-crate reread.

No coordinator-followup.md was present in the assignment directory listing. No needed native-specific correction remains; no source edits or runtime verification.

## Shared criteria and scope supplied

State scope: op-core static audit, every native occurrence including case/compound and conservative embedded substring accounting; assess actual wording, intended meaning and behavior accuracy, not merely absence of AI routing. Prior full sequential coverage of all 22 files is retained. Tests not run; no deployment claim.

Each question explicitly offered **preserve_accurate**, **change_needed**, and **insufficient**. Per-question rubric and actual probabilities follow.

## OC1 context intent

Source: `crates/op-core/src/config.rs:53-84,86-143`; occurrence `:56`, token **Alternative**, native at token characters 6–11.

Exact occurrence and declaration supplied:

```rust
/// Alternative paths to check (in order of priority)
pub const ENV_FILE_PATHS: &[&str] = &["/etc/op-dbus/environment", "/etc/op-dbus.env", ".env"];
```

The actual `load_environment` function was also supplied, including the earlier explicit `OP_ENV_FILE` check, the loop `for path in ENV_FILE_PATHS`, and immediate return after the first successful `try_load_env_file`. Boundary supplied: the comment modifies ENV_FILE_PATHS, not the entire loader priority; custom OP_ENV_FILE is explicitly checked before the array. `try_load_env_file` returns None for missing/unreadable files. Native is an embedded substring of Alternative, not a separate adjective.

Exact question: Assess OC1 wording and behavior. Is Alternative an ordinary English label accurately describing the listed fallback candidates and their order? Consider custom OP_ENV_FILE separately. Does the embedded native substring need correction?

Criteria:
- preserve_accurate: Ordinary English alternative; array order is faithfully documented; no misleading native product claim or needed change in this occurrence.
- change_needed: This occurrence inaccurately describes the listed paths/order or improperly claims native behavior; needs a concrete wording/code correction.
- insufficient: Evidence cannot establish meaning or accuracy.

Actual result: **preserve_accurate**, confidence **0.97**; probabilities preserve_accurate **0.98**, change_needed **0.02**, insufficient **0**.

Decision: preserve. The comment accurately labels ordered configuration fallback candidates. No native product/backend/platform assertion.

## OC2 context intent

Source: `crates/op-core/src/identity_projection_transaction_tests.rs:23-29,66-114`; occurrence `:72`, token **native**.

Exact test prefix supplied:

```rust
#[test]
fn uncertain_callback_blocks_identity_but_preserves_unrelated_transactions() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let result =
        commit_identity_projection_durable_in::<()>(root(&dir), gid(), "uncertain", |_| {
            bail!("native commit outcome uncertain")
        })
        .unwrap();
    assert!(matches!(
        result,
        CredentialPublication::Pending { durable: None, .. }
    ));
    unavailable(&dir);
```

Boundary supplied: unavailable asserts strict identity credential and snapshot reads and both ordinary identity updater APIs fail. Remaining test successfully writes unrelated state, rejects recovery token wrong without invoking its callback, then recovers the exact uncertain token. This is a simulated error from an authoritative durable-store callback, not an actual database executed by the test. Production helper calls `commit(old_private.as_deref())` and on Err returns `CredentialPublication::Pending { operation_id, durable: None, error }`. Tests are cfg(test) in identity_projection_transaction.rs.

Exact question: Assess OC2 fixture wording and intent, not only model routing. Does native commit outcome uncertain reasonably describe the simulated authoritative durable-store failure in this test, without claiming an actual native database commit was executed?

Criteria:
- preserve_accurate: Accurate synthetic storage-error vocabulary in explicit fixture context; intended uncertain durable callback path is represented; no correction needed.
- change_needed: Wording misleadingly asserts a different backend/operation or fails to represent the fixture's intended behavior; change warranted.
- insufficient: Context cannot establish intent or wording accuracy.

Actual result: **preserve_accurate**, confidence **0.92**; probabilities preserve_accurate **0.95**, change_needed **0.05**, insufficient **0**.

Decision: preserve. Error text describes the simulated uncertain durable-store boundary; it does not claim a real database operation ran. No model/provider/product coupling.

## OC3 context intent

Source: `crates/op-core/src/private_document_publication.rs:146-168,171-255,275-315`; occurrence `:173`, token **alternative**, native at token characters 6–11.

Exact recovery documentation and wrapper supplied:

```rust
    /// Exact-operation recovery invokes the authoritative durable reader. It
    /// never passes potentially mixed SHM bytes to the callback and has no
    /// clear-marker/old-state rollback alternative. On process activation an
    /// owner must recover/reconcile durability before enabling private reads;
    /// volatile SHM cannot retain markers across machine reboot.
    pub fn recover_document<T>(
        &self,
        plugin_id: &str,
        operation_id: &str,
        load_durable: impl FnOnce() -> Result<(Option<Vec<u8>>, T)>,
    ) -> Result<PrivateDocumentPublication<T>> {
        self.recover_document_with_decision(plugin_id, operation_id, || {
            load_durable()
                .map(|(current, durable)| PrivateDocumentCommit::Publish { current, durable })
        })
    }
```

Also supplied verbatim: exact pending plugin/operation `ensure!` at :208-215, and the recovery Abort branch at :243-249 returning Pending with error `private recovery owner did not provide an authoritative durable row`.

Boundary supplied: `recover_document_with_decision` calls load_durable() with no SHM predecessor. It only checks old file safety; Publish calls finish with previous=None and current from durable callback. finish writes current, syncs data, and clears document_pending at commit. RetainPending/error/Abort keep recovery Pending. Distinct `commit_document_durable_with_decision` permits Abort only for proven pre-effect rejection, not recovery. Native is embedded inside alternative; this comment is scoped to recovery.

Exact question: Assess OC3 documentation against recovery implementation. Does 'no clear-marker/old-state rollback alternative' accurately describe this recovery API? Distinguish separate pre-effect commit Abort. Is the native substring merely part of ordinary English alternative?

Criteria:
- preserve_accurate: Recovery requires exact token and authoritative durable return; Abort cannot clear recovery gate; old SHM is not passed as predecessor; alternative is ordinary English and wording is accurate in scope.
- change_needed: Recovery offers a clear-marker/old-state rollback route contradicting the comment, or the occurrence otherwise needs correction.
- insufficient: Evidence insufficient to assess documentation accuracy.

Actual result: **preserve_accurate**, confidence **0.98**; probabilities preserve_accurate **0.99**, change_needed **0.01**, insufficient **0**.

Decision: preserve. The recovery API's data flow and Abort branch match its documentation. Trusted durable callback correctness remains the upstream owner's obligation; source does not prove runtime admission or durability.

## OC4 context intent

Source: `crates/op-core/src/projection_transactions_tests.rs:8-42,109-155`; production hook `crates/op-core/src/projection_transactions.rs:364-415` retained full-read evidence. Occurrence `:8`, token **native-write**.

Exact occurrence supplied:

```rust
// Inject a single native-write failure at the file/manifest boundary. This
// leaves rollback writes operational, exercising the public transaction API.
thread_local! {
    static FAILURE: RefCell<Option<(String, usize, bool)>> = const { RefCell::new(None) };
}
```

Full `write_fault` body at :14-38 was supplied, including matching target and countdown, consuming `*fault = None` before returning a failure, optional panic `simulated publisher interruption before commit`, and error `injected file publication failure`.

Boundary supplied: production write_file calls `#[cfg(test)] tests::write_fault(name)?` before temp-file allocation/write/fsync/rename. Single-shot injection is removed before its error, so later rollback writes are not reinjected. `file_and_manifest_failures_restore_previous_committed_bytes` injects plugin.json / manifest write failures and checks old bytes/generation. Separate panic mode tests interruption rather than successful rollback. Comment describes simulated filesystem publication failure, not a claim that it hooks an actual OS syscall or all failure cases.

Exact question: Assess OC4 native-write failure comment in actual injection context. Is it accurate storage-boundary fault-simulation vocabulary? Consider single-shot clearing, rollback availability and separate panic mode. Do not mistake a simulated failure for a claim to intercept an actual syscall.

Criteria:
- preserve_accurate: Single-shot test-only file/manifest publication fault leaves rollback uninjected; native-write reasonably describes this simulated low-level write boundary; no wording/behavior correction needed.
- change_needed: Comment falsely claims native-write behavior or rollback availability in the described failure path; a concrete change is needed.
- insufficient: Evidence cannot resolve contextual intent/accuracy.

Actual result: **preserve_accurate**, confidence **0.98**; probabilities preserve_accurate **0.99**, change_needed **0.01**, insufficient **0**.

Decision: preserve. Correct fault-simulation vocabulary; no assertion of actual OS syscall interception. The comment's ordinary error path leaves rollback uninjected; optional panic tests a distinct interrupted-publication scenario.

## Raw successful returned answers

```json
{
  "OC1_context_intent": {"type":"choice","choice":"preserve_accurate","confidence":0.97,"probabilities":{"preserve_accurate":0.98,"change_needed":0.02,"insufficient":0}},
  "OC2_context_intent": {"type":"choice","choice":"preserve_accurate","confidence":0.92,"probabilities":{"preserve_accurate":0.95,"change_needed":0.05,"insufficient":0}},
  "OC3_context_intent": {"type":"choice","choice":"preserve_accurate","confidence":0.98,"probabilities":{"preserve_accurate":0.99,"change_needed":0.01,"insufficient":0}},
  "OC4_context_intent": {"type":"choice","choice":"preserve_accurate","confidence":0.98,"probabilities":{"preserve_accurate":0.99,"change_needed":0.01,"insufficient":0}}
}
```
