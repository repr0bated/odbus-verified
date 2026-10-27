# Declarative JEV receipts

## J1 — all eight native contexts, 2026-10-24

Actual installed `judge(state, questions)` through JavaScript eval; successful receipt printed in session under `Judging all native occurrence contexts`. No JEV model routing, search, extraction or arbitrary execution. Fixed worker model unchanged.

### Submitted evidence/state

Task native-line-audit-20261024/op-services. Judge every actual native context for wording correctness, intended meaning and behavior, not merely AI routing. Static evidence only; no runtime proof. Keep unrelated generic stale documentation separate.

Authority:
- Cargo.toml:7-13: `[[bin]] name="op-services" path="src/bin/op-services.rs"; [[bin]] name="op-svctl" path="src/bin/op-svctl.rs"`.
- Complete crate root and src/bin directory reads show only op-services.rs and op-svctl.rs; no systemctl-native.rs file. All 15 inventory files / 1825 lines sequentially read; remaining 13 files contain no native occurrence.
- src/bin/op-svctl.rs:1 `//! Runit service-manager gRPC client.`; :17 `ServiceManagerClient::connect("http://[::1]:50053")`; :79 `eprintln!("Usage: op-svctl <command> [service]")`.
- src/grpc/server.rs:26-32 GrpcServer::new assigns Arc::new(FailClosedVerifier). Every RPC calls authorize before manager. src/grpc/auth.rs:15-29 documents missing canonical shared identity/grant verifier and always returns unavailable after SID1 open.

Actual occurrence contexts supplied (paths relative to crates/op-services):
- N1 SPEC.md:22: `op-services/src/bin/systemctl-native.rs`
- N2 SPEC.md:71: `name = "systemctl-native"`
- N3 SPEC.md:72: `path = "src/bin/systemctl-native.rs"`
- N4,N5 compare-op-services.md:35 positions1,2: `| `src/bin/systemctl-native.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/bin/systemctl-native.rs |`
- N6 compare-op-services.md:47: `| `bin` | ✅ Present | bin group | src/bin/op-services.rs, src/bin/systemctl-native.rs, src/bin/systemctl.rs |`
- N7,N8 compare-op-services.md:63 positions1,2: `| Binary `systemctl-native` | ✅ Implemented | src/bin/systemctl-native.rs | Cargo bin target |`

Proposal supplied: For all 8 occurrences only, replace systemctl-native by op-svctl, preserving other document lines. This updates those nonexistent source/implemented-bin claims to the actual bin target, without changing code/schema/auth behavior. General stale dinit/sqlx/systemctl claims remain separately disclosed, not silently considered fixed.

### Questions, contrary outcomes, answers

1. **wording**: Given actual N1-N8 contexts and manifest/directory/CLI evidence, how should these occurrences be dispositioned?
   - correct_to_op_svctl: all eight stale native-suffixed source/bin claims should name actual op-svctl target/path; concrete documentation correctness fix.
   - retain_accurate_history: retain all because accurate clearly historical-only references, not current claims.
   - mixed: some change, some accurate; finer analysis needed.
   - insufficient: evidence cannot establish concrete correction.
   - Answer `correct_to_op_svctl`, confidence **0.97**; probabilities correct_to_op_svctl=0.98, retain_accurate_history=0, mixed=0.01, insufficient=0.01.
2. **selection**: What role do all eight supplied occurrences play?
   - stale_service_cli_inventory: stale documentation identifiers for service CLI, not provider/model/config selection or actual native billing binding.
   - runtime_selection: tokens cause runtime provider/model/config overrides.
   - insufficient: role cannot be established.
   - Answer `stale_service_cli_inventory`, confidence **1**; probabilities 1,0,0 respectively.
3. **patch**: Does exact proposed eight-token replacement resolve this native-specific defect without asserting broad doc refresh or changing identity/capability behavior?
   - bounded_fix: actual current paths/bin names replace nonexistent claims; code/admission unchanged; generic staleness explicitly excluded.
   - unsafe_or_incomplete_native: native-specific issue remains or change alters authority/admission.
   - insufficient: additional source evidence needed.
   - Answer `bounded_fix`, confidence **0.92**; probabilities bounded_fix=0.95, unsafe_or_incomplete_native=0.04, insufficient=0.01.

Decision: accept bounded wording correction for N1-N8; no runtime change, provider/model routing finding or tched overlap. JEV is declarative evidence, not mutation permission or runtime proof. Full sequential coverage is independently recorded in coverage.tsv.
