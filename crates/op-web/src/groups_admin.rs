//! Tool Groups Admin UI
//!
//! Web interface for managing tool groups - replaces the old tool picker.
//! Features:
//! - Domain-based tool groups (~5 tools each)
//! - Presets for common use cases
//! - IP-based access control display
//! - Real-time tool count tracking

use axum::{
    extract::ConnectInfo,
    response::{Html, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use tokio::sync::RwLock;
use tracing::info;

/// Tool groups configuration storage
#[derive(Debug)]
pub struct GroupsConfig {
    /// Enabled groups per profile
    profiles: RwLock<HashMap<String, EnabledGroups>>,
    /// Trusted network prefixes
    trusted_networks: RwLock<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnabledGroups {
    pub groups: HashSet<String>,
    pub preset: Option<String>,
}

const GROUPS_CONFIG_PATH: &str = "/var/lib/op-dbus/tool-groups.json";

impl GroupsConfig {
    pub fn new() -> Self {
        let mut profiles = HashMap::new();

        // Try to load from disk
        if let Ok(content) = std::fs::read_to_string(GROUPS_CONFIG_PATH) {
            if let Ok(saved) = serde_json::from_str::<HashMap<String, EnabledGroups>>(&content) {
                info!(
                    "Loaded {} tool group profiles from {}",
                    saved.len(),
                    GROUPS_CONFIG_PATH
                );
                profiles = saved;
            }
        }

        // Default profile
        if !profiles.contains_key("default") {
            let mut default_groups = HashSet::new();
            default_groups.insert("respond".to_string());
            default_groups.insert("info".to_string());
            profiles.insert(
                "default".to_string(),
                EnabledGroups {
                    groups: default_groups,
                    preset: Some("minimal".to_string()),
                },
            );
        }

        Self {
            profiles: RwLock::new(profiles),
            trusted_networks: RwLock::new(vec![]),
        }
    }

    pub async fn get_profile(&self, name: &str) -> Option<EnabledGroups> {
        self.profiles.read().await.get(name).cloned()
    }

    pub async fn set_profile(&self, name: String, config: EnabledGroups) {
        self.profiles.write().await.insert(name, config);
        self.save_to_disk().await;
    }

    pub async fn list_profiles(&self) -> Vec<String> {
        self.profiles.read().await.keys().cloned().collect()
    }

    pub async fn add_trusted_network(&self, prefix: String) {
        self.trusted_networks.write().await.push(prefix);
    }

    pub async fn get_trusted_networks(&self) -> Vec<String> {
        self.trusted_networks.read().await.clone()
    }

    async fn save_to_disk(&self) {
        if let Ok(json) = simd_json::to_string_pretty(&*self.profiles.read().await) {
            if let Err(e) = tokio::fs::write(GROUPS_CONFIG_PATH, json).await {
                tracing::error!("Failed to save groups config: {}", e);
            }
        }
    }
}

impl Default for GroupsConfig {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    pub static ref GROUPS_CONFIG: GroupsConfig = GroupsConfig::new();
}

/// Create the groups admin router
pub fn create_groups_admin_router() -> Router {
    Router::new()
        .route("/", get(groups_admin_page))
        .route("/api/groups", get(list_groups))
        .route("/api/presets", get(list_presets))
        .route("/api/profiles", get(list_profiles))
        .route("/api/profiles/:name", get(get_profile).post(save_profile))
        .route("/api/access-zone", get(get_access_zone))
        .route(
            "/api/trusted-networks",
            get(get_trusted_networks).post(add_trusted_network),
        )
}

/// Serve the groups admin HTML page
async fn groups_admin_page() -> Html<String> {
    Html(GROUPS_ADMIN_HTML.to_string())
}

/// List all available tool groups
async fn list_groups() -> Json<Value> {
    // Import from aggregator
    let groups = op_mcp_aggregator::builtin_groups();

    let mut by_domain: HashMap<String, Vec<Value>> = HashMap::new();

    for group in groups {
        let entry = by_domain.entry(group.domain.clone()).or_default();
        entry.push(json!({
            "id": group.id,
            "name": group.name,
            "description": group.description,
            "count": group.estimated_count,
            "security": format!("{:?}", group.security).to_lowercase(),
            "default_enabled": group.default_enabled,
            "dependencies": group.dependencies,
            "tags": group.tags,
        }));
    }

    // Sort groups within each domain
    for groups in by_domain.values_mut() {
        groups.sort_by(|a, b| {
            let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            a_name.cmp(b_name)
        });
    }

    // Domain order
    let domain_order = [
        "core",
        "files",
        "shell",
        "systemd",
        "network",
        "dbus",
        "monitoring",
        "git",
        "devops",
        "security",
        "business",
        "architect",
        "database",
        "ovs",
        "agents",
        "system",
    ];

    let domains: Vec<Value> = domain_order
        .iter()
        .filter_map(|d| {
            by_domain.get(*d).map(|groups| {
                json!({
                    "domain": d,
                    "groups": groups,
                    "total_tools": groups.iter()
                        .map(|g| g.get("count").and_then(|c| c.as_u64()).unwrap_or(0))
                        .sum::<u64>()
                })
            })
        })
        .collect();

    Json(json!({
        "domains": domains,
        "max_tools": 40
    }))
}

/// List available presets
async fn list_presets() -> Json<Value> {
    let presets = op_mcp_aggregator::builtin_presets();

    let presets_json: Vec<Value> = presets
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "groups": p.groups,
                "estimated_total": p.estimated_total,
                "requires_localhost": p.requires_localhost,
            })
        })
        .collect();

    Json(json!({ "presets": presets_json }))
}

