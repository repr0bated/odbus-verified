# approvals

Rust implementation of `docs/APPROVALS-SPEC.md` §§4–7 — **implemented as pure
logic (12 tests pass); not yet wired** into the bridge, the co-signer model call
or the runtime (no I/O or network): structured `ApprovalRequest` with canonical argument
hashing, the co-signer truth table over model judgments, single-use tickets
bound to request/args/executor/TTL. Tests cover acceptance criteria 3
(free text cannot move the co-signer) and 5 (reuse/args-change/expiry refused).

Run: `cargo test` in this directory.
