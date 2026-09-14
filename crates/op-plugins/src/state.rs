//! Desired state management and change tracking

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simd_json::OwnedValue as Value;

/// Desired state configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesiredState {
    /// The target state configuration
    pub state: Value,
    /// When this desired state was set
    pub timestamp: DateTime<Utc>,
    /// Hash of the state for verification
    pub hash: String,
    /// Optional description of the change
    pub description: Option<String>,
    /// Source of the desired state (user, auto, import, etc.)
    pub source: StateSource,
}

impl DesiredState {
    /// Create a new desired state
    pub fn new(state: Value) -> Self {
        let hash = Self::compute_hash(&state);
        Self {
            state,
            timestamp: Utc::now(),
            hash,
            description: None,
            source: StateSource::User,
        }
    }

    /// Create with description
    pub fn with_description(state: Value, description: impl Into<String>) -> Self {
        let mut ds = Self::new(state);
        ds.description = Some(description.into());
        ds
    }

    /// Create from imported configuration
    pub fn from_import(state: Value, source: &str) -> Self {
        let mut ds = Self::new(state);
        ds.source = StateSource::Import(source.to_string());
        ds
    }

    /// Compute hash of the state
    pub fn compute_hash(state: &Value) -> String {
        let mut hasher = Sha256::new();
        hasher.update(simd_json::to_string(state).unwrap_or_default().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Verify the hash matches
    pub fn verify(&self) -> bool {
        Self::compute_hash(&self.state) == self.hash
    }
}

impl Default for DesiredState {
    fn default() -> Self {
        Self::new(Value::Object(Box::new(
            simd_json::value::owned::Object::new(),
        )))
    }
}

/// Source of the desired state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StateSource {
    /// Set by user
    User,
    /// Auto-discovered from system
    AutoDiscovered,
    /// Imported from file or URL
    Import(String),
    /// From another plugin
    Plugin(String),
    /// System default
    Default,
}

/// Represents a change to be applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    /// Type of change operation
    pub operation: ChangeOperation,
    /// Path to the changed element (JSONPath-like)
    pub path: String,
    /// Previous value (if any)
    pub old_value: Option<Value>,
    /// New value (if any)
    pub new_value: Option<Value>,
    /// Human-readable description
    pub description: String,
    /// Hash of this change for snowball
    pub hash: String,
    /// Timestamp of the change
    pub timestamp: DateTime<Utc>,
}

impl StateChange {
    /// Create a new state change
    pub fn new(
        operation: ChangeOperation,
        path: impl Into<String>,
        old_value: Option<Value>,
        new_value: Option<Value>,
        description: impl Into<String>,
    ) -> Self {
        let path = path.into();
        let description = description.into();

        // Compute hash
        let mut hasher = Sha256::new();
        hasher.update(format!("{:?}", operation).as_bytes());
        hasher.update(path.as_bytes());
        hasher.update(
            simd_json::to_string(&old_value)
                .unwrap_or_default()
                .as_bytes(),
        );
        hasher.update(
            simd_json::to_string(&new_value)
                .unwrap_or_default()
                .as_bytes(),
        );
        let hash = format!("{:x}", hasher.finalize());

        Self {
            operation,
            path,
            old_value,
            new_value,
            description,
            hash,
            timestamp: Utc::now(),
        }
    }

    /// Create a create operation
    pub fn create(path: impl Into<String>, value: Value, description: impl Into<String>) -> Self {
        Self::new(
            ChangeOperation::Create,
            path,
            None,
            Some(value),
            description,
        )
    }

    /// Create an update operation
    pub fn update(
        path: impl Into<String>,
        old: Value,
        new: Value,
        description: impl Into<String>,
    ) -> Self {
        Self::new(
            ChangeOperation::Update,
            path,
            Some(old),
            Some(new),
            description,
        )
    }

    /// Create a delete operation
    pub fn delete(path: impl Into<String>, old: Value, description: impl Into<String>) -> Self {
        Self::new(ChangeOperation::Delete, path, Some(old), None, description)
    }

    /// Create a no-op (for audit logging)
    pub fn noop(path: impl Into<String>, value: Value, description: impl Into<String>) -> Self {
        Self::new(
            ChangeOperation::NoOp,
            path,
            Some(value.clone()),
            Some(value),
            description,
        )
    }
}

/// Change operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOperation {
    Create,
    Update,
    Delete,
    NoOp,
}

/// Validation result from a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
}

impl ValidationResult {
    /// Create a success result
    pub fn success() -> Self {
        Self {
            valid: true,
            errors: vec![],
            warnings: vec![],
            suggestions: vec![],
        }
    }

    /// Create a failure result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            valid: false,
            errors: vec![ValidationError {
                path: "".to_string(),
                message: error.into(),
                code: "validation_failed".to_string(),
            }],
            warnings: vec![],
            suggestions: vec![],
        }
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add an error (makes result invalid)
    pub fn with_error(mut self, path: impl Into<String>, message: impl Into<String>) -> Self {
        self.valid = false;
        self.errors.push(ValidationError {
            path: path.into(),
            message: message.into(),
            code: "validation_error".to_string(),
        });
        self
    }
}

/// Validation error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desired_state_hash() {
        let state = simd_json::json!({"key": "value"});
        let ds = DesiredState::new(state);
        assert!(ds.verify());
    }

    #[test]
    fn test_state_change_hash() {
        let change =
            StateChange::create("/test/path", simd_json::json!({"value": 42}), "Test change");
        assert!(!change.hash.is_empty());
    }
}
