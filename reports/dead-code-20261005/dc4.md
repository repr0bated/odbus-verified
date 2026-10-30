# dc4 — Scope 4 compiler-dead audit, 2026-10-05

Worker: Codex dc4 (`/root`); task `dead-code-20261005/dc4`. Branch `feat/od-108-native-lookup`; baseline HEAD `3cf5c47d1dd3baec2522c5ddac03de5bba1c5523`. Scope: op-tools, op-agents, op-chat-responder, threetched-fs. Read AGENTS.md and all four mandatory skills; identity storage lifecycle correction preserved. Existing responder diff (56 added/123 removed at intake) belongs to another worker and is untouched.

## Outcome and verification

No source deletion is justified. Scoped compilation passed: gpt-6-luna worker ran `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-tools -p op-agents -p op-chat-responder -p threetched-fs` with existing target lock, exit0 in39.05s, no warnings/errors. Primary independently read `/tmp/opencode/dc4-check.log`; no proof-a candidates emerged. No alternate target, release-build interruption, commits/pushes, golden/seal/restarts, runtime/grant/storage changes. No Rust file edited, so file-only rustfmt is inapplicable. Coordinator owns final workspace `/verify`; this worker does not claim workspace fmt/clippy/tests passed.

## Authority and graph evidence

Started at mounted `/run/mount/3tchedFS/plugins/cozo/schema.json`, read `list_relations` declaration: read-only, capability `cap.data.cozo.relations.list@v1`. Current worker's `zcall cozo list_relations -a '{}'` failed at local transport: `Failed to connect to system scope bus via local transport: Operation not permitted (consider using --machine=<user>@.host --user to connect to bus of other user)`. Missing grant is not established. No alternate identity/transport attempted; permitted graph-file fallback used.

Read `docs/crate-graph/crate-graph.cozo` (crate nodes lines41/46/75/79, agents contains edges begin1328), `.json` (1267 nodes/2940 edges), and fresh `/tmp/opencode/system-graph-snapshot.json` (1246/2939, source:op-dbus, generated2026-10-05T02:17:01Z, baseline commit with dirty source). Same scoped reverse dependencies in both JSON graphs:

- op-agents: op-chat, op-cognitive-mcp, op-grpc-bridge, op-plugins, op-tools, op-web.
- op-tools: op-chat, op-dbus, op-dynamic-loader, op-mcp, op-mcp-aggregator, op-web, op-workflows.
- op-chat-responder and threetched-fs: binary targets via provides_bin; no incoming crate depends_on found, which does not establish symbol death.

Both graphs contain scoped modules through `contains`; imports into agents from bridge agent_catalog_dispatch/agent_runs/agent_workflow/context_bundle/specialist, cognitive agent_tools, plugin cognitive_agents/persona/project/auto_create, tools agent_tool and web agents/status/state confirm crate consumers. Tools imports from chat, dynamic-loader, MCP, web and workflows likewise confirm consumers. Public/exported APIs remain where those reverse consumers cannot be ruled out. Graphs support reachability, not symbol liveness or deployment.

SHA256: `.cozo` `672ddf7f6eecfd777ceaee3bcfd334246fd950619a8000379c0a02c7c994af5b`; `.json` `3de03935fe8ce697c269f52d1269861394ff52e7a1c6ac50b98bfe172c1c9b50`; fresh projection `23c782cf5d3ae2e554f4db210b2a0d452b3a00641152c72d69b650abefa64bfd`.

## Per-file disposition

All locations are intake working-tree lines. `deleted` = none. Suppression scan used `rg -n 'allow\(dead_code\)|allow\(unused|warn\(dead_code'` over all four crates. Cross-reference searches covered crates/ + docs/ (which includes negative_topology_gates.rs); pinned inventory read without edits.

