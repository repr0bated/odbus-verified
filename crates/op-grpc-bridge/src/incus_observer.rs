//! Native Incus lifecycle observation and complete-root publication.
//!
//! The WebSocket is only a wakeup source. Every accepted lifecycle event is
//! followed by a strict full default-project readback; event payloads never
//! become plugin state.

use crate::{
    method_contract_admission::admit_compiled_state_contract,
    mutation_engine::MutationEngine,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
};
use anyhow::{ensure, Context, Result};
use op_plugins::state_plugins::incus::{
    dispatch_incus_method_observed, subscribe_incus_default_project, IncusLifecycleObserver,
    IncusState,
};
use std::{path::Path, sync::Arc};

const PLUGIN: &str = "incus";

pub(crate) struct PreparedIncusObservation {
    pub full_state: simd_json::OwnedValue,
    pub expected_schema_hash: String,
}

fn compatible_seal(catalog: &Path) -> Result<()> {
    let pinned = read_manifest_pinned_schema_document(catalog, PLUGIN)?;
    admit_compiled_state_contract(&pinned, PLUGIN)
}

fn prepare_observation(catalog: &Path, snapshot: &IncusState) -> Result<PreparedIncusObservation> {
    let pinned = read_manifest_pinned_schema_document(catalog, PLUGIN)?;
    admit_compiled_state_contract(&pinned, PLUGIN)?;
    let full_state = simd_json::serde::to_owned_value(snapshot)?;
    ensure!(
        pinned.schema.validate(&full_state).valid,
        "native Incus snapshot does not conform to the pinned plugin state schema"
    );
    pinned.schema.validate_public_state_policy()?;
    // Fail closed on an invalid policy or state/type drift. MutationEngine
    // repeats this exact pinned projection as the sole authority for audit and
    // live notification data while retaining the full private root in SHM.
    pinned.schema.project_public_state(&full_state)?;
    let after = read_manifest_pinned_schema_document(catalog, PLUGIN)?;
    ensure!(
        after.schema_hash == pinned.schema_hash
            && after.blob.schema_json == pinned.blob.schema_json,
        "Incus plugin seal changed during observation validation"
    );
    Ok(PreparedIncusObservation {
        full_state,
        expected_schema_hash: pinned.schema_hash,
    })
}

async fn readback_and_publish(
    engine: &MutationEngine,
    observer: &IncusLifecycleObserver,
) -> Result<()> {
    // The same engine lock is held by the canonical Incus method dispatcher
    // from native effect through readback and commit. It prevents an older
    // observer read from overwriting a newer method observation.
    let _observation = engine.lock_host_observation().await;
    let snapshot = observer.default_project_state().await?;
    let prepared = prepare_observation(&blob_catalog_dir_from_env(), &snapshot)?;
    engine
        .process_observed_state(PLUGIN, prepared.full_state, prepared.expected_schema_hash)
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// Subscribe first, then publish one initial complete snapshot. Each native
/// lifecycle wakeup produces another complete root replacement. Disconnect or
/// malformed data ends this task without deleting the last committed state;
/// only an explicit engine start/reload retries it.
pub(crate) async fn observe(engine: Arc<MutationEngine>) -> Result<()> {
    let catalog = blob_catalog_dir_from_env();
    compatible_seal(&catalog)?;
    let mut observer = subscribe_incus_default_project().await?;
    readback_and_publish(&engine, &observer)
        .await
        .context("initial Incus snapshot publication failed")?;
    loop {
        observer.wait_for_lifecycle().await?;
        readback_and_publish(&engine, &observer)
            .await
            .context("Incus lifecycle readback publication failed")?;
    }
}

/// Canonical Incus method integration. The native method, immediate readback,
/// seal validation, and full/private plus projected/public commit share the
/// observer lock. A completed provider mutation is never reported as success
/// if its readback or publication fails.
pub(crate) async fn dispatch_method(
    engine: &MutationEngine,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let engine = engine.clone();
    let method = method.to_string();
    let args = args.clone();
    tokio::spawn(async move { dispatch_method_owned(&engine, &method, &args).await })
        .await
        .context("Incus operation owner failed; reconciliation may be required")?
}

async fn dispatch_method_owned(
    engine: &MutationEngine,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let _observation = engine.lock_host_observation().await;
    compatible_seal(&blob_catalog_dir_from_env())?;
    let observed = dispatch_incus_method_observed(method, args).await?;
    if let Some(snapshot) = observed.state {
        let prepared = prepare_observation(&blob_catalog_dir_from_env(), &snapshot).with_context(
            || {
                format!(
                    "Incus {method} completed but its state preparation failed; reconciliation is required"
                )
            },
        )?;
        engine
            .process_observed_state(
                PLUGIN,
                prepared.full_state,
                prepared.expected_schema_hash,
            )
            .await
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "Incus {method} completed but authoritative state publication failed; reconciliation is required"
                )
            })?;
    }
    Ok(observed.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_plugins::state_plugins::incus::{IncusInstance, IncusPlugin};
    use op_state::StatePlugin;
    use simd_json::prelude::ValueObjectAccess;

    fn snapshot() -> IncusState {
        IncusState {
            instances: vec![IncusInstance {
                name: "private-instance".into(),
                status: "Running".into(),
                instance_type: "container".into(),
                status_code: Some(103),
                architecture: Some("x86_64".into()),
                ephemeral: Some(false),
                stateful: Some(false),
                location: Some("none".into()),
                project: Some("default".into()),
                config: Some(std::collections::HashMap::from([(
                    "user.secret".into(),
                    "must-remain-private".into(),
                )])),
                ..IncusInstance::default()
            }],
        }
    }

    #[test]
    fn observation_is_pinned_and_public_projection_is_filtered() {
        let directory = tempfile::tempdir().unwrap();
        assert!(compatible_seal(directory.path()).is_err());
        assert!(prepare_observation(directory.path(), &snapshot()).is_err());
        let mut store = op_blob::BlobStore::open(directory.path()).unwrap();
        let schema = IncusPlugin::new().schema().unwrap();
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, schema.clone()))
            .unwrap();
        let prepared = prepare_observation(directory.path(), &snapshot()).unwrap();
        assert!(compatible_seal(directory.path()).is_ok());
        assert_eq!(
            prepared.full_state["instances"][0]["name"],
            "private-instance"
        );
        let full = prepared.full_state.clone();
        let pinned = read_manifest_pinned_schema_document(directory.path(), PLUGIN).unwrap();
        let public = pinned.schema.project_public_state(&full).unwrap().unwrap();
        assert!(public["instances"][0].get("name").is_none());
        assert!(public["instances"][0].get("config").is_none());
        assert_eq!(public["instances"][0]["status"], "Running");

        let mut drifted = schema.clone();
        let instances = drifted.fields.get_mut("instances").unwrap();
        instances.required = !instances.required;
        assert!(drifted.validate(&full).valid);
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, drifted))
            .unwrap();
        assert!(compatible_seal(directory.path()).is_err());
        assert!(prepare_observation(directory.path(), &snapshot()).is_err());

        let mut stale = schema;
        stale.version = "1.0.0".into();
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, stale))
            .unwrap();
        assert!(prepare_observation(directory.path(), &snapshot()).is_err());
        assert!(compatible_seal(directory.path()).is_err());
    }
}
