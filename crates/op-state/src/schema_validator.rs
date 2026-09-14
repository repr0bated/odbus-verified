//! Schema Validator - Prevents random/unrealistic schema generation
//! Validates schemas against curated use cases and constraints

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::{HashMap, HashSet};

/// Validated use case template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UseCaseTemplate {
    /// Use case name
    pub name: String,
    /// Description
    pub description: String,
    /// Required plugins
    pub required_plugins: Vec<String>,
    /// Required fields per plugin
    pub required_fields: HashMap<String, Vec<String>>,
    /// Valid field combinations
    pub valid_combinations: Vec<FieldCombination>,
    /// Dependencies (plugin A requires plugin B)
    pub dependencies: Vec<Dependency>,
    /// Constraints (field A requires field B)
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldCombination {
    /// Plugin name
    pub plugin: String,
    /// Valid field combinations
    pub fields: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Plugin that requires another
    pub requires: String,
    /// Required plugin
    pub required: String,
    /// Optional: specific condition
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    /// Plugin name
    pub plugin: String,
    /// Field that has constraint
    pub field: String,
    /// Constraint type
    pub constraint_type: ConstraintType,
    /// Required value or field
    pub required: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    /// Field must equal value
    Equals,
    /// Field must be in list
    In,
    /// Field requires another field to be set
    RequiresField,
    /// Field value must match pattern
    Pattern,
    /// Field must be within range
    Range { min: f64, max: f64 },
}

/// Schema validator
pub struct SchemaValidator {
    /// Curated use case templates
    use_cases: Vec<UseCaseTemplate>,
    /// Plugin field definitions
    plugin_fields: HashMap<String, HashSet<String>>,
}

impl SchemaValidator {
    pub fn new() -> Self {
        Self {
            use_cases: Self::load_default_use_cases(),
            plugin_fields: Self::load_plugin_fields(),
        }
    }

    /// Validate a generated schema against use cases
    pub fn validate_schema(&self, schema: &Value) -> Result<ValidationResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Extract plugins from schema
        let plugins = schema
            .get("plugins")
            .and_then(|p| p.as_object())
            .ok_or_else(|| anyhow::anyhow!("Missing 'plugins' in schema"))?;

        // Check if schema matches any use case
        let matching_use_case = self.find_matching_use_case(plugins);

        if matching_use_case.is_none() {
            warnings.push("Schema does not match any curated use case template".to_string());
        }

        // Validate required plugins for matched use case
        if let Some(use_case) = &matching_use_case {
            for required_plugin in &use_case.required_plugins {
                if !plugins.contains_key(required_plugin) {
                    errors.push(format!(
                        "Use case '{}' requires plugin '{}'",
                        use_case.name, required_plugin
                    ));
                }
            }
        }

        // Validate dependencies
        for dep in self.get_all_dependencies() {
            if plugins.contains_key(&dep.requires) && !plugins.contains_key(&dep.required) {
                errors.push(format!(
                    "Plugin '{}' requires plugin '{}'",
                    dep.requires, dep.required
                ));
            }
        }

        // Validate field constraints
        for (plugin_name, plugin_config) in plugins {
            if let Some(fields) = self.plugin_fields.get(plugin_name) {
                // Check for unknown fields
                if let Some(config_obj) = plugin_config.as_object() {
                    for field_name in config_obj.keys() {
                        if !fields.contains(field_name) {
                            warnings.push(format!(
                                "Unknown field '{}' in plugin '{}'",
                                field_name, plugin_name
                            ));
                        }
                    }
                }
            }
        }

