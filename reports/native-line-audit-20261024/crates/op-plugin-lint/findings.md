# op-plugin-lint native audit — complete

Task `native-line-audit-20261024/op-plugin-lint`; Codex crate worker; 2026-10-24.

## Coverage and outcome

26/26 inventory files fully read sequentially through EOF, including all12 source modules, all4 test/fixture files, and every historical generated capture. Directory listings root/src/examples/tests/tests-fixtures reconcile with inventory; no extra nested guidance, binary/symlink/dependency entry discovered. No pending files or unresolved native-specific source hunk.

71 native occurrences individually mapped in occurrences.tsv:59 preserved terminology/path references and12 historical generator/owner-wording caveats. Eight installed declarative judge() calls yielded16 typed answers; three occurrence-context jev_verify calls plus one delivery-sufficiency verification (J12 supports/0.89). Low-confidence judgments explicitly unaccepted and resolved using closer/context-separated evidence. No source edits, independent native routing repair, proposed cross-crate hunk or tched overlap established.

`zeroclaw.complete.json` has4245 observed lines versus supplied newline count4244; final unterminated line included. Supplied hashes are initial receipts, not recomputed current digests. HEAD unqueried.

## F1 — historical generator/owner terminology mismatch; report-only disposition

Exact wording: `Crate or package that owns the native schema.` Captured schema_crate default/example=`schemars`, whereas native_type=`zeroclaw::config::schema::Config`. Generator versus declaring-type owner is ambiguous/misleading; this is not evidence of provider/model/auth override.

All12 contexts:
- examples/zeroclaw-final-plugin-schema.json:722,7019,7161 — J5 confidence1.
- examples/zeroclaw-final-schema.json:1043,7340,7482 — J5 confidence1.
- examples/zeroclaw-full-schema.json:3385,5858,6149 — J7 confidence1.
- examples/zeroclaw-full-schema.with-compare.json:3712,6185,6476 — J10 confidence0.93.

J5 supersedes J1 blanket metadata-accuracy conclusion for the first capture's three ownership labels. Native_type keys/descriptions/taxonomy have separate judgments; ownership ambiguity is not dismissed as accurate terminology.

Decision: preserve original historical evidence and record this caveat. examples/README.md:28-32 expressly identifies these captures as historical, not current tched_router schema or release configuration, retaining original metadata and requiring regeneration from the intended source rather than relabeling captures. No current schema producer with native wording exists in the fully read assigned Rust modules. Editing generated captures as independent schema authority is prohibited. No current-producer repair demonstrated; no external hunk proposed or unresolved overlap. Do not interpret capture wording as authoritative declaring-owner metadata.

## Retained contexts

- Six schema/RPC-native descriptions are historical integration adjectives, not transport/model/billing selectors. Emitter copies PLUGIN_DESCRIPTION into JSON/Markdown. J1/J3/J4/J8/J11 evaluate actual wording; historical/current execution not certified.
- Forty-four native_type keys, source-language descriptions and native-type taxonomy references consistently denote type-name metadata (qualified Rust type or empty string), not runtime routes. J1/J6/J8/J11. Upstream existence/implementation unverified.
- Nine NativeRuntime[.shell], ThinkingConfig.native_thinking and AuthMethod.Native references are discovered paths in historical exports/gap arrays. J2 supports preserving exact identifiers, not upstream execution/auth semantics or availability.
- No native substrings encountered in any source module, manifest, README, tests/fixtures or introspect-gaps.md. Full-reader evidence in coverage.tsv, not a search result or JEV absence certification.

## Generic observations — no native fix

src/introspect.rs::resolve_plugin_object tries optional OP_PLUGIN_LINT_SCHEMA_URL then three hardcoded HTTP drift-check URLs, without credential metadata shown in that function. Static concern about alternate schema drift-check resolution/authenticity; no request run, denial, exploit or deployed exposure established. J2 generic_report_only confidence0.98. Unrelated HTTP-client/admission repair is outside this native audit.

Gap coverage/delegation is heuristic path classification (gaps.rs), not runtime routing. rust_emit.rs augmentation explicitly emits review-only candidates requiring ownership/types/side-effects/dispatch review; it is not current authoritative schema. Historical fixed-model/provider declarations do not establish live routing and are not relabeled.

## Verification and limits

Sequential reads, directory listings, requested report writes, required shared HANDOFF bookkeeping, and actual declarative JEV judgments only. No searches, extraction, shell/process/network code, Cargo, build, tests, lint, formatting, smoke fixtures, runtime operations, deployment, seal, grants, credentials, staging, commits, reset/stash or recursive delegation. No source behavioral change requiring execution proof. NOT built/tested/installed/deployed.

Coordinator: consume complete coverage/occurrence/JEV ledgers and retain F1 historical caveat. No source patch from this crate needs building or tched reconciliation. Current upstream runtime/admission/availability remains outside this static assignment.
