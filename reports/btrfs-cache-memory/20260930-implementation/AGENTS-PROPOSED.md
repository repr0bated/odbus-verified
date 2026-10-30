# Repository Guidelines

Proposal for review; this file does not replace `AGENTS.md`. Merge these additions with its mandatory skills, native GNU build, runit, Btrfs, Cozo and deployment rules. Architecture requirements below do not establish live implementation or access grants.

## Identity and Local Control

Operate through the caller's verified container identity and session. For local 3tchedFS lungs, verify identity binding, capability admission, canonical dispatch, MutationEngine commit and projection separately. Provider/model names are not identities or grants.

The local interface exposes `call`, `result` and `error`. Preserve canonical D-Bus control, sealed plugin schemas and committed SHM as state authority. Distinguish state projection from operation receipts: `crates/threetched-fs/src/fuse_fs.rs` currently retains receipts and diagnostics in a local map. Verify both paths without assuming their storage must match.

Respect capability denial. Do not switch identities, borrow host credentials, call a backend directly or mint grants to bypass it. Report the denied operation and missing capability; continue independent work within verified access.

## Scoped Memory and Provenance

Identify each memory source's owner and permitted principal, container, session and project scope before retrieval or writing. Use only verified access. Memory provides context; canonical committed SHM remains live state authority.

The owner's architecture syncs conversations to NotebookLM alongside other memory sources. Use named, verified access to that history; availability to this agent is not assumed. Label material with notebook/conversation identifiers, source references, revision or capture date, and retrieval date. Treat it as conversation provenance, not verified current state or permission. Report unavailable sources; continue from accessible evidence.

## Work Ownership and Progress

Read `AGENTS.md`, required skills, `HANDOFF.md` and relevant recent `SIGNALS.md` entries. Claim paths in the shared active-work record, or follow the delegating agent's recorded scope. Preserve other agents' edits. Use `WISHLIST.md` IDs; append durable observations to `SIGNALS.md` and update handoff status.

Continue already authorized changes without repeated approval. Ask only when work crosses an unresolved authorization boundary; user task authorization does not create runtime grants. During sustained work, report findings, blockers and next steps about every minute.

## Validation and Completion

Separate source findings, inferred behavior, synthetic measurements, native fixtures and live observations. Record revision, paths, commands and evidence. Run focused checks such as `cargo test --locked -p op-cache --lib`, then the applicable workspace gate: `sh scripts/check-workspace.sh verify`.

State incomplete, blocked, failed, skipped and unverified work explicitly. Passing source tests do not prove deployment or live behavior; distinguish wrapper tests, normal Cargo results and deployed-source drift.
