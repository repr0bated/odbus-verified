//! Generic Cozo methods operate on the caller's subsidiary store, or the
//! existing cognitive store for other callers. Identity, runtime documents
//! and the bridge's durable audit ledger remain distinct owners.
use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, ensure};
use op_cozo_store::{CozoGraphShuttle, GraphProvenance, GraphProvenanceKind, GraphSnapshot};
use op_plugins::state_plugins::cozo::{
    self, CozoColumn, CozoRelation, CozoTrigger,
    methods::{ListRelationsOutput, RelationDescription},
};
use op_state_store::SideEffect;
use serde_json::{Value, json};

use crate::{
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, RuntimeError},
    sealed_schema_reader::read_manifest_pinned_schema_document,
};

#[path = "system_graph_projection.rs"]
mod graph_projection;

const METHODS: &[&str] = &[
    "run_query",
    "import_relations",
    "export_relations",
    "backup",
    "restore",
    "list_relations",
    "describe_relation",
    "list_system_graphs",
    "replace_system_graph",
    "refresh_system_graph",
    "system_graph_status",
    "query_system_graph_nodes",
    "query_system_graph_edges",
    "traverse_system_graph",
];
const GRAPH_STATE_PLUGINS: &[&str] = &[
    "incus",
    "btrfs",
    "qdrant",
    "mail_server",
    "cognitive",
    "memory",
    "embedding_model",
    "oscal_subid_registry",
    "json_render",
    "model_registry",
    "unix_socket",
    "shared_unix_socket",
    "ovsdb_bridge",
];

/// Personal memory and soul live in the cognitive store this generic plugin
/// opens (owner rule 2026-11-22: personal chat and soul never leave the
/// personal store). Raw Datalog, relation archives and imports therefore never
/// name these relations; the scoped cognitive memory methods remain their only
/// readers and writers.
pub(crate) const PERSONAL_RELATIONS: &[&str] = &[
    "memory_entries",
    "memory_namespaces",
    "soul_memories",
    "agent_namespace_bindings",
];

fn names_personal_relation(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| PERSONAL_RELATIONS.contains(&word))
}

/// Cozo's reader rules open any path as the bridge's own host user.
fn refuse_file_readers(method: &str, args: &Value) -> Result<()> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or_default();
    ensure!(
        method != "run_query" || !["CsvReader", "JsonReader"].iter().any(|rule| query.contains(rule)),
        "a subsidiary query cannot read host files"
    );
    Ok(())
}

