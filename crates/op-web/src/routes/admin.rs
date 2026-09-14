//! Admin Routes - System Prompt Editor and Configuration
//!
//! Provides API endpoints for:
//! - Viewing system prompt (fixed + custom parts)
//! - Editing custom prompt part
//! - Testing prompt changes

use axum::{
    extract::Extension,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::AppState;

/// Create admin routes
pub fn admin_routes() -> Router {
    Router::new()
        .route("/prompt", get(get_system_prompt))
        .route("/prompt/custom", get(get_custom_prompt))
        .route("/prompt/custom", post(set_custom_prompt))
        .route("/prompt/test", post(test_prompt))
        .route("/prompt/reload", post(reload_prompt))
        .route("/config", get(get_config))
}

// =============================================================================
// TYPES
// =============================================================================

#[derive(Serialize)]
pub struct SystemPromptResponse {
    /// The complete generated system prompt
    pub full_prompt: String,
    /// Just the fixed part (not editable)
    pub fixed_part: String,
    /// Just the custom part (editable)
    pub custom_part: String,
    /// Where the custom part was loaded from
    pub custom_source: String,
    /// Whether self-repo tools are enabled
    pub has_self_repo: bool,
    /// Self-repo path if configured
    pub self_repo_path: Option<String>,
    /// Character count
    pub char_count: usize,
    /// Estimated token count (rough)
    pub estimated_tokens: usize,
}

#[derive(Serialize)]
pub struct CustomPromptResponse {
    pub content: String,
    pub source: String,
    pub last_modified: Option<String>,
}

#[derive(Deserialize)]
pub struct SetCustomPromptRequest {
    pub content: String,
}

#[derive(Serialize)]
pub struct SetCustomPromptResponse {
    pub success: bool,
    pub message: String,
    pub saved_to: Option<String>,
}

#[derive(Deserialize)]
pub struct TestPromptRequest {
    pub custom_content: String,
    pub test_message: Option<String>,
}

#[derive(Serialize)]
pub struct TestPromptResponse {
    pub success: bool,
    pub preview: String,
    pub char_count: usize,
    pub estimated_tokens: usize,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct AdminConfigResponse {
    pub version: String,
    pub llm_provider: String,
    pub llm_model: String,
    pub self_repo_configured: bool,
    pub self_repo_path: Option<String>,
    pub custom_prompt_path: String,
    pub tool_count: usize,
}

// =============================================================================
// HANDLERS
// =============================================================================

/// GET /admin/prompt - Get full system prompt with metadata
async fn get_system_prompt(Extension(_state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let metadata = op_chat::system_prompt::get_prompt_metadata().await;
    let full_prompt = op_chat::generate_system_prompt(None).await;

    let char_count = full_prompt.content.len();
    // Rough token estimate: ~4 chars per token
    let estimated_tokens = char_count / 4;

    Json(SystemPromptResponse {
        full_prompt: full_prompt.content,
        fixed_part: metadata.fixed_part,
        custom_part: metadata.custom_part,
        custom_source: metadata.custom_source,
        has_self_repo: metadata.has_self_repo,
        self_repo_path: metadata.self_repo_path,
        char_count,
        estimated_tokens,
    })
}

/// GET /admin/prompt/custom - Get just the custom prompt part
async fn get_custom_prompt(Extension(_state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let (content, source) = op_chat::system_prompt::load_custom_prompt().await;

    // Try to get last modified time
    let last_modified = if source.starts_with("file:") {
        let path = source.strip_prefix("file:").unwrap_or(&source);
        tokio::fs::metadata(path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let datetime: chrono::DateTime<chrono::Utc> = t.into();
                datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            })
    } else {
        None
    };

    Json(CustomPromptResponse {
        content,
        source,
        last_modified,
    })
}

/// POST /admin/prompt/custom - Set the custom prompt part
async fn set_custom_prompt(
    Extension(_state): Extension<Arc<AppState>>,
    Json(request): Json<SetCustomPromptRequest>,
) -> impl IntoResponse {
    info!(
        "Admin updating custom prompt ({} chars)",
        request.content.len()
    );

    match op_chat::system_prompt::save_custom_prompt(&request.content).await {
        Ok(path) => {
            info!("Custom prompt saved to: {}", path);
            Json(SetCustomPromptResponse {
                success: true,
                message: "Custom prompt saved successfully".to_string(),
                saved_to: Some(path),
            })
        }
        Err(e) => {
            error!("Failed to save custom prompt: {}", e);
            Json(SetCustomPromptResponse {
                success: false,
                message: format!("Failed to save: {}", e),
                saved_to: None,
            })
        }
    }
}

/// POST /admin/prompt/test - Test a custom prompt without saving
async fn test_prompt(
    Extension(_state): Extension<Arc<AppState>>,
    Json(request): Json<TestPromptRequest>,
) -> impl IntoResponse {
    let mut warnings = Vec::new();

    // Build preview
    let fixed = op_chat::system_prompt::get_fixed_prompt();
    let preview = format!(
        "{}\n\n## 📝 CUSTOM INSTRUCTIONS\n<!-- Preview - not saved -->\n{}",
        fixed, request.custom_content
    );

    let char_count = preview.len();
    let estimated_tokens = char_count / 4;

    // Check for potential issues
    if request.custom_content.len() > 10000 {
        warnings.push("Custom prompt is very long (>10K chars). This may reduce context space for conversation.".to_string());
    }

    if request
        .custom_content
        .to_lowercase()
        .contains("ignore all previous")
        || request
            .custom_content
            .to_lowercase()
            .contains("disregard the above")
    {
        warnings.push("⚠️ Detected potential prompt injection pattern. Be careful with instructions that override core rules.".to_string());
    }

    if estimated_tokens > 4000 {
        warnings.push(format!("System prompt is ~{} tokens. Models with 8K context may have limited conversation space.", estimated_tokens));
    }

    // Check for forbidden command suggestions
    let forbidden = ["ovs-vsctl", "systemctl", "ip addr", "nmcli"];
    for cmd in forbidden {
        if request.custom_content.contains(cmd) {
            warnings.push(format!("Warning: Custom prompt mentions '{}'. The chatbot should use native tools instead.", cmd));
        }
    }

    Json(TestPromptResponse {
        success: true,
        preview,
        char_count,
        estimated_tokens,
        warnings,
    })
}

/// POST /admin/prompt/reload - Force reload of custom prompt
async fn reload_prompt(Extension(_state): Extension<Arc<AppState>>) -> impl IntoResponse {
    op_chat::system_prompt::invalidate_prompt_cache().await;
    info!("Prompt cache invalidated by admin");

    Json(simd_json::json!({
        "success": true,
        "message": "Prompt cache cleared. Next request will reload from disk."
    }))
}

/// GET /admin/config - Get admin configuration overview
async fn get_config(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let llm_model = state.chat_manager.current_model().await;
    let llm_provider = state.chat_manager.current_provider().await.to_string();
    let tool_count = state.tool_registry.list().await.len();

    Json(AdminConfigResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        llm_provider,
        llm_model,
        self_repo_configured: std::env::var("OP_SELF_REPO_PATH").is_ok(),
        self_repo_path: std::env::var("OP_SELF_REPO_PATH").ok(),
        custom_prompt_path: "/etc/op-dbus/custom-prompt.txt".to_string(),
        tool_count,
    })
}
