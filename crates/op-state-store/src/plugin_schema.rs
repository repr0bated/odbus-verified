//! Plugin schema catalog and derived-document helpers.
//!
//! Architectural rule:
//! - plugin code is the source of schema truth
//! - that schema is also the footprint and JSON render contract
//! - this module stores indexed copies of plugin-provided schemas for
//!   validation/export; it does not define or discover plugin schemas

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::public_state::PublicStatePolicy;

/// Default JSON Schema dialect (can be overridden per-schema)
pub const DEFAULT_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Known dialect identifiers
pub mod dialects {
    pub const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";
    pub const DRAFT_2019_09: &str = "https://json-schema.org/draft/2019-09/schema";
    pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";
}

/// Path to the json-schema-spec repository relative to workspace root
const SCHEMA_SPEC_PATH: &str = "json-schema-spec";

/// Schema field type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Array(Box<FieldType>),
    Object(HashMap<String, FieldSchema>),
    /// Dynamic string keys with one declared value type. Object retains its
    /// existing named-field representation; map keys are never public implicitly.
    Map(Box<FieldType>),
    /// Explicit, self-contained exact JSON Schema retained from the plugin.
    /// Not a generic fallback; shared public projection denies this boundary.
    JsonSchema(Box<Value>),
    Enum(Vec<String>),
    /// Discriminated union (tagged enum): the value must match exactly one of
    /// the inner branch types. Serializes as `{"one_of": [<FieldType>, ...]}`
    /// and renders to JSON Schema `{"oneOf": [...]}`. Distinct from
    /// `Constraint::OneOf` (a value-membership constraint).
    OneOf(Vec<FieldType>),
    Any,
}

/// Schema for a single field
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldSchema {
    /// Field type
    pub field_type: FieldType,
    /// Whether the field is required
    #[serde(default)]
    pub required: bool,
    /// Description of the field
    #[serde(default)]
    pub description: String,
    /// Default value if not provided
    #[serde(default)]
    pub default: Option<Value>,
    /// Example value for documentation
    #[serde(default)]
    pub example: Option<Value>,
    /// Validation constraints
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Unconditional readOnly - field cannot be modified
    #[serde(default)]
    pub read_only: bool,
    /// Conditional readOnly via propertyDependencies
    #[serde(default)]
    pub read_only_when: Option<ReadOnlyCondition>,
}

/// Condition for conditional readOnly (via propertyDependencies)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadOnlyCondition {
    /// The property to check (e.g., "status", "running")
    pub property: String,
    /// The value that triggers readOnly (e.g., "locked", "true")
    pub value: String,
}

/// Validation constraint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    /// Minimum value (for numbers) or length (for strings/arrays)
    Min { value: f64 },
    /// Maximum value (for numbers) or length (for strings/arrays)
    Max { value: f64 },
    /// Regex pattern (for strings)
    Pattern { regex: String },
    /// Value must be one of these
    OneOf { values: Vec<Value> },
    /// Reference to another field that must exist
    RequiresField { field: String },
    /// Custom validation function name
    Custom { validator: String },
}

/// Side-effect classification for a method declaration.
///
/// `Read` methods do not mutate plugin state; `Mutation` methods do.
/// The serde representation is `snake_case` so serialized output is
/// `"read"` or `"mutation"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
}

/// Declares a single callable method on a plugin's capability surface.
///
/// Every `MethodDecl` is exposed as a D-Bus method (and gRPC route) by the
/// bridge.  The `args` field is a JSON Schema object that the bridge uses to
/// validate caller arguments before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MethodDecl {
    /// Method name as it appears on the D-Bus interface / gRPC route.
    pub name: String,
    /// JSON Schema describing the method's input arguments.
    pub args: Value,
    /// Optional JSON Schema describing the method's return value.
    #[serde(default)]
    pub returns: Option<Value>,
    /// Whether the method reads or mutates state.
    pub side_effect: SideEffect,
    /// Whether repeated calls with the same args produce the same effect.
    pub idempotent: bool,
    /// If non-null, the caller's footprint must grant this capability.
    #[serde(default)]
    pub required_capability: Option<String>,
    /// OSCAL subid for this method (e.g. `mut.network.wireguard.set-key@v1`).
    pub subid: String,
}

/// Declares a signal emitted by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDecl {
    /// Signal name as it appears on the D-Bus interface.
    pub name: String,
    /// Optional JSON Schema describing the signal payload.
    #[serde(default)]
    pub payload: Option<Value>,
    /// OSCAL subid for this signal (category must be `evt`).
    pub subid: String,
}

/// A capability declared by a plugin.
///
/// The plugin schema is the single declaration point for capabilities:
/// every `MethodDecl::required_capability` must resolve to an entry in
/// `PluginSchema::capabilities` (see `validate_capability_closure`). The
/// gRPC interceptor and generated client indexes are *readers* of this
/// declaration, never independent sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDecl {
    /// Capability id, canonical OSCAL form
    /// (e.g. `cap.software.zeroclaw.gateway-config.read@v1`).
    pub id: String,
    /// What this capability authorizes.
    pub description: String,
}

/// True when `id` follows the canonical OSCAL capability form
/// (`cap.<domain>.<subject>.<op>@vN`). Anything else is a legacy
/// short-form id (`agent.read`, `factory.invoke`, …) tolerated until the
/// capability refactor completes — see `validate_capability_closure`.
pub fn is_canonical_capability_id(id: &str) -> bool {
    id.starts_with("cap.")
        && id.rsplit('@').next().is_some_and(|v| {
            v.len() > 1 && v.starts_with('v') && v[1..].chars().all(|c| c.is_ascii_digit())
        })
}

/// Where a method's effective capability was found — the three migration
/// lookup places, in resolution order. See
/// `PluginSchema::resolve_method_capability`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityResolution {
    /// Declared on the plugin and referenced by the method (end state).
    Declared(String),
    /// On the method only, canonical form but undeclared — must be fixed.
    Undeclared(String),
    /// On the method only, legacy short form — tolerated until refactor.
    Legacy(String),
    /// Nowhere on the method — legacy derived default `{plugin}.{read|invoke}`.
    Derived(String),
}

impl CapabilityResolution {
    /// The resolved capability id, wherever it was found.
    pub fn capability(&self) -> &str {
        match self {
            Self::Declared(c) | Self::Undeclared(c) | Self::Legacy(c) | Self::Derived(c) => c,
        }
    }
}

/// Result of `PluginSchema::validate_capability_closure`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityClosure {
    /// Capability ids used by methods and declared on the plugin.
    pub declared: Vec<String>,
    /// Canonical-form ids used by methods but NOT declared — must be fixed.
    pub missing: Vec<String>,
    /// Legacy short-form ids on methods — tolerated until the refactor.
    pub legacy: Vec<String>,
    /// Methods with no capability at all, resolved to the legacy derived
    /// default `{plugin}.{read|invoke}` — tolerated until the refactor.
    pub derived: Vec<String>,
}

impl CapabilityClosure {
    /// True when there is nothing to fix (legacy/derived are tolerated).
    pub fn ok(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Canonical plugin capability guarantees.
///
/// This is the **single** definition of `PluginCapabilities` in the workspace.
/// The 4-field structure is the source of truth; all other definitions are
/// removed.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PluginCapabilities {
    /// Whether the plugin supports rolling back state changes.
    #[serde(default)]
    pub supports_rollback: bool,
    /// Whether the plugin supports checkpointing state.
    #[serde(default)]
    pub supports_checkpoints: bool,
    /// Whether the plugin supports state verification.
    #[serde(default)]
    pub supports_verification: bool,
    /// Whether the plugin's operations are atomic.
    #[serde(default)]
    pub atomic_operations: bool,
}

/// A property write is an explicit alias for one declared mutation method.
/// The target method's capability and input contract remain authoritative.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldWriteDecl {
    pub method: String,
    pub argument: String,
}

