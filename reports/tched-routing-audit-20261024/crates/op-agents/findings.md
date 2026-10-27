# op-agents tched routing audit

Task: `tched-routing-audit-20261024/op-agents`; 2026-10-24. Audit/report only.

## Result

149/149 supplied inventory files read sequentially through EOF; 21,956 lines, 724,265 observed bytes (724,257 supplied baseline bytes). All 27 inventoried directory listings were read; no additional file or nested guidance file was encountered. One literal `tched` substring, in ordinary English. Zero tched-associated routing/model/FUSE migration findings, zero dead ends, zero deletions or renames, one leave-as-is. No source changes.

This is not a general correctness/security certification of op-agents, or proof of deployment/provider availability.

## Every occurrence

| ID | Exact source | Token and context | Role | Integrity | Required action | Jev confidence role/integrity/action |
| --- | --- | --- | --- | --- | --- | --- |
| O001 | `crates/op-agents/src/bin/gen-agent-defs.rs:10:21` | `matched`: “The layout is matched deliberately so that module can be revived as a reader” | `ordinary_word` | `not_applicable` | `leave_as_is` | 0.97 / 1 / 1 |

Context lines 4–16 and all verbatim questions/answers are preserved in `judgments.md`, section “Authoritative targeted occurrence judgment”. The comment describes definition layout compatibility, not a runtime receiver or a product. Do not rename part of “matched”, delete the generator, or route this prose through a model/FUSE API.

## Read and judgment method

- Used the explicit supplied inventory, not search/discovery hits. Sequential `read` requests covered each file from line 1 through EOF, in chunks of at most 160 lines; literal character accounting examined every returned line, including longer words and case variants. No grep/rg/glob/search tools, sed or awk.
- Raw-prefix selectors provided full content; reconstructed SHA-256 matched 148 supplied initial hashes. Directory listings reconciled the 149 inventory paths.
- The entire numbered source of every file was submitted to one installed declarative Jev batch, followed by one targeted judge call with three independent choices for the sole actual occurrence. This is tool-mediated sequential reading plus Jev semantic review, not a claim that all source was echoed into the primary chat.
- `coverage.tsv` records every sequential range, EOF, initial and observed hashes, sizes and finding references. `judgments.md` preserves all complete source states and all answers, including contradictory ones. `result.json` counts actual occurrences, not arbitrary full-file labels.

## D001 — Supplied inventory drift, preserved

`crates/op-agents/src/agents/mobile/ios_developer.rs:1–89` was fully read and then fully reread after the mismatch. Inventory: 3,303 bytes, SHA-256 `81befc51f0c9c957bb3abf2cb847e37909cfdf5c1411184596c155e3d8bd6978`. Observed: 3,311 bytes, SHA-256 `0014ff2f33f1caeec4e4a8f536344443de7755873dba90bcf855847ae7fc4a35`, still 89 lines. No `tched` occurrence. The directory listing also reports a more recent modification time for this file than its peers. Attribution was not investigated; the current source is preserved, not reset or overwritten. This audit certifies the observed version, not the earlier inventory bytes.

## Authority conflict and limits

The common assignment says `tched_router` is retired and `threetched` is the captured external plugin reached through identity-bound FUSE. Older root AGENTS.md:45–50, 208–209 and the architecture integration paragraph describe a retained tched_router/8084 integration. Recent SIGNALS.md rows record the isolated seal excluding tched_router and other evolving producer/patch concerns. The assignment authority is used for classification; no out-of-scope registry source, live mount, catalog or listener was examined. Because the sole occurrence is ordinary English, this conflict does not change its action.

## Non-routing observations, intentionally unchanged

- `src/bin/gen-agent-defs.rs:8–11` says the generator is orphaned/not declared in lib.rs, but `src/lib.rs:12` declares `pub mod generator`. This stale comment is not a retired-tched receiver and does not justify `delete_dead_end` for O001.
- The same generator emits `DEFAULT_MODEL = "sonnet"` (`src/bin/gen-agent-defs.rs:23,108`). It is model metadata without a `tched` substring; no provider call or tched migration is established by that emission. No broader model-policy repair was authorized or applied.

## Supplemental Jev disagreements and uncertainty

