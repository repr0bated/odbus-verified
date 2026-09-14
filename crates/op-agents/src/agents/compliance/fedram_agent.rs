//! FedRAMP Agent
//!
//! FedRAMP authorization package authoring: boundary definition, SSP generation,
//! control baseline selection (Low/Moderate/High), ConMon requirements.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct FedRampAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl FedRampAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("fedramp"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "select_baseline" => {
                let baseline = if input.contains("high")
                    || input.contains("dod")
                    || input.contains("classified")
                {
                    "High"
                } else if input.contains("low")
                    || input.contains("public")
                    || input.contains("saas")
                {
                    "Low"
                } else {
                    "Moderate"
                };
                json!({
                    "operation": "select_baseline",
                    "recommended_baseline": baseline,
                    "control_counts": { "Low": 125, "Moderate": 323, "High": 421 },
                    "fedramp_profiles": {
                        "Low": "https://github.com/GSA/fedramp-automation/blob/master/dist/content/rev5/baselines/json/FedRAMP_rev5_LOW-baseline-resolved-profile_catalog.json",
                        "Moderate": "https://github.com/GSA/fedramp-automation/blob/master/dist/content/rev5/baselines/json/FedRAMP_rev5_MODERATE-baseline-resolved-profile_catalog.json",
                        "High": "https://github.com/GSA/fedramp-automation/blob/master/dist/content/rev5/baselines/json/FedRAMP_rev5_HIGH-baseline-resolved-profile_catalog.json"
                    },
                    "input": args.unwrap_or("")
                })
            }
            "authorization_boundary" => json!({
                "operation": "authorization_boundary",
                "required_diagrams": ["network", "data_flow", "authorization_boundary"],
                "boundary_components": [
                    "System owner org", "CSP infrastructure", "External services (leveraged FedRAMP)",
                    "Interconnections", "User types (federal/non-federal)"
                ],
                "fedramp_ssp_sections": [
                    "9 — System Description", "10 — System Environment",
                    "11 — System Interconnections", "13 — E-Authentication"
                ],
                "oscal_mapping": "system-characteristics → authorization-boundary in SSP"
            }),
            "conmon" => json!({
                "operation": "conmon",
                "continuous_monitoring_requirements": {
                    "vulnerability_scanning": "Monthly OS/web app scans",
                    "penetration_testing": "Annual",
                    "plan_of_action": "POA&M updated monthly",
                    "incident_reporting": "Within 1 hour for US-CERT",
                    "significant_change": "Prior approval from AO required"
                },
                "automation": "OSCAL SAR + POA&M as code, submitted via FedRAMP Automation repository"
            }),
            "review_ssp" => json!({
                "operation": "review_ssp",
                "checklist": [
                    "All Moderate controls implemented, planned, or inherited",
                    "CSP-provided vs customer-responsible responsibility matrix complete",
                    "All external services are FedRAMP-authorized or have an exception",
                    "Boundary diagram matches system-implementation in OSCAL",
                    "Data categorization follows FIPS 199"
                ],
                "input": args.unwrap_or("")
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("baseline", "select_baseline"),
                        ("boundary", "authorization_boundary"),
                        ("ssp", "review_ssp"),
                        ("conmon", "conmon"),
                        ("oscal", "review_ssp"),
                    ],
                    "review_ssp",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [select_baseline, authorization_boundary, conmon, review_ssp, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for FedRampAgent {
    fn agent_type(&self) -> &str {
        "fedramp"
    }
    fn name(&self) -> &str {
        "FedRAMP Advisor"
    }
    fn description(&self) -> &str {
        "FedRAMP Rev5 authorization expert: baseline selection (Low/Moderate/High), authorization boundary definition, SSP review, ConMon requirements, OSCAL-based package authoring."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "select_baseline".to_string(),
            "authorization_boundary".to_string(),
            "conmon".to_string(),
            "review_ssp".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("FedRAMP agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "fedramp" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
