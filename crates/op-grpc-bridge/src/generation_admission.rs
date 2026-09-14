//! Exact pinned admission retained through a private generation worker.
//! The owner context is not reconstructed from prompt/job fields. An optional
//! facade origin carries only original schema evidence, never extra authority.
use anyhow::{Context, Result};
use op_plugins::state_plugins::tched_router::generation::GenerationOwner;
use op_state_store::PluginSchema;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{read_manifest_pinned_catalog, ManifestPinnedSchemaDocument},
};

pub(crate) const PLUGIN: &str = "tched_router";

#[derive(Clone)]
pub(crate) struct GenerationOrigin {
    pub(crate) schema_hash: String,
    pub(crate) schema_json: String,
}
impl GenerationOrigin {
    pub(crate) fn admit(catalog: &Path, admitted: &PluginSchema) -> Result<Self> {
        let catalog = read_manifest_pinned_catalog(catalog)?;
        let pinned = catalog
            .plugins
            .get("large_language_model")
            .context("generation facade seal is absent")?;
        method_contract_admission::admit_pinned_compiled_method(
            pinned,
            "large_language_model",
            "stream_generate",
            Some("llm.invoke"),
        )?;
        anyhow::ensure!(
            serde_json::to_value(admitted)? == serde_json::to_value(&pinned.schema)?,
            "generation facade seal changed after admission"
        );
        Ok(Self {
            schema_hash: pinned.schema_hash.clone(),
            schema_json: pinned.blob.schema_json.clone(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct GenerationCaller {
    pub actor: String,
    pub session: String,
    pub genesis: String,
}
impl GenerationCaller {
    pub async fn verify(
        &self,
        engine: &MutationEngine,
        capability: &str,
    ) -> Result<GenerationOwner> {
        let session = project_dispatch::verified_session(
            engine,
            &self.actor,
            Some(&self.session),
            Some(&self.genesis),
        )
        .await?;
        anyhow::ensure!(session == self.session, "generation session changed");
        anyhow::ensure!(
            load_capability_grants(&self.actor).contains(capability),
            "current exact generation capability grant is required"
        );
        GenerationOwner::new(self.actor.clone(), session)
    }
}

#[derive(Clone)]
pub(crate) struct GenerationPins {
    pub catalog: PathBuf,
    pub catalog_hash: String,
    pub document: Arc<ManifestPinnedSchemaDocument>,
    pub origin: Option<GenerationOrigin>,
}
impl GenerationPins {
    pub fn load(
        catalog: &Path,
        admitted: Option<&PluginSchema>,
        method: &str,
        capability: &str,
        origin: Option<GenerationOrigin>,
    ) -> Result<Self> {
        let mut catalog_value = read_manifest_pinned_catalog(catalog)?;
        let document = Arc::new(
            catalog_value
                .plugins
                .remove(PLUGIN)
                .context("generation owner plugin seal is absent")?,
        );
        method_contract_admission::admit_compiled_state_contract(&document, PLUGIN)?;
        method_contract_admission::admit_pinned_compiled_method(
            &document,
            PLUGIN,
            method,
            Some(capability),
        )?;
        if let Some(admitted) = admitted {
            anyhow::ensure!(
                serde_json::to_value(admitted)? == serde_json::to_value(&document.schema)?,
                "generation owner seal changed after admission"
            );
        }
        let pins = Self {
            catalog: catalog.to_owned(),
            catalog_hash: catalog_value.catalog_hash,
            document,
            origin,
        };
        pins.check_catalog(method, capability)?;
        Ok(pins)
    }

    pub fn check_catalog(&self, method: &str, capability: &str) -> Result<String> {
        let current = read_manifest_pinned_catalog(&self.catalog)?;
        anyhow::ensure!(
            current.catalog_hash == self.catalog_hash,
            "generation catalog changed; explicit recovery required"
        );
        let document = current
            .plugins
            .get(PLUGIN)
            .context("generation owner plugin seal was withdrawn")?;
        anyhow::ensure!(
            document.schema_hash == self.document.schema_hash
                && document.blob.schema_json == self.document.blob.schema_json,
            "generation owner seal changed; explicit recovery required"
        );
        method_contract_admission::admit_compiled_state_contract(document, PLUGIN)?;
        method_contract_admission::admit_pinned_compiled_method(
            document,
            PLUGIN,
            method,
            Some(capability),
        )?;
        if let Some(origin) = &self.origin {
            let facade = current
                .plugins
                .get("large_language_model")
                .context("generation facade seal was withdrawn")?;
            anyhow::ensure!(
                facade.schema_hash == origin.schema_hash
                    && facade.blob.schema_json == origin.schema_json,
                "generation facade seal changed"
            );
            method_contract_admission::admit_pinned_compiled_method(
                facade,
                "large_language_model",
                "stream_generate",
                Some("llm.invoke"),
            )?;
        }
        Ok(current.catalog_hash)
    }

    /// Call immediately before native input or private publication, with the
    /// same actor/session. Inside the private transaction the only projection
    /// read permitted is the DISTINCT canonical identity namespace; generation
    /// state must never be read recursively. Identity writers never wait on
    /// generation workers while retaining an identity lock.
    pub async fn check(
        &self,
        engine: &MutationEngine,
        caller: &GenerationCaller,
        method: &str,
        capability: &str,
    ) -> Result<GenerationOwner> {
        let owner = caller.verify(engine, capability).await?;
        if self.origin.is_some() {
            anyhow::ensure!(
                load_capability_grants(&caller.actor).contains("llm.invoke"),
                "generation facade grant was withdrawn"
            );
        }
        self.check_catalog(method, capability)?;
        Ok(owner)
    }
}
