//! The Compliance Engine: "The Law Firm"
//! Responsible for validating the PluginSchema against legal and security frameworks.

use serde_json::Value;

/// Strongly-typed error for compliance validation operations.
///
/// Keeps `anyhow` out of the public API surface while preserving
/// ergonomic `?` propagation inside the crate.
#[derive(Debug, thiserror::Error)]
pub enum ComplianceError {
    #[error("validation failed: {0}")]
    Validation(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub mod attorneys {
    use super::ComplianceError;
    use serde_json::Value;

    /// Olivia Scal: Managing Partner / OSCAL (Open Security Controls Assessment Language)
    pub struct OliviaScal;
    impl OliviaScal {
        pub fn validate_controls(schema: &Value) -> std::result::Result<(), ComplianceError> {
            // Check for security control metadata
            if let Some(caps) = schema.get("capabilities") {
                if caps.get("requires_root").and_then(|v| v.as_bool()) == Some(true) {
                    tracing::warn!("Plugin requires root; OSCAL assessment recommended");
                }
            }
            Ok(())
        }
    }

    /// E.U.gene Risk: EU AI Act Counsel
    pub struct EugeneRisk;
    impl EugeneRisk {
        pub fn validate_ai_risk(schema: &Value) -> std::result::Result<(), ComplianceError> {
            // If it's an AI/ML plugin, check for transparency requirements
            if schema.get("plugin_type").and_then(|v| v.as_str()) == Some("custom") {
                if let Some(meta) = schema.get("schema") {
                    if meta.get("model_name").is_some()
                        && meta.get("training_data_source").is_none()
                    {
                        return Err(ComplianceError::Validation(
                            "EU AI Act violation: Training data source must be declared for models"
                                .into(),
                        ));
                    }
                }
            }
            Ok(())
        }
    }

    /// Penny Privacy: GDPR Engine
    pub struct PennyPrivacy;
    impl PennyPrivacy {
        pub fn validate_privacy(schema: &Value) -> std::result::Result<(), ComplianceError> {
            // Check for PII handling without retention policy
            if let Some(s) = schema.get("schema") {
                let schema_str = s.to_string().to_lowercase();
                if (schema_str.contains("email")
                    || schema_str.contains("user_id")
                    || schema_str.contains("phone"))
                    && !schema_str.contains("retention")
                {
                    return Err(ComplianceError::Validation(
                        "GDPR violation: PII fields detected without retention policy".into(),
                    ));
                }
            }
            Ok(())
        }
    }

    /// Reggie O.P.A.: Cloud Prosecutor
    pub struct ReggieOpa;
    impl ReggieOpa {
        pub fn validate_policy(schema: &Value) -> std::result::Result<(), ComplianceError> {
            // Basic policy check: Versioning must be present
            if schema.get("version").is_none() {
                return Err(ComplianceError::Validation(
                    "OPA Policy failure: Missing version field".into(),
                ));
            }
            Ok(())
        }
    }
}

pub struct LawFirm;

impl LawFirm {
    /// Review the canonical typed plugin schema. The generated JSON schema is
    /// only a view for the attorney checks; no repository schema file is read.
    pub fn review_plugin_schema(
        schema: &op_state_store::PluginSchema,
    ) -> std::result::Result<(), ComplianceError> {
        let mut v = serde_json::to_value(schema)?;
        v["plugin_type"] = serde_json::Value::String(schema.category.clone());
        v["schema"] = serde_json::to_value(schema.to_json_schema())?;
        Self::validate_structure(&v)?;
        attorneys::OliviaScal::validate_controls(&v)?;
        attorneys::EugeneRisk::validate_ai_risk(&v)?;
        attorneys::PennyPrivacy::validate_privacy(&v)?;
        attorneys::ReggieOpa::validate_policy(&v)?;
        Ok(())
    }

    pub fn review_schema(schema_json: &str) -> std::result::Result<(), ComplianceError> {
        let v: Value = serde_json::from_str(schema_json)?;
        Self::validate_legacy_structure(&v)?;

        // 2. Attorney Reviews
        attorneys::OliviaScal::validate_controls(&v)?;
        attorneys::EugeneRisk::validate_ai_risk(&v)?;
        attorneys::PennyPrivacy::validate_privacy(&v)?;
        attorneys::ReggieOpa::validate_policy(&v)?;

        Ok(())
    }

    fn validate_structure(schema: &Value) -> std::result::Result<(), ComplianceError> {
        let object = schema.as_object().ok_or_else(|| {
            ComplianceError::Validation("Plugin schema must be a JSON object".into())
        })?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            return Err(ComplianceError::Validation(
                "Plugin schema missing required field: name".into(),
            ));
        }
        let version = object
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let valid_version = version.split('.').count() == 3
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        if !valid_version {
            return Err(ComplianceError::Validation(format!(
                "Invalid plugin schema version: {version}"
            )));
        }
        Ok(())
    }

    /// Preserve the legacy JSON review contract while its source is migrated
    /// away from the deleted repository meta-schema file.
    fn validate_legacy_structure(schema: &Value) -> std::result::Result<(), ComplianceError> {
        Self::validate_structure(schema)?;
        let object = schema.as_object().expect("validated object");
        let name = object["name"].as_str().unwrap();
        if name.len() > 64
            || name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            return Err(ComplianceError::Validation(
                "Invalid plugin schema name".into(),
            ));
        }
        let plugin_type = object
            .get("plugin_type")
            .and_then(Value::as_str)
            .ok_or_else(|| ComplianceError::Validation("Missing plugin_type".into()))?;
        if !matches!(
            plugin_type,
            "service" | "network" | "storage" | "system" | "custom"
        ) {
            return Err(ComplianceError::Validation("Invalid plugin_type".into()));
        }
        if let Some(caps) = object.get("capabilities") {
            let caps = caps.as_object().ok_or_else(|| {
                ComplianceError::Validation("capabilities must be an object".into())
            })?;
            for (key, value) in caps {
                if matches!(
                    key.as_str(),
                    "can_read"
                        | "can_write"
                        | "can_delete"
                        | "supports_rollback"
                        | "supports_checkpoints"
                        | "requires_root"
                ) && !value.is_boolean()
                {
                    return Err(ComplianceError::Validation(
                        "Invalid capabilities field".into(),
                    ));
                }
            }
        }
        if let Some(dependencies) = object.get("dependencies") {
            let dependencies = dependencies.as_array().ok_or_else(|| {
                ComplianceError::Validation("dependencies must be an array".into())
            })?;
            for dependency in dependencies {
                let dependency = dependency.as_object().ok_or_else(|| {
                    ComplianceError::Validation("dependency must be an object".into())
                })?;
                if dependency.get("name").and_then(Value::as_str).is_none() {
                    return Err(ComplianceError::Validation(
                        "dependency missing name".into(),
                    ));
                }
                for key in ["version", "required"] {
                    if let Some(value) = dependency.get(key) {
                        let valid = (key == "version" && value.is_string())
                            || (key == "required" && value.is_boolean());
                        if !valid {
                            return Err(ComplianceError::Validation(
                                "Invalid dependency field".into(),
                            ));
                        }
                    }
                }
            }
        }
        if let Some(plugin_schema) = object.get("schema") {
            if !plugin_schema.is_object() {
                return Err(ComplianceError::Validation(
                    "schema must be an object".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_valid_schema_passes() {
        let schema = json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "plugin_type": "service",
            "capabilities": {
                "can_read": true
            }
        });
        assert!(LawFirm::review_schema(&schema.to_string()).is_ok());
    }

    #[test]
    fn test_invalid_version_fails() {
        let schema = json!({
            "name": "test-plugin",
            "version": "invalid",
            "plugin_type": "service"
        });
        assert!(LawFirm::review_schema(&schema.to_string()).is_err());
    }

    #[test]
    fn legacy_shape_rules_are_preserved_without_a_meta_schema_file() {
        for (field, value) in [
            ("name", json!("BadName")),
            ("plugin_type", json!("unknown")),
            ("capabilities", json!({"can_read": "yes"})),
            ("dependencies", json!(["not-an-object"])),
            ("schema", json!("not-an-object")),
        ] {
            let mut schema = json!({
                "name": "valid-plugin",
                "version": "1.0.0",
                "plugin_type": "service"
            });
            schema[field] = value;
            assert!(
                LawFirm::review_schema(&schema.to_string()).is_err(),
                "legacy field {field} should be rejected"
            );
        }
    }

    #[test]
    fn test_gdpr_violation() {
        let schema = json!({
            "name": "pii-plugin",
            "version": "1.1.0",
            "plugin_type": "custom",
            "schema": {
                "user_email": "string"
            }
        });
        let result = LawFirm::review_schema(&schema.to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("GDPR"));
    }

    #[test]
    fn test_ai_act_violation() {
        let schema = json!({
            "name": "ai-plugin",
            "version": "2.0.0",
            "plugin_type": "custom",
            "schema": {
                "model_name": "gpt-4"
            }
        });
        let result = LawFirm::review_schema(&schema.to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("AI Act"));
    }

    #[test]
    fn typed_plugin_schema_is_reviewed_without_meta_schema_file() {
        let schema = op_state_store::PluginSchema::builder("typed-plugin")
            .version("1.0.0")
            .description("A typed test plugin")
            .build();
        assert!(LawFirm::review_plugin_schema(&schema).is_ok());
    }
}