| File:line and candidate | Disposition | Proof status / reason |
|---|---|---|
| op-agents/src/unified/persona/framework_experts.rs:8 DjangoExpert | kept | Explicit allow at7, but PERSONA_AGENTS factory caller persona/mod.rs:26; proof b fails. |
| Same file:37 FastAPIExpert | kept | Allow36; factory caller persona/mod.rs:27. |
| Same file:62 ReactExpert | kept | Allow61; factory caller persona/mod.rs:28. |
| op-agents/src/unified/persona/architecture_experts.rs:8 BackendArchitect | kept | Allow7; factory caller persona/mod.rs:31. |
| Same file:33 SecurityAuditor | kept | Allow32; factory caller persona/mod.rs:32. |
| Same file:58 CodeReviewer | kept | Allow57; factory caller persona/mod.rs:33. |
| op-agents/src/unified/persona/operations_experts.rs:8 KubernetesExpert | kept | Allow7; factory caller persona/mod.rs:36. |
| Same file:32 SystemdExpert | kept | Allow31; factory caller persona/mod.rs:37. Historical host terminology does not establish dead code. |
| Same file:55 DbusExpert | kept | Allow54; factory caller persona/mod.rs:38. |
| op-agents/src/unified/orchestration/code_review_orchestrator.rs:13 CodeReviewOrchestrator | kept | Allow12; ORCHESTRATION_AGENTS caller orchestration/mod.rs:24 and re-export:10. |
| op-agents/src/unified/orchestration/tdd_orchestrator.rs:12 TddOrchestrator | kept | Allow11; factory caller orchestration/mod.rs:22 and re-export:11. |
| op-agents/src/agents/mod.rs:1 blanket suppression / exported subtree | kept | Inherited suppression is not a compiler finding. Public modules/types and unresolved reverse consumers prevent deletion of whole subtree. Per-file type locations below. No field/method independently proven dead. |
| op-agents/src/agents/base.rs:253 impl_agent_common | candidate (needs human); kept | Inherits suppression; rg declaration-only across crates/docs. However #[macro_export] at252 exposes public macro to unresolved crate reverse consumers; proof b alone cannot rule them out. No deletion. |
| op-agents/src/agents/base.rs:14/64/128/161 AgentTask/TaskResult/AgentContext/AgentTrait | kept | Public root re-exports lib.rs:24 and widespread trait/type consumers; suppression does not prove death. Validation constants at194/197/198 have local callers201/205/237/241; validation funcs200/236 used by agent implementations and protected test277. |
| op-agents/src/agents/advise.rs:10/18/39 is_advise_op/extract_query/route_advise_to_op | kept | References from compliance modules and others; proof b fails. |
| crates/op-tools (all source files) | kept | No dead_code/unused suppression matches; no source-only deletion permitted without compiler warning. |
| crates/op-chat-responder/src/main.rs | kept | No suppression matches; existing shared edits preserved. |
| crates/threetched-fs (all source files) | kept | No suppression matches; no source-only deletion permitted without compiler warning. |

## Blanket-suppressed public agent inventory retained

The following exact per-file declarations were considered as public subtree candidates. Every row is **kept: unresolved public reverse consumers; no independent proof a/b**. This is not a claim that every legacy type is called in current production. Associated implementations remain with their retained public types; protected tests remain untouched.