/// List saved profiles
async fn list_profiles() -> Json<Value> {
    let profiles = GROUPS_CONFIG.list_profiles().await;
    Json(json!({ "profiles": profiles }))
}

/// Get a specific profile
async fn get_profile(axum::extract::Path(name): axum::extract::Path<String>) -> Json<Value> {
    match GROUPS_CONFIG.get_profile(&name).await {
        Some(config) => {
            let groups: Vec<String> = config.groups.into_iter().collect();
            Json(json!({
                "profile": name,
                "groups": groups,
                "preset": config.preset
            }))
        }
        None => Json(json!({
            "error": format!("Profile '{}' not found", name)
        })),
    }
}

#[derive(Debug, Deserialize)]
struct SaveProfileRequest {
    groups: Vec<String>,
    preset: Option<String>,
}

/// Save a profile
async fn save_profile(
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(request): Json<SaveProfileRequest>,
) -> Json<Value> {
    let config = EnabledGroups {
        groups: request.groups.into_iter().collect(),
        preset: request.preset,
    };

    let count = config.groups.len();
    GROUPS_CONFIG.set_profile(name.clone(), config).await;

    info!("Saved tool groups profile '{}' with {} groups", name, count);

    Json(json!({
        "success": true,
        "profile": name,
        "group_count": count
    }))
}

/// Get access zone for client IP
async fn get_access_zone(request: axum::extract::Request) -> Json<Value> {
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let zone = op_core::security::AccessZone::from_ip(&ip);

    Json(json!({
        "client_ip": ip,
        "zone": format!("{:?}", zone).to_lowercase(),
        "description": zone.description(),
        "can_access": {
            "public": zone.can_access(op_core::security::SecurityLevel::Public),
            "standard": zone.can_access(op_core::security::SecurityLevel::Standard),
            "elevated": zone.can_access(op_core::security::SecurityLevel::Elevated),
            "restricted": zone.can_access(op_core::security::SecurityLevel::Restricted),
        }
    }))
}

/// Get trusted networks
async fn get_trusted_networks() -> Json<Value> {
    let networks = GROUPS_CONFIG.get_trusted_networks().await;
    Json(json!({ "trusted_networks": networks }))
}

#[derive(Debug, Deserialize)]
struct AddNetworkRequest {
    prefix: String,
}

/// Add a trusted network
async fn add_trusted_network(Json(request): Json<AddNetworkRequest>) -> Json<Value> {
    GROUPS_CONFIG
        .add_trusted_network(request.prefix.clone())
        .await;
    info!("Added trusted network: {}", request.prefix);
    Json(json!({ "success": true, "prefix": request.prefix }))
}

