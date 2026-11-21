use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Safe correlation only. No context carries authority or an execution gate
/// across a new FUSE operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    pub operation_id: String,
    pub trace_id: String,
    pub parent_operation_id: Option<String>,
    pub tool_name: String,
    pub executor_principal_id: Option<String>,
    pub executor_session_id: Option<String>,
    pub attribution: Option<DelegationAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DelegationAttribution {
    pub requestor_principal_id: String,
    pub requestor_session_id: String,
    pub executor_principal_id: String,
    pub executor_session_id: String,
    pub policy_id: String,
    pub policy_revision: u64,
    pub run_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub root_id: String,
}

impl ExecutionContext {
    /// Reserved correlation for discovery/legacy observations, not proof of an
    /// admitted operation. Runtime supplies its actual operation UUID.
    pub fn new(tool_name: &str) -> Self {
        let operation_id = Uuid::new_v4().to_string();
        Self {
            trace_id: operation_id.clone(), operation_id,
            parent_operation_id: None, tool_name: tool_name.to_owned(),
            executor_principal_id: None, executor_session_id: None, attribution: None,
        }
    }

    pub fn new_child(parent: &Self, tool_name: &str) -> Self {
        let mut context = Self::new(tool_name);
        context.trace_id = parent.trace_id.clone();
        context.parent_operation_id = Some(parent.operation_id.clone());
        context
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(Uuid::parse_str(&self.operation_id)?.to_string() == self.operation_id,
            "execution operation id is not canonical");
        for value in [&self.trace_id, &self.tool_name] {
            anyhow::ensure!(!value.is_empty() && value.len() <= 512
                && !value.chars().any(char::is_control), "invalid execution correlation");
        }
        if let Some(parent) = &self.parent_operation_id {
            anyhow::ensure!(Uuid::parse_str(parent)?.to_string() == *parent
                && parent != &self.operation_id, "invalid parent operation correlation");
        }
        anyhow::ensure!(self.executor_principal_id.is_some() == self.executor_session_id.is_some(),
            "incomplete execution scope");
        if let Some(attribution) = &self.attribution {
            anyhow::ensure!(self.executor_principal_id.as_deref() == Some(attribution.executor_principal_id.as_str())
                && self.executor_session_id.as_deref() == Some(attribution.executor_session_id.as_str())
                && attribution.policy_revision > 0, "delegation executor attribution mismatch");
            for value in [&attribution.requestor_principal_id, &attribution.requestor_session_id,
                &attribution.executor_principal_id, &attribution.executor_session_id,
                &attribution.policy_id, &attribution.run_id, &attribution.task_id,
                &attribution.attempt_id, &attribution.root_id] {
                anyhow::ensure!(!value.is_empty() && value.len() <= 512
                    && !value.chars().any(char::is_control), "invalid delegation correlation");
            }
        }
        Ok(())
    }
}

/// Private domain result, never stored or broadcast by ExecutionTracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub result: Option<simd_json::OwnedValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    pub finished_at: chrono::DateTime<chrono::Utc>,
}