/// Plugin schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSchema {
    /// Plugin name
    pub name: String,
    /// Logical category used by the registry, renderer, and compliance overlays
    #[serde(default = "default_category")]
    pub category: String,
    /// Schema version
    pub version: String,
    /// Description
    pub description: String,
    /// Display name for UI/contract surfaces (e.g. "GB.Keypair"). Display-only;
    /// does not affect D-Bus path or interface name. None = use `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Fields in the plugin state
    pub fields: HashMap<String, FieldSchema>,
    /// Explicit allowlist for the shared public state projection. Absence or
    /// an empty policy withholds all state; methods and field declarations do
    /// not infer exposure.
    #[serde(default, skip_serializing_if = "PublicStatePolicy::is_empty")]
    pub public_state: PublicStatePolicy,
    /// Explicit property-to-method write bindings. Absence means that a field
    /// cannot be written through a generic property surface. This map grants
    /// no authority; callers must satisfy the mapped method's capability.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub field_writes: HashMap<String, FieldWriteDecl>,
    /// Dependencies on other plugins
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Example state for documentation
    #[serde(default)]
    pub example: Option<Value>,
    /// Paths that are always readOnly (e.g., ["/id", "/metadata"])
    #[serde(default)]
    pub immutable_paths: Vec<String>,
    /// Schema tags (e.g., ["immutable"] for fully immutable schemas)
    #[serde(default)]
    pub tags: Vec<String>,
    /// JSON Schema dialect to use (defaults to DEFAULT_SCHEMA_DIALECT)
    #[serde(default = "default_dialect")]
    pub dialect: String,
    /// Mutation index for the identity sled
    #[serde(default)]
    pub mutation_index: Option<u64>,
    /// OSCAL subids keyed by field/tool name (e.g. "code_search" →
    /// "obs.service.code-rag.search@v1"). The single place a field's
    /// operational taxonomy key is declared; surfaced via `field_input_schema`.
    #[serde(default)]
    pub subids: HashMap<String, String>,
    /// Organization/tenant this schema instance belongs to. Deliberately a
    /// separate field from the subid taxonomy (subids stay a universal,
    /// org-agnostic vocabulary shared across all tenants) — OSCAL/compliance
    /// validation (`validate_subid`, the subid registry) never reads this
    /// field. Its *value* follows the same dotted-hierarchy convention as a
    /// subid, just with its own vocabulary instead of the seven OSCAL
    /// categories:
    ///
    /// ```text
    /// <tenant>.<org-unit>[.<sub-unit>]*[@vN]
    /// ```
    ///
    /// e.g. `3tched.ops` or `3tched.customers.acme@v1`. No fixed category
    /// list here (unlike subid's seven), since org structure is
    /// deployment-specific, not an OSCAL vocabulary.
    /// None = no org scoping (single-tenant / not yet assigned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    /// Declared methods on the plugin's capability surface.
    #[serde(default)]
    pub methods: HashMap<String, MethodDecl>,
    /// Capabilities declared by this plugin, keyed by capability id.
    ///
    /// Single declaration point: every `MethodDecl::required_capability`
    /// must resolve to a key here. Empty map = no gated methods (or a
    /// legacy schema that predates the declaration point).
    #[serde(default)]
    pub capabilities: HashMap<String, CapabilityDecl>,
    /// Legacy per-footprint grant annotations carried by older schemas.
    /// Runtime authorization uses the independent principal grant store, not
    /// this field. Wildcard entries are discarded on read and never sealed.
    #[serde(
        default,
        skip_serializing_if = "capability_grants_have_no_exact_entries",
        serialize_with = "serialize_exact_capability_grants",
        deserialize_with = "deserialize_exact_capability_grants"
    )]
    pub capability_grants: HashMap<String, Vec<String>>,
    /// Declared signals emitted by the plugin.
    #[serde(default)]
    pub signals: Vec<SignalDecl>,
    /// Plugin capability guarantees (4-field canonical block).
    #[serde(default)]
    pub guarantees: PluginCapabilities,
}

fn capability_grants_have_no_exact_entries(grants: &HashMap<String, Vec<String>>) -> bool {
    grants.keys().all(|footprint| footprint == "*")
}

fn serialize_exact_capability_grants<S>(
    grants: &HashMap<String, Vec<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    grants
        .iter()
        .filter(|(footprint, _)| footprint.as_str() != "*")
        .collect::<HashMap<_, _>>()
        .serialize(serializer)
}

fn deserialize_exact_capability_grants<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut grants = HashMap::<String, Vec<String>>::deserialize(deserializer)?;
    grants.remove("*");
    Ok(grants)
}

fn default_dialect() -> String {
    DEFAULT_SCHEMA_DIALECT.to_string()
}

fn default_category() -> String {
    "uncategorized".to_string()
}

/// Key under which a plugin's schema-level OSCAL subid is stored in `subids`.
pub const SCHEMA_SUBID_KEY: &str = "__schema__";

impl PluginSchema {
    /// Check if the schema state is valid (for the identity sled)
    pub fn is_valid(&self) -> bool {
        !self.name.is_empty() && !self.version.is_empty()
    }

    /// Create a new plugin schema builder
    pub fn builder(name: &str) -> PluginSchemaBuilder {
        PluginSchemaBuilder::new(name)
    }

    /// Resolve the effective capability for one method.
    ///
    /// Migration rule — capabilities are found by looking in THREE places,
    /// in order, until the refactor completes:
    ///
    /// 1. `PluginSchema::capabilities` — the plugin's own declaration
    ///    (canonical home; a hit here is the end state).
    /// 2. `MethodDecl::required_capability` — the method's raw string,
    ///    which may be canonical-but-undeclared (must fix) or a legacy
    ///    short-form id like `agent.read` (tolerated for now).
    /// 3. Legacy derived default `{plugin}.{read|invoke}` — when the method
    ///    declares nothing, the vocabulary the generated clients stamp
    ///    today, derived from the plugin name and the method's side effect.
    pub fn resolve_method_capability(&self, method: &MethodDecl) -> CapabilityResolution {
        if let Some(cap) = method.required_capability.as_deref()
            && !cap.is_empty()
        {
            if self.capabilities.contains_key(cap) {
                return CapabilityResolution::Declared(cap.to_string());
            }
            if is_canonical_capability_id(cap) {
                return CapabilityResolution::Undeclared(cap.to_string());
            }
            return CapabilityResolution::Legacy(cap.to_string());
        }
        let op = match method.side_effect {
            SideEffect::Read => "read",
            SideEffect::Mutation => "invoke",
        };
        CapabilityResolution::Derived(format!("{}.{op}", self.name))
    }

    /// Check capability closure across all methods: every method must
    /// resolve, canonical ids must be declared, legacy/derived are reported
    /// for migration but tolerated.
    pub fn validate_capability_closure(&self) -> CapabilityClosure {
        let mut closure = CapabilityClosure::default();
        // Dedup per bucket: the same id may legitimately appear as both a
        // legacy method string and a derived default.
        let mut seen = std::collections::BTreeSet::new();
        for method in self.methods.values() {
            let (tag, bucket, cap) = match self.resolve_method_capability(method) {
                CapabilityResolution::Declared(c) => (0u8, &mut closure.declared, c),
                CapabilityResolution::Undeclared(c) => (1u8, &mut closure.missing, c),
                CapabilityResolution::Legacy(c) => (2u8, &mut closure.legacy, c),
                CapabilityResolution::Derived(c) => (3u8, &mut closure.derived, c),
            };
            if seen.insert((tag, cap.clone())) {
                bucket.push(cap);
            }
        }
        closure
    }

    /// Validate a state value against this schema
    pub fn validate(&self, state: &Value) -> ValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check required fields
        for (field_name, field_schema) in &self.fields {
            if field_schema.required && state.get(field_name).is_none() {
                errors.push(format!("Missing required field: {}", field_name));
            }
        }

