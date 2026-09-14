//! STIG Auditor Agent
//!
//! DISA STIG and SCAP compliance: XCCDF benchmark evaluation, CKL generation,
//! automated remediation via Ansible/shell, SRG mapping.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct StigAuditorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl StigAuditorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("stig-auditor"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "evaluate" => {
                let benchmark = if input.contains("rhel") || input.contains("red hat") {
                    "Red Hat Enterprise Linux STIG"
                } else if input.contains("ubuntu") {
                    "Canonical Ubuntu STIG"
                } else if input.contains("windows") {
                    "Microsoft Windows Server STIG"
                } else if input.contains("nginx") || input.contains("web") {
                    "NGINX Web Server STIG"
                } else if input.contains("postgres") || input.contains("db") {
                    "PostgreSQL STIG"
                } else {
                    "General OS STIG"
                };
                json!({
                    "operation": "evaluate",
                    "benchmark": benchmark,
                    "scap_tool": "OpenSCAP (oscap xccdf eval --profile stig ...)",
                    "output_formats": ["xccdf-results.xml", "arf.xml", "html-report"],
                    "ckl_generation": "STIG Viewer 2.x or oscap to CKL converter",
                    "finding_categories": {
                        "CAT I": "Critical — must fix immediately",
                        "CAT II": "High — should fix within 30 days",
                        "CAT III": "Medium — fix within 90 days"
                    },
                    "input": args.unwrap_or("")
                })
            }
            "remediate" => json!({
                "operation": "remediate",
                "approaches": {
                    "ansible": "ComplianceAsCode/content generates RHEL/Ubuntu STIG Ansible playbooks",
                    "shell": "oscap generates fix scripts per finding",
                    "manual": "CKL notations with compensating controls"
                },
                "commands": [
                    "oscap xccdf generate fix --fix-type ansible --result-id '' xccdf-results.xml > fix.yml",
                    "ansible-playbook fix.yml --check",
                    "ansible-playbook fix.yml"
                ],
                "exceptions": "Document accepted risk in CKL with AO signature"
            }),
            "map_srg" => json!({
                "operation": "map_srg",
                "description": "Map STIG rules to parent SRG controls and NIST 800-53",
                "srg_sources": [
                    "General Purpose OS SRG", "Application Server SRG",
                    "Database SRG", "Web Server SRG", "Network Device SRG"
                ],
                "oscal_mapping": "STIG XCCDF → OSCAL component-definition with control-mappings",
                "tools": ["DISA STIG Viewer", "oscal-cli", "ComplianceAsCode"]
            }),
            "ckl_review" => json!({
                "operation": "ckl_review",
                "ckl_statuses": ["Open", "Not_Applicable", "Not_a_Finding", "Not_Reviewed"],
                "review_checklist": [
                    "All CAT I findings are Not_a_Finding or have AO-signed exception",
                    "Finding details field explains why status was set",
                    "Comment field references ticket/POA&M entry",
                    "CKL version matches current STIG release"
                ],
                "input": args.unwrap_or("")
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("scan", "evaluate"),
                        ("xccdf", "evaluate"),
                        ("evaluat", "evaluate"),
                        ("oscal", "map_srg"),
                        ("srg", "map_srg"),
                        ("fix", "remediate"),
                        ("ckl", "ckl_review"),
                    ],
                    "evaluate",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [evaluate, remediate, map_srg, ckl_review, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for StigAuditorAgent {
    fn agent_type(&self) -> &str {
        "stig-auditor"
    }
    fn name(&self) -> &str {
        "STIG Auditor"
    }
    fn description(&self) -> &str {
        "DISA STIG/SCAP compliance expert: XCCDF benchmark evaluation, CKL review, OpenSCAP remediation, SRG-to-800-53 mapping, Ansible fix playbook generation."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "evaluate".to_string(),
            "remediate".to_string(),
            "map_srg".to_string(),
            "ckl_review".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("STIG Auditor agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "stig-auditor" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
