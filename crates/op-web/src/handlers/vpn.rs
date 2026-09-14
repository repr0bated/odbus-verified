//! VPN Status and Connection Handlers

use axum::{extract::Extension, response::Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct VpnStatus {
    pub running: bool,
    pub interface: String,
    pub active_connections: usize,
    pub peer_count: usize,
    pub bandwidth: Bandwidth,
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
    pub server_public_key: String,
    pub endpoint: String,
    pub network: String,
    pub dns: String,
}

/// Check whether a WireGuard interface is active by looking at sysfs.
fn is_wg_interface_up(interface: &str) -> bool {
    std::fs::metadata(format!("/sys/class/net/{}/wireguard", interface)).is_ok()
}

/// Read peer count and transfer totals from `/proc/net/wireguard`.
fn read_wg_proc_stats(interface: &str) -> Option<(usize, u64, u64)> {
    let data = std::fs::read_to_string("/proc/net/wireguard").ok()?;
    let mut lines = data.lines();

    while let Some(line) = lines.next() {
        // Interface line starts with "<iface>: ..."
        if let Some(rest) = line.strip_prefix(interface) {
            if !rest.starts_with(':') {
                continue;
            }

            let mut peer_count = 0;
            let mut total_rx = 0u64;
            let mut total_tx = 0u64;

            // Peer lines are tab-indented after the interface line
            for peer_line in lines.by_ref() {
                if !peer_line.starts_with('\t') {
                    // Next interface or end of file
                    break;
                }
                let trimmed = peer_line.trim_start_matches('\t');
                if !trimmed.starts_with("peer:") {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                // peer: public_key preshared_key endpoint allowed_ips latest_handshake transfer_rx transfer_tx persistent_keepalive
                if parts.len() >= 8 {
                    peer_count += 1;
                    if let Ok(rx) = parts[5].parse::<u64>() {
                        total_rx += rx;
                    }
                    if let Ok(tx) = parts[6].parse::<u64>() {
                        total_tx += tx;
                    }
                }
            }
            return Some((peer_count, total_rx, total_tx));
        }
    }
    None
}

/// GET /api/vpn/status - Get VPN server status
pub async fn vpn_status_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<VpnStatus> {
    let interface = "wg0";

    let running = is_wg_interface_up(interface);

    if !running {
        return Json(VpnStatus {
            running: false,
            interface: interface.to_string(),
            active_connections: 0,
            peer_count: 0,
            bandwidth: Bandwidth { up: 0, down: 0 },
        });
    }

    let (peer_count, total_rx, total_tx) = read_wg_proc_stats(interface).unwrap_or_else(|| {
        warn!("Failed to read /proc/net/wireguard for {}", interface);
        (0, 0, 0)
    });

    Json(VpnStatus {
        running: true,
        interface: interface.to_string(),
        active_connections: peer_count,
        peer_count,
        bandwidth: Bandwidth {
            up: total_tx,
            down: total_rx,
        },
    })
}

/// GET /api/vpn/connections - Get active VPN connections
pub async fn vpn_connections_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<Vec<VpnConnection>> {
    // TODO: Match WireGuard peers with users from database
    Json(vec![])
}

/// GET /api/vpn/config - Get VPN server configuration
pub async fn vpn_config_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<VpnConfig> {
    let interface = "wg0";

    // Get server public key from sysfs
    let server_public_key =
        std::fs::read_to_string(format!("/sys/class/net/{}/wireguard/public_key", interface))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

    // Get endpoint from environment or default
    let endpoint =
        std::env::var("VPN_ENDPOINT").unwrap_or_else(|_| "148.113.204.83:51820".to_string());

    Json(VpnConfig {
        server_public_key,
        endpoint,
        network: "10.100.0.0/24".to_string(),
        dns: "1.1.1.1".to_string(),
    })
}

// Helper structs for parsing
#[derive(Debug)]
#[allow(dead_code)]
struct WgPeer {
    public_key: String,
    rx: u64,
    tx: u64,
    last_handshake: String,
}

/// Parse `wg show dump` output into peer list
#[allow(dead_code)]
fn parse_wg_peers(data: &str) -> Vec<WgPeer> {
    let mut peers = Vec::new();

    for (i, line) in data.lines().enumerate() {
        if i == 0 {
            // Skip interface line
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7 {
            let public_key = parts[0].to_string();
            let last_handshake_ts = parts[4].parse::<i64>().unwrap_or(0);
            let rx = parts[5].parse::<u64>().unwrap_or(0);
            let tx = parts[6].parse::<u64>().unwrap_or(0);

            // Convert timestamp to relative time
            let last_handshake = if last_handshake_ts > 0 {
                let now = chrono::Utc::now().timestamp();
                let seconds_ago = now - last_handshake_ts;
                format_duration(seconds_ago)
            } else {
                "never".to_string()
            };

            peers.push(WgPeer {
                public_key,
                rx,
                tx,
                last_handshake,
            });
        }
    }

    peers
}

#[allow(dead_code)]
fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}
