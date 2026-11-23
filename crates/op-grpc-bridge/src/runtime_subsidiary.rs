//! Backend resource ownership comes from the admitted LXD identity, never a
//! tenant selector supplied in arguments. All effects still enter Runtime.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use op_plugins::state_plugins::identity_sled::{OrgRelation, OrgRelationKind};
use op_plugins::state_plugins::runtime_documents::RuntimeDocuments;
use anyhow::{Context, Result, ensure};
use op_cozo_store::{CozoGraphShuttle, LxdSubsidiary};
use serde_json::Value;
use tokio::sync::Mutex;

use super::{BackendContext, RuntimeError};

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

pub(crate) async fn resolve(ctx: &BackendContext<'_>) -> Result<Option<Scope>, RuntimeError> {
    let store = ctx.runtime.identity_store().clone();
    let session = ctx.caller.session_id.clone();
    let key = ctx.caller.claims().wireguard_pubkey.clone();
    let genesis = ctx.caller.session_genesis.clone();
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

/// One native Cozo handle and one document writer per subsidiary. Domain
/// configuration and generic Cozo methods never reopen competing owners.
struct Store {
    documents: Arc<RuntimeDocuments>,
    archives: PathBuf,
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
        let path = directory.join("backend/cozo");
        let cozo = tokio::task::spawn_blocking(move || CozoGraphShuttle::new_persistent_private(path)).await
            .map_err(|error| RuntimeError::before_effect(error.into()))?
            .map_err(|error| RuntimeError::before_effect(error.into()))?;
        for other in open.values() {
            cozo.ensure_distinct_from(&other.documents.shared_store())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
        }
        let store = Arc::new(Store {
            documents: Arc::new(RuntimeDocuments::new(cozo)),
            archives: directory.join("backend/cozo-archives"),
        });
        open.insert(scope.row.session_id.clone(), store.clone());
        Ok(store)
    }

    pub(crate) async fn cozo(&self, scope: &Scope) -> Result<(Arc<CozoGraphShuttle>, PathBuf), RuntimeError> {
        let store = self.store(scope).await?;
        Ok((store.documents.shared_store(), store.archives.clone()))
    }

    pub(crate) async fn documents(&self, scope: &Scope) -> Result<Arc<RuntimeDocuments>, RuntimeError> {
        Ok(self.store(scope).await?.documents.clone())
    }
}

pub(crate) async fn documents(ctx: &BackendContext<'_>) -> Result<Arc<RuntimeDocuments>, RuntimeError> {
    match resolve(ctx).await? {
        Some(scope) => {
            let documents = ctx.runtime.subsidiary().documents(&scope).await?;
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