        // Validate present fields
        if let Some(obj) = state.as_object() {
            for (field_name, field_value) in obj {
                if let Some(field_schema) = self.fields.get(field_name) {
                    if let Err(e) = validate_field(field_name, field_value, field_schema) {
                        errors.push(e);
                    }
                } else {
                    warnings.push(format!("Unknown field: {}", field_name));
                }
            }
        }

        ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
        }
    }

    /// Generate a template state with default values
    pub fn generate_template(&self) -> Value {
        let mut template = simd_json::value::owned::Object::new();

        for (field_name, field_schema) in &self.fields {
            let value = if let Some(default) = &field_schema.default {
                default.clone()
            } else if let Some(example) = &field_schema.example {
                example.clone()
            } else {
                default_for_type(&field_schema.field_type)
            };
            template.insert(field_name.clone(), value);
        }

        Value::Object(Box::new(template))
    }

    /// Convert to JSON Schema 2026 format (default)
    ///
    /// Includes support for:
    /// - `readOnly` on individual fields
    /// - `propertyDependencies` for conditional immutability
    /// - Schema-level immutability via tags
    pub fn to_json_schema(&self) -> Value {
        if self.fields.values().any(|field| {
            crate::exact_schema::contains_exact(&field.field_type).expect("bounded field type")
        }) {
            assert_eq!(
                self.dialect, DEFAULT_SCHEMA_DIALECT,
                "exact fields require the declared 2020-12 export dialect"
            );
        }
        let is_fully_immutable = self.tags.contains(&"immutable".to_string());
        let mut properties = simd_json::value::owned::Object::new();
        let mut required = Vec::new();
        let mut property_dependencies: HashMap<String, HashMap<String, Vec<String>>> =
            HashMap::new();

        for (field_name, field_schema) in &self.fields {
            let mut field_json = field_type_to_json_schema_2026(&field_schema.field_type);

            // Add description if present
            if !field_schema.description.is_empty()
                && let Some(obj) = field_json.as_object_mut()
            {
                obj.insert("description".to_string(), json!(field_schema.description));
            }

            // Add readOnly if field is unconditionally immutable, in immutable_paths, or schema is fully immutable
            let path = format!("/{}", field_name);
            if (field_schema.read_only
                || self.immutable_paths.contains(&path)
                || is_fully_immutable)
                && let Some(obj) = field_json.as_object_mut()
            {
                obj.insert("readOnly".to_string(), json!(true));
            }

            // Collect propertyDependencies for conditional readOnly
            if let Some(condition) = &field_schema.read_only_when {
                property_dependencies
                    .entry(condition.property.clone())
                    .or_default()
                    .entry(condition.value.clone())
                    .or_default()
                    .push(field_name.clone());
            }

            properties.insert(field_name.clone(), field_json);
            if field_schema.required {
                required.push(Value::String(field_name.clone()));
            }
        }

        let mut schema = json!({
            "$schema": &self.dialect,
            "title": self.name,
            "description": self.description,
            "x-plugin-category": self.category,
            "type": "object",
            "properties": properties,
            "required": required
        });

        // Add propertyDependencies if any conditional readOnly fields exist
        if !property_dependencies.is_empty() {
            let mut deps_json = simd_json::value::owned::Object::new();
            for (prop, value_map) in property_dependencies {
                let mut values_json = simd_json::value::owned::Object::new();
                for (value, fields) in value_map {
                    let mut props = simd_json::value::owned::Object::new();
                    for field in fields {
                        props.insert(field, json!({"readOnly": true}));
                    }
                    values_json.insert(
                        value,
                        json!({
                            "properties": props
                        }),
                    );
                }
                deps_json.insert(prop, Value::Object(Box::new(values_json)));
            }
            if let Some(obj) = schema.as_object_mut() {
                obj.insert(
                    "propertyDependencies".to_string(),
                    Value::Object(Box::new(deps_json)),
                );
            }
        }

        crate::exact_schema::scope_field_exports(&mut schema, &self.fields, "");
        schema
    }

    /// Set the methods map on this schema (builder-style).
    pub fn with_methods(mut self, methods: HashMap<String, MethodDecl>) -> Self {
        self.methods = methods;
        self
    }

    /// Set the signals list on this schema (builder-style).
    pub fn with_signals(mut self, signals: Vec<SignalDecl>) -> Self {
        self.signals = signals;
        self
    }

    /// Set the guarantees block on this schema (builder-style).
    pub fn with_guarantees(mut self, guarantees: PluginCapabilities) -> Self {
        self.guarantees = guarantees;
        self
    }

    /// Render a single named field as a standalone JSON Schema suitable for an
    /// MCP tool's `input_schema()`. The field is expected to be a
    /// `FieldType::Object` describing the tool's input parameters.
    ///
    /// This makes the `PluginSchema` the single source of truth for a tool's
    /// input contract: the tool derives its schema from here rather than
    /// declaring it inline. The field's registered OSCAL subid (if any) is
    /// attached as `x-oscal-subid`. Returns `None` if the field is absent.
    pub fn field_input_schema(&self, field_name: &str) -> Option<Value> {
        let field = self.fields.get(field_name)?;
        let mut schema = field_type_to_json_schema_2026(&field.field_type);
        if let Some(obj) = schema.as_object_mut() {
            if !field.description.is_empty() {
                obj.insert("description".to_string(), json!(field.description));
            }
            if let Some(subid) = self.subids.get(field_name) {
                obj.insert("x-oscal-subid".to_string(), json!(subid));
            }
        }
        crate::exact_schema::scope_type_export(&mut schema, &field.field_type, "");
        Some(schema)
    }

    /// Convert to JSON Schema draft-07 format (deprecated, for backward compatibility)
    #[deprecated(since = "2.0.0", note = "Use to_json_schema() for JSON Schema 2026")]
    pub fn to_json_schema_draft07(&self) -> Value {
        assert!(
            !self
                .fields
                .values()
                .any(
                    |field| crate::exact_schema::contains_exact(&field.field_type)
                        .expect("bounded field type")
                ),
            "exact 2020-12 fields cannot use legacy draft07 export"
        );
        let mut properties = simd_json::value::owned::Object::new();
        let mut required = Vec::new();

        for (field_name, field_schema) in &self.fields {
            properties.insert(
                field_name.clone(),
                field_type_to_json_schema(&field_schema.field_type),
            );
            if field_schema.required {
                required.push(Value::String(field_name.clone()));
            }
        }

        let mut schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": self.name,
            "description": self.description,
            "type": "object",
            "properties": properties,
            "required": required
        });
        crate::exact_schema::scope_field_exports(&mut schema, &self.fields, "");
        schema
    }

    /// Convert to the legacy contract-style schema while using this registry
    /// entry as the only source of truth for the tunable section.
    pub fn to_contract_json_schema(&self) -> Value {
        self.to_contract_json_schema_as(&self.name)
    }

    /// Convert to the legacy contract-style schema using an alternate public
    /// plugin name for compatibility aliases such as `systemd`.
    pub fn to_contract_json_schema_as(&self, public_name: &str) -> Value {
        let mut include_paths: Vec<String> = self
            .fields
            .keys()
            .map(|field| format!("/tunable/{field}"))
            .collect();
        include_paths.sort();

        let mut secret_paths: Vec<String> = self
            .fields
            .keys()
            .filter(|field| is_secret_field_name(field))
            .map(|field| format!("/tunable/{field}"))
            .collect();
        secret_paths.sort();

        let mut pii_paths: Vec<String> = self
            .fields
            .keys()
            .filter(|field| is_pii_field_name(field))
            .map(|field| format!("/tunable/{field}"))
            .collect();
        pii_paths.sort();

        let sensitivity = if secret_paths.is_empty() {
            "internal"
        } else {
            "secret"
        };

        json!({
            "$schema": DEFAULT_SCHEMA_DIALECT,
            "$id": format!("https://op-dbus.local/schemas/plugins/{public_name}.contract.json"),
            "title": format!("{public_name} contract schema"),
            "description": self.description,
            "type": "object",
            "required": [
                "schema_version",
                "plugin",
                "object_type",
                "object_id",
                "stub",
                "immutable",
                "tunable",
                "observed",
                "meta",
                "semantic_index",
                "privacy_index"
            ],
            "properties": {
                "schema_version": {
                    "type": "string",
                    "const": self.version
                },
                "plugin": {
                    "type": "string",
                    "const": public_name
                },
                "object_type": {
                    "type": "string",
                    "const": format!("{}_object", public_name.replace('-', "_"))
                },
                "object_id": {
                    "type": "string",
                    "minLength": 1
                },
                "stub": {
                    "type": "object",
                    "required": ["system_id", "source", "source_ref", "discovered_at"],
                    "properties": {
                        "system_id": { "type": "string", "minLength": 1 },
                        "source": { "type": "string", "minLength": 1 },
                        "source_ref": { "type": "string", "minLength": 1 },
                        "discovered_at": { "type": "string", "format": "date-time" }
                    },
                    "additionalProperties": false
                },
                "immutable": {
                    "type": "object",
                    "required": ["created_at", "created_by_plugin", "identity_keys", "provider"],
                    "properties": {
                        "created_at": { "type": "string", "format": "date-time" },
                        "created_by_plugin": { "type": "string", "const": public_name },
                        "identity_keys": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "default": ["object_id"]
                        },
                        "provider": { "type": "string", "default": "op-dbus" }
                    },
                    "additionalProperties": false
                },
                "tunable": self.to_json_schema(),
                "observed": {
                    "type": "object",
                    "required": ["last_observed_at"],
                    "properties": {
                        "last_observed_at": { "type": "string", "format": "date-time" },
                        "status": { "type": "string" },
                        "drift_detected": { "type": "boolean", "default": false },
                        "metrics": { "type": "object" }
                    },
                    "additionalProperties": true
                },
                "meta": {
                    "type": "object",
                    "required": [
                        "dependencies",
                        "include_in_recovery",
                        "recovery_priority",
                        "category",
                        "sensitivity",
                        "tags",
                        "enabled"
                    ],
                    "properties": {
                        "dependencies": {
                            "type": "array",
                            "items": { "type": "string" },
                            "default": self.dependencies
                        },
                        "include_in_recovery": { "type": "boolean", "default": true },
                        "recovery_priority": { "type": "integer", "minimum": 0, "default": 50 },
                        "category": {
                            "type": "string",
                            "default": self.category
                        },
                        "sensitivity": {
                            "type": "string",
                            "enum": ["public", "internal", "secret"],
                            "default": sensitivity
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "default": self.tags
                        },
                        "enabled": { "type": "boolean", "default": true }
                    },
                    "additionalProperties": false
                },
                "semantic_index": {
                    "type": "object",
                    "required": ["include_paths", "exclude_paths", "chunking", "redaction"],
                    "properties": {
                        "include_paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "default": include_paths
                        },
                        "exclude_paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "default": ["/stub/discovered_at"]
                        },
                        "chunking": {
                            "type": "object",
                            "required": ["strategy", "max_tokens"],
                            "properties": {
                                "strategy": { "type": "string", "enum": ["json-path-group"], "default": "json-path-group" },
                                "max_tokens": { "type": "integer", "minimum": 64, "default": 512 }
                            },
                            "additionalProperties": false
                        },
                        "redaction": {
                            "type": "object",
                            "required": ["enabled"],
                            "properties": {
                                "enabled": { "type": "boolean", "default": true }
                            },
                            "additionalProperties": false
                        }
                    },
                    "additionalProperties": false
                },
                "privacy_index": {
                    "type": "object",
                    "required": ["redaction"],
                    "properties": {
                        "redaction": {
                            "type": "object",
                            "required": [
                                "rules",
                                "default_action",
                                "secret_paths",
                                "pii_paths",
                                "hash_salt_ref",
                                "reversible"
                            ],
                            "properties": {
                                "rules": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "required": ["path", "action"],
                                        "properties": {
                                            "path": { "type": "string" },
                                            "action": { "type": "string", "enum": ["drop", "mask", "hash"] },
                                            "reason": { "type": "string" }
                                        },
                                        "additionalProperties": false
                                    },
                                    "default": []
                                },
                                "default_action": {
                                    "type": "string",
                                    "enum": ["drop", "mask", "hash"],
                                    "default": "mask"
                                },
                                "secret_paths": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "default": secret_paths
                                },
                                "pii_paths": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "default": pii_paths
                                },
                                "hash_salt_ref": {
                                    "type": "string",
                                    "default": "vault://op-dbus/privacy/hash-salt"
                                },
                                "reversible": {
                                    "type": "boolean",
                                    "default": false
                                }
                            },
                            "additionalProperties": false
                        }
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        })
    }
}

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Builder for creating plugin schemas
pub struct PluginSchemaBuilder {
    name: String,
    category: String,
    version: String,
    description: String,
    display_name: Option<String>,
    fields: HashMap<String, FieldSchema>,
    public_state: PublicStatePolicy,
    field_writes: HashMap<String, FieldWriteDecl>,
    dependencies: Vec<String>,
    example: Option<Value>,
    immutable_paths: Vec<String>,
    tags: Vec<String>,
    dialect: String,
    subids: HashMap<String, String>,
    org: Option<String>,
    methods: HashMap<String, MethodDecl>,
    capabilities: HashMap<String, CapabilityDecl>,
    signals: Vec<SignalDecl>,
    guarantees: PluginCapabilities,
}