The broad full-file batch `jdgb-15b3299009447e45` completed 149/149 states, zero failures, model `typesafe/jev-1.14.0`. It returned 92 `domain_occurrences` and 57 `no_occurrences`; 123 confidence scores were below 0.6. All 92 domain labels conflict with deterministic literal accounting (91 files have no substring; gen-agent-defs has only “matched”). These labels do not supply a line, token or missing receiver and are NOT treated as discovered routes. The targeted O001 result supersedes its broad-file label. No semantic uncertainty remains for O001.

The table below records every uncertain or contradictory broad-file decision with its exact source range, confidence and competing literal disposition. Full exact source context and returned probabilities are in the corresponding `judgments.md` section. These are evidence-quality limitations, not additional invented `tched` occurrences or blockers to the exact occurrence ledger. No low-confidence answer was used to recommend mutation.

| Complete source context | Broad Jev answer | Confidence | Exact competing disposition |
| --- | --- | --- | --- |
| `crates/op-agents/Cargo.toml:1-59` | `domain_occurrences` | 0.27 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/compare-op-agents.md:1-103` | `domain_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/SPEC.md:1-108` | `domain_occurrences` | 0.6 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agent_catalog.rs:1-78` | `domain_occurrences` | 0.68 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agent_registry.rs:1-477` | `domain_occurrences` | 0.61 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/advise.rs:1-47` | `domain_occurrences` | 0.44 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/ai_engineer.rs:1-102` | `domain_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/data_engineer.rs:1-89` | `domain_occurrences` | 0.43 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/data_scientist.rs:1-89` | `domain_occurrences` | 0.24 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/ml_engineer.rs:1-84` | `domain_occurrences` | 0.35 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/mlops_engineer.rs:1-89` | `domain_occurrences` | 0.45 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/mod.rs:1-22` | `no_occurrences` | 0.57 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/aiml/prompt_engineer.rs:1-90` | `no_occurrences` | 0.35 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/analysis/code_reviewer.rs:1-142` | `domain_occurrences` | 0.61 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/analysis/debugger.rs:1-43` | `domain_occurrences` | 0.69 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/analysis/performance.rs:1-93` | `domain_occurrences` | 0.41 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/analysis/security_auditor.rs:1-132` | `domain_occurrences` | 0.69 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/architecture/backend_architect.rs:1-187` | `domain_occurrences` | 0.71 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/architecture/frontend_developer.rs:1-124` | `domain_occurrences` | 0.48 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/architecture/graphql_architect.rs:1-115` | `domain_occurrences` | 0.25 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/architecture/mod.rs:1-14` | `no_occurrences` | 0.48 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/base.rs:1-292` | `domain_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/business_analyst.rs:1-89` | `no_occurrences` | 0.35 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/customer_support.rs:1-89` | `no_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/hr_pro.rs:1-89` | `no_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/legal_advisor.rs:1-90` | `domain_occurrences` | 0.4 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/payment_integration.rs:1-90` | `no_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/business/sales_automator.rs:1-89` | `no_occurrences` | 0.29 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agent_schema.rs:1-192` | `domain_occurrences` | 0.56 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/fedram_agent.rs:1-151` | `domain_occurrences` | 0.41 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/gdpr_counsel.rs:1-170` | `no_occurrences` | 0.24 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/mod.rs:1-17` | `no_occurrences` | 0.48 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/opencontrol.rs:1-145` | `domain_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/oscal_auditor.rs:1-161` | `domain_occurrences` | 0.55 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/policy_enforcer.rs:1-155` | `domain_occurrences` | 0.56 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/schema_as_code.rs:1-480` | `domain_occurrences` | 0.6 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/stig_auditor.rs:1-155` | `domain_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/compliance/trestle.rs:1-143` | `domain_occurrences` | 0.68 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/content/api_documenter.rs:1-115` | `domain_occurrences` | 0.68 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/content/docs_architect.rs:1-113` | `domain_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/content/mermaid_expert.rs:1-106` | `domain_occurrences` | 0.57 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/content/tutorial_engineer.rs:1-102` | `domain_occurrences` | 0.31 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/database/database_architect.rs:1-127` | `domain_occurrences` | 0.63 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/database/database_optimizer.rs:1-131` | `domain_occurrences` | 0.69 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/database/sql_pro.rs:1-133` | `domain_occurrences` | 0.63 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/cloud.rs:1-103` | `domain_occurrences` | 0.44 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/deployment.rs:1-135` | `domain_occurrences` | 0.61 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/kubernetes.rs:1-148` | `domain_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/mod.rs:1-13` | `no_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/network.rs:1-78` | `domain_occurrences` | 0.76 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/infrastructure/terraform.rs:1-147` | `domain_occurrences` | 0.64 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/bash_pro.rs:1-148` | `domain_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/cpp_pro.rs:1-125` | `domain_occurrences` | 0.44 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/c_pro.rs:1-160` | `domain_occurrences` | 0.44 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/csharp_pro.rs:1-144` | `domain_occurrences` | 0.36 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/elixir_pro.rs:1-144` | `domain_occurrences` | 0.43 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/golang_pro.rs:1-252` | `domain_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/java_pro.rs:1-153` | `domain_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/javascript_pro.rs:1-234` | `domain_occurrences` | 0.55 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/julia_pro.rs:1-121` | `domain_occurrences` | 0.56 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/mod.rs:1-36` | `no_occurrences` | 0.55 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/php_pro.rs:1-174` | `domain_occurrences` | 0.51 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/python_pro.rs:1-244` | `domain_occurrences` | 0.53 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/ruby_pro.rs:1-143` | `domain_occurrences` | 0.55 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/rust_pro.rs:1-277` | `domain_occurrences` | 0.49 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/scala_pro.rs:1-116` | `domain_occurrences` | 0.36 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/language/typescript_pro.rs:1-182` | `domain_occurrences` | 0.45 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/mobile/flutter_expert.rs:1-89` | `domain_occurrences` | 0.64 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/mobile/ios_developer.rs:1-89` | `domain_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/mobile/mobile_developer.rs:1-92` | `no_occurrences` | 0.28 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/mobile/mod.rs:1-13` | `no_occurrences` | 0.57 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/mod.rs:1-41` | `no_occurrences` | 0.56 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/operations/devops_troubleshooter.rs:1-140` | `domain_occurrences` | 0.69 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/operations/incident_responder.rs:1-140` | `no_occurrences` | 0.32 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/operations/mod.rs:1-15` | `no_occurrences` | 0.57 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/operations/test_automator.rs:1-145` | `domain_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/context_manager.rs:1-108` | `no_occurrences` | 0.52 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/dx_optimizer.rs:1-144` | `domain_occurrences` | 0.51 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/mem0_wrapper.rs:1-114` | `domain_occurrences` | 0.44 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/memory.rs:1-573` | `domain_occurrences` | 0.35 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/mod.rs:1-15` | `no_occurrences` | 0.51 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/sequential_thinking.rs:1-77` | `no_occurrences` | 0.41 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/orchestration/tdd_orchestrator.rs:1-111` | `no_occurrences` | 0.39 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/security/backend_security_coder.rs:1-89` | `no_occurrences` | 0.27 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/security/frontend_security_coder.rs:1-89` | `domain_occurrences` | 0.27 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/security/mobile_security_coder.rs:1-89` | `domain_occurrences` | 0.28 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/content_marketer.rs:1-83` | `no_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/mod.rs:1-15` | `no_occurrences` | 0.59 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/search_specialist.rs:1-341` | `domain_occurrences` | 0.71 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/seo_content_writer.rs:1-83` | `no_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/seo_keyword_strategist.rs:1-84` | `no_occurrences` | 0.31 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/seo/seo_meta_optimizer.rs:1-72` | `no_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/arm_cortex_expert.rs:1-89` | `no_occurrences` | 0.28 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/error_detective.rs:1-84` | `no_occurrences` | 0.31 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/hybrid_cloud_architect.rs:1-83` | `no_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/legacy_modernizer.rs:1-83` | `no_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/observability_engineer.rs:1-89` | `no_occurrences` | 0.29 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/quant_analyst.rs:1-84` | `no_occurrences` | 0.23 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/snowball_developer.rs:1-84` | `domain_occurrences` | 0.57 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/ui_ux_designer.rs:1-89` | `no_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/specialty/unity_developer.rs:1-83` | `domain_occurrences` | 0.25 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/webframeworks/django_pro.rs:1-89` | `domain_occurrences` | 0.32 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/webframeworks/fastapi_pro.rs:1-89` | `domain_occurrences` | 0.48 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/webframeworks/mod.rs:1-14` | `no_occurrences` | 0.48 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/agents/webframeworks/temporal_python_pro.rs:1-90` | `domain_occurrences` | 0.25 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/bin/dbus-agent-manager.rs:1-266` | `no_occurrences` | 0.29 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/bin/dbus-agent.rs:1-260` | `domain_occurrences` | 0.32 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/bin/gen-agent-defs.rs:1-208` | `domain_occurrences` | 0.4 | `ordinary_words_only`; retain receipt, no routing inference |
| `crates/op-agents/src/dbus_service.rs:1-772` | `domain_occurrences` | 0.36 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/dynamic/assemble.rs:1-435` | `domain_occurrences` | 0.36 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/dynamic/mod.rs:1-27` | `no_occurrences` | 0.49 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/dynamic/pieces.rs:1-444` | `no_occurrences` | 0.37 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/generator/md_parser.rs:1-393` | `domain_occurrences` | 0.6 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/generator/mod.rs:1-12` | `no_occurrences` | 0.55 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/generator/template.rs:1-744` | `domain_occurrences` | 0.53 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/lib.rs:1-105` | `domain_occurrences` | 0.43 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/router.rs:1-155` | `domain_occurrences` | 0.36 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/security/mod.rs:1-17` | `no_occurrences` | 0.39 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/security/profiles.rs:1-505` | `domain_occurrences` | 0.49 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/security/sandbox.rs:1-388` | `domain_occurrences` | 0.4 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/security/validation.rs:1-283` | `domain_occurrences` | 0.43 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/adapter.rs:1-102` | `domain_occurrences` | 0.35 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/agent_trait.rs:1-244` | `domain_occurrences` | 0.28 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/base.rs:1-263` | `domain_occurrences` | 0.43 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/golang.rs:1-121` | `domain_occurrences` | 0.4 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/javascript.rs:1-210` | `domain_occurrences` | 0.56 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/mod.rs:1-37` | `no_occurrences` | 0.31 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/python.rs:1-321` | `domain_occurrences` | 0.53 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/rust.rs:1-151` | `domain_occurrences` | 0.51 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/execution/shell.rs:1-130` | `domain_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/legacy_catalog.rs:1-853` | `domain_occurrences` | 0.27 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/mod.rs:1-28` | `no_occurrences` | 0.47 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/orchestration/base.rs:1-235` | `no_occurrences` | 0.28 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/orchestration/code_review_orchestrator.rs:1-78` | `no_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/orchestration/mod.rs:1-27` | `no_occurrences` | 0.39 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/orchestration/tdd_orchestrator.rs:1-71` | `no_occurrences` | 0.25 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/persona/architecture_experts.rs:1-79` | `no_occurrences` | 0.51 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/persona/base.rs:1-274` | `domain_occurrences` | 0.39 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/persona/framework_experts.rs:1-84` | `no_occurrences` | 0.41 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/persona/mod.rs:1-41` | `no_occurrences` | 0.4 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/persona/operations_experts.rs:1-75` | `no_occurrences` | 0.4 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/prompts.rs:1-369` | `domain_occurrences` | 0.33 | `no_occurrences`; retain receipt, no routing inference |
| `crates/op-agents/src/unified/registry.rs:1-173` | `no_occurrences` | 0.25 | `no_occurrences`; retain receipt, no routing inference |

## Execution and delivery status

No source, manifests or tests modified. No Cargo, builds, tests, lint, formatting, custom fixtures/smokes, runtime operations, deployment, sealing, grants, credentials or git mutations. No scripts added. Shared HANDOFF.md bookkeeping is required by higher-priority session rules; other shared docs and other workers’ work remain untouched.

No code remediation or deployment is part of this audit. Next: coordinator consumes the five reports and reconciles D001 against its inventory; this crate supplies no tched routing patch.
