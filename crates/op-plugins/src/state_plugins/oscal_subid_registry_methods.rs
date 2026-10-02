//! Native methods for the sealed OSCAL subid index.
//!
//! Sealed plugin documents remain the only source of subid membership and
//! method references. Cozo stores review annotations for those existing
//! identifiers; it cannot add or redefine one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use op_state_store::PluginSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::common::oscal::validate_subid;
use super::oscal_subid_registry::OscalSubidCategory;
use super::runtime_documents::RuntimeDocuments;

const DOCUMENT_ID: &str = "oscal_subid_registry.annotations.v1";
const READ_CAPABILITY: &str = "oscal.read";
const WRITE_CAPABILITY: &str = "oscal.invoke";
const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;
const MAX_PLUGIN_COUNT: usize = 4_096;
const MAX_SUBID_COUNT: usize = 100_000;
const MAX_BINDINGS_PER_SUBID: usize = 32;
const MAX_SCHEMA_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONTROL_REFS: usize = 64;
const MAX_REFERENCE_LEN: usize = 256;
const MAX_SCHEMA_REFERENCE_LEN: usize = 512;
const MAX_CONTROL_SOURCE_LEN: usize = 2_048;

#[derive(Debug, Clone)]
pub struct OscalInvocationContext {
    pub actor_id: String,
    pub capability_id: String,
}

