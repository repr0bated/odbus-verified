# Declarative JEV receipts — op-workflows

Invocation: Eval / `judge(state, questions)`; tool call title `Judging crate coverage declaratively`. The installed tool instructions were read from `xd://eval/judge` before invocation. One successful call evaluated one evidence state against three independent questions. No source-extraction code, search, reranking, or model routing was used.

Evidence supplied: task scope and non-blanket-rename requirement; current root `AGENTS.md` product-name distinctions and source-vs-runtime boundary; the exact 16-file assignment inventory with listed line counts; statement that every file was directly read in complete sequential raw ranges through EOF, and no case-insensitive `tched` occurrence was seen. The specific file paths and total line coverage are recorded in `coverage.tsv`. No occurrence context existed to provide because the pass found zero.

| ID | Question | Answer | Confidence | Probabilities |
|---|---|---|---:|---|
| `coverage` | Does evidence establish complete zero-occurrence coverage, partial, or insufficient? | `complete_zero_occurrences` | 0.97 | complete 0.98; partial 0.01; insufficient 0.01 |
| `needed_change` | Is a tched-specific correction needed, no change supported, or evidence insufficient? | `no_tched_specific_change` | 0.92 | no change 0.95; change 0; insufficient 0.05 |
| `runtime` | Does static evidence establish current sealed catalog/runtime/deployment state? | `not_established` | 1.00 | not established 1.00; established 0; insufficient 0 |

Questions explicitly limited conclusions to supplied evidence; runtime status was not inferred. JEV is evidence, not permission or runtime proof. Human disposition: zero occurrences, no source changes, static-only completion.
