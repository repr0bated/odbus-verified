//! Persona Agents
//!
//! LLM-only agents that provide expertise without code execution.
//! These augment LLM responses with domain knowledge.

mod architecture_experts;
mod base;
mod framework_experts;
mod operations_experts;

pub use architecture_experts::*;
pub use base::PersonaAgent;
pub use framework_experts::*;
pub use operations_experts::*;

use std::collections::HashMap;
use std::sync::LazyLock;

/// All available persona agents
#[allow(clippy::type_complexity)]
pub static PERSONA_AGENTS: LazyLock<HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>>> =
    LazyLock::new(|| {
        let mut m: HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>> = HashMap::new();

        // Framework experts
        m.insert("django-expert", || Box::new(DjangoExpert::new()));
        m.insert("fastapi-expert", || Box::new(FastAPIExpert::new()));
        m.insert("react-expert", || Box::new(ReactExpert::new()));

        // Architecture experts
        m.insert("backend-architect", || Box::new(BackendArchitect::new()));
        m.insert("security-auditor", || Box::new(SecurityAuditor::new()));
        m.insert("code-reviewer", || Box::new(CodeReviewer::new()));

        // Operations experts
        m.insert("kubernetes-expert", || Box::new(KubernetesExpert::new()));
        m.insert("systemd-expert", || Box::new(SystemdExpert::new()));
        m.insert("dbus-expert", || Box::new(DbusExpert::new()));

        m
    });
