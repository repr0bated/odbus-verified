//! Canonical method accountability envelope, independent of domain schemas.
//! The admitted plugin still owns the schema of `result`.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_METHOD_ARGUMENT_BYTES: usize = 1024 * 1024;
pub const MAX_METHOD_RECEIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MethodEventProof {
    #[schemars(range(min = 1))]
    pub event_id: u64,
    #[schemars(length(min = 32, max = 32), regex(pattern = "^[0-9a-f]{32}$"))]
    pub event_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MethodRecoveryStatus {
    Completed,
    Failed,
    Unknown,
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetMethodReceiptInput {
    #[schemars(
        length(min = 36, max = 36),
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    pub operation_id: String,
}

/// Caller-private recovery metadata, never a copy of a private domain result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MethodReceiptRecovery {
    #[schemars(
        length(min = 36, max = 36),
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    pub operation_id: String,
    pub status: MethodRecoveryStatus,
    pub plugin_id: Option<String>,
    pub method: Option<String>,
    pub attempt: Option<MethodEventProof>,
    pub terminal: Option<MethodEventProof>,
    pub result_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MethodExecutionReceipt {
    pub success: bool,
    pub operation_id: String,
    pub outcome: String,
    pub event_id: u64,
    pub event_hash: String,
    pub terminal_event_id: u64,
    pub terminal_event_hash: String,
    pub plugin_id: String,
    pub method: String,
    pub result: Value,
}

pub fn valid_event_hash(hash: &str) -> bool {
    // ChainEvent::compute_hash delegates to event_chain::compute_hash, whose
    // persisted wire format is lowercase hexadecimal MD5 (32 characters).
    // This is an audit locator, not a plugin's BLAKE3 schema/content hash.
    hash.len() == 32
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl MethodExecutionReceipt {
    /// Check transport attribution and both durable audit references. This is
    /// not a grant or independent ledger lookup: the authenticated canonical
    /// transport supplies the proof; callers must keep their pinned identity.
    pub fn validate_attribution(&self, plugin: &str, method: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.success && self.outcome == "completed",
            "method outcome is not completed"
        );
        anyhow::ensure!(
            self.plugin_id == plugin && self.method == method,
            "method receipt attribution mismatch"
        );
        let operation = uuid::Uuid::parse_str(&self.operation_id)?;
        anyhow::ensure!(
            operation.to_string() == self.operation_id,
            "noncanonical operation id"
        );
        anyhow::ensure!(
            self.event_id > 0 && self.terminal_event_id > self.event_id,
            "method receipt event ordering invalid"
        );
        anyhow::ensure!(
            valid_event_hash(&self.event_hash) && valid_event_hash(&self.terminal_event_hash),
            "method receipt event proof invalid"
        );
        Ok(())
    }
}

/// Bounded serialization avoids allocating a second unbounded payload merely
/// to measure a response. The backend remains responsible for its own buffers.
pub fn validate_json_size(value: &Value, limit: usize) -> anyhow::Result<()> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("method document byte bound"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(limit), value)?;
    Ok(())
}

/// A missing output declaration cannot establish a verified domain result.
pub fn validate_method_result(schema: Option<&Value>, result: &Value) -> anyhow::Result<()> {
    validate_json_size(result, MAX_METHOD_RECEIPT_BYTES)?;
    let schema = schema.ok_or_else(|| anyhow::anyhow!("missing_result_contract"))?;
    let validator = jsonschema::validator_for(schema)?;
    validator.validate(result).map_err(|error| {
        anyhow::anyhow!(
            "method result violates declaration at {}",
            error.instance_path
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt() -> MethodExecutionReceipt {
        MethodExecutionReceipt {
            success: true,
            operation_id: "00000000-0000-4000-8000-000000000001".into(),
            outcome: "completed".into(),
            event_id: 7,
            event_hash: "a".repeat(32),
            terminal_event_id: 11,
            terminal_event_hash: "b".repeat(32),
            plugin_id: "fixture".into(),
            method: "read".into(),
            result: json!({"value": 42}),
        }
    }

    #[test]
    fn accountability_requires_completed_attribution_and_ordered_proofs() {
        let valid = receipt();
        valid.validate_attribution("fixture", "read").unwrap();
        assert!(valid.validate_attribution("other", "read").is_err());
        assert!(valid.validate_attribution("fixture", "write").is_err());
        for changed in [
            MethodExecutionReceipt {
                success: false,
                ..valid.clone()
            },
            MethodExecutionReceipt {
                outcome: "unknown".into(),
                ..valid.clone()
            },
            MethodExecutionReceipt {
                terminal_event_id: 7,
                ..valid.clone()
            },
            MethodExecutionReceipt {
                event_hash: "invented".into(),
                ..valid.clone()
            },
            MethodExecutionReceipt {
                operation_id: "not-an-id".into(),
                ..valid
            },
        ] {
            assert!(changed.validate_attribution("fixture", "read").is_err());
        }
    }

    #[test]
    fn proof_grammar_accepts_the_actual_chain_producer() {
        let event = crate::ChainEvent::new(
            7,
            String::new(),
            "fixture-actor".into(),
            "fixture".into(),
            "1.0.0".into(),
            crate::OperationType::MethodCall,
            "fixture.read".into(),
            Vec::new(),
            crate::Decision::Allow,
            &simd_json::json!({}),
        );
        assert!(event.verify());
        assert!(valid_event_hash(&event.event_hash));
        let mut result = receipt();
        result.event_hash = event.event_hash.clone();
        result.terminal_event_hash = event.compute_hash();
        result.validate_attribution("fixture", "read").unwrap();
        // Blob and schema digests are different locators, never audit proofs.
        assert!(!valid_event_hash(&"a".repeat(64)));
        assert!(!valid_event_hash(&"A".repeat(32)));
    }

    #[test]
    fn missing_contract_is_not_inferred_and_declared_results_are_checked() {
        let schema = json!({"type":"object","required":["value"],"properties":{"value":{"type":"integer"}},"additionalProperties":false});
        validate_method_result(Some(&schema), &json!({"value":42})).unwrap();
        assert!(validate_method_result(Some(&schema), &json!({"value":"42"})).is_err());
        assert!(validate_method_result(None, &Value::Null).is_err());
        validate_method_result(Some(&json!({"type":"null"})), &Value::Null).unwrap();
        assert!(validate_json_size(&json!("bounded"), 2).is_err());
    }
}
