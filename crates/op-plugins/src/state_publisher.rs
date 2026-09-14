use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue as Value;

#[derive(Debug, Clone)]
pub enum ChangeType {
    PropertySet,
    Signal,
    Deleted,
}

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait StatePublisher: Send + Sync {
    async fn publish_change(
        &self,
        plugin_id: String,
        path: String,
        change_type: ChangeType,
        property: Option<String>,
        old_value: Option<Value>,
        new_value: Value,
        tags: Vec<String>,
        source: String,
    ) -> Result<()>;
}
