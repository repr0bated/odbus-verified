# Contextual JEV judgments

JEV was used declaratively on source evidence already read. Answers guide review; they are not runtime receipts or permission to edit.

## JEV-1 — `Cargo.toml:54-57`

Evidence supplied: exact comment block claims a ZeroClaw base runtime is imported for real config/tools/providers types; claims the sealed `tched_router` blob is projected from those types; refers to `default-features = false` and an in-repo stand-in. The adjacent `[workspace.dependencies]` declarations are path-based OP-DBUS crates (`op-core`, `op-tools`, `op-chat`, etc.) and the shown section does not declare ZeroClaw. Current root AGENTS authority says verified `threetched` binary schema is fully captured at seal time, runtime consumes the sealed catalog, and `tched_router_contract.rs` has helpers only; it distinguishes threetched executable/runtime-facing surface from tched_router integration.

Question: “Contextually, are the Cargo.toml comment claims that workspace imports ZeroClaw's real types and that a sealed tched_router blob is projected from those types stale/misleading and should be removed or corrected, still accurate, or unsupported by available evidence? Assess exact displayed dependency context against current owner schema-capture contract; distinguish tched_router vs threetched semantics.”

Answer: **stale and should be removed or corrected**; confidence **0.91**; probabilities stale **0.94**, still accurate **0.01**, insufficient **0.05**. Human decision: **correct** by removing the obsolete four-line comment block; leave dependencies untouched.

## JEV-2 — `Cargo.lock` filesystem package references

Evidence supplied all four local contexts: `op-chat-responder` dependency array includes `tched-fs` at line 4198; `op-grpc-bridge` dependency array includes it at 4529; `op-plugins` dependency array includes it at 4855; package declaration `name = "tched-fs"`, version 0.1.0 at line 7408. Root AGENTS explicitly says the 3tchedFS filesystem Cargo package is `tched-fs`, plugin `tched_fs`, directory `crates/threetched-fs`. Lockfile context records package/dependency identifiers, not runtime usage. No member crate manifest/runtime was inspected.

Question: “For each of the four exact Cargo.lock contexts, judge whether the case-insensitive 'tched' occurrence is an expected filesystem Cargo package identifier, stale/wrong product/runtime naming requiring correction, or insufficient evidence. Assess package declarations and dependency references, apply current owner guidance, and explicitly avoid inferring that these lock entries prove live usage.”

Answer: **expected filesystem Cargo identifier**; confidence **0.86**; probabilities expected **0.91**, stale **0.06**, insufficient **0.03**. Human decision: **retain** all four identifiers, with no claim about member runtime or deployment.

No lower-confidence result required reframing. JEV is one input; source context and owner guidance support the recorded human dispositions.
