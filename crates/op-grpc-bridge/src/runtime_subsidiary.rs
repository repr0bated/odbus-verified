//! Backend resource ownership comes from the admitted LXD identity, never a
//! tenant selector supplied in arguments. All effects still enter Runtime.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use op_plugins::state_plugins::identity_sled::{OrgRelation, OrgRelationKind};
use op_plugins::state_plugins::runtime_documents::RuntimeDocuments;
use anyhow::{Context, Result, ensure};
use op_cozo_store::{CozoGraphShuttle, LxdSubsidiary};
use serde_json::Value;
use tokio::sync::{Mutex, OnceCell};

use super::{BackendContext, Runtime, RuntimeError, VerifiedCaller};

pub(crate) struct Scope {
    row: LxdSubsidiary,
    prefix: String,
}

impl Scope {
    pub(crate) fn collection_prefix(&self) -> &str {
        &self.prefix
    }
    pub(crate) fn native_uuid(&self) -> &str {
        &self.row.native_uuid
    }


    pub(crate) fn collection(&self, name: &str) -> Result<String> {
        ensure!(!name.is_empty(), "collection name is empty");
        if let Some(local) = name.strip_prefix(&self.prefix) {
            ensure!(!local.is_empty() && !local.contains("::"), "invalid subsidiary collection name");
            return Ok(name.to_owned());
        }
        ensure!(!name.contains("::"), "collection belongs to another subsidiary");
        Ok(format!("{}{name}", self.prefix))
    }

    pub(crate) fn owns_collection(&self, name: &str) -> bool {
        name.strip_prefix(&self.prefix).is_some_and(|local| !local.is_empty() && !local.contains("::"))
    }

    fn directory(&self) -> Result<PathBuf> {
        let volume = PathBuf::from(format!(
            "/var/lib/lxd/storage-pools/lxd-pool/custom/default_identity-{}", self.row.session_id
        ));
        ensure!(std::fs::symlink_metadata(&volume)?.file_type().is_dir(), "subsidiary sled is not a directory");
        ensure!(std::fs::read_to_string(volume.join("session_id"))?.trim() == self.row.session_id,
            "subsidiary sled differs from its admitted identity");
        Ok(volume)
    }
}

pub(crate) fn binding(row: Option<LxdSubsidiary>, relation_json: &str) -> Result<Option<LxdSubsidiary>> {
    let relation = if relation_json.is_empty() {
        None
    } else {
        Some(serde_json::from_str::<OrgRelation>(relation_json)?)
    };
    match (row, relation) {
        (None, None) => Ok(None),
        (Some(row), Some(relation)) => {
            ensure!(relation.kind == OrgRelationKind::Subsidiary && relation.name == row.name,
                "subsidiary relation differs from its native binding");
            Ok(Some(row))
        }
        _ => anyhow::bail!("subsidiary identity requires its native binding and organizational relation"),
    }
}

/// Execution-tracker admission for a subsidiary: the sealed schema purpose must
/// mark the required capability subsidiary yes. A caller with any
/// subsidiary mark but no native binding is refused, so it can never fall
/// through to a host adapter.
pub(crate) async fn admit(runtime: &Runtime, caller: &VerifiedCaller, allowed: bool)
    -> Result<(), RuntimeError> {
    let store = runtime.identity_store().clone();
    let session = caller.session_id.clone();
    let key = caller.claims().wireguard_pubkey.clone();
    let (bound, marked) = tokio::task::spawn_blocking(move || -> Result<_> {
        let rows = store.lxd_subsidiaries()?;
        let by_key = rows.iter().any(|row| !key.is_empty() && row.wireguard_pubkey == key);
        let row = rows.into_iter().find(|row| row.session_id == session);
        let relation = store.read_identity_revision(&session)?.record
            .map(|record| record.org_relation_json).unwrap_or_default();
        let bound = binding(row, &relation)?.is_some();
        Ok((bound, bound || by_key))
    }).await.map_err(|error| RuntimeError::before_effect(error.into()))?
        .map_err(|_| RuntimeError::denied("subsidiary authority could not be established"))?;
    if !marked {
        return Ok(());
    }
    if !bound || resolve_caller(runtime, caller).await?.is_none() {
        return Err(RuntimeError::denied("subsidiary authority requires its native binding"));
    }
    if !allowed {
        return Err(RuntimeError::denied("sealed purpose does not allow subsidiaries this capability"));
    }
    Ok(())
}

pub(crate) async fn resolve(ctx: &BackendContext<'_>) -> Result<Option<Scope>, RuntimeError> {
    resolve_caller(ctx.runtime, ctx.caller).await
}

async fn resolve_caller(runtime: &Runtime, caller: &VerifiedCaller) -> Result<Option<Scope>, RuntimeError> {
    let store = runtime.identity_store().clone();
    let session = caller.session_id.clone();
    let key = caller.claims().wireguard_pubkey.clone();
    let genesis = caller.session_genesis.clone();
    tokio::task::spawn_blocking(move || -> Result<Option<Scope>> {
        let identity = store.read_identity_revision(&session)?.record.context("backend caller identity absent")?;
        let row = store.lxd_subsidiaries()?.into_iter().find(|row| row.session_id == session);
        let Some(row) = binding(row, &identity.org_relation_json)? else { return Ok(None) };
        ensure!(identity.active && !identity.sealed_id.is_empty() && identity.session_genesis == genesis
            && identity.wireguard_pubkey == key && row.wireguard_pubkey == key && !row.native_uuid.is_empty(),
            "subsidiary identity or native binding changed");
        let native: Value = serde_json::from_str(&identity.instance_json)?;
        ensure!(native["name"] == row.name && native["config"]["volatile.uuid"] == row.native_uuid,
            "subsidiary native binding differs from its admitted container");
        let prefix = format!("{}::", row.name);
        Ok(Some(Scope { row, prefix }))
    }).await.map_err(|error| RuntimeError::before_effect(error.into()))?
        .map_err(RuntimeError::before_effect)
}

