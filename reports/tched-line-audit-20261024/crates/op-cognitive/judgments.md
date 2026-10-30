# Declarative JEV receipts

Installed `judge` documented by `xd://eval/judge`; eval used only for declarative judgments over exact already-read evidence. No routing, filtering, source extraction or tool enablement.

## Receipt J1 — filesystem branding and ordinary English batch words

Tool: functions.eval / JavaScript / title `Judging filesystem and batch contexts`.
Date: 2026-10-24. Two independent questions over one supplied evidence state.

Evidence:
- `crates/op-cognitive/SPEC.md:26-32`: "This source cutover is not part of the already-built deployment. A release / using it must compile the affected consumers, seal and persist a matching / catalog, and integrate the matching environment, capability grants, MCP toolset / manifest and mounted workspace links together. Regenerate catalog-pinned / 3tchedFS specialization artifacts from that catalog; never hand-edit their / schema hashes. Historical receipts and retired standalone-service denylists / retain their original names. No compatibility shim or second listener is added."
- `crates/op-cognitive/src/chain_vectors.rs:499-506`: "Backfilled vectors (a vector attached to an older block after the / watermark moved past it) and points lost from the disposable index are / caught by a gap check: whenever the watermark leaves any vector / unaccounted for, one batched retrieve per 256 blocks confirms what is / actually present and re-upserts only the difference." Context continues with persisted watermark resume.
- `crates/op-cognitive/src/chain_vectors.rs:603-606`: "Block numbers whose point is absent from the index, asked in one batched / retrieve per chunk rather than by scrolling the whole collection." Followed by `missing_from_index` and CHUNK=256.

Questions and explicit criteria:
- `spec_fs`: "Disposition of the single 3tchedFS occurrence SPEC.md:30: judge intended filesystem naming, not release readiness." Choices: leave_as_is (correct qualified filesystem name); replace_with_threetched (external surface); replace_with_3tched (incorrect filesystem name); insufficient_evidence.
- `batch_word`: "Disposition of both literal tched substrings inside batched at chain_vectors.rs:502 and603; context is Qdrant gap retrieval." Same choices; leave_as_is explicitly means ordinary English chunked retrieval, not branding.

Actual response:
```json
{"spec_fs":{"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}},"batch_word":{"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}}}
```

Decision: leave all three occurrences unchanged. SPEC names our FS specialization artifacts; both batched words mean batched Qdrant retrieval, not either product. No source edit needed.

## Receipt J2 — retrieval verbs and qualified external fixtures

Tool: functions.eval / JavaScript / title `Judging retrieval words and fixture identifiers`.
Date: 2026-10-24. Two independent questions over one evidence state.

Exact source evidence supplied:
```rust
// crates/op-cognitive/src/qdrant_shuttle.rs:145-152
tracing::info!(
    trace_id = %trace.trace_id,
    mutation_index = trace.mutation_index,
    matches = response.result.len(),
    "Accountability Loop fetched semantic trace episodes"
);
Ok(response.result)

// crates/op-cognitive/src/qdrant_shuttle.rs:190-199
tracing::info!(
    trace_id = %trace.trace_id,
    mutation_index = trace.mutation_index,
    schema_name = %extract_schema_title(&schema_query_text),
    matches = response.result.len(),
    model = %self.voyage_client.model(),
    "Accountability Loop fetched semantic matches from the shared-memory schema projection"
);
Ok(response.result)

// crates/op-cognitive/src/qdrant_shuttle.rs:572-584
#[test]
fn plugin_id_to_uuid_is_stable() {
    assert_eq!(
        plugin_id_to_uuid("threetched"),
        plugin_id_to_uuid("threetched"),
        "same plugin id must produce the same UUID every time"
    );
    assert_ne!(
        plugin_id_to_uuid("threetched"),
        plugin_id_to_uuid("antigravity"),
        "different plugin ids must not collide"
    );
}
```

Questions:
- `fetched_word`: "Disposition of fetched at qdrant_shuttle.rs:149 and196. Determine exact contextual intent." Choices: leave_as_is (ordinary retrieval verb); replace_with_threetched (incorrect external branding); replace_with_3tched (incorrect filesystem branding); insufficient_evidence.
- `external_fixture`: "Disposition of all three threetched string occurrences at qdrant_shuttle.rs:575,576,580; fixture inputs to deterministic plugin UUID generation." Choices: leave_as_is (qualified external/plugin fixture, no filesystem intent); replace_with_threetched (unqualified external brand); replace_with_3tched (incorrect FS name); insufficient_evidence.

