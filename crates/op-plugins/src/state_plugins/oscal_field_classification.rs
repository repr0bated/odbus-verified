//! Shared OSCAL field assessment. This module consumes schema contracts, never
//! runtime values. It neither publishes schemas/rules nor grants disclosure.
//!
//! A candidate pin is producer evidence, not sealed membership. Runtime callers
//! must obtain sealed pins from the manifest reader and rules from OSCAL's
//! private document. Missing authority is deliberately different from no rules.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ASSESS_CAPABILITY: &str = "cap.standard.oscal.field-classification.read@v1";
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_NODES: usize = 8_192;
const MAX_ROOTS: usize = 512;
const MAX_DEPTH: usize = 64;
const MAX_REFS: usize = 512;
const MAX_BRANCHES: usize = 1_024;
const MAX_RULES: usize = 4_096;
const MAX_RULE_CHECKS: usize = 1_048_576;
const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;
const MAX_POINTER: usize = 2_048;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssessFieldsInput {
    /// Exact plugin id; the backend selects its current sealed source and rules.
    #[schemars(length(min = 1, max = 256))]
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldSourcePin {
    pub plugin_id: String,
    pub schema_version: String,
    pub source: FieldSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldSource {
    Candidate {
        candidate_digest: String,
        producer_id: String,
        /// Pins the producing source/binary and discovery procedure together.
        producer_digest: String,
    },
    Sealed {
        catalog_hash: String,
        schema_hash: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldRoot {
    State,
    MethodInput {
        method_key: String,
    },
    MethodResult {
        method_key: String,
    },
    SignalPayload {
        signal_name: String,
    },
    /// The schema document's annotations, never a runtime payload selector.
    DocumentMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PayloadStep {
    Property { name: String },
    EachItem,
    EachMapKey,
    EachMapValue,
}

/// Enforcement floors, not a second privacy taxonomy. Class identifiers and
/// their meaning come only from the reviewed shared OSCAL vocabulary.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionFloor {
    None,
    Private,
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldClass {
    pub id: String,
    pub revision: u64,
    pub minimum_protection: ProtectionFloor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldRuleScope {
    Exact,
    Subtree,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldRule {
    pub id: String,
    pub revision: u64,
    pub source: FieldSourcePin,
    pub root: FieldRoot,
    /// Pointer within the selected exact root, NOT a runtime data path.
    pub schema_pointer: String,
    /// None for metadata. Empty means the whole payload root.
    pub selector: Option<Vec<PayloadStep>>,
    pub scope: FieldRuleScope,
    pub include_map_keys: bool,
    pub class_id: String,
}

/// Owner-private domain data, not another schema source. No runtime method in
/// this slice can create it. Authoring/initialization must use reviewed CAS and
/// a separately implemented release seed; an absent store never bootstraps.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewedFieldRules {
    pub revision: u64,
    pub vocabulary_revision: u64,
    pub classes: Vec<FieldClass>,
    pub rules: Vec<FieldRule>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldResolution {
    Resolved,
    RestrictedOpaque,
    Uncovered,
    Conflict,
    Stale,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuleEvidence {
    pub rule_id: String,
    pub rule_revision: u64,
    pub class_id: String,
    pub class_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldFinding {
    pub root: FieldRoot,
    pub schema_pointer: String,
    pub selector: Option<Vec<PayloadStep>>,
    /// Local reference and alternative pointers only; no defaults/examples.
    pub reference_chain: Vec<String>,
    pub branch_chain: Vec<String>,
    pub protection_floor: ProtectionFloor,
    pub resolution: FieldResolution,
    pub rules: Vec<RuleEvidence>,
    pub opaque_interior: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentLimit {
    Nodes,
    Roots,
    Depth,
    References,
    Branches,
    Pointer,
    RuleChecks,
    ReportBytes,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldAuthority {
    Unavailable,
    Reviewed {
        document_revision: u64,
        rule_revision: u64,
        vocabulary_revision: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldRuleIssueKind {
    StaleSource,
    UnmatchedAnchor,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldRuleIssue {
    pub rule_id: String,
    pub rule_revision: u64,
    pub kind: FieldRuleIssueKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldAssessment {
    pub source: FieldSourcePin,
    pub authority: FieldAuthority,
    pub roots: Vec<FieldRoot>,
    pub findings: Vec<FieldFinding>,
    pub rule_issues: Vec<FieldRuleIssue>,
    pub limits_hit: Vec<AssessmentLimit>,
    pub traversal_complete: bool,
    pub boundary_coverage_complete: bool,
    /// False for free-form strings/JSON even when their boundary is protected.
    pub interior_inspection_complete: bool,
    /// The supplied representation alone cannot attest producer-to-adapter or
    /// release preservation. That integration needs both exact source inputs.
    #[schemars(extend("const" = false))]
    pub source_preservation_verified: bool,
    /// Existing public-state projection still applies independently. This
    /// report is never a disclosure permit, including when all fields resolve.
    #[schemars(extend("const" = false))]
    pub grants_disclosure: bool,
}

/// Count JSON output without allocating a serialized copy. A failing writer
/// stops serialization at the bound, including escaping and punctuation.
fn encoded_size<T: Serialize>(value: &T, maximum: usize) -> Option<usize> {
    struct Counter {
        used: usize,
        maximum: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.maximum.saturating_sub(self.used) {
                return Err(std::io::Error::other("field report exceeds bound"));
            }
            self.used += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { used: 0, maximum };
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.used)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-@/".contains(&b))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate_pin(pin: &FieldSourcePin) -> Result<()> {
    ensure!(
        valid_id(&pin.plugin_id) && valid_id(&pin.schema_version),
        "invalid field source identity"
    );
    let valid = match &pin.source {
        FieldSource::Candidate {
            candidate_digest,
            producer_id,
            producer_digest,
        } => valid_hash(candidate_digest) && valid_id(producer_id) && valid_hash(producer_digest),
        FieldSource::Sealed {
            catalog_hash,
            schema_hash,
        } => valid_hash(catalog_hash) && valid_hash(schema_hash),
    };
    ensure!(valid, "invalid field source pin");
    Ok(())
}

impl ReviewedFieldRules {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.revision > 0 && self.vocabulary_revision > 0,
            "invalid field authority revision"
        );
        ensure!(
            self.classes.len() <= MAX_RULES && self.rules.len() <= MAX_RULES,
            "field authority exceeds bounds"
        );
        let mut classes = BTreeSet::new();
        for class in &self.classes {
            ensure!(
                valid_id(&class.id) && class.revision > 0 && classes.insert(&class.id),
                "invalid field class"
            );
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            validate_pin(&rule.source)?;
            ensure!(
                valid_id(&rule.id) && rule.revision > 0 && ids.insert(&rule.id),
                "invalid field rule"
            );
            ensure!(
                classes.contains(&rule.class_id),
                "field rule class is absent from reviewed vocabulary"
            );
            ensure!(
                valid_pointer(&rule.schema_pointer),
                "invalid field schema pointer"
            );
            ensure!(
                (rule.root == FieldRoot::DocumentMetadata) == rule.selector.is_none(),
                "metadata and payload selectors must remain distinct"
            );
            if let Some(selector) = &rule.selector {
                ensure!(selector.len() <= MAX_DEPTH, "field selector exceeds bounds");
                for step in selector {
                    if let PayloadStep::Property { name } = step {
                        ensure!(name.len() <= 256, "field selector property exceeds bounds");
                    }
                }
            }
        }
        Ok(())
    }
}

fn valid_pointer(pointer: &str) -> bool {
    if pointer.len() > MAX_POINTER || (!pointer.is_empty() && !pointer.starts_with('/')) {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

fn escape(part: &str) -> String {
    part.replace('~', "~0").replace('/', "~1")
}

#[derive(Clone)]
struct Position {
    root: FieldRoot,
    pointer: String,
    selector: Option<Vec<PayloadStep>>,
    refs: Vec<String>,
    branches: Vec<String>,
    ancestors: Vec<usize>,
    floor: ProtectionFloor,
    depth: usize,
}

impl Position {
    fn root(root: FieldRoot) -> Self {
        let selector = (root != FieldRoot::DocumentMetadata).then(Vec::new);
        Self {
            root,
            pointer: String::new(),
            selector,
            refs: vec![],
            branches: vec![],
            ancestors: vec![],
            floor: ProtectionFloor::None,
            depth: 0,
        }
    }

    fn child(&self, suffix: &str, step: Option<PayloadStep>, parent: usize) -> Self {
        let mut next = self.clone();
        next.pointer.push_str(suffix);
        if let (Some(selector), Some(step)) = (&mut next.selector, step) {
            selector.push(step);
        }
        next.ancestors.push(parent);
        next.depth += 1;
        next
    }
}

struct Boundary {
    finding: FieldFinding,
    ancestors: Vec<usize>,
    unsupported: bool,
}

#[derive(Default)]
struct Walker {
    boundaries: Vec<Boundary>,
    roots: Vec<FieldRoot>,
    limits: Vec<AssessmentLimit>,
    refs: usize,
    branches: usize,
    report_bytes: usize,
    rule_issues: Vec<FieldRuleIssue>,
}

impl Walker {
    fn limit(&mut self, limit: AssessmentLimit) {
        if !self.limits.contains(&limit) {
            self.limits.push(limit);
        }
    }

    fn add(&mut self, pos: &Position, opaque: bool, unsupported: bool) -> Option<usize> {
        if !self.limits.is_empty() {
            return None;
        }
        if pos.depth > MAX_DEPTH {
            self.limit(AssessmentLimit::Depth);
            return None;
        }
        if pos.pointer.len() > MAX_POINTER
            || pos.selector.as_ref().is_some_and(|s| {
                s.iter()
                    .any(|v| matches!(v, PayloadStep::Property { name } if name.len() > 256))
            })
        {
            self.limit(AssessmentLimit::Pointer);
            return None;
        }
        if self.boundaries.len() >= MAX_NODES {
            self.limit(AssessmentLimit::Nodes);
            return None;
        }
        let finding = FieldFinding {
            root: pos.root.clone(),
            schema_pointer: pos.pointer.clone(),
            selector: pos.selector.clone(),
            reference_chain: pos.refs.clone(),
            branch_chain: pos.branches.clone(),
            protection_floor: pos.floor,
            resolution: FieldResolution::Uncovered,
            rules: vec![],
            opaque_interior: opaque,
        };
        let Some(bytes) =
            encoded_size(&finding, MAX_REPORT_BYTES.saturating_sub(self.report_bytes))
        else {
            self.limit(AssessmentLimit::ReportBytes);
            return None;
        };
        // Reserve changes to outcome strings/punctuation plus internal ancestry.
        let charge = bytes + 64 + pos.ancestors.len() * 8;
        if self.report_bytes.saturating_add(charge) > MAX_REPORT_BYTES {
            self.limit(AssessmentLimit::ReportBytes);
            return None;
        }
        self.report_bytes += charge;
        let index = self.boundaries.len();
        self.boundaries.push(Boundary {
            finding,
            ancestors: pos.ancestors.clone(),
            unsupported,
        });
        Some(index)
    }

    fn start(&mut self, root: FieldRoot) -> Option<Position> {
        if self.roots.len() >= MAX_ROOTS {
            self.limit(AssessmentLimit::Roots);
            return None;
        }
        if !self.limits.is_empty() {
            return None;
        }
        if self.report_bytes == 0 {
            self.report_bytes = 4_096;
        } // source, authority, flags, limits
        let Some(bytes) = encoded_size(
            &root,
            MAX_REPORT_BYTES.saturating_sub(self.report_bytes + 1),
        ) else {
            self.limit(AssessmentLimit::ReportBytes);
            return None;
        };
        self.report_bytes += bytes + 1;
        self.roots.push(root.clone());
        Some(Position::root(root))
    }

    fn protected(mut pos: Position, schema: &Value) -> Position {
        if schema.get("x-opdbus-private").and_then(Value::as_bool) == Some(true) {
            pos.floor = pos.floor.max(ProtectionFloor::Private);
        }
        if schema.get("x-secret").and_then(Value::as_bool) == Some(true) {
            pos.floor = ProtectionFloor::Secret;
        }
        pos
    }

    /// Full method/signal schemas and exact state fields. Every graph expansion
    /// has a depth/node bound. Only local JSON pointers are resolved.
    fn json_schema(&mut self, schema: &Value, document: &Value, base: &str, pos: Position) {
        let pos = Self::protected(pos, schema);
        let unsupported = schema.as_object().is_some_and(|o| {
            o.keys().any(|k| {
                matches!(
                    k.as_str(),
                    "$dynamicRef"
                        | "$recursiveRef"
                        | "$id"
                        | "if"
                        | "then"
                        | "else"
                        | "not"
                        | "dependentSchemas"
                        | "dependencies"
                        | "unevaluatedProperties"
                        | "unevaluatedItems"
                        | "patternProperties"
                        | "prefixItems"
                        | "additionalItems"
                        | "contains"
                )
            })
        });
        let Some(parent) = self.add(&pos, false, unsupported) else {
            return;
        };
        if schema == &Value::Bool(false) {
            return;
        }
        if schema == &Value::Bool(true) || schema.as_object().is_none() {
            self.boundaries[parent].finding.opaque_interior = true;
            if schema != &Value::Bool(true) {
                self.boundaries[parent].unsupported = true;
            }
            return;
        }
        if let Some(reference) = schema.get("$ref") {
            let reference = reference.as_str().unwrap_or("");
            if self.refs >= MAX_REFS {
                self.limit(AssessmentLimit::References);
                return;
            }
            self.refs += 1;
            let provenance = format!("{base}{reference}");
            if !reference.starts_with("#/")
                || !valid_pointer(&reference[1..])
                || pos.refs.contains(&provenance)
            {
                self.boundaries[parent].unsupported = true;
            } else if let Some(target) = document.pointer(&reference[1..]) {
                let mut next = pos.child("", None, parent);
                next.pointer = format!("{base}{}", &reference[1..]);
                next.refs.push(provenance);
                self.json_schema(target, document, base, next);
            } else {
                self.boundaries[parent].unsupported = true;
            }
        }
        let mut alternatives = false;
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(branches) = schema.get(key) {
                alternatives = true;
                let Some(branches) = branches.as_array().filter(|b| !b.is_empty()) else {
                    self.boundaries[parent].unsupported = true;
                    continue;
                };
                for (index, branch) in branches.iter().enumerate() {
                    if self.branches >= MAX_BRANCHES {
                        self.limit(AssessmentLimit::Branches);
                        return;
                    }
                    self.branches += 1;
                    let mut next = pos.child(&format!("/{key}/{index}"), None, parent);
                    next.branches.push(next.pointer.clone());
                    self.json_schema(branch, document, base, next);
                    if !self.limits.is_empty() {
                        return;
                    }
                }
            }
        }
        // Type arrays (e.g. [string, null]) share one contract. Walk all their
        // structured possibilities; never select a permissive first branch.
        let types: Vec<&str> = match schema.get("type") {
            Some(Value::String(value)) => vec![value.as_str()],
            Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if schema.get("type").is_some_and(|t| match t {
            Value::String(_) => false,
            Value::Array(values) => values.is_empty() || values.iter().any(|v| !v.is_string()),
            _ => true,
        }) {
            self.boundaries[parent].unsupported = true;
        }
        if types.contains(&"object")
            || schema.get("properties").is_some()
            || schema.get("additionalProperties").is_some()
        {
            if let Some(properties) = schema.get("properties") {
                if let Some(properties) = properties.as_object() {
                    for (name, child) in properties {
                        let next = pos.child(
                            &format!("/properties/{}", escape(name)),
                            Some(PayloadStep::Property { name: name.clone() }),
                            parent,
                        );
                        self.json_schema(child, document, base, next);
                        if !self.limits.is_empty() {
                            return;
                        }
                    }
                } else {
                    self.boundaries[parent].unsupported = true;
                }
            }
            if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
                let keys = pos.child(
                    if schema.get("propertyNames").is_some() {
                        "/propertyNames"
                    } else {
                        ""
                    },
                    Some(PayloadStep::EachMapKey),
                    parent,
                );
                self.json_schema(
                    schema.get("propertyNames").unwrap_or(&Value::Bool(true)),
                    document,
                    base,
                    keys,
                );
                let values = pos.child(
                    if schema.get("additionalProperties").is_some() {
                        "/additionalProperties"
                    } else {
                        ""
                    },
                    Some(PayloadStep::EachMapValue),
                    parent,
                );
                self.json_schema(
                    schema
                        .get("additionalProperties")
                        .unwrap_or(&Value::Bool(true)),
                    document,
                    base,
                    values,
                );
            }
        }
        if types.contains(&"array") || schema.get("items").is_some() {
            let next = pos.child(
                if schema.get("items").is_some() {
                    "/items"
                } else {
                    ""
                },
                Some(PayloadStep::EachItem),
                parent,
            );
            self.json_schema(
                schema.get("items").unwrap_or(&Value::Bool(true)),
                document,
                base,
                next,
            );
        }
        let constrained_literal = schema.get("enum").is_some() || schema.get("const").is_some();
        if let Some(values) = schema.get("enum") {
            match values.as_array() {
                Some(values) if !values.is_empty() => {
                    if values.iter().any(|v| v.is_object() || v.is_array()) {
                        self.boundaries[parent].finding.opaque_interior = true;
                    }
                }
                _ => self.boundaries[parent].unsupported = true,
            }
        }
        if schema
            .get("const")
            .is_some_and(|v| v.is_object() || v.is_array())
        {
            self.boundaries[parent].finding.opaque_interior = true;
        }
        if !constrained_literal
            && (types.contains(&"string")
                || (types.is_empty() && !alternatives && schema.get("$ref").is_none()))
        {
            self.boundaries[parent].finding.opaque_interior = true;
        }
        if types.iter().any(|t| {
            !matches!(
                *t,
                "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
            )
        }) {
            self.boundaries[parent].unsupported = true;
        }
    }

    /// Read the exact serialized FieldType. Re-exporting via to_json_schema
    /// would change provenance and may discard protective extensions.
    fn field_type(&mut self, value: &Value, pos: Position) {
        if let Some(schema) = value.get("json_schema") {
            let mut pos = pos;
            pos.pointer.push_str("/json_schema");
            // Exact state schemas are self-contained; their references are
            // local to this embedded document, not to the whole PluginSchema.
            let base = pos.pointer.clone();
            self.json_schema(schema, schema, &base, pos);
            return;
        }
        let opaque = matches!(value.as_str(), Some("any" | "string"));
        let Some(parent) = self.add(&pos, opaque, false) else {
            return;
        };
        match value {
            Value::String(kind)
                if matches!(
                    kind.as_str(),
                    "string" | "integer" | "float" | "boolean" | "any"
                ) => {}
            Value::Object(map) if map.len() == 1 => {
                if let Some(fields) = map.get("object").and_then(Value::as_object) {
                    self.fields(fields, &pos, parent, "/object");
                } else if let Some(item) = map.get("array") {
                    self.field_type(
                        item,
                        pos.child("/array", Some(PayloadStep::EachItem), parent),
                    );
                } else if let Some(item) = map.get("map") {
                    self.add(
                        &pos.child("/map", Some(PayloadStep::EachMapKey), parent),
                        true,
                        false,
                    );
                    self.field_type(
                        item,
                        pos.child("/map", Some(PayloadStep::EachMapValue), parent),
                    );
                } else if let Some(branches) = map.get("one_of").and_then(Value::as_array) {
                    if branches.is_empty() {
                        self.boundaries[parent].unsupported = true;
                    }
                    for (i, branch) in branches.iter().enumerate() {
                        if self.branches >= MAX_BRANCHES {
                            self.limit(AssessmentLimit::Branches);
                            return;
                        }
                        self.branches += 1;
                        let mut next = pos.child(&format!("/one_of/{i}"), None, parent);
                        next.branches.push(next.pointer.clone());
                        self.field_type(branch, next);
                        if !self.limits.is_empty() {
                            return;
                        }
                    }
                } else if !map.get("enum").is_some_and(Value::is_array) {
                    self.boundaries[parent].unsupported = true;
                }
            }
            _ => self.boundaries[parent].unsupported = true,
        }
    }

    fn fields(
        &mut self,
        fields: &serde_json::Map<String, Value>,
        pos: &Position,
        parent: usize,
        prefix: &str,
    ) {
        for (name, field) in fields {
            let next = Self::protected(
                pos.child(
                    &format!("{prefix}/{}/field_type", escape(name)),
                    Some(PayloadStep::Property { name: name.clone() }),
                    parent,
                ),
                field,
            );
            self.field_type(field.get("field_type").unwrap_or(&Value::Null), next);
            if !self.limits.is_empty() {
                return;
            }
        }
        // Finite FieldSchema objects do not close additional runtime members.
        // The existing public projection filters them, independently of this
        // assessment. Their unknown keys/values remain opaque boundaries.
        self.add(
            &pos.child("", Some(PayloadStep::EachMapKey), parent),
            true,
            false,
        );
        self.add(
            &pos.child("", Some(PayloadStep::EachMapValue), parent),
            true,
            false,
        );
    }

    /// Metadata remains one explicitly opaque document boundary in this first
    /// resolver. No traversal of examples/defaults/help, and no implied payload
    /// mapping. Finer metadata coverage requires its own supported contract.
    fn metadata(&mut self) {
        if let Some(pos) = self.start(FieldRoot::DocumentMetadata) {
            self.add(&pos, true, false);
        }
    }
}

/// Pure candidate evaluation for a trusted producer/release preparation. The
/// caller verifies the candidate/producer digests. This does not authenticate
/// that caller, persist review, issue a seal receipt, or add runtime membership.
pub fn assess_candidate(
    source: FieldSourcePin,
    schema_json: &str,
    reviewed: Option<(&ReviewedFieldRules, u64)>,
) -> Result<FieldAssessment> {
    ensure!(
        matches!(source.source, FieldSource::Candidate { .. }),
        "candidate source pin is required"
    );
    assess(source, schema_json, reviewed)
}

/// Runtime entry point; only the bridge's manifest reader supplies these bytes
/// and pins. Never expose schema_json, rules, or source pins as caller inputs.
pub fn assess_sealed(
    source: FieldSourcePin,
    schema_json: &str,
    reviewed: Option<(&ReviewedFieldRules, u64)>,
) -> Result<FieldAssessment> {
    ensure!(
        matches!(source.source, FieldSource::Sealed { .. }),
        "sealed source pin is required"
    );
    assess(source, schema_json, reviewed)
}

fn assess(
    source: FieldSourcePin,
    schema_json: &str,
    reviewed: Option<(&ReviewedFieldRules, u64)>,
) -> Result<FieldAssessment> {
    validate_pin(&source)?;
    ensure!(
        schema_json.len() <= MAX_BYTES,
        "field schema exceeds byte bound"
    );
    // serde's recursion limit stays enabled. Parse failures are redacted and
    // never become an apparently empty/complete schema.
    let schema: Value = serde_json::from_str(schema_json)
        .map_err(|_| anyhow::anyhow!("invalid field schema document"))?;
    ensure!(
        schema.get("name").and_then(Value::as_str) == Some(&source.plugin_id)
            && schema.get("version").and_then(Value::as_str) == Some(&source.schema_version),
        "field schema source identity mismatch"
    );
    let fields = schema
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("field schema state root is missing"))?;
    ensure!(
        schema.get("methods").is_none_or(Value::is_object),
        "invalid field methods root"
    );
    ensure!(
        schema.get("signals").is_none_or(Value::is_array),
        "invalid field signals root"
    );
    let authority = if let Some((rules, revision)) = reviewed {
        rules.validate()?;
        ensure!(revision > 0, "reviewed field document revision is required");
        FieldAuthority::Reviewed {
            document_revision: revision,
            rule_revision: rules.revision,
            vocabulary_revision: rules.vocabulary_revision,
        }
    } else {
        FieldAuthority::Unavailable
    };
    let mut walker = Walker::default();
    if let Some(pos) = walker.start(FieldRoot::State) {
        if let Some(parent) = walker.add(&pos, false, false) {
            walker.fields(fields, &pos, parent, "");
        }
    }
    if let Some(methods) = schema.get("methods").and_then(Value::as_object) {
        for (key, method) in methods {
            ensure!(
                !key.is_empty() && key.len() <= 256,
                "invalid field method key"
            );
            for (root, member) in [
                (
                    FieldRoot::MethodInput {
                        method_key: key.clone(),
                    },
                    "args",
                ),
                (
                    FieldRoot::MethodResult {
                        method_key: key.clone(),
                    },
                    "returns",
                ),
            ] {
                if let Some(pos) = walker.start(root) {
                    if let Some(value) = method.get(member).filter(|v| !v.is_null()) {
                        walker.json_schema(value, value, "", pos);
                    } else {
                        walker.add(&pos, true, true);
                    }
                }
            }
            if !walker.limits.is_empty() {
                break;
            }
        }
    }
    if let Some(signals) = schema.get("signals").and_then(Value::as_array) {
        let mut seen = BTreeSet::new();
        for signal in signals {
            let name = signal
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("invalid field signal root"))?;
            ensure!(
                !name.is_empty() && name.len() <= 256 && seen.insert(name),
                "invalid or duplicate field signal root"
            );
            if let Some(pos) = walker.start(FieldRoot::SignalPayload {
                signal_name: name.to_owned(),
            }) {
                if let Some(value) = signal.get("payload").filter(|v| !v.is_null()) {
                    walker.json_schema(value, value, "", pos);
                } else {
                    walker.add(&pos, true, true);
                }
            }
            if !walker.limits.is_empty() {
                break;
            }
        }
    }
    walker.metadata();
    resolve(&mut walker, &source, reviewed.map(|(rules, _)| rules));
    let traversal_complete =
        walker.limits.is_empty() && !walker.boundaries.iter().any(|b| b.unsupported);
    let boundary_coverage_complete = reviewed.is_some()
        && traversal_complete
        && walker.rule_issues.is_empty()
        && walker.boundaries.iter().all(|b| {
            matches!(
                b.finding.resolution,
                FieldResolution::Resolved | FieldResolution::RestrictedOpaque
            )
        });
    let interior_inspection_complete =
        traversal_complete && walker.boundaries.iter().all(|b| !b.finding.opaque_interior);
    let mut output = FieldAssessment {
        source,
        authority,
        roots: walker.roots,
        findings: walker.boundaries.into_iter().map(|b| b.finding).collect(),
        rule_issues: walker.rule_issues,
        limits_hit: walker.limits,
        traversal_complete,
        boundary_coverage_complete,
        interior_inspection_complete,
        source_preservation_verified: false,
        grants_disclosure: false,
    };
    if encoded_size(&output, MAX_REPORT_BYTES).is_none() {
        // No truncated findings can masquerade as complete coverage. Do not
        // repeatedly serialize a huge report in an output-trimming loop.
        output.findings.clear();
        output.rule_issues.clear();
        output.limits_hit.push(AssessmentLimit::ReportBytes);
        output.traversal_complete = false;
        output.boundary_coverage_complete = false;
        output.interior_inspection_complete = false;
    }
    Ok(output)
}

fn resolve(walker: &mut Walker, source: &FieldSourcePin, reviewed: Option<&ReviewedFieldRules>) {
    let Some(reviewed) = reviewed else {
        for boundary in &mut walker.boundaries {
            if boundary.unsupported {
                boundary.finding.resolution = FieldResolution::Unsupported;
            }
        }
        return;
    };
    let classes: BTreeMap<_, _> = reviewed.classes.iter().map(|c| (&c.id, c)).collect();
    // Anchors are actual walked boundaries; a guessed pointer/selector never
    // gains subtree coverage. Ancestors retain reference and branch context.
    let mut anchors: Vec<(&FieldRule, BTreeSet<usize>)> = vec![];
    let mut checks = 0usize;
    for rule in reviewed
        .rules
        .iter()
        .filter(|r| r.source.plugin_id == source.plugin_id)
    {
        let mut indices = BTreeSet::new();
        for (index, boundary) in walker.boundaries.iter().enumerate() {
            checks += 1;
            if checks > MAX_RULE_CHECKS {
                walker.limit(AssessmentLimit::RuleChecks);
                return;
            }
            let finding = &boundary.finding;
            if finding.root == rule.root
                && finding.schema_pointer == rule.schema_pointer
                && finding.selector == rule.selector
            {
                indices.insert(index);
            }
        }
        let issue = if rule.source != *source {
            Some(FieldRuleIssueKind::StaleSource)
        } else if indices.is_empty() {
            Some(FieldRuleIssueKind::UnmatchedAnchor)
        } else {
            None
        };
        if let Some(kind) = issue {
            let issue = FieldRuleIssue {
                rule_id: rule.id.clone(),
                rule_revision: rule.revision,
                kind,
            };
            let Some(bytes) = encoded_size(
                &issue,
                MAX_REPORT_BYTES.saturating_sub(walker.report_bytes + 1),
            ) else {
                walker.limit(AssessmentLimit::ReportBytes);
                return;
            };
            walker.report_bytes += bytes + 1;
            walker.rule_issues.push(issue);
        }
        anchors.push((rule, indices));
    }
    for (index, boundary) in walker.boundaries.iter_mut().enumerate() {
        let mut applicable = BTreeSet::new();
        let mut stale = false;
        let original_floor = boundary.finding.protection_floor;
        for (rule, indices) in &anchors {
            // Set membership avoids a scan through every duplicate anchor.
            // Charge ancestry comparisons too (at most MAX_DEPTH per node).
            checks += (1 + boundary.ancestors.len()) * 16;
            if checks > MAX_RULE_CHECKS {
                walker.limits.push(AssessmentLimit::RuleChecks);
                return;
            }
            let exact = indices.contains(&index);
            if !exact
                && !(rule.scope == FieldRuleScope::Subtree
                    && boundary.ancestors.iter().any(|i| indices.contains(i)))
            {
                continue;
            }
            if !rule.include_map_keys
                && boundary.finding.selector.as_ref().is_some_and(|selector| {
                    selector
                        .iter()
                        .any(|step| matches!(step, PayloadStep::EachMapKey))
                })
            {
                continue;
            }
            if rule.source != *source {
                stale = true;
                continue;
            }
            let class = classes[&rule.class_id];
            let evidence = RuleEvidence {
                rule_id: rule.id.clone(),
                rule_revision: rule.revision,
                class_id: class.id.clone(),
                class_revision: class.revision,
            };
            let Some(evidence_bytes) = encoded_size(
                &evidence,
                MAX_REPORT_BYTES.saturating_sub(walker.report_bytes + 1),
            ) else {
                walker.limits.push(AssessmentLimit::ReportBytes);
                return;
            };
            walker.report_bytes += evidence_bytes + 1;
            applicable.insert(class.id.clone());
            boundary.finding.protection_floor = boundary
                .finding
                .protection_floor
                .max(class.minimum_protection);
            boundary.finding.rules.push(evidence);
        }
        boundary.finding.resolution = if boundary.unsupported {
            FieldResolution::Unsupported
        } else if stale {
            FieldResolution::Stale
        } else if applicable.len() > 1
            || boundary
                .finding
                .rules
                .iter()
                .any(|r| classes[&r.class_id].minimum_protection < original_floor)
        {
            FieldResolution::Conflict
        } else if applicable.is_empty() {
            FieldResolution::Uncovered
        } else if boundary.finding.opaque_interior {
            if boundary.finding.protection_floor > ProtectionFloor::None {
                FieldResolution::RestrictedOpaque
            } else {
                FieldResolution::Uncovered
            }
        } else {
            FieldResolution::Resolved
        };
    }
    // Alternatives may overlap at payload time. Keep the strongest floor and
    // report conflicting classes across the same selector; never pick a first
    // permissive branch. This is schema coverage, not payload validation.
    let mut alternatives = BTreeMap::<
        (FieldRoot, Option<Vec<PayloadStep>>),
        (ProtectionFloor, BTreeSet<String>),
    >::new();
    for boundary in &walker.boundaries {
        if boundary.finding.branch_chain.is_empty() {
            continue;
        }
        let group = alternatives
            .entry((
                boundary.finding.root.clone(),
                boundary.finding.selector.clone(),
            ))
            .or_insert((ProtectionFloor::None, BTreeSet::new()));
        group.0 = group.0.max(boundary.finding.protection_floor);
        group
            .1
            .extend(boundary.finding.rules.iter().map(|r| r.class_id.clone()));
    }
    for boundary in &mut walker.boundaries {
        if let Some((floor, classes)) = alternatives.get(&(
            boundary.finding.root.clone(),
            boundary.finding.selector.clone(),
        )) {
            boundary.finding.protection_floor = boundary.finding.protection_floor.max(*floor);
            if classes.len() > 1
                && matches!(
                    boundary.finding.resolution,
                    FieldResolution::Resolved | FieldResolution::RestrictedOpaque
                )
            {
                boundary.finding.resolution = FieldResolution::Conflict;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProtectionLoss {
    pub root: FieldRoot,
    pub selector: Option<Vec<PayloadStep>>,
    pub required_floor: ProtectionFloor,
}

/// Compare effective floors across two assessable PluginSchema representations.
/// Raw producer annotations lost before either input are outside this evidence.
/// Refuse incomplete or differently reviewed inputs. This is only a loss
/// detector, not the as-yet-unwired sealer gate or a public-projection review.
pub fn protection_losses(
    before: &FieldAssessment,
    after: &FieldAssessment,
) -> Result<Vec<ProtectionLoss>> {
    ensure!(
        before.source.plugin_id == after.source.plugin_id && before.authority == after.authority,
        "protection comparison requires the same subject and reviewed authority"
    );
    ensure!(
        before.traversal_complete && after.traversal_complete,
        "protection comparison requires complete traversal"
    );
    ensure!(
        before.boundary_coverage_complete && after.boundary_coverage_complete,
        "protection comparison requires complete reviewed boundary coverage"
    );
    let mut required = BTreeMap::<(FieldRoot, Option<Vec<PayloadStep>>), ProtectionFloor>::new();
    let mut retained = BTreeMap::<(FieldRoot, Option<Vec<PayloadStep>>), ProtectionFloor>::new();
    for finding in &before.findings {
        let floor = required
            .entry((finding.root.clone(), finding.selector.clone()))
            .or_insert(ProtectionFloor::None);
        *floor = (*floor).max(finding.protection_floor);
    }
    for finding in &after.findings {
        // Be conservative across overlapping branches/reference sites: one
        // retained restrictive branch cannot hide a newly permissive branch.
        // An unresolved wrapper can therefore require further review even if
        // its referenced leaf still carries a protective marker.
        let floor = retained
            .entry((finding.root.clone(), finding.selector.clone()))
            .or_insert(finding.protection_floor);
        *floor = (*floor).min(finding.protection_floor);
    }
    Ok(required
        .into_iter()
        .filter_map(|((root, selector), floor)| {
            (floor > ProtectionFloor::None
                && retained
                    .get(&(root.clone(), selector.clone()))
                    .copied()
                    .unwrap_or(ProtectionFloor::None)
                    < floor)
                .then_some(ProtectionLoss {
                    root,
                    selector,
                    required_floor: floor,
                })
        })
        .collect())
}

#[cfg(test)]
#[path = "oscal_field_classification_tests.rs"]
mod tests;
