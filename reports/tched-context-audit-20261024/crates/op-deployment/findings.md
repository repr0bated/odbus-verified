# Findings — op-deployment tched context audit

- The single observed case-insensitive `tched` occurrence is `/var/lib/threetched` in `crates/op-deployment/src/bin/release/inventory.rs:44`.
- It is a protected host-content boundary in `validate_targets`: a release overlay target is rejected if it contains this protected path or lies beneath it. The surrounding denylist also protects other host/runtime data.
- Disposition: retain. Current owner guidance distinguishes the threetched product surface and forbids treating source as runtime/deployment proof. It also requires preserving identity/runtime data. No authoritative evidence supports changing this denylist entry or substituting a different path.
- No source files changed. The crate's SPEC.md describes a generic image/symlink deployment design that does not match the current release-controller implementation; this is not a tched-specific correction and was not expanded into unrelated documentation repair.
- Inventory discrepancy: `Cargo.toml` is listed as 28 lines, but the directly read file has 29 lines; the final line was read separately. Inventory hashes were treated as initial receipts and not recomputed.
- Static source audit only. No searches, Cargo/build/test/lint/format/smoke/runtime/deployment/operation/seal/grant/credential actions. Current path existence, active release graph membership, and deployed state are unverified.
