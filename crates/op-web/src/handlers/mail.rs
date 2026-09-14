//! Mail Server Status Handlers

use axum::{extract::Extension, response::Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct MailStatus {
    pub running: bool,
    pub smtp_port: u16,
    pub imap_port: u16,
    pub sent_today: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MailQueueItem {
    pub id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub status: String,
    pub retry_count: usize,
    pub created_at: String,
}

/// Check whether the maddy process is running by scanning `/proc/*/comm`.
/// Container processes are visible from the host procfs, so this covers
/// both containerised and host-native deployments without shelling out.
async fn is_maddy_running() -> bool {
    let mut entries = match tokio::fs::read_dir("/proc").await {
        Ok(e) => e,
        Err(_) => return false,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        if pid_str.parse::<u32>().is_err() {
            continue;
        }

        let comm_path = entry.path().join("comm");
        if let Ok(comm) = tokio::fs::read_to_string(&comm_path).await {
            if comm.trim() == "maddy" {
                return true;
            }
        }
    }

    false
}

/// GET /api/mail/status - Get mail server status
pub async fn mail_status_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<MailStatus> {
    let running = is_maddy_running().await;

    Json(MailStatus {
        running,
        smtp_port: 25,
        imap_port: 143,
        sent_today: 0, // TODO: Parse from maddy logs
    })
}

/// GET /api/mail/queue - Get mail queue
pub async fn mail_queue_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<Vec<MailQueueItem>> {
    // TODO: Query maddy's queue directory or database
    Json(vec![])
}

/// GET /api/mail/accounts - Get email accounts
pub async fn mail_accounts_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Json<Vec<String>> {
    // Return list of user emails
    let users = state.user_store.list_users().await;
    Json(users.into_iter().map(|u| u.email).collect())
}
