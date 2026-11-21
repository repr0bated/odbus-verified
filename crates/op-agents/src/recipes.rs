//! Registry-derived, credential-free recipes. Metadata describes expertise, never authority.
use std::{collections::{BTreeMap, BTreeSet}, sync::LazyLock};
use anyhow::{Result, ensure, Context};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use crate::{dynamic::{PieceLibrary, PieceSource, PieceKind}, unified::GLOBAL_REGISTRY};

pub fn digest(value: &impl Serialize) -> Result<String> {
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.update(bytes); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)?;
    Ok(format!("{:x}", writer.0.finalize()))
}

/// Reuse the canonical bounded, local-reference-only validator with its
/// deny-all external retriever. No recipe can fetch a schema or read a file.
pub fn validate_contract(schema: &Value) -> Result<()> {
    op_state_store::exact_schema::validate_exact_schema_document(schema)
        .context("invalid self-contained orchestration contract")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipeStatus { Enabled, Disabled, Retired }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub source_id: String,
    pub revision: String,
    pub content_digest: String,
    pub trust: String,
    pub access_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PieceRef {
    pub piece_id: String,
    pub content_digest: String,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OperationBinding {
    pub plugin_id: String,
    pub method: String,
    pub schema_hash: String,
    pub required_capability: String,
}

impl OperationBinding {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.plugin_id.is_empty() && !self.method.is_empty() && !self.required_capability.is_empty(), "operation pin incomplete");
        ensure!(self.schema_hash.len() == 64 && self.schema_hash.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "operation requires exact sealed schema hash");
        Ok(())
    }
}

pub fn validate_resource_selector(value: &str) -> Result<()> {
    ensure!(value.starts_with('/') && value.len() <= 4096 && !value.bytes().any(|c| c < 32 || c == 127)
        && !value.contains(['\\', '%']), "resource selector must be a canonical absolute path");
    ensure!(value == "/" || value[1..].split('/').all(|part| !part.is_empty() && part != "." && part != ".."),
        "resource selector contains noncanonical path components");
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunBudget {
    pub max_context_bytes: u64,
    pub reserved_output_bytes: u64,
    pub reserved_tool_bytes: u64,
    pub max_output_tokens: u32,
    pub max_cost_microunits: u64,
    pub max_tool_calls: u32,
    pub max_transitions: u32,
    pub deadline_unix_ms: i64,
}
impl RunBudget {
    pub fn validate(&self, now: i64) -> Result<()> {
        ensure!(self.max_context_bytes > 0 && self.max_context_bytes <= 128 * 1024, "invalid context budget");
        ensure!(self.reserved_output_bytes.checked_add(self.reserved_tool_bytes).is_some_and(|n| n < self.max_context_bytes), "context reserve exhausts budget");
        ensure!(self.max_transitions > 0 && self.max_transitions <= 32, "reasoning transitions exceed shared profile");
        ensure!(self.max_output_tokens > 0 && self.max_cost_microunits > 0, "output and spend budgets must be explicit");
        ensure!(self.deadline_unix_ms > now, "budget deadline expired");
        Ok(())
    }
    pub fn contains(&self, child: &Self) -> bool {
        child.max_context_bytes <= self.max_context_bytes && child.max_output_tokens <= self.max_output_tokens
            && child.max_cost_microunits <= self.max_cost_microunits && child.max_tool_calls <= self.max_tool_calls
            && child.max_transitions <= self.max_transitions && child.deadline_unix_ms <= self.deadline_unix_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRecipe {
    pub recipe_id: String,
    pub version: u64,
    pub content_digest: String,
    pub name: String,
    pub description: String,
    pub status: RecipeStatus,
    pub source_refs: Vec<SourceRef>,
    pub piece_refs: Vec<PieceRef>,
    pub input_contract: Value,
    pub output_contract: Value,
    /// Reference only; never a principal or credential from the prompt.
    pub execution_role_id: Option<String>,
    pub required_operations: Vec<OperationBinding>,
}
impl AgentRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.recipe_id.is_empty() && self.recipe_id.len() <= 256 && self.version > 0, "invalid recipe identity/version");
        ensure!(!self.piece_refs.is_empty(), "recipe has no attributed expertise");
        validate_contract(&self.input_contract)?;
        validate_contract(&self.output_contract)?;
        ensure!(self.piece_refs.iter().map(|p| &p.piece_id).collect::<BTreeSet<_>>().len() == self.piece_refs.len(), "duplicate piece reference");
        for operation in &self.required_operations { operation.validate()?; }
        ensure!(self.content_digest == self.unsigned_digest()?, "recipe digest mismatch");
        Ok(())
    }
    pub fn seal(&mut self) -> Result<()> { self.content_digest = self.unsigned_digest()?; Ok(()) }
    fn unsigned_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Content<'a> {
            recipe_id: &'a str, version: u64, content_digest: &'static str,
            name: &'a str, description: &'a str, status: &'a RecipeStatus,
            source_refs: &'a [SourceRef], piece_refs: &'a [PieceRef],
            input_contract: &'a Value, output_contract: &'a Value,
            execution_role_id: &'a Option<String>, required_operations: &'a [OperationBinding],
        }
        digest(&Content {
            recipe_id:&self.recipe_id, version:self.version, content_digest:"", name:&self.name,
            description:&self.description, status:&self.status, source_refs:&self.source_refs,
            piece_refs:&self.piece_refs, input_contract:&self.input_contract, output_contract:&self.output_contract,
            execution_role_id:&self.execution_role_id, required_operations:&self.required_operations,
        })
    }
}

