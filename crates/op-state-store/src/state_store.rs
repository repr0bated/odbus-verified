use crate::error::Result;
use crate::execution_job::ExecutionJob;
use crate::{CanonicalDbExport, StoredObject};
use async_trait::async_trait;
use uuid::Uuid;

/// Tool record from database
#[derive(Debug, Clone)]
pub struct ToolRecord {
    pub tool_name: String,
    pub definition_json: String, // Serialized ToolDefinition
    pub category: String,
    pub namespace: String,
    pub schema_version: String, // JSON Schema version
    pub source: String,         // "builtin", "dbus-session.v1", "dbus-system.v1", "mcp", "agent"
    pub created_at: String,
    pub updated_at: String,
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn save_job(&self, job: &ExecutionJob) -> Result<()>;
    async fn get_job(&self, id: Uuid) -> Result<Option<ExecutionJob>>;
    async fn update_job(&self, job: &ExecutionJob) -> Result<()>;

    async fn get_object(&self, id: &str) -> Result<Option<StoredObject>>;
    async fn upsert_object(
        &self,
        id: &str,
        object_type: &str,
        namespace: &str,
        data: &simd_json::OwnedValue,
    ) -> Result<()>;
    async fn export_canonical(&self) -> Result<CanonicalDbExport>;

    // Tool persistence (READ on startup, WRITE only on onboarding/upgrade/migration)
    async fn save_tools(&self, tools: Vec<ToolRecord>) -> Result<()>;
    async fn load_tools(&self) -> Result<Vec<ToolRecord>>;
    async fn is_tools_empty(&self) -> Result<bool>;
    async fn clear_tools(&self) -> Result<()>;
}
