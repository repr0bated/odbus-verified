//! Policy Enforcer Agent
//!
//! OPA/Rego policy-as-code: writes admission policies, evaluates decisions,
//! integrates with Kubernetes, CI/CD gates, and API authorization.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct PolicyEnforcerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl PolicyEnforcerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("policy-enforcer"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "write_policy" => {
                let domain = if input.contains("kubernetes") || input.contains("k8s") {
                    "kubernetes-admission"
                } else if input.contains("api") || input.contains("http") {
                    "api-authorization"
                } else if input.contains("terraform") || input.contains("iac") {
                    "iac-validation"
                } else {
                    "general"
                };
                json!({
                    "operation": "write_policy",
                    "domain": domain,
                    "rego_template": {
                        "package": "example.policy",
                        "default_deny": "default allow := false",
                        "allow_rule": "allow if { input.user.role == \"admin\" }",
                        "violation_rule": "violation[msg] { not allow; msg := \"Access denied\" }"
                    },
                    "testing": "opa test policy.rego policy_test.rego -v",
                    "input": args.unwrap_or("")
                })
            }
            "evaluate" => json!({
                "operation": "evaluate",
                "commands": [
                    "opa eval -i input.json -d policy.rego 'data.example.policy.allow'",
                    "opa run --server --addr :8181 policy.rego",
                    "curl -X POST http://localhost:8181/v1/data/example/policy/allow -d @input.json"
                ],
                "bundle_deployment": "opa build -b policy/ && upload to OPA bundle server",
                "decision_logging": "Configure data.decision_logs in OPA config"
            }),
            "kubernetes_gatekeeper" => json!({
                "operation": "kubernetes_gatekeeper",
                "description": "OPA Gatekeeper admission controller with ConstraintTemplate CRDs",
                "workflow": [
                    "1. Define ConstraintTemplate (wraps Rego policy)",
                    "2. Create Constraint resource (applies template to resources)",
                    "3. Gatekeeper webhook enforces on CREATE/UPDATE",
                    "4. Audit mode scans existing resources"
                ],
                "example_policies": [
                    "Require resource limits on all containers",
                    "Block privileged containers",
                    "Enforce label requirements",
                    "Restrict image registries to approved list"
                ]
            }),
            "c2p_bridge" => json!({
                "operation": "c2p_bridge",
                "description": "Compliance-to-Policy: map OSCAL controls → OPA/Kyverno/Falco policy",
                "c2p_providers": ["kyverno", "ocm-policy-generator", "falco"],
                "workflow": [
                    "1. Start from OSCAL component-definition with control-mappings",
                    "2. C2P generates native policy for each provider",
                    "3. Policy deployed to enforcement engine",
                    "4. Results harvested back to OSCAL assessment-results"
                ],
                "reference": "https://github.com/oscal-compass/compliance-to-policy"
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("opa", "write_policy"),
                        ("rego", "write_policy"),
                        ("kyverno", "write_policy"),
                        ("gatekeeper", "kubernetes_gatekeeper"),
                        ("kubernetes", "kubernetes_gatekeeper"),
                        ("k8s", "kubernetes_gatekeeper"),
                        ("evaluat", "evaluate"),
                        ("oscal", "c2p_bridge"),
                        ("c2p", "c2p_bridge"),
                    ],
                    "write_policy",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [write_policy, evaluate, kubernetes_gatekeeper, c2p_bridge, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for PolicyEnforcerAgent {
    fn agent_type(&self) -> &str {
        "policy-enforcer"
    }
    fn name(&self) -> &str {
        "Policy Enforcer"
    }
    fn description(&self) -> &str {
        "OPA/Rego policy-as-code expert: writes admission policies, Kubernetes Gatekeeper constraints, API authorization rules, CI/CD policy gates, and OSCAL Compliance-to-Policy (C2P) bridges."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "write_policy".to_string(),
            "evaluate".to_string(),
            "kubernetes_gatekeeper".to_string(),
            "c2p_bridge".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Policy Enforcer agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "policy-enforcer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
