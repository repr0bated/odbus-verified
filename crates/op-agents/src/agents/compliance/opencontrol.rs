//! OpenControl Agent
//!
//! OpenControl YAML schema, compliance-masonry, SSP generation,
//! FedRAMP and NIST 800-53 control narratives.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct OpenControlAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl OpenControlAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("opencontrol"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let result = match op {
            "scaffold" => json!({
                "operation": "scaffold",
                "opencontrol_yaml": {
                    "schema_version": "3.1.0",
                    "name": "System Name",
                    "metadata": {
                        "description": "Brief system description",
                        "maintainers": ["team@example.com"]
                    },
                    "components": ["./components/web-server", "./components/database"],
                    "standards": {
                        "NIST-800-53": "./standards/NIST-800-53.yaml",
                        "PCI-DSS": "./standards/PCI-DSS.yaml"
                    }
                },
                "commands": [
                    "compliance-masonry get",
                    "compliance-masonry build gitbook -e ./exports",
                    "compliance-masonry build docx -e ./exports"
                ]
            }),
            "write_narrative" => json!({
                "operation": "write_narrative",
                "component_yaml_example": {
                    "schema_version": "3.1.0",
                    "name": "NGINX Web Server",
                    "key": "NGINX",
                    "satisfies": [{
                        "standard_key": "NIST-800-53",
                        "control_key": "AC-2",
                        "implementation_statuses": ["partial"],
                        "control_origins": ["service provider system specific"],
                        "narrative": [{
                            "key": "a",
                            "text": "NGINX enforces access control through..."
                        }]
                    }]
                },
                "input": args.unwrap_or("")
            }),
            "convert_to_oscal" => json!({
                "operation": "convert_to_oscal",
                "tool": "compliance-io (CivicActions)",
                "command": "compliance-io convert --from opencontrol --to oscal-component-definition --output component.json",
                "result": "OSCAL component-definition JSON with control-implementations",
                "reference": "https://github.com/CivicActions/compliance-io"
            }),
            "generate_ssp" => json!({
                "operation": "generate_ssp",
                "tools": [
                    "compliance-masonry build gitbook — human-readable GitBook SSP",
                    "gocomply/fedramp — converts OpenControl to OSCAL FedRAMP SSP",
                    "CivicActions ssp-toolkit — NIST RMF 800-53 Rev4 SSP"
                ],
                "fedramp_command": "fedramp convert --source opencontrol.yaml --output fedramp-ssp.json",
                "input": args.unwrap_or("")
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("scaffold", "scaffold"),
                        ("narrative", "write_narrative"),
                        ("convert", "convert_to_oscal"),
                        ("oscal", "convert_to_oscal"),
                        ("ssp", "generate_ssp"),
                    ],
                    "convert_to_oscal",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [scaffold, write_narrative, convert_to_oscal, generate_ssp, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for OpenControlAgent {
    fn agent_type(&self) -> &str {
        "opencontrol"
    }
    fn name(&self) -> &str {
        "OpenControl Expert"
    }
    fn description(&self) -> &str {
        "OpenControl YAML schema and compliance-masonry expert: scaffolds repositories, writes control narratives, generates GitBook/DOCX SSPs, converts OpenControl to OSCAL component-definition."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "scaffold".to_string(),
            "write_narrative".to_string(),
            "convert_to_oscal".to_string(),
            "generate_ssp".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("OpenControl agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "opencontrol" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
