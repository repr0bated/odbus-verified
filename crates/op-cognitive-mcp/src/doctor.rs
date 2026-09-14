//! 🛷 Doctor Diagnostics — R15
//!
//! Comprehensive system diagnostics: auth status, quota, memory store
//! health, session state, NotebookLM bridge status, Gemini fallback,
//! and query history.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::gemini_fallback::GeminiFallback;
use crate::memory_store::CognitiveMemoryStore;
use crate::quota::QuotaManager;
use crate::session::SessionManager;
use crate::tool_profiles;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub timestamp: String,
    pub overall_status: String,
    pub components: Vec<ComponentStatus>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub name: String,
    pub status: String,
    pub details: serde_json::Value,
}

/// Run full diagnostics across all components.
pub async fn run_diagnostics(
    memory_store: &Arc<CognitiveMemoryStore>,
    session_manager: &Arc<SessionManager>,
    quota_manager: &Arc<QuotaManager>,
    gemini: &Arc<GeminiFallback>,
) -> DiagnosticReport {
    let mut components = Vec::new();
    let mut recommendations = Vec::new();
    let mut all_ok = true;

    // 1. Memory Store
    match memory_store.get_stats().await {
        Ok(stats) => {
            components.push(ComponentStatus {
                name: "memory_store".into(),
                status: "ok".into(),
                details: serde_json::json!({
                    "total_namespaces": stats.total_namespaces,
                    "total_entries": stats.total_entries,
                    "entries_by_kind": stats.entries_by_kind,
                }),
            });
        }
        Err(e) => {
            all_ok = false;
            components.push(ComponentStatus {
                name: "memory_store".into(),
                status: "error".into(),
                details: serde_json::json!({ "error": e.to_string() }),
            });
            recommendations.push(
                "Memory store is unreachable. Check SQLite database path and permissions.".into(),
            );
        }
    }

    // 2. Session Manager
    let active = session_manager.active_count();
    let total = session_manager.count();
    components.push(ComponentStatus {
        name: "session_manager".into(),
        status: "ok".into(),
        details: serde_json::json!({
            "active_sessions": active,
            "total_sessions": total,
        }),
    });

    // 3. Quota Manager
    let (remaining, limit) = quota_manager.status().await;
    let tier = quota_manager.tier().await;
    let quota_status = if remaining == 0 { "exhausted" } else { "ok" };
    if remaining == 0 {
        recommendations.push(
            "Query quota exhausted. Consider upgrading tier or waiting for daily reset.".into(),
        );
    }
    components.push(ComponentStatus {
        name: "quota_manager".into(),
        status: quota_status.into(),
        details: serde_json::json!({
            "tier": tier.name,
            "remaining": remaining,
            "limit": limit,
        }),
    });

    // 4. Gemini Fallback
    let gemini_available = gemini.is_available().await;
    components.push(ComponentStatus {
        name: "gemini_fallback".into(),
        status: if gemini_available {
            "ok"
        } else {
            "unavailable"
        }
        .into(),
        details: serde_json::json!({
            "available": gemini_available,
        }),
    });
    if !gemini_available {
        recommendations.push("Gemini fallback unavailable. Set GEMINI_API_KEY for resilient queries when NotebookLM is down.".into());
    }

    // 5. Tool Profile
    let profile = tool_profiles::current_profile();
    let estimate = tool_profiles::token_estimate(profile);
    components.push(ComponentStatus {
        name: "tool_profile".into(),
        status: "ok".into(),
        details: serde_json::json!({
            "profile": profile.to_string(),
            "tool_count": estimate.tool_count,
            "schema_tokens": estimate.schema_tokens,
            "savings_percent": estimate.savings_percent,
        }),
    });

    // 6. Auth Status
    let auth_method =
        std::env::var("COGNITIVE_MCP_AUTH_METHOD").unwrap_or_else(|_| "chrome_profile".into());
    components.push(ComponentStatus {
        name: "auth".into(),
        status: "configured".into(),
        details: serde_json::json!({
            "method": auth_method,
        }),
    });

    let overall = if all_ok { "healthy" } else { "degraded" };

    DiagnosticReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        overall_status: overall.into(),
        components,
        recommendations,
    }
}

/// Get query history from the session manager.
pub fn get_query_history(session_manager: &SessionManager, limit: usize) -> Vec<serde_json::Value> {
    let sessions = session_manager.list_sessions();
    let mut all_turns = Vec::new();

    for session in sessions {
        for turn in &session.history {
            all_turns.push(serde_json::json!({
                "conversation_id": session.id,
                "notebook_id": session.notebook_id,
                "query": turn.query,
                "answer_preview": if turn.answer.len() > 200 {
                    format!("{}...", &turn.answer[..200])
                } else {
                    turn.answer.clone()
                },
                "timestamp": turn.timestamp.to_rfc3339(),
                "citations_count": turn.citations_count,
                "grounded": turn.grounded,
            }));
        }
    }

    // Sort by timestamp descending
    all_turns.sort_by(|a, b| {
        let ta = a["timestamp"].as_str().unwrap_or("");
        let tb = b["timestamp"].as_str().unwrap_or("");
        tb.cmp(ta)
    });

    all_turns.into_iter().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_get_empty_query_history() {
        let mgr = SessionManager::with_defaults();
        let history = get_query_history(&mgr, 10);
        assert!(history.is_empty());
    }

    #[test]
    fn should_get_query_history_with_turns() {
        let mgr = SessionManager::with_defaults();
        mgr.get_or_create("conv-1", "nb-1");
        mgr.append_turn(
            "conv-1",
            crate::session::QueryTurn {
                query: "test query".into(),
                answer: "test answer".into(),
                timestamp: chrono::Utc::now(),
                citations_count: 2,
                grounded: true,
            },
        )
        .unwrap();

        let history = get_query_history(&mgr, 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["query"], "test query");
        assert_eq!(history[0]["grounded"], true);
    }
}
