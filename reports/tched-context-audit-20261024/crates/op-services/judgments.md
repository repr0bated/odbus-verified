# JEV judgments — op-services

## J1 — complete coverage and source-change disposition

**Method:** `judge(state, questions)` through the installed declarative JEV. One successful call, two independent choices over the same state. No source search, extraction, or runtime assertion.

**Evidence state:** Fresh `op-services` audit; all 15 paths in the provided inventory were read sequentially to EOF. The complete inventory provides listed line counts totaling 1,825: Cargo.toml 54; SPEC.md 102; build.rs 7; compare-op-services.md 100; proto/services.proto 133; `src/bin/op-services.rs` 47; `src/bin/op-svctl.rs` 87; `src/grpc/auth.rs` 97; `src/grpc/mod.rs` 8; `src/grpc/server.rs` 522 (ranges 1-180, 181-360, 361-522); `src/lib.rs` 14; `src/manager/mod.rs` 5; `src/manager/service_manager.rs` 445 (ranges 1-150, 151-300, 301-445); `src/schema/mod.rs` 9; `src/store/mod.rs` 195. Full raw reads showed no case-insensitive contiguous `tched` substring. Complete source-tree listings match the extant inventory paths. Relevant implementation context: `op-services` describes a standalone compatibility manager whose gRPC listener is restricted to loopback; `FailClosedVerifier` refuses authorization until a shared canonical identity/revocation/grant verifier exists. No claim about deployed runtime/catalog state.

**Question 1:** Does this evidence support complete source coverage for this assigned crate audit, with the correct category only if every assigned file was read fully in source order? Labels: `complete_zero_occurrences` (all assigned inventory files have sequential full EOF read receipts and report no case-insensitive tched substring); `incomplete` (missing/incomplete file/range or omitted occurrence); `insufficient` (receipts ambiguous).  
**Answer:** `complete_zero_occurrences`; confidence 0.92; probabilities: complete_zero_occurrences 0.95, incomplete 0.04, insufficient 0.01.

**Question 2:** Given complete crate inventory/source coverage and the described source role, is any tched-specific source correction supported? Labels: `no_tched_specific_change` (no tched occurrence or other tched-specific source claim; no tched-specific edit supported); `change_needed` (concrete inaccurate tched-specific reference); `insufficient` (cannot resolve).  
**Answer:** `no_tched_specific_change`; confidence 1.0; probabilities: no_tched_specific_change 1.0, change_needed 0, insufficient 0.

**Human disposition:** No crate source edit. This is a static source result only; runtime/deployment remain unverified. JEV is evidence, not authorization or runtime proof.
