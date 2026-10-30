# Findings — op-assistant-grpc

## Tched-specific findings

None. The complete fresh read found zero case-insensitive `tched` occurrences in the assigned inventory. No source call path or identifier specific to `tched_router`, `threetched`, or `zeroclaw` was established. Contextual JEV selected leave-as-is (0.97 confidence) and did not establish retired-router dispatch.

## Separate generic documentation drift (not changed)

`crates/op-assistant-grpc/src/lib.rs:8-10` says authentication verifies session genesis and transport falls back to HTTP JSON-RPC. The source reviewed in `src/auth.rs` deliberately denies structurally valid SID1 without canonical receiver/grant checks; `src/transport.rs` rejects direct HTTP RPC dispatch. JEV judged this discrepancy generic, not shown tched-specific (0.89 confidence). It is intentionally left outside this task's requested correction scope.

## Verification limits

Static source review only. No source changes, build, tests, runtime receiver, service availability, or deployment were checked. The Rust source contracts do not establish what is currently installed or deployed.

## Follow-up completeness correction

The first report omitted two mandatory-preload receipts. `json-render/SKILL.md:301-377` and the repository-mandated `.agents/skills/using-superpowers/SKILL.md` are now fully read. The 28-path preload list is reconciled: the other 26 paths had complete reads in the prior session. The coordinator's source receipt identifies no missing-line or column-limited source files. The additional skill reference to `tched_router` is outside this crate and does not change the source finding.