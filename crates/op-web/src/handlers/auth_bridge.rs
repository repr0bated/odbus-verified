//! Web UI handler for PTY Auth Bridge
//!
//! Provides a web interface to:
//! - View pending authentication requests
//! - Get notification of new auth requirements
//! - Complete auth flows remotely

use axum::{
    extract::{Path, Extension},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use crate::AppState;

// =============================================================================
// TYPES
// =============================================================================

/// Pending authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAuth {
    pub id: String,
    pub tool: String,
    pub auth_type: String,
    pub url: Option<String>,
    pub device_code: Option<String>,
    pub message: String,
    pub created_at: i64,
    pub completed: bool,
}

/// State for tracking pending auths
#[derive(Default)]
pub struct AuthBridgeState {
    pub pending: RwLock<HashMap<String, PendingAuth>>,
}

// =============================================================================
// ROUTES
// =============================================================================

pub fn auth_bridge_routes() -> Router {
    Router::new()
        .route("/auth-bridge", get(auth_bridge_page))
        .route("/api/auth-bridge/pending", get(list_pending_auths))
        .route("/api/auth-bridge/webhook", post(webhook_handler))
        .route("/api/auth-bridge/:id/complete", post(complete_auth))
}

// =============================================================================
// HANDLERS
// =============================================================================

/// Main auth bridge web page
async fn auth_bridge_page() -> impl IntoResponse {
    Html(AUTH_BRIDGE_HTML)
}

/// List pending auth requests
async fn list_pending_auths(
    Extension(state): Extension<Arc<AppState>>,
) -> Json<Vec<PendingAuth>> {
    let bridge = &state.auth_bridge;
    let pending = bridge.pending.read().await;
    Json(pending.values().cloned().collect())
}

/// Webhook handler for incoming auth requirements
async fn webhook_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(payload): Json<WebhookPayload>,
) -> impl IntoResponse {
    let bridge = &state.auth_bridge;
    
    match payload.event.as_str() {
        "auth_required" => {
            if let Some(auth) = payload.auth {
                let id = auth.id.clone();
                bridge.pending.write().await.insert(id.clone(), auth);
                tracing::info!(id = %id, "New auth requirement received via webhook");
            }
        }
        "auth_completed" => {
            if let Some(auth_id) = payload.auth_id {
                bridge.pending.write().await.remove(&auth_id);
                tracing::info!(auth_id = %auth_id, "Auth completed via webhook");
            }
        }
        _ => {}
    }
    
    StatusCode::OK
}

#[derive(Debug, Deserialize)]
struct WebhookPayload {
    event: String,
    #[serde(default)]
    auth: Option<PendingAuth>,
    #[serde(default)]
    auth_id: Option<String>,
}

/// Mark an auth as completed
async fn complete_auth(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let bridge = &state.auth_bridge;
    
    if let Some(auth) = bridge.pending.write().await.get_mut(&id) {
        auth.completed = true;
        tracing::info!(id = %id, "Auth marked as completed via web UI");
        return (StatusCode::OK, "Completed");
    }
    
    (StatusCode::NOT_FOUND, "Not found")
}

// =============================================================================
// HTML PAGE
// =============================================================================

const AUTH_BRIDGE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>PTY Auth Bridge</title>
    <style>
        :root {
            --bg: #1a1a2e;
            --card: #16213e;
            --accent: #0f3460;
            --text: #e6e6e6;
            --highlight: #e94560;
        }
        
        * { box-sizing: border-box; margin: 0; padding: 0; }
        
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg);
            color: var(--text);
            min-height: 100vh;
            padding: 20px;
        }
        
        h1 {
            text-align: center;
            margin-bottom: 30px;
            color: var(--highlight);
        }
        
        .subtitle {
            text-align: center;
            color: #888;
            margin-bottom: 40px;
        }
        
        .container {
            max-width: 800px;
            margin: 0 auto;
        }
        
        .auth-card {
            background: var(--card);
            border-radius: 12px;
            padding: 24px;
            margin-bottom: 20px;
            border-left: 4px solid var(--highlight);
        }
        
        .auth-card.completed {
            border-left-color: #4caf50;
            opacity: 0.6;
        }
        
        .auth-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 16px;
        }
        
        .auth-tool {
            font-size: 1.2em;
            font-weight: bold;
            color: var(--highlight);
        }
        
        .auth-type {
            background: var(--accent);
            padding: 4px 12px;
            border-radius: 20px;
            font-size: 0.85em;
        }
        
        .auth-url {
            background: #0a0a15;
            padding: 16px;
            border-radius: 8px;
            margin: 16px 0;
            word-break: break-all;
        }
        
        .auth-url a {
            color: #4fc3f7;
            text-decoration: none;
        }
        
        .auth-url a:hover {
            text-decoration: underline;
        }
        
        .device-code {
            background: var(--highlight);
            color: white;
            padding: 8px 16px;
            border-radius: 8px;
            font-family: monospace;
            font-size: 1.3em;
            display: inline-block;
            margin: 8px 0;
        }
        
        .auth-message {
            color: #aaa;
            font-size: 0.9em;
            margin-top: 12px;
        }
        
        .btn {
            background: var(--highlight);
            color: white;
            border: none;
            padding: 10px 24px;
            border-radius: 8px;
            cursor: pointer;
            font-size: 1em;
            margin-top: 12px;
        }
        
        .btn:hover {
            background: #d63850;
        }
        
        .btn.complete {
            background: #4caf50;
        }
        
        .empty {
            text-align: center;
            padding: 60px;
            color: #666;
        }
        
        .empty-icon {
            font-size: 4em;
            margin-bottom: 20px;
        }
        
        .refresh {
            text-align: center;
            margin-top: 30px;
        }
        
        .refresh button {
            background: var(--accent);
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔐 PTY Auth Bridge</h1>
        <p class="subtitle">Pending authentication requests from headless server</p>
        
        <div id="auths"></div>
        
        <div class="refresh">
            <button class="btn" onclick="refresh()">↻ Refresh</button>
            <p style="margin-top: 10px; color: #666; font-size: 0.9em;">Auto-refreshes every 5 seconds</p>
        </div>
    </div>
    
    <script>
        async function refresh() {
            try {
                const resp = await fetch('/api/auth-bridge/pending');
                const auths = await resp.json();
                render(auths);
            } catch (e) {
                console.error('Failed to fetch:', e);
            }
        }
        
        function render(auths) {
            const container = document.getElementById('auths');
            
            if (auths.length === 0) {
                container.innerHTML = `
                    <div class="empty">
                        <div class="empty-icon">✓</div>
                        <p>No pending authentication requests</p>
                        <p style="margin-top: 10px; color: #888;">Requests from CLI tools will appear here</p>
                    </div>
                `;
                return;
            }
            
            container.innerHTML = auths.map(auth => `
                <div class="auth-card ${auth.completed ? 'completed' : ''}">
                    <div class="auth-header">
                        <span class="auth-tool">${auth.tool || 'Unknown Tool'}</span>
                        <span class="auth-type">${auth.auth_type || 'OAuth'}</span>
                    </div>
                    
                    ${auth.url ? `
                        <div class="auth-url">
                            <a href="${auth.url}" target="_blank" rel="noopener">${auth.url}</a>
                        </div>
                    ` : ''}
                    
                    ${auth.device_code ? `
                        <p>Enter this code:</p>
                        <span class="device-code">${auth.device_code}</span>
                    ` : ''}
                    
                    <p class="auth-message">${auth.message}</p>
                    
                    ${!auth.completed ? `
                        <button class="btn complete" onclick="markComplete('${auth.id}')">
                            ✓ I've completed this auth
                        </button>
                    ` : `
                        <p style="color: #4caf50; margin-top: 12px;">✓ Completed</p>
                    `}
                </div>
            `).join('');
        }
        
        async function markComplete(id) {
            try {
                await fetch(`/api/auth-bridge/${id}/complete`, { method: 'POST' });
                refresh();
            } catch (e) {
                console.error('Failed to mark complete:', e);
            }
        }
        
        // Initial load
        refresh();
        
        // Auto-refresh every 5 seconds
        setInterval(refresh, 5000);
    </script>
</body>
</html>
"##;