fn historical_contributes(historical: &str, recipe: &str) -> bool {
    match historical {
        "deployment-engineer" => recipe == "deployment",
        "kubernetes-architect" => ["kubernetes", "kubernetes-expert", "k8s"].contains(&recipe),
        "terraform-specialist" => recipe == "terraform",
        "mem0" => recipe == "memory",
        _ => historical == recipe,
    }
}

fn piece_source_ref(piece: &crate::dynamic::AgentPiece, content_digest: &str) -> Result<SourceRef> {
    let (source_id, revision, trust) = match &piece.source {
        PieceSource::Catalog { agent_id } => (format!("native:catalog:{agent_id}"), "1".into(), "registry_seed_data"),
        PieceSource::Historical { agent_id, path, source_digest } => (format!("historical:{agent_id}:{path}"), source_digest.clone(), "historical_expertise_data_not_execution_policy"),
        _ => anyhow::bail!("source requires an explicit private publisher/access policy"),
    };
    Ok(SourceRef { source_id, revision, content_digest:content_digest.into(), trust:trust.into(), access_scope:"catalog_visible".into() })
}

/// Resolve exact content AND source pins, only in the caller-authorized library.
/// Shared memory/runtime text is not implicitly a trusted catalog publisher.
pub fn validate_recipe_pieces(recipe: &AgentRecipe, library: &PieceLibrary, authorized_scopes: &BTreeSet<String>) -> Result<()> {
    recipe.validate()?;
    for pin in &recipe.piece_refs {
        ensure!(authorized_scopes.contains(&pin.source.access_scope), "piece source is not visible in this scope");
        let piece = library.pieces().iter().find(|piece| piece.id == pin.piece_id).context("pinned piece unavailable")?;
        let actual_digest = digest(piece)?;
        ensure!(actual_digest == pin.content_digest, "pinned piece content changed");
        let source = piece_source_ref(piece, &actual_digest)?;
        ensure!(source == pin.source, "pinned source provenance changed");
    }
    Ok(())
}

pub static CATALOG_PIECES: LazyLock<PieceLibrary> = LazyLock::new(PieceLibrary::seeded_from_catalog);
pub static NATIVE_RECIPES: LazyLock<Result<BTreeMap<String, AgentRecipe>>> = LazyLock::new(native_recipes);

