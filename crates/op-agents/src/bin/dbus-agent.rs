//! D-Bus Agent Launcher
//!
//! Universal binary to run any agent type as a D-Bus service.
//!
//! # Usage
//!
//! ```bash
//! # Run an agent on the session bus
//! dbus-agent python-pro
//!
//! # Run with a custom agent ID
//! dbus-agent python-pro my-python-agent
//!
//! # Run on the system bus (requires privileges)
//! dbus-agent --system rust-pro
//!
//! # List available agent types
//! dbus-agent --list
//! ```
//!
//! # D-Bus Registration
//!
//! The agent will be registered as:
//! - Service name: `org.dbusmcp.Agent.{AgentType}` (e.g., `org.dbusmcp.Agent.PythonPro`)
//! - Object path: `/org/dbusmcp/Agent/{AgentType}`
//! - Interface: `org.dbusmcp.Agent`
//!
//! # Discovery
//!
//! Once running, the agent can be discovered by the ChatActor's tool_loader
//! and registered as a tool that the LLM can call.

use std::env;

use op_core::BusType;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use op_agents::agent_catalog::builtin_agent_descriptors;
use op_agents::agents::{
    aiml::{
        AIEngineerAgent, DataEngineerAgent, DataScientistAgent, MLEngineerAgent,
        MLOpsEngineerAgent, PromptEngineerAgent,
    },
    analysis::{CodeReviewerAgent, DebuggerAgent, PerformanceEngineerAgent, SecurityAuditorAgent},
    architecture::{BackendArchitectAgent, FrontendDeveloperAgent, GraphQLArchitectAgent},
    business::{
        BusinessAnalystAgent, CustomerSupportAgent, HRProAgent, LegalAdvisorAgent,
        PaymentIntegrationAgent, SalesAutomatorAgent,
    },
    content::{ApiDocumenterAgent, DocsArchitectAgent, MermaidExpertAgent, TutorialEngineerAgent},
    database::{DatabaseArchitectAgent, DatabaseOptimizerAgent, SqlProAgent},
    infrastructure::{
        CloudArchitectAgent, DeploymentAgent, KubernetesAgent, NetworkEngineerAgent, TerraformAgent,
    },
    language::{
        BashProAgent, CProAgent, CSharpProAgent, CppProAgent, ElixirProAgent, GolangProAgent,
        JavaProAgent, JavaScriptProAgent, JuliaProAgent, PhpProAgent, PythonProAgent, RubyProAgent,
        RustProAgent, ScalaProAgent, TypeScriptProAgent,
    },
    mobile::{FlutterExpertAgent, IOSDeveloperAgent, MobileDeveloperAgent},
    operations::{DevOpsTroubleshooterAgent, IncidentResponderAgent, TestAutomatorAgent},
    orchestration::{
        ContextManagerAgent, DxOptimizerAgent, MemoryAgent, SequentialThinkingAgent,
        TddOrchestratorAgent,
    },
    security::{BackendSecurityCoderAgent, FrontendSecurityCoderAgent, MobileSecurityCoderAgent},
    seo::{
        ContentMarketerAgent, SEOContentWriterAgent, SEOKeywordStrategistAgent,
        SEOMetaOptimizerAgent, SearchSpecialistAgent,
    },
    specialty::{
        ARMCortexExpertAgent, ErrorDetectiveAgent, HybridCloudArchitectAgent,
        LegacyModernizerAgent, ObservabilityEngineerAgent, QuantAnalystAgent,
        SnowballDeveloperAgent, UIUXDesignerAgent, UnityDeveloperAgent,
    },
    webframeworks::{DjangoProAgent, FastAPIProAgent, TemporalPythonProAgent},
    AgentTrait,
};
use op_agents::dbus_service::{generate_agent_id, start_agent};

fn print_usage() {
    eprintln!(
        "Usage:\n  dbus-agent [--system] <agent-type> [agent-id]\n  dbus-agent --list\n\nExamples:\n  dbus-agent python-pro\n  dbus-agent --system rust-pro\n  dbus-agent python-pro my-agent-id"
    );
}

fn normalize_agent_type(raw: &str) -> String {
    raw.trim().to_lowercase().replace('_', "-")
}