/// The HTML page for tool groups admin
const GROUPS_ADMIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Tool Groups Admin - op-dbus</title>
    <style>
        :root {
            --bg-primary: #0a0a12;
            --bg-secondary: #12121e;
            --bg-tertiary: #1a1a2e;
            --bg-card: #16162a;
            --text-primary: #e8e8f0;
            --text-secondary: #a0a0b8;
            --text-muted: #606078;
            --accent: #6366f1;
            --accent-hover: #818cf8;
            --success: #10b981;
            --warning: #f59e0b;
            --danger: #ef4444;
            --border: #2a2a45;
            --border-light: #3a3a55;
        }
        
        * { box-sizing: border-box; margin: 0; padding: 0; }
        
        body {
            font-family: 'Inter', -apple-system, sans-serif;
            background: var(--bg-primary);
            color: var(--text-primary);
            min-height: 100vh;
            line-height: 1.5;
        }
        
        .container { max-width: 1400px; margin: 0 auto; padding: 1.5rem; }
        
        header {
            background: linear-gradient(135deg, var(--bg-secondary), var(--bg-tertiary));
            border: 1px solid var(--border);
            border-radius: 12px;
            padding: 1.5rem;
            margin-bottom: 1.5rem;
        }
        
        h1 {
            font-size: 1.75rem;
            margin-bottom: 0.25rem;
            background: linear-gradient(135deg, var(--accent), #a855f7);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        
        .subtitle { color: var(--text-secondary); font-size: 0.95rem; }
        
        .stats-row {
            display: flex;
            gap: 2rem;
            margin-top: 1rem;
            padding: 1rem;
            background: var(--bg-primary);
            border-radius: 8px;
        }
        
        .stat { text-align: center; }
        .stat-value { font-size: 1.75rem; font-weight: 700; color: var(--accent); }
        .stat-value.warning { color: var(--warning); }
        .stat-value.danger { color: var(--danger); }
        .stat-value.success { color: var(--success); }
        .stat-label { font-size: 0.8rem; color: var(--text-secondary); }
        
        .access-badge {
            display: inline-flex;
            align-items: center;
            gap: 0.5rem;
            padding: 0.5rem 1rem;
            border-radius: 20px;
            font-size: 0.85rem;
            font-weight: 500;
        }
        
        .access-badge.localhost { background: rgba(16, 185, 129, 0.2); color: var(--success); }
        .access-badge.trustedmesh { background: rgba(99, 102, 241, 0.2); color: var(--accent); }
        .access-badge.privatenetwork { background: rgba(245, 158, 11, 0.2); color: var(--warning); }
        .access-badge.public { background: rgba(239, 68, 68, 0.2); color: var(--danger); }
        
        .main-layout { display: flex; gap: 1.5rem; }
        
        .groups-column { flex: 1; }
        
        .sidebar {
            width: 320px;
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 12px;
            position: sticky;
            top: 1rem;
            max-height: calc(100vh - 2rem);
            display: flex;
            flex-direction: column;
        }
        
        .sidebar-header {
            padding: 1rem;
            border-bottom: 1px solid var(--border);
            background: var(--bg-tertiary);
            border-radius: 12px 12px 0 0;
        }
        
        .sidebar-content { flex: 1; overflow-y: auto; padding: 1rem; }
        
        .sidebar-actions {
            padding: 1rem;
            border-top: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }
        
        .presets-section { margin-bottom: 1.5rem; }
        
        .preset-btn {
            display: block;
            width: 100%;
            padding: 0.75rem;
            margin-bottom: 0.5rem;
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: 8px;
            color: var(--text-primary);
            cursor: pointer;
            text-align: left;
            transition: all 0.2s;
        }
        
        .preset-btn:hover { border-color: var(--accent); background: var(--bg-tertiary); }
        .preset-btn.active { border-color: var(--accent); background: rgba(99, 102, 241, 0.1); }
        .preset-btn.locked { opacity: 0.5; cursor: not-allowed; }
        
        .preset-name { font-weight: 600; font-size: 0.9rem; }
        .preset-desc { font-size: 0.75rem; color: var(--text-secondary); }
        .preset-count { font-size: 0.7rem; color: var(--text-muted); margin-top: 0.25rem; }
        
        .domain-section {
            background: var(--bg-secondary);
            border: 1px solid var(--border);
            border-radius: 12px;
            margin-bottom: 1rem;
            overflow: hidden;
        }
        
        .domain-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0.75rem 1rem;
            background: var(--bg-tertiary);
            cursor: pointer;
            user-select: none;
        }
        
        .domain-header:hover { background: var(--border); }
        
        .domain-name {
            font-weight: 600;
            font-size: 0.95rem;
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }
        
        .domain-icon { font-size: 1.1rem; }
        .domain-meta { font-size: 0.8rem; color: var(--text-secondary); }
        
        .domain-groups { display: none; padding: 0.5rem; }
        .domain-groups.expanded { display: block; }
        
        .group-card {
            display: flex;
            align-items: center;
            gap: 0.75rem;
            padding: 0.75rem;
            margin: 0.25rem 0;
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: 8px;
            cursor: pointer;
            transition: all 0.15s;
        }
        
        .group-card:hover { border-color: var(--border-light); }
        .group-card.selected { border-color: var(--accent); background: rgba(99, 102, 241, 0.1); }
        .group-card.locked { opacity: 0.6; cursor: not-allowed; }
        
        .group-checkbox {
            width: 18px;
            height: 18px;
            accent-color: var(--accent);
        }
        
        .group-info { flex: 1; min-width: 0; }
        .group-name { font-weight: 500; font-size: 0.9rem; }
        .group-desc { font-size: 0.75rem; color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
        
        .group-badges { display: flex; gap: 0.25rem; flex-shrink: 0; }
        
        .badge {
            padding: 0.15rem 0.4rem;
            font-size: 0.65rem;
            border-radius: 4px;
            font-weight: 500;
        }
        
        .badge-count { background: var(--bg-tertiary); color: var(--text-secondary); }
        .badge-public { background: rgba(16, 185, 129, 0.2); color: var(--success); }
        .badge-standard { background: rgba(99, 102, 241, 0.2); color: var(--accent); }
        .badge-elevated { background: rgba(245, 158, 11, 0.2); color: var(--warning); }
        .badge-restricted { background: rgba(239, 68, 68, 0.2); color: var(--danger); }
        
        .selected-list { list-style: none; }
        
        .selected-item {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0.4rem 0.6rem;
            margin-bottom: 0.25rem;
            background: var(--bg-primary);
            border-radius: 4px;
            font-size: 0.85rem;
        }
        
        .remove-btn {
            color: var(--text-muted);
            cursor: pointer;
            font-size: 1rem;
        }
        .remove-btn:hover { color: var(--danger); }
        
        button {
            padding: 0.6rem 1rem;
            border: none;
            border-radius: 6px;
            font-size: 0.9rem;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.2s;
        }
        
        .btn-primary { background: var(--accent); color: white; }
        .btn-primary:hover { background: var(--accent-hover); }
        .btn-primary:disabled { background: var(--border); cursor: not-allowed; }
        
        .btn-secondary { background: var(--bg-tertiary); color: var(--text-primary); border: 1px solid var(--border); }
        .btn-secondary:hover { background: var(--border); }
        
        input[type="text"], select {
            width: 100%;
            padding: 0.6rem;
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: 6px;
            color: var(--text-primary);
            font-size: 0.9rem;
        }
        
        input:focus, select:focus { outline: none; border-color: var(--accent); }
        
        .config-output {
            margin-top: 1rem;
            padding: 1rem;
            background: var(--bg-primary);
            border: 1px solid var(--border);
            border-radius: 8px;
            display: none;
        }
        
        .config-output.show { display: block; }
        
        .config-output pre {
            background: var(--bg-secondary);
            padding: 1rem;
            border-radius: 6px;
            font-size: 0.8rem;
            overflow-x: auto;
            margin-top: 0.5rem;
        }
        
        .json-key { color: #a78bfa; }
        .json-string { color: #34d399; }
        
        @media (max-width: 1024px) {
            .main-layout { flex-direction: column; }
            .sidebar { width: 100%; position: static; max-height: none; }
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>🔧 Tool Groups Admin</h1>
            <p class="subtitle">Manage tool groups by domain (~5 tools each) • Stay under Cursor's 40-tool limit</p>
            
            <div class="stats-row">
                <div class="stat">
                    <div class="stat-value" id="enabled-count">0</div>
                    <div class="stat-label">Groups Enabled</div>
                </div>
                <div class="stat">
                    <div class="stat-value" id="tool-count">0</div>
                    <div class="stat-label">Total Tools</div>
                </div>
                <div class="stat">
                    <div class="stat-value success" id="remaining">40</div>
                    <div class="stat-label">Remaining</div>
                </div>
                <div style="flex: 1;"></div>
                <div class="stat">
                    <div class="access-badge" id="access-zone">
                        <span>🔒</span>
                        <span>Detecting...</span>
                    </div>
                    <div class="stat-label">Your Access Level</div>
                </div>
            </div>
        </header>
        
        <div class="main-layout">
            <div class="groups-column">
                <div id="domains-container">
                    <!-- Domains populated by JS -->
                </div>
            </div>
            
            <div class="sidebar">
                <div class="sidebar-header">
                    <strong>Configuration</strong>
                </div>
                <div class="sidebar-content">
                    <div class="presets-section">
                        <h4 style="margin-bottom: 0.5rem; font-size: 0.85rem; color: var(--text-secondary);">Quick Presets</h4>
                        <div id="presets-container">
                            <!-- Presets populated by JS -->
                        </div>
                    </div>
                    
                    <h4 style="margin-bottom: 0.5rem; font-size: 0.85rem; color: var(--text-secondary);">
                        Enabled Groups (<span id="sidebar-count">0</span>)
                    </h4>
                    <ul class="selected-list" id="selected-list">
                        <!-- Populated by JS -->
                    </ul>
                </div>
                <div class="sidebar-actions">
                    <div style="display: flex; gap: 0.5rem;">
                        <select id="saved-profiles" style="flex: 1;">
                            <option value="">Load profile...</option>
                        </select>
                        <button class="btn-secondary" onclick="loadProfile()">Load</button>
                    </div>
                    <div style="display: flex; gap: 0.5rem;">
                        <input type="text" id="profile-name" placeholder="Profile name" value="default" style="flex: 1;">
                        <button class="btn-primary" onclick="saveProfile()">💾 Save</button>
                    </div>
                    
                    <div class="config-output" id="config-output">
                        <strong>MCP Endpoint:</strong>
                        <pre id="endpoint-display"></pre>
                        <button class="btn-secondary" style="width: 100%; margin-top: 0.5rem;" onclick="copyConfig()">📋 Copy Config</button>
                    </div>
                </div>
            </div>
        </div>
    </div>
    
    <script>
        const MAX_TOOLS = 40;
        let allGroups = {};
        let enabledGroups = new Set();
        let groupCounts = {};
        let accessZone = 'public';
        let currentPreset = null;
        
        const domainIcons = {
            'core': '💎', 'files': '📁', 'shell': '💻', 'systemd': '⚙️',
            'network': '🌐', 'dbus': '🔌', 'monitoring': '📊', 'git': '📦',
            'devops': '🚀', 'security': '🔒', 'business': '💼', 'architect': '🏗️',
            'database': '🗄️', 'ovs': '🔀', 'agents': '🤖', 'system': '⚠️'
        };
        
        async function init() {
            await Promise.all([
                loadGroups(),
                loadPresets(),
                loadAccessZone(),
                loadSavedProfiles()
            ]);
        }
        
        async function loadGroups() {
            const res = await fetch('/groups-admin/api/groups');
            const data = await res.json();
            
            const container = document.getElementById('domains-container');
            container.innerHTML = '';
            
            data.domains.forEach(domain => {
                const icon = domainIcons[domain.domain] || '📦';
                const isSystem = domain.domain === 'system';
                
                const section = document.createElement('div');
                section.className = 'domain-section';
                section.innerHTML = `
                    <div class="domain-header" onclick="toggleDomain('${domain.domain}')">
                        <span class="domain-name">
                            <span class="domain-icon">${icon}</span>
                            ${domain.domain.charAt(0).toUpperCase() + domain.domain.slice(1)}
                        </span>
                        <span class="domain-meta">${domain.groups.length} groups • ~${domain.total_tools} tools</span>
                    </div>
                    <div class="domain-groups" id="domain-${domain.domain}">
                        ${domain.groups.map(g => {
                            allGroups[g.id] = g;
                            groupCounts[g.id] = g.count;
                            const locked = !canAccessSecurity(g.security);
                            return `
                                <div class="group-card ${locked ? 'locked' : ''}" 
                                     data-id="${g.id}" 
                                     onclick="${locked ? '' : `toggleGroup('${g.id}')`}">
                                    <input type="checkbox" class="group-checkbox" 
                                           id="cb-${g.id}" 
                                           ${locked ? 'disabled' : ''}
                                           onchange="toggleGroup('${g.id}')">
                                    <div class="group-info">
                                        <div class="group-name">${g.name}</div>
                                        <div class="group-desc">${g.description}</div>
                                    </div>
                                    <div class="group-badges">
                                        <span class="badge badge-count">~${g.count}</span>
                                        <span class="badge badge-${g.security}">${g.security}</span>
                                    </div>
                                </div>
                            `;
                        }).join('')}
                    </div>
                `;
                container.appendChild(section);
            });
            
            // Expand core by default
            document.getElementById('domain-core')?.classList.add('expanded');
        }
        
        async function loadPresets() {
            const res = await fetch('/groups-admin/api/presets');
            const data = await res.json();
            
            const container = document.getElementById('presets-container');
            container.innerHTML = data.presets.map(p => {
                const locked = p.requires_localhost && accessZone !== 'localhost' && accessZone !== 'trustedmesh';
                return `
                    <button class="preset-btn ${locked ? 'locked' : ''}" 
                            data-preset="${p.id}"
                            onclick="${locked ? '' : `applyPreset('${p.id}', ${JSON.stringify(p.groups)})`}"
                            ${locked ? 'disabled' : ''}>
                        <div class="preset-name">${p.name}</div>
                        <div class="preset-desc">${p.description}</div>
                        <div class="preset-count">${p.groups.length} groups • ~${p.estimated_total} tools${locked ? ' • 🔒 localhost only' : ''}</div>
                    </button>
                `;
            }).join('');
        }
        
        async function loadAccessZone() {
            const res = await fetch('/groups-admin/api/access-zone');
            const data = await res.json();
            accessZone = data.zone;
            
            const badge = document.getElementById('access-zone');
            const icons = { localhost: '🏠', trustedmesh: '🔗', privatenetwork: '🏢', public: '🌍' };
            badge.className = `access-badge ${data.zone}`;
            badge.innerHTML = `<span>${icons[data.zone] || '🔒'}</span><span>${data.description}</span>`;
        }
        
        async function loadSavedProfiles() {
            const res = await fetch('/groups-admin/api/profiles');
            const data = await res.json();
            
            const select = document.getElementById('saved-profiles');
            select.innerHTML = '<option value="">Load profile...</option>';
            data.profiles.forEach(name => {
                select.innerHTML += `<option value="${name}">${name}</option>`;
            });
        }
        
        function canAccessSecurity(level) {
            if (accessZone === 'localhost' || accessZone === 'trustedmesh') return true;
            if (accessZone === 'privatenetwork') return level !== 'restricted';
            return level === 'public' || level === 'standard';
        }
        
        function toggleDomain(domain) {
            document.getElementById(`domain-${domain}`).classList.toggle('expanded');
        }
        
        function toggleGroup(id, forceState) {
            const group = allGroups[id];
            if (!group || !canAccessSecurity(group.security)) return;
            
            const isEnabled = forceState !== undefined ? forceState : !enabledGroups.has(id);
            const count = groupCounts[id] || 0;
            
            if (isEnabled) {
                if (getTotalTools() + count > MAX_TOOLS) {
                    alert(`Cannot enable "${group.name}" - would exceed ${MAX_TOOLS} tool limit`);
                    return;
                }
                enabledGroups.add(id);
                // Enable dependencies
                (group.dependencies || []).forEach(dep => {
                    if (!enabledGroups.has(dep)) toggleGroup(dep, true);
                });
            } else {
                enabledGroups.delete(id);
            }
            
            currentPreset = null;
            updateUI();
        }
        
        function applyPreset(presetId, groups) {
            enabledGroups.clear();
            groups.forEach(g => {
                if (allGroups[g] && canAccessSecurity(allGroups[g].security)) {
                    enabledGroups.add(g);
                }
            });
            currentPreset = presetId;
            updateUI();
            
            document.querySelectorAll('.preset-btn').forEach(btn => {
                btn.classList.toggle('active', btn.dataset.preset === presetId);
            });
        }
        
        function getTotalTools() {
            return Array.from(enabledGroups).reduce((sum, id) => sum + (groupCounts[id] || 0), 0);
        }
        
        function updateUI() {
            // Update checkboxes
            document.querySelectorAll('.group-card').forEach(card => {
                const id = card.dataset.id;
                const enabled = enabledGroups.has(id);
                card.classList.toggle('selected', enabled);
                const cb = card.querySelector('.group-checkbox');
                if (cb) cb.checked = enabled;
            });
            
            // Update stats
            const total = getTotalTools();
            const remaining = MAX_TOOLS - total;
            
            document.getElementById('enabled-count').textContent = enabledGroups.size;
            document.getElementById('tool-count').textContent = total;
            document.getElementById('remaining').textContent = remaining;
            document.getElementById('sidebar-count').textContent = enabledGroups.size;
            
            const remEl = document.getElementById('remaining');
            remEl.className = 'stat-value ' + (remaining <= 0 ? 'danger' : remaining <= 10 ? 'warning' : 'success');
            
            // Update selected list
            const list = document.getElementById('selected-list');
            const sorted = Array.from(enabledGroups).sort();
            list.innerHTML = sorted.map(id => {
                const g = allGroups[id];
                return `<li class="selected-item">
                    <span>${g?.name || id} <small style="color: var(--text-muted);">(~${groupCounts[id] || 0})</small></span>
                    <span class="remove-btn" onclick="toggleGroup('${id}', false)">×</span>
                </li>`;
            }).join('');
        }
        
        async function loadProfile() {
            const name = document.getElementById('saved-profiles').value;
            if (!name) return;
            
            const res = await fetch(`/groups-admin/api/profiles/${name}`);
            const data = await res.json();
            
            if (data.error) {
                alert(data.error);
                return;
            }
            
            enabledGroups.clear();
            data.groups.forEach(g => {
                if (allGroups[g] && canAccessSecurity(allGroups[g].security)) {
                    enabledGroups.add(g);
                }
            });
            
            document.getElementById('profile-name').value = name;
            currentPreset = data.preset;
            updateUI();
        }
        
        async function saveProfile() {
            const name = document.getElementById('profile-name').value.trim() || 'default';
            
            const res = await fetch(`/groups-admin/api/profiles/${name}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    groups: Array.from(enabledGroups),
                    preset: currentPreset
                })
            });
            
            const data = await res.json();
            
            if (data.success) {
                const baseUrl = window.location.origin;
                const endpoint = baseUrl + data.mcp_endpoint;
                
                const config = {
                    mcpServers: {
                        [`op-dbus-${name}`]: {
                            url: endpoint
                        }
                    }
                };
                
                document.getElementById('endpoint-display').innerHTML = syntaxHighlight(JSON.stringify(config, null, 2));
                document.getElementById('config-output').classList.add('show');
                
                await loadSavedProfiles();
            }
        }
        
        function copyConfig() {
            const text = document.getElementById('endpoint-display').textContent;
            navigator.clipboard.writeText(text);
            alert('Config copied!');
        }
        
        function syntaxHighlight(json) {
            return json.replace(/(".*?")(:|,)?/g, (match, key, suffix) => {
                if (suffix === ':') return `<span class="json-key">${key}</span>:`;
                return `<span class="json-string">${key}</span>${suffix || ''}`;
            });
        }
        
        init();
    </script>
</body>
</html>
"##;
