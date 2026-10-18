//! Resolve routing proposals from one authoritative sealed catalog snapshot.
//! No registry membership, guessed method aliases, or caller-authored subids.

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{
    routing_decision_methods::{CandidateOperation, ResolvedOperationRef},
    schema_tag_registry_methods::{SchemaTagAssociation, SchemaTagAssociationRequest},
};

use crate::sealed_schema_reader::{
    ManifestPinnedCatalog, blob_catalog_dir_from_env, read_manifest_pinned_catalog,
};

pub(crate) fn resolve_in(
    catalog: &ManifestPinnedCatalog,
    plugin_id: &str,
    method_key: &str,
) -> Result<ResolvedOperationRef> {
    let pinned = catalog.plugins.get(plugin_id).context("routing target plugin is not sealed")?;
    let declaration = pinned.schema.methods.get(method_key)
        .context("routing target method key is not declared in its sealed plugin")?;
    crate::method_contract_admission::admit_compiled_state_contract(pinned, plugin_id)?;
    crate::method_contract_admission::admit_pinned_compiled_method(
        pinned, plugin_id, method_key, declaration.required_capability.as_deref(),
    )?;
    let operation = ResolvedOperationRef {
        plugin_id: plugin_id.to_owned(),
        method_key: method_key.to_owned(),
        transport_name: declaration.name.clone(),
        schema_hash: pinned.schema_hash.clone(),
        schema_version: pinned.schema.version.clone(),
        catalog_hash: catalog.catalog_hash.clone(),
        subid: declaration.subid.clone(),
        required_capability: declaration.required_capability.clone(),
    };
    operation.validate()?;
    Ok(operation)
}

pub(crate) async fn resolve_candidate(candidate: CandidateOperation) -> Result<ResolvedOperationRef> {
    candidate.validate()?;
    tokio::task::spawn_blocking(move || {
        let catalog = read_manifest_pinned_catalog(&blob_catalog_dir_from_env())?;
        let operation = resolve_in(&catalog, &candidate.plugin_id, &candidate.method_key)?;
        ensure!(operation.schema_hash == candidate.schema_hash, "routing candidate schema hash is stale");
        Ok(operation)
    }).await.context("routing catalog reader failed")?
}

pub(crate) async fn resolve_associations(
    requests: Vec<SchemaTagAssociationRequest>,
    tag_id: u32,
) -> Result<Vec<SchemaTagAssociation>> {
    ensure!(tag_id > 0 && requests.len() <= 256, "routing association request exceeds bounds");
    for request in &requests { request.validate()?; }
    tokio::task::spawn_blocking(move || {
        let catalog = read_manifest_pinned_catalog(&blob_catalog_dir_from_env())?;
        let mut keys = std::collections::BTreeSet::new();
        requests.into_iter().map(|request| {
            ensure!(keys.insert((request.plugin_id.clone(), request.method_key.clone())), "duplicate routing association");
            let operation = resolve_in(&catalog, &request.plugin_id, &request.method_key)?;
            Ok(SchemaTagAssociation {
                plugin_id: operation.plugin_id,
                method_key: operation.method_key,
                schema_hash: operation.schema_hash,
                tag_id,
            })
        }).collect()
    }).await.context("routing association reader failed")?
}

/// Final catalog check after a store/worker queue. Catalog identity is compared
/// independently from the target schema identity; neither is recomputed here.
pub(crate) async fn recheck(operation: ResolvedOperationRef) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let catalog = read_manifest_pinned_catalog(&blob_catalog_dir_from_env())?;
        let current = resolve_in(&catalog, &operation.plugin_id, &operation.method_key)?;
        ensure!(current == operation, "routing operation or catalog changed while queued");
        Ok(())
    }).await.context("routing catalog recheck failed")?
}
