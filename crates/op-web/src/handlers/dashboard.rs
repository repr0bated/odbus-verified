//! Dashboard metrics backed only by committed, sealed SHM projections.

use axum::{extract::Extension, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use std::sync::Arc;

use crate::state::AppState;
use crate::state_tree;

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardMetrics {
    pub active_connections: Option<usize>,
    pub total_users: usize,
    pub mail_queue: Option<usize>,
    pub mcp_services: Option<usize>,
    /// CPU utilization is absent until a canonical producer publishes that
    /// measurement. Load average is not relabeled as CPU percentage.
    pub cpu: Option<f32>,
    pub memory: Option<f32>,
    pub network: Option<f32>,
    pub source: String,
    pub schema_version: String,
}

type DashboardError = (StatusCode, String);

/// GET /api/dashboard/metrics — return only measurements present in the
/// committed canonical `procfs` projection. Missing or invalid backing state is
/// service-unavailable rather than a fabricated zero or a direct host probe.
pub async fn dashboard_metrics_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<DashboardMetrics>, DashboardError> {
    let tree = state_tree::read_committed_tree()
        .await
        .map_err(state_unavailable)?;
    let procfs = tree.plugins.get("procfs").ok_or_else(|| {
        state_unavailable(anyhow::anyhow!("canonical procfs plugin is not sealed"))
    })?;
    let projection = procfs.state.as_ref().ok_or_else(|| {
        state_unavailable(anyhow::anyhow!(
            "canonical procfs state has not been published"
        ))
    })?;

    // The retained get_meminfo output publishes these byte-derived KiB values.
    // Reject missing or inconsistent data instead of converting defaults.
    let memory = memory_percent(projection).ok_or_else(|| {
        state_unavailable(anyhow::anyhow!(
            "procfs memory measurement is unavailable or inconsistent"
        ))
    })?;

    let total_users = state.user_store.list_users().await.len();
    Ok(Json(DashboardMetrics {
        active_connections: None,
        total_users,
        mail_queue: None,
        mcp_services: None,
        cpu: None,
        memory: Some(memory),
        network: None,
        source: "committed_procfs_projection".to_string(),
        schema_version: procfs.schema.version.clone(),
    }))
}

/// GET /api/dashboard/projections — return the complete committed public tree.
pub async fn dashboard_projections_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Result<Json<std::collections::HashMap<String, simd_json::OwnedValue>>, DashboardError> {
    state_tree::read_all()
        .await
        .map(Json)
        .map_err(state_unavailable)
}

fn state_unavailable(error: anyhow::Error) -> DashboardError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        format!("committed dashboard state unavailable: {error}"),
    )
}

fn memory_percent(projection: &simd_json::OwnedValue) -> Option<f32> {
    let memory = projection.get("memory")?;
    let total_kb = memory
        .get("total_kb")
        .and_then(|value| value.as_u64())
        .filter(|total| *total > 0)?;
    let available_kb = memory
        .get("available_kb")
        .and_then(|value| value.as_u64())
        .filter(|available| *available <= total_kb)?;
    Some(((total_kb - available_kb) as f64 * 100.0 / total_kb as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;

    #[test]
    fn derives_only_supported_memory_measurement() {
        assert_eq!(
            memory_percent(&json!({"memory":{"total_kb": 1000, "available_kb": 250}})),
            Some(75.0)
        );
        assert_eq!(
            memory_percent(&json!({"memory":{"total_kb": 0, "available_kb": 0}})),
            None
        );
        assert_eq!(
            memory_percent(&json!({"memory":{"total_kb": 100, "available_kb": 101}})),
            None
        );
        assert_eq!(memory_percent(&json!({"load1": 0.5})), None);
        assert_eq!(
            memory_percent(&json!({"total_kb": 1000, "available_kb": 250})),
            None
        );
    }
}