/// One recipe for each exact registry intent ID, even where backend components are shared.
fn native_recipes() -> Result<BTreeMap<String, AgentRecipe>> {
    let mut recipes = BTreeMap::new();
    for id in GLOBAL_REGISTRY.list_ids() {
        let agent = GLOBAL_REGISTRY.get(id).context("registered agent has no metadata")?;
        let pieces: Vec<_> = CATALOG_PIECES.pieces().iter().filter(|piece| {
            piece.kind != PieceKind::Capability && match &piece.source {
                PieceSource::Catalog { agent_id } => agent_id == agent.id(),
                PieceSource::Historical { agent_id, .. } => historical_contributes(agent_id, id),
                _ => false,
            }
        }).map(|piece| {
            let content_digest = digest(piece)?;
            Ok(PieceRef { piece_id:piece.id.clone(), source:piece_source_ref(piece, &content_digest)?, content_digest })
        }).collect::<Result<_>>()?;
        let mut recipe = AgentRecipe {
            recipe_id: id.to_owned(), version: 1, content_digest: String::new(), name: agent.name().into(),
            description: agent.description().into(), status: RecipeStatus::Enabled,
            source_refs: vec![SourceRef { source_id: format!("native:registry:{id}"), revision: "1".into(),
                content_digest: digest(&(id, agent.description(), agent.system_prompt(), agent.knowledge_base(), agent.examples()))?,
                trust: "registry_seed_data".into(), access_scope: "catalog_visible".into() }],
            piece_refs: pieces,
            input_contract: json!({"type":"object","properties":{"question":{"type":"string","minLength":1},"context":{"type":"string"}},"required":["question"],"additionalProperties":false}),
            output_contract: json!({"type":"object","properties":{"answer":{"type":"string","minLength":1},"verification":{"type":"string","enum":["unverified","receipt_grounded"]},"evidence":{"type":"array","items":{"type":"string"}}},"required":["answer","verification","evidence"],"additionalProperties":false}),
            execution_role_id: None, required_operations: Vec::new(),
        };
        for seed in crate::historical_sources::HISTORICAL_SOURCES {
            if historical_contributes(seed.id, id) {
                recipe.source_refs.push(SourceRef {
                    source_id:format!("historical:{}:{}",seed.id,seed.path), revision:seed.source_digest.into(),
                    content_digest:seed.source_digest.into(), trust:"historical_expertise_data_not_execution_policy".into(),
                    access_scope:"catalog_visible".into(),
                });
            }
        }
        recipe.seal()?; recipe.validate()?;
        ensure!(recipes.insert(id.to_owned(), recipe).is_none(), "duplicate registry intent");
    }
    let registry: BTreeSet<_> = GLOBAL_REGISTRY.list_ids().into_iter().map(str::to_owned).collect();
    let descriptors: BTreeSet<_> = crate::builtin_agent_descriptors().into_iter().map(|d| d.agent_type).collect();
    ensure!(registry == descriptors && registry == recipes.keys().cloned().collect(), "native registry/descriptor/recipe coverage mismatch");
    for seed in crate::historical_sources::HISTORICAL_SOURCES {
        ensure!(registry.contains(seed.id) || ["deployment-engineer", "kubernetes-architect", "terraform-specialist", "mem0"].contains(&seed.id),
            "unclassified historical specialist: {}", seed.id);
    }
    Ok(recipes)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentSpec {
    pub goal: String,
    pub desired_output_contract: Value,
    pub success_evidence: Vec<String>,
    pub project_id: String,
    pub resources: Vec<String>,
    pub read_only: bool,
    pub constraints: Vec<String>,
    pub dependencies: Vec<String>,
    pub budget: RunBudget,
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutorDelegationRole {
    pub role_id: String,
    pub version: u64,
    pub enabled: bool,
    pub executor_principal_id: String,
    pub container_id: String,
    pub requestor_principal_id: String,
    pub project_id: String,
    pub resource_prefixes: Vec<String>,
    pub read_only: bool,
    pub operations: Vec<OperationBinding>,
    pub inference_operation: OperationBinding,
    pub judgment_operation: OperationBinding,
    pub expires_unix_ms: i64,
    pub budget: RunBudget,
    pub disclose_result: bool,
    /// Written from the verified management caller, never accepted as authored authority.
    pub approved_by: String,
}
impl ExecutorDelegationRole {
    pub fn permits_intent(&self, principal: &str, intent: &IntentSpec, now: i64) -> Result<()> {
        self.budget.validate(now)?;
        intent.budget.validate(now)?;
        ensure!(!self.approved_by.is_empty() && !self.executor_principal_id.is_empty() && !self.container_id.is_empty(), "delegation lacks owner or actual executor binding");
        self.inference_operation.validate()?;
        self.judgment_operation.validate()?;
        for operation in &self.operations { operation.validate()?; }
        for prefix in &self.resource_prefixes { validate_resource_selector(prefix)?; }
        ensure!(self.enabled && self.version > 0 && self.expires_unix_ms > now, "delegation disabled or expired");
        ensure!(self.requestor_principal_id == principal && self.project_id == intent.project_id, "delegation requestor/project mismatch");
        ensure!(!self.read_only || intent.read_only, "read-only delegation cannot permit mutations");
        ensure!(self.budget.contains(&intent.budget), "task budget exceeds delegation");
        for resource in &intent.resources {
            validate_resource_selector(resource)?;
            ensure!(self.resource_prefixes.iter().any(|prefix| prefix == "/" || resource == prefix || resource.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))), "resource outside delegation scope");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureDirection { Maximize, Minimize }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UtilityFeature {
    pub name: String,
    pub meaning: String,
    pub minimum: i64,
    pub maximum: i64,
    pub weight: u32,
    pub direction: FeatureDirection,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UtilityPolicy {
    pub policy_id: String,
    pub version: u64,
    pub project_id: String,
    pub enabled: bool,
    pub approved_by: String,
    pub features: Vec<UtilityFeature>,
    pub minimum_score: i64,
    pub minimum_margin: u64,
    /// A tie requires review; stable IDs order evidence, not arbitrary choices.
    pub ties_require_review: bool,
    pub judgment_policy_version: String,
}
impl UtilityPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version > 0 && !self.policy_id.is_empty() && !self.approved_by.is_empty(), "utility policy lacks owner/version");
        ensure!(!self.features.is_empty() && self.features.len() <= 16 && self.ties_require_review, "invalid utility feature/tie policy");
        let mut names = BTreeSet::new();
        let mut weight_sum = 0u64;
        for f in &self.features {
            ensure!(names.insert(&f.name) && !f.meaning.is_empty() && f.maximum > f.minimum && f.weight > 0, "invalid utility feature");
            weight_sum = weight_sum.checked_add(u64::from(f.weight)).context("utility weight overflow")?;
        }
        ensure!(weight_sum <= 1_000_000, "utility weights exceed exact arithmetic bound");
        Ok(())
    }
    /// Millionths normalization and checked i128 products: never float/ranking drift.
    pub fn score(&self, inputs: &BTreeMap<String, i64>) -> Result<(i64, BTreeMap<String, i64>)> {
        self.validate()?;
        ensure!(inputs.len() == self.features.len(), "utility inputs must match feature set exactly");
        let mut normalized = BTreeMap::new(); let mut total = 0i128; let mut weights = 0i128;
        for f in &self.features {
            let value = *inputs.get(&f.name).context("missing grounded utility estimate")?;
            ensure!(value >= f.minimum && value <= f.maximum, "utility input outside approved normalization bounds");
            let numerator = match f.direction { FeatureDirection::Maximize => i128::from(value)-i128::from(f.minimum), FeatureDirection::Minimize => i128::from(f.maximum)-i128::from(value) };
            let n = numerator * 1_000_000 / (i128::from(f.maximum)-i128::from(f.minimum));
            total += n * i128::from(f.weight); weights += i128::from(f.weight);
            normalized.insert(f.name.clone(), i64::try_from(n)?);
        }
        Ok((i64::try_from(total / weights)?, normalized))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportedEntry {
    pub source_id: String,
    pub source_revision: String,
    pub kind: String,
    pub alias: String,
    pub enabled: Option<bool>,
    pub reachable: Option<bool>,
    pub assignments: BTreeMap<String, String>,
    pub unsupported_reason: Option<String>,
}
pub fn reconcile_import(visible: &[ImportedEntry], represented: &[ImportedEntry]) -> Result<()> {
    let keys = |items: &[ImportedEntry]| -> Result<BTreeSet<(String,String,String)>> {
        let set: BTreeSet<_> = items.iter().map(|x| (x.source_id.clone(),x.kind.clone(),x.alias.clone())).collect();
        ensure!(set.len() == items.len(), "duplicate source/kind/alias import identity"); Ok(set)
    };
    ensure!(keys(visible)? == keys(represented)?, "external inventory set mismatch");
    for entry in visible {
        let actual = represented.iter().find(|x| x.source_id == entry.source_id && x.kind == entry.kind && x.alias == entry.alias).context("missing import")?;
        ensure!(actual.enabled == entry.enabled && actual.reachable == entry.reachable && actual.assignments == entry.assignments && actual.source_revision == entry.source_revision, "import lost state/assignment/provenance");
    }
    Ok(())
}
