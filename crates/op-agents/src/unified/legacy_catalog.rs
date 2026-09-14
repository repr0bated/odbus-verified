//! Data-driven migration of legacy static agent IDs into the unified registry.
//!
//! Every public catalog ID from `list_agent_types()` is registered here as a
//! persona, execution, or orchestration factory so `create_agent` no longer
//! needs the giant static match.

use super::agent_trait::{AgentCapability, UnifiedAgent};
use super::execution::{
    GoExecutor, JavaScriptExecutor, PythonExecutor, RustExecutor, ShellExecutor,
};
use super::orchestration::OrchestrationAgent;
use super::persona::PersonaAgent;
use super::prompts::templates::{BASE_AGENT, PERSONA_AGENT};

type Factory = fn() -> Box<dyn UnifiedAgent>;

fn persona(
    id: &'static str,
    name: &'static str,
    domain: &'static str,
    description: &'static str,
    capability: AgentCapability,
) -> Box<dyn UnifiedAgent> {
    let prompt = PERSONA_AGENT
        .replace("{agent_name}", name)
        .replace("{domain}", domain)
        .replace("{expertise_list}", description);
    let knowledge = BASE_AGENT
        .replace("{agent_name}", name)
        .replace("{specialization}", domain);
    Box::new(
        PersonaAgent::new(id, name, description, domain, &prompt, &knowledge)
            .with_capability(capability),
    )
}

