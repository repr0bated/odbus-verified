//! Namespace-scoped, evidence-bearing system inventory snapshots. The typed
//! snapshot is the import contract, not a second plugin schema or live scanner.
//! Fixed read queries never execute caller-supplied CozoScript.
use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

pub const SYSTEM_GRAPH_FORMAT_VERSION: u32 = 1;
pub const MAX_SYSTEM_GRAPH_NODES: usize = 50_000;
pub const MAX_SYSTEM_GRAPH_EDGES: usize = 200_000;
pub const MAX_SYSTEM_GRAPH_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SYSTEM_GRAPH_PAGE: u32 = 1_000;
pub const MAX_SYSTEM_GRAPH_NAMESPACES: usize = 256;
pub const MAX_SYSTEM_GRAPH_RESULT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROPERTIES: usize = 16 * 1024;
const NODE_COLUMNS: &str = "namespace, id, kind, label, attributes";
const EDGE_COLUMNS: &str = "namespace, src, relation, dst, evidence, attributes";

type GraphResult<T> = std::result::Result<T, CozoError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphProvenanceKind {
    Source,
    PreparedRelease,
    ObservedRuntime,
}
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum GraphAssertion {
    Declared,
    Observed,
    Inferred,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphEvidence {
    pub source: String,
    pub locator: String,
    pub assertion: GraphAssertion,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphProvenance {
    pub kind: GraphProvenanceKind,
    pub source_commit: String,
    pub release_id: Option<String>,
    pub generated_at: String,
    pub inputs: BTreeMap<String, String>,
    pub coverage: Vec<String>,
    pub limitations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub attributes: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub src: String,
    pub relation: String,
    pub dst: String,
    pub evidence: Vec<GraphEvidence>,
    pub attributes: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphSnapshot {
    pub format_version: u32,
    pub namespace: String,
    pub provenance: GraphProvenance,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphSnapshotReceipt {
    pub namespace: String,
    pub revision: u64,
    pub digest: String,
    pub node_count: u64,
    pub edge_count: u64,
    pub provenance: GraphProvenance,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphNamespaceQuery {
    pub after_namespace: Option<String>,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphNamespacePage {
    pub snapshots: Vec<GraphSnapshotReceipt>,
    pub next_after_namespace: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    Outgoing,
    Incoming,
    Both,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphNodeQuery {
    pub namespace: String,
    pub revision: Option<u64>,
    pub after_id: Option<String>,
    pub kind: Option<String>,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphEdgeKey {
    pub src: String,
    pub relation: String,
    pub dst: String,
}
impl From<&GraphEdge> for GraphEdgeKey {
    fn from(edge: &GraphEdge) -> Self {
        Self {
            src: edge.src.clone(),
            relation: edge.relation.clone(),
            dst: edge.dst.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphEdgeQuery {
    pub namespace: String,
    pub revision: Option<u64>,
    pub after: Option<GraphEdgeKey>,
    pub node_id: Option<String>,
    pub direction: GraphDirection,
    pub relation: Option<String>,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphNodePage {
    pub snapshot: GraphSnapshotReceipt,
    pub nodes: Vec<GraphNode>,
    pub next_after_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphEdgePage {
    pub snapshot: GraphSnapshotReceipt,
    pub edges: Vec<GraphEdge>,
    pub next_after: Option<GraphEdgeKey>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphTraversalQuery {
    pub namespace: String,
    pub revision: Option<u64>,
    pub start: String,
    pub direction: GraphDirection,
    pub relations: Vec<String>,
    pub max_depth: u32,
    pub max_nodes: u32,
    pub max_edges: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphReachedNode {
    pub id: String,
    pub depth: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphTraversal {
    pub snapshot: GraphSnapshotReceipt,
    pub nodes: Vec<GraphReachedNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
    pub truncation_reasons: Vec<String>,
}

fn error(message: impl Into<String>) -> CozoError {
    CozoError::Other(message.into())
}
fn text(value: &str) -> DataValue {
    DataValue::Str(value.into())
}
fn check_text(value: &str, max: usize, name: &str) -> GraphResult<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(error(format!("invalid system graph {name}")))
    } else {
        Ok(())
    }
}
fn optional_text(value: Option<&str>, name: &str) -> GraphResult<()> {
    if let Some(value) = value {
        check_text(value, 512, name)?;
    }
    Ok(())
}
fn encode(value: &impl Serialize) -> GraphResult<String> {
    Ok(serde_json::to_string(value)?)
}
fn run(tx: &MultiTransaction, script: &str, params: Params) -> GraphResult<NamedRows> {
    tx.run_script(script, params)
        .map_err(|e| error(format!("system graph: {e}")))
}
fn cell(row: &[DataValue], index: usize) -> GraphResult<&str> {
    row.get(index)
        .and_then(dv_as_str)
        .ok_or_else(|| error("system graph row has incompatible columns"))
}
fn canonical(snapshot: &GraphSnapshot) -> GraphResult<GraphSnapshot> {
    if snapshot.format_version != SYSTEM_GRAPH_FORMAT_VERSION {
        return Err(error("unsupported system graph format"));
    }
    check_text(&snapshot.namespace, 512, "namespace")?;
    if snapshot.nodes.len() > MAX_SYSTEM_GRAPH_NODES
        || snapshot.edges.len() > MAX_SYSTEM_GRAPH_EDGES
    {
        return Err(error("system graph snapshot cardinality exceeds bounds"));
    }
    let provenance = &snapshot.provenance;
    check_text(&provenance.source_commit, 512, "source commit")?;
    optional_text(provenance.release_id.as_deref(), "release id")?;
    if provenance.kind == GraphProvenanceKind::PreparedRelease && provenance.release_id.is_none() {
        return Err(error("prepared release graph requires a release id"));
    }
    chrono::DateTime::parse_from_rfc3339(&provenance.generated_at)
        .map_err(|_| error("system graph generation time must be RFC3339"))?;
    if provenance.inputs.len() > 256
        || provenance.coverage.len() > 256
        || provenance.limitations.len() > 256
    {
        return Err(error("system graph provenance exceeds bounds"));
    }
    for (name, value) in &provenance.inputs {
        check_text(name, 512, "input name")?;
        check_text(value, 1024, "input value")?;
    }
    for value in provenance.coverage.iter().chain(&provenance.limitations) {
        check_text(value, 2048, "coverage/limitation")?;
    }
    let mut bytes = encode(provenance)?.len();
    if bytes > 128 * 1024 {
        return Err(error("system graph provenance bytes exceed bounds"));
    }
    let mut ids = BTreeSet::new();
    for node in &snapshot.nodes {
        check_text(&node.id, 512, "node id")?;
        check_text(&node.kind, 128, "node kind")?;
        check_text(&node.label, 1024, "node label")?;
        if !ids.insert(node.id.as_str()) {
            return Err(error("duplicate system graph node id"));
        }
        if encode(&node.attributes)?.len() > MAX_PROPERTIES {
            return Err(error("system graph node attributes exceed bounds"));
        }
        bytes += encode(node)?.len();
        if bytes > MAX_SYSTEM_GRAPH_BYTES {
            return Err(error("system graph snapshot bytes exceed bounds"));
        }
    }
    let mut keys = BTreeSet::new();
    for edge in &snapshot.edges {
        check_text(&edge.src, 512, "edge source")?;
        check_text(&edge.dst, 512, "edge target")?;
        check_text(&edge.relation, 128, "edge relation")?;
        if !ids.contains(edge.src.as_str()) || !ids.contains(edge.dst.as_str()) {
            return Err(error("dangling system graph edge"));
        }
        if !keys.insert(GraphEdgeKey::from(edge)) {
            return Err(error("duplicate system graph edge key"));
        }
        if edge.evidence.is_empty() || edge.evidence.len() > 32 {
            return Err(error("system graph edges require 1..=32 evidence records"));
        }
        for evidence in &edge.evidence {
            check_text(&evidence.source, 512, "evidence source")?;
            check_text(&evidence.locator, 2048, "evidence locator")?;
        }
        if encode(&edge.attributes)?.len() > MAX_PROPERTIES {
            return Err(error("system graph edge attributes exceed bounds"));
        }
        bytes += encode(edge)?.len();
        if bytes > MAX_SYSTEM_GRAPH_BYTES {
            return Err(error("system graph snapshot bytes exceed bounds"));
        }
    }
    let mut snapshot = snapshot.clone();
    snapshot.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.edges.sort_by_key(|edge| GraphEdgeKey::from(edge));
    for node in &mut snapshot.nodes {
        for value in node.attributes.values_mut() {
            canonical_value(value);
        }
    }
    for edge in &mut snapshot.edges {
        edge.evidence.sort();
        edge.evidence.dedup();
        for value in edge.attributes.values_mut() {
            canonical_value(value);
        }
    }
    snapshot.provenance.coverage.sort();
    snapshot.provenance.coverage.dedup();
    snapshot.provenance.limitations.sort();
    snapshot.provenance.limitations.dedup();
    if encode(&snapshot)?.len() > MAX_SYSTEM_GRAPH_BYTES {
        return Err(error("system graph snapshot bytes exceed bounds"));
    }
    Ok(snapshot)
}
fn canonical_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for value in object.values_mut() {
                canonical_value(value);
            }
            let sorted: BTreeMap<_, _> = std::mem::take(object).into_iter().collect();
            *object = sorted.into_iter().collect();
        }
        Value::Array(array) => {
            for value in array {
                canonical_value(value);
            }
        }
        _ => {}
    }
}
/// Digest of validated canonical snapshot bytes, including provenance. Array
/// order of nodes, edges, evidence and coverage does not alter the digest.
pub fn system_graph_snapshot_digest(snapshot: &GraphSnapshot) -> GraphResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(encode(&canonical(snapshot)?)?.as_bytes())
    ))
}
fn status(tx: &MultiTransaction, namespace: &str) -> GraphResult<Option<GraphSnapshotReceipt>> {
    let rows = run(
        tx,
        "?[receipt] := *system_graph_state{namespace, receipt}, namespace = $namespace :limit 1 :timeout 1",
        BTreeMap::from([("namespace".into(), text(namespace))]),
    )?;
    rows.rows
        .first()
        .map(|row| serde_json::from_str(cell(row, 0)?).map_err(CozoError::from))
        .transpose()
}
fn require_status(
    tx: &MultiTransaction,
    namespace: &str,
    revision: Option<u64>,
) -> GraphResult<GraphSnapshotReceipt> {
    let receipt =
        status(tx, namespace)?.ok_or_else(|| error("system graph namespace not found"))?;
    if revision.is_some_and(|revision| revision != receipt.revision) {
        return Err(error("system graph revision conflict"));
    }
    Ok(receipt)
}
fn page_limit(limit: u32) -> GraphResult<()> {
    if !(1..=MAX_SYSTEM_GRAPH_PAGE).contains(&limit) {
        return Err(error("system graph page limit must be 1..=1000"));
    }
    Ok(())
}
fn bound_page<T: Serialize>(
    entries: &mut Vec<T>,
    limit: u32,
    base_bytes: usize,
) -> GraphResult<bool> {
    let mut bytes = base_bytes + 4096;
    let mut count = 0;
    for entry in entries.iter().take(limit as usize) {
        let size = encode(entry)?.len() + 1;
        if bytes + size > MAX_SYSTEM_GRAPH_RESULT_BYTES {
            break;
        }
        bytes += size;
        count += 1;
    }
    if count == 0 && !entries.is_empty() {
        return Err(error("system graph record exceeds response byte bound"));
    }
    let more = count < entries.len();
    entries.truncate(count);
    Ok(more)
}
fn node_from_row(row: &[DataValue]) -> GraphResult<GraphNode> {
    Ok(GraphNode {
        id: cell(row, 0)?.into(),
        kind: cell(row, 1)?.into(),
        label: cell(row, 2)?.into(),
        attributes: serde_json::from_str(cell(row, 3)?)?,
    })
}
fn edge_from_row(row: &[DataValue]) -> GraphResult<GraphEdge> {
    Ok(GraphEdge {
        src: cell(row, 0)?.into(),
        relation: cell(row, 1)?.into(),
        dst: cell(row, 2)?.into(),
        evidence: serde_json::from_str(cell(row, 3)?)?,
        attributes: serde_json::from_str(cell(row, 4)?)?,
    })
}
fn read_edges(
    tx: &MultiTransaction,
    query: &GraphEdgeQuery,
    limit: u32,
    allowed: &[String],
) -> GraphResult<Vec<GraphEdge>> {
    let mut clauses = vec!["namespace = $namespace".to_string()];
    let mut params = BTreeMap::from([("namespace".into(), text(&query.namespace))]);
    if let Some(after) = &query.after {
        clauses.push("[src, relation, dst] > $after".into());
        params.insert(
            "after".into(),
            DataValue::List(vec![
                text(&after.src),
                text(&after.relation),
                text(&after.dst),
            ]),
        );
    }
    if let Some(node) = &query.node_id {
        clauses.push(
            match query.direction {
                GraphDirection::Outgoing => "src = $node",
                GraphDirection::Incoming => "dst = $node",
                GraphDirection::Both => "(src == $node || dst == $node)",
            }
            .into(),
        );
        params.insert("node".into(), text(node));
    }
    if let Some(relation) = &query.relation {
        clauses.push("relation = $relation".into());
        params.insert("relation".into(), text(relation));
    }
    if !allowed.is_empty() {
        clauses.push("is_in(relation, $allowed)".into());
        params.insert(
            "allowed".into(),
            DataValue::List(allowed.iter().map(|v| text(v)).collect()),
        );
    }
    let script = format!(
        "?[src, relation, dst, evidence, attributes] := *system_graph_edges{{{EDGE_COLUMNS}}}, {} :order src, relation, dst :limit {limit} :timeout 1",
        clauses.join(", ")
    );
    run(tx, &script, params)?
        .rows
        .iter()
        .map(|row| edge_from_row(row))
        .collect()
}

impl CozoGraphShuttle {
    pub(crate) fn initialize_system_graph_schema(&self) -> GraphResult<()> {
        for script in [
            ":create system_graph_state {namespace: String => receipt: String}",
            ":create system_graph_nodes {namespace: String, id: String => kind: String, label: String, attributes: String}",
            ":create system_graph_edges {namespace: String, src: String, relation: String, dst: String => evidence: String, attributes: String}",
        ] {
            if let Err(e) = self
                .db
                .run_script(script, BTreeMap::new(), ScriptMutability::Mutable)
            {
                let message = e.to_string();
                if !message.contains("already exists")
                    && !message.contains("AlreadyExists")
                    && !message.contains("conflicts with an existing")
                {
                    return Err(error(format!(
                        "system graph schema initialization: {message}"
                    )));
                }
            }
        }
        for script in [
            "?[namespace,receipt] := *system_graph_state{namespace,receipt} :limit 0".to_string(),
            format!("?[{NODE_COLUMNS}] := *system_graph_nodes{{{NODE_COLUMNS}}} :limit 0"),
            format!("?[{EDGE_COLUMNS}] := *system_graph_edges{{{EDGE_COLUMNS}}} :limit 0"),
        ] {
            self.db
                .run_script(&script, BTreeMap::new(), ScriptMutability::Immutable)
                .map_err(|e| error(format!("system graph schema verification: {e}")))?;
        }
        Ok(())
    }
    fn system_graph_read<T>(
        &self,
        body: impl FnOnce(&MultiTransaction) -> GraphResult<T>,
    ) -> GraphResult<T> {
        let tx = self.db.multi_transaction(false);
        let result = body(&tx);
        let _ = tx.abort();
        result
    }
    /// Replace exactly one namespace in one checked native transaction. A
    /// persistence-barrier failure returns an error (no durable receipt); read
    /// the current namespace before retrying an uncertain commit.
    pub fn replace_system_graph(
        &self,
        snapshot: &GraphSnapshot,
        expected_revision: u64,
    ) -> GraphResult<GraphSnapshotReceipt> {
        let snapshot = canonical(snapshot)?;
        let digest = format!("{:x}", Sha256::digest(encode(&snapshot)?.as_bytes()));
        let _guard = self
            .system_graph_transaction
            .lock()
            .map_err(|_| error("system graph owner poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let current = status(&tx, &snapshot.namespace)?;
        if current.as_ref().map_or(0, |r| r.revision) != expected_revision {
            return Err(error("system graph revision conflict"));
        }
        if current
            .as_ref()
            .is_some_and(|r| r.provenance.kind != snapshot.provenance.kind)
        {
            return Err(error(
                "system graph namespace provenance kind cannot change",
            ));
        }
        if current.as_ref().is_some_and(|r| {
            r.provenance.kind == GraphProvenanceKind::PreparedRelease
                && r.provenance.release_id != snapshot.provenance.release_id
        }) {
            return Err(error(
                "prepared release graph namespace release id cannot change",
            ));
        }
        if current.is_none() {
            let namespaces = run(
                &tx,
                "?[namespace] := *system_graph_state{namespace} :limit 256 :timeout 1",
                BTreeMap::new(),
            )?;
            if namespaces.rows.len() >= MAX_SYSTEM_GRAPH_NAMESPACES {
                return Err(error(
                    "system graph namespace capacity reached; no automatic history deletion",
                ));
            }
        }
        let revision = expected_revision
            .checked_add(1)
            .filter(|r| *r <= i64::MAX as u64)
            .ok_or_else(|| error("system graph revision exhausted"))?;
        let receipt = GraphSnapshotReceipt {
            namespace: snapshot.namespace.clone(),
            revision,
            digest,
            node_count: snapshot.nodes.len() as u64,
            edge_count: snapshot.edges.len() as u64,
            provenance: snapshot.provenance.clone(),
        };
        let params = BTreeMap::from([("namespace".into(), text(&snapshot.namespace))]);
        run(
            &tx,
            "?[namespace,id] := *system_graph_nodes{namespace,id}, namespace=$namespace :rm system_graph_nodes {namespace,id}",
            params.clone(),
        )?;
        run(
            &tx,
            "?[namespace,src,relation,dst] := *system_graph_edges{namespace,src,relation,dst}, namespace=$namespace :rm system_graph_edges {namespace,src,relation,dst}",
            params,
        )?;
        for chunk in snapshot.nodes.chunks(200) {
            let rows = chunk
                .iter()
                .map(|n| {
                    Ok(DataValue::List(vec![
                        text(&snapshot.namespace),
                        text(&n.id),
                        text(&n.kind),
                        text(&n.label),
                        text(&encode(&n.attributes)?),
                    ]))
                })
                .collect::<GraphResult<Vec<_>>>()?;
            run(
                &tx,
                &format!("?[{NODE_COLUMNS}] <- $rows :put system_graph_nodes {{{NODE_COLUMNS}}}"),
                BTreeMap::from([("rows".into(), DataValue::List(rows))]),
            )?;
        }
        for chunk in snapshot.edges.chunks(200) {
            let rows = chunk
                .iter()
                .map(|e| {
                    Ok(DataValue::List(vec![
                        text(&snapshot.namespace),
                        text(&e.src),
                        text(&e.relation),
                        text(&e.dst),
                        text(&encode(&e.evidence)?),
                        text(&encode(&e.attributes)?),
                    ]))
                })
                .collect::<GraphResult<Vec<_>>>()?;
            run(
                &tx,
                &format!("?[{EDGE_COLUMNS}] <- $rows :put system_graph_edges {{{EDGE_COLUMNS}}}"),
                BTreeMap::from([("rows".into(), DataValue::List(rows))]),
            )?;
        }
        run(
            &tx,
            "?[namespace,receipt] <- [[$namespace,$receipt]] :put system_graph_state {namespace=>receipt}",
            BTreeMap::from([
                ("namespace".into(), text(&snapshot.namespace)),
                ("receipt".into(), text(&encode(&receipt)?)),
            ]),
        )?;
        #[cfg(test)]
        if snapshot.namespace == "test:rollback-before-commit" && expected_revision > 0 {
            return Err(error("injected system graph precommit failure"));
        }
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok(receipt)
    }
    pub fn system_graph_status(
        &self,
        namespace: &str,
    ) -> GraphResult<Option<GraphSnapshotReceipt>> {
        check_text(namespace, 512, "namespace")?;
        self.system_graph_read(|tx| status(tx, namespace))
    }
    /// Each page is transaction-consistent. This cursor orders namespaces,
    /// not a frozen multi-page catalog: concurrent new namespaces before the
    /// cursor require starting discovery again. Snapshots themselves carry
    /// individual revisions for subsequent pinned reads.
    pub fn list_system_graphs(
        &self,
        query: &GraphNamespaceQuery,
    ) -> GraphResult<GraphNamespacePage> {
        page_limit(query.limit)?;
        optional_text(query.after_namespace.as_deref(), "namespace cursor")?;
        self.system_graph_read(|tx|{
            let mut params=BTreeMap::new();
            let filter=if let Some(after)=&query.after_namespace {params.insert("after".into(),text(after));", namespace > $after"}else{""};
            let script=format!("?[namespace,receipt] := *system_graph_state{{namespace,receipt}}{filter} :order namespace :limit {} :timeout 1",query.limit+1);
            let rows=run(tx,&script,params)?;
            let mut snapshots=rows.rows.iter().map(|row|serde_json::from_str(cell(row,1)?).map_err(CozoError::from)).collect::<GraphResult<Vec<GraphSnapshotReceipt>>>()?;
            let more=bound_page(&mut snapshots,query.limit,128)?;
            let next_after_namespace=if more {snapshots.last().map(|r|r.namespace.clone())}else{None};
            // Typed decode is the only source of public metadata.
            Ok(GraphNamespacePage{snapshots:std::mem::take(&mut snapshots),next_after_namespace})
        })
    }
    pub fn query_system_graph_nodes(&self, query: &GraphNodeQuery) -> GraphResult<GraphNodePage> {
        check_text(&query.namespace, 512, "namespace")?;
        page_limit(query.limit)?;
        optional_text(query.after_id.as_deref(), "node cursor")?;
        optional_text(query.kind.as_deref(), "node kind")?;
        if query.after_id.is_some() && query.revision.is_none() {
            return Err(error("system graph cursor requires revision"));
        }
        self.system_graph_read(|tx|{
            let snapshot=require_status(tx,&query.namespace,query.revision)?;
            let mut clauses=vec!["namespace=$namespace"];
            let mut params=BTreeMap::from([("namespace".into(),text(&query.namespace))]);
            if let Some(after)=&query.after_id {clauses.push("id > $after");params.insert("after".into(),text(after));}
            if let Some(kind)=&query.kind {clauses.push("kind=$kind");params.insert("kind".into(),text(kind));}
            let script=format!("?[id,kind,label,attributes] := *system_graph_nodes{{{NODE_COLUMNS}}}, {} :order id :limit {} :timeout 1",clauses.join(", "),query.limit+1);
            let mut nodes=run(tx,&script,params)?.rows.iter().map(|row|node_from_row(row)).collect::<GraphResult<Vec<_>>>()?;
            let more=bound_page(&mut nodes,query.limit,encode(&snapshot)?.len())?;
            let next_after_id=if more {nodes.last().map(|n|n.id.clone())}else{None};
            Ok(GraphNodePage{snapshot,nodes,next_after_id})
        })
    }
    pub fn query_system_graph_edges(&self, query: &GraphEdgeQuery) -> GraphResult<GraphEdgePage> {
        check_text(&query.namespace, 512, "namespace")?;
        page_limit(query.limit)?;
        optional_text(query.node_id.as_deref(), "node id")?;
        optional_text(query.relation.as_deref(), "relation")?;
        if let Some(after) = &query.after {
            check_text(&after.src, 512, "edge cursor")?;
            check_text(&after.relation, 128, "edge cursor")?;
            check_text(&after.dst, 512, "edge cursor")?;
            if query.revision.is_none() {
                return Err(error("system graph cursor requires revision"));
            }
        }
        self.system_graph_read(|tx| {
            let snapshot = require_status(tx, &query.namespace, query.revision)?;
            let mut edges = read_edges(tx, query, query.limit + 1, &[])?;
            let more = bound_page(&mut edges, query.limit, encode(&snapshot)?.len())?;
            let next_after = if more {
                edges.last().map(GraphEdgeKey::from)
            } else {
                None
            };
            Ok(GraphEdgePage {
                snapshot,
                edges,
                next_after,
            })
        })
    }
    /// Breadth-first dependencies or reverse impact, with visited-node/edge
    /// deduplication and fixed-size adjacency reads. Bounds apply to work and
    /// encoded output, not only the final number of nodes.
    pub fn traverse_system_graph(
        &self,
        query: &GraphTraversalQuery,
    ) -> GraphResult<GraphTraversal> {
        check_text(&query.namespace, 512, "namespace")?;
        check_text(&query.start, 512, "start node")?;
        if query.max_depth > 32
            || !(1..=1000).contains(&query.max_nodes)
            || !(1..=5000).contains(&query.max_edges)
            || query.relations.len() > 32
        {
            return Err(error("system graph traversal bounds exceeded"));
        }
        for relation in &query.relations {
            check_text(relation, 128, "relation")?;
        }
        self.system_graph_read(|tx| {
            let snapshot = require_status(tx, &query.namespace, query.revision)?;
            let exists = run(tx,
                "?[id] := *system_graph_nodes{namespace,id},namespace=$namespace,id=$id :limit 1 :timeout 1",
                BTreeMap::from([("namespace".into(),text(&query.namespace)),("id".into(),text(&query.start))]))?;
            if exists.rows.is_empty() { return Err(error("system graph start node not found")); }
            let mut visited = BTreeSet::from([query.start.clone()]);
            let mut queue = VecDeque::from([(query.start.clone(),0)]);
            let mut nodes = vec![GraphReachedNode{id:query.start.clone(),depth:0}];
            let mut edges = BTreeMap::new();
            let mut reasons = BTreeSet::new();
            let mut bytes = encode(&snapshot)?.len() + encode(&nodes)?.len() + 4096;
            let mut scanned = 0;
            let deadline = Instant::now() + Duration::from_secs(5);
            'walk: while let Some((node,depth)) = queue.pop_front() {
                let mut edge_query = GraphEdgeQuery {namespace:query.namespace.clone(),revision:Some(snapshot.revision),after:None,node_id:Some(node.clone()),direction:query.direction,relation:None,limit:32};
                loop {
                    if Instant::now() >= deadline {reasons.insert("time_limit".to_string());break 'walk;}
                    let adjacent = read_edges(tx,&edge_query,32,&query.relations)?;
                    let full = adjacent.len()==32;
                    edge_query.after = adjacent.last().map(GraphEdgeKey::from);
                    for edge in adjacent {
                        scanned += 1;
                        if scanned > 20_000 {reasons.insert("scan_limit".to_string());break 'walk;}
                        let next = if edge.src==node {edge.dst.clone()} else {edge.src.clone()};
                        if depth==query.max_depth {
                            if !visited.contains(&next) {reasons.insert("depth_limit".to_string());break;}
                            continue;
                        }
                        let key = GraphEdgeKey::from(&edge);
                        if edges.contains_key(&key) {continue;}
                        if edges.len() >= query.max_edges as usize {reasons.insert("edge_limit".to_string());break 'walk;}
                        let reached = GraphReachedNode{id:next.clone(),depth:depth+1};
                        let new_node = !visited.contains(&next);
                        if new_node && nodes.len() >= query.max_nodes as usize {reasons.insert("node_limit".to_string());continue;}
                        let size = encode(&edge)?.len()+1 + if new_node {encode(&reached)?.len()+1} else {0};
                        if bytes+size > MAX_SYSTEM_GRAPH_RESULT_BYTES {reasons.insert("byte_limit".to_string());break 'walk;}
                        bytes += size;
                        if new_node {visited.insert(next.clone());nodes.push(reached);queue.push_back((next,depth+1));}
                        edges.insert(key,edge);
                    }
                    if !full || (depth==query.max_depth && reasons.contains("depth_limit")) {break;}
                }
            }
            Ok(GraphTraversal{snapshot,nodes,edges:edges.into_values().collect(),truncated:!reasons.is_empty(),truncation_reasons:reasons.into_iter().collect()})
        })
    }
}

#[cfg(test)]
#[path = "system_graph_tests.rs"]
mod tests;
