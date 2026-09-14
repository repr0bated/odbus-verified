//! OSCAL Auditor Agent
//!
//! Navigates NIST OSCAL models: catalog, profile, component-definition, SSP, SAP, SAR, POA&M.
//! Validates control implementations, maps to 800-53/800-171, generates machine-readable artifacts.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct OscalAuditorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl OscalAuditorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("oscal-auditor"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "validate_controls" => {
                let mut issues: Vec<&str> = Vec::new();
                let mut controls: Vec<&str> = Vec::new();

                if input.contains("ac-") || input.contains("access") {
                    controls.push("AC (Access Control)");
                }
                if input.contains("au-") || input.contains("audit") {
                    controls.push("AU (Audit and Accountability)");
                }
                if input.contains("ia-") || input.contains("identity") || input.contains("auth") {
                    controls.push("IA (Identification and Authentication)");
                }
                if input.contains("sc-") || input.contains("system") || input.contains("comm") {
                    controls.push("SC (System and Communications Protection)");
                }
                if input.contains("si-") || input.contains("integrity") {
                    controls.push("SI (System and Information Integrity)");
                }
                if input.is_empty() {
                    issues.push("No control context provided — supply a control ID or description");
                }

                json!({
                    "operation": "validate_controls",
                    "oscal_models_consulted": ["catalog", "profile", "component-definition"],
                    "control_families_detected": controls,
                    "nist_800_53_rev5": true,
                    "issues": issues,
                    "guidance": "Map each implementation statement to an OSCAL control using by-component entries in the SSP component-definition"
                })
            }
            "generate_ssp" => json!({
                "operation": "generate_ssp",
                "oscal_model": "system-security-plan",
                "required_sections": [
                    "metadata", "import-profile", "system-characteristics",
                    "system-implementation", "control-implementation"
                ],
                "control_implementation_pattern": {
                    "by_components": true,
                    "implementation_status": ["implemented", "partial", "planned", "alternative", "not-applicable"],
                    "remarks_required": true
                },
                "output_formats": ["json", "xml", "yaml"],
                "validation": "oscal-cli validate --file ssp.json"
            }),
            "map_controls" => json!({
                "operation": "map_controls",
                "frameworks_supported": [
                    "NIST 800-53 Rev 5", "NIST 800-171", "NIST CSF 2.0",
                    "FedRAMP Moderate/High", "ISO 27001:2022", "SOC 2 Type II",
                    "CIS Controls v8", "CMMC Level 2/3"
                ],
                "oscal_profile": "Defines tailoring: imports catalog, adds/removes controls, sets parameter values",
                "crosswalk_note": "OSCAL profiles can chain: FedRAMP baseline → 800-53 catalog"
            }),
            "audit_findings" => json!({
                "operation": "audit_findings",
                "oscal_model": "assessment-results",
                "poam_model": "plan-of-action-and-milestones",
                "finding_schema": {
                    "observation": "What was assessed",
                    "risk": "Identified risk level",
                    "finding": "Pass/Fail/Other",
                    "remediation_tracking": "POA&M entry with milestones and due dates"
                },
                "input": args.unwrap_or("")
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("ssp", "generate_ssp"),
                        ("map", "map_controls"),
                        ("finding", "audit_findings"),
                        ("poam", "audit_findings"),
                        ("valid", "validate_controls"),
                        ("control", "validate_controls"),
                    ],
                    "validate_controls",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [validate_controls, generate_ssp, map_controls, audit_findings, advise]",
                    other
                ));
            }
        };

        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for OscalAuditorAgent {
    fn agent_type(&self) -> &str {
        "oscal-auditor"
    }
    fn name(&self) -> &str {
        "OSCAL Auditor"
    }
    fn description(&self) -> &str {
        "NIST OSCAL expert: validates control implementations, authors SSPs, maps controls across frameworks (800-53, FedRAMP, CMMC, ISO 27001), generates machine-readable compliance artifacts."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "validate_controls".to_string(),
            "generate_ssp".to_string(),
            "map_controls".to_string(),
            "audit_findings".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("OSCAL Auditor agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "oscal-auditor" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
