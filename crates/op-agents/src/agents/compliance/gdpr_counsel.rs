//! GDPR Counsel Agent
//!
//! GDPR, EU AI Act, CCPA, and privacy-by-design guidance.
//! Wraps the OliviaScal/PennyPrivacy/EugeneRisk compliance attorneys from op-compliance.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct GdprCounselAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl GdprCounselAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("gdpr-counsel"),
            agent_id,
        }
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "privacy_review" => {
                let mut issues: Vec<&str> = Vec::new();
                if (input.contains("email") || input.contains("phone") || input.contains("name"))
                    && !input.contains("retention")
                {
                    issues.push("PII fields detected without a retention policy");
                }
                if input.contains("log") && !input.contains("pseudonym") {
                    issues.push("Logging may capture PII — pseudonymization recommended");
                }
                json!({
                    "operation": "privacy_review",
                    "attorney": "Penny Privacy — GDPR Engine",
                    "legal_bases": ["consent", "contract", "legal_obligation", "vital_interests", "public_task", "legitimate_interests"],
                    "issues_detected": issues,
                    "recommendations": [
                        "Implement data minimization: collect only what is necessary",
                        "Define retention periods for each data category",
                        "Provide data subject rights: access, rectification, erasure, portability",
                        "Conduct DPIA if high-risk processing"
                    ],
                    "input": args.unwrap_or("")
                })
            }
            "eu_ai_act" => json!({
                "operation": "eu_ai_act",
                "attorney": "E.U.gene Risk — EU AI Act Counsel",
                "risk_categories": {
                    "unacceptable": "Prohibited — social scoring, subliminal manipulation",
                    "high_risk": "Requires conformity assessment, technical documentation, human oversight",
                    "limited_risk": "Transparency obligations (chatbot disclosure)",
                    "minimal_risk": "No obligations beyond existing law"
                },
                "high_risk_domains": [
                    "Biometric identification", "Critical infrastructure", "Education",
                    "Employment", "Essential services", "Law enforcement", "Migration"
                ],
                "key_requirements": [
                    "Technical documentation (Art. 11)",
                    "Data governance (Art. 10)",
                    "Human oversight measures (Art. 14)",
                    "Accuracy, robustness, cybersecurity (Art. 15)",
                    "Transparency to users (Art. 13)",
                    "Registration in EU database (Art. 51)"
                ],
                "effective": "Full obligations August 2, 2026"
            }),
            "dpia" => json!({
                "operation": "dpia",
                "description": "Data Protection Impact Assessment (GDPR Art. 35)",
                "required_when": [
                    "Systematic profiling of individuals",
                    "Large-scale processing of special category data",
                    "Systematic monitoring of public areas",
                    "New technologies with high risk"
                ],
                "dpia_structure": [
                    "1. Description of processing and its purposes",
                    "2. Assessment of necessity and proportionality",
                    "3. Assessment of risks to rights and freedoms",
                    "4. Measures to address risks",
                    "5. DPO consultation opinion"
                ]
            }),
            "ccpa_review" => json!({
                "operation": "ccpa_review",
                "attorney": "Cal Privacy — CCPA/CPRA Counsel",
                "consumer_rights": [
                    "Right to know (categories + specific pieces of PI)",
                    "Right to delete",
                    "Right to opt-out of sale/sharing",
                    "Right to correct",
                    "Right to limit use of sensitive PI"
                ],
                "sensitive_pi": ["SSN", "financial", "health", "biometric", "geolocation", "race/ethnicity"],
                "opt_out_signal": "Honor Global Privacy Control (GPC) as opt-out signal"
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("dpia", "dpia"),
                        ("ai act", "eu_ai_act"),
                        ("eu ai", "eu_ai_act"),
                        ("ccpa", "ccpa_review"),
                        ("cpra", "ccpa_review"),
                        ("privacy", "privacy_review"),
                        ("gdpr", "privacy_review"),
                        ("pii", "privacy_review"),
                    ],
                    "privacy_review",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [privacy_review, eu_ai_act, dpia, ccpa_review, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for GdprCounselAgent {
    fn agent_type(&self) -> &str {
        "gdpr-counsel"
    }
    fn name(&self) -> &str {
        "GDPR / EU AI Act Counsel"
    }
    fn description(&self) -> &str {
        "Privacy and regulatory compliance counsel: GDPR data minimization/retention/subject rights, EU AI Act risk categorization (full obligations Aug 2026), DPIA authoring, CCPA/CPRA opt-out flows."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "privacy_review".to_string(),
            "eu_ai_act".to_string(),
            "dpia".to_string(),
            "ccpa_review".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("GDPR Counsel agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "gdpr-counsel" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
