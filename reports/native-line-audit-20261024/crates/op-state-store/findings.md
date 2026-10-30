# op-state-store native contextual audit findings

Task `native-line-audit-20261024/op-state-store`; Codex crate worker; 2026-10-24.

## Native-specific disposition

No native-specific defect established. No source changes or proposed external/tched hunks.

Complete sequential coverage: 18/18 inventory files, 8,723 observed source/document/manifest lines, through EOF. Root and src directory listings match all 18 entries; no nested directories, nested guidance, binaries, symlinks, missing files or dependencies required individual exceptions. The supplied inventory's hashes are initial receipts, not current hash verification; reads are not an atomic snapshot.

Occurrence ledger: 20 rows = 19 semantic native uses plus one conservative lexical-only substring in “alternatives”. Every occurrence has its actual source context supplied to installed declarative JEV and maps to a retained judgment:

| Context | Count | Decision / evidence |
| --- | ---: | --- |
| exact_schema.rs:16 — native validator diagnostics | 1 | Retain upstream/library-original diagnostic wording; fixed-code ExactSchemaError and is_valid boundary, J1.diagnostics confidence0.94 |
| exact_schema.rs fixture keys/pointers at542,546,551,556,557,558,564,566,569,573,615,617,631,741,760,761 | 16 | Retain consistently bound exact-schema fixture names, J1.fixtures confidence0.92 |
| schema_validator.rs:192 — natively | 1 | Retain direct keyword support wording for propertyDependencies translation helper, J2.natively confidence0.95 |
| state_schema_shape.rs:462 — native tagged device | 1 | Retain descriptive original-schema fixture annotation; no product/runtime authority claim, J2.tagged_fixture confidence0.98 |
| schema_projection.rs:82 — alternatives | 1 lexical | Retain ordinary schema-branch wording, conservative any-branch redaction matches sentence, J2.lexical_alternatives confidence0.92 |

No occurrence selects a provider/model, overrides admitted configuration, creates schema authority or claims a native billing integration. Fixture identifiers are not deployed contract evidence. Preserve all existing source and unrelated edits.

## Separate generic observations — not native defects

- SPEC.md:19-55,70-83 and compare-op-state-store.md:13-42,48-91 describe missing sqlite_store/redis_stream modules and retired dependencies. Current manifest and complete directory/source reads contradict that inventory. No native occurrence in either document; no unrelated documentation rewrite authorized.
- disaster_recovery.rs:345-407 marks packages successful after Resolve or InstallPackages method acceptance without consuming transaction completion signals;419-449 equates SearchNames acceptance with installed-package existence;521-526 records plugins_restored while actual state application is explicitly delegated elsewhere. Static receipt/wording risks only; no runtime reproduction or native-specific relation. Left unchanged.
- plugin_schema.rs:1619-1628 accepts any matching OneOf branch, although type docs48-51 say exactly one. Unrelated generic schema-semantic debt, not a native occurrence. No schema repair included.

These observations do not constitute a complete general correctness/security review or new runtime failures.

## Verification and limits

Evidence: complete sequential file reads with anchored tool receipts; directory-to-inventory reconciliation; 3 actual installed declarative JEV calls / 6 typed contextual answers (all retain, confidence0.83–0.98). A separate compact-summary jev_verify returned insufficient0.43 and was not accepted; J3 resupplied original actual contexts and resolved the bounded native disposition at0.83. Full questions, contrary outcomes, source references and answers: judgments.md. Coverage and per-occurrence dispositions: coverage.tsv and occurrences.tsv.

No searches, source extraction, arbitrary eval execution, shell/process/network commands, Cargo/build/check/tests/lint/format/smoke fixtures, runtime operations, deployment, sealing, grants/credentials, staging/commits/reset/stash or recursive delegation. Eval used only declarative JEV over already-read source. No built/tested/installed/deployed claim. HEAD/current hashes unqueried. Shared HANDOFF bookkeeping is the only outside-exclusive-report mutation, required by higher-priority session instructions.

Next: coordinator aggregates the completed static audit. No op-state-store native patch requires a build.
