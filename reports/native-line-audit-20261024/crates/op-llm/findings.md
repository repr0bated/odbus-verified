# op-llm native contextual audit

Task `native-line-audit-20261024/op-llm`; Codex crate worker; 2026-10-24.

## Outcome

Complete static native audit:26/26 inventory files,9,522 preedit lines read sequentially through EOF. Root/src/scripts directory listings reconcile;19 Rust source files,4 scripts,2 Markdown documents,1 manifest. No unlisted nested guidance, unreadable/binary/symlink/dependency entries shown. No pending coverage or unresolved native-specific blocker.

All21 initial native occurrences mapped individually to actual JEV context judgments.18 preserved, two descriptions corrected retaining native, one unsupported external-emitter comment removed.20 native tokens remain after edit; postedit source total9,521lines. Accurate terminology preserved, no blanket rename.

## F1 — fixed incomplete Antigravity authentication descriptions

- Source: `crates/op-llm/src/chat.rs:6,627`.
- Problem: native-Google-ADC-or-compatible-bridge descriptions omit Gemini API-key mode. `AntigravityProvider` supports explicit API-key credentials; Automatic Gemini mode prefers nonempty GEMINI_API_KEY before SDK ADC.
- Change: add `Gemini API key` to module documentation and detailed-status auth_type label. Preserve native SDK terminology in those descriptions and existing126/132.
- Evidence: full antigravity read, particularly593–733; J1.chat_descriptions confidence0.95; applied edit receipt6712; J2.patch_correctness0.85.
- No selection, authentication, authorization, request, serialization shape or schema logic changed. Only displayed descriptive string changes. Docs updated in same module; no new test/re-pin.

## F2 — fixed unsupported native-factory assertion in fixture comment

- Source: `crates/op-llm/src/graph_turn_reminder.rs`, initial59–60, current59.
- Problem: comment asserts external native factory emission; this fixture only establishes preservation of supplied exact reminder content without a trailing newline. External emitter source is unexamined.
- Change: comment now `Reuse an exact reminder supplied without a trailing newline too.` Both local native identifier occurrences and all fixture logic unchanged.
- Evidence: full74-line original read and complete preedit reread; J1.fixture_description confidence1; applied edit receipt9587; J2.patch_correctness0.85.
- This does not claim external emitter broken or absent. No tched/threetched logic, spelling, caller contract, grants, identity-bound reminder text or behavior changed; no overlapping tched hunk proposed.

## Preserved contexts

-13 antigravity occurrences: five legitimate SDK/direct-HTTP/vendor-usage descriptions/log/fixture names; eight canonical NativeBridgeStatus references and bounded bridge-model observation fixture. Observations are not billing/native model routing proof.
- Native audio substring in exact `gemini-live-2.5-flash-native-audio` ID: preserve vendor modality identifier. Static metadata does not establish current provider availability.
- Native fixture binding/assertion: preserve exact-input test sample identifiers.

## Separate generic observations — no native patch expansion

These are source-only observations, not reproduced/runtime findings or exhaustive general review:

1. `chat.rs:247–266,371–384,539–587` contains unavailable/invalid explicit-provider fallback, first HashMap provider fallback and model-family/first-provider discovery heuristics. `gemini.rs:781–1040,1041–1281` includes implicit BALANCED routing for Gemini3/VertexAI. These branches are not controlled by native tokens/flags. Public external consumers and admission unexamined; this audit is not a global routing-safety certification. J2.generic_disposition says report separately1.
2. `SPEC.md`/comparison source inventories and lib systemctl guidance are stale relative to listings/runit policy. `gcloud_adc.rs` describes replacing old Antigravity while ChatManager constructs the SDK-backed Antigravity implementation. No generic documentation cleanup performed.
3. `openclaw.rs:74–97` forwards legacy genesis/footprint/trace identity headers rather than a sealed SID1 credential and can send when configured identity resolution fails. Receiver, deployed endpoint and actual callers unexamined; do not infer current live bypass.
4. Source scripts include credential extraction/spoofing and legacy systemctl guidance; all read as source, none executed, no credential/runtime files read or modified.

## Mutation receipts and ownership

Only crate edits:
- `crates/op-llm/src/chat.rs`:two descriptive lines6/627.
- `crates/op-llm/src/graph_turn_reminder.rs`:two-line comment replaced by one line59.

Full exact BEFORE copies under assigned scratch directory:
- `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-llm/chat.rs.BEFORE`:28997bytes, snapshot402C matches source preedit.
- `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-llm/graph_turn_reminder.rs.BEFORE`:3145bytes, snapshot239A matches source preedit.

Minimal hash-anchored edit succeeded; no surprise concurrent source change observed. Source/edit scope remains op-llm only. Shared HANDOFF/SIGNALS bookkeeping is required by higher-priority session rules and preserves other agents' work. No cross-crate mutation, generated-schema edit, shim, grant/identity weakening, staging/commit/reset/stash or recursive delegation.

## Verification and limitations

Observed verification: complete sequential source reads; directory/inventory reconciliation; exact preedit rereads/BEFORE-copy receipts; surgical edit receipts; eight actual successful installed declarative JEV answers across two calls. No significant algorithmic behavior change; description/comment corrections only. No tests, lint, formatting, custom smoke, Cargo/build, shell/process/network/runtime operations, deployment, seals, grants or credential actions performed, as assignment requires. Primary coordinator owns any required build after reconciliation.

HEAD and current SHA256 unqueried. coverage.tsv hashes are supplied initial inventory receipts, not postedit/current digest attestations. External canonical type definitions/dependencies/callers, current vendor model availability, actual credentials/admission and live deployment are not certified. No built/tested/installed/deployed success claim.

## Handoff

Coordinator: reconcile26 coverage rows,21 occurrence rows, F1/F2 minimal wording patches, eight JEV answers and scratch result.json. Native remediation has no unresolved overlap; generic observations remain explicitly outside this narrow task. No further native source action identified.

## Self-evaluation

Accuracy4/5: exact read/edit/JEV receipts retained; external consumers and live vendor availability not verified and explicitly excluded. Completeness4/5:26/26 files and21/21 occurrences dispositioned; no execution authority, so no runtime claim. Clarity4/5: occurrence ledger identifies original/current positions; lengthy preload/receipts kept outside final response. Actionability4/5: exact patches, backups and coordinator reconciliation listed; primary still owns build. Conciseness4/5: detailed judgment contexts needed for reproducible audit; final reply remains brief. Overall4.0/5. Improvement: coordinator can attach build receipt without treating it as deployment or runtime verification. Would the user agree: evidence supports complete static assignment, not broader runtime safety.
