//! Base Persona Agent Implementation

use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};

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

/// Base implementation for persona agents (domain guidance from embedded knowledge)
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

    /// Split embedded knowledge into markdown `##` sections.
    fn knowledge_sections(knowledge: &str) -> Vec<(String, String)> {
        let mut sections = Vec::new();
        let mut current_title = String::new();
        let mut current_body = String::new();

        for line in knowledge.lines() {
            if let Some(rest) = line.strip_prefix("## ") {
                if !current_title.is_empty() || !current_body.trim().is_empty() {
                    sections.push((current_title, current_body.trim().to_string()));
                }
                current_title = rest.trim().to_string();
                current_body.clear();
            } else {
                current_body.push_str(line);
                current_body.push('\n');
            }
        }
        if !current_title.is_empty() || !current_body.trim().is_empty() {
            sections.push((current_title, current_body.trim().to_string()));
        }
        sections
    }

    /// Significant lowercase tokens from the query (length ≥ 3, not stopwords).
    fn query_tokens(query: &str) -> Vec<String> {
        const STOP: &[&str] = &[
            "the", "and", "for", "with", "how", "what", "when", "where", "why", "this", "that",
            "from", "into", "about", "please", "help", "need", "want", "can", "you", "are", "is",
            "a", "an", "of", "to", "in", "on", "or", "be", "do", "does", "did",
        ];
        query
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() >= 3 && !STOP.contains(&t.as_str()))
            .collect()
    }

    /// Select knowledge sections that match query keywords (fallback: first sections).
    fn relevant_knowledge(&self, query: &str) -> Vec<(String, String)> {
        let sections = Self::knowledge_sections(&self.knowledge);
        if sections.is_empty() {
            return Vec::new();
        }
        let tokens = Self::query_tokens(query);
        if tokens.is_empty() {
            return sections.into_iter().take(2).collect();
        }

        let mut matched: Vec<(String, String)> = sections
            .iter()
            .filter(|(title, body)| {
                let hay = format!("{title}\n{body}").to_lowercase();
                tokens.iter().any(|t| hay.contains(t))
            })
            .cloned()
            .collect();

        if matched.is_empty() {
            matched = sections.into_iter().take(2).collect();
        }
        matched
    }

    /// Bullet recommendations drawn from matched knowledge section lines.
    fn recommendations_from_sections(sections: &[(String, String)], limit: usize) -> Vec<String> {
        let mut out = Vec::new();
        for (_title, body) in sections {
            for line in body.lines() {
                let trimmed = line.trim();
                let bullet = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                    .unwrap_or("");
                if !bullet.is_empty() {
                    out.push(bullet.to_string());
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// Build structured domain guidance from system prompt + keyword-matched knowledge.
    fn domain_guidance(&self, operation: &str, query: &str) -> Value {
        let sections = self.relevant_knowledge(query);
        let recommendations = Self::recommendations_from_sections(&sections, 8);

        let mut answer = String::new();
        answer.push_str(&format!(
            "Domain guidance from embedded {} knowledge (not an LLM completion) for operation `{}`.\n\n",
            self.domain, operation
        ));
        answer.push_str(&format!("Regarding: \"{}\"\n\n", query));

        let role = self.system_prompt.lines().find(|l| !l.trim().is_empty());
        if let Some(line) = role {
            answer.push_str("Role framing: ");
            answer.push_str(line.trim());
            answer.push_str("\n\n");
        }

        if sections.is_empty() {
            answer.push_str(
                "No structured knowledge sections were available; apply the agent role framing above to the query.\n",
            );
        } else {
            answer.push_str("Relevant knowledge applied to the query:\n");
            for (title, body) in &sections {
                if !title.is_empty() {
                    answer.push_str(&format!("\n### {}\n", title));
                }
                answer.push_str(body);
                answer.push('\n');
            }
        }

        if recommendations.is_empty() {
            // Derive actionable lines from system prompt bullets if knowledge had none
            let from_prompt = Self::recommendations_from_sections(
                &[(String::new(), self.system_prompt.clone())],
                5,
            );
            let recs = if from_prompt.is_empty() {
                vec![format!(
                    "Apply {} best practices from this agent's embedded knowledge to: {}",
                    self.domain, query
                )]
            } else {
                from_prompt
            };
            json!({
                "query": query,
                "domain": self.domain,
                "agent": self.id,
                "operation": operation,
                "source": "embedded_knowledge",
                "answer": answer,
                "recommendations": recs,
                "matched_sections": sections.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>(),
            })
        } else {
            json!({
                "query": query,
                "domain": self.domain,
                "agent": self.id,
                "operation": operation,
                "source": "embedded_knowledge",
                "answer": answer,
                "recommendations": recommendations,
                "matched_sections": sections.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>(),
            })
        }
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

        let data = self.domain_guidance(op, &query);
        AgentResponse::success(
            data,
            format!(
                "Domain guidance from embedded {} knowledge (not an LLM completion)",
                self.domain
            ),
        )
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
    async fn consult_applies_keyword_matched_knowledge() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "consult".into(),
                args: json!({"query": "How should I set resource limits on pods?"}),
                context: None,
                files: vec![],
            })
            .await;
        assert!(resp.success, "{}", resp.message);
        assert_eq!(
            resp.data.get("query").and_then(|v| v.as_str()),
            Some("How should I set resource limits on pods?")
        );
        assert_eq!(
            resp.data.get("domain").and_then(|v| v.as_str()),
            Some("kubernetes")
        );
        assert_eq!(
            resp.data.get("agent").and_then(|v| v.as_str()),
            Some("k8s-expert")
        );
        let answer = resp
            .data
            .get("answer")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(answer.contains("not an LLM completion"));
        assert!(answer.contains("resource") || answer.contains("Best Practices"));
        let recs = resp.data.get("recommendations");
        assert!(recs.is_some());
        assert!(!resp.message.to_lowercase().contains("prompt augmented"));
    }

    #[tokio::test]
    async fn string_args_accepted_as_query() {
        let agent = sample_agent();
        let resp = agent
            .execute(AgentRequest {
                operation: "explain".into(),
                args: json!("explain canary releases"),
                context: None,
                files: vec![],
            })
            .await;
        assert!(resp.success, "{}", resp.message);
        let answer = resp
            .data
            .get("answer")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            answer.contains("Canary") || answer.contains("Patterns") || answer.contains("canary")
        );
    }
}
