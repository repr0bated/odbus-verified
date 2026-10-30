# Declarative JEV receipts

Task: native-line-audit-20261024/op-mcp-aggregator. Date: 2026-10-24. Reader: Codex crate worker.

One successful installed `await judge(audit_evidence, audit_questions)` call, Python eval title `Judging actual crate context`. Facility instructions read at `xd://eval/judge`; no JEV routing extension, model override, search/reranking, arbitrary execution or recursive delegation. The eval transcript retains the exact complete submitted state and questions. This receipt records source references, material evidence, questions, contrary outcomes and returned probabilities. No failed calls.

## Evidence state

All paths below relative to `crates/op-mcp-aggregator/`. Full sequential coverage is in coverage.tsv. Zero native substring occurrences is the reader's full-file observation, NOT an absence claim certified by these excerpts.

The actual submitted state contained exact source excerpts:
- `src/config.rs:506-522`: enabled check, lowercasing, Gemini branch with log and `return ToolMode::Full`.
- `src/config.rs:592-608`: complete `ToolMode` enum with Compact, Full, Hybrid and their tool-exposure documentation.
- `src/aggregator.rs:305-324`: complete `set_client_info`, calls `self.config.client_detection.detect_mode(name)` and writes `client_info`/`detected_mode`.
- `src/aggregator.rs:355-365`: complete `get_mcp_tools`, matches Compact/Full/Hybrid to listing methods.
- `src/aggregator.rs:197-218`: complete `call_tool_in_profile`, obtains `(tool,server_id)` through `authorized_tool_in_profile` and dispatches returned owner.
- `src/compact.rs:319-341`: execute body parses tool and arguments, calls `self.aggregator.call_tool(tool_name, arguments).await?`, returns result/server/success. (Closing function brace is line342, present in the submitted snippet as well.)
- `src/aggregator.rs:160-174`: complete `call_tool`, resolves owner from `cache.get_server_id(name)`, then dispatches without profile validation.
- `README.md:49-54`: claims connections to `(SSE, stdio, websocket)`, caching/profiles/routing.
- `src/client.rs:162-170`: complete `validate_transport` rejects anything not `TransportType::Sse` with `Unsupported MCP transport ... configure an HTTP/SSE upstream`.
- Observed complete SPEC/src/Cargo listings: SPEC lists `src/unused/context.rs` and op-plugins dependency; actual src contains eight Rust files, no unused directory; Cargo internal dependencies only op-core/op-tools.

Scope supplied: native terminology/behavior and implicit provider/model/config overrides; unrelated generic defects stay report-only; no external callers or runtime examined.

## J1_mode_meaning

Question: Classify what the shown client-name auto-detection actually selects. Is this an AI provider/model selection override, or an MCP tool-presentation choice? Judge the source, not branding in its log. Do not certify unshown external paths.

Criteria:
- tool_presentation_only: Shown branch selects Compact/Full/Hybrid tool listings; no provider/model binding selected by this code.
- provider_model_override: Shown branch selects or overrides a provider/model binding, beyond listing presentation.
- insufficient: Shown implementation is insufficient to distinguish.

Actual answer: `tool_presentation_only`, confidence **1**. Probabilities: tool_presentation_only=1; provider_model_override=0; insufficient=0.
Decision: preserve presentation behavior. Client/AI branding alone does not justify a model-routing patch. This does not certify the log's Vertex capability claim or any external selection path.

## J2_profile_boundary

Question: Compare ExecuteToolTool::execute to Aggregator::call_tool_in_profile using shown call_tool. Does the standalone wrapper skip profile validation locally? This is source-only; do not infer live exposure or upstream identity bypass.

Criteria:
- generic_profile_gap: Wrapper uses unrestricted local call_tool instead of profile-authorized API; generic admission concern, not evidence of a native/provider override.
- equivalent_validation: Both shown paths enforce the same profile validation.
- insufficient: Cannot tell whether local wrapper validates profile from provided code.

Actual answer: `generic_profile_gap`, confidence **0.95**. Probabilities: generic_profile_gap=0.97; equivalent_validation=0; insufficient=0.03.
Decision: G1 report-only. Do not conflate exported standalone Tool wrapper with authenticated embedding's profile-bound handler. Actual embedding/exposure unexamined; upstream authorization not certified or bypassed by this judgment.

## J3_documentation_scope

Question: Classify README unsupported transports and SPEC inventory discrepancies under the explicit task criteria. Should they be recorded separately without unrelated source cleanup?

Criteria:
- generic_report_only: Concrete documentation drift unrelated to native; report separately, do not expand patch scope.
- native_fix_needed: Evidence demonstrates native-specific semantic or provider/model defect requiring this audit to edit those docs.
- insufficient: Cannot classify from supplied source/docs.

Actual answer: `generic_report_only`, confidence **0.94**. Probabilities: generic_report_only=0.96; native_fix_needed=0.01; insufficient=0.03.
Decision: G2 report-only. No source edits under native audit.

## Verification boundary

JEV judges source evidence, not runtime. No tests, lint, format, Cargo, custom fixtures, runtime calls, deployment, seal, grants or credentials operations. No native occurrence needed an occurrence-specific semantic judgment; no occurrence was fabricated. Full coverage, rather than routing-only scope evidence, establishes the explicit zero-occurrence result.
