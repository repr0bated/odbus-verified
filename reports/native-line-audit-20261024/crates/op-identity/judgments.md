# Declarative JEV receipts — op-identity

Date: 2026-10-24. Task: native-line-audit-20261024/op-identity.
Installed instructions: `xd://eval/judge`, read completely. Actual successful `functions.eval` Python call titled **Judging native contexts declaratively** used `await judge(native_evidence, native_questions)` and printed the returned object. No model routing, searches, arbitrary execution, client invention or delegation. One state with three independent typed choice questions. All probability/confidence values below are returned values, not human estimates.

## Evidence supplied

### Header-helper context

`crates/op-identity/src/bin/op-identity-headers.rs:1-7`:

```rust
//! Emit the selected sled's MutationEngine-authored identity as direct MCP
//! HTTP headers.
//!
//! Codex invokes this short-lived helper before each Streamable HTTP request
//! through its native `http_headers_helper` support.  It is not an MCP shim,
//! proxy, listener, or broker: stdout is one JSON header object and the MCP
//! connection still goes directly to the unified `:8090` fabric.
```

Supplied implementation context: lines21-36 parse the explicit selector, resolve the selected credential session, reject noncurrent identity, obtain `mcp_sealed_id_header`, write one JSON header object and newline to stdout. Lines39-69 accept one explicit canonical UUID `--session`, reject duplicate/missing/noncanonical selectors. No model/provider/config routing.

Supplied authority context: `session_projection.rs:110-156` opens existing inline SID1 and checks transport scope, derived principal, session, pubkey, genesis, trace, schema version, expiry, issued/arrival timestamps and chain/catalog arrival fields before returning the unchanged encoded payload without `sid1:`. Lines238-269 read private credential projection, propagate errors, reject missing committed projection. Lines289-333 reject ambiguous/missing/unanchored/expired selected sessions. Explicit limitation supplied: external Codex source/config not inspected; support/invocation frequency not independently certified.

### Genesis context — all seven occurrences supplied together

`crates/op-identity/src/session_genesis.rs:20-30`:

```rust
/// # Arguments
///
/// * `wg_pubkey` — WireGuard public key, decoded from base64 to 32 bytes.
/// * `chain_head_hash` — Exact native chain-head digest, decoded from hex.
///   The current EventChain uses 16 bytes; existing 32-byte callers retain
///   their original output. The identity producer validates the native width
///   before calling, and stores the original hash alongside the minted anchor.
/// * `head_timestamp` — Unix seconds of the chain head event.
/// * `catalog_hash` — Schema catalog hash, decoded from hex to 32 bytes
///   (`schema_catalog_hash()`).
/// * `arrival_timestamp` — `Utc::now().timestamp()` at mint time.
```

Lines41-55 supplied exact function semantics: `mint_genesis(wg_pubkey: &[u8;32], chain_head_hash: &[u8], head_timestamp: i64, catalog_hash: &[u8;32], arrival_timestamp:i64) -> [u8;32]`; creates Blake3 Hasher, updates key, unmodified chain-head slice, head timestamp little-endian, catalog hash, arrival timestamp little-endian, returns final digest.

Lines61-74 supplied:

```rust
    #[test]
    fn native_sixteen_byte_chain_heads_are_bound_without_zero_padding_or_substitution() {
        let key = [0x42; 32];
        let catalog = [0x77; 32];
        let head = [0x55; 16];
        let native = mint_genesis(&key, &head, 10, &catalog, 11);
        let mut changed = head;
        changed[15] ^= 1;
        assert_ne!(native, mint_genesis(&key, &changed, 10, &catalog, 11));
        assert_ne!(native, mint_genesis(&key, &[0; 32], 10, &catalog, 11));
        let mut padded = [0; 32];
        padded[..16].copy_from_slice(&head);
        assert_ne!(native, mint_genesis(&key, &padded, 10, &catalog, 11));
    }
```

External supporting authority was read sequentially, not searched: `crates/op-state-store/src/event_chain.rs:1-180;181-400;401-630;631-860;861-1100;1101-1145`. Supplied relevant source: ChainEvent lines249-305 computes event_hash through `compute_hash`; EventChain lines581-599 initializes genesis via `compute_hash_str("genesis")` and `last_hash` returns last event hash or genesis hash. Lines1100-1109 serialize value then `format!("{:x}", md5::compute(...))` for `compute_hash`; `compute_hash_str` likewise formats MD5 bytes. MD5 is16 bytes; storage uses its hex representation. No model/provider semantics in the occurrences. Caller width validation was explicitly not read/certified. This supporting partial file read is not op-state-store crate coverage or edit ownership.

### Generic documentation context

Supplied `SPEC.md:19-28,40-41,49-52,63`: lists token.rs/keyring/rusqlite, says no binaries and7 Rust files. `compare-op-identity.md:13-20,34,46,54`: lists7 Rust sources,0 binaries, token present/implemented, no internal dependencies. Complete current crate/src/bin listings show17 Rust source files, no token.rs,2 binary files. Cargo.toml:6-8 explicitly declares op-identity-headers,22 depends on op-core, neither keyring nor rusqlite present. Neither documentation file contains native.

## J1_helper

Type: choice.

Question supplied: Judge the exact native occurrence at op-identity-headers.rs:5 in its actual implementation context. Assess intended meaning and whether a native-specific repair is supported, not merely whether it is routing-related. Do not claim external Codex invocation is verified.

Explicit criteria:
- `preserve_descriptive_builtin_with_external_limit`: native modifies named client helper support, not a model/provider or identity bypass; implementation is an explicit-session header emitter, no contrary evidence makes this native terminology inaccurate; keep it, qualify external support/invocation as unverified in report.
- `repair_native_specific_defect`: source establishes inaccurate native semantics or hidden routing/admission override requiring a native-specific change.
- `insufficient_context_to_dispose`: actual context inadequate even to classify/retain terminology.