/// Rechecks only the identity of the bridge-pinned catalog. It never loads a
/// schema or supplies membership, so it cannot become another registry. These
/// comparisons fail closed and narrow catalog-change races, but they are not
/// an atomic transaction spanning the sealer and the Cozo annotation store.
pub trait CatalogPinVerifier: Send + Sync {
    fn verify_current(&self, expected_catalog_hash: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct SealedSchemaDocument {
    pub plugin_id: String,
    pub schema_hash: String,
    pub schema_json: String,
}

#[derive(Debug, Clone)]
pub struct SealedCatalogSnapshot {
    pub catalog_hash: String,
    entries: BTreeMap<String, RegistryEntry>,
    methods: BTreeMap<(String, String), SubidBinding>,
    plugins: BTreeSet<String>,
}

impl SealedCatalogSnapshot {
    /// Build a read model from exact schema bytes already pinned by the bridge.
    /// This function does no filesystem access and computes no catalog identity.
    pub fn from_documents(
        catalog_hash: String,
        documents: Vec<SealedSchemaDocument>,
    ) -> Result<Self> {
        validate_hash("catalog", &catalog_hash)?;
        ensure!(
            documents.len() <= MAX_PLUGIN_COUNT,
            "sealed catalog is too large"
        );
        let mut bindings = BTreeMap::<String, BTreeSet<SubidBinding>>::new();
        let mut methods = BTreeMap::new();
        let mut plugins = BTreeSet::new();

        for document in documents {
            validate_plugin_id(&document.plugin_id)?;
            validate_hash("schema", &document.schema_hash)?;
            ensure!(
                document.schema_json.len() <= MAX_SCHEMA_JSON_BYTES,
                "sealed schema document is too large"
            );
            ensure!(
                plugins.insert(document.plugin_id.clone()),
                "duplicate plugin in sealed snapshot"
            );
            let schema: PluginSchema = serde_json::from_str(&document.schema_json)
                .with_context(|| format!("invalid sealed schema for {}", document.plugin_id))?;
            ensure!(
                schema.name == document.plugin_id,
                "sealed schema/plugin identity mismatch"
            );
            ensure!(
                !schema.version.is_empty() && schema.version.len() <= MAX_REFERENCE_LEN,
                "invalid sealed schema version"
            );

            for (schema_key, subid) in &schema.subids {
                ensure!(
                    !schema_key.is_empty() && schema_key.len() <= MAX_SCHEMA_REFERENCE_LEN,
                    "invalid sealed schema subid key"
                );
                add_binding(
                    &mut bindings,
                    subid,
                    SubidBinding {
                        plugin_id: document.plugin_id.clone(),
                        schema_hash: document.schema_hash.clone(),
                        schema_version: schema.version.clone(),
                        kind: BindingKind::SchemaAnnotation,
                        schema_key: schema_key.clone(),
                        transport_name: None,
                    },
                )?;
            }
            for (schema_key, method) in &schema.methods {
                ensure!(
                    !schema_key.is_empty()
                        && schema_key.len() <= MAX_SCHEMA_REFERENCE_LEN
                        && !method.name.is_empty()
                        && method.name.len() <= MAX_SCHEMA_REFERENCE_LEN,
                    "invalid sealed method reference"
                );
                let binding = SubidBinding {
                    plugin_id: document.plugin_id.clone(),
                    schema_hash: document.schema_hash.clone(),
                    schema_version: schema.version.clone(),
                    kind: BindingKind::Method,
                    schema_key: schema_key.clone(),
                    transport_name: Some(method.name.clone()),
                };
                add_binding(&mut bindings, &method.subid, binding.clone())?;
                ensure!(
                    methods
                        .insert((document.plugin_id.clone(), schema_key.clone()), binding)
                        .is_none(),
                    "duplicate sealed method reference"
                );
            }
            for signal in &schema.signals {
                ensure!(
                    !signal.name.is_empty() && signal.name.len() <= MAX_SCHEMA_REFERENCE_LEN,
                    "invalid sealed signal reference"
                );
                add_binding(
                    &mut bindings,
                    &signal.subid,
                    SubidBinding {
                        plugin_id: document.plugin_id.clone(),
                        schema_hash: document.schema_hash.clone(),
                        schema_version: schema.version.clone(),
                        kind: BindingKind::Signal,
                        schema_key: signal.name.clone(),
                        transport_name: Some(signal.name.clone()),
                    },
                )?;
            }

            let exact: Value = serde_json::from_str(&document.schema_json)?;
            collect_extended_annotations(
                &exact,
                "",
                &document.plugin_id,
                &document.schema_hash,
                &schema.version,
                &mut bindings,
            )?;
            ensure!(
                bindings.len() <= MAX_SUBID_COUNT,
                "sealed subid index is too large"
            );
        }

        let entries = bindings
            .into_iter()
            .map(|(subid, bindings)| {
                ensure!(
                    bindings.len() <= MAX_BINDINGS_PER_SUBID,
                    "sealed subid has too many bindings"
                );
                Ok((
                    subid.clone(),
                    RegistryEntry::new(subid, bindings.into_iter().collect())?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            catalog_hash,
            entries,
            methods,
            plugins,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    Method,
    Signal,
    SchemaAnnotation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SubidBinding {
    pub plugin_id: String,
    pub schema_hash: String,
    pub schema_version: String,
    pub kind: BindingKind,
    /// Canonical PluginSchema map key or an exact JSON pointer for an embedded
    /// x-oscal-subid annotation.
    pub schema_key: String,
    /// MethodDecl.name for method bindings. It may differ from schema_key.
    pub transport_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicComplianceAnnotation {
    pub revision: u64,
    pub control_source: Option<String>,
    pub control_refs: Vec<String>,
    pub statement_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub subid: String,
    pub category: String,
    pub component_type: String,
    pub subject: String,
    pub verb: String,
    pub facets: Vec<String>,
    pub version: u32,
    pub bindings: Vec<SubidBinding>,
    pub annotation: Option<PublicComplianceAnnotation>,
}

impl RegistryEntry {
    fn new(subid: String, bindings: Vec<SubidBinding>) -> Result<Self> {
        validate_bounded_subid(&subid)?;
        let (body, version) = match subid.rsplit_once("@v") {
            Some((body, version)) => (body, version.parse::<u32>()?),
            None => (subid.as_str(), 1),
        };
        let segments = body.split('.').collect::<Vec<_>>();
        ensure!(
            (4..=6).contains(&segments.len()),
            "invalid subid segment count"
        );
        let category = segments[0].to_string();
        let component_type = segments[1].to_string();
        let subject = segments[2].to_string();
        let verb = segments[3].to_string();
        let facets = segments[4..]
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        Ok(Self {
            subid,
            category,
            component_type,
            subject,
            verb,
            facets,
            version,
            bindings,
            annotation: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    pub subid: String,
    pub expected_revision: u64,
    pub control_source: Option<String>,
    #[serde(default)]
    pub control_refs: Vec<String>,
    #[serde(default)]
    pub statement_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterOutput {
    pub subid: String,
    pub annotation_revision: u64,
    pub catalog_hash: String,
    pub binding_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LookupInput {
    pub subid: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LookupOutput {
    pub catalog_hash: String,
    pub entry: Option<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListByCategoryInput {
    pub category: OscalSubidCategory,
    #[serde(default = "default_page_size")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: u16,
    pub cursor: Option<String>,
    /// Required with cursor so a page cannot cross catalog generations.
    pub catalog_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListByCategoryOutput {
    pub catalog_hash: String,
    pub entries: Vec<RegistryEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    pub plugin_id: String,
    pub schema_method_key: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveOutput {
    pub catalog_hash: String,
    pub binding: Option<SubidBinding>,
    pub subid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportInput {
    pub plugin_id: Option<String>,
    pub category: Option<OscalSubidCategory>,
    #[serde(default = "default_page_size")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: u16,
    pub cursor: Option<String>,
    /// Required with cursor so a page cannot cross catalog generations.
    pub catalog_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportOutput {
    pub catalog_hash: String,
    pub entries: Vec<RegistryEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AnnotationDocument {
    annotations: BTreeMap<String, PrivateAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateAnnotation {
    revision: u64,
    control_source: Option<String>,
    control_refs: Vec<String>,
    statement_refs: Vec<String>,
    reviewed_by: String,
    capability_id: String,
}

impl From<&PrivateAnnotation> for PublicComplianceAnnotation {
    fn from(value: &PrivateAnnotation) -> Self {
        Self {
            revision: value.revision,
            control_source: value.control_source.clone(),
            control_refs: value.control_refs.clone(),
            statement_refs: value.statement_refs.clone(),
        }
    }
}

pub struct OscalSubidRegistryMethods {
    documents: Arc<RuntimeDocuments>,
    catalog_pin: Arc<dyn CatalogPinVerifier>,
}

impl OscalSubidRegistryMethods {
    pub fn from_shared(
        documents: Arc<RuntimeDocuments>,
        catalog_pin: Arc<dyn CatalogPinVerifier>,
    ) -> Self {
        Self {
            documents,
            catalog_pin,
        }
    }

    pub async fn dispatch(
        &self,
        method: &str,
        args: &Value,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<Value> {
        let output = match method {
            "register" => serde_json::to_value(
                self.register(serde_json::from_value(args.clone())?, context, snapshot)
                    .await?,
            )?,
            "lookup" => serde_json::to_value(
                self.lookup(serde_json::from_value(args.clone())?, context, snapshot)
                    .await?,
            )?,
            "list_by_category" => serde_json::to_value(
                self.list_by_category(serde_json::from_value(args.clone())?, context, snapshot)
                    .await?,
            )?,
            "resolve" => serde_json::to_value(
                self.resolve(serde_json::from_value(args.clone())?, context, snapshot)
                    .await?,
            )?,
            "export" => serde_json::to_value(
                self.export(serde_json::from_value(args.clone())?, context, snapshot)
                    .await?,
            )?,
            _ => bail!("unknown oscal_subid_registry method: {method}"),
        };
        Ok(output)
    }

    pub async fn register(
        &self,
        input: RegisterInput,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<RegisterOutput> {
        require_context(context, WRITE_CAPABILITY)?;
        validate_bounded_subid(&input.subid)?;
        let entry = snapshot
            .entries
            .get(&input.subid)
            .context("subid is absent from the sealed catalog")?;
        ensure!(
            entry.bindings.len() <= u16::MAX as usize,
            "subid binding count exceeds receipt capacity"
        );
        let binding_count = entry.bindings.len() as u16;
        let control_source = normalize_control_source(input.control_source)?;
        let control_refs = normalize_refs(input.control_refs, "control_refs")?;
        let statement_refs = normalize_refs(input.statement_refs, "statement_refs")?;
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;

        let subid = input.subid.clone();
        let actor = context.actor_id.clone();
        let capability = context.capability_id.clone();
        let expected_revision = input.expected_revision;
        ensure!(
            expected_revision <= i64::MAX as u64,
            "expected_revision is too large"
        );
        let catalog_pin = self.catalog_pin.clone();
        let catalog_hash = snapshot.catalog_hash.clone();
        let (_, revision) = self
            .documents
            .update_with_result(DOCUMENT_ID, move |old| {
                catalog_pin.verify_current(&catalog_hash)?;
                let mut document = parse_annotation_document(old)?;
                let current = document
                    .annotations
                    .get(&subid)
                    .map_or(0, |row| row.revision);
                ensure!(
                    current == expected_revision,
                    "annotation revision conflict: expected {expected_revision}, current {current}"
                );
                let revision = current
                    .checked_add(1)
                    .filter(|revision| *revision <= i64::MAX as u64)
                    .context("annotation revision exhausted")?;
                document.annotations.insert(
                    subid,
                    PrivateAnnotation {
                        revision,
                        control_source,
                        control_refs,
                        statement_refs,
                        reviewed_by: actor,
                        capability_id: capability,
                    },
                );
                ensure!(
                    document.annotations.len() <= MAX_SUBID_COUNT,
                    "durable OSCAL annotation document is too large"
                );
                Ok((serde_json::to_value(document)?, revision))
            })
            .await?;
        Ok(RegisterOutput {
            subid: input.subid,
            annotation_revision: revision,
            catalog_hash: snapshot.catalog_hash.clone(),
            binding_count,
        })
    }

    pub async fn lookup(
        &self,
        input: LookupInput,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<LookupOutput> {
        require_context(context, READ_CAPABILITY)?;
        validate_bounded_subid(&input.subid)?;
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        let annotations = self.annotations().await?;
        let output = LookupOutput {
            catalog_hash: snapshot.catalog_hash.clone(),
            entry: snapshot
                .entries
                .get(&input.subid)
                .cloned()
                .map(|entry| merge_annotation(entry, &annotations)),
        };
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        Ok(output)
    }

    pub async fn list_by_category(
        &self,
        input: ListByCategoryInput,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<ListByCategoryOutput> {
        require_context(context, READ_CAPABILITY)?;
        validate_page(
            input.limit,
            input.cursor.as_deref(),
            input.catalog_hash.as_deref(),
            &snapshot.catalog_hash,
        )?;
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        let category = category_name(input.category);
        let annotations = self.annotations().await?;
        let (entries, next_cursor) = page_entries(
            snapshot
                .entries
                .values()
                .filter(|entry| entry.category == category),
            input.cursor.as_deref(),
            input.limit,
            &annotations,
        );
        let output = ListByCategoryOutput {
            catalog_hash: snapshot.catalog_hash.clone(),
            entries,
            next_cursor,
        };
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        Ok(output)
    }

    pub async fn resolve(
        &self,
        input: ResolveInput,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<ResolveOutput> {
        require_context(context, READ_CAPABILITY)?;
        validate_plugin_id(&input.plugin_id)?;
        ensure!(
            !input.schema_method_key.is_empty(),
            "schema_method_key is empty"
        );
        ensure!(
            input.schema_method_key.len() <= MAX_REFERENCE_LEN,
            "schema_method_key is too long"
        );
        ensure!(
            snapshot.plugins.contains(&input.plugin_id),
            "plugin is absent from the sealed catalog"
        );
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        let binding = snapshot
            .methods
            .get(&(input.plugin_id, input.schema_method_key))
            .cloned();
        let subid = binding.as_ref().and_then(|binding| {
            snapshot.entries.values().find_map(|entry| {
                entry
                    .bindings
                    .contains(binding)
                    .then(|| entry.subid.clone())
            })
        });
        let output = ResolveOutput {
            catalog_hash: snapshot.catalog_hash.clone(),
            binding,
            subid,
        };
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        Ok(output)
    }

    pub async fn export(
        &self,
        input: ExportInput,
        context: &OscalInvocationContext,
        snapshot: &SealedCatalogSnapshot,
    ) -> Result<ExportOutput> {
        require_context(context, READ_CAPABILITY)?;
        validate_page(
            input.limit,
            input.cursor.as_deref(),
            input.catalog_hash.as_deref(),
            &snapshot.catalog_hash,
        )?;
        if let Some(plugin_id) = input.plugin_id.as_deref() {
            validate_plugin_id(plugin_id)?;
            ensure!(
                snapshot.plugins.contains(plugin_id),
                "plugin is absent from the sealed catalog"
            );
        }
        let category = input.category.map(category_name);
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        let annotations = self.annotations().await?;
        let (entries, next_cursor) = page_entries(
            snapshot.entries.values().filter(|entry| {
                category
                    .as_ref()
                    .is_none_or(|category| &entry.category == category)
                    && input.plugin_id.as_ref().is_none_or(|plugin_id| {
                        entry
                            .bindings
                            .iter()
                            .any(|binding| &binding.plugin_id == plugin_id)
                    })
            }),
            input.cursor.as_deref(),
            input.limit,
            &annotations,
        );
        let output = ExportOutput {
            catalog_hash: snapshot.catalog_hash.clone(),
            entries,
            next_cursor,
        };
        self.catalog_pin.verify_current(&snapshot.catalog_hash)?;
        Ok(output)
    }

    async fn annotations(&self) -> Result<AnnotationDocument> {
        parse_annotation_document(
            self.documents
                .read(DOCUMENT_ID)
                .await?
                .map(|row| row.document),
        )
    }
}

fn default_page_size() -> u16 {
    DEFAULT_PAGE_SIZE
}

fn category_name(category: OscalSubidCategory) -> String {
    serde_json::to_value(category)
        .expect("OSCAL category serializes")
        .as_str()
        .expect("OSCAL category is a string")
        .to_string()
}

fn require_context(context: &OscalInvocationContext, capability: &str) -> Result<()> {
    ensure!(
        !context.actor_id.trim().is_empty(),
        "verified actor is required"
    );
    ensure!(
        context.capability_id == capability,
        "exact method capability is required"
    );
    Ok(())
}

fn validate_hash(kind: &str, hash: &str) -> Result<()> {
    ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid {kind} hash"
    );
    Ok(())
}

fn validate_plugin_id(plugin_id: &str) -> Result<()> {
    ensure!(
        !plugin_id.is_empty()
            && plugin_id.len() <= MAX_REFERENCE_LEN
            && plugin_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid plugin id"
    );
    Ok(())
}

fn add_binding(
    bindings: &mut BTreeMap<String, BTreeSet<SubidBinding>>,
    subid: &str,
    binding: SubidBinding,
) -> Result<()> {
    validate_bounded_subid(subid)?;
    let rows = bindings.entry(subid.to_string()).or_default();
    rows.insert(binding);
    ensure!(
        rows.len() <= MAX_BINDINGS_PER_SUBID,
        "sealed subid has too many bindings"
    );
    Ok(())
}

fn collect_extended_annotations(
    value: &Value,
    pointer: &str,
    plugin_id: &str,
    schema_hash: &str,
    schema_version: &str,
    bindings: &mut BTreeMap<String, BTreeSet<SubidBinding>>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let child = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                ensure!(
                    child.len() <= MAX_SCHEMA_REFERENCE_LEN,
                    "sealed schema annotation path is too long"
                );
                if key == "x-oscal-subid" {
                    let subid = nested.as_str().context("x-oscal-subid must be a string")?;
                    add_binding(
                        bindings,
                        subid,
                        SubidBinding {
                            plugin_id: plugin_id.to_string(),
                            schema_hash: schema_hash.to_string(),
                            schema_version: schema_version.to_string(),
                            kind: BindingKind::SchemaAnnotation,
                            schema_key: child.clone(),
                            transport_name: None,
                        },
                    )?;
                }
                collect_extended_annotations(
                    nested,
                    &child,
                    plugin_id,
                    schema_hash,
                    schema_version,
                    bindings,
                )?;
            }
        }
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                collect_extended_annotations(
                    nested,
                    &format!("{pointer}/{index}"),
                    plugin_id,
                    schema_hash,
                    schema_version,
                    bindings,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_control_source(value: Option<String>) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim().to_string();
            ensure!(!value.is_empty(), "control_source is empty");
            ensure!(
                value.len() <= MAX_CONTROL_SOURCE_LEN,
                "control_source is too long"
            );
            ensure!(
                !value.chars().any(char::is_control),
                "control_source contains control characters"
            );
            Ok(value)
        })
        .transpose()
}

fn normalize_refs(values: Vec<String>, field: &str) -> Result<Vec<String>> {
    ensure!(
        values.len() <= MAX_CONTROL_REFS,
        "{field} has too many values"
    );
    let mut normalized = BTreeSet::new();
    for value in values {
        let value = value.trim().to_string();
        ensure!(!value.is_empty(), "{field} contains an empty value");
        ensure!(
            value.len() <= MAX_REFERENCE_LEN,
            "{field} value is too long"
        );
        ensure!(
            value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.' | '(' | ')')
            }),
            "{field} contains invalid characters"
        );
        normalized.insert(value);
    }
    Ok(normalized.into_iter().collect())
}

fn validate_page(
    limit: u16,
    cursor: Option<&str>,
    expected_catalog_hash: Option<&str>,
    catalog_hash: &str,
) -> Result<()> {
    ensure!(
        (1..=MAX_PAGE_SIZE).contains(&limit),
        "limit must be between 1 and {MAX_PAGE_SIZE}"
    );
    if let Some(cursor) = cursor {
        validate_bounded_subid(cursor)?;
        ensure!(
            expected_catalog_hash == Some(catalog_hash),
            "cursor requires its exact catalog_hash"
        );
    } else if let Some(expected) = expected_catalog_hash {
        ensure!(
            expected == catalog_hash,
            "catalog_hash does not match current snapshot"
        );
    }
    Ok(())
}

fn page_entries<'a>(
    entries: impl Iterator<Item = &'a RegistryEntry>,
    cursor: Option<&str>,
    limit: u16,
    annotations: &AnnotationDocument,
) -> (Vec<RegistryEntry>, Option<String>) {
    let mut rows = entries
        .filter(|entry| cursor.is_none_or(|cursor| entry.subid.as_str() > cursor))
        .take(limit as usize + 1)
        .cloned()
        .collect::<Vec<_>>();
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last().expect("non-empty bounded page").subid.clone());
    let rows = rows
        .into_iter()
        .map(|entry| merge_annotation(entry, annotations))
        .collect();
    (rows, next_cursor)
}

fn merge_annotation(mut entry: RegistryEntry, annotations: &AnnotationDocument) -> RegistryEntry {
    entry.annotation = annotations.annotations.get(&entry.subid).map(Into::into);
    entry
}

fn parse_annotation_document(value: Option<Value>) -> Result<AnnotationDocument> {
    let document: AnnotationDocument = value
        .map(serde_json::from_value)
        .transpose()
        .context("invalid durable OSCAL annotation document")
        .map(Option::unwrap_or_default)?;
    ensure!(
        document.annotations.len() <= MAX_SUBID_COUNT,
        "durable OSCAL annotation document is too large"
    );
    for (subid, annotation) in &document.annotations {
        validate_bounded_subid(subid)?;
        ensure!(annotation.revision > 0, "annotation revision is invalid");
        ensure!(
            normalize_control_source(annotation.control_source.clone())?
                == annotation.control_source,
            "annotation control_source is not canonical"
        );
        ensure!(
            normalize_refs(annotation.control_refs.clone(), "control_refs")?
                == annotation.control_refs,
            "annotation control_refs are not canonical"
        );
        ensure!(
            normalize_refs(annotation.statement_refs.clone(), "statement_refs")?
                == annotation.statement_refs,
            "annotation statement_refs are not canonical"
        );
        ensure!(
            !annotation.reviewed_by.trim().is_empty()
                && annotation.capability_id == WRITE_CAPABILITY,
            "annotation private attribution is invalid"
        );
    }
    Ok(document)
}

fn validate_bounded_subid(subid: &str) -> Result<()> {
    ensure!(subid.len() <= MAX_REFERENCE_LEN, "subid is too long");
    validate_subid(subid).map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_registry::DefaultPluginRegistry;
    use op_cozo_store::CozoGraphShuttle;
    use op_state_store::{MemoryStore, MethodDecl, SideEffect, SignalDecl};
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Pin(Mutex<String>);

    impl CatalogPinVerifier for Pin {
        fn verify_current(&self, expected_catalog_hash: &str) -> Result<()> {
            ensure!(
                *self.0.lock().unwrap() == expected_catalog_hash,
                "sealed catalog changed"
            );
            Ok(())
        }
    }

    struct ChangesAfterFirstCheck(AtomicUsize);

    impl CatalogPinVerifier for ChangesAfterFirstCheck {
        fn verify_current(&self, _expected_catalog_hash: &str) -> Result<()> {
            ensure!(
                self.0.fetch_add(1, Ordering::SeqCst) == 0,
                "sealed catalog changed"
            );
            Ok(())
        }
    }

    fn method(name: &str, subid: &str, side_effect: SideEffect) -> MethodDecl {
        MethodDecl {
            name: name.to_string(),
            args: simd_json::json!({"type":"object","additionalProperties":false}),
            returns: None,
            side_effect,
            idempotent: true,
            required_capability: None,
            subid: subid.to_string(),
        }
    }

    fn snapshot() -> SealedCatalogSnapshot {
        let mut alpha = PluginSchema::builder("alpha")
            .string_field("status", false, "status")
            .subid("status", "obs.service.alpha.status@v1")
            .build();
        alpha.methods.insert(
            "canonical_key".into(),
            method(
                "TransportName",
                "obs.service.alpha.inspect@v1",
                SideEffect::Read,
            ),
        );
        alpha.signals.push(SignalDecl {
            name: "Changed".into(),
            payload: None,
            subid: "evt.service.alpha.changed@v1".into(),
        });
        let mut beta = PluginSchema::builder("beta").build();
        beta.methods.insert(
            "apply".into(),
            method("Apply", "mut.service.beta.apply@v1", SideEffect::Mutation),
        );
        let catalog = tempfile::tempdir().unwrap();
        let mut store = op_blob::BlobStore::open(catalog.path()).unwrap();
        store
            .write(&op_blob::blobify_plugin_schema("beta", beta))
            .unwrap();
        store
            .write(&op_blob::blobify_plugin_schema("alpha", alpha))
            .unwrap();
        let catalog_hash = store.catalog_hash();
        drop(store);
        let reopened = op_blob::BlobStore::open(catalog.path()).unwrap();
        let documents = reopened
            .plugin_object_blobs()
            .into_iter()
            .map(|blob| SealedSchemaDocument {
                plugin_id: blob.manifest.plugin_id,
                schema_hash: blob.manifest.schema_hash,
                schema_json: blob.schema_json,
            })
            .collect();
        SealedCatalogSnapshot::from_documents(catalog_hash, documents).unwrap()
    }

    #[tokio::test]
    async fn every_compiled_canonical_schema_builds_the_bounded_sealed_index() {
        let catalog = tempfile::tempdir().unwrap();
        let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
        let plugin_ids = DefaultPluginRegistry::available_plugins();
        assert!(!plugin_ids.is_empty());

        let mut store = op_blob::BlobStore::open(catalog.path()).unwrap();
        for plugin_id in &plugin_ids {
            let plugin = registry
                .load_plugin(plugin_id)
                .await
                .unwrap_or_else(|error| panic!("load compiled plugin {plugin_id}: {error}"));
            let schema = plugin
                .schema()
                .unwrap_or_else(|| panic!("compiled plugin {plugin_id} has no canonical schema"));
            store
                .write(&op_blob::blobify_plugin_schema(plugin_id, schema))
                .unwrap_or_else(|error| panic!("seal compiled plugin {plugin_id}: {error}"));
        }
        let catalog_hash = store.catalog_hash();
        drop(store);

        let reopened = op_blob::BlobStore::open(catalog.path()).unwrap();
        let blobs = reopened.plugin_object_blobs();
        assert_eq!(blobs.len(), plugin_ids.len());
        let documents = blobs
            .into_iter()
            .map(|blob| SealedSchemaDocument {
                plugin_id: blob.manifest.plugin_id,
                schema_hash: blob.manifest.schema_hash,
                schema_json: blob.schema_json,
            })
            .collect();
        let snapshot = SealedCatalogSnapshot::from_documents(catalog_hash, documents)
            .expect("every compiled schema must have valid bounded subids and bindings");
        assert_eq!(snapshot.plugins.len(), plugin_ids.len());
        assert!(!snapshot.entries.is_empty());
    }

    fn context(capability: &str) -> OscalInvocationContext {
        OscalInvocationContext {
            actor_id: "principal:test".into(),
            capability_id: capability.into(),
        }
    }

    fn service(path: std::path::PathBuf, hash: &str) -> (OscalSubidRegistryMethods, Arc<Pin>) {
        let store = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent(path).unwrap(),
        ));
        let pin = Arc::new(Pin(Mutex::new(hash.to_string())));
        (
            OscalSubidRegistryMethods::from_shared(store, pin.clone()),
            pin,
        )
    }

    #[tokio::test]
    async fn sealed_lookup_resolve_and_pages_preserve_canonical_method_keys() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = snapshot();
        let (service, _) = service(dir.path().join("cozo"), &snapshot.catalog_hash);
        let read = context(READ_CAPABILITY);
        let resolved = service
            .resolve(
                ResolveInput {
                    plugin_id: "alpha".into(),
                    schema_method_key: "canonical_key".into(),
                },
                &read,
                &snapshot,
            )
            .await
            .unwrap();
        let binding = resolved.binding.unwrap();
        assert_eq!(
            resolved.subid.as_deref(),
            Some("obs.service.alpha.inspect@v1")
        );
        assert_eq!(binding.schema_key, "canonical_key");
        assert_eq!(binding.transport_name.as_deref(), Some("TransportName"));

        let first = service
            .export(
                ExportInput {
                    plugin_id: None,
                    category: None,
                    limit: 2,
                    cursor: None,
                    catalog_hash: None,
                },
                &read,
                &snapshot,
            )
            .await
            .unwrap();
        assert_eq!(first.entries.len(), 2);
        assert!(first.entries[0].subid < first.entries[1].subid);
        let second = service
            .export(
                ExportInput {
                    plugin_id: None,
                    category: None,
                    limit: 2,
                    cursor: first.next_cursor,
                    catalog_hash: Some(first.catalog_hash),
                },
                &read,
                &snapshot,
            )
            .await
            .unwrap();
        assert_eq!(second.entries.len(), 2);
        assert!(second.next_cursor.is_none());

        let observations = service
            .list_by_category(
                ListByCategoryInput {
                    category: OscalSubidCategory::Obs,
                    limit: 10,
                    cursor: None,
                    catalog_hash: None,
                },
                &read,
                &snapshot,
            )
            .await
            .unwrap();
        assert_eq!(observations.entries.len(), 2);
        assert!(
            observations
                .entries
                .iter()
                .all(|entry| entry.category == "obs")
        );
    }

    #[tokio::test]
    async fn register_is_cas_scoped_to_existing_sealed_subids_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cozo");
        let snapshot = snapshot();
        let write = context(WRITE_CAPABILITY);
        let input = RegisterInput {
            subid: "obs.service.alpha.inspect@v1".into(),
            expected_revision: 0,
            control_source: Some("NIST SP 800-53".into()),
            control_refs: vec!["CM-3".into(), "AC-1".into(), "CM-3".into()],
            statement_refs: vec!["AC-1_smt.a".into()],
        };
        {
            let (service, _) = service(path.clone(), &snapshot.catalog_hash);
            let receipt = service
                .register(input.clone(), &write, &snapshot)
                .await
                .unwrap();
            assert_eq!(receipt.annotation_revision, 1);
            assert!(service.register(input, &write, &snapshot).await.is_err());
            assert!(
                service
                    .register(
                        RegisterInput {
                            subid: "obs.service.missing.lookup@v1".into(),
                            expected_revision: 0,
                            control_source: None,
                            control_refs: vec![],
                            statement_refs: vec![],
                        },
                        &write,
                        &snapshot,
                    )
                    .await
                    .is_err()
            );
        }
        let (service, _) = service(path, &snapshot.catalog_hash);
        let output = service
            .lookup(
                LookupInput {
                    subid: "obs.service.alpha.inspect@v1".into(),
                },
                &context(READ_CAPABILITY),
                &snapshot,
            )
            .await
            .unwrap();
        let value = serde_json::to_value(output).unwrap();
        assert_eq!(value["entry"]["annotation"]["revision"], 1);
        assert_eq!(
            value["entry"]["annotation"]["control_refs"],
            json!(["AC-1", "CM-3"])
        );
        let encoded = value.to_string();
        assert!(!encoded.contains("principal:test"));
        assert!(!encoded.contains(WRITE_CAPABILITY));
        assert!(!encoded.contains("reviewed_by"));
    }

    #[tokio::test]
    async fn changed_catalog_and_wrong_context_reject_before_durable_write() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = snapshot();
        let (service, pin) = service(dir.path().join("cozo"), &snapshot.catalog_hash);
        *pin.0.lock().unwrap() = "d".repeat(64);
        let input = RegisterInput {
            subid: "obs.service.alpha.inspect@v1".into(),
            expected_revision: 0,
            control_source: None,
            control_refs: vec![],
            statement_refs: vec![],
        };
        assert!(
            service
                .register(input, &context(WRITE_CAPABILITY), &snapshot)
                .await
                .is_err()
        );
        assert!(service.documents.read(DOCUMENT_ID).await.unwrap().is_none());
        assert!(
            service
                .lookup(
                    LookupInput {
                        subid: "obs.service.alpha.inspect@v1".into(),
                    },
                    &context(WRITE_CAPABILITY),
                    &snapshot,
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn read_fails_closed_when_catalog_changes_during_result_construction() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = snapshot();
        let pin = Arc::new(ChangesAfterFirstCheck(AtomicUsize::new(0)));
        let service = OscalSubidRegistryMethods::from_shared(
            Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_persistent(dir.path().join("cozo")).unwrap(),
            )),
            pin.clone(),
        );

        let error = service
            .lookup(
                LookupInput {
                    subid: "obs.service.alpha.inspect@v1".into(),
                },
                &context(READ_CAPABILITY),
                &snapshot,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("sealed catalog changed"));
        assert_eq!(pin.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn subtree_annotations_are_distinct_bindings_and_invalid_subids_fail_closed() {
        let schema = PluginSchema::builder("nested").build();
        let mut value = serde_json::to_value(schema).unwrap();
        value["example"] = json!({
            "left": {"x-oscal-subid":"obs.service.nested.inspect@v1"},
            "right": {"x-oscal-subid":"obs.service.nested.inspect@v1"}
        });
        let snapshot = SealedCatalogSnapshot::from_documents(
            "e".repeat(64),
            vec![SealedSchemaDocument {
                plugin_id: "nested".into(),
                schema_hash: "f".repeat(64),
                schema_json: value.to_string(),
            }],
        )
        .unwrap();
        let entry = &snapshot.entries["obs.service.nested.inspect@v1"];
        assert_eq!(entry.bindings.len(), 2);
        assert_ne!(entry.bindings[0].schema_key, entry.bindings[1].schema_key);

        let mut invalid = value;
        invalid["example"]["bad"] = json!({"x-oscal-subid":"not-valid"});
        assert!(
            SealedCatalogSnapshot::from_documents(
                "e".repeat(64),
                vec![SealedSchemaDocument {
                    plugin_id: "nested".into(),
                    schema_hash: "f".repeat(64),
                    schema_json: invalid.to_string(),
                }],
            )
            .is_err()
        );
    }
}
