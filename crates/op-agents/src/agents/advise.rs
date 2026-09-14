//! Shared natural-language advise helpers for domain agents.
//!
//! Operations like `advise` / `consult` / `explain` / `recommend` must answer the
//! caller's question by routing into a real specialized `analyze` operation —
//! never return a static capability card or canned advice stub.

use simd_json::prelude::*;

/// True when the operation is a natural-language advice alias.
pub fn is_advise_op(op: &str) -> bool {
    matches!(
        op,
        "advise" | "consult" | "explain" | "recommend" | "analyze" | "help"
    )
}

/// Extract a user-facing query string from free-form args (raw string or JSON).
pub fn extract_query(args: Option<&str>) -> String {
    let raw = args.unwrap_or("").trim();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with('{') {
        let mut bytes = raw.as_bytes().to_vec();
        if let Ok(v) = simd_json::to_owned_value(&mut bytes) {
            for key in ["query", "question", "prompt", "input", "text", "message"] {
                if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        return s.trim().to_string();
                    }
                }
            }
        }
    }
    raw.to_string()
}

/// Keyword-route a free-form advise query onto a more specific operation name.
pub fn route_advise_to_op(query: &str, routes: &[(&str, &str)], default_op: &str) -> String {
    let q = query.to_lowercase();
    for (needle, op) in routes {
        if q.contains(needle) {
            return (*op).to_string();
        }
    }
    default_op.to_string()
}