Returned:
```json
{"type":"choice","choice":"preserve_descriptive_builtin_with_external_limit","confidence":0.98,"probabilities":{"preserve_descriptive_builtin_with_external_limit":0.99,"repair_native_specific_defect":0.01,"insufficient_context_to_dispose":0}}
```
Decision: preserve native terminology, no source patch. This source does not prove external Codex invocation or client configuration.

## J2_genesis

Type: choice.

Question supplied: Judge ALL SEVEN actual native occurrences shown in genesis_source: doc lines23,25; test-name62; local variable66; references69,70,73. Assess native wording correctness,16-byte digest meaning, behavior of hashing supplied bytes, and whether renaming/behavior correction is needed. Do not certify unread producer validation or claim fixtures executed.

Explicit criteria:
- `preserve_digest_vocabulary`: native means exact producer digest representation; EventChain MD5 evidence supports16-byte statement, mint_genesis hashes supplied slice unchanged, fixture compares16-byte digest with altered/zero32/padded32 inputs; docs/test/local references accurate native vocabulary with no native-specific repair.
- `repair_inaccurate_native_semantics`: some supplied native context inaccurate or changes selection/identity needing native-specific repair.
- `insufficient_context`: cannot evaluate all seven actual contexts from supplied source.

Returned:
```json
{"type":"choice","choice":"preserve_digest_vocabulary","confidence":0.79,"probabilities":{"preserve_digest_vocabulary":0.86,"repair_inaccurate_native_semantics":0.12,"insufficient_context":0.02}}
```
Decision: preserve all seven occurrences; normal producer-digest terminology, not AI routing. Confidence is evidence, not proof of runtime, caller validation or cryptographic guarantees. Human source review agrees: MD5 supports width and genesis hashes exact slice without padding/substitution. No native-specific correction identified.

## J3_generic_docs

Type: choice.

Question supplied: Classify shown documentation mismatch versus current manifest and complete directory evidence. Keep unrelated defects outside native-only edits.

Criteria:
- `generic_stale_inventory_report_only`: outdated crate/module/dependency/binary inventory; no native occurrence, report separate generic drift, no native repair.
- `native_defect_requires_change`: evidence makes this specifically native-related.
- `no_mismatch`: shown docs/current manifest/listings agree.
- `insufficient_evidence`: supplied evidence cannot establish mismatch.

Returned:
```json
{"type":"choice","choice":"generic_stale_inventory_report_only","confidence":0.99,"probabilities":{"generic_stale_inventory_report_only":0.99,"native_defect_requires_change":0.01,"no_mismatch":0,"insufficient_evidence":0}}
```
Decision: G1 report-only, no unrelated documentation cleanup.
## J4_helper_timing — supplemental coordinator follow-up

Type: choice. Successful installed declarative call via `await judge(state, questions)` after reading the complete official reference; no model routing or arbitrary execution.

Evidence supplied to JEV:

- Current occurrence and implementation: `crates/op-identity/src/bin/op-identity-headers.rs:1-7,21-36,39-69`. It requires one explicit canonical UUID selector, resolves that selected identity session, rejects noncurrent identity, emits its existing authored SID1 value as JSON headers. No identity mint or model/provider choice.
- Official current public Codex config reference `https://learn.chatgpt.com/docs/config-file/config-reference.md`, captured at `/var/tmp/native-line-audit-20261024.ILroaQ/codex-config-reference.md`, read sequentially through all 2,977 lines. Lines671-675 state `mcp_servers.<id>.http_headers_helper` is a string command printing JSON header names/values; supported only for locally connected HTTP MCP servers; configured bearer/OAuth Authorization takes precedence.
- Evidence limit: reference does not specify callback timing. Installed Codex package version 0.162.1 and JS launcher were examined by coordinator; launcher only starts the binary. Neither package evidence nor public reference verifies installed binary's invocation frequency, actual local configuration, or live invocation.

Question: Is the exact `native` occurrence accurate, and does it warrant a scoped correction? Choose preserve, correct, or insufficient; do not infer runtime proof from documentation or lack of contrary evidence.

Criteria:
- `preserve`: built-in documented helper option is enough to retain the current wording, with external invocation explicitly left unverified.
- `correct`: official docs support the helper option/local command contract but do not establish “before each ... request”; remove that unsupported timing assertion while preserving accurate native vocabulary and identity behavior.
- `insufficient`: supplied code and authoritative public documentation cannot establish whether the occurrence needs correction.

Returned:
```json
{"type":"choice","choice":"correct","confidence":0.72,"probabilities":{"preserve":0.11,"correct":0.81,"insufficient":0.07}}
```

Decision: Corrected `op-identity-headers.rs` lines4-5 to say the helper is intended for Codex's native `http_headers_helper` option on locally connected HTTP MCP servers. This preserves the supported built-in integration vocabulary and removes the unsupported per-request invocation assertion. It does not claim actual configuration, installed-version behavior, invocation timing, or deployment. Exact BEFORE excerpt: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-identity/op-identity-headers.before.md`.

Occurrence reconciliation: N1 now cites J4 and is `changed;resolved`; N2-N8 retain J2 `preserve_digest_vocabulary` (confidence0.79) based on all seven actual contexts supplied together to JEV in J2. No new evidence changes those digest meanings. J1 remains the earlier judgment on the original wording; J4 is the supplemental, more specific adjudication after the coordinator's authoritative-reference evidence and supersedes J1 for final timing disposition. J3 remains a generic report-only documentation finding.
