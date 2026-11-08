use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT_VERSION: &str = "3tchedfs/1";

/// Explicitly distinguish a complete state from its least-privilege projection.
/// Legacy objects decode as Full and must be sanitized before public rendering.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateValueScope {
    #[default]
    Full,
    PublicProjection,
    /// Boot-time catalog/schema capture, with no claimed state value.
    SchemaOnly,
}

impl StateValueScope {
    fn is_full(&self) -> bool {
        *self == Self::Full
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypedObjectBlob {
    pub format: String,
    pub plugin_id: String,
    pub source_schema_hash: String,
    pub schema: PluginSchema,
    pub value: Value,
    #[serde(default, skip_serializing_if = "StateValueScope::is_full")]
    pub value_scope: StateValueScope,
}

impl TypedObjectBlob {
    pub fn new(
        plugin_id: String,
        source_schema_hash: String,
        schema: PluginSchema,
        value: Value,
    ) -> Result<Self> {
        let blob = Self {
            format: FORMAT_VERSION.to_string(),
            plugin_id,
            source_schema_hash,
            schema,
            value,
            value_scope: StateValueScope::Full,
        };
        blob.validate(None)?;
        Ok(blob)
    }

    pub fn new_public(
        plugin_id: String,
        source_schema_hash: String,
        schema: PluginSchema,
        value: Value,
    ) -> Result<Self> {
        let blob = Self {
            format: FORMAT_VERSION.to_owned(),
            plugin_id,
            source_schema_hash,
            schema,
            value,
            value_scope: StateValueScope::PublicProjection,
        };
        blob.validate(None)?;
        Ok(blob)
    }

    pub fn schema_only(
        plugin_id: String,
        source_schema_hash: String,
        schema: PluginSchema,
    ) -> Result<Self> {
        let blob = Self {
            format: FORMAT_VERSION.to_owned(),
            plugin_id,
            source_schema_hash,
            schema,
            value: Value::Null,
            value_scope: StateValueScope::SchemaOnly,
        };
        blob.validate(None)?;
        Ok(blob)
    }

    /// Freshly sanitize legacy complete snapshots before any public rendering.
    /// Schema and operation declarations remain intact; only state is filtered.
    pub fn public_projection(&self) -> Result<Self> {
        self.validate(None)?;
        if self.value_scope == StateValueScope::SchemaOnly {
            return Ok(self.clone());
        }
        let value = match self.value_scope {
            StateValueScope::Full => self.schema.project_public_value(&self.value)?,
            StateValueScope::PublicProjection => self.value.clone(),
            StateValueScope::SchemaOnly => unreachable!("handled above"),
        };
        Self::new_public(
            self.plugin_id.clone(),
            self.source_schema_hash.clone(),
            self.schema.clone(),
            value,
        )
    }

    pub fn validate(&self, base: Option<&Value>) -> Result<()> {
        if self.format != FORMAT_VERSION {
            bail!("unsupported typed object format '{}'", self.format);
        }
        if self.schema.name != self.plugin_id {
            bail!(
                "schema name '{}' does not match plugin id '{}'",
                self.schema.name,
                self.plugin_id
            );
        }
        match self.value_scope {
            StateValueScope::Full => self.schema.validate_value(&self.value, base),
            StateValueScope::SchemaOnly => {
                if !self.value.is_null() || base.is_some() {
                    bail!("schema-only object has no readable or writable state value");
                }
                self.schema.sealed()?.validate_public_state_policy()?;
                Ok(())
            }
            StateValueScope::PublicProjection => {
                self.schema.validate_public_value(&self.value)?;
                if let Some(base) = base {
                    validate_projected_constraints(
                        &self.schema.fields,
                        &self.value,
                        &self.value,
                        Some(base),
                    )?;
                    self.schema.validate_immutability(&self.value, base)?;
                }
                Ok(())
            }
        }
    }
}

/// 3tchedFS's typed view of a sealed plugin schema. The sealed JSON is decoded
/// by its authority, `op_state_store::PluginSchema`, and converted here by
/// [`from_sealed`]; this view only adds deterministic key order and
/// `serde_json` values for projection and validation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginSchema {
    pub name: String,
    #[serde(default)]
    pub category: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSchema>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub immutable_paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dialect: String,
    #[serde(default)]
    pub mutation_index: Option<u64>,
    #[serde(default)]
    pub subids: BTreeMap<String, String>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub methods: BTreeMap<String, MethodDecl>,
    #[serde(default)]
    pub signals: Vec<SignalDecl>,
    #[serde(default)]
    pub guarantees: PluginGuarantees,
    /// Declared capabilities, keyed by capability id.
    #[serde(default)]
    pub capabilities: BTreeMap<String, Value>,
    /// Exact per-footprint grant annotations carried by older schemas.
    #[serde(default)]
    pub capability_grants: BTreeMap<String, Vec<String>>,
    /// Which state fields a public projection may expose.
    #[serde(default)]
    pub public_state: Value,
    /// Field write routes, keyed by field.
    #[serde(default)]
    pub field_writes: BTreeMap<String, Value>,
    /// An example of the plugin's state.
    #[serde(default)]
    pub example: Option<Value>,
}

impl PluginSchema {
    fn sealed(&self) -> Result<op_state_store::PluginSchema> {
        let mut value = serde_json::to_value(self)?;
        if value["public_state"].is_null() {
            value["public_state"] = serde_json::json!({"paths": []});
        }
        serde_json::from_value(value)
            .context("restore canonical sealed schema for public projection")
    }

