# op-llm tched context-and-intent audit

**Task:** `tched-context-audit-20261024`  
**Date:** 2026-10-24  
**Scope:** `crates/op-llm/` source only; exclusive report directory for this crate.

## Result

Full sequential read of all 26 inventory files through EOF. Eight case-insensitive `tched` occurrences were recorded and dispositioned. One inaccurate current-schema comment was corrected in `Cargo.toml`; explicitly legacy API identifiers and identity-bound filesystem reference were retained. No source searches or prior tched decisions were reused.

## Finding F1 — stale schema-authority claim (corrected)

`crates/op-llm/Cargo.toml:12-13` originally said the `tched_router` plugin owns the single LLM schema embedded by `op-llm`. Current `crates/op-plugins/src/default_registry.rs:21-39` identifies `tched_router` as retired and specifically says the captured `threetched` contract is the only binary application contract that may publish operations. `crates/op-plugins/src/state_plugins/threetched.rs:1-6,142-150` describes that captured schema source. `op-llm/src/schema.rs` itself explicitly characterizes its old router DTO/projection as legacy and says it publishes no schema.

Updated the manifest comment to state that the captured `threetched` plugin owns the current binary-application schema and that `src/schema.rs` retains legacy router payloads for unmigrated callers. This corrects the source-authority distinction without claiming runtime deployment or removing compatibility API. The exact 998-byte pre-edit `Cargo.toml` is preserved as `Cargo.toml.before`.

Fresh JEV chose `inaccurate` for the original wording but with low confidence (.29); its change decision was `insufficient` (.65). Human correction is grounded in the explicit current registry retirement rule and current captured-plugin source, while preserving the legacy type boundary. JEV references: `jdgb-15b33567c3185918`, `jdgb-15b3356b7fd85919`.

## Finding F2 — identity-bound discovery wording (retained)

`src/graph_turn_reminder.rs:6` uses `3tchedFS` in a reminder directing graph discovery through identity-bound filesystem operations and stating that grants apply. Lines 1-3 state that the caller lacks a verified OP-DBUS principal and that this is discovery guidance, not a claim of graph access. Retain. The source does not prove current mount availability, operation permission, or runtime success.

JEV meaning was `filesystem` (.59), wording `accurate` (.51), and change `insufficient` (.88); the first two are low-confidence. Human retention follows the surrounding non-authorizing wording and current owner identity/capability policy, not a live runtime claim. References above.

## Finding F3 — retired router symbols are explicitly legacy (retained; migration remains)

`src/schema.rs:1-17` exposes `TchedRouterError`, `LlmTransport`, `TchedRouterPlugin`, `TchedRouterState`, and `embedded_projection()`. The file states this is for callers not yet migrated, publishes no schema, and that the only binary application authority is captured threetched. The function docstring states its projection is not schema authority. Retain these labels/API in this bounded crate audit; deleting or relabeling public compatibility symbols without a downstream caller inventory would be an unsupported cutover. Do not use this in-memory projection as current schema authority.

The old-to-captured API migration is still required for callers that remain. This audit does not inspect external crate consumers, and the available current source evidence does not establish whether those callers have migrated. Coordinator follow-up should reconcile the migration boundary before any removal. No runtime or deployment claim.

JEV classified this state as mixed-context (.85), wording accurate (.57), and change retain (.28). Accuracy/change judgments are weak; retain is the human disposition based on the file's exact legacy boundary plus current registry retirement. References above.

## Zero-occurrence files

The remaining 23 files had no case-insensitive `tched` substring in their complete sequential reads. Coverage is documented by exact full-read ranges in `coverage.tsv`; no occurrence judgments are fabricated for zero-occurrence files.

## Verification and limitations

- Re-read changed `Cargo.toml:9-15` after edit; comment now reflects current schema authority and legacy DTO purpose.
- Re-read `schema.rs:1-18` to verify its legacy/no-schema-authority boundary.
- No Cargo/build/check/test/Clippy/fmt/custom smoke, runtime operation, credential action, grant, seal, or deployment was run. Assignment explicitly prohibits those checks/actions; the change is comment-only.
- Source files and current source registry declarations do not prove live catalog or deployed state. Current mounted data access status is outside this crate audit.

**Status:** complete static source audit; implementation is comment-only and unbuilt/untested by instruction.

## Post-edit occurrence check

The corrected manifest introduces the current identifier `threetched` at line 12. A fresh judgment of that exact post-edit occurrence classified its meaning as current captured contract (.87), accuracy as accurate (.49), and change as insufficient (.11); accuracy/change are weak. Human disposition: retain the source-level authority wording without implying live deployment. This is a post-edit occurrence ledger entry, not an extra baseline occurrence: both the pre-edit source and current source contain eight occurrences. See `occurrences.tsv` and `judgments.md`.