impl PluginSchemaBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            category: default_category(),
            version: "1.0.0".to_string(),
            description: String::new(),
            display_name: None,
            fields: HashMap::new(),
            public_state: PublicStatePolicy::default(),
            field_writes: HashMap::new(),
            dependencies: Vec::new(),
            example: None,
            immutable_paths: Vec::new(),
            tags: Vec::new(),
            dialect: DEFAULT_SCHEMA_DIALECT.to_string(),
            subids: HashMap::new(),
            org: None,
            methods: HashMap::new(),
            capabilities: HashMap::new(),
            signals: Vec::new(),
            guarantees: PluginCapabilities::default(),
        }
    }

    pub fn version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }

    /// Bind a property to its plugin-owned mutation method and input argument.
    pub fn field_write(mut self, property: &str, method: &str, argument: &str) -> Self {
        self.field_writes.insert(
            property.to_string(),
            FieldWriteDecl {
                method: method.to_string(),
                argument: argument.to_string(),
            },
        );
        self
    }

    pub fn category(mut self, category: &str) -> Self {
        self.category = category.to_string();
        self
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    /// Set the display name (e.g. "GB.Keypair"). Display-only.
    pub fn display_name(mut self, display_name: &str) -> Self {
        self.display_name = Some(display_name.to_string());
        self
    }

    /// Set the org/tenant this schema instance belongs to. Separate field
    /// from the subid taxonomy, but the value should follow the same
    /// dotted-hierarchy format as a subid (`<tenant>.<org-unit>[.<sub-unit>]*[@vN]`)
    /// — see `PluginSchema::org`'s doc comment.
    pub fn org(mut self, org: &str) -> Self {
        self.org = Some(org.to_string());
        self
    }

    pub fn field(mut self, name: &str, schema: FieldSchema) -> Self {
        self.fields.insert(name.to_string(), schema);
        self
    }

    /// Publish one schema-declared JSON pointer through the shared public
    /// state projection. The projector validates the complete policy and
    /// fails closed on opaque types, invalid paths, or state type drift.
    pub fn public_state_path(mut self, path: &str) -> Self {
        self.public_state.paths.push(path.to_string());
        self
    }

    /// Publish multiple schema-declared JSON pointers through the shared
    /// public state projection.
    pub fn public_state_paths(mut self, paths: &[&str]) -> Self {
        self.public_state
            .paths
            .extend(paths.iter().map(|path| (*path).to_string()));
        self
    }

    pub fn string_field(self, name: &str, required: bool, description: &str) -> Self {
        self.field(
            name,
            FieldSchema {
                field_type: FieldType::String,
                required,
                description: description.to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
    }

    pub fn integer_field(self, name: &str, required: bool, description: &str) -> Self {
        self.field(
            name,
            FieldSchema {
                field_type: FieldType::Integer,
                required,
                description: description.to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
    }

    pub fn boolean_field(self, name: &str, required: bool, description: &str) -> Self {
        self.field(
            name,
            FieldSchema {
                field_type: FieldType::Boolean,
                required,
                description: description.to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
    }

    pub fn array_field(
        self,
        name: &str,
        item_type: FieldType,
        required: bool,
        description: &str,
    ) -> Self {
        self.field(
            name,
            FieldSchema {
                field_type: FieldType::Array(Box::new(item_type)),
                required,
                description: description.to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
    }

    pub fn object_field(
        self,
        name: &str,
        fields: HashMap<String, FieldSchema>,
        required: bool,
        description: &str,
    ) -> Self {
        self.field(
            name,
            FieldSchema {
                field_type: FieldType::Object(fields),
                required,
                description: description.to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
    }

    pub fn dependency(mut self, plugin_name: &str) -> Self {
        self.dependencies.push(plugin_name.to_string());
        self
    }

    pub fn example(mut self, example: Value) -> Self {
        self.example = Some(example);
        self
    }

    /// Add a path that should always be readOnly (e.g., "/id")
    pub fn immutable_path(mut self, path: &str) -> Self {
        self.immutable_paths.push(path.to_string());
        self
    }

    /// Add multiple immutable paths at once
    pub fn immutable_paths(mut self, paths: &[&str]) -> Self {
        self.immutable_paths
            .extend(paths.iter().map(|s| s.to_string()));
        self
    }

    /// Add a tag to the schema (e.g., "immutable" for fully immutable)
    pub fn tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Mark the entire schema as immutable
    pub fn fully_immutable(self) -> Self {
        self.tag("immutable")
    }

    /// Set the JSON Schema dialect (e.g., dialects::DRAFT_2020_12)
    pub fn dialect(mut self, dialect: &str) -> Self {
        self.dialect = dialect.to_string();
        self
    }

    /// Register an OSCAL subid for a named field/tool. This is the single
    /// declaration site for that artifact's taxonomy key; it is surfaced in the
    /// field's rendered input schema (`field_input_schema`) as `x-oscal-subid`.
    pub fn subid(mut self, field: &str, subid: &str) -> Self {
        self.subids.insert(field.to_string(), subid.to_string());
        self
    }

    /// Set the methods map for the schema being built.
    pub fn methods(mut self, methods: HashMap<String, MethodDecl>) -> Self {
        self.methods = methods;
        self
    }

    /// Set the signals list for the schema being built.
    pub fn signals(mut self, signals: Vec<SignalDecl>) -> Self {
        self.signals = signals;
        self
    }

    /// Add a single method declaration to the schema being built.
    pub fn method(mut self, method: MethodDecl) -> Self {
        self.methods.insert(method.name.clone(), method);
        self
    }

    /// Set the declared capabilities map for the schema being built.
    pub fn capabilities(mut self, capabilities: HashMap<String, CapabilityDecl>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Add a single capability declaration to the schema being built.
    pub fn capability(mut self, capability: CapabilityDecl) -> Self {
        self.capabilities.insert(capability.id.clone(), capability);
        self
    }

    /// Add a single signal declaration to the schema being built.
    pub fn signal(mut self, signal: SignalDecl) -> Self {
        self.signals.push(signal);
        self
    }

    /// Set the guarantees block for the schema being built.
    pub fn guarantees(mut self, guarantees: PluginCapabilities) -> Self {
        self.guarantees = guarantees;
        self
    }

    pub fn build(self) -> PluginSchema {
        PluginSchema {
            name: self.name,
            category: self.category,
            version: self.version,
            description: self.description,
            display_name: self.display_name,
            fields: self.fields,
            public_state: self.public_state,
            field_writes: self.field_writes,
            dependencies: self.dependencies,
            example: self.example,
            immutable_paths: self.immutable_paths,
            tags: self.tags,
            dialect: self.dialect,
            mutation_index: None,
            subids: self.subids,
            org: self.org,
            methods: self.methods,
            capabilities: self.capabilities,
            signals: self.signals,
            guarantees: self.guarantees,
            capability_grants: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredSchemaCopies {
    pub plugin: PluginSchema,
    pub json_schema: Value,
    pub contract_schema: Value,
}

/// In-memory schema catalog.
///
/// Compatibility note:
/// this type is still named `SchemaRegistry` in much of the workspace, but its
/// architectural role is now a catalog/index over canonical plugin documents.
/// It stores derived schema copies for lookup, validation, rendering, and
/// compatibility export. It is not the origin of schema truth.
pub struct SchemaRegistry {
    schemas: HashMap<String, PluginSchema>,
    categorized: HashMap<String, HashMap<String, StoredSchemaCopies>>,
    meta_schemas: HashMap<String, Value>,
    spec_base_path: Option<PathBuf>,
}

impl SchemaRegistry {
    /// Create an empty catalog. Runtime code should populate this from plugins
    /// or from persisted canonical plugin documents.
    pub fn empty() -> Self {
        Self {
            schemas: HashMap::new(),
            categorized: HashMap::new(),
            meta_schemas: HashMap::new(),
            spec_base_path: None,
        }
    }

    /// Create a new runtime catalog.
    ///
    /// Plugins are the canonical schema source; runtime code should register
    /// plugin-provided schemas into this empty catalog.
    pub fn new() -> Self {
        Self::empty()
    }

    /// Create a runtime catalog with a custom spec base path.
    pub fn with_spec_path(spec_path: impl AsRef<Path>) -> Self {
        let mut registry = Self::empty();
        registry.spec_base_path = Some(spec_path.as_ref().to_path_buf());
        registry
    }

    /// Set the base path for the json-schema-spec repository
    pub fn set_spec_path(&mut self, path: impl AsRef<Path>) {
        self.spec_base_path = Some(path.as_ref().to_path_buf());
    }

    /// Load meta-schema from the spec repository
    pub fn load_meta_schema(&mut self, dialect: &str) -> Result<(), SchemaLoadError> {
        let spec_path = self
            .spec_base_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(SCHEMA_SPEC_PATH));

        // Map dialect URL to file path
        let meta_path = match dialect {
            d if d.contains("2026") => spec_path.join("specs/meta/meta.json"),
            _ => return Err(SchemaLoadError::UnsupportedDialect(dialect.to_string())),
        };

        let mut content = fs::read_to_string(&meta_path)
            .map_err(|e| SchemaLoadError::IoError(meta_path.clone(), e.to_string()))?;

        let schema: Value = unsafe { simd_json::from_str(&mut content) }
            .map_err(|e| SchemaLoadError::ParseError(meta_path.clone(), e.to_string()))?;

        self.meta_schemas.insert(dialect.to_string(), schema);
        Ok(())
    }

    /// Get a loaded meta-schema
    pub fn get_meta_schema(&self, dialect: &str) -> Option<&Value> {
        self.meta_schemas.get(dialect)
    }

    /// Export all schemas as JSON Schema documents
    pub fn export_all(&self) -> HashMap<String, Value> {
        self.schemas
            .iter()
            .map(|(name, schema)| (name.clone(), schema.to_json_schema()))
            .collect()
    }

    /// Export all schemas in draft-07 format (for backward compatibility)
    #[allow(deprecated)]
    pub fn export_all_draft07(&self) -> HashMap<String, Value> {
        self.schemas
            .iter()
            .map(|(name, schema)| (name.clone(), schema.to_json_schema_draft07()))
            .collect()
    }

    /// Export all schemas as legacy contract documents keyed by canonical plugin name.
    pub fn export_all_contract(&self) -> HashMap<String, Value> {
        self.schemas
            .iter()
            .map(|(name, schema)| (name.clone(), schema.to_contract_json_schema()))
            .collect()
    }

    /// Index one plugin schema and cache all derived copies under its category.
    pub fn register(&mut self, schema: PluginSchema) {
        let copies = StoredSchemaCopies {
            json_schema: schema.to_json_schema(),
            contract_schema: schema.to_contract_json_schema(),
            plugin: schema.clone(),
        };

        self.categorized
            .entry(schema.category.clone())
            .or_default()
            .insert(schema.name.clone(), copies);
        self.schemas.insert(schema.name.clone(), schema);
    }

    /// Get a plugin schema by name
    pub fn get(&self, name: &str) -> Option<&PluginSchema> {
        self.schemas.get(Self::canonical_name(name))
    }

    /// Get the categorized schema copies stored for a plugin.
    pub fn get_copies(&self, name: &str) -> Option<&StoredSchemaCopies> {
        let schema = self.get(name)?;
        self.categorized
            .get(&schema.category)
            .and_then(|schemas| schemas.get(&schema.name))
    }

    /// Export one schema as a legacy contract document, preserving alias names.
    pub fn export_contract_for(&self, name: &str) -> Option<Value> {
        self.get(name)
            .map(|schema| schema.to_contract_json_schema_as(name))
    }

    /// List all registered schema names
    pub fn list(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }

    /// List all schema categories currently present in the catalog.
    pub fn categories(&self) -> Vec<&str> {
        let mut categories: Vec<&str> = self.categorized.keys().map(|s| s.as_str()).collect();
        categories.sort_unstable();
        categories
    }

    /// Return all schema copies stored under one category.
    pub fn by_category(&self, category: &str) -> Option<&HashMap<String, StoredSchemaCopies>> {
        self.categorized.get(category)
    }

    /// Validate state for a plugin
    pub fn validate(&self, plugin_name: &str, state: &Value) -> Option<ValidationResult> {
        self.get(plugin_name).map(|schema| schema.validate(state))
    }

    fn canonical_name(name: &str) -> &str {
        match name {
            "incus_wireguard_ingress" => "incus-wireguard-ingress",
            "incus_xray_reality_client" => "incus-xray-reality-client",
            "incus_xray_reality_server" => "incus-xray-reality-server",
            "network" => "net",
            "systemd" | "dinit" | "s6" | "runit" => "service",
            "web-ui" => "web_ui",
            other => other,
        }
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Preferred architectural name for `SchemaRegistry`.
///
/// The old name remains for compatibility while the workspace is migrated
/// crate by crate, but new code should talk in terms of a schema catalog.
pub type SchemaCatalog = SchemaRegistry;

/// Errors that can occur when loading schemas
#[derive(Debug, Clone)]
pub enum SchemaLoadError {
    IoError(PathBuf, String),
    ParseError(PathBuf, String),
    UnsupportedDialect(String),
}

impl std::fmt::Display for SchemaLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(path, msg) => write!(f, "IO error reading {:?}: {}", path, msg),
            Self::ParseError(path, msg) => write!(f, "Parse error in {:?}: {}", path, msg),
            Self::UnsupportedDialect(d) => write!(f, "Unsupported dialect: {}", d),
        }
    }
}

impl std::error::Error for SchemaLoadError {}

// ============================================================================
// Schema helpers
// ============================================================================

fn is_secret_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "secret",
        "private",
        "token",
        "password",
        "credential",
        "license",
        "api_key",
        "key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_pii_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["email", "account", "google_id", "google_email", "user_id"]
        .iter()
        .any(|needle| lower.contains(needle))
}

// ============================================================================
// Helper Functions
// ============================================================================

fn validate_field(name: &str, value: &Value, schema: &FieldSchema) -> Result<(), String> {
    validate_value_against_type(name, value, &schema.field_type)?;

    // Validate constraints
    for constraint in &schema.constraints {
        match constraint {
            Constraint::Min { value: min } => {
                if let Some(n) = value.as_f64()
                    && n < *min
                {
                    return Err(format!("Field '{}' must be >= {}", name, min));
                }
                if let Some(s) = value.as_str()
                    && (s.len() as f64) < *min
                {
                    return Err(format!("Field '{}' length must be >= {}", name, min));
                }
            }
            Constraint::Max { value: max } => {
                if let Some(n) = value.as_f64()
                    && n > *max
                {
                    return Err(format!("Field '{}' must be <= {}", name, max));
                }
                if let Some(s) = value.as_str()
                    && (s.len() as f64) > *max
                {
                    return Err(format!("Field '{}' length must be <= {}", name, max));
                }
            }
            Constraint::Pattern { regex } => {
                if let Some(s) = value.as_str()
                    && let Ok(re) = regex::Regex::new(regex)
                    && !re.is_match(s)
                {
                    return Err(format!("Field '{}' must match pattern: {}", name, regex));
                }
            }
            Constraint::OneOf { values } if !values.contains(value) => {
                return Err(format!("Field '{}' must be one of: {:?}", name, values));
            }
            _ => {}
        }
    }

    Ok(())
}

pub(crate) fn validate_value_against_type(
    name: &str,
    value: &Value,
    field_type: &FieldType,
) -> Result<(), String> {
    match field_type {
        FieldType::String => {
            if !value.is_str() {
                return Err(format!("Field '{}' must be a string", name));
            }
        }
        FieldType::Integer => {
            if !value.is_i64() && !value.is_u64() {
                return Err(format!("Field '{}' must be an integer", name));
            }
        }
        FieldType::Float => {
            if !value.is_f64() && !value.is_i64() {
                return Err(format!("Field '{}' must be a number", name));
            }
        }
        FieldType::Boolean => {
            if !value.is_bool() {
                return Err(format!("Field '{}' must be a boolean", name));
            }
        }
        FieldType::Array(_) => {
            if !value.is_array() {
                return Err(format!("Field '{}' must be an array", name));
            }
            if let Some(items) = value.as_array()
                && let FieldType::Array(item_type) = field_type
            {
                for (index, item) in items.iter().enumerate() {
                    validate_value_against_type(&format!("{}[{}]", name, index), item, item_type)?;
                }
            }
        }
        FieldType::Map(item_type) => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("Field '{}' must be an object map", name))?;
            for item in object.values() {
                // Do not include dynamic (possibly private) keys in errors.
                validate_value_against_type(&format!("{}[map value]", name), item, item_type)?;
            }
        }
        FieldType::Object(fields) => {
            if !value.is_object() {
                return Err(format!("Field '{}' must be an object", name));
            }
            validate_object_fields(name, value, fields)?;
        }
        FieldType::Enum(valid_values) => {
            if let Some(s) = value.as_str() {
                if !valid_values.contains(&s.to_string()) {
                    return Err(format!(
                        "Field '{}' must be one of: {:?}",
                        name, valid_values
                    ));
                }
            } else {
                return Err(format!("Field '{}' must be a string enum value", name));
            }
        }
        FieldType::OneOf(branches) => {
            if !branches
                .iter()
                .any(|branch| validate_value_against_type(name, value, branch).is_ok())
            {
                return Err(format!(
                    "Field '{}' must match one of the allowed variants",
                    name
                ));
            }
        }
        FieldType::JsonSchema(schema) => {
            crate::exact_schema::validate_value(schema, value).map_err(|error| error.to_string())?
        }
        FieldType::Any => {}
    }

    Ok(())
}

fn validate_object_fields(
    name: &str,
    value: &Value,
    fields: &HashMap<String, FieldSchema>,
) -> Result<(), String> {
    let Some(obj) = value.as_object() else {
        return Err(format!("Field '{}' must be an object", name));
    };

    for (field_name, field_schema) in fields {
        if field_schema.required && obj.get(field_name).is_none() {
            return Err(format!("Missing required field: {}.{}", name, field_name));
        }
    }

    for (field_name, field_value) in obj {
        if let Some(field_schema) = fields.get(field_name) {
            validate_field(
                &format!("{}.{}", name, field_name),
                field_value,
                field_schema,
            )?;
        }
    }

    Ok(())
}

fn default_for_type(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::String => json!(""),
        FieldType::Integer => json!(0),
        FieldType::Float => json!(0.0),
        FieldType::Boolean => json!(false),
        FieldType::Array(_) => json!([]),
        FieldType::Object(_) | FieldType::Map(_) => json!({}),
        FieldType::Enum(values) => values.first().map(|s| json!(s)).unwrap_or(json!("")),
        FieldType::OneOf(branches) => branches
            .first()
            .map(default_for_type)
            .unwrap_or(json!(null)),
        FieldType::JsonSchema(_) | FieldType::Any => json!(null),
    }
}

fn field_type_to_json_schema(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::JsonSchema(schema) => schema.as_ref().clone(),
        FieldType::Map(item_type) => {
            json!({"type":"object", "additionalProperties": typed_map_value_to_json_schema(item_type)})
        }
        FieldType::String => json!({"type": "string"}),
        FieldType::Integer => json!({"type": "integer"}),
        FieldType::Float => json!({"type": "number"}),
        FieldType::Boolean => json!({"type": "boolean"}),
        FieldType::Array(item_type) => json!({
            "type": "array",
            "items": field_type_to_json_schema(item_type)
        }),
        FieldType::Object(fields) => {
            let mut properties = simd_json::value::owned::Object::new();
            for (name, schema) in fields {
                properties.insert(name.clone(), field_type_to_json_schema(&schema.field_type));
            }
            json!({
                "type": "object",
                "properties": properties
            })
        }
        FieldType::Enum(values) => json!({
            "type": "string",
            "enum": values
        }),
        FieldType::OneOf(branches) => json!({
            "oneOf": branches
                .iter()
                .map(field_type_to_json_schema)
                .collect::<Vec<_>>()
        }),
        FieldType::Any => json!({}),
    }
}

/// Export a map value without dropping constraints on its named descendants.
/// The map's own value type has no metadata slot; producer preflight rejects
/// constraints there that Map(Box<FieldType>) cannot represent.
fn typed_map_value_to_json_schema(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::Map(item) => {
            json!({"type":"object", "additionalProperties": typed_map_value_to_json_schema(item)})
        }
        FieldType::Array(item) => {
            json!({"type":"array", "items": typed_map_value_to_json_schema(item)})
        }
        FieldType::Object(fields) => {
            let mut properties = simd_json::value::owned::Object::new();
            let mut required = Vec::new();
            for (name, field) in fields {
                let mut schema = typed_map_value_to_json_schema(&field.field_type);
                let object = schema.as_object_mut().expect("type schema is object");
                if !field.description.is_empty() {
                    object.insert("description".into(), json!(field.description));
                }
                if field.read_only {
                    object.insert("readOnly".into(), json!(true));
                }
                for constraint in &field.constraints {
                    match constraint {
                        Constraint::Min { value } => {
                            object.insert("minimum".into(), json!(value));
                        }
                        Constraint::Max { value } => {
                            object.insert("maximum".into(), json!(value));
                        }
                        Constraint::Pattern { regex } => {
                            object.insert("pattern".into(), json!(regex));
                        }
                        Constraint::OneOf { values } => {
                            object.insert("enum".into(), json!(values));
                        }
                        _ => {}
                    }
                }
                properties.insert(name.clone(), schema);
                if field.required {
                    required.push(name.clone());
                }
            }
            json!({"type":"object", "properties": properties, "required": required})
        }
        FieldType::OneOf(branches) => {
            json!({"oneOf": branches.iter().map(typed_map_value_to_json_schema).collect::<Vec<_>>()})
        }
        _ => field_type_to_json_schema_2026(field_type),
    }
}

/// Convert field type to JSON Schema 2026 format with full metadata
fn field_type_to_json_schema_2026(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::JsonSchema(schema) => schema.as_ref().clone(),
        FieldType::Map(item_type) => {
            json!({"type":"object", "additionalProperties": typed_map_value_to_json_schema(item_type)})
        }
        FieldType::String => json!({"type": "string"}),
        FieldType::Integer => json!({"type": "integer"}),
        FieldType::Float => json!({"type": "number"}),
        FieldType::Boolean => json!({"type": "boolean"}),
        FieldType::Array(item_type) => json!({
            "type": "array",
            "items": field_type_to_json_schema_2026(item_type)
        }),
        FieldType::Object(fields) => {
            let mut properties = simd_json::value::owned::Object::new();
            let mut required = Vec::new();
            for (name, schema) in fields {
                let mut field_json = field_type_to_json_schema_2026(&schema.field_type);
                if !schema.description.is_empty()
                    && let Some(obj) = field_json.as_object_mut()
                {
                    obj.insert("description".to_string(), json!(schema.description));
                }
                if schema.read_only
                    && let Some(obj) = field_json.as_object_mut()
                {
                    obj.insert("readOnly".to_string(), json!(true));
                }
                properties.insert(name.clone(), field_json);
                if schema.required {
                    required.push(json!(name));
                }
            }
            let mut result = json!({
                "type": "object",
                "properties": properties
            });
            if !required.is_empty()
                && let Some(obj) = result.as_object_mut()
            {
                obj.insert("required".to_string(), json!(required));
            }
            result
        }
        FieldType::Enum(values) => json!({
            "type": "string",
            "enum": values
        }),
        FieldType::OneOf(branches) => json!({
            "oneOf": branches
                .iter()
                .map(field_type_to_json_schema_2026)
                .collect::<Vec<_>>()
        }),
        FieldType::Any => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_write_bindings_are_optional_and_round_trip_in_plugin_schema() {
        let legacy = PluginSchema::builder("fixture").build();
        let serialized = serde_json::to_value(&legacy).unwrap();
        assert!(serialized.get("field_writes").is_none());
        let decoded: PluginSchema = serde_json::from_value(serialized.clone()).unwrap();
        assert!(decoded.field_writes.is_empty());
        assert_eq!(serde_json::to_value(decoded).unwrap(), serialized);

        let bound = PluginSchema::builder("fixture")
            .string_field("selected", false, "Selection")
            .field_write("selected", "Select", "selection")
            .build();
        let decoded: PluginSchema =
            serde_json::from_value(serde_json::to_value(bound).unwrap()).unwrap();
        assert_eq!(
            decoded.field_writes["selected"],
            FieldWriteDecl {
                method: "Select".into(),
                argument: "selection".into()
            }
        );
    }

    // ====================================================================
    // Capability declaration closure (migration: 3-place lookup)
    // ====================================================================

    fn cap_method(name: &str, cap: Option<&str>, side: SideEffect) -> MethodDecl {
        let idempotent = matches!(side, SideEffect::Read);
        MethodDecl {
            name: name.to_string(),
            args: json!({"type": "object"}),
            returns: None,
            side_effect: side,
            idempotent,
            required_capability: cap.map(str::to_string),
            subid: format!("obs.service.demo.{}@v1", name.to_lowercase()),
        }
    }

    #[test]
    fn canonical_capability_ids_are_recognized() {
        assert!(is_canonical_capability_id(
            "cap.software.zeroclaw.gateway-config.read@v1"
        ));
        assert!(!is_canonical_capability_id("agent.read"));
        assert!(!is_canonical_capability_id("cap.software.zeroclaw.read"));
        assert!(!is_canonical_capability_id(""));
    }

    #[test]
    fn wildcard_capability_grants_are_never_sealed_or_rehydrated() {
        let mut schema = PluginSchema::builder("demo").build();
        schema
            .capability_grants
            .insert("*".to_string(), vec!["demo.admin".to_string()]);
        schema
            .capability_grants
            .insert("exact-footprint".to_string(), vec!["demo.read".to_string()]);

        let encoded = serde_json::to_value(&schema).expect("schema serializes");
        assert!(encoded["capability_grants"].get("*").is_none());
        assert_eq!(
            encoded["capability_grants"]["exact-footprint"],
            serde_json::json!(["demo.read"])
        );

        let mut legacy = encoded;
        legacy["capability_grants"]["*"] = serde_json::json!(["demo.admin"]);
        let decoded: PluginSchema = serde_json::from_value(legacy).expect("legacy schema parses");
        assert!(!decoded.capability_grants.contains_key("*"));
        assert!(decoded.capability_grants.contains_key("exact-footprint"));
    }

    #[test]
    fn capability_resolution_looks_in_three_places() {
        let schema = PluginSchema::builder("demo")
            .capability(CapabilityDecl {
                id: "cap.software.demo.state.read@v1".to_string(),
                description: "read demo state".to_string(),
            })
            .method(cap_method(
                "GetState",
                Some("cap.software.demo.state.read@v1"),
                SideEffect::Read,
            ))
            .method(cap_method(
                "SetThing",
                Some("cap.software.demo.thing.write@v1"), // canonical, undeclared
                SideEffect::Mutation,
            ))
            .method(cap_method("ListTools", Some("demo.read"), SideEffect::Read)) // legacy
            .method(cap_method("Ping", None, SideEffect::Read)) // derived
            .build();

        let closure = schema.validate_capability_closure();
        assert_eq!(closure.declared, ["cap.software.demo.state.read@v1"]);
        assert_eq!(closure.missing, ["cap.software.demo.thing.write@v1"]);
        assert_eq!(closure.legacy, ["demo.read"]);
        assert_eq!(closure.derived, ["demo.read"]);
        assert!(!closure.ok(), "canonical-but-undeclared must fail");
    }

    #[test]
    fn fully_declared_schema_passes_closure() {
        let schema = PluginSchema::builder("demo")
            .capability(CapabilityDecl {
                id: "cap.software.demo.state.read@v1".to_string(),
                description: "read demo state".to_string(),
            })
            .method(cap_method(
                "GetState",
                Some("cap.software.demo.state.read@v1"),
                SideEffect::Read,
            ))
            .build();
        let closure = schema.validate_capability_closure();
        assert!(closure.ok());
        assert!(closure.legacy.is_empty() && closure.derived.is_empty());
    }

    #[test]
    fn derived_default_matches_side_effect() {
        let schema = PluginSchema::builder("demo")
            .method(cap_method("DoThing", None, SideEffect::Mutation))
            .build();
        let m = schema.methods.get("DoThing").unwrap();
        let r = schema.resolve_method_capability(m);
        assert_eq!(r.capability(), "demo.invoke");
        assert!(matches!(r, CapabilityResolution::Derived(_)));
    }

    // ====================================================================
    // WS1 — Capability Model tests (R1.1–R1.5, R2.1, R2.4, R11.1–R11.4)
    // ====================================================================

    #[test]
    fn method_decl_fields_complete() {
        // R1.2: MethodDecl MUST contain all seven fields
        let decl = MethodDecl {
            name: "SetKey".to_string(),
            args: json!({"type": "object"}),
            returns: Some(json!({"type": "string"})),
            side_effect: SideEffect::Mutation,
            idempotent: false,
            required_capability: Some("wireguard.admin".to_string()),
            subid: "mut.network.wireguard.set-key@v1".to_string(),
        };
        assert!(!decl.name.is_empty());
        assert!(decl.args.is_object());
        assert!(decl.returns.is_some());
        assert!(matches!(decl.side_effect, SideEffect::Mutation));
        assert!(!decl.idempotent);
        assert!(decl.required_capability.is_some());
        assert!(!decl.subid.is_empty());
    }

    #[test]
    fn signal_decl_fields_complete() {
        // R1.3: SignalDecl MUST contain name, payload, subid
        let sig = SignalDecl {
            name: "KeyRotated".to_string(),
            payload: Some(json!({"type": "string"})),
            subid: "evt.network.wireguard.key-rotated@v1".to_string(),
        };
        assert!(!sig.name.is_empty());
        assert!(sig.payload.is_some());
        assert!(!sig.subid.is_empty());
    }

    #[test]
    fn side_effect_serializes_snake_case() {
        // R1.2: side_effect serializes as "read" or "mutation"
        let read_json = serde_json::to_string(&SideEffect::Read).unwrap();
        assert_eq!(read_json, r#""read""#);
        let mut_json = serde_json::to_string(&SideEffect::Mutation).unwrap();
        assert_eq!(mut_json, r#""mutation""#);

        // Round-trip
        let read_back: SideEffect = serde_json::from_str(&read_json).unwrap();
        assert!(matches!(read_back, SideEffect::Read));
        let mut_back: SideEffect = serde_json::from_str(&mut_json).unwrap();
        assert!(matches!(mut_back, SideEffect::Mutation));
    }

    #[test]
    fn guarantees_has_four_bool_fields() {
        // R1.4 / VAL-CAP-004: exactly four boolean fields
        let caps = PluginCapabilities::default();
        assert!(!caps.supports_rollback);
        assert!(!caps.supports_checkpoints);
        assert!(!caps.supports_verification);
        assert!(!caps.atomic_operations);

        // Verify the serialized form has exactly four keys, all bool
        let json = serde_json::to_value(&caps).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 4);
        assert!(obj.contains_key("supports_rollback"));
        assert!(obj.contains_key("supports_checkpoints"));
        assert!(obj.contains_key("supports_verification"));
        assert!(obj.contains_key("atomic_operations"));
        for (_, v) in obj {
            assert_eq!(*v, serde_json::Value::Bool(false));
        }
    }

    #[test]
    fn schema_serializes_capability_keys() {
        // R1.5 / R11.1–R11.2: serialized PluginSchema always includes
        // methods, signals, guarantees keys with empty defaults
        let schema = PluginSchema::builder("test")
            .version("1.0.0")
            .description("Test")
            .string_field("name", true, "Name")
            .build();

        let json = serde_json::to_value(&schema).unwrap();
        let obj = json.as_object().unwrap();

        // methods → empty object
        assert!(obj.contains_key("methods"));
        assert!(obj["methods"].is_object());
        assert!(obj["methods"].as_object().unwrap().is_empty());

        // signals → empty array
        assert!(obj.contains_key("signals"));
        assert!(obj["signals"].is_array());
        assert!(obj["signals"].as_array().unwrap().is_empty());

        // guarantees → full four-key block, all false
        assert!(obj.contains_key("guarantees"));
        let guar = obj["guarantees"].as_object().unwrap();
        assert_eq!(guar.len(), 4);
        assert_eq!(guar["supports_rollback"], serde_json::json!(false));
        assert_eq!(guar["supports_checkpoints"], serde_json::json!(false));
        assert_eq!(guar["supports_verification"], serde_json::json!(false));
        assert_eq!(guar["atomic_operations"], serde_json::json!(false));
    }

    #[test]
    fn schema_round_trip_preserves_capability_surface() {
        // R11.4: round-trip serialize/deserialize preserves full capability surface
        let mut methods = std::collections::HashMap::new();
        methods.insert(
            "SetKey".to_string(),
            MethodDecl {
                name: "SetKey".to_string(),
                args: json!({
                    "type": "object",
                    "properties": {
                        "key": {"type": "string"}
                    },
                    "required": ["key"]
                }),
                returns: Some(json!({"type": "string"})),
                side_effect: SideEffect::Mutation,
                idempotent: false,
                required_capability: Some("wireguard.admin".to_string()),
                subid: "mut.network.wireguard.set-key@v1".to_string(),
            },
        );
        methods.insert(
            "GetStatus".to_string(),
            MethodDecl {
                name: "GetStatus".to_string(),
                args: json!({"type": "object"}),
                returns: Some(json!({"type": "string"})),
                side_effect: SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "obs.network.wireguard.get-status@v1".to_string(),
            },
        );

        let signals = vec![SignalDecl {
            name: "KeyRotated".to_string(),
            payload: Some(json!({"type": "string"})),
            subid: "evt.network.wireguard.key-rotated@v1".to_string(),
        }];

        let guarantees = PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        };

        let original = PluginSchema::builder("wireguard")
            .version("2.0.0")
            .description("WireGuard interface state")
            .string_field("interfaces", true, "Interfaces")
            .build()
            .with_methods(methods)
            .with_signals(signals)
            .with_guarantees(guarantees);

        // Serialize
        let json_str = serde_json::to_string(&original).unwrap();

        // Deserialize
        let restored: PluginSchema = serde_json::from_str(&json_str).unwrap();

        // Verify capability surface preserved
        assert_eq!(restored.methods.len(), 2);
        let set_key = restored.methods.get("SetKey").unwrap();
        assert_eq!(set_key.name, "SetKey");
        assert!(matches!(set_key.side_effect, SideEffect::Mutation));
        assert!(!set_key.idempotent);
        assert_eq!(
            set_key.required_capability.as_deref(),
            Some("wireguard.admin")
        );
        assert_eq!(set_key.subid, "mut.network.wireguard.set-key@v1");

        let get_status = restored.methods.get("GetStatus").unwrap();
        assert!(matches!(get_status.side_effect, SideEffect::Read));
        assert!(get_status.idempotent);
        assert!(get_status.required_capability.is_none());

        assert_eq!(restored.signals.len(), 1);
        assert_eq!(restored.signals[0].name, "KeyRotated");
        assert_eq!(
            restored.signals[0].subid,
            "evt.network.wireguard.key-rotated@v1"
        );

        assert!(restored.guarantees.supports_rollback);
        assert!(!restored.guarantees.supports_checkpoints);
        assert!(restored.guarantees.supports_verification);
        assert!(!restored.guarantees.atomic_operations);
    }

    #[test]
    fn schema_default_deserializes_without_capability_keys() {
        // R11.2: backward-compatible deserialization — a JSON without
        // methods/signals/guarantees keys should still deserialize,
        // filling in empty defaults.
        let legacy_json = r#"{
            "name": "legacy",
            "category": "uncategorized",
            "version": "1.0.0",
            "description": "Legacy schema without capability fields",
            "fields": {}
        }"#;

        let schema: PluginSchema = serde_json::from_str(legacy_json).unwrap();
        assert!(schema.methods.is_empty());
        assert!(schema.signals.is_empty());
        assert!(!schema.guarantees.supports_rollback);
    }

    // ====================================================================
    // Existing tests
    // ====================================================================

    #[test]
    fn test_schema_catalog_registration() {
        let mut catalog = SchemaCatalog::empty();
        let schema = PluginSchema::builder("fixture")
            .version("1.0.0")
            .description("fixture schema")
            .string_field("id", true, "Identifier")
            .build();
        catalog.register(schema);
        assert!(catalog.get("fixture").is_some());
        assert_eq!(catalog.list(), vec!["fixture"]);
    }

    #[test]
    fn test_schema_catalog_aliases_are_canonicalized() {
        let mut catalog = SchemaCatalog::empty();
        catalog.register(
            PluginSchema::builder("net")
                .version("1.0.0")
                .description("fixture")
                .build(),
        );
        // The catalog remains keyed by the canonical plugin document name.
        assert!(std::ptr::eq(
            catalog.get("net").unwrap(),
            catalog.get("network").unwrap()
        ));
        assert_eq!(catalog.list(), vec!["net"]);
    }

    #[test]
    fn test_schema_registry_compatibility_type_is_empty_by_default() {
        let registry = SchemaRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_fixture_validation() {
        let mut registry = SchemaRegistry::new();
        registry.register(
            PluginSchema::builder("fixture")
                .version("1.0.0")
                .description("fixture")
                .string_field("id", true, "Identifier")
                .build(),
        );
        let schema = registry.get("fixture").unwrap();
        assert!(schema.validate(&json!({"id": "ok"})).valid);
        assert!(!schema.validate(&json!({})).valid);
    }

    #[test]
    fn test_template_generation_from_registered_schema() {
        let schema = PluginSchema::builder("fixture")
            .version("1.0.0")
            .description("fixture")
            .string_field("id", true, "Identifier")
            .build();
        let template = schema.generate_template();
        assert!(template.get("id").is_some());
    }

    #[test]
    fn test_json_schema_export_from_registered_schema() {
        let schema = PluginSchema::builder("fixture")
            .version("1.0.0")
            .description("fixture")
            .string_field("id", true, "Identifier")
            .build();
        let json_schema = schema.to_json_schema();
        assert_eq!(json_schema["title"], "fixture");
        assert!(json_schema["properties"].is_object());
    }

    #[test]
    fn test_json_schema_2026_dialect_from_registered_schema() {
        let schema = PluginSchema::builder("fixture")
            .version("1.0.0")
            .description("fixture")
            .build();
        assert_eq!(schema.to_json_schema()["$schema"], DEFAULT_SCHEMA_DIALECT);
    }

    #[test]
    fn test_json_schema_property_dependencies() {
        // Create a schema with conditional readOnly
        let schema = PluginSchema::builder("test")
            .version("1.0.0")
            .description("Test schema")
            .field(
                "status",
                FieldSchema {
                    field_type: FieldType::String,
                    required: true,
                    description: "Status".to_string(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: false,
                    read_only_when: None,
                },
            )
            .field(
                "id",
                FieldSchema {
                    field_type: FieldType::String,
                    required: true,
                    description: "ID".to_string(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: false,
                    read_only_when: Some(ReadOnlyCondition {
                        property: "status".to_string(),
                        value: "locked".to_string(),
                    }),
                },
            )
            .build();

        let json_schema = schema.to_json_schema();

        // Check that propertyDependencies is generated
        assert!(json_schema.get("propertyDependencies").is_some());
        let deps = &json_schema["propertyDependencies"];
        assert!(
            deps["status"]["locked"]["properties"]["id"]["readOnly"]
                .as_bool()
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_json_schema_immutable_paths() {
        let schema = PluginSchema::builder("test")
            .version("1.0.0")
            .description("Test schema")
            .string_field("id", true, "ID field")
            .string_field("name", true, "Name field")
            .immutable_path("/id")
            .build();

        let json_schema = schema.to_json_schema();
        let properties = json_schema["properties"].as_object().unwrap();

        // Check that id has readOnly
        assert!(properties["id"]["readOnly"].as_bool().unwrap_or(false));
        // name should not be readOnly
        assert!(
            !properties
                .get("name")
                .and_then(|value| value.get("readOnly"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_json_schema_fully_immutable() {
        let schema = PluginSchema::builder("test")
            .version("1.0.0")
            .description("Test schema")
            .string_field("id", true, "ID field")
            .string_field("name", true, "Name field")
            .fully_immutable()
            .build();

        let json_schema = schema.to_json_schema();

        // All fields should be readOnly
        assert!(
            json_schema["properties"]["id"]["readOnly"]
                .as_bool()
                .unwrap_or(false)
        );
        assert!(
            json_schema["properties"]["name"]["readOnly"]
                .as_bool()
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_schema_custom_dialect() {
        let schema = PluginSchema::builder("test")
            .version("1.0.0")
            .description("Test schema")
            .dialect(dialects::DRAFT_07)
            .string_field("name", true, "Name")
            .build();

        let json_schema = schema.to_json_schema();
        assert_eq!(json_schema["$schema"], dialects::DRAFT_07);
    }
}

#[cfg(test)]
mod typed_map_tests {
    use super::*;
    fn schema() -> PluginSchema {
        let leaf = PluginSchema::builder("leaf")
            .boolean_field("ready", true, "")
            .build();
        let value = PluginSchema::builder("value")
            .field(
                "children",
                FieldSchema {
                    field_type: FieldType::Map(Box::new(FieldType::Object(leaf.fields))),
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: vec![],
                    read_only: false,
                    read_only_when: None,
                },
            )
            .build();
        PluginSchema::builder("map_fixture")
            .field(
                "jobs",
                FieldSchema {
                    field_type: FieldType::Map(Box::new(FieldType::Object(value.fields))),
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: vec![],
                    read_only: false,
                    read_only_when: None,
                },
            )
            .build()
    }
    #[test]
    fn map_roundtrip_preserves_children_grandchildren_and_object_wire_shape() {
        let schema = schema();
        let encoded = serde_json::to_value(&schema).unwrap();
        assert!(
            encoded["fields"]["jobs"]["field_type"]["map"]["object"]["children"]["field_type"]
                ["map"]["object"]["ready"]
                .is_object()
        );
        let decoded: PluginSchema = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.fields, schema.fields);
        assert!(
            decoded
                .validate(
                    &json!({"jobs":{"stable_job":{"children":{"stable_child":{"ready":false}}}}})
                )
                .valid
        );
        assert!(
            !decoded
                .validate(
                    &json!({"jobs":{"stable_job":{"children":{"stable_child":{"ready":"wrong"}}}}})
                )
                .valid
        );
        assert!(
            !decoded
                .validate(&json!({"jobs":{"stable_job":{"children":{"stable_child":{}}}}}))
                .valid
        );
        let errors = decoded
            .validate(&json!({"jobs":{"PRIVATE_KEY":{"children":false}}}))
            .errors;
        assert!(!format!("{errors:?}").contains("PRIVATE_KEY"));
        let exported = serde_json::to_value(decoded.to_json_schema()).unwrap();
        assert_eq!(
            exported["properties"]["jobs"]["additionalProperties"]["properties"]["children"]["additionalProperties"]
                ["properties"]["ready"]["type"],
            "boolean"
        );
        assert_eq!(
            serde_json::to_value(FieldType::Object(HashMap::new())).unwrap(),
            serde_json::json!({"object":{}})
        );
    }
}