        // Validate field combinations
        if let Some(use_case) = &matching_use_case {
            for combo in &use_case.valid_combinations {
                if let Some(plugin_config) = plugins.get(&combo.plugin) {
                    if let Some(config_obj) = plugin_config.as_object() {
                        for (field, valid_values) in &combo.fields {
                            if let Some(field_value) = config_obj.get(field) {
                                let value_str = field_value.to_string();
                                if !valid_values.iter().any(|v| value_str.contains(v)) {
                                    warnings.push(format!(
                                        "Field '{}' in plugin '{}' has unusual value: {}",
                                        field, combo.plugin, value_str
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
            matched_use_case: matching_use_case.map(|uc| uc.name.clone()),
        })
    }

    /// Find matching use case for a schema
    fn find_matching_use_case(
        &self,
        plugins: &simd_json::value::owned::Object,
    ) -> Option<&UseCaseTemplate> {
        for use_case in &self.use_cases {
            let mut matches = 0;
            for required_plugin in &use_case.required_plugins {
                if plugins.contains_key(required_plugin) {
                    matches += 1;
                }
            }
            // Match if at least 80% of required plugins are present
            if matches as f64 / use_case.required_plugins.len() as f64 >= 0.8 {
                return Some(use_case);
            }
        }
        None
    }

    /// Get all dependencies from all use cases
    fn get_all_dependencies(&self) -> Vec<Dependency> {
        self.use_cases
            .iter()
            .flat_map(|uc| uc.dependencies.clone())
            .collect()
    }

    /// Load default curated use cases
    fn load_default_use_cases() -> Vec<UseCaseTemplate> {
        vec![
            // Basic Network Use Case
            UseCaseTemplate {
                name: "basic_network".to_string(),
                description: "Basic OVS bridge with DHCP".to_string(),
                required_plugins: vec!["net".to_string()],
                required_fields: {
                    let mut m = HashMap::new();
                    m.insert("net".to_string(), vec!["interfaces".to_string()]);
                    m
                },
                valid_combinations: vec![],
                dependencies: vec![],
                constraints: vec![],
            },
            // Container Mesh Use Case
            UseCaseTemplate {
                name: "container_mesh".to_string(),
                description: "Incus containers with Netmaker mesh networking".to_string(),
                required_plugins: vec![
                    "incus".to_string(),
                    "netmaker".to_string(),
                    "openflow".to_string(),
                ],
                required_fields: HashMap::new(),
                valid_combinations: vec![],
                dependencies: vec![Dependency {
                    requires: "netmaker".to_string(),
                    required: "net".to_string(),
                    condition: None,
                }],
                constraints: vec![],
            },
        ]
    }

    /// Load plugin field definitions
    fn load_plugin_fields() -> HashMap<String, HashSet<String>> {
        let mut fields = HashMap::new();

        // Net plugin fields
        fields.insert("net".to_string(), {
            let mut s = HashSet::new();
            s.insert("interfaces".to_string());
            s
        });

        // OpenFlow plugin fields
        fields.insert("openflow".to_string(), {
            let mut s = HashSet::new();
            s.insert("bridges".to_string());
            s.insert("controller_endpoint".to_string());
            s.insert("flow_policies".to_string());
            s.insert("auto_discover_containers".to_string());
            s.insert("enable_security_flows".to_string());
            s.insert("obfuscation_level".to_string());
            s
        });

        fields
    }

    /// Get curated use case templates
    pub fn get_use_cases(&self) -> &[UseCaseTemplate] {
        &self.use_cases
    }

    /// Suggest realistic schema based on use case
    pub fn suggest_schema(&self, use_case_name: &str) -> Option<Value> {
        self.use_cases
            .iter()
            .find(|uc| uc.name == use_case_name)
            .map(|uc| {
                let mut schema = simd_json::value::owned::Object::new();
                schema.insert("version".to_string(), json!(1));

                let mut plugins = simd_json::value::owned::Object::new();

                for plugin_name in &uc.required_plugins {
                    let mut plugin_config = simd_json::value::owned::Object::new();

                    // Add required fields with sensible defaults
                    if let Some(required_fields) = uc.required_fields.get(plugin_name) {
                        for field in required_fields {
                            let parts: Vec<&str> = field.split('.').collect();
                            if parts.len() == 1 {
                                plugin_config.insert(field.clone(), json!(""));
                            } else {
                                // Nested field
                                let mut nested = simd_json::value::owned::Object::new();
                                nested.insert(parts[1].to_string(), json!(""));
                                plugin_config.insert(parts[0].to_string(), json!(nested));
                            }
                        }
                    }

                    plugins.insert(plugin_name.clone(), json!(plugin_config));
                }

                schema.insert("plugins".to_string(), json!(plugins));
                json!(schema)
            })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub matched_use_case: Option<String>,
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}
