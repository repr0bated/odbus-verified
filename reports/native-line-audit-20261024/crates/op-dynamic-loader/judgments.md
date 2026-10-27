# Declarative JEV receipts

Task: native-line-audit-20261024/op-dynamic-loader
Date: 2026-10-24
Facility: installed eval `await judge(state, questions)`; instructions read from `xd://eval/judge`.
Successful call: `Judging loader source context`; one shared evidence state, three independent typed choice questions. No routing or model selection requested. Eval used only for declarative judgments on already read source.

## Evidence state

Full-file coverage: all eight entries in coverage.tsv, 585 supplied newline-counted lines read through EOF; no `native` occurrence observed. Exact source excerpts were supplied in the eval call from:

- `src/dynamic_registry.rs:53-79`: cache lookup `cache.get(name)` returns `Arc::clone(tool)` before `self.loading_strategy.should_load(name, context).await`; miss path calls `self.base_registry.get(name).await` and caches the supplied name.
- `src/execution_aware_loader.rs:19-35,43-56`: constructor creates SmartLoadingStrategy with 300-second base TTL; lookup forwards caller tool_name/context; convenience lookup creates `ExecutionContext::new(tool_name)`.
- `src/loading_strategy.rs:35-57,59-103`: should_load always ultimately returns true; recent execution counts drive numeric priority; critical membership doubles base TTL. Critical labels are `respond_to_user`, `cannot_perform`, `systemd_status`, `file_read`, `agent_status`. No command execution or provider/model field appears in these paths.

The complete quoted evidence and criteria remain in the successful eval call transcript; the numbered source reads are the primary source receipts.

## Q1 — runtime selection

Question: “Does this code implicitly override an LLM provider/model/native configuration, or only select/cache a caller-named tool? Restrict the answer to shown source.”

Outcomes:
- tool_lookup_only: supplied tool name, registry, execution-history heuristic and LRU; no provider/model/config rewrite.
- implicit_model_override: shown source rewrites provider/model/native config without explicit admitted selection.
- insufficient_evidence: cannot classify the shown runtime code.

Actual answer: `tool_lookup_only`; confidence **1**.
Probabilities: tool_lookup_only=1; implicit_model_override=0; insufficient_evidence=0.
Decision: no native-specific runtime-selection fix. This is a source-local conclusion, not external caller certification.

## Q2 — historical service label

Question: “Classify the literal systemd_status in this exact source. Do not infer running systemctl from a tool-name comparison.”

Outcomes:
- heuristic_label_only: membership affects loading-priority/TTL heuristics; no OS command invocation or provider rewrite shown.
- service_command_execution: shown code invokes systemctl/systemd services directly.
- insufficient_evidence: cannot determine the literal's role.

Actual answer: `heuristic_label_only`; confidence **1**.
Probabilities: heuristic_label_only=1; service_command_execution=0; insufficient_evidence=0.
Decision: preserve the literal. Host runit policy does not justify inventing a renamed tool contract or blanket native/OS renaming.

## Q3 — admission evidence boundary

Question: “What does the name-keyed cache prove about caller-context and admission? LoadingStrategy is extensible and receives context, while cache hits return before should_load. Distinguish source ordering from a demonstrated system security exploit.”

Outcomes:
- cache_skips_strategy_not_proven_exploit: hit skips context-sensitive strategy; downstream authorization unknown; generic cache/strategy concern, not native selection.
- strategy_rechecked_on_hit: source revalidates strategy for each hit.
- proven_unauthorized_execution: source conclusively executes a tool without any authorization in the complete system.
- insufficient_evidence: cannot determine cache ordering.

Actual answer: `cache_skips_strategy_not_proven_exploit`; confidence **1**.
Probabilities: cache_skips_strategy_not_proven_exploit=1; strategy_rechecked_on_hit=0; proven_unauthorized_execution=0; insufficient_evidence=0.
Decision: record generic contract limitation without native-only patch or exploit claim. No caller search, external admission audit or runtime reproduction performed.

## Every-occurrence reconciliation

The updated common assignment and repeated reconciliation request are satisfied by the explicit zero-occurrence ledger in `occurrences.tsv`, linked to all eight complete sequential reads in `coverage.tsv` (585 supplied lines). Q1–Q3 above remain the actual successful source-context judgments; they are not presented as judgments of native-word occurrences. No native-word occurrence exists in the assigned source inventory, so no new occurrence judgment is applicable.

The supplied `source-coverage-receipt-v2.json` records 8 inventory entries, 8 files with read metadata, and no deleted entries, missing metadata, missing-line files or column-limited files. Supplied `changed-since-inventory.tsv` contains only its header: no changed source entries reported. These are coordinator receipts read as evidence, not independently executed checks. Assignment-directory listing shows no coordinator-followup.md.