fn build_agent(agent_type: &str, agent_id: String) -> Option<Box<dyn AgentTrait>> {
    match agent_type {
        // Language agents
        "bash-pro" => Some(Box::new(BashProAgent::new(agent_id))),
        "c-pro" => Some(Box::new(CProAgent::new(agent_id))),
        "cpp-pro" => Some(Box::new(CppProAgent::new(agent_id))),
        "csharp-pro" => Some(Box::new(CSharpProAgent::new(agent_id))),
        "elixir-pro" => Some(Box::new(ElixirProAgent::new(agent_id))),
        "golang-pro" => Some(Box::new(GolangProAgent::new(agent_id))),
        "java-pro" => Some(Box::new(JavaProAgent::new(agent_id))),
        "javascript-pro" => Some(Box::new(JavaScriptProAgent::new(agent_id))),
        "julia-pro" => Some(Box::new(JuliaProAgent::new(agent_id))),
        "php-pro" => Some(Box::new(PhpProAgent::new(agent_id))),
        "python-pro" => Some(Box::new(PythonProAgent::new(agent_id))),
        "ruby-pro" => Some(Box::new(RubyProAgent::new(agent_id))),
        "rust-pro" => Some(Box::new(RustProAgent::new(agent_id))),
        "scala-pro" => Some(Box::new(ScalaProAgent::new(agent_id))),
        "typescript-pro" => Some(Box::new(TypeScriptProAgent::new(agent_id))),
        // Architecture agents
        "backend-architect" => Some(Box::new(BackendArchitectAgent::new(agent_id))),
        "frontend-developer" => Some(Box::new(FrontendDeveloperAgent::new(agent_id))),
        "graphql-architect" => Some(Box::new(GraphQLArchitectAgent::new(agent_id))),
        // Infrastructure agents
        "cloud-architect" => Some(Box::new(CloudArchitectAgent::new(agent_id))),
        "deployment-engineer" => Some(Box::new(DeploymentAgent::new(agent_id))),
        "kubernetes-architect" => Some(Box::new(KubernetesAgent::new(agent_id))),
        "network-engineer" => Some(Box::new(NetworkEngineerAgent::new(agent_id))),
        "terraform-specialist" => Some(Box::new(TerraformAgent::new(agent_id))),
        // Analysis agents
        "code-reviewer" => Some(Box::new(CodeReviewerAgent::new(agent_id))),
        "debugger" => Some(Box::new(DebuggerAgent::new(agent_id))),
        "performance-engineer" => Some(Box::new(PerformanceEngineerAgent::new(agent_id))),
        "security-auditor" => Some(Box::new(SecurityAuditorAgent::new(agent_id))),
        // Business agents
        "business-analyst" => Some(Box::new(BusinessAnalystAgent::new(agent_id))),
        "customer-support" => Some(Box::new(CustomerSupportAgent::new(agent_id))),
        "hr-pro" => Some(Box::new(HRProAgent::new(agent_id))),
        "legal-advisor" => Some(Box::new(LegalAdvisorAgent::new(agent_id))),
        "payment-integration" => Some(Box::new(PaymentIntegrationAgent::new(agent_id))),
        "sales-automator" => Some(Box::new(SalesAutomatorAgent::new(agent_id))),
        // Content agents
        "api-documenter" => Some(Box::new(ApiDocumenterAgent::new(agent_id))),
        "docs-architect" => Some(Box::new(DocsArchitectAgent::new(agent_id))),
        "mermaid-expert" => Some(Box::new(MermaidExpertAgent::new(agent_id))),
        "tutorial-engineer" => Some(Box::new(TutorialEngineerAgent::new(agent_id))),
        // Database agents
        "database-architect" => Some(Box::new(DatabaseArchitectAgent::new(agent_id))),
        "database-optimizer" => Some(Box::new(DatabaseOptimizerAgent::new(agent_id))),
        "sql-pro" => Some(Box::new(SqlProAgent::new(agent_id))),
        // Operations agents
        "devops-troubleshooter" => Some(Box::new(DevOpsTroubleshooterAgent::new(agent_id))),
        "incident-responder" => Some(Box::new(IncidentResponderAgent::new(agent_id))),
        "test-automator" => Some(Box::new(TestAutomatorAgent::new(agent_id))),
        // Orchestration agents
        "context-manager" => Some(Box::new(ContextManagerAgent::new(agent_id))),
        "memory" => Some(Box::new(MemoryAgent::new(agent_id))),
        "sequential-thinking" => Some(Box::new(SequentialThinkingAgent::new(agent_id))),
        "dx-optimizer" => Some(Box::new(DxOptimizerAgent::new(agent_id))),
        "tdd-orchestrator" => Some(Box::new(TddOrchestratorAgent::new(agent_id))),
        // Security agents
        "backend-security-coder" => Some(Box::new(BackendSecurityCoderAgent::new(agent_id))),
        "frontend-security-coder" => Some(Box::new(FrontendSecurityCoderAgent::new(agent_id))),
        "mobile-security-coder" => Some(Box::new(MobileSecurityCoderAgent::new(agent_id))),
        // SEO agents
        "content-marketer" => Some(Box::new(ContentMarketerAgent::new(agent_id))),
        "search-specialist" => Some(Box::new(SearchSpecialistAgent::new(agent_id))),
        "seo-content-writer" => Some(Box::new(SEOContentWriterAgent::new(agent_id))),
        "seo-keyword-strategist" => Some(Box::new(SEOKeywordStrategistAgent::new(agent_id))),
        "seo-meta-optimizer" => Some(Box::new(SEOMetaOptimizerAgent::new(agent_id))),
        // Specialty agents
        "arm-cortex-expert" => Some(Box::new(ARMCortexExpertAgent::new(agent_id))),
        "snowball-developer" => Some(Box::new(SnowballDeveloperAgent::new(agent_id))),
        "error-detective" => Some(Box::new(ErrorDetectiveAgent::new(agent_id))),
        "hybrid-cloud-architect" => Some(Box::new(HybridCloudArchitectAgent::new(agent_id))),
        "legacy-modernizer" => Some(Box::new(LegacyModernizerAgent::new(agent_id))),
        "observability-engineer" => Some(Box::new(ObservabilityEngineerAgent::new(agent_id))),
        "quant-analyst" => Some(Box::new(QuantAnalystAgent::new(agent_id))),
        "ui-ux-designer" => Some(Box::new(UIUXDesignerAgent::new(agent_id))),
        "unity-developer" => Some(Box::new(UnityDeveloperAgent::new(agent_id))),
        // AI/ML agents
        "ai-engineer" => Some(Box::new(AIEngineerAgent::new(agent_id))),
        "data-engineer" => Some(Box::new(DataEngineerAgent::new(agent_id))),
        "data-scientist" => Some(Box::new(DataScientistAgent::new(agent_id))),
        "ml-engineer" => Some(Box::new(MLEngineerAgent::new(agent_id))),
        "mlops-engineer" => Some(Box::new(MLOpsEngineerAgent::new(agent_id))),
        "prompt-engineer" => Some(Box::new(PromptEngineerAgent::new(agent_id))),
        // Web framework agents
        "django-pro" => Some(Box::new(DjangoProAgent::new(agent_id))),
        "fastapi-pro" => Some(Box::new(FastAPIProAgent::new(agent_id))),
        "temporal-python-pro" => Some(Box::new(TemporalPythonProAgent::new(agent_id))),
        // Mobile agents
        "flutter-expert" => Some(Box::new(FlutterExpertAgent::new(agent_id))),
        "ios-developer" => Some(Box::new(IOSDeveloperAgent::new(agent_id))),
        "mobile-developer" => Some(Box::new(MobileDeveloperAgent::new(agent_id))),
        _ => None,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("op_agents=info".parse()?))
        .init();

    let args = env::args().skip(1);
    let mut use_system = false;

    let mut raw = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--system" => use_system = true,
            "--list" => {
                for descriptor in builtin_agent_descriptors() {
                    println!("{} - {}", descriptor.agent_type, descriptor.description);
                }
                return Ok(());
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            _ => raw.push(arg),
        }
    }

    if raw.is_empty() {
        print_usage();
        return Ok(());
    }

    let agent_type = normalize_agent_type(&raw[0]);
    let agent_id = raw
        .get(1)
        .cloned()
        .unwrap_or_else(|| generate_agent_id(&agent_type));

    let Some(agent) = build_agent(&agent_type, agent_id.clone()) else {
        error!("Unknown agent type: {}", agent_type);
        warn!("Use --list to see available agents.");
        return Ok(());
    };

    let bus_type = if use_system {
        BusType::System
    } else {
        BusType::Session
    };

    info!(
        "Starting agent '{}' with id '{}' on {:?} bus",
        agent_type, agent_id, bus_type
    );

    let _conn = start_agent(agent, &agent_id, bus_type).await?;
    tokio::signal::ctrl_c().await?;
    Ok(())
}
