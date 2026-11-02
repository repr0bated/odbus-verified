//! Shared strict Cozo-backed principal lookup for authenticated receivers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail};
use op_cozo_store::{CozoGraphShuttle, HumanPrincipalRecord, human_principal_cozo_db_path};

fn stores() -> &'static Mutex<HashMap<PathBuf, CozoGraphShuttle>> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, CozoGraphShuttle>>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn store() -> Option<CozoGraphShuttle> {
    let path = human_principal_cozo_db_path();
    let mut map = stores()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = map.get(&path) {
        return Some(existing.clone());
    }
    match CozoGraphShuttle::new_persistent(path.clone()) {
        Ok(shuttle) => {
            map.insert(path, shuttle.clone());
            Some(shuttle)
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "human principal Cozo store unavailable");
            None
        }
    }
}

// Used by the retained external MCP integration when that module is linked.
#[allow(dead_code)]
pub(crate) fn external_mcp_store() -> anyhow::Result<CozoGraphShuttle> {
    store().ok_or_else(|| anyhow!("human principal Cozo store unavailable"))
}

pub(crate) async fn get_by_pubkey_strict(
    pubkey: &str,
) -> anyhow::Result<Option<HumanPrincipalRecord>> {
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    lookup_in(cozo, pubkey).await
}

async fn lookup_in(
    cozo: CozoGraphShuttle,
    pubkey: &str,
) -> anyhow::Result<Option<HumanPrincipalRecord>> {
    let pubkey = pubkey.to_owned();
    tokio::task::spawn_blocking(move || cozo.get_human_principal_by_pubkey(&pubkey))
        .await
        .map_err(|error| anyhow!("human principal pubkey lookup join: {error}"))?
        .map_err(|error| anyhow!("human principal pubkey lookup: {error}"))
}

/// Store unavailability is distinct from an unregistered principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryUnavailable;

pub async fn resolve_key_for_assertion(
    pubkey: &str,
) -> Result<Option<HumanPrincipalRecord>, RegistryUnavailable> {
    get_by_pubkey_strict(pubkey)
        .await
        .map_err(|_| RegistryUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_preserves_absence_and_revocation() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let record = HumanPrincipalRecord {
            principal_id: "principal".into(),
            human_pubkey: "key".into(),
            display_alias: String::new(),
            registered_at: 1,
            revoked_at: 0,
        };
        assert!(lookup_in(store.clone(), "key").await.unwrap().is_none());
        store.put_human_principal(&record).unwrap();
        assert_eq!(
            lookup_in(store.clone(), "key").await.unwrap(),
            Some(record.clone())
        );
        store
            .revoke_human_principal(&record.principal_id, 10)
            .unwrap();
        assert_eq!(
            lookup_in(store, "key").await.unwrap().unwrap().revoked_at,
            10
        );
    }
}