    /// Validate complete committed state before deriving its public view.
    pub fn project_public_value(&self, root: &Value) -> Result<Value> {
        self.validate_value(root, None).map_err(|_| anyhow::anyhow!("committed state does not satisfy the complete sealed schema; public projection unavailable"))?;
        let sealed = self.sealed()?;
        sealed.validate_public_state_policy()?;
        let raw = serde_json::from_value(root.clone())?;
        let projected = sealed.project_public_state(&raw)?;
        Ok(projected
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map(|value| value.unwrap_or_else(|| serde_json::json!({})))?)
    }

    /// Validate the public shape against the sealed allowlist, not against the
    /// complete state's required private fields. Re-projection checks canonical
    /// selected types and rejects all extra or private members. Complete-state
    /// constraints are checked before filtering; a partial value is not evidence
    /// that a private peer-field condition still holds in current raw state.
    pub fn validate_public_value(&self, root: &Value) -> Result<()> {
        let sealed = self.sealed()?;
        sealed.validate_public_state_policy()?;
        let raw = serde_json::from_value(root.clone())?;
        let projected = sealed.project_public_state(&raw)?;
        let expected = projected
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        if expected != *root {
            bail!("value contains state outside the sealed public projection");
        }
        for pointer in &sealed.public_state.paths {
            let components = pointer[1..]
                .split('/')
                .map(|part| part.replace("~1", "/").replace("~0", "~"))
                .collect::<Vec<_>>();
            validate_required_public_path(
                &FieldType::Object(self.fields.clone()),
                &components,
                root,
            )?;
        }
        validate_projected_constraints(&self.fields, root, root, None)?;
        Ok(())
    }

    pub fn validate_value(&self, root: &Value, base: Option<&Value>) -> Result<()> {
        let object = root
            .as_object()
            .with_context(|| format!("{} state must be an object", self.name))?;

        for (name, field) in &self.fields {
            match object.get(name) {
                Some(value) => field
                    .validate(value, root)
                    .with_context(|| format!("field /{name}"))?,
                None if field.required => bail!("required field /{name} is missing"),
                None => {}
            }
        }

        if let Some(base) = base {
            self.validate_immutability(root, base)?;
        }

        Ok(())
    }

    fn validate_immutability(&self, root: &Value, base: &Value) -> Result<()> {
        let object = root.as_object().context("state must be an object")?;
        let base_object = base.as_object().context("base state must be an object")?;
        validate_object_immutability(&self.fields, object, base_object, "")?;
        for pointer in &self.immutable_paths {
            if root.pointer(pointer) != base.pointer(pointer) {
                bail!("immutable path {pointer} changed");
            }
        }
        if self.tags.iter().any(|tag| tag == "immutable") && root != base {
            bail!("plugin {} is immutable", self.name);
        }
        Ok(())
    }