fn string_list(args: &Value, field: &str) -> Vec<String> {
    args.get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Refuse, before any effect, every generic Cozo request that could read or
/// write personal memory or soul. A whole-database backup or restore would
/// include them, so only an explicit, non-personal relation list may be
/// archived and restore is refused.
pub(crate) fn refuse_personal_relations(method: &str, args: &Value) -> Result<()> {
    let named: Vec<String> = match method {
        "run_query" => vec![
            args.get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ],
        "import_relations" => args
            .get("data")
            .and_then(Value::as_object)
            .map(|data| data.keys().cloned().collect())
            .unwrap_or_default(),
        "export_relations" => string_list(args, "relations"),
        "backup" => {
            let relations = string_list(args, "relations");
            ensure!(
                !relations.is_empty(),
                "a whole-database cozo backup would include personal memory; name the relations"
            );
            relations
        }
        "restore" => anyhow::bail!(
            "cozo restore is refused: an archive could overwrite personal memory or soul"
        ),
        "describe_relation" => vec![
            args.get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ],
        _ => Vec::new(),
    };
    ensure!(
        !named.iter().any(|text| names_personal_relation(text)),
        "personal memory and soul relations are not available through the generic cozo plugin"
    );
    Ok(())
}

pub(super) fn supports(method: &str) -> bool {
    METHODS.contains(&method)
}

pub(super) async fn dispatch(
    ctx: &BackendContext<'_>,
    method: &str,
    args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let subsidiary = crate::runtime::subsidiary::resolve(ctx).await?;
    let (store, archive_root) = if let Some(scope) = &subsidiary {
        refuse_file_readers(method, args).map_err(RuntimeError::before_effect)?;
        let (store, archives) = ctx.runtime.subsidiary_stores().cozo(scope).await?;
        (store, Some(archives))
    } else {
        refuse_personal_relations(method, args).map_err(RuntimeError::before_effect)?;
        (ctx.runtime.cognitive().cognitive().await?.cozo_shuttle(), None)
    };
    store
        .ensure_distinct_from(ctx.runtime.identity_store())
        .map_err(|error| RuntimeError::before_effect(error.into()))?;
    store
        .ensure_distinct_from(&ctx.runtime.documents().shared_store())
        .map_err(|error| RuntimeError::before_effect(error.into()))?;
    store
        .ensure_distinct_from(&ctx.runtime.workspace().gallery_store())
        .map_err(|error| RuntimeError::before_effect(error.into()))?;
    ctx.runtime
        .chat()
        .ensure_distinct_from(&store)
        .map_err(RuntimeError::before_effect)?;
    let mutates = ctx.declaration.side_effect == SideEffect::Mutation;

    // Construct observations before entering the native graph replacement. No
    // host scan, caller-supplied topology, or private state enters this graph.
    let graph = if method == "refresh_system_graph" {
        let input: cozo::RefreshSystemGraphInput = serde_json::from_value(args.clone())
            .map_err(|error| RuntimeError::before_effect(error.into()))?;
        let runtime = ctx.runtime.clone();
        let graph_scope = if subsidiary.is_some() { ctx.caller.private_scope() } else { StateScope::Public };
        Some((
            tokio::task::spawn_blocking(move || committed_system_graph(&runtime, &graph_scope))
                .await
                .map_err(|error| RuntimeError::before_effect(error.into()))?
                .map_err(RuntimeError::before_effect)?,
            input.expected_revision,
        ))
    } else {
        None
    };
    super::revalidate(ctx).await?;
    let result = if let Some((graph, revision)) = graph {
        let store = store.clone();
        tokio::task::spawn_blocking(move || -> Result<Value> {
            Ok(serde_json::to_value(
                store.replace_system_graph(&graph, revision)?,
            )?)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|result| result)
    } else if let Some(archives) = archive_root {
        cozo::methods::dispatch_cozo_with_archive_dir(store.clone(), archives, method, args).await
    } else {
        cozo::dispatch_cozo_method(store.clone(), method, args).await
    };
    // Even a native error may follow a committed mutable script/trigger or
    // archive. Never retry it. Preserve one fresh metadata observation and
    // classify the outcome as uncertain unless the operation was read-only.
    // backup owns its file+directory fsync; do not flush an unrelated database.
    let barrier = if mutates && method != "backup" {
        let store = store.clone();
        tokio::task::spawn_blocking(move || store.sync_runtime_document_store())
            .await
            .map_err(|error| anyhow::anyhow!(error))
            .and_then(|result| result.map_err(Into::into))
    } else {
        Ok(())
    };
    let observed = observe(store.clone()).await;
    let scope = ctx.caller.private_scope();
    let error = match (result, barrier) {
        (Ok(result), Ok(())) => {
            let state = match observed {
                Ok(state) => state,
                Err(error) => {
                    let error = if mutates {
                        RuntimeError::unknown(error)
                    } else {
                        RuntimeError::backend(error)
                    };
                    return Err(error.with_guard(Box::new(store)));
                }
            };
            if let Err(error) = super::revalidate(ctx).await {
                return Err(RuntimeError::backend(error.into())
                    .with_state(state, scope)
                    .with_guard(Box::new(store)));
            }
            let mut outcome = super::private_result(ctx, result, state);
            outcome.guard = Some(Box::new(store));
            return Ok(outcome);
        }
        (Err(error), _) | (Ok(_), Err(error)) => {
            if mutates {
                RuntimeError::unknown(error)
            } else {
                RuntimeError::backend(error)
            }
        }
    };
    let error = match observed {
        Ok(state) => error.with_state(state, scope),
        Err(_) => error,
    };
    Err(error.with_guard(Box::new(store)))
}

async fn observe(store: Arc<CozoGraphShuttle>) -> Result<BackendStateUpdate> {
    let listing: ListRelationsOutput = serde_json::from_value(
        cozo::dispatch_cozo_method(store.clone(), "list_relations", &json!({})).await?,
    )?;
    let mut relations = Vec::with_capacity(listing.relations.len());
    let mut triggers = Vec::new();
    for relation in listing.relations {
        // Native index relations belong to their base relation. Cozo's own
        // describe method rejects selecting their internal implementation.
        if relation.access_level == "index" {
            continue;
        }
        let description: RelationDescription = serde_json::from_value(
            cozo::dispatch_cozo_method(
                store.clone(),
                "describe_relation",
                &json!({"name":relation.name}),
            )
            .await?,
        )?;
        let mut keys = Vec::new();
        let mut values = Vec::new();
        for column in description.columns {
            let is_key = column.is_key;
            let column = CozoColumn {
                name: column.column,
                r#type: column.type_name,
                is_key,
                // Native 0.7.6 exposes only has_default, not its expression.
                // None is unknown/omitted, never a synthesized default value.
                default: None,
                // Cozo has no column-level description facility.
                description: String::new(),
            };
            if is_key {
                keys.push(column);
            } else {
                values.push(column);
            }
        }
        for trigger in description.triggers {
            triggers.push(CozoTrigger {
                relation: description.relation.name.clone(),
                event: trigger.event,
                query: trigger.trigger,
            });
        }
        relations.push(CozoRelation {
            name: description.relation.name,
            description: description.relation.description,
            keys,
            values,
            access_level: description.relation.access_level,
            // Metadata inspection is not an unbounded scan of private data.
            row_count: None,
        });
    }
    // Indices/HNSW/default expressions cannot be reconstructed faithfully
    // from this native metadata contract. Do not publish empty replacements
    // or invented engine/path/version/query counters for those fields.
    Ok(BackendStateUpdate::Fields(
        json!({"relations":relations,"triggers":triggers}),
    ))
}

fn committed_system_graph(runtime: &crate::runtime::Runtime, scope: &StateScope) -> Result<GraphSnapshot> {
    ensure!(
        runtime.catalog().plugins().len() <= 512,
        "system graph catalog exceeds plugin limit"
    );
    let mut plugins = BTreeMap::new();
    let expected: BTreeMap<String, String> = runtime
        .catalog()
        .plugins()
        .iter()
        .map(|entry| (entry.id.clone(), entry.schema_hash.clone()))
        .collect();
    ensure!(
        crate::sealed_schema_reader::manifest_plugins(runtime.catalog_dir())? == expected,
        "system graph sealed catalog changed"
    );
    let mut generations = BTreeMap::<String, Value>::new();
    let mut total_bytes = 0usize;
    for entry in runtime.catalog().plugins() {
        let document = read_manifest_pinned_schema_document(runtime.catalog_dir(), &entry.id)?;
        ensure!(
            document.schema_hash == entry.schema_hash,
            "system graph sealed catalog changed"
        );
        let state = if GRAPH_STATE_PLUGINS.contains(&entry.id.as_str()) {
            runtime
                .core()
                .committed_snapshot(scope, &entry.id, &canonical_path(&entry.id))?
                .map(|snapshot| -> Result<Value> {
                    ensure!(
                        snapshot.schema_hash == entry.schema_hash,
                        "graph state schema pin changed"
                    );
                    total_bytes = total_bytes
                        .checked_add(serde_json::to_vec(&snapshot.state)?.len())
                        .context("graph state size overflow")?;
                    ensure!(
                        total_bytes <= 32 * 1024 * 1024,
                        "system graph state exceeds byte bound"
                    );
                    generations.insert(
                        entry.id.clone(),
                        json!({"generation":snapshot.generation,"event_id":snapshot.event_id}),
                    );
                    Ok(snapshot.state)
                })
                .transpose()?
        } else {
            None
        };
        plugins.insert(
            entry.id.clone(),
            graph_projection::GraphPluginProjection {
                schema: document.schema,
                schema_hash: document.schema_hash,
                state,
            },
        );
    }
    ensure!(
        crate::sealed_schema_reader::manifest_plugins(runtime.catalog_dir())? == expected,
        "system graph sealed catalog changed during observation"
    );
    let provenance = GraphProvenance {
        kind: GraphProvenanceKind::ObservedRuntime,
        source_commit: "unknown".into(), release_id: None,
        generated_at: chrono::Utc::now().to_rfc3339(),
        inputs: BTreeMap::from([
            ("sealed_catalog_sha256".into(), runtime.catalog().hash().to_owned()),
            ("committed_scope".into(), serde_json::to_string(scope)?),
            ("committed_heads".into(), serde_json::to_string(&generations)?),
        ]),
        coverage: vec!["manifest-pinned plugin contracts".into(), "committed subsystem state in the selected owner scope".into()],
        limitations: vec![
            "Capture time is not backend observation time; unavailable public state is unknown, not absent infrastructure.".into(),
            "This graph does not independently probe services or identify the active release.".into(),
            "Private identity, conversations, memory, credentials and mail contents are excluded.".into(),
        ],
    };
    Ok(graph_projection::project_system_graph(&plugins, provenance))
}

fn canonical_path(plugin: &str) -> String {
    op_plugins::canonical::plugin_path(plugin)
}

#[cfg(test)]
mod personal_relation_tests {
    use super::refuse_personal_relations;
    use serde_json::json;

    #[test]
    fn generic_cozo_cannot_read_or_write_personal_memory_or_soul() {
        for (method, args) in [
            (
                "run_query",
                json!({"query": "?[v] := *memory_entries{namespace, value: v}"}),
            ),
            (
                "run_query",
                json!({"query": "?[s] := *soul_memories:by_key{s}"}),
            ),
            (
                "run_query",
                json!({"query": "?[n] <- [['x']] :put memory_namespaces {n}"}),
            ),
            (
                "run_query",
                json!({"query": "::remove agent_namespace_bindings"}),
            ),
            ("export_relations", json!({"relations": ["soul_memories"]})),
            (
                "import_relations",
                json!({"data": {"memory_entries": {"headers": [], "rows": []}}}),
            ),
            ("backup", json!({"path": "all"})),
            (
                "backup",
                json!({"path": "x", "relations": ["graph_node", "memory_entries"]}),
            ),
            ("restore", json!({"path": "x"})),
            ("describe_relation", json!({"name": "soul_memories"})),
        ] {
            assert!(
                refuse_personal_relations(method, &args).is_err(),
                "{method} {args} must be refused"
            );
        }
    }

    #[test]
    fn generic_cozo_keeps_non_personal_relations() {
        for (method, args) in [
            ("run_query", json!({"query": "?[id] := *graph_node{id}"})),
            ("run_query", json!({"query": "::relations"})),
            ("export_relations", json!({"relations": ["graph_node"]})),
            (
                "backup",
                json!({"path": "g", "relations": ["graph_node", "graph_edge"]}),
            ),
            ("describe_relation", json!({"name": "system_graph_nodes"})),
            ("list_relations", json!({})),
            ("traverse_system_graph", json!({})),
        ] {
            assert!(
                refuse_personal_relations(method, &args).is_ok(),
                "{method} {args} must stay available"
            );
        }
    }
}
