//! Structured approval request: docs/APPROVALS-SPEC.md §4.
//!
//! The co-signer never sees free-text justification: `notes` exists so callers
//! can attach context, but it is excluded from the arguments hash and from
//! every decision input (acceptance criterion 3).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Plugin operation to execute, e.g. `ovsdb_bridge.create_bridge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRef {
    pub plugin: String,
    pub method: String,
}

/// OSCAL profile reference pinning the policy version the request is judged under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRef {
    pub profile: String,
    pub control_ids: Vec<String>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub operation: OperationRef,
    /// sha256 hex of the canonicalised arguments (see [`canonical_hash`]).
    pub arguments_hash: String,
    /// Resource under control, e.g. `"bridge:etched0"`.
    pub resource: String,
    /// Session id of the identity that will run the operation.
    pub executor: String,
    /// Principal id of the requesting owner.
    pub requested_by: String,
    pub policy_ref: PolicyRef,
    /// Free-text context. Never a decision input.
    #[serde(default)]
    pub notes: Option<String>,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds; requests must not outlive this.
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    Expired,
    BadWindow,
}

impl ApprovalRequest {
    /// Structural checks only (time window). Policy judgment is the co-signer.
    pub fn validate(&self, now: u64) -> Result<(), RequestError> {
        if self.expires_at <= self.created_at {
            return Err(RequestError::BadWindow);
        }
        if now > self.expires_at {
            return Err(RequestError::Expired);
        }
        Ok(())
    }
}

/// sha256 hex of canonical JSON: object keys sorted recursively, compact form.
/// Key order, whitespace, and duplicate-free-text placement never change the hash.
pub fn canonical_hash(arguments: &serde_json::Value) -> String {
    let canonical = canonicalise(arguments);
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical value always serialises");
    format!("{:x}", Sha256::digest(bytes))
}

fn canonicalise(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = std::collections::BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k.clone(), canonicalise(v));
            }
            let map: serde_json::Map<String, serde_json::Value> =
                sorted.into_iter().collect();
            serde_json::Value::Object(map)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items.iter().map(canonicalise).collect(),
        ),
        scalar => scalar.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_stable_across_key_order_and_whitespace() {
        let a = json!({"bridge": "etched0", "ports": ["a", "b"], "opts": {"x": 1, "y": 2}});
        let b = json!({"opts": {"y": 2, "x": 1}, "ports": ["a", "b"], "bridge": "etched0"});
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn hash_changes_with_arguments() {
        let a = json!({"bridge": "etched0"});
        let b = json!({"bridge": "ovsbr0"});
        assert_ne!(canonical_hash(&a), canonical_hash(&b));
    }
}
