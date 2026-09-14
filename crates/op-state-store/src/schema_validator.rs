//! Schema Validation Module
//!
//! Provides validation of:
//! - Generated schemas against JSON Schema meta-schemas
//! - Instance data against plugin schemas
//! - Expansion of propertyDependencies to if/then for broader compatibility

use crate::plugin_schema::{PluginSchema, SchemaCatalog, DEFAULT_SCHEMA_DIALECT};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;

/// Validation report with detailed error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Whether validation passed
    pub valid: bool,
    /// List of validation errors
    pub errors: Vec<ValidationError>,
    /// List of validation warnings (non-fatal)
    pub warnings: Vec<String>,
    /// Schema dialect used
    pub dialect: String,
    /// Hash of the validated content (for audit trail)
    pub content_hash: Option<String>,
}

impl ValidationReport {
    /// Create a successful validation report
    pub fn success(dialect: &str) -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            dialect: dialect.to_string(),
            content_hash: None,
        }
    }

    /// Create a failed validation report with errors
    pub fn failure(dialect: &str, errors: Vec<ValidationError>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
            dialect: dialect.to_string(),
            content_hash: None,
        }
    }

    /// Add a content hash for audit trail
    pub fn with_content_hash(mut self, hash: String) -> Self {
        self.content_hash = Some(hash);
        self
    }
}

/// A single validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// JSON Pointer path to the error location
    pub path: String,
    /// Error message
    pub message: String,
    /// The keyword that caused the error (e.g., "type", "required")
    pub keyword: Option<String>,
    /// The schema path where the error originated
    pub schema_path: Option<String>,
}

impl ValidationError {
    pub fn new(path: &str, message: &str) -> Self {
        Self {
            path: path.to_string(),
            message: message.to_string(),
            keyword: None,
            schema_path: None,
        }
    }

    pub fn with_keyword(mut self, keyword: &str) -> Self {
        self.keyword = Some(keyword.to_string());
        self
    }
}

/// Schema validator that uses the jsonschema crate
pub struct SchemaValidator {
    /// Cached compiled validators
    validators: HashMap<String, jsonschema::Validator>,
}