/// Arbitrary graph queries and revisioned documents are separate native
/// databases. Both owners are cached and opened only when first needed.
struct Store {
    directory: PathBuf,
    cozo: OnceCell<Arc<CozoGraphShuttle>>,
    documents: OnceCell<Arc<RuntimeDocuments>>,
}

impl Store {
    fn new(directory: PathBuf) -> Self {
        Self { directory, cozo: OnceCell::new(), documents: OnceCell::new() }
    }

    async fn cozo(&self) -> Result<Arc<CozoGraphShuttle>, RuntimeError> {
        let store = self.cozo.get_or_try_init(|| async {
            open_private_cozo(self.directory.join("backend/cozo")).await
        }).await?;
        if let Some(documents) = self.documents.get() {
            store.ensure_distinct_from(&documents.shared_store())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
        }
        Ok(store.clone())
    }

    async fn documents(&self) -> Result<Arc<RuntimeDocuments>, RuntimeError> {
        let documents = self.documents.get_or_try_init(|| async {
            let store = open_private_cozo(self.directory.join("backend/runtime-documents")).await?;
            Ok::<_, RuntimeError>(Arc::new(RuntimeDocuments::from_shared(store)))
        }).await?;
        if let Some(store) = self.cozo.get() {
            store.ensure_distinct_from(&documents.shared_store())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
        }
        Ok(documents.clone())
    }
}

async fn open_private_cozo(path: PathBuf) -> Result<Arc<CozoGraphShuttle>, RuntimeError> {
    tokio::task::spawn_blocking(move || CozoGraphShuttle::new_persistent_private(path).map(Arc::new)).await
        .map_err(|error| RuntimeError::before_effect(error.into()))?
        .map_err(|error| RuntimeError::before_effect(error.into()))
}

#[derive(Default)]
pub(crate) struct Stores {
    open: Mutex<HashMap<String, Arc<Store>>>,
}

impl Stores {
    async fn store(&self, scope: &Scope) -> Result<Arc<Store>, RuntimeError> {
        let directory = scope.directory().map_err(RuntimeError::before_effect)?;
        let mut open = self.open.lock().await;
        if let Some(store) = open.get(&scope.row.session_id) {
            return Ok(store.clone());
        }
        let store = Arc::new(Store::new(directory));
        open.insert(scope.row.session_id.clone(), store.clone());
        Ok(store)
    }

    pub(crate) async fn cozo(&self, scope: &Scope) -> Result<(Arc<CozoGraphShuttle>, PathBuf), RuntimeError> {
        let owner = self.store(scope).await?;
        Ok((owner.cozo().await?, owner.directory.join("backend/cozo-archives")))
    }

    pub(crate) async fn documents(&self, scope: &Scope) -> Result<Arc<RuntimeDocuments>, RuntimeError> {
        self.store(scope).await?.documents().await
    }
}

pub(crate) async fn documents(ctx: &BackendContext<'_>) -> Result<Arc<RuntimeDocuments>, RuntimeError> {
    match resolve(ctx).await? {
        Some(scope) => {
            let documents = ctx.runtime.subsidiary_stores().documents(&scope).await?;
            let store = documents.shared_store();
            store.ensure_distinct_from(&ctx.runtime.documents().shared_store())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            store.ensure_distinct_from(ctx.runtime.identity_store())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            Ok(documents)
        }
        None => Ok(ctx.runtime.documents().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generic_query_cannot_remove_configuration_documents() {
        let directory = tempfile::tempdir().unwrap();
        let owner = Store::new(directory.path().to_owned());
        let documents = owner.documents().await.unwrap();
        let original = documents.update("config", |_| {
            Ok(serde_json::json!({"project":"kiroween"}))
        }).await.unwrap();
        let graph = owner.cozo().await.unwrap();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let read = "?[plugin_id] := *plugin_runtime_documents{plugin_id}";
            graph.run_query(read, None)?;
            graph.run_query("::remove plugin_runtime_documents", None)?;
            assert!(graph.run_query(read, None).is_err(), "generic relation was not removed");
            Ok(())
        }).await.unwrap().unwrap();
        assert_eq!(documents.read_durable("config").await.unwrap(), Some(original));
    }

    fn scope() -> Scope {
        Scope {
            row: LxdSubsidiary {
                session_id: "session".into(), name: "kiroween".into(),
                wireguard_pubkey: String::new(), actor_id: String::new(),
                caller_session: String::new(), caller_genesis: String::new(),
                request_json: String::new(), native_uuid: String::new(),
            },
            prefix: "kiroween::".into(),
        }
    }

    #[test]
    fn collection_names_stay_in_the_verified_subsidiary() {
        let scope = scope();
        assert_eq!(scope.collection("inventory").unwrap(), "kiroween::inventory");
        assert_eq!(scope.collection("kiroween::inventory").unwrap(), "kiroween::inventory");
        assert!(scope.collection("jeremy::inventory").is_err());
        assert!(scope.collection("kiroween-other::inventory").is_err());
        assert!(scope.collection("kiroween::").is_err());
        assert!(scope.collection("").is_err());
        assert!(scope.collection("kiroween::other::inventory").is_err());
        assert!(scope.owns_collection("kiroween::inventory"));
        assert!(!scope.owns_collection("kiroween-other::inventory"));
        assert!(binding(None, r#"{"kind":"subsidiary","name":"kiroween"}"#).is_err());
        assert!(binding(Some(scope.row), "").is_err());
    }
}
