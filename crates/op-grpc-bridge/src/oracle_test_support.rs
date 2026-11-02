//! Isolated Cozo fixtures for the retained OIA1 validator tests.

use std::sync::{Mutex, MutexGuard};

use base64::Engine as _;
use op_cozo_store::HumanPrincipalRecord;

pub(crate) struct TempCozo {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    previous_path: Option<std::ffi::OsString>,
}

impl Drop for TempCozo {
    fn drop(&mut self) {
        // All OIA1 fixture users hold the same registry environment lock.
        unsafe {
            match self.previous_path.take() {
                Some(path) => std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", path),
                None => std::env::remove_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH"),
            }
        }
    }
}

pub(crate) fn temp_cozo() -> TempCozo {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let previous_path = std::env::var_os("OP_HUMAN_PRINCIPAL_COZO_DB_PATH");
    // The fixture lock covers every mutation and use of this override in these tests.
    unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", dir.path().join("cozo")) };
    TempCozo {
        _guard: guard,
        _dir: dir,
        previous_path,
    }
}

pub(crate) fn pk(byte: u8) -> String {
    base64::engine::general_purpose::STANDARD.encode([byte; 32])
}

pub(crate) async fn register(pubkey: &str, alias: &str) -> anyhow::Result<serde_json::Value> {
    let store = crate::human_principal_registry::store()
        .ok_or_else(|| anyhow::anyhow!("fixture principal store unavailable"))?;
    store.put_human_principal(&HumanPrincipalRecord {
        principal_id: op_identity::session::derive_principal_id(pubkey),
        human_pubkey: pubkey.to_owned(),
        display_alias: alias.to_owned(),
        registered_at: 1,
        revoked_at: 0,
    })?;
    Ok(serde_json::json!({}))
}

pub(crate) async fn revoke(pubkey: &str) -> anyhow::Result<serde_json::Value> {
    let store = crate::human_principal_registry::store()
        .ok_or_else(|| anyhow::anyhow!("fixture principal store unavailable"))?;
    let record = store
        .get_human_principal_by_pubkey(pubkey)?
        .ok_or_else(|| anyhow::anyhow!("fixture principal missing"))?;
    store.revoke_human_principal(&record.principal_id, 1)?;
    Ok(serde_json::json!({}))
}