Actual response:
```json
{"fetched_word":{"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}},"external_fixture":{"type":"choice","choice":"leave_as_is","confidence":0.96,"probabilities":{"leave_as_is":0.97,"replace_with_threetched":0.03,"replace_with_3tched":0,"insufficient_evidence":0}}}
```

Decision: leave all five occurrences. Retrieval log verbs are not brand names. All three fixture strings already use the qualified external spelling; no filesystem intent is shown.

## Receipt J3 — qualified container fixture

Tool: functions.eval / JavaScript / title `Judging mail fixture naming`.
Date: 2026-10-24.

Exact source evidence: `crates/op-cognitive/tests/e2e_voyage_qdrant.rs:112-125`.
```rust
// ── 1. Voyage vectorization ──────────────────────────────────────────
let ovs_doc = "The rovs plugin drives Open vSwitch natively over OVSDB JSON-RPC: \
               it creates bridges, attaches ports, and installs OpenFlow rules.";
let mail_doc = "The mail-3tched container runs Postfix and Dovecot, exposing SMTP \
                submission on 587 and IMAPS on 993 for mailbox delivery.";

let ovs_vector = shuttle
    .embed_document(ovs_doc)
    .await
    .context("Voyage embed_document failed for the OVS document")?;
let mail_vector = shuttle
    .embed_document(mail_doc)
    .await
    .context("Voyage embed_document failed for the mail document")?;
```

Question `mail_fixture`: "Choose disposition of the single mail-3tched occurrence at line115. Is this a fixture container name requiring naming correction? Do not infer deployed existence from a test string." Choices: leave_as_is (already-qualified 3tched container fixture; no external intent/demonstrated defect); replace_with_threetched (external executable); replace_with_3tched (incorrect unqualified filesystem spelling); insufficient_evidence.

Actual response:
```json
{"mail_fixture":{"type":"choice","choice":"leave_as_is","confidence":0.77,"probabilities":{"leave_as_is":0.83,"replace_with_threetched":0.01,"replace_with_3tched":0.01,"insufficient_evidence":0.15}}}
```

Decision: leave_as_is. Source is an embedding contrast document, not a binary invocation or filesystem mount declaration. Existing `3tched` qualification is consistent with owner naming rules; a container rename or asserted external intent is unsupported. Confidence is lower than other judgments; no deployed container claim is made.

## Receipt J4 — supplemental internal accounting and claim boundary

Tool: functions.eval / JavaScript / title `Checking audit receipt consistency`.
State: all 35 already-read inventory line counts (14,75,81,380,145,512,136,27,284,52,202,188,1108,550,893,1167,5,66,217,475,1190,65,69,769,686,140,1316,90,205,235,257,154,442,139,261); claimed total12595; explicit O1–O9 locations/tokens and J1–J3 answers/confidences; no source edits; only reports/shared bookkeeping; no commands/build/tests/runtime; initial hashes not recomputed and concurrency limitation.

Questions:
- `coverage_consistency`: "Given this explicit coverage and per-occurrence evidence, is the static completion accounting internally consistent? This is not independent verification of source absence or reading." Choices: consistent (35 counts total12595; nine rows map to judgments; nothing unread); inconsistent (numerical or mapping contradiction); insufficient_evidence.
- `claim_boundary`: "Which completion claim does this evidence justify?" Choices: static_audit_only (assigned static audit, no source changes, hash/concurrency limits); built_tested_deployed; incomplete_audit (unread/unjudged entries).

Actual response:
```json
{"coverage_consistency":{"type":"choice","choice":"inconsistent","confidence":0.37,"probabilities":{"consistent":0.21,"inconsistent":0.58,"insufficient_evidence":0.21}},"claim_boundary":{"type":"choice","choice":"static_audit_only","confidence":0.89,"probabilities":{"static_audit_only":0.93,"built_tested_deployed":0,"incomplete_audit":0.07}}}
```

Resolution: accounting confidence0.37 is below the required0.6 threshold; it is NOT accepted as an answer. Manually reconciled every supplied file count against read endpoints and summed in inventory groups: first5 files695; next7 files1401; next4 files3718; next7 files2087; final12 files4694. These cover5+7+4+7+12=35 entries and695+1401+3718+2087+4694=12595 lines. O1/J1 spec_fs; O2–O3/J1 batch_word; O4–O5/J2 fetched_word; O6–O8/J2 external_fixture; O9/J3 mail_fixture maps all9 rows to five naming choices. No missing file/occurrence was identified. This manual accounting, not the weak judge response, supports the stated totals. The claim boundary answer supports static-audit-only reporting; runtime readiness remains unverified.
