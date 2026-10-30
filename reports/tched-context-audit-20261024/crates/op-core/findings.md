# op-core findings — fresh tched context audit

## Scope and result

Reviewed all 22 files in the supplied op-core inventory in sequential inventory order, every listed line through EOF: 5,187 lines total. Files and supplied initial line counts/hash receipts are in `coverage.tsv`. The source inventory and crate directory listing reconcile; there are no additional files in the supplied inventory requiring separate disposition. No file had zero coverage or a pending range.

Two case-insensitive `tched` occurrences were encountered, both using the fixture ID `tched_router` in `crates/op-core/src/private_projection_directory_tests.rs` at lines 221 and 246. They are separately recorded in `occurrences.tsv`. The local meaning was judged by JEV as a fixture identifier, not a current-runtime claim (0.89 confidence). JEV's retain-vs-insufficient result was low confidence (0.52), so human disposition uses source API and test semantics: retain the fixture name; the string is only a private projection key in an isolation/revocation test. It does not establish current catalog membership or authorize use. No in-scope tched-specific correction is supported.

## Limits

The supplied hash/line-count values are initial inventory receipts and were not recomputed. Reads are non-atomic. No runtime operation, sealed catalog verification, current plugin receiver/contract verification, build, test, lint, formatting, smoke, or deployment was performed; the task instructions prohibit those actions. The coordinator note reports mounted listing EIO and explicitly forbids bypass; no bypass was attempted. No source files changed.
