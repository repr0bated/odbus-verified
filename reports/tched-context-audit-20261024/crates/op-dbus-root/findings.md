# Findings

## F-1 — stale schema-source comment in root Cargo.toml

`Cargo.toml:54-57` described a ZeroClaw base runtime as imported for real types, claimed the sealed `tched_router` blob was projected from those types, and referred to default-feature settings and an in-repo stand-in. In the observed `[workspace.dependencies]` context, the following declarations are local OP-DBUS crates and no ZeroClaw dependency is declared. Current root `AGENTS.md` says the full config schema is captured from the verified `threetched` binary at sealing, runtime consumes the sealed catalog, and `tched_router_contract.rs` contains helpers only. The comment therefore misstates both the declared workspace dependency context and current schema authority. JEV-1 agreed stale/misleading (confidence 0.91). Removed the obsolete four-line claim block; retained `# Internal Crates` and all dependency declarations.

Scope: this corrects root manifest commentary only. It does not establish which schema or plugin is deployed, nor change schema capture or runtime behavior.

## F-2 — `tched-fs` in Cargo.lock

Four matches are the `tched-fs` package declaration and dependency identifiers for `op-chat-responder`, `op-grpc-bridge`, and `op-plugins` (see `occurrences.tsv`). Current root owner guidance explicitly names the filesystem Cargo package `tched-fs` (filesystem `3tched-fs`, plugin `tched_fs`, source directory `crates/threetched-fs`). JEV-2 judged these expected filesystem identifiers (confidence 0.86). Retained; Cargo.lock identifiers do not prove member runtime use or deployment.

No additional matched substrings were identified in the other fully read inventory files. No member crate files or runtime state were read.