fn orchestration(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    allowed: &[&'static str],
) -> Box<dyn UnifiedAgent> {
    Box::new(OrchestrationAgent::new(
        id,
        name,
        description,
        allowed.to_vec(),
    ))
}

macro_rules! p {
    ($id:expr, $name:expr, $domain:expr, $desc:expr, $cap:expr) => {
        || persona($id, $name, $domain, $desc, $cap)
    };
}

/// Extra factories beyond the hand-written unified experts/executors.
/// Keys must be unique vs EXECUTION_AGENTS / PERSONA_AGENTS / ORCHESTRATION_AGENTS
/// or they intentionally override those IDs with catalog-compatible names.
pub fn migrated_factories() -> Vec<(&'static str, Factory)> {
    use AgentCapability::*;

    let mut out: Vec<(&'static str, Factory)> = Vec::new();

    // --- Language / execution (catalog IDs) ---
    out.push(("rust-pro", || Box::new(RustExecutor::new())));
    out.push(("python-pro", || Box::new(PythonExecutor::new())));
    out.push(("javascript-pro", || Box::new(JavaScriptExecutor::new())));
    out.push(("typescript-pro", || Box::new(JavaScriptExecutor::new())));
    out.push(("golang-pro", || Box::new(GoExecutor::new())));
    out.push(("go-pro", || Box::new(GoExecutor::new())));
    out.push(("bash-pro", || Box::new(ShellExecutor::new())));
    out.push(("k8s", || {
        persona(
            "k8s",
            "Kubernetes Agent",
            "kubernetes",
            "Kubernetes cluster operations",
            AgentCapability::ArchitectureDesign,
        )
    }));
    out.push((
        "java-pro",
        p!(
            "java-pro",
            "Java Pro",
            "java",
            "Java build, test, and packaging specialist",
            RunCode {
                language: "java".into()
            }
        ),
    ));
    out.push((
        "csharp-pro",
        p!(
            "csharp-pro",
            "C# Pro",
            "csharp",
            "C# / .NET specialist",
            RunCode {
                language: "csharp".into()
            }
        ),
    ));
    out.push((
        "cpp-pro",
        p!(
            "cpp-pro",
            "C++ Pro",
            "cpp",
            "C++ systems programming specialist",
            RunCode {
                language: "cpp".into()
            }
        ),
    ));
    out.push((
        "c-pro",
        p!(
            "c-pro",
            "C Pro",
            "c",
            "C systems programming specialist",
            RunCode {
                language: "c".into()
            }
        ),
    ));
    out.push((
        "ruby-pro",
        p!(
            "ruby-pro",
            "Ruby Pro",
            "ruby",
            "Ruby and Rails specialist",
            RunCode {
                language: "ruby".into()
            }
        ),
    ));
    out.push((
        "php-pro",
        p!(
            "php-pro",
            "PHP Pro",
            "php",
            "PHP specialist",
            RunCode {
                language: "php".into()
            }
        ),
    ));
    out.push((
        "scala-pro",
        p!(
            "scala-pro",
            "Scala Pro",
            "scala",
            "Scala specialist",
            RunCode {
                language: "scala".into()
            }
        ),
    ));
    out.push((
        "elixir-pro",
        p!(
            "elixir-pro",
            "Elixir Pro",
            "elixir",
            "Elixir / OTP specialist",
            RunCode {
                language: "elixir".into()
            }
        ),
    ));
    out.push((
        "julia-pro",
        p!(
            "julia-pro",
            "Julia Pro",
            "julia",
            "Julia scientific computing specialist",
            RunCode {
                language: "julia".into()
            }
        ),
    ));

    // --- Orchestration (catalog IDs) ---
    out.push(("memory", || {
        orchestration(
            "memory",
            "Memory Agent",
            "Manages conversational and task memory across agents",
            &["code-reviewer", "debugger"],
        )
    }));
    out.push(("context-manager", || {
        orchestration(
            "context-manager",
            "Context Manager",
            "Maintains and compresses agent context windows",
            &["memory", "sequential-thinking"],
        )
    }));
    out.push(("sequential-thinking", || {
        orchestration(
            "sequential-thinking",
            "Sequential Thinking",
            "Stepwise reasoning orchestrator",
            &["memory", "debugger"],
        )
    }));
    out.push(("dx-optimizer", || {
        orchestration(
            "dx-optimizer",
            "DX Optimizer",
            "Developer-experience workflow orchestrator",
            &["rust-pro", "python-pro", "docs-architect"],
        )
    }));

    out.extend(persona_dispatch_table());
    out
}

fn persona_dispatch_table() -> Vec<(&'static str, Factory)> {
    use AgentCapability::*;
    vec![
        (
            "frontend-developer",
            p!(
                "frontend-developer",
                "Frontend Developer",
                "frontend",
                "UI/SPA frontend specialist",
                ArchitectureDesign
            ),
        ),
        (
            "graphql-architect",
            p!(
                "graphql-architect",
                "GraphQL Architect",
                "graphql",
                "GraphQL schema and API design",
                ArchitectureDesign
            ),
        ),
        (
            "network-engineer",
            p!(
                "network-engineer",
                "Network Engineer",
                "network",
                "Networking and routing specialist",
                ArchitectureDesign
            ),
        ),
        (
            "deployment",
            p!(
                "deployment",
                "Deployment Agent",
                "deployment",
                "Release and deployment specialist",
                WorkflowManagement
            ),
        ),
        (
            "kubernetes",
            p!(
                "kubernetes",
                "Kubernetes Agent",
                "kubernetes",
                "Kubernetes cluster operations",
                ArchitectureDesign
            ),
        ),
        (
            "terraform",
            p!(
                "terraform",
                "Terraform Agent",
                "terraform",
                "Infrastructure as code with Terraform",
                ArchitectureDesign
            ),
        ),
        (
            "cloud-architect",
            p!(
                "cloud-architect",
                "Cloud Architect",
                "cloud",
                "Multi-cloud architecture specialist",
                ArchitectureDesign
            ),
        ),
        (
            "debugger",
            p!(
                "debugger",
                "Debugger",
                "debugging",
                "Root-cause debugging specialist",
                Debugging
            ),
        ),
        (
            "performance-engineer",
            p!(
                "performance-engineer",
                "Performance Engineer",
                "performance",
                "Performance analysis and tuning",
                Optimization
            ),
        ),
        (
            "search-specialist",
            p!(
                "search-specialist",
                "Search Specialist",
                "search",
                "Search and retrieval specialist",
                Documentation
            ),
        ),
        (
            "seo-content-writer",
            p!(
                "seo-content-writer",
                "SEO Content Writer",
                "seo",
                "SEO-oriented content writing",
                Documentation
            ),
        ),
        (
            "seo-keyword-strategist",
            p!(
                "seo-keyword-strategist",
                "SEO Keyword Strategist",
                "seo",
                "Keyword research and strategy",
                Documentation
            ),
        ),
        (
            "seo-meta-optimizer",
            p!(
                "seo-meta-optimizer",
                "SEO Meta Optimizer",
                "seo",
                "Meta tag and SERP optimization",
                Documentation
            ),
        ),
        (
            "content-marketer",
            p!(
                "content-marketer",
                "Content Marketer",
                "marketing",
                "Content marketing specialist",
                Documentation
            ),
        ),
        (
            "prompt-engineer",
            p!(
                "prompt-engineer",
                "Prompt Engineer",
                "prompting",
                "LLM prompt design specialist",
                Documentation
            ),
        ),
        (
            "ai-engineer",
            p!(
                "ai-engineer",
                "AI Engineer",
                "ai",
                "Applied AI systems specialist",
                ArchitectureDesign
            ),
        ),
        (
            "ml-engineer",
            p!(
                "ml-engineer",
                "ML Engineer",
                "ml",
                "Machine learning engineering",
                ArchitectureDesign
            ),
        ),
        (
            "mlops-engineer",
            p!(
                "mlops-engineer",
                "MLOps Engineer",
                "mlops",
                "ML operations and pipelines",
                ArchitectureDesign
            ),
        ),
        (
            "data-scientist",
            p!(
                "data-scientist",
                "Data Scientist",
                "data-science",
                "Data science and analytics",
                Documentation
            ),
        ),
        (
            "data-engineer",
            p!(
                "data-engineer",
                "Data Engineer",
                "data-engineering",
                "Data pipelines and warehouses",
                ArchitectureDesign
            ),
        ),
        (
            "database-architect",
            p!(
                "database-architect",
                "Database Architect",
                "database",
                "Database design specialist",
                ArchitectureDesign
            ),
        ),
        (
            "database-optimizer",
            p!(
                "database-optimizer",
                "Database Optimizer",
                "database",
                "Query and schema optimization",
                Optimization
            ),
        ),
        (
            "sql-pro",
            p!(
                "sql-pro",
                "SQL Pro",
                "sql",
                "SQL query specialist",
                Documentation
            ),
        ),
        (
            "devops-troubleshooter",
            p!(
                "devops-troubleshooter",
                "DevOps Troubleshooter",
                "devops",
                "Incident and ops troubleshooting",
                Debugging
            ),
        ),
        (
            "incident-responder",
            p!(
                "incident-responder",
                "Incident Responder",
                "incidents",
                "Incident response specialist",
                Debugging
            ),
        ),
        (
            "test-automator",
            p!(
                "test-automator",
                "Test Automator",
                "testing",
                "Test automation specialist",
                Documentation
            ),
        ),
        (
            "backend-security-coder",
            p!(
                "backend-security-coder",
                "Backend Security Coder",
                "security",
                "Secure backend coding",
                SecurityAudit
            ),
        ),
        (
            "frontend-security-coder",
            p!(
                "frontend-security-coder",
                "Frontend Security Coder",
                "security",
                "Secure frontend coding",
                SecurityAudit
            ),
        ),
        (
            "mobile-security-coder",
            p!(
                "mobile-security-coder",
                "Mobile Security Coder",
                "security",
                "Secure mobile coding",
                SecurityAudit
            ),
        ),
        (
            "business-analyst",
            p!(
                "business-analyst",
                "Business Analyst",
                "business",
                "Business analysis specialist",
                Documentation
            ),
        ),
        (
            "customer-support",
            p!(
                "customer-support",
                "Customer Support",
                "support",
                "Customer support specialist",
                Documentation
            ),
        ),
        (
            "hr-pro",
            p!(
                "hr-pro",
                "HR Pro",
                "hr",
                "HR process specialist",
                Documentation
            ),
        ),
        (
            "legal-advisor",
            p!(
                "legal-advisor",
                "Legal Advisor",
                "legal",
                "Legal guidance specialist",
                Documentation
            ),
        ),
        (
            "payment-integration",
            p!(
                "payment-integration",
                "Payment Integration",
                "payments",
                "Payment systems specialist",
                ArchitectureDesign
            ),
        ),
        (
            "sales-automator",
            p!(
                "sales-automator",
                "Sales Automator",
                "sales",
                "Sales automation specialist",
                Documentation
            ),
        ),
        (
            "api-documenter",
            p!(
                "api-documenter",
                "API Documenter",
                "docs",
                "API documentation specialist",
                Documentation
            ),
        ),
        (
            "docs-architect",
            p!(
                "docs-architect",
                "Docs Architect",
                "docs",
                "Documentation architecture",
                Documentation
            ),
        ),
        (
            "mermaid-expert",
            p!(
                "mermaid-expert",
                "Mermaid Expert",
                "diagrams",
                "Mermaid diagram specialist",
                Documentation
            ),
        ),
        (
            "tutorial-engineer",
            p!(
                "tutorial-engineer",
                "Tutorial Engineer",
                "docs",
                "Tutorial and guide authoring",
                Documentation
            ),
        ),
        (
            "flutter-expert",
            p!(
                "flutter-expert",
                "Flutter Expert",
                "flutter",
                "Flutter mobile specialist",
                ArchitectureDesign
            ),
        ),
        (
            "ios-developer",
            p!(
                "ios-developer",
                "iOS Developer",
                "ios",
                "iOS development specialist",
                ArchitectureDesign
            ),
        ),
        (
            "mobile-developer",
            p!(
                "mobile-developer",
                "Mobile Developer",
                "mobile",
                "Cross-platform mobile specialist",
                ArchitectureDesign
            ),
        ),
        (
            "arm-cortex-expert",
            p!(
                "arm-cortex-expert",
                "ARM Cortex Expert",
                "embedded",
                "ARM Cortex embedded specialist",
                ArchitectureDesign
            ),
        ),
        (
            "snowball-developer",
            p!(
                "snowball-developer",
                "Snowball Developer",
                "snowball",
                "Snowball development",
                ArchitectureDesign
            ),
        ),
        (
            "error-detective",
            p!(
                "error-detective",
                "Error Detective",
                "debugging",
                "Error analysis specialist",
                Debugging
            ),
        ),
        (
            "hybrid-cloud-architect",
            p!(
                "hybrid-cloud-architect",
                "Hybrid Cloud Architect",
                "cloud",
                "Hybrid cloud architecture",
                ArchitectureDesign
            ),
        ),
        (
            "legacy-modernizer",
            p!(
                "legacy-modernizer",
                "Legacy Modernizer",
                "modernization",
                "Legacy system modernization",
                ArchitectureDesign
            ),
        ),
        (
            "observability-engineer",
            p!(
                "observability-engineer",
                "Observability Engineer",
                "observability",
                "Metrics, logs, traces",
                ArchitectureDesign
            ),
        ),
        (
            "quant-analyst",
            p!(
                "quant-analyst",
                "Quant Analyst",
                "quant",
                "Quantitative analysis",
                Documentation
            ),
        ),
        (
            "ui-ux-designer",
            p!(
                "ui-ux-designer",
                "UI/UX Designer",
                "design",
                "UI/UX design specialist",
                Documentation
            ),
        ),
        (
            "unity-developer",
            p!(
                "unity-developer",
                "Unity Developer",
                "unity",
                "Unity game development",
                ArchitectureDesign
            ),
        ),
        (
            "django-pro",
            p!(
                "django-pro",
                "Django Pro",
                "django",
                "Django web framework specialist",
                ArchitectureDesign
            ),
        ),
        (
            "fastapi-pro",
            p!(
                "fastapi-pro",
                "FastAPI Pro",
                "fastapi",
                "FastAPI specialist",
                ArchitectureDesign
            ),
        ),
        (
            "temporal-python-pro",
            p!(
                "temporal-python-pro",
                "Temporal Python Pro",
                "temporal",
                "Temporal workflows in Python",
                ArchitectureDesign
            ),
        ),
        (
            "fedramp",
            p!(
                "fedramp",
                "FedRAMP Agent",
                "compliance",
                "FedRAMP compliance specialist",
                SecurityAudit
            ),
        ),
        (
            "gdpr-counsel",
            p!(
                "gdpr-counsel",
                "GDPR Counsel",
                "compliance",
                "GDPR counsel specialist",
                SecurityAudit
            ),
        ),
        (
            "oscal-auditor",
            p!(
                "oscal-auditor",
                "OSCAL Auditor",
                "compliance",
                "OSCAL auditing specialist",
                SecurityAudit
            ),
        ),
        (
            "stig-auditor",
            p!(
                "stig-auditor",
                "STIG Auditor",
                "compliance",
                "STIG auditing specialist",
                SecurityAudit
            ),
        ),
        (
            "opencontrol",
            p!(
                "opencontrol",
                "OpenControl Agent",
                "compliance",
                "OpenControl compliance",
                SecurityAudit
            ),
        ),
        (
            "policy-enforcer",
            p!(
                "policy-enforcer",
                "Policy Enforcer",
                "compliance",
                "Policy enforcement specialist",
                SecurityAudit
            ),
        ),
        (
            "schema-as-code",
            p!(
                "schema-as-code",
                "Schema as Code",
                "compliance",
                "Schema-as-code specialist",
                Documentation
            ),
        ),
        (
            "compliance-trestle",
            p!(
                "compliance-trestle",
                "Compliance Trestle",
                "compliance",
                "OSCAL trestle workflows",
                SecurityAudit
            ),
        ),
    ]
}
