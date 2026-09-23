//! Mail status is unavailable until the mail plugin publishes a projection.

use axum::{extract::Extension, response::Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct MailStatus {
    pub running: Option<bool>,
    pub smtp_port: Option<u16>,
    pub imap_port: Option<u16>,
    pub sent_today: Option<usize>,
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

pub async fn mail_status_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<MailStatus> {
    Json(MailStatus {
        running: None,
        smtp_port: None,
        imap_port: None,
        sent_today: None,
    })
}

pub async fn mail_queue_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<Option<Vec<MailQueueItem>>> {
    Json(None)
}

pub async fn mail_accounts_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Json<Vec<String>> {
    let users = state.user_store.list_users().await;
    Json(users.into_iter().map(|u| u.email).collect())
}
