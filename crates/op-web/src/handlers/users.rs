//! User Management Handlers

use axum::{
    extract::{Extension, Path},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub wireguard_ip: String,
    pub wireguard_public_key: String,
    pub privacy_quota_bytes: u64,
    pub privacy_quota_used_bytes: u64,
    pub status: String,
    pub created_at: String,
    pub last_seen: Option<String>,
}

/// GET /api/users - List all users
pub async fn list_users_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Json<Vec<UserResponse>> {
    let users = state.user_store.list_users().await;

    Json(
        users
            .into_iter()
            .map(|u| UserResponse {
                id: u.id,
                email: u.email,
                wireguard_ip: u.assigned_ip,
                wireguard_public_key: u.wg_public_key,
                privacy_quota_bytes: u.privacy_quota_bytes,
                privacy_quota_used_bytes: u.privacy_quota_used_bytes,
                status: "active".to_string(),
                created_at: u.created_at.to_rfc3339(),
                last_seen: None, // TODO: Track from WireGuard handshakes
            })
            .collect(),
    )
}

/// GET /api/users/:id - Get user details
pub async fn get_user_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<Option<UserResponse>> {
    let users = state.user_store.list_users().await;

    let user = users
        .into_iter()
        .find(|u| u.id == user_id)
        .map(|u| UserResponse {
            id: u.id,
            email: u.email,
            wireguard_ip: u.assigned_ip,
            wireguard_public_key: u.wg_public_key,
            privacy_quota_bytes: u.privacy_quota_bytes,
            privacy_quota_used_bytes: u.privacy_quota_used_bytes,
            status: "active".to_string(),
            created_at: u.created_at.to_rfc3339(),
            last_seen: None,
        });

    Json(user)
}
