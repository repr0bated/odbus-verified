# op-identity — native contextual audit

Task: `native-line-audit-20261024/op-identity`. Date:2026-10-24. Owner:Codex crate worker.

## Outcome

Complete static review of all20 supplied inventory files,4549 lines, including17 Rust files, manifest,2 documents, inline/separate fixtures and the included deprecated WireGuard module. Root/src/bin directory listings agree with inventory; no missing, unreadable, binary or symlink entry encountered, no nested instruction file in these listings. Every file read sequentially through EOF; exact ranges and supplied initial SHA-256 receipts in `coverage.tsv`. Hashes not recomputed; no claim that initial hashes establish current byte identity.

Eight literal case/compound native occurrences, individually mapped to actual declarative JEV judgments. Seven digest uses are preserved; the header-helper occurrence received a minimal documentation correction after authoritative public configuration docs established the option but not per-request invocation timing. No native-specific unresolved blocker or overlap remains.

## Native dispositions

- N1: `src/bin/op-identity-headers.rs:4` — `native` identifies Codex's documented built-in `http_headers_helper` option. Official public config reference lines671-675 documents a local command returning JSON headers for locally connected HTTP MCP servers, but says nothing about invocation timing. J4 selects correction with confidence0.72; removed the unsupported “before each ... request” assertion while preserving native terminology and the helper's exact identity/admission behavior. This public documentation is not installed-version or live-invocation proof.
- N2–N8: `src/session_genesis.rs:23,25,62,66,69,70,73` — native denotes actual producer digest width/representation and fixture value. `mint_genesis` hashes the supplied slice unchanged; fixture distinguishes16-byte input from changed byte,32 zero bytes and padded32 bytes. Supporting EventChain source uses MD5 hex for genesis/event hashes, agreeing with16 decoded bytes. J2 selects preserve with confidence0.79. No fixture execution or unread producer validation certified.

## Separate generic observation — G1

`SPEC.md:19-28,40-41,49-52,63` and `compare-op-identity.md:13-20,34,46,54` are stale generated inventory: claim token.rs, keyring/rusqlite,7 Rust files/no binaries/no internal dependency. Current listings have17 Rust sources/no token.rs/two binaries; Cargo.toml declares op-identity-headers and op-core and no keyring/rusqlite. J3: generic stale inventory, confidence0.99. Report-only; these documents contain no native occurrence. No unrelated cleanup performed.

## Authority and limits

- Assigned crate only for edits; supporting `crates/op-state-store/src/event_chain.rs` read sequentially through1145 solely to check declared digest authority. Relevant lines249-305,581-599,1100-1109; not a full audit or ownership claim for that crate.
- No content searches/extraction/reranking, arbitrary eval execution, shell/process/network actions, recursive delegation, Cargo/builds, tests, lint, formatting, custom smoke fixtures, runtime/deploy/seal/grant/credential operations, git commands or commits/staging/reset/stash.
- One crate source documentation change; source tests and runtime checks not run. No built, installed, deployed, tested or end-to-end security claim.
- Shared HANDOFF bookkeeping required by higher-priority session rules; other agents' work preserved. HEAD not queried.
- Two successful installed declarative JEV calls with four typed answers; evidence/questions/results in `judgments.md`; JEV does not grant mutation authority or prove execution.

## Handoff

Coordinator: reconcile20/20 coverage rows and8/8 occurrence rows with J1/J2/J3/J4 receipts. One documentation-only correction resolved the unsupported helper invocation-timing assertion while preserving native terminology. No runtime change; no build/test performed or requested.

## Self-evaluation

Accuracy4/5: all claims source-referenced; external Codex and producer validation intentionally unverified. Completeness4/5:20/20 assigned files and8/8 occurrences dispositioned; initial hashes not revalidated. Clarity4/5: separate native dispositions/generic drift/limits, receipt detail is verbose. Actionability4/5: coordinator reconciliation paths explicit; broader integration remains outside scope. Conciseness4/5: repeated limits retained for independently usable receipts. Average4.0/5. Improvements: coordinator can consolidate repeated limitations; broader client/caller certification would require separately scoped evidence, not inference. Expected owner assessment: complete static native audit, not runtime certification.
