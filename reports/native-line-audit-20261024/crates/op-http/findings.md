# op-http native contextual audit findings

Task `native-line-audit-20261024/op-http`; Codex crate worker; 2026-10-24.

## Native-specific outcome

All eight inventory files fully read sequentially through EOF: 1,259 observed/supplied lines. Root and src directory listings agree with inventory; no unlisted files or nested instructions encountered. All entries are readable text; no missing, binary, symlink or dependency entry required separate exclusion. Inline TLS regression test and both documentation files included.

No `native` occurrence observed, including case/compound use. No independent native-specific provider/model/config override, duplicated plugin schema or admission bypass established. No crate source edits, proposed cross-crate hunk or tched/threetched overlap.

JEV source classifications: J1 HTTP composition only; J2 TLS transport configuration with generic plaintext risk; J3 stale documentation inventory. All three successful declarative answers returned confidence 1. Full criteria, contrary outcomes, evidence paths and results in `judgments.md`. The zero-occurrence result rests on full read coverage, not a JEV claim of absence.

## Generic observations — outside native-only repair scope

### G1 — HTTPS configuration does not require encrypted transport

Source: `src/server.rs:74-101,109-140`, `src/tls.rs:91-101`. JEV J2.

When a TLS acceptor exists, the server binds both listeners and serves a clone of the same router on plaintext HTTP while separately accepting HTTPS. Auto mode returns `None` when certificate detection finds nothing, allowing HTTP-only serving. [INFERENCE] Callers relying on enabling TLS to prohibit plaintext credential/request transport could be exposed unless external admission/network policy constrains HTTP. Caller usage and deployed network exposure were not inspected or exercised; this is not proof of a live vulnerability.

Disposition: source-only/report-only. A separate authorized transport-policy task would need to define whether HTTP is disabled or redirect-only and whether Auto must fail closed. No native repair is implicated.

### G2 — historical source inventory no longer matches files

Source: `SPEC.md:19-29,68-78`; `compare-op-http.md:13-21,31-51,77-78`; `src/lib.rs:22-25`; complete root/src listings. JEV J3.

Docs list eight Rust source files and mark request_filters.rs, metrics.rs and health.rs present/implemented, while current inventory/listings contain five Rust sources and four root-declared modules. The comparison also says no development dependencies, while Cargo.toml:41-42 declares tempfile. These are generic stale-documentation claims; no native terminology is present.

Disposition: preserve unrelated historical documentation; report for separate reconciliation. Do not invent absent modules.

## Verification and limitations

Verification consists of complete sequential static reads, inventory/listing reconciliation and three successful contextual declarative JEV judgments. No searches, shell/process execution, Cargo, build, tests, lint, formatting, custom smoke, runtime operations, seal/deploy/grant/credential changes, recursive delegation, staging or commits. No build/test/deployed claim. HEAD was not queried; coverage hashes remain supplied initial inventory receipts, not independently recomputed current hashes. No source mutation required a BEFORE copy.

Required shared HANDOFF bookkeeping is the sole shared-file exception to the assignment's report-only bookkeeping restriction, mandated by higher-priority session rules; unrelated workers' claims preserved.

## Handoff

Coordinator: reconcile `coverage.tsv`, `occurrences.tsv`, `judgments.md` and this report with other crates. No op-http native patch needs a build. Generic observations require separate scope; no native blocker remains.

## Self-evaluation

| Axis | Score | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4/5 | Exact source references and returned JEV values retained; runtime/caller/dependency behavior not certified. |
| Completeness | 4/5 | All eight files and every line covered; supplied hashes deliberately remain initial receipts rather than present-state checksums. |
| Clarity | 4/5 | Native result separated from generic G1/G2; receipt file is longer than the conclusion. |
| Actionability | 4/5 | No patch to reconcile; G1 identifies specific policy choices but needs caller/deployment scope before repair. |
| Conciseness | 4/5 | Compact ledgers plus full declarative evidence; some traceability duplication across reports. |

Overall 4.0/5. Improvements: coordinator can add authorized caller/deployment evidence for G1 and separately reconcile historical docs. These are not omitted native work. User agreement expected for the scoped static result, not a general security certification.