impl SchemaValidator {
    /// Create a new schema validator
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
        }
    }

    /// Validate a generated schema against the meta-schema
    pub fn validate_schema_against_meta(
        &mut self,
        schema: &Value,
        schema_catalog: &SchemaCatalog,
    ) -> Result<ValidationReport, ValidatorError> {
        let dialect = schema
            .get("$schema")
            .and_then(|s| s.as_str())
            .unwrap_or(DEFAULT_SCHEMA_DIALECT);

        // Get or load the meta-schema
        let meta_schema = schema_catalog
            .get_meta_schema(dialect)
            .ok_or_else(|| ValidatorError::MetaSchemaNotLoaded(dialect.to_string()))?;

        // Compile the meta-schema validator if not cached
        let validator = self.get_or_compile_validator(dialect, meta_schema)?;

        // Convert simd_json::Value to serde_json::Value for jsonschema
        let serde_schema: serde_json::Value = serde_json::to_value(schema)
            .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;

        // Validate
        match validator.validate(&serde_schema) {
            Ok(_) => Ok(ValidationReport::success(dialect)),
            Err(error) => {
                // Single error returned, but we can get all errors via iter_errors
                let validation_errors: Vec<ValidationError> = validator
                    .iter_errors(&serde_schema)
                    .map(|e| ValidationError::new(&e.instance_path.to_string(), &e.to_string()))
                    .collect();
                if validation_errors.is_empty() {
                    // Fallback if iter_errors returns empty but validate failed
                    Ok(ValidationReport::failure(
                        dialect,
                        vec![ValidationError::new("", &error.to_string())],
                    ))
                } else {
                    Ok(ValidationReport::failure(dialect, validation_errors))
                }
            }
        }
    }

    /// Validate instance data against a plugin schema
    pub fn validate_instance(
        &mut self,
        schema: &PluginSchema,
        instance: &Value,
    ) -> Result<ValidationReport, ValidatorError> {
        let json_schema = schema.to_json_schema();
        let dialect = &schema.dialect;

        // Convert to serde_json for jsonschema
        let serde_json_schema: serde_json::Value = serde_json::to_value(&json_schema)
            .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;
        let serde_instance: serde_json::Value = serde_json::to_value(instance)
            .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;

        // Compile the schema validator
        let validator = jsonschema::validator_for(&serde_json_schema)
            .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;

        // Validate
        match validator.validate(&serde_instance) {
            Ok(_) => {
                let mut report = ValidationReport::success(dialect);
                // Add content hash for audit trail
                report.content_hash = Some(compute_content_hash(instance));
                Ok(report)
            }
            Err(error) => {
                // Get all errors via iter_errors
                let validation_errors: Vec<ValidationError> = validator
                    .iter_errors(&serde_instance)
                    .map(|e| ValidationError::new(&e.instance_path.to_string(), &e.to_string()))
                    .collect();
                if validation_errors.is_empty() {
                    Ok(ValidationReport::failure(
                        dialect,
                        vec![ValidationError::new("", &error.to_string())],
                    ))
                } else {
                    Ok(ValidationReport::failure(dialect, validation_errors))
                }
            }
        }
    }

    /// Expand propertyDependencies to if/then for validators that don't support it natively
    ///
    /// Transforms:
    /// ```json
    /// {
    ///   "propertyDependencies": {
    ///     "status": {
    ///       "locked": { "properties": { "id": { "readOnly": true } } }
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// To:
    /// ```json
    /// {
    ///   "allOf": [
    ///     {
    ///       "if": { "properties": { "status": { "const": "locked" } } },
    ///       "then": { "properties": { "id": { "readOnly": true } } }
    ///     }
    ///   ]
    /// }
    /// ```
    pub fn expand_property_dependencies(schema: &Value) -> Result<Value, ValidatorError> {
        let mut result = schema.clone();

        if let Some(obj) = result.as_object_mut() {
            if let Some(prop_deps) = obj.remove("propertyDependencies") {
                let mut all_of = obj
                    .get("allOf")
                    .and_then(|a| a.as_array())
                    .cloned()
                    .unwrap_or_default();

                if let Some(deps_obj) = prop_deps.as_object() {
                    for (prop_name, value_map) in deps_obj {
                        if let Some(values) = value_map.as_object() {
                            for (value, then_schema) in values {
                                let if_schema = json!({
                                    "properties": {
                                        prop_name: { "const": value }
                                    },
                                    "required": [prop_name]
                                });

                                all_of.push(json!({
                                    "if": if_schema,
                                    "then": then_schema
                                }));
                            }
                        }
                    }
                }

                if !all_of.is_empty() {
                    obj.insert("allOf".to_string(), json!(all_of));
                }
            }
        }

        // Recursively expand nested schemas
        if let Some(obj) = result.as_object_mut() {
            for (_key, value) in obj.iter_mut() {
                if value.is_object() {
                    *value = Self::expand_property_dependencies(value)?;
                } else if let Some(arr) = value.as_array_mut() {
                    for item in arr.iter_mut() {
                        if item.is_object() {
                            *item = Self::expand_property_dependencies(item)?;
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    fn get_or_compile_validator(
        &mut self,
        key: &str,
        schema: &Value,
    ) -> Result<&jsonschema::Validator, ValidatorError> {
        if !self.validators.contains_key(key) {
            let serde_schema: serde_json::Value = serde_json::to_value(schema)
                .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;
            let validator = jsonschema::validator_for(&serde_schema)
                .map_err(|e| ValidatorError::CompilationError(e.to_string()))?;
            self.validators.insert(key.to_string(), validator);
        }
        self.validators.get(key).ok_or_else(|| {
            ValidatorError::CompilationError(format!(
                "validator for {} not found after compilation",
                key
            ))
        })
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during validation
#[derive(Debug, Clone)]
pub enum ValidatorError {
    MetaSchemaNotLoaded(String),
    CompilationError(String),
    InvalidSchema(String),
}

impl std::fmt::Display for ValidatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MetaSchemaNotLoaded(d) => write!(f, "Meta-schema not loaded for dialect: {}", d),
            Self::CompilationError(e) => write!(f, "Schema compilation error: {}", e),
            Self::InvalidSchema(e) => write!(f, "Invalid schema: {}", e),
        }
    }
}

impl std::error::Error for ValidatorError {}

/// Compute a hash of the content for audit trail
fn compute_content_hash(value: &Value) -> String {
    // Canonicalize the JSON for consistent hashing
    let canonical = canonicalize_json(value);
    let canonical_str = simd_json::to_string(&canonical).unwrap_or_default();
    format!("{:x}", md5::compute(canonical_str.as_bytes()))
}

/// Canonicalize JSON for consistent hashing
/// - Sort object keys
/// - Normalize numbers
/// - Remove optional whitespace variance
pub fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            // Sort keys for consistent ordering
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));

            let canonical_map: simd_json::value::owned::Object = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), canonicalize_json(v)))
                .collect();

            Value::Object(Box::new(canonical_map))
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        Value::Static(s) => {
            if let Some(f) = s.as_f64() {
                if f.fract() == 0.0 && f.abs() < i64::MAX as f64 {
                    // It's an integer
                    json!(f as i64)
                } else {
                    json!(f)
                }
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize_json() {
        let input = json!({
            "z": 1,
            "a": 2,
            "m": [3, 2, 1]
        });

        let canonical = canonicalize_json(&input);
        let output = simd_json::to_string(&canonical).unwrap();

        // Keys should be sorted
        assert!(output.find("\"a\"").unwrap() < output.find("\"m\"").unwrap());
        assert!(output.find("\"m\"").unwrap() < output.find("\"z\"").unwrap());
    }

    #[test]
    fn test_expand_property_dependencies() {
        let schema = json!({
            "type": "object",
            "propertyDependencies": {
                "status": {
                    "locked": {
                        "properties": {
                            "id": { "readOnly": true }
                        }
                    }
                }
            }
        });

        let expanded = SchemaValidator::expand_property_dependencies(&schema).unwrap();

        // Should have allOf with if/then
        assert!(expanded.get("allOf").is_some());
        assert!(expanded.get("propertyDependencies").is_none());

        let all_of = expanded.get("allOf").unwrap().as_array().unwrap();
        assert_eq!(all_of.len(), 1);

        let first = &all_of[0];
        assert!(first.get("if").is_some());
        assert!(first.get("then").is_some());
    }

    #[test]
    fn test_content_hash_consistency() {
        let value1 = json!({"b": 2, "a": 1});
        let value2 = json!({"a": 1, "b": 2});

        // Same content, different order should produce same hash
        let hash1 = compute_content_hash(&value1);
        let hash2 = compute_content_hash(&value2);

        assert_eq!(hash1, hash2);
    }
}
