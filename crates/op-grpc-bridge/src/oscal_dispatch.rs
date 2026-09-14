//! OSCAL discovery consumes exact sealed schema documents. Durable review
//! annotations cannot create plugin membership or replace a method contract.
use std::{path::PathBuf, sync::Arc};

use anyhow::{ensure, Context, Result};
use op_plugins::state_plugins::oscal_subid_registry_methods::{
    CatalogPinVerifier, OscalInvocationContext, OscalSubidRegistryMethods, SealedCatalogSnapshot,
    SealedSchemaDocument,
};

use crate::sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_catalog};

struct ManifestPin(PathBuf);

impl CatalogPinVerifier for ManifestPin {
    fn verify_current(&self, expected_catalog_hash: &str) -> Result<()> {
        let current = op_blob::catalog::ActiveReflectionCatalog::read_catalog_hash(&self.0)
            .context("sealed OSCAL catalog manifest is unavailable")?;
        ensure!(
            current == expected_catalog_hash,
            "sealed OSCAL catalog changed"
        );
        Ok(())
    }
}

pub(crate) async fn dispatch(
    method: &str,
    args: &serde_json::Value,
    actor_id: &str,
    capability_id: Option<&str>,
) -> Result<serde_json::Value> {
    ensure!(
        !actor_id.trim().is_empty(),
        "verified OSCAL actor is required"
    );
    crate::method_contract_admission::admit_compiled_invocation(
        "oscal_subid_registry",
        method,
        capability_id,
    )?;
    let directory = blob_catalog_dir_from_env();
    let pin = Arc::new(ManifestPin(directory.clone()));
    let snapshot = tokio::task::spawn_blocking(move || -> Result<_> {
        let catalog = read_manifest_pinned_catalog(&directory)?;
        SealedCatalogSnapshot::from_documents(
            catalog.catalog_hash,
            catalog
                .plugins
                .into_iter()
                .map(|(plugin_id, document)| SealedSchemaDocument {
                    plugin_id,
                    schema_hash: document.schema_hash,
                    schema_json: document.blob.schema_json,
                })
                .collect(),
        )
    })
    .await??;
    let documents = op_plugins::state_plugins::runtime_documents::runtime_documents().await?;
    let context = OscalInvocationContext {
        actor_id: actor_id.to_owned(),
        capability_id: capability_id
            .context("OSCAL method requires its exact capability")?
            .to_owned(),
    };
    OscalSubidRegistryMethods::from_shared(documents, pin)
        .dispatch(method, args, &context, &snapshot)
        .await
}
