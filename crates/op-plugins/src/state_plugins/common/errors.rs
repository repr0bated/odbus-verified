//! 3tched Router error taxonomy (spec §14).
//!
//! `TchedRouterError` is a schema-contract type: it derives `schemars::JsonSchema`
//! so the error shape is surfaced in MCP tool responses and D-Bus method error
//! payloads. subid: `sch.software.3tched-router-error.schema@v1`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Failure modes for 3tched Router route resolution, model selection, and execution
/// authorization. Every variant maps to a deterministic, schema-derived denial
/// reason — nothing here reflects a runtime network error (those live in the
/// Provider Adapter Layer).
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TchedRouterError {
    #[error("route not declared: {hint}")]
    RouteNotDeclared { hint: String },

    #[error("provider not declared: {provider}")]
    ProviderNotDeclared { provider: String },

    #[error("model not declared: {model}")]
    ModelNotDeclared { model: String },

    #[error("tool not declared: {tool}")]
    ToolNotDeclared { tool: String },

    #[error("route unavailable: {hint} ({reason})")]
    RouteUnavailable { hint: String, reason: String },

    #[error("privacy tier violation: required {required}, provided {provided}")]
    PrivacyTierViolation { required: String, provided: String },

    #[error("cost policy violation: {policy} ({reason})")]
    CostPolicyViolation { policy: String, reason: String },

    #[error("context window exceeded: limit {limit}, requested {requested}")]
    ContextWindowExceeded { limit: u32, requested: u32 },

    #[error("execution denied: {reason}")]
    ExecutionDenied { reason: String },

    #[error("no candidate route after filtering")]
    NoCandidateAfterFiltering,
}
