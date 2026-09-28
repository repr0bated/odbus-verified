//! Base Persona Agent Implementation

use async_trait::async_trait;
use simd_json::OwnedValue as Value;
#[cfg(test)]
use simd_json::json;
use simd_json::prelude::*;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use crate::security::SecurityProfile;

/// Advice-style operations that require a non-empty query.
const ADVICE_OPS: &[&str] = &[
    "consult",
    "review",
    "explain",
    "recommend",
    "advise",
    "analyze",
    "help",
];

/// Base metadata implementation for persona agents.
///
/// Persona completion is owned by the authenticated bridge. This type retains
/// the catalog role and knowledge used to compose a bridge consultation, but it
/// must never manufacture an unauthenticated model client or return canned
/// advice as if a model had answered.
pub struct PersonaAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub system_prompt: String,
    pub knowledge: String,
    pub capabilities: Vec<AgentCapability>,
    pub examples: Vec<(String, String)>,
}

impl PersonaAgent {
    /// Create a new persona agent
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        domain: &str,
        system_prompt: &str,
        knowledge: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            domain: domain.to_string(),
            system_prompt: system_prompt.to_string(),
            knowledge: knowledge.to_string(),
            capabilities: Vec::new(),
            examples: vec![],
        }
    }

    /// Add a capability
    pub fn with_capability(mut self, cap: AgentCapability) -> Self {
        self.capabilities.push(cap);
        self
    }

    /// Add an example interaction
    pub fn with_example(mut self, question: &str, answer: &str) -> Self {
        self.examples
            .push((question.to_string(), answer.to_string()));
        self
    }

    /// Generate augmented prompt for LLM (retained for callers that compose prompts)
    pub fn augmented_prompt(&self, user_query: &str) -> String {
        let mut prompt = self.system_prompt.clone();

        if !self.knowledge.is_empty() {
            prompt.push_str("\n\n## Domain Knowledge\n");
            prompt.push_str(&self.knowledge);
        }

        if !self.examples.is_empty() {
            prompt.push_str("\n\n## Example Interactions\n");
            for (q, a) in &self.examples {
                prompt.push_str(&format!("\nQ: {}\nA: {}\n", q, a));
            }
        }

        prompt.push_str(&format!("\n\n## Current Query\n{}", user_query));
        prompt
    }

    /// Extract a non-empty query from `args.query` or a string-valued `args`.
    fn extract_query(args: &Value) -> Option<String> {
        if let Some(s) = args.as_str() {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
            return None;
        }
        for key in ["query", "question", "prompt", "input", "text", "message"] {
            if let Some(s) = args.get(key).and_then(|v| v.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
        None
    }
}

#[async_trait]
impl UnifiedAgent for PersonaAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> AgentCategory {
        AgentCategory::Persona
    }

    fn capabilities(&self) -> Vec<AgentCapability> {
        self.capabilities.clone()
    }

    fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    fn knowledge_base(&self) -> Option<&str> {
        if self.knowledge.is_empty() {
            None
        } else {
            Some(&self.knowledge)
        }
    }

    fn examples(&self) -> Vec<(&str, &str)> {
        self.examples
            .iter()
            .map(|(q, a)| (q.as_str(), a.as_str()))
            .collect()
    }

    fn security_profile(&self) -> Option<&SecurityProfile> {
        None // Persona agents don't execute code
    }

    fn operations(&self) -> Vec<&str> {
        ADVICE_OPS.to_vec()
    }

    async fn execute(&self, request: AgentRequest) -> AgentResponse {
        let op = request.operation.as_str();

        if !ADVICE_OPS.contains(&op) {
            return AgentResponse::failure("unsupported operation");
        }

        let Some(query) = Self::extract_query(&request.args) else {
            return AgentResponse::failure(format!(
                "operation '{}' requires a non-empty query (args.query or string args)",
                op
            ));
        };

        let _ = query;
        AgentResponse::failure(format!(
            "persona operation '{}' requires authenticated bridge consultation via cognitive_mcp.specialist",
            op
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::agent_trait::UnifiedAgent;
    use super::*;

    fn sample_agent() -> PersonaAgent {
        PersonaAgent::new(
            "k8s-expert",
            "Kubernetes Expert",
            "K8s guidance",
            "kubernetes",
            "You are a Kubernetes expert.\n- Prefer declarative manifests",
            r#"
## Kubernetes Best Practices
- Use namespaces for isolation
- Set resource requests and limits
- Use liveness and readiness probes

## Common Patterns
- Sidecar containers
- Blue-green deployments
- Canary releases
"#,
        )
    }

    #[tokio::test]
    async fn missing_query_fails() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "consult".into(),
                args: json!({}),
                context: None,
                files: vec![],
            })
            .await;
        assert!(!resp.success);
        assert!(resp.message.contains("non-empty query"));
    }

    #[tokio::test]
    async fn unknown_operation_fails() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "run".into(),
                args: json!({"query": "pods"}),
                context: None,
                files: vec![],
            })
            .await;
        assert!(!resp.success);
        assert_eq!(resp.message, "unsupported operation");
    }

    #[tokio::test]
    async fn consult_fails_closed_without_authenticated_bridge_context() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "consult".into(),
                args: json!({"query": "How should I set resource limits on pods?"}),
                context: None,
                files: vec![],
            })
            .await;
        assert!(!resp.success);
        assert!(resp.message.contains("authenticated bridge consultation"));
    }

    #[tokio::test]
    async fn string_args_fail_closed_without_authenticated_bridge_context() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "explain".into(),
                args: json!("explain canary releases"),
                context: None,
                files: vec![],
            })
            .await;
        assert!(!resp.success);
        assert!(resp.message.contains("authenticated bridge consultation"));
    }
}
