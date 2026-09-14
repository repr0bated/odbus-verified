use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue as Value;
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub enum ChangeType {
    PropertySet,
    Signal,
    Deleted,
}

#[async_trait]
pub trait StatePublisher: Send + Sync + Debug {
    #[allow(clippy::too_many_arguments)]
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
