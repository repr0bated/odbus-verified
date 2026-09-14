//! Compliance Trestle Agent
//!
//! Manages compliance-as-code workflows using the OSCAL Compass / compliance-trestle pattern:
//! Git-based OSCAL document management, CI/CD integration, automated validation.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct ComplianceTrestleAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ComplianceTrestleAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("compliance-trestle"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let result = match op {
            "init_workspace" => json!({
                "operation": "init_workspace",
                "trestle_commands": [
                    "trestle init --verbose",
                    "trestle import -f nist-800-53-r5.json -o nist",
                    "trestle import -f fedramp-moderate-profile.json -o fedramp-moderate",
                    "trestle author ssp-generate --profile fedramp-moderate --output my-ssp"
                ],
                "directory_structure": {
                    "catalogs/": "OSCAL catalog documents (NIST 800-53)",
                    "profiles/": "Tailored control selections",
                    "component-definitions/": "System component compliance data",
                    "system-security-plans/": "Generated SSPs",
                    "assessment-plans/": "SAPs",
                    "assessment-results/": "SARs",
                    "plan-of-action-and-milestones/": "POA&Ms"
                },
                "ci_integration": "trestle validate runs in CI on every PR"
            }),
            "author_ssp" => json!({
                "operation": "author_ssp",
                "workflow": [
                    "1. trestle author ssp-generate — creates editable markdown stubs from profile",
                    "2. Human authors fill implementation statements in markdown",
                    "3. trestle author ssp-assemble — converts markdown back to OSCAL JSON",
                    "4. trestle validate -f ssp.json — schema + profile validation",
                    "5. CI publishes artifact to compliance repository"
                ],
                "markdown_format": "Each control family becomes a separate .md file with by-component tables",
                "input": args.unwrap_or("")
            }),
            "validate" => json!({
                "operation": "validate",
                "validation_levels": {
                    "schema": "OSCAL JSON Schema compliance",
                    "profile": "All required controls have implementation statements",
                    "rules": "Custom rules via trestle task plugins"
                },
                "commands": [
                    "trestle validate -f ssp.json -t ssp",
                    "trestle task detect-inconsistencies",
                    "oscal-cli validate --file ssp.json"
                ]
            }),
            "sync_controls" => json!({
                "operation": "sync_controls",
                "description": "Propagate upstream catalog/profile changes to SSP",
                "commands": [
                    "trestle import -f updated-catalog.json -o nist --regenerate",
                    "trestle author ssp-generate --force-overwrite-header"
                ],
                "automation": "GitHub Actions: on upstream catalog release, open PR with control diff"
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("init", "init_workspace"),
                        ("workspace", "init_workspace"),
                        ("author", "author_ssp"),
                        ("ssp", "author_ssp"),
                        ("valid", "validate"),
                        ("sync", "sync_controls"),
                        ("ci", "validate"),
                    ],
                    "author_ssp",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [init_workspace, author_ssp, validate, sync_controls, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for ComplianceTrestleAgent {
    fn agent_type(&self) -> &str {
        "compliance-trestle"
    }
    fn name(&self) -> &str {
        "Compliance Trestle"
    }
    fn description(&self) -> &str {
        "Compliance-as-code expert using OSCAL Compass / trestle: Git-managed OSCAL documents, markdown authoring workflows, CI/CD validation pipelines, upstream control sync."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "init_workspace".to_string(),
            "author_ssp".to_string(),
            "validate".to_string(),
            "sync_controls".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Compliance Trestle agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "compliance-trestle" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
