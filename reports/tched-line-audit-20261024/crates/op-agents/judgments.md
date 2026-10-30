# op-agents declarative JEV judgments

Task: tched-line-audit-20261024; date: 2026-10-24.
Facility instructions read: `xd://eval/judge`. Successful installed `judge()` call through Python eval; no routing extension, model selection, source filtering, filesystem execution or network code used.

## J1 — literal substring in ordinary English

Source: `crates/op-agents/src/bin/gen-agent-defs.rs:6-11`; occurrence at line 10, `matched` (one literal `tched` substring).
Exact evidence submitted:

```rust
//! Output follows the frontmatter + section layout that `src/generator/`
//! defines (`name`/`description`/`model`, then `## Purpose`, `## Capabilities`,
//! `## Behavioral Traits`, `## Knowledge Base`). Note that `src/generator/` is
//! currently orphaned — it is not declared in `lib.rs`, so it never compiles.
//! The layout is matched deliberately so that module can be revived as a reader
//! without reformatting anything here, but nothing verifies the round trip yet.
```

Policy submitted: Our OP-DBUS filesystem branding is 3tched/3tched-fs/3tchedFS. External ZeroClaw-derived executable/surface branding is threetched. Preserve ordinary English substring collisions, correct qualified branding, and sealed contract identifiers. Judge naming only, not accuracy of surrounding historical documentation.

Question: Choose the contextual naming disposition of the literal tched substring inside matched at line 10. Does this word refer to product branding or ordinary matching of layouts?

Explicit choice criteria:
- `leave_as_is`: Ordinary English matched meaning compatible layouts, not either branded product; preserve text.
- `replace_with_threetched`: Incorrect external executable/surface branding requiring threetched.
- `replace_with_3tched`: Incorrect OP-DBUS filesystem branding requiring 3tched.
- `insufficient_evidence`: Context cannot establish intent.

Actual successful tool output:

```json
{"disposition":{"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}}}
```

Decision: `leave_as_is`. Matching a layout is not product naming; either replacement would corrupt ordinary English. No source edit. The surrounding stale orphaned-module statement is outside this naming-only assignment: current `src/lib.rs:12` declares `pub mod generator;`.

## Coverage and inference limits

All 149 inventory entries read fully in sequential ranges, including embedded tests, docs, generator templates and catalog data. Exactly one literal substring identified in those reads. No product-name occurrence or sealed naming identifier found. Absence conclusions are the worker's complete line-by-line source review, not fabricated JEV source scanning. J1 judges only the exact context above and is not runtime verification.
