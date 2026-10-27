# op-core tched naming findings

Task `tched-line-audit-20261024/op-core`; Codex; 2026-10-24.

## Outcome

Complete static naming audit:22/22 supplied inventory files,5,187 supplied newline-counted/observed source lines read sequentially through EOF. Root, src and tests listings reconcile with inventory. No extra nested directory/context file, binary, missing, symlink or dependency entry was encountered. Coverage includes manifest, generated historical documentation, all production source, inline tests and separate test files; no size exclusion or search.

Only two literal `tched` substrings observed. Both remain unchanged after actual declarative JEV judgments. No source edits; no independent native-audit overlap; no cross-crate proposed hunk. Other workers' source and reports preserved.

## Complete occurrence ledger

| Occurrence | Source | Exact containing text | Intent | Disposition | Evidence |
| --- | --- | --- | --- | --- | --- |
| O1 | `crates/op-core/src/private_projection_directory_tests.rs:221` | `writer.commit_document_durable_with_decision("tched_router", "output", |_| {` | Fixture plugin identifier for the document whose publication remains pending after credential revocation | `leave_as_is` | J1 occurrence_221 confidence1; J2 migration-signal reconciliation confidence1 |
| O2 | `crates/op-core/src/private_projection_directory_tests.rs:246` | `assert!(namespace.read_snapshot(&["tched_router".into()]).is_err());` | Same fixture plugin identifier; assertion rejects reading pending private document | `leave_as_is` | J1 occurrence_246 confidence1; J2 migration-signal reconciliation confidence1 |

Counts: leave_as_is2; replace_with_threetched0; replace_with_3tched0.

`commit_document_durable_with_decision` accepts `plugin_id` (`private_document_publication.rs:85-89`); `read_snapshot` accepts selected plugin IDs (`private_projection_directory.rs:117-125`). The literal is not human-facing abbreviated product branding, a mount path or binary invocation. Both fixture uses must agree. Assignment explicitly preserves contract/plugin identifiers absent an authorized migration; no migration authority is inferred merely from spelling.

## Current-source versus migration-signal distinction

`AGENTS.md:45-52` still explicitly names `tched_router`. Newer `SIGNALS.md:915` reports its retirement from an isolated catalog and replacement by threetched; `:916` reports transferred/unfinished caller migration ownership and explicitly says the prior seal does not verify current source. These sources disagree about inventory/currency; neither establishes this worker's current/live catalog. J2 considered this contrary evidence and retained the exact fixture keys within the assignment contract exception.

The op-core fixture loads no sealed catalog and invokes no external executable; it exercises generic filesystem lock/publication behavior using temporary owner directories. Retaining its key does NOT certify runtime plugin presence. Coordinator can reconcile this context with its separately owned plugin/caller migration. No concrete source repair or cross-crate migration is justified by this fixture alone.

## Coverage and verification boundaries

- `coverage.tsv`: every supplied inventory entry, complete sequential ranges actually read, initial SHA-256 receipts, disposition/status. Hashes copied from supplied inventory, not recomputed; HEAD unqueried.
- `judgments.md`: exact source/context criteria, four successful typed answers across two installed declarative JEV calls; all confidence1.
- Source absence in the other21 files is reader-observed through complete sequential reading, not excerpt-based JEV certification.
- No searches, extraction, shell/process/network commands, recursive delegation, Cargo/build/test/lint/format/smoke/runtime/deploy/seal/grant/credential operations.
- No behavioral change was made. No tested, built, installed, deployed, live-catalog or full-system correctness claim.

## Handoff

Next: coordinator reconcile full coverage, the two preserved fixture IDs, migration-signal limits and JEV/result receipts. No naming patch from this worker requires a build. Shared HANDOFF bookkeeping is required by higher session instructions; no shared SIGNALS mutation.

## Self-evaluation

Accuracy4/5: complete read and exact occurrence/JEV receipts; hashes and live catalog are not independently checked, explicitly disclosed. Completeness5/5: all22 inventory files and both occurrences dispositioned. Clarity4/5: paths/ranges and contrary signal explicit; evidence repetition retained for reproducibility. Actionability4/5: ready for coordinator reconciliation; separate migration state remains coordinator-owned. Conciseness4/5: findings short; detailed judge receipts intentionally separate. Overall4.2/5. Improvement: coordinator should reconcile latest authoritative migration state rather than infer it from this static fixture. Expected user assessment: complete scoped static audit, no runtime certification.
