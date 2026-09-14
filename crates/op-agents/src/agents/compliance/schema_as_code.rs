//! Schema-as-Code Agent
//!
//! Designs and validates machine-readable schema contracts: JSON Schema, OpenAPI,
//! AsyncAPI, Protocol Buffers, OSCAL extensions, OpenControl YAML.

use crate::agents::advise::{extract_query, is_advise_op, route_advise_to_op};
use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};

pub struct SchemaAsCodeAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SchemaAsCodeAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("schema-as-code"),
            agent_id,
        }
    }

    fn parse_json_args(args: &str) -> Option<Value> {
        let trimmed = args.trim();
        if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
            return None;
        }
        let mut bytes = trimmed.as_bytes().to_vec();
        simd_json::to_owned_value(&mut bytes).ok()
    }

    /// Validate schema against instance when both are present; otherwise lint the
    /// supplied document. Always includes the caller input — never a static card.
    fn validate_payload(args: Option<&str>) -> Result<Value, String> {
        let raw = args.unwrap_or("").trim();
        if raw.is_empty() {
            return Err(
                "validate requires args: JSON `{\"schema\":...,\"instance\":...}` \
                 or a schema document / path description"
                    .to_string(),
            );
        }

        if let Some(v) = Self::parse_json_args(raw) {
            if let Some(obj) = v.as_object() {
                let schema_v = obj
                    .get("schema")
                    .or_else(|| obj.get("json_schema"))
                    .cloned();
                let instance_v = obj
                    .get("instance")
                    .or_else(|| obj.get("data"))
                    .or_else(|| obj.get("document"))
                    .cloned();

                if let (Some(schema_v), Some(instance_v)) = (schema_v.clone(), instance_v) {
                    return Self::run_jsonschema(&schema_v, &instance_v, raw);
                }

                // Treat the whole object as a schema document to lint.
                if obj.contains_key("type")
                    || obj.contains_key("$schema")
                    || obj.contains_key("properties")
                    || obj.contains_key("openapi")
                {
                    return Ok(Self::lint_schema_document(&v, raw));
                }
            }
        }

        // Free-form / non-JSON: produce input-specific guidance (not a cookbook card).
        Ok(Self::validate_prose(raw))
    }

    fn run_jsonschema(
        schema_v: &Value,
        instance_v: &Value,
        raw_input: &str,
    ) -> Result<Value, String> {
        let schema_sj: serde_json::Value =
            serde_json::from_str(&simd_json::to_string(schema_v).map_err(|e| e.to_string())?)
                .map_err(|e| format!("schema not JSON-compatible: {e}"))?;
        let instance_sj: serde_json::Value =
            serde_json::from_str(&simd_json::to_string(instance_v).map_err(|e| e.to_string())?)
                .map_err(|e| format!("instance not JSON-compatible: {e}"))?;

        let validator = jsonschema::validator_for(&schema_sj)
            .map_err(|e| format!("schema compilation failed: {e}"))?;

        let errors: Vec<Value> = validator
            .iter_errors(&instance_sj)
            .map(|e| {
                json!({
                    "path": e.instance_path.to_string(),
                    "message": e.to_string(),
                })
            })
            .collect();

        let valid = errors.is_empty();
        Ok(json!({
            "operation": "validate",
            "mode": "jsonschema",
            "valid": valid,
            "error_count": errors.len(),
            "errors": errors,
            "input": raw_input,
            "schema_summary": Self::schema_summary(&schema_sj),
        }))
    }

    fn schema_summary(schema: &serde_json::Value) -> Value {
        json!({
            "type": schema.get("type").cloned().unwrap_or(serde_json::Value::Null),
            "has_properties": schema.get("properties").is_some(),
            "required": schema.get("required").cloned().unwrap_or(serde_json::Value::Null),
            "$schema": schema.get("$schema").cloned().unwrap_or(serde_json::Value::Null),
        })
    }

    fn lint_schema_document(doc: &Value, raw_input: &str) -> Value {
        let mut findings: Vec<Value> = Vec::new();
        let obj = doc.as_object();

        if let Some(o) = obj {
            if o.get("$schema").and_then(|v| v.as_str()).is_none() && o.get("openapi").is_none() {
                findings.push(json!({
                    "severity": "warning",
                    "message": "Missing $schema (JSON Schema) or openapi version field"
                }));
            }
            if o.get("type").is_none()
                && o.get("properties").is_none()
                && o.get("openapi").is_none()
                && o.get("$ref").is_none()
            {
                findings.push(json!({
                    "severity": "error",
                    "message": "Document has no type/properties/$ref/openapi — not a recognizable schema"
                }));
            }
            if let Some(props) = o.get("properties").and_then(|p| p.as_object()) {
                if props.is_empty() {
                    findings.push(json!({
                        "severity": "warning",
                        "message": "properties object is empty"
                    }));
                }
                for (name, prop) in props.iter() {
                    if prop
                        .as_object()
                        .map(|p| p.get("type").is_none())
                        .unwrap_or(true)
                        && prop.get("$ref").is_none()
                    {
                        findings.push(json!({
                            "severity": "warning",
                            "message": format!("property '{name}' lacks type or $ref")
                        }));
                    }
                }
            }
            if let Some(required) = o.get("required").and_then(|r| r.as_array()) {
                let props = o
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .map(|p| p.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                for req in required {
                    if let Some(name) = req.as_str() {
                        if !props.iter().any(|p| p == name) {
                            findings.push(json!({
                                "severity": "error",
                                "message": format!(
                                    "required field '{name}' is not declared in properties"
                                )
                            }));
                        }
                    }
                }
            }
            if o.get("openapi").is_some() {
                if o.get("paths").is_none() {
                    findings.push(json!({
                        "severity": "error",
                        "message": "OpenAPI document missing paths"
                    }));
                }
                if o.get("info").is_none() {
                    findings.push(json!({
                        "severity": "error",
                        "message": "OpenAPI document missing info"
                    }));
                }
            }
        } else {
            findings.push(json!({
                "severity": "error",
                "message": "Top-level schema must be a JSON object"
            }));
        }

        let error_count = findings
            .iter()
            .filter(|f| f.get("severity").and_then(|s| s.as_str()) == Some("error"))
            .count();

        json!({
            "operation": "validate",
            "mode": "schema_lint",
            "valid": error_count == 0,
            "finding_count": findings.len(),
            "findings": findings,
            "input": raw_input,
        })
    }

    fn validate_prose(raw: &str) -> Value {
        let lower = raw.to_lowercase();
        let detected =
            if lower.contains("openapi") || lower.ends_with(".yaml") || lower.ends_with(".yml") {
                "openapi"
            } else if lower.contains("proto") || lower.ends_with(".proto") {
                "protobuf"
            } else if lower.contains("oscal") {
                "oscal"
            } else if lower.contains("asyncapi") {
                "asyncapi"
            } else {
                "json_schema"
            };

        let mut findings = vec![json!({
            "severity": "info",
            "message": format!(
                "No JSON schema+instance payload detected; treating input as {detected} validation request"
            )
        })];

        // Quote distinctive tokens so two different prose inputs cannot collide.
        let tokens: Vec<String> = raw
            .split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-')
            .filter(|t| t.len() >= 4)
            .take(12)
            .map(|t| t.to_string())
            .collect();

        for t in &tokens {
            if t.contains('.') {
                findings.push(json!({
                    "severity": "info",
                    "message": format!("Referenced artifact: {t}")
                }));
            }
        }

        findings.push(json!({
            "severity": "warning",
            "message": format!(
                "Pass JSON {{\"schema\":{{...}},\"instance\":{{...}}}} for executable validation of: {}",
                if raw.len() > 120 { format!("{}…", &raw[..120]) } else { raw.to_string() }
            )
        }));

        json!({
            "operation": "validate",
            "mode": "prose_request",
            "detected_format": detected,
            "valid": false,
            "executable": false,
            "findings": findings,
            "input": raw,
            "input_tokens": tokens,
        })
    }

    fn analyze(&self, op: &str, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let result = match op {
            "design_schema" => {
                let format = if input.contains("openapi") || input.contains("rest") {
                    "OpenAPI 3.1"
                } else if input.contains("proto") || input.contains("grpc") {
                    "Protocol Buffers 3"
                } else if input.contains("asyncapi")
                    || input.contains("event")
                    || input.contains("kafka")
                {
                    "AsyncAPI 3.0"
                } else if input.contains("oscal") {
                    "OSCAL JSON Schema"
                } else {
                    "JSON Schema Draft 2020-12"
                };
                json!({
                    "operation": "design_schema",
                    "recommended_format": format,
                    "best_practices": [
                        "Use $id and $schema for self-describing documents",
                        "Prefer explicit required over nullable for mandatory fields",
                        "Use $defs for reusable types",
                        "Version your schema with semver in the URI",
                        "Provide examples for every complex type"
                    ],
                    "compliance_extensions": {
                        "oscal": "x-oscal-control-id for mapping fields to controls",
                        "gdpr": "x-pii: true to flag personal data fields",
                        "fedramp": "x-fedramp-impact for sensitivity classification"
                    },
                    "input": args.unwrap_or("")
                })
            }
            "validate" => Self::validate_payload(args)?,
            "opencontrol" => json!({
                "operation": "opencontrol",
                "description": "OpenControl YAML schema for compliance-as-code (alternative to OSCAL)",
                "schema_elements": {
                    "component": "system component declaration",
                    "satisfies": "list of control satisfed with narrative",
                    "documentation_complete": "boolean flag per control"
                },
                "example": {
                    "schema_version": "3.1.0",
                    "name": "My Component",
                    "key": "MC",
                    "satisfies": [{
                        "standard_key": "NIST-800-53",
                        "control_key": "AC-1",
                        "narrative": [{"text": "The system implements..."}]
                    }]
                },
                "tools": ["compliance-masonry", "opencontrol/schemas", "fedramp-gocomply"],
                "migration_to_oscal": "compliance-io converts OpenControl → OSCAL component-definition",
                "input": args.unwrap_or("")
            }),
            "generate_schema" => json!({
                "operation": "generate_schema",
                "from_rust": "schemars crate: #[derive(JsonSchema)] generates JSON Schema",
                "from_typescript": "zod.toJsonSchema() or ts-json-schema-generator",
                "from_proto": "buf generate --template buf.gen.yaml (openapi/jsonschema plugins)",
                "from_examples": "quicktype infers schema from JSON examples",
                "input": args.unwrap_or("")
            }),
            op if is_advise_op(op) => {
                let query = extract_query(args);
                let routed_op = route_advise_to_op(
                    &query,
                    &[
                        ("oscal", "design_schema"),
                        ("openapi", "design_schema"),
                        ("proto", "design_schema"),
                        ("opencontrol", "opencontrol"),
                        ("valid", "validate"),
                        ("generat", "generate_schema"),
                        ("schemars", "generate_schema"),
                        ("rust", "generate_schema"),
                    ],
                    "design_schema",
                );
                return self.analyze(&routed_op, args);
            }
            other => {
                return Err(format!(
                    "unsupported operation '{}'; supported: [design_schema, validate, opencontrol, generate_schema, advise]",
                    other
                ));
            }
        };
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for SchemaAsCodeAgent {
    fn agent_type(&self) -> &str {
        "schema-as-code"
    }
    fn name(&self) -> &str {
        "Schema-as-Code Architect"
    }
    fn description(&self) -> &str {
        "Schema-as-code design expert: JSON Schema, OpenAPI, AsyncAPI, Protobuf, OSCAL extensions, OpenControl YAML. Designs compliance-aware schemas with PII/control annotations, validates with linters, detects breaking changes."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "design_schema".to_string(),
            "validate".to_string(),
            "opencontrol".to_string(),
            "generate_schema".to_string(),
            "advise".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Schema-as-Code agent {} is running", self.agent_id)
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "schema-as-code" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(&task.operation, task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty_args_fails() {
        let err = SchemaAsCodeAgent::validate_payload(None).unwrap_err();
        assert!(err.contains("requires args"), "{err}");
    }

    #[test]
    fn validate_rejects_invalid_instance() {
        let args = r#"{"schema":{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]},"instance":{"n":"nope"}}"#;
        let v = SchemaAsCodeAgent::validate_payload(Some(args)).unwrap();
        assert_eq!(v.get("valid").and_then(|x| x.as_bool()), Some(false));
        assert_eq!(v.get("mode").and_then(|x| x.as_str()), Some("jsonschema"));
        assert!(v.get("error_count").and_then(|x| x.as_u64()).unwrap_or(0) >= 1);
        assert!(v
            .get("input")
            .and_then(|x| x.as_str())
            .unwrap()
            .contains("nope"));
    }

    #[test]
    fn validate_accepts_valid_instance() {
        let args = r#"{"schema":{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]},"instance":{"n":3}}"#;
        let v = SchemaAsCodeAgent::validate_payload(Some(args)).unwrap();
        assert_eq!(v.get("valid").and_then(|x| x.as_bool()), Some(true));
        assert_eq!(v.get("error_count").and_then(|x| x.as_u64()), Some(0));
    }

    #[test]
    fn validate_prose_inputs_differ() {
        let a = SchemaAsCodeAgent::validate_payload(Some("validate THIS_UNIQUE_AAA_schema.json"))
            .unwrap();
        let b = SchemaAsCodeAgent::validate_payload(Some(
            "validate THAT_UNIQUE_BBB.yaml completely different",
        ))
        .unwrap();
        assert_ne!(
            simd_json::to_string(&a).unwrap(),
            simd_json::to_string(&b).unwrap()
        );
        assert!(a
            .get("input")
            .and_then(|x| x.as_str())
            .unwrap()
            .contains("AAA"));
        assert!(b
            .get("input")
            .and_then(|x| x.as_str())
            .unwrap()
            .contains("BBB"));
    }

    #[test]
    fn validate_lints_schema_document() {
        let args = r#"{"type":"object","properties":{"id":{}},"required":["missing"]}"#;
        let v = SchemaAsCodeAgent::validate_payload(Some(args)).unwrap();
        assert_eq!(v.get("mode").and_then(|x| x.as_str()), Some("schema_lint"));
        assert_eq!(v.get("valid").and_then(|x| x.as_bool()), Some(false));
    }
}