```text
crates/op-agents/src/agents/aiml/ai_engineer.rs:9:pub struct AIEngineerAgent {
crates/op-agents/src/agents/aiml/data_engineer.rs:8:pub struct DataEngineerAgent {
crates/op-agents/src/agents/aiml/data_scientist.rs:8:pub struct DataScientistAgent {
crates/op-agents/src/agents/aiml/ml_engineer.rs:8:pub struct MLEngineerAgent {
crates/op-agents/src/agents/aiml/mlops_engineer.rs:8:pub struct MLOpsEngineerAgent {
crates/op-agents/src/agents/aiml/prompt_engineer.rs:8:pub struct PromptEngineerAgent {
crates/op-agents/src/agents/analysis/code_reviewer.rs:11:pub struct CodeReviewerAgent {
crates/op-agents/src/agents/analysis/debugger.rs:8:pub struct DebuggerAgent {
crates/op-agents/src/agents/analysis/performance.rs:9:pub struct PerformanceEngineerAgent {
crates/op-agents/src/agents/analysis/security_auditor.rs:11:pub struct SecurityAuditorAgent {
crates/op-agents/src/agents/architecture/backend_architect.rs:16:pub struct BackendArchitectAgent {
crates/op-agents/src/agents/architecture/frontend_developer.rs:13:pub struct FrontendDeveloperAgent {
crates/op-agents/src/agents/architecture/graphql_architect.rs:13:pub struct GraphQLArchitectAgent {
crates/op-agents/src/agents/base.rs:128:pub struct AgentContext {
crates/op-agents/src/agents/base.rs:14:pub struct AgentTask {
crates/op-agents/src/agents/base.rs:161:pub trait AgentTrait: Send + Sync {
crates/op-agents/src/agents/base.rs:253:macro_rules! impl_agent_common {
crates/op-agents/src/agents/base.rs:64:pub struct TaskResult {
crates/op-agents/src/agents/business/business_analyst.rs:8:pub struct BusinessAnalystAgent {
crates/op-agents/src/agents/business/customer_support.rs:8:pub struct CustomerSupportAgent {
crates/op-agents/src/agents/business/hr_pro.rs:8:pub struct HRProAgent {
crates/op-agents/src/agents/business/legal_advisor.rs:8:pub struct LegalAdvisorAgent {
crates/op-agents/src/agents/business/payment_integration.rs:8:pub struct PaymentIntegrationAgent {
crates/op-agents/src/agents/business/sales_automator.rs:8:pub struct SalesAutomatorAgent {
crates/op-agents/src/agents/compliance/fedram_agent.rs:12:pub struct FedRampAgent {
crates/op-agents/src/agents/compliance/gdpr_counsel.rs:12:pub struct GdprCounselAgent {
crates/op-agents/src/agents/compliance/opencontrol.rs:12:pub struct OpenControlAgent {
crates/op-agents/src/agents/compliance/oscal_auditor.rs:12:pub struct OscalAuditorAgent {
crates/op-agents/src/agents/compliance/policy_enforcer.rs:12:pub struct PolicyEnforcerAgent {
crates/op-agents/src/agents/compliance/schema_as_code.rs:13:pub struct SchemaAsCodeAgent {
crates/op-agents/src/agents/compliance/stig_auditor.rs:12:pub struct StigAuditorAgent {
crates/op-agents/src/agents/compliance/trestle.rs:12:pub struct ComplianceTrestleAgent {
crates/op-agents/src/agents/content/api_documenter.rs:11:pub struct ApiDocumenterAgent {
crates/op-agents/src/agents/content/docs_architect.rs:11:pub struct DocsArchitectAgent {
crates/op-agents/src/agents/content/mermaid_expert.rs:11:pub struct MermaidExpertAgent {
crates/op-agents/src/agents/content/tutorial_engineer.rs:10:pub struct TutorialEngineerAgent {
crates/op-agents/src/agents/database/database_architect.rs:11:pub struct DatabaseArchitectAgent {
crates/op-agents/src/agents/database/database_optimizer.rs:11:pub struct DatabaseOptimizerAgent {
crates/op-agents/src/agents/database/sql_pro.rs:11:pub struct SqlProAgent {
crates/op-agents/src/agents/infrastructure/cloud.rs:9:pub struct CloudArchitectAgent {
crates/op-agents/src/agents/infrastructure/deployment.rs:11:pub struct DeploymentAgent {
crates/op-agents/src/agents/infrastructure/kubernetes.rs:11:pub struct KubernetesAgent {
crates/op-agents/src/agents/infrastructure/network.rs:9:pub struct NetworkEngineerAgent {
crates/op-agents/src/agents/infrastructure/terraform.rs:11:pub struct TerraformAgent {
crates/op-agents/src/agents/language/bash_pro.rs:11:pub struct BashProAgent {
crates/op-agents/src/agents/language/c_pro.rs:11:pub struct CProAgent {
crates/op-agents/src/agents/language/cpp_pro.rs:11:pub struct CppProAgent {
crates/op-agents/src/agents/language/csharp_pro.rs:11:pub struct CSharpProAgent {
crates/op-agents/src/agents/language/elixir_pro.rs:11:pub struct ElixirProAgent {
crates/op-agents/src/agents/language/golang_pro.rs:17:pub struct GolangProAgent {
crates/op-agents/src/agents/language/java_pro.rs:11:pub struct JavaProAgent {
crates/op-agents/src/agents/language/javascript_pro.rs:18:pub struct JavaScriptProAgent {
crates/op-agents/src/agents/language/julia_pro.rs:11:pub struct JuliaProAgent {
crates/op-agents/src/agents/language/php_pro.rs:11:pub struct PhpProAgent {
crates/op-agents/src/agents/language/python_pro.rs:18:pub struct PythonProAgent {
crates/op-agents/src/agents/language/ruby_pro.rs:11:pub struct RubyProAgent {
crates/op-agents/src/agents/language/rust_pro.rs:17:pub struct RustProAgent {
crates/op-agents/src/agents/language/scala_pro.rs:11:pub struct ScalaProAgent {
crates/op-agents/src/agents/language/typescript_pro.rs:11:pub struct TypeScriptProAgent {
crates/op-agents/src/agents/mobile/flutter_expert.rs:8:pub struct FlutterExpertAgent {
crates/op-agents/src/agents/mobile/ios_developer.rs:8:pub struct IOSDeveloperAgent {
crates/op-agents/src/agents/mobile/mobile_developer.rs:8:pub struct MobileDeveloperAgent {
crates/op-agents/src/agents/operations/devops_troubleshooter.rs:13:pub struct DevOpsTroubleshooterAgent {
crates/op-agents/src/agents/operations/incident_responder.rs:13:pub struct IncidentResponderAgent {
crates/op-agents/src/agents/operations/test_automator.rs:13:pub struct TestAutomatorAgent {
crates/op-agents/src/agents/orchestration/context_manager.rs:10:pub struct ContextManagerAgent {
crates/op-agents/src/agents/orchestration/dx_optimizer.rs:11:pub struct DxOptimizerAgent {
crates/op-agents/src/agents/orchestration/mem0_wrapper.rs:38:pub struct Mem0WrapperAgent {
crates/op-agents/src/agents/orchestration/memory.rs:19:pub struct MemoryEntry {
crates/op-agents/src/agents/orchestration/memory.rs:33:pub enum MemoryType {
crates/op-agents/src/agents/orchestration/memory.rs:70:pub struct MemoryAgent {
crates/op-agents/src/agents/orchestration/sequential_thinking.rs:11:pub struct SequentialThinkingAgent {
crates/op-agents/src/agents/orchestration/tdd_orchestrator.rs:8:pub struct TddOrchestratorAgent {
crates/op-agents/src/agents/security/backend_security_coder.rs:8:pub struct BackendSecurityCoderAgent {
crates/op-agents/src/agents/security/frontend_security_coder.rs:8:pub struct FrontendSecurityCoderAgent {
crates/op-agents/src/agents/security/mobile_security_coder.rs:8:pub struct MobileSecurityCoderAgent {
crates/op-agents/src/agents/seo/content_marketer.rs:8:pub struct ContentMarketerAgent {
crates/op-agents/src/agents/seo/search_specialist.rs:18:pub struct SearchSpecialistAgent {
crates/op-agents/src/agents/seo/seo_content_writer.rs:8:pub struct SEOContentWriterAgent {
crates/op-agents/src/agents/seo/seo_keyword_strategist.rs:8:pub struct SEOKeywordStrategistAgent {
crates/op-agents/src/agents/seo/seo_meta_optimizer.rs:8:pub struct SEOMetaOptimizerAgent {
crates/op-agents/src/agents/specialty/arm_cortex_expert.rs:8:pub struct ARMCortexExpertAgent {
crates/op-agents/src/agents/specialty/error_detective.rs:8:pub struct ErrorDetectiveAgent {
crates/op-agents/src/agents/specialty/hybrid_cloud_architect.rs:8:pub struct HybridCloudArchitectAgent {
crates/op-agents/src/agents/specialty/legacy_modernizer.rs:8:pub struct LegacyModernizerAgent {
crates/op-agents/src/agents/specialty/observability_engineer.rs:8:pub struct ObservabilityEngineerAgent {
crates/op-agents/src/agents/specialty/quant_analyst.rs:8:pub struct QuantAnalystAgent {
crates/op-agents/src/agents/specialty/snowball_developer.rs:8:pub struct SnowballDeveloperAgent {
crates/op-agents/src/agents/specialty/ui_ux_designer.rs:8:pub struct UIUXDesignerAgent {
crates/op-agents/src/agents/specialty/unity_developer.rs:8:pub struct UnityDeveloperAgent {
crates/op-agents/src/agents/webframeworks/django_pro.rs:8:pub struct DjangoProAgent {
crates/op-agents/src/agents/webframeworks/fastapi_pro.rs:8:pub struct FastAPIProAgent {
crates/op-agents/src/agents/webframeworks/temporal_python_pro.rs:8:pub struct TemporalPythonProAgent {
```

Next action: fleet coordinator consolidates reports and performs final workspace `/verify`. No source deletion has been made; all scoped checks passed without diagnostics.
