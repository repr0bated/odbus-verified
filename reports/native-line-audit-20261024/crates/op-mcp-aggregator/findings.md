# Native audit findings — op-mcp-aggregator

Task native-line-audit-20261024/op-mcp-aggregator; Codex crate worker; 2026-10-24.

## Outcome

Complete static native audit. All 14 inventory files read sequentially through EOF, 5,660 supplied/observed lines. Root, src and examples listings reconcile; no binary, missing, symlink, external dependency file or unlisted nested directory encountered. No case-insensitive `native` substring occurrences observed, including compounds, identifiers, documentation and fixture text. No independent native-specific defect, crate source edit, proposed cross-crate hunk or tched/threetched overlap.

`coverage.tsv` records every file, actual contiguous read ranges and supplied initial hash. Initial hashes were not recomputed; they are inventory provenance, not proof against later concurrent changes. Current HEAD was not queried. No source edit required a BEFORE copy.

One successful installed declarative JEV call returned three choices (judgments.md): tool-presentation-only selection confidence1; generic profile-boundary gap0.95; generic documentation drift0.94. Full-read observation supports the zero-occurrence result; JEV excerpts are not represented as an independent whole-crate absence certification.

## Native-related contextual decision

`src/config.rs:506-608`, `src/aggregator.rs:305-365`: client-name detection selects Compact/Full/Hybrid tool presentation. It does not select an AI provider/model in this code. Preserve; no blanket renaming or speculative routing patch. Vertex capability wording in the log is not proof of negotiated capability and was not verified. No external caller/provider implementation or deployment reviewed.

## Separate generic observations — no native patch

### G1 — exported standalone wrappers differ from profile-bound handler

`src/compact.rs:319-342,394-418,620-686` uses unrestricted `Aggregator::call_tool` or raw cache lookup; `ListToolsTool` accepts a profile from input at178-205. In contrast, `src/aggregator.rs:197-218,497-518,639-675` uses an embedding-selected profile and `ProfileManager::authorized_tool_in_profile`; its authorization resolves exact owner from one snapshot (`src/profile.rs:98-135`). Standalone execute/batch wrappers do not locally apply this profile authorization. J2 confirms the execute-call distinction; other cited paths were fully reader-reviewed.

Risk only if these standalone wrappers are exposed where profile-limited admission is expected. Their actual external consumers, exposure and caller identity were not examined; this is not evidence of a live upstream capability bypass. Generic follow-up would first trace the embedding owner and caller contract. No change authorized by this native audit.

### G2 — stale generic documentation

- README.md:51,193 advertises stdio/websocket; client.rs:107-108,162-170 rejects non-SSE construction.
- SPEC.md:21,37,71 and compare-op-mcp-aggregator.md:13,26,32,38,42,61,85 list removed unused/context.rs, patch artifact, ninth Rust file and op-plugins dependency. Current complete listings have eight Rust files; Cargo.toml:10-12 only op-core/op-tools internal dependencies.
- CLEANUP-CONTEXT-AWARE.md:17-32 says groups was removed; current lib.rs:76 exports groups and groups.rs is present.
- README.md:29 and examples/aggregator.json:81-86 claim Gemini compact defaults, whereas config.rs:515-521 forces Full while detection enabled. Generic tool-presentation drift, not provider/model routing.

J3 classifies transport/SPEC discrepancies as generic report-only. No broad documentation cleanup performed.

## Verification and deployment

Verification performed: complete sequential source/document/fixture/test reads, directory parity review, actual declarative JEV source judgments and saved report/result receipts. No searches, extraction, shell commands, Cargo/build, tests, lint, formatting, smoke fixtures, runtime, network operations, deploy/seal/grant/credential changes, staging/commits/reset/stash or recursive delegation. No built, tested, installed or deployed claim. No behavioral source changes to exercise. Coordinator handles reconciliation and any separately authorized build work.

## Handoff

Next action: coordinator reconcile 14 complete coverage rows, explicit zero-occurrence ledger, three actual JEV answers and result.json. No native patch from this crate requires building. Shared HANDOFF bookkeeping follows higher-priority session requirements despite the common assignment's shared-file restriction; other claims preserved.