    /// The capabilities that at least one method requires.
    pub fn required_capabilities(&self) -> BTreeSet<&str> {
        self.methods
            .values()
            .filter_map(|method| method.required_capability.as_deref())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldSchema {
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub example: Option<Value>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub read_only_when: Option<ReadOnlyCondition>,
}

impl FieldSchema {
    pub fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        // PluginSchema represents `Option<T>` as a non-required T while live
        // state may preserve an explicit JSON null for that option.
        if value.is_null() && !self.required {
            return Ok(());
        }
        self.field_type.validate(value, root)?;
        for constraint in &self.constraints {
            constraint.validate(value, root)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn template(&self) -> Value {
        self.default
            .clone()
            .or_else(|| self.example.clone())
            .unwrap_or_else(|| self.field_type.template())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Array(Box<FieldType>),
    Object(BTreeMap<String, FieldSchema>),
    /// Dynamic string keys with one declared value type. Wire form matches
    /// op-state-store: `{"map": <FieldType>}`.
    Map(Box<FieldType>),
    /// Explicit exact JSON Schema document retained from the sealed plugin.
    /// Wire form matches op-state-store: `{"json_schema": <document>}`.
    JsonSchema(Box<Value>),
    Enum(Vec<String>),
    OneOf(Vec<FieldType>),
    Any,
}

impl FieldType {
    pub fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        match self {
            Self::String if !value.is_string() => bail!("expected string"),
            Self::Integer if !value.is_i64() && !value.is_u64() => bail!("expected integer"),
            Self::Float if !value.is_number() => bail!("expected number"),
            Self::Boolean if !value.is_boolean() => bail!("expected boolean"),
            Self::Array(item_type) => {
                let values = value.as_array().context("expected array")?;
                for (index, value) in values.iter().enumerate() {
                    item_type
                        .validate(value, root)
                        .with_context(|| format!("array element {index}"))?;
                }
            }
            Self::Object(fields) => {
                let object = value.as_object().context("expected object")?;
                for (name, field) in fields {
                    match object.get(name) {
                        Some(value) => field
                            .validate(value, root)
                            .with_context(|| format!("object field {name}"))?,
                        None if field.required => bail!("required object field {name} is missing"),
                        None => {}
                    }
                }
            }
            Self::Map(item_type) => {
                let object = value.as_object().context("expected object map")?;
                for (index, value) in object.values().enumerate() {
                    // Dynamic keys may be private; keep errors key-agnostic.
                    item_type
                        .validate(value, root)
                        .with_context(|| format!("map value {index}"))?;
                }
            }
            Self::JsonSchema(schema) => {
                jsonschema::Validator::new(schema)
                    .context("compile embedded json_schema field")?
                    .validate(value)
                    .map_err(|error| anyhow::anyhow!("value failed json_schema: {error}"))?;
            }
            Self::Enum(variants) => {
                let value = value.as_str().context("expected enum string")?;
                if !variants.iter().any(|variant| variant == value) {
                    bail!("'{value}' is not an allowed enum value");
                }
            }
            Self::OneOf(branches) => {
                let matches = branches
                    .iter()
                    .filter(|branch| branch.validate(value, root).is_ok())
                    .count();
                if matches != 1 {
                    bail!("value must match exactly one union branch; matched {matches}");
                }
            }
            Self::Any | Self::String | Self::Integer | Self::Float | Self::Boolean => {}
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn template(&self) -> Value {
        match self {
            Self::String => Value::String(String::new()),
            Self::Integer => Value::from(0),
            Self::Float => Value::from(0.0),
            Self::Boolean => Value::Bool(false),
            Self::Array(_) => Value::Array(Vec::new()),
            Self::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.template()))
                    .collect(),
            ),
            Self::Map(_) => Value::Object(serde_json::Map::new()),
            Self::JsonSchema(schema) => json_schema_template(schema),
            Self::Any => Value::Null,
            Self::Enum(values) => values
                .first()
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null),
            Self::OneOf(branches) => branches.first().map(Self::template).unwrap_or(Value::Null),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadOnlyCondition {
    pub property: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    Min { value: f64 },
    Max { value: f64 },
    Pattern { regex: String },
    OneOf { values: Vec<Value> },
    RequiresField { field: String },
    Custom { validator: String },
}

impl Constraint {
    fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        match self {
            Self::Min { value: minimum } => {
                let actual = measure(value).context("min constraint requires number or length")?;
                if actual < *minimum {
                    bail!("{actual} is below minimum {minimum}");
                }
            }
            Self::Max { value: maximum } => {
                let actual = measure(value).context("max constraint requires number or length")?;
                if actual > *maximum {
                    bail!("{actual} exceeds maximum {maximum}");
                }
            }
            Self::Pattern { regex } => {
                let actual = value
                    .as_str()
                    .context("pattern constraint requires string")?;
                if !Regex::new(regex)
                    .with_context(|| format!("invalid schema regex '{regex}'"))?
                    .is_match(actual)
                {
                    bail!("value does not match pattern '{regex}'");
                }
            }
            Self::OneOf { values } if !values.contains(value) => {
                bail!("value is not in the allowed set");
            }
            Self::RequiresField { field } if root.get(field).is_none() => {
                bail!("required peer field '{field}' is missing");
            }
            Self::Custom { validator } => {
                bail!(
                    "custom validator '{validator}' cannot be executed by this filesystem; validation is unavailable"
                );
            }
            Self::OneOf { .. } | Self::RequiresField { .. } => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MethodDecl {
    pub name: String,
    pub args: Value,
    #[serde(default)]
    pub returns: Option<Value>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    #[serde(default)]
    pub required_capability: Option<String>,
    pub subid: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalDecl {
    pub name: String,
    #[serde(default)]
    pub payload: Option<Value>,
    pub subid: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginGuarantees {
    #[serde(default)]
    pub supports_rollback: bool,
    #[serde(default)]
    pub supports_checkpoints: bool,
    #[serde(default)]
    pub supports_verification: bool,
    #[serde(default)]
    pub atomic_operations: bool,
}

// Conversion from the sealed authority.
//
// `op_state_store::PluginSchema` decodes the sealed JSON; the functions below
// turn it into this crate's view. Every struct is destructured by name and
// every enum is matched without a wildcard, so when the authority gains a
// field or a field type this crate stops compiling until it handles it. That
// is how a new sealed field type can no longer break capture at run time.

/// Convert a schema decoded by its authority into this crate's view.
pub fn from_sealed(sealed: op_state_store::PluginSchema) -> Result<PluginSchema> {
    let op_state_store::PluginSchema {
        name,
        category,
        version,
        description,
        display_name,
        fields,
        public_state,
        field_writes,
        dependencies,
        example,
        immutable_paths,
        tags,
        dialect,
        mutation_index,
        subids,
        org,
        methods,
        capabilities,
        capability_grants,
        signals,
        guarantees,
    } = sealed;
    let op_state_store::PluginCapabilities {
        supports_rollback,
        supports_checkpoints,
        supports_verification,
        atomic_operations,
    } = guarantees;
    Ok(PluginSchema {
        name,
        category,
        version,
        description,
        display_name,
        fields: sealed_fields(fields)?,
        dependencies,
        immutable_paths,
        tags,
        dialect,
        mutation_index,
        subids: subids.into_iter().collect(),
        org,
        methods: methods
            .into_iter()
            .map(|(name, method)| Ok((name, sealed_method(method)?)))
            .collect::<Result<_>>()?,
        signals: signals
            .into_iter()
            .map(sealed_signal)
            .collect::<Result<_>>()?,
        guarantees: PluginGuarantees {
            supports_rollback,
            supports_checkpoints,
            supports_verification,
            atomic_operations,
        },
        capabilities: capabilities
            .iter()
            .map(|(id, declaration)| Ok((id.clone(), to_json(declaration)?)))
            .collect::<Result<_>>()?,
        capability_grants: capability_grants.into_iter().collect(),
        public_state: to_json(&public_state)?,
        field_writes: field_writes
            .iter()
            .map(|(field, route)| Ok((field.clone(), to_json(route)?)))
            .collect::<Result<_>>()?,
        example: example.as_ref().map(to_json).transpose()?,
    })
}

fn sealed_fields(
    fields: std::collections::HashMap<String, op_state_store::FieldSchema>,
) -> Result<BTreeMap<String, FieldSchema>> {
    fields
        .into_iter()
        .map(|(name, field)| Ok((name, sealed_field(field)?)))
        .collect()
}

fn sealed_field(sealed: op_state_store::FieldSchema) -> Result<FieldSchema> {
    let op_state_store::FieldSchema {
        field_type,
        required,
        description,
        default,
        example,
        constraints,
        read_only,
        read_only_when,
    } = sealed;
    Ok(FieldSchema {
        field_type: sealed_field_type(field_type)?,
        required,
        description,
        default: default.as_ref().map(to_json).transpose()?,
        example: example.as_ref().map(to_json).transpose()?,
        constraints: constraints
            .into_iter()
            .map(sealed_constraint)
            .collect::<Result<_>>()?,
        read_only,
        read_only_when: read_only_when.map(
            |op_state_store::ReadOnlyCondition { property, value }| ReadOnlyCondition {
                property,
                value,
            },
        ),
    })
}

fn sealed_field_type(sealed: op_state_store::FieldType) -> Result<FieldType> {
    use op_state_store::FieldType as Sealed;
    Ok(match sealed {
        Sealed::String => FieldType::String,
        Sealed::Integer => FieldType::Integer,
        Sealed::Float => FieldType::Float,
        Sealed::Boolean => FieldType::Boolean,
        Sealed::Array(item) => FieldType::Array(Box::new(sealed_field_type(*item)?)),
        Sealed::Object(fields) => FieldType::Object(sealed_fields(fields)?),
        Sealed::Map(item) => FieldType::Map(Box::new(sealed_field_type(*item)?)),
        Sealed::JsonSchema(document) => FieldType::JsonSchema(Box::new(to_json(&*document)?)),
        Sealed::Enum(values) => FieldType::Enum(values),
        Sealed::OneOf(branches) => FieldType::OneOf(
            branches
                .into_iter()
                .map(sealed_field_type)
                .collect::<Result<_>>()?,
        ),
        Sealed::Any => FieldType::Any,
    })
}

fn sealed_constraint(sealed: op_state_store::Constraint) -> Result<Constraint> {
    use op_state_store::Constraint as Sealed;
    Ok(match sealed {
        Sealed::Min { value } => Constraint::Min { value },
        Sealed::Max { value } => Constraint::Max { value },
        Sealed::Pattern { regex } => Constraint::Pattern { regex },
        Sealed::OneOf { values } => Constraint::OneOf {
            values: values.iter().map(to_json).collect::<Result<_>>()?,
        },
        Sealed::RequiresField { field } => Constraint::RequiresField { field },
        Sealed::Custom { validator } => Constraint::Custom { validator },
    })
}

fn sealed_method(sealed: op_state_store::MethodDecl) -> Result<MethodDecl> {
    let op_state_store::MethodDecl {
        name,
        args,
        returns,
        side_effect,
        idempotent,
        required_capability,
        subid,
    } = sealed;
    Ok(MethodDecl {
        name,
        args: to_json(&args)?,
        returns: returns.as_ref().map(to_json).transpose()?,
        side_effect: match side_effect {
            op_state_store::SideEffect::Read => SideEffect::Read,
            op_state_store::SideEffect::Mutation => SideEffect::Mutation,
        },
        idempotent,
        required_capability,
        subid,
    })
}

fn sealed_signal(sealed: op_state_store::SignalDecl) -> Result<SignalDecl> {
    let op_state_store::SignalDecl {
        name,
        payload,
        subid,
    } = sealed;
    Ok(SignalDecl {
        name,
        payload: payload.as_ref().map(to_json).transpose()?,
        subid,
    })
}

/// The authority embeds `simd_json` values; this view uses `serde_json`.
fn to_json(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value).context("convert sealed value to JSON")
}

fn measure(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().map(|value| value.chars().count() as f64))
        .or_else(|| value.as_array().map(|value| value.len() as f64))
        .or_else(|| value.as_object().map(|value| value.len() as f64))
}

fn json_scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

// Policy syntax and path admissibility have already been checked by the
// canonical authority. This additional walk enforces only required members
// along selected paths, without imposing required private siblings.
fn validate_required_public_path(
    field_type: &FieldType,
    components: &[String],
    value: &Value,
) -> Result<()> {
    let Some((head, tail)) = components.split_first() else {
        return Ok(());
    };
    match field_type {
        FieldType::Object(fields) => {
            let field = fields
                .get(head)
                .context("public path field is undeclared")?;
            let Some(child) = value.get(head) else {
                if field.required {
                    bail!("required public field '{head}' is missing");
                }
                return Ok(());
            };
            if child.is_null() && !field.required {
                return Ok(());
            }
            validate_required_public_path(&field.field_type, tail, child)?;
        }
        FieldType::Array(item) => {
            for child in value
                .as_array()
                .context("public path array has incorrect type")?
            {
                validate_required_public_path(item, tail, child)?;
            }
        }
        FieldType::Map(item) => {
            if let Some(child) = value.get(head) {
                validate_required_public_path(item, tail, child)?;
            }
        }
        _ => bail!("public path traverses a non-container"),
    }
    Ok(())
}

// Public shape validation must not demand omitted private required members.
// Value-local constraints still apply. A private cross-field precondition is
// validated on the full raw state before capture, but cannot be re-proven from
// a projected snapshot: changing its dependent field fails closed.
fn validate_projected_constraints(
    fields: &BTreeMap<String, FieldSchema>,
    value: &Value,
    root: &Value,
    base: Option<&Value>,
) -> Result<()> {
    let object = value
        .as_object()
        .context("public state must be an object")?;
    for (name, field) in fields {
        let Some(value) = object.get(name) else {
            continue;
        };
        if value.is_null() && !field.required {
            continue;
        }
        let previous = base.and_then(|base| base.get(name));
        let changed = base.is_some() && previous != Some(value);
        if changed
            && let Some(condition) = &field.read_only_when
            && base
                .and_then(|base| base.get(&condition.property))
                .is_none()
        {
            bail!("cannot determine private read-only condition for public field '{name}'");
        }
        for constraint in &field.constraints {
            if let Constraint::RequiresField { field: peer } = constraint
                && root.get(peer).is_none()
            {
                if changed {
                    bail!(
                        "cannot verify private peer-field condition '{peer}' for changed public field '{name}'"
                    );
                }
                continue;
            }
            constraint.validate(value, root)?;
        }
        let absent = Value::Null;
        let next_base = base.map(|_| previous.unwrap_or(&absent));
        validate_projected_type_constraints(&field.field_type, value, root, next_base)?;
    }
    Ok(())
}

fn validate_projected_type_constraints(
    field_type: &FieldType,
    value: &Value,
    root: &Value,
    base: Option<&Value>,
) -> Result<()> {
    match field_type {
        FieldType::Object(fields) => validate_projected_constraints(fields, value, root, base)?,
        FieldType::Array(item) => {
            for (index, value) in value
                .as_array()
                .context("public array has incorrect type")?
                .iter()
                .enumerate()
            {
                let absent = Value::Null;
                validate_projected_type_constraints(
                    item,
                    value,
                    root,
                    base.map(|base| base.get(index).unwrap_or(&absent)),
                )?;
            }
        }
        FieldType::Map(item) => {
            for (key, value) in value.as_object().context("public map has incorrect type")? {
                let absent = Value::Null;
                validate_projected_type_constraints(
                    item,
                    value,
                    root,
                    base.map(|base| base.get(key).unwrap_or(&absent)),
                )?;
            }
        }
        FieldType::OneOf(branches) => {
            field_type.validate(value, root)?;
            let branch = branches
                .iter()
                .find(|branch| branch.validate(value, root).is_ok())
                .context("public union has no branch")?;
            validate_projected_type_constraints(branch, value, root, base)?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_object_immutability(
    fields: &BTreeMap<String, FieldSchema>,
    current: &serde_json::Map<String, Value>,
    base: &serde_json::Map<String, Value>,
    parent_pointer: &str,
) -> Result<()> {
    for (name, field) in fields {
        let pointer = push_pointer(parent_pointer, name);
        let current_value = current.get(name);
        let base_value = base.get(name);
        if field.read_only && current_value != base_value {
            bail!("read-only field {pointer} changed");
        }
        if let Some(condition) = &field.read_only_when {
            let trigger = base
                .get(&condition.property)
                .map(json_scalar_text)
                .as_deref()
                == Some(condition.value.as_str());
            if trigger && current_value != base_value {
                bail!("conditionally read-only field {pointer} changed");
            }
        }
        if let (Some(current_value), Some(base_value)) = (current_value, base_value) {
            validate_type_immutability(&field.field_type, current_value, base_value, &pointer)?;
        }
    }
    Ok(())
}

fn validate_type_immutability(
    field_type: &FieldType,
    current: &Value,
    base: &Value,
    pointer: &str,
) -> Result<()> {
    match field_type {
        FieldType::Object(fields) => {
            if let (Some(current), Some(base)) = (current.as_object(), base.as_object()) {
                validate_object_immutability(fields, current, base, pointer)?;
            }
        }
        FieldType::Map(item_type) => {
            if let (Some(current), Some(base)) = (current.as_object(), base.as_object()) {
                for (key, current_value) in current {
                    if let Some(base_value) = base.get(key) {
                        validate_type_immutability(
                            item_type,
                            current_value,
                            base_value,
                            &push_pointer(pointer, key),
                        )?;
                    }
                }
            }
        }
        FieldType::Array(item_type) => {
            if let (Some(current), Some(base)) = (current.as_array(), base.as_array()) {
                for (index, (current, base)) in current.iter().zip(base).enumerate() {
                    validate_type_immutability(
                        item_type,
                        current,
                        base,
                        &push_pointer(pointer, &index.to_string()),
                    )?;
                }
            }
        }
        FieldType::OneOf(branches) => {
            for branch in branches {
                if branch.validate(current, current).is_ok() && branch.validate(base, base).is_ok()
                {
                    validate_type_immutability(branch, current, base, pointer)?;
                }
            }
        }
        FieldType::String
        | FieldType::Integer
        | FieldType::Float
        | FieldType::Boolean
        | FieldType::Enum(_)
        | FieldType::JsonSchema(_)
        | FieldType::Any => {}
    }
    Ok(())
}

fn push_pointer(parent: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

/// A schema-template utility. This is never a committed-state fallback.
#[cfg(test)]
fn json_schema_template(schema: &Value) -> Value {
    if let Some(const_value) = schema.get("const") {
        return const_value.clone();
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        return first.clone();
    }
    match json_schema_type_name(schema) {
        Some("object") => {
            let mut object = serde_json::Map::new();
            let properties = schema.get("properties").and_then(Value::as_object);
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    let child = properties
                        .and_then(|fields| fields.get(name))
                        .unwrap_or(&Value::Null);
                    object.insert(name.to_string(), json_schema_template(child));
                }
            }
            Value::Object(object)
        }
        Some("array") => Value::Array(Vec::new()),
        Some("string") => Value::String(String::new()),
        Some("integer") => schema
            .get("minimum")
            .and_then(Value::as_i64)
            .map(Value::from)
            .unwrap_or_else(|| Value::from(0)),
        Some("number") => Value::from(0.0),
        Some("boolean") => Value::Bool(false),
        Some("null") => Value::Null,
        _ => schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
            .and_then(|branches| branches.first())
            .map(json_schema_template)
            .unwrap_or(Value::Null),
    }
}

#[cfg(test)]
fn json_schema_type_name(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(name)) => Some(name.as_str()),
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> PluginSchema {
        PluginSchema {
            name: "demo".into(),
            category: "test".into(),
            version: "1".into(),
            description: String::new(),
            display_name: None,
            fields: BTreeMap::from([
                (
                    "name".into(),
                    FieldSchema {
                        field_type: FieldType::String,
                        required: true,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: vec![Constraint::Min { value: 1.0 }],
                        read_only: false,
                        read_only_when: None,
                    },
                ),
                (
                    "id".into(),
                    FieldSchema {
                        field_type: FieldType::Integer,
                        required: true,
                        description: String::new(),
                        default: Some(Value::from(1)),
                        example: None,
                        constraints: Vec::new(),
                        read_only: true,
                        read_only_when: None,
                    },
                ),
            ]),
            dependencies: Vec::new(),
            immutable_paths: Vec::new(),
            tags: Vec::new(),
            dialect: String::new(),
            mutation_index: None,
            subids: BTreeMap::new(),
            org: None,
            methods: BTreeMap::new(),
            signals: Vec::new(),
            guarantees: PluginGuarantees::default(),
            ..Default::default()
        }
    }

    #[test]
    fn unions_require_exactly_one_matching_branch() {
        let overlapping = FieldType::OneOf(vec![FieldType::Integer, FieldType::Float]);
        assert!(overlapping.validate(&Value::from(1), &Value::Null).is_err());
        assert!(
            overlapping
                .validate(&Value::from(1.5), &Value::Null)
                .is_ok()
        );
        assert!(
            overlapping
                .validate(&Value::from("other"), &Value::Null)
                .is_err()
        );
    }

    #[test]
    fn named_custom_validation_is_unavailable_instead_of_success() {
        let mut schema = schema();
        schema
            .fields
            .get_mut("name")
            .unwrap()
            .constraints
            .push(Constraint::Custom {
                validator: "plugin_specific".to_owned(),
            });
        let error = schema
            .validate_value(&serde_json::json!({"name":"valid","id":1}), None)
            .unwrap_err();
        assert!(format!("{error:#}").contains("validation is unavailable"));
    }

    #[test]
    fn public_objects_omit_required_private_fields_without_becoming_full_state() {
        let mut schema = schema();
        schema.public_state = serde_json::json!({"paths":["/name"]});
        let full = TypedObjectBlob::new(
            "demo".to_owned(),
            "a".repeat(64),
            schema.clone(),
            serde_json::json!({"name":"before","id":1}),
        )
        .unwrap();
        let public = full.public_projection().unwrap();
        assert_eq!(public.value, serde_json::json!({"name":"before"}));
        assert_eq!(public.value_scope, StateValueScope::PublicProjection);
        assert!(schema.validate_value(&public.value, None).is_err());
        public.validate(None).unwrap();
        assert!(
            schema
                .validate_public_value(&serde_json::json!({"name":"before","id":1}))
                .is_err()
        );
        assert!(
            schema
                .validate_public_value(&serde_json::json!({}))
                .is_err()
        );
        assert!(
            schema
                .validate_public_value(&serde_json::json!({"name":""}))
                .is_err()
        );
    }

    #[test]
    fn public_edits_cannot_assume_a_private_precondition() {
        let mut schema = schema();
        schema.public_state = serde_json::json!({"paths":["/name"]});
        schema
            .fields
            .get_mut("name")
            .unwrap()
            .constraints
            .push(Constraint::RequiresField {
                field: "id".to_owned(),
            });
        let public = schema
            .project_public_value(&serde_json::json!({"name":"before","id":1}))
            .unwrap();
        let unchanged = TypedObjectBlob::new_public(
            "demo".to_owned(),
            "a".repeat(64),
            schema.clone(),
            public.clone(),
        )
        .unwrap();
        unchanged.validate(Some(&public)).unwrap();
        let changed = TypedObjectBlob::new_public(
            "demo".to_owned(),
            "a".repeat(64),
            schema,
            serde_json::json!({"name":"after"}),
        )
        .unwrap();
        assert!(changed.validate(Some(&public)).is_err());
    }

    #[test]
    fn schema_only_captures_carry_no_claimed_state() {
        let mut schema = schema();
        schema.public_state = serde_json::json!({"paths":["/name"]});
        let object =
            TypedObjectBlob::schema_only("demo".to_owned(), "a".repeat(64), schema).unwrap();
        assert_eq!(object.value_scope, StateValueScope::SchemaOnly);
        assert!(object.value.is_null());
        assert_eq!(object.public_projection().unwrap(), object);
        assert!(object.validate(Some(&Value::Null)).is_err());
    }

    #[test]
    fn validates_typed_state_and_read_only_fields() {
        let schema = schema();
        let base = serde_json::json!({"name": "before", "id": 1});
        schema
            .validate_value(&serde_json::json!({"name": "after", "id": 1}), Some(&base))
            .unwrap();
        let error = schema
            .validate_value(&serde_json::json!({"name": "after", "id": 2}), Some(&base))
            .unwrap_err();
        assert!(error.to_string().contains("read-only field /id"));
    }

    #[test]
    fn validates_nested_read_only_fields() {
        let mut schema = schema();
        schema.fields.insert(
            "nested".into(),
            FieldSchema {
                field_type: FieldType::Object(BTreeMap::from([(
                    "fixed".into(),
                    FieldSchema {
                        field_type: FieldType::Integer,
                        required: true,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: Vec::new(),
                        read_only: true,
                        read_only_when: None,
                    },
                )])),
                required: true,
                description: String::new(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        let base = serde_json::json!({"name": "before", "id": 1, "nested": {"fixed": 1}});
        let error = schema
            .validate_value(
                &serde_json::json!({"name": "after", "id": 1, "nested": {"fixed": 2}}),
                Some(&base),
            )
            .unwrap_err();
        assert!(error.to_string().contains("read-only field /nested/fixed"));
    }

    #[test]
    fn parses_producer_map_and_json_schema_field_types() {
        // Wire forms from op-state-store FieldType (externally tagged,
        // rename_all = snake_case): {"map": <FieldType>} and
        // {"json_schema": <document>}.
        let map: FieldType = serde_json::from_value(serde_json::json!({"map": "string"})).unwrap();
        assert_eq!(map, FieldType::Map(Box::new(FieldType::String)));

        let nested_map: FieldType = serde_json::from_value(serde_json::json!({
            "map": {
                "object": {
                    "ready": {
                        "field_type": "boolean",
                        "required": true
                    }
                }
            }
        }))
        .unwrap();
        match nested_map {
            FieldType::Map(inner) => match *inner {
                FieldType::Object(fields) => {
                    assert_eq!(fields["ready"].field_type, FieldType::Boolean);
                    assert!(fields["ready"].required);
                }
                other => panic!("expected object value type, got {other:?}"),
            },
            other => panic!("expected map, got {other:?}"),
        }

        let document = serde_json::json!({
            "type": "object",
            "properties": {
                "revision": {"type": "integer", "minimum": 2}
            },
            "required": ["revision"],
            "additionalProperties": false
        });
        let exact: FieldType = serde_json::from_value(serde_json::json!({
            "json_schema": document
        }))
        .unwrap();
        match &exact {
            FieldType::JsonSchema(schema) => assert_eq!(schema.as_ref(), &document),
            other => panic!("expected json_schema, got {other:?}"),
        }

        exact
            .validate(&serde_json::json!({"revision": 2}), &Value::Null)
            .unwrap();
        assert!(
            exact
                .validate(&serde_json::json!({"revision": 1}), &Value::Null)
                .is_err()
        );
        let templated = exact.template();
        exact.validate(&templated, &Value::Null).unwrap();
        assert_eq!(templated, serde_json::json!({"revision": 2}));

        let open_object = FieldType::JsonSchema(Box::new(serde_json::json!({
            "type": "object",
            "additionalProperties": {"type": "object"}
        })));
        assert_eq!(open_object.template(), serde_json::json!({}));
        open_object
            .validate(&open_object.template(), &Value::Null)
            .unwrap();

        FieldType::Map(Box::new(FieldType::Integer))
            .validate(&serde_json::json!({"a": 1, "b": 2}), &Value::Null)
            .unwrap();
        assert!(
            FieldType::Map(Box::new(FieldType::Integer))
                .validate(&serde_json::json!({"a": "no"}), &Value::Null)
                .is_err()
        );
    }

    #[test]
    fn sealed_schemas_are_decoded_by_their_authority_without_loss() {
        let exact = serde_json::json!({
            "type": "object",
            "properties": {"revision": {"type": "integer", "minimum": 2}},
            "required": ["revision"],
            "additionalProperties": false
        });
        let sealed_json = serde_json::json!({
            "name": "demo",
            "version": "1",
            "description": "sealed fixture",
            "tags": ["immutable"],
            "dependencies": ["other"],
            "example": {"labels": {"a": "b"}},
            "fields": {
                "labels": {"field_type": {"map": "string"}, "required": true},
                "document": {"field_type": {"json_schema": exact}, "required": false},
                "choice": {
                    "field_type": {"one_of": ["string", "integer"]},
                    "required": false,
                    "constraints": [{"type": "one_of", "values": ["a", 1]}],
                    "read_only_when": {"property": "state", "value": "locked"}
                }
            },
            "methods": {
                "record": {
                    "name": "record",
                    "args": {"type": "object", "properties": {"kind": {"type": "string"}}},
                    "returns": null,
                    "side_effect": "mutation",
                    "idempotent": false,
                    "required_capability": "cap.demo.record@v1",
                    "subid": "mut.demo.record@v1"
                }
            },
            "signals": [{"name": "recorded", "subid": "evt.demo.recorded@v1"}]
        });
        let sealed: op_state_store::PluginSchema = serde_json::from_value(sealed_json).unwrap();
        let schema = from_sealed(sealed).unwrap();

        assert_eq!(
            schema.fields["labels"].field_type,
            FieldType::Map(Box::new(FieldType::String))
        );
        assert_eq!(
            schema.fields["document"].field_type,
            FieldType::JsonSchema(Box::new(exact))
        );
        assert_eq!(
            schema.fields["choice"].field_type,
            FieldType::OneOf(vec![FieldType::String, FieldType::Integer])
        );
        assert_eq!(
            schema.fields["choice"].constraints,
            vec![Constraint::OneOf {
                values: vec![Value::from("a"), Value::from(1)]
            }]
        );
        assert_eq!(
            schema.fields["choice"].read_only_when,
            Some(ReadOnlyCondition {
                property: "state".into(),
                value: "locked".into()
            })
        );
        let record = &schema.methods["record"];
        assert_eq!(record.side_effect, SideEffect::Mutation);
        assert_eq!(record.args["properties"]["kind"]["type"], "string");
        assert_eq!(
            record.required_capability.as_deref(),
            Some("cap.demo.record@v1")
        );
        assert_eq!(schema.signals[0].subid, "evt.demo.recorded@v1");
        assert_eq!(schema.tags, vec!["immutable".to_string()]);
        assert_eq!(schema.dependencies, vec!["other".to_string()]);
        // Fields this projection used to drop now survive the conversion.
        assert_eq!(
            schema.example,
            Some(serde_json::json!({"labels": {"a": "b"}}))
        );
        assert_eq!(
            schema
                .required_capabilities()
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["cap.demo.record@v1"]
        );

        // The converted view still validates state as before.
        schema
            .validate_value(
                &serde_json::json!({"labels": {"k": "v"}, "document": {"revision": 3}}),
                None,
            )
            .unwrap();
        assert!(
            schema
                .validate_value(&serde_json::json!({"labels": {"k": 1}}), None)
                .is_err()
        );
    }
}
