//! VPN information is decoy-owned. The web server has no canonical host VPN
//! projection, so it reports unavailable fields rather than probing sysfs.

use axum::{extract::Extension, response::Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct VpnStatus {
    pub running: Option<bool>,
    pub interface: Option<String>,
    pub active_connections: Option<usize>,
    pub peer_count: Option<usize>,
    pub bandwidth: Option<Bandwidth>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bandwidth {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VpnConnection {
    pub user_id: String,
    pub email: String,
    pub ip: String,
    pub connected_since: String,
    pub rx: u64,
    pub tx: u64,
    pub last_handshake: String,
    pub public_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VpnConfig {
    pub server_public_key: Option<String>,
    pub endpoint: Option<String>,
    pub network: Option<String>,
    pub dns: Option<String>,
}

pub async fn vpn_status_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<VpnStatus> {
    Json(VpnStatus {
        running: None,
        interface: None,
        active_connections: None,
        peer_count: None,
        bandwidth: None,
    })
}

pub async fn vpn_connections_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<Option<Vec<VpnConnection>>> {
    Json(None)
}

pub async fn vpn_config_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<VpnConfig> {
    Json(VpnConfig {
        server_public_key: None,
        endpoint: None,
        network: None,
        dns: None,
    })
}
