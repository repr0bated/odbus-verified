//! Backend Architect Agent
//!
//! Expert backend architect specializing in scalable API design,
//! microservices architecture, and distributed systems.

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

/// Backend Architect Agent
///
/// Masters REST/GraphQL/gRPC APIs, event-driven architectures,
/// service mesh patterns, and modern backend frameworks.
pub struct BackendArchitectAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl BackendArchitectAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("backend-architect"),
            agent_id,
        }
    }

    fn analyze_architecture(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let mut recommendations = Vec::new();
        let mut patterns = Vec::new();
        let mut questions = Vec::new();

        // API Design Analysis
        if input.contains("api") || input.contains("rest") || input.contains("graphql") {
            recommendations.push("Define API contracts first using OpenAPI/GraphQL schemas");
            recommendations
                .push("Implement versioning strategy (URL path, header, or content negotiation)");
            recommendations
                .push("Design consistent error response format with proper HTTP status codes");
            patterns.push("API-First Design");
        }

        // Microservices Analysis
        if input.contains("microservice") || input.contains("service") {
            recommendations.push("Define clear service boundaries using Domain-Driven Design");
            recommendations.push("Implement service discovery (Consul, etcd, or K8s native)");
            recommendations.push("Design async communication patterns for loose coupling");
            patterns.push("Microservices Architecture");
            patterns.push("Service Mesh");
        }

        // Event-Driven Analysis
        if input.contains("event") || input.contains("kafka") || input.contains("message") {
            recommendations.push("Use event sourcing for audit trail and replay capability");
            recommendations.push("Implement dead letter queues for failed message handling");
            recommendations.push("Design idempotent consumers for at-least-once delivery");
            patterns.push("Event-Driven Architecture");
            patterns.push("CQRS");
        }

        // Resilience Analysis
        if input.contains("scale") || input.contains("resilient") || input.contains("fault") {
            recommendations.push("Implement circuit breakers for external service calls");
            recommendations.push("Design for horizontal scalability with stateless services");
            recommendations.push("Add health checks (liveness, readiness) for orchestration");
            patterns.push("Circuit Breaker");
            patterns.push("Bulkhead");
        }

        // Default recommendations
        if recommendations.is_empty() {
            recommendations
                .push("Start with requirements analysis for scale and consistency needs");
            recommendations.push("Define service boundaries based on business capabilities");
            recommendations.push("Design API contracts before implementation");
            recommendations.push("Plan observability from day one (logging, metrics, tracing)");
            patterns.push("Clean Architecture");
            patterns.push("Domain-Driven Design");
        }

        // Targeted questions for chat UI integration
        if input.contains("chat-ui")
            || input.contains("huggingface")
            || input.contains("interface")
            || input.contains("ui")
            || input.contains("mcp")
            || input.contains("model")
            || input.contains("stream")
            || input.contains("auth")
        {
            questions.push(
                "Which backend is canonical for chat requests today (op-web or another service)?",
            );
            questions.push("What are the existing chat endpoints and payload formats, and are they OpenAI-compatible?");
            questions.push("How should model selection work (per user, per conversation, per request), and where is it configured?");
            questions.push("Do you need streaming responses? If yes, what protocol (SSE/WS) and token format are expected?");
            questions.push("What is the desired scope of the MCP tab (tool discovery only vs full config and execution)?");
            questions.push("What authentication model is required (API keys, session cookies, HuggingFace token passthrough)?");
            questions.push("Where should chat history persist (DB, filesystem, in-memory), and is multi-user isolation required?");
            questions.push("Where should chat-ui live: separate app with reverse proxy, static build served by op-web, or embedded route?");
        }

        let result = json!({
            "analysis": {
                "input": args.unwrap_or(""),
                "recommended_patterns": patterns,
                "recommendations": recommendations,
                "questions": questions,
                "next_steps": [
                    "Define bounded contexts and service boundaries",
                    "Create API contract specifications",
                    "Design data model and ownership",
                    "Plan inter-service communication",
                    "Set up observability infrastructure"
                ]
            },
            "architecture_principles": {
                "scalability": "Design stateless services for horizontal scaling",
                "resilience": "Implement circuit breakers, retries, timeouts",
                "observability": "Structured logging, distributed tracing, metrics",
                "security": "Defense in depth, least privilege, zero trust"
            }
        });

        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for BackendArchitectAgent {
    fn agent_type(&self) -> &str {
        "backend-architect"
    }

    fn name(&self) -> &str {
        "Backend Architect"
    }

    fn description(&self) -> &str {
        "Expert backend architect specializing in scalable API design, microservices architecture, and distributed systems."
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "design_api".to_string(),
            "design_microservices".to_string(),
            "design_event_architecture".to_string(),
            "review_architecture".to_string(),
            "analyze".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    fn get_status(&self) -> String {
        format!("Backend Architect agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "backend-architect" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "design_api"
            | "design_microservices"
            | "design_event_architecture"
            | "review_architecture"
            | "analyze" => self.analyze_architecture(task.args.as_deref()),
            _ => Err(format!(
                "Unknown operation: {}. Available: {:?}",
                task.operation,
                self.operations()
            )),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
