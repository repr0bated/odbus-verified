//! OSCAL discovery consumes exact sealed schema documents. Durable review
//! annotations cannot create plugin membership or replace a method contract.
use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::oscal_field_classification::{
    AssessFieldsInput, FieldSource, FieldSourcePin,
};
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
    let directory = blob_catalog_dir_from_env();
    let pin = Arc::new(ManifestPin(directory.clone()));
    let catalog =
        tokio::task::spawn_blocking(move || read_manifest_pinned_catalog(&directory)).await??;
    // Admission, target selection and result validation use this one pinned
    // catalog. This does not replace outer caller/grant admission or claim an
    // atomic transaction with the sealer or OSCAL's Cozo document.
    let declaration = crate::method_contract_admission::admit_pinned_compiled_method(
        catalog
            .plugins
            .get("oscal_subid_registry")
            .context("OSCAL is absent from the sealed catalog")?,
        "oscal_subid_registry",
        method,
        capability_id,
    )?;
    crate::method_contract_admission::validate_admitted_arguments(&declaration, args)?;
    pin.verify_current(&catalog.catalog_hash)?;
    let documents = op_plugins::state_plugins::runtime_documents::runtime_documents().await?;
    let context = OscalInvocationContext {
        actor_id: actor_id.to_owned(),
        capability_id: capability_id
            .context("OSCAL method requires its exact capability")?
            .to_owned(),
    };
    let methods = OscalSubidRegistryMethods::from_shared(documents, pin.clone());
    let catalog_hash = catalog.catalog_hash.clone();
    let output = if method == "assess_fields" {
        let input: AssessFieldsInput = serde_json::from_value(args.clone())?;
        let mut plugins = catalog.plugins;
        let document = plugins
            .remove(&input.plugin_id)
            .context("field subject is absent from the sealed catalog")?;
        let source = FieldSourcePin {
            plugin_id: input.plugin_id,
            schema_version: document.schema.version,
            source: FieldSource::Sealed {
                catalog_hash: catalog_hash.clone(),
                schema_hash: document.schema_hash,
            },
        };
        serde_json::to_value(
            methods
                .assess_fields(&context, source, document.blob.schema_json)
                .await?,
        )?
    } else {
        let snapshot = tokio::task::spawn_blocking(move || {
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
        methods.dispatch(method, args, &context, &snapshot).await?
    };
    crate::method_contract_admission::validate_admitted_output(&declaration, &output)?;
    pin.verify_current(&catalog_hash)?;
    Ok(output)
}
