//! human_principal method dispatch — humans are not containers.
//!
//! The human_principal plugin (op-plugins) is the schema of record; this
//! module is the bridge-side dispatch for its six contract methods. Registry
//! records persist in the plugin's own Cozo relations (op-cozo-store) — the
//! durable source of truth — via `OP_HUMAN_PRINCIPAL_COZO_DB_PATH`.
//!
//! Semantics pinned by the validation contract:
//! - `principal_id` is DERIVED from the WireGuard pubkey
//!   (`op_identity::session::derive_principal_id`), never caller-supplied.
//! - Pubkey uniqueness holds across active AND revoked records: revocation
//!   is a permanent tombstone, never a re-registration window.
//! - Alias uniqueness holds among ACTIVE principals only; the alias is
//!   display-only and never authoritative on any resolution path.
//! - Revoked principals stay VISIBLE to resolve_key/get_principal with
//!   `revoked_at` set — the assertion pipeline depends on seeing the
//!   revocation — they never resolve as active and never as not-found.
//! - Writes fail closed: when the Cozo store cannot be opened, register /
//!   revoke / set_alias error (no partial state, no cache fallback). Reads
//!   degrade to not-found / empty, which is fail-closed for authorization
//!   (an unresolvable principal is an unauthorized one).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail};
use base64::Engine as _;
use op_cozo_store::{CozoGraphShuttle, HumanPrincipalRecord, human_principal_cozo_db_path};
use op_plugins::state_plugins::human_principal::{HumanPrincipal, HumanPrincipalState};
use serde_json::Value as JsonValue;

/// One open store per DB path: RocksDB locks its directory, so a second open
/// of the same path in-process would fail. Keyed by path (not a single
/// process-wide OnceLock) so tests can point
/// `OP_HUMAN_PRINCIPAL_COZO_DB_PATH` at per-test tempdirs.
fn stores() -> &'static Mutex<HashMap<PathBuf, CozoGraphShuttle>> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, CozoGraphShuttle>>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The store for the currently-resolved DB path, or `None` when it cannot be
/// opened. Open failures are NOT cached: a later call with a writable path
/// must recover.
fn store() -> Option<CozoGraphShuttle> {
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
            tracing::warn!(
                path = %path.display(),
                %error,
                "human principal Cozo store unavailable"
            );
            None
        }
    }
}

/// Registry lookup for the assertion validator. Unlike the degraded read
/// path used by ordinary `resolve_key` dispatch, store unavailability is an
/// error (VAL-BRIDGE-017).
pub async fn resolve_key_for_assertion(
    pubkey: &str,
) -> Result<Option<HumanPrincipalRecord>, RegistryUnavailable> {
    get_by_pubkey_strict(pubkey)
        .await
        .map_err(|_| RegistryUnavailable)
}

/// Cozo store unavailable for strict assertion resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryUnavailable;

/// Dispatch one human_principal schema method. Unknown method names are an
/// error (the schema declares exactly six).
pub async fn dispatch_human_principal_method(
    method: &str,
    args: &JsonValue,
) -> anyhow::Result<JsonValue> {
    match method {
        "register_key" => register_key(args).await,
        "revoke_key" => revoke_key(args).await,
        "set_alias" => set_alias(args).await,
        "resolve_key" => resolve_key(args).await,
        "get_principal" => get_principal(args).await,
        "list_principals" => list_principals().await,
        other => Err(anyhow!("unknown human_principal method '{other}'")),
    }
}

/// The full present state (every principal, revoked tombstones included,
/// sorted by principal_id) — the MutationEngine's authoritative_value and
/// shm projection after a mutation.
pub async fn current_state() -> HumanPrincipalState {
    let mut principals: Vec<HumanPrincipal> = list_records()
        .await
        .iter()
        .map(record_to_principal)
        .collect();
    principals.sort_by(|a, b| a.principal_id.cmp(&b.principal_id));
    HumanPrincipalState { principals }
}

/// The method's OSCAL subid, read once from the plugin's schema of record
/// (the MethodDecls are the single source — never a hardcoded parallel).
/// MutationEngine events carry this in `tags_touched` (VAL-CROSS-020).
pub fn method_subid(method: &str) -> Option<String> {
    use op_state::StatePlugin as _;
    static SUBIDS: OnceLock<HashMap<String, String>> = OnceLock::new();
    SUBIDS
        .get_or_init(|| {
            op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
                .schema()
                .map(|schema| {
                    schema
                        .methods
                        .into_iter()
                        .map(|(name, decl)| (name, decl.subid))
                        .collect()
                })
                .unwrap_or_default()
        })
        .get(method)
        .cloned()
}

fn arg_str<'a>(args: &'a JsonValue, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| anyhow!("missing or non-string argument '{key}'"))
}

/// WireGuard pubkeys are base64 and decode to exactly 32 bytes.
fn validate_wireguard_pubkey(pubkey: &str) -> anyhow::Result<()> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(pubkey)
        .map_err(|error| anyhow!("malformed human_pubkey (not base64): {error}"))?;
    if raw.len() != 32 {
        bail!(
            "malformed human_pubkey: decodes to {} bytes, expected 32",
            raw.len()
        );
    }
    Ok(())
}

fn record_to_principal(rec: &HumanPrincipalRecord) -> HumanPrincipal {
    HumanPrincipal {
        principal_id: rec.principal_id.clone(),
        human_pubkey: rec.human_pubkey.clone(),
        display_alias: rec.display_alias.clone(),
        registered_at: rec.registered_at,
        revoked_at: (rec.revoked_at != 0).then_some(rec.revoked_at),
    }
}

// --- Cozo access: strict for writes (fail closed), degraded for reads ---

async fn list_records_strict() -> anyhow::Result<Vec<HumanPrincipalRecord>> {
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    tokio::task::spawn_blocking(move || cozo.list_human_principals())
        .await
        .map_err(|error| anyhow!("human principal list join: {error}"))?
        .map_err(|error| anyhow!("human principal list: {error}"))
}

async fn get_by_pubkey_strict(pubkey: &str) -> anyhow::Result<Option<HumanPrincipalRecord>> {
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    let pubkey = pubkey.to_string();
    tokio::task::spawn_blocking(move || cozo.get_human_principal_by_pubkey(&pubkey))
        .await
        .map_err(|error| anyhow!("human principal pubkey lookup join: {error}"))?
        .map_err(|error| anyhow!("human principal pubkey lookup: {error}"))
}

async fn get_strict(principal_id: &str) -> anyhow::Result<Option<HumanPrincipalRecord>> {
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    let principal_id = principal_id.to_string();
    tokio::task::spawn_blocking(move || cozo.get_human_principal(&principal_id))
        .await
        .map_err(|error| anyhow!("human principal get join: {error}"))?
        .map_err(|error| anyhow!("human principal get: {error}"))
}

async fn list_records() -> Vec<HumanPrincipalRecord> {
    match list_records_strict().await {
        Ok(records) => records,
        Err(error) => {
            tracing::warn!(%error, "human principal list degraded to empty");
            Vec::new()
        }
    }
}

async fn get_by_pubkey_record(pubkey: &str) -> Option<HumanPrincipalRecord> {
    match get_by_pubkey_strict(pubkey).await {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(%error, "human principal resolve degraded to not-found");
            None
        }
    }
}

async fn get_record(principal_id: &str) -> Option<HumanPrincipalRecord> {
    match get_strict(principal_id).await {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(%error, "human principal get degraded to not-found");
            None
        }
    }
}

// --- The six contract methods ---

async fn register_key(args: &JsonValue) -> anyhow::Result<JsonValue> {
    let pubkey = arg_str(args, "human_pubkey")?;
    // display_alias is optional (schema default ""). A caller-supplied
    // principal_id is NEVER honored (VAL-REGISTRY-005): the id is derived
    // below and extra fields are ignored outright.
    let alias = args
        .get("display_alias")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    validate_wireguard_pubkey(pubkey)?;

    // Policy is read-then-write against Cozo as the source of truth; the
    // strict read fails closed when the store is unavailable.
    let existing = list_records_strict().await?;
    if existing.iter().any(|rec| rec.human_pubkey == pubkey) {
        bail!("human_pubkey already registered (active or revoked tombstone)");
    }
    if !alias.is_empty()
        && existing
            .iter()
            .any(|rec| rec.revoked_at == 0 && rec.display_alias == alias)
    {
        bail!("display_alias already held by an active principal");
    }

    let principal = HumanPrincipal {
        principal_id: op_identity::session::derive_principal_id(pubkey),
        human_pubkey: pubkey.to_string(),
        display_alias: alias,
        registered_at: chrono::Utc::now().timestamp(),
        revoked_at: None,
    };
    let record = HumanPrincipalRecord {
        principal_id: principal.principal_id.clone(),
        human_pubkey: principal.human_pubkey.clone(),
        display_alias: principal.display_alias.clone(),
        registered_at: principal.registered_at,
        revoked_at: 0,
    };
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    tokio::task::spawn_blocking(move || cozo.put_human_principal(&record))
        .await
        .map_err(|error| anyhow!("register_key join: {error}"))?
        .map_err(|error| anyhow!("register_key persist: {error}"))?;
    Ok(serde_json::json!({ "principal": principal }))
}

async fn revoke_key(args: &JsonValue) -> anyhow::Result<JsonValue> {
    let pubkey = arg_str(args, "human_pubkey")?;
    let Some(existing) = get_by_pubkey_strict(pubkey).await? else {
        bail!("human_pubkey not registered");
    };
    if existing.revoked_at != 0 {
        // Idempotent no-op: the original revoked_at is preserved verbatim.
        return Ok(serde_json::json!({ "principal": record_to_principal(&existing) }));
    }
    let revoked_at = chrono::Utc::now().timestamp();
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    let principal_id = existing.principal_id.clone();
    tokio::task::spawn_blocking(move || cozo.revoke_human_principal(&principal_id, revoked_at))
        .await
        .map_err(|error| anyhow!("revoke_key join: {error}"))?
        .map_err(|error| anyhow!("revoke_key persist: {error}"))?;
    let updated = get_strict(&existing.principal_id)
        .await?
        .ok_or_else(|| anyhow!("principal vanished after revoke"))?;
    Ok(serde_json::json!({ "principal": record_to_principal(&updated) }))
}

async fn set_alias(args: &JsonValue) -> anyhow::Result<JsonValue> {
    let principal_id = arg_str(args, "principal_id")?;
    let alias = args
        .get("display_alias")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let Some(existing) = get_strict(principal_id).await? else {
        bail!("unknown principal_id");
    };
    // Alias uniqueness holds among ACTIVE principals only. A revoked target
    // is a display-only retitle with no constraint (it may even take an
    // alias held by an active principal); an active target must not collide
    // with ANOTHER active principal (self is excluded; empty never collides).
    if existing.revoked_at == 0 && !alias.is_empty() {
        let all = list_records_strict().await?;
        if all.iter().any(|rec| {
            rec.revoked_at == 0 && rec.principal_id != principal_id && rec.display_alias == alias
        }) {
            bail!("display_alias already held by an active principal");
        }
    }
    let Some(cozo) = store() else {
        bail!("human principal store unavailable");
    };
    let pid = principal_id.to_string();
    let new_alias = alias.clone();
    tokio::task::spawn_blocking(move || cozo.update_human_principal_alias(&pid, &new_alias))
        .await
        .map_err(|error| anyhow!("set_alias join: {error}"))?
        .map_err(|error| anyhow!("set_alias persist: {error}"))?;
    let updated = get_strict(principal_id)
        .await?
        .ok_or_else(|| anyhow!("principal vanished after set_alias"))?;
    Ok(serde_json::json!({ "principal": record_to_principal(&updated) }))
}

async fn resolve_key(args: &JsonValue) -> anyhow::Result<JsonValue> {
    // Deliberately NO pubkey-format validation here: any unknown string —
    // including a display alias — is simply not-found (VAL-REGISTRY-017).
    let pubkey = arg_str(args, "human_pubkey")?;
    let principal = get_by_pubkey_record(pubkey)
        .await
        .map(|rec| record_to_principal(&rec));
    Ok(serde_json::json!({ "principal": principal }))
}

async fn get_principal(args: &JsonValue) -> anyhow::Result<JsonValue> {
    // Malformed (non-UUID) ids are an ordinary lookup miss, never a panic.
    let principal_id = arg_str(args, "principal_id")?;
    let principal = get_record(principal_id)
        .await
        .map(|rec| record_to_principal(&rec));
    Ok(serde_json::json!({ "principal": principal }))
}

async fn list_principals() -> anyhow::Result<JsonValue> {
    let mut principals: Vec<HumanPrincipal> = list_records()
        .await
        .iter()
        .map(record_to_principal)
        .collect();
    principals.sort_by(|a, b| a.principal_id.cmp(&b.principal_id));
    Ok(serde_json::json!({ "principals": principals }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::interceptor::GhostbridgeIdentity;
    use crate::mutation_engine::{ChangeType, MutationEngine};
    use crate::schema_router::{PluginRoute, SchemaBackedInterface};
    use op_state::StatePlugin;
    use op_state_store::{ChainConfig, EventChain};
    use serde_json::Value as JsonValue;
    use std::collections::HashMap;
    use std::sync::{Arc, MutexGuard};
    use tokio::sync::RwLock;

    /// Serialize tests that redirect the process-wide principal-store path.
    ///
    /// The store itself is correctly keyed by path, but the path selector is
    /// an environment variable shared by every test thread. Holding this guard
    /// for the lifetime of the temp directory keeps registration and strict
    /// assertion resolution on the same authoritative store.
    pub(crate) fn test_registry_guard() -> MutexGuard<'static, ()> {
        static TEST_REGISTRY: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TEST_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) struct TempCozo {
        _guard: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        previous_path: Option<std::ffi::OsString>,
    }

    impl Drop for TempCozo {
        fn drop(&mut self) {
            match self.previous_path.take() {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(path) => unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", path) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH") },
            }
        }
    }

    pub(crate) struct TempShm {
        _guard: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        environment: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl Drop for TempShm {
        fn drop(&mut self) {
            for (key, value) in &self.environment {
                match value {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    pub(crate) fn test_engine() -> Arc<MutationEngine> {
        let event_chain = Arc::new(RwLock::new(EventChain::new(ChainConfig::default())));
        let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
        Arc::new(MutationEngine::new(event_chain, ovsdb))
    }

    /// Unique temp Cozo path per test, held together with the process-global
    /// environment lock so parallel test-harness threads cannot redirect a
    /// validator between registration and resolution.
    pub(crate) fn temp_cozo() -> TempCozo {
        let guard = test_registry_guard();
        let dir = tempfile::tempdir().expect("tempdir");
        let previous_path = std::env::var_os("OP_HUMAN_PRINCIPAL_COZO_DB_PATH");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", dir.path().join("cozo")) };
        TempCozo {
            _guard: guard,
            _dir: dir,
            previous_path,
        }
    }

    /// Unique temp projection dir per test so MutationEngine-driving tests
    /// never write the live `/dev/shm/opdbus/state` tree.
    pub(crate) fn temp_shm() -> TempShm {
        temp_shm_with_schemas(&[
            op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
                .schema()
                .unwrap(),
            op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
                .schema()
                .unwrap(),
        ])
    }

    pub(crate) fn temp_shm_with_schemas(schemas: &[op_state_store::PluginSchema]) -> TempShm {
        static TEST_SHM: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = TEST_SHM
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let environment = ["OP_SHM_STATE_DIR", "OP_BLOB_CATALOG_DIR"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
        let catalog = dir.path().join("blobs");
        let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
        for schema in schemas {
            sealer
                .write(&op_blob::blobify_plugin_schema(
                    &schema.name,
                    schema.clone(),
                ))
                .unwrap();
        }
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_SHM_STATE_DIR", dir.path()) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_BLOB_CATALOG_DIR", catalog) };
        TempShm {
            _guard: guard,
            _dir: dir,
            environment,
        }
    }

    /// A valid WireGuard pubkey: base64 of 32 repeated bytes.
    pub(crate) fn pk(byte: u8) -> String {
        base64::engine::general_purpose::STANDARD.encode([byte; 32])
    }

    pub(crate) async fn register(pubkey: &str, alias: &str) -> anyhow::Result<JsonValue> {
        dispatch_human_principal_method(
            "register_key",
            &serde_json::json!({ "human_pubkey": pubkey, "display_alias": alias }),
        )
        .await
    }

    pub(crate) async fn revoke(pubkey: &str) -> anyhow::Result<JsonValue> {
        dispatch_human_principal_method(
            "revoke_key",
            &serde_json::json!({ "human_pubkey": pubkey }),
        )
        .await
    }

    pub(crate) async fn set_alias(principal_id: &str, alias: &str) -> anyhow::Result<JsonValue> {
        dispatch_human_principal_method(
            "set_alias",
            &serde_json::json!({ "principal_id": principal_id, "display_alias": alias }),
        )
        .await
    }

    pub(crate) async fn resolve(pubkey: &str) -> JsonValue {
        dispatch_human_principal_method(
            "resolve_key",
            &serde_json::json!({ "human_pubkey": pubkey }),
        )
        .await
        .expect("resolve_key dispatches")
    }

    pub(crate) async fn get(principal_id: &str) -> JsonValue {
        dispatch_human_principal_method(
            "get_principal",
            &serde_json::json!({ "principal_id": principal_id }),
        )
        .await
        .expect("get_principal dispatches")
    }

    pub(crate) async fn list() -> Vec<JsonValue> {
        dispatch_human_principal_method("list_principals", &serde_json::json!({}))
            .await
            .expect("list_principals dispatches")["principals"]
            .as_array()
            .expect("principals is an array")
            .clone()
    }

    fn principal_id_of(result: &JsonValue) -> &str {
        result["principal"]["principal_id"]
            .as_str()
            .expect("principal_id present")
    }

    /// VAL-REGISTRY-004 (happy path, derived id) + VAL-REGISTRY-009 (resolve
    /// returns the full record field-by-field).
    #[tokio::test]
    async fn register_key_happy_path() {
        let _cozo = temp_cozo();
        let pubkey = pk(1);
        let result = register(&pubkey, "jeremy")
            .await
            .expect("register succeeds");

        let derived = op_identity::session::derive_principal_id(&pubkey);
        assert_eq!(principal_id_of(&result), derived);
        assert_ne!(
            derived,
            op_identity::session::derive_session_id(&pubkey),
            "principal id must never collide with a container session id"
        );
        let registered_at = result["principal"]["registered_at"]
            .as_i64()
            .expect("registered_at set");
        assert!(registered_at > 0);
        assert!(
            result["principal"]["revoked_at"].is_null(),
            "revoked_at empty"
        );

        // VAL-REGISTRY-009: resolve returns the full record, field-by-field.
        let resolved = resolve(&pubkey).await;
        let principal = &resolved["principal"];
        assert_eq!(principal["principal_id"].as_str().unwrap(), derived);
        assert_eq!(principal["human_pubkey"].as_str().unwrap(), pubkey);
        assert_eq!(principal["display_alias"].as_str().unwrap(), "jeremy");
        assert_eq!(principal["registered_at"].as_i64().unwrap(), registered_at);
        assert!(principal["revoked_at"].is_null());
    }

    /// VAL-REGISTRY-005: a caller-supplied principal_id is never honored.
    #[tokio::test]
    async fn register_key_ignores_forged_principal_id() {
        let _cozo = temp_cozo();
        let pubkey = pk(2);
        let forged = "00000000-forged-principal-id-000000000000";
        let result = dispatch_human_principal_method(
            "register_key",
            &serde_json::json!({
                "human_pubkey": pubkey,
                "display_alias": "forger",
                "principal_id": forged,
            }),
        )
        .await
        .expect("extra field tolerated");

        let derived = op_identity::session::derive_principal_id(&pubkey);
        assert_eq!(principal_id_of(&result), derived, "derived id, not forged");
        assert_ne!(principal_id_of(&result), forged);
        assert_eq!(resolve(&pubkey).await["principal"]["principal_id"], derived);
        assert!(
            get(forged).await["principal"].is_null(),
            "forged id must not be resolvable"
        );
    }

    /// VAL-REGISTRY-006: duplicate pubkey registration is rejected and the
    /// original record is unmodified.
    #[tokio::test]
    async fn register_key_rejects_duplicate_pubkey() {
        let _cozo = temp_cozo();
        let pubkey = pk(3);
        let first = register(&pubkey, "first").await.expect("first register");
        let first_registered_at = first["principal"]["registered_at"].as_i64().unwrap();

        let duplicate = register(&pubkey, "second").await;
        assert!(duplicate.is_err(), "duplicate pubkey must error");

        let all = list().await;
        assert_eq!(all.len(), 1, "exactly one principal for the pubkey");
        assert_eq!(all[0]["display_alias"].as_str().unwrap(), "first");
        assert_eq!(
            all[0]["registered_at"].as_i64().unwrap(),
            first_registered_at,
            "original registered_at unmodified"
        );
    }

    /// VAL-REGISTRY-007: duplicate non-empty alias among active principals is
    /// rejected at register time.
    #[tokio::test]
    async fn register_key_rejects_duplicate_active_alias() {
        let _cozo = temp_cozo();
        register(&pk(4), "alice").await.expect("A registers");
        let b = register(&pk(5), "alice").await;
        assert!(b.is_err(), "active alias collision must error");
        assert!(
            resolve(&pk(5)).await["principal"].is_null(),
            "B must not be registered"
        );
    }

    /// VAL-REGISTRY-008: a revoked principal's alias is reusable, and empty
    /// aliases never collide.
    #[tokio::test]
    async fn revoked_alias_reusable_and_empty_aliases_never_collide() {
        let _cozo = temp_cozo();
        register(&pk(6), "alice").await.expect("A registers");
        revoke(&pk(6)).await.expect("revoke A");
        register(&pk(7), "alice")
            .await
            .expect("revoked principal's alias is reusable");

        register(&pk(8), "").await.expect("first empty alias");
        register(&pk(9), "")
            .await
            .expect("second empty alias never collides");
        assert_eq!(list().await.len(), 4);
    }

    /// VAL-REGISTRY-010: resolve_key on an unknown pubkey is not-found and
    /// fabricates nothing.
    #[tokio::test]
    async fn resolve_key_unknown_is_not_found() {
        let _cozo = temp_cozo();
        let before = list().await;
        let resolved = resolve(&pk(100)).await;
        assert!(
            resolved["principal"].is_null(),
            "unknown key resolves to null"
        );
        assert_eq!(list().await.len(), before.len(), "no record fabricated");
    }

    /// VAL-REGISTRY-011: a revoked principal still resolves — with revoked_at
    /// visible, never as active.
    #[tokio::test]
    async fn revoked_principal_resolves_with_revoked_at_visible() {
        let _cozo = temp_cozo();
        let pubkey = pk(10);
        register(&pubkey, "revokee").await.expect("register");
        let revoked = revoke(&pubkey).await.expect("revoke succeeds");
        let revoked_at = revoked["principal"]["revoked_at"]
            .as_i64()
            .expect("revoke returns revoked_at");
        assert!(revoked_at > 0);

        let resolved = resolve(&pubkey).await;
        let principal = &resolved["principal"];
        assert!(
            !principal.is_null(),
            "revoked principal must remain visible (never unknown)"
        );
        assert_eq!(
            principal["revoked_at"].as_i64().unwrap(),
            revoked_at,
            "revoked_at visible, never active-looking"
        );
    }

    /// VAL-REGISTRY-012: revoke_key is an idempotent no-op on an
    /// already-revoked key.
    #[tokio::test]
    async fn revoke_key_idempotent_on_already_revoked() {
        let _cozo = temp_cozo();
        let pubkey = pk(11);
        register(&pubkey, "twice").await.expect("register");
        let first = revoke(&pubkey).await.expect("first revoke");
        let first_revoked_at = first["principal"]["revoked_at"].as_i64().unwrap();

        let second = revoke(&pubkey)
            .await
            .expect("second revoke is a no-op success");
        assert_eq!(
            second["principal"]["revoked_at"].as_i64().unwrap(),
            first_revoked_at,
            "revoked_at byte-identical after idempotent re-revoke"
        );
        let stored = get(principal_id_of(&first)).await;
        assert_eq!(
            stored["principal"]["revoked_at"].as_i64().unwrap(),
            first_revoked_at
        );
    }

    /// VAL-REGISTRY-013: revoke_key on an unknown key is an error and mutates
    /// no state.
    #[tokio::test]
    async fn revoke_key_unknown_key_is_error() {
        let _cozo = temp_cozo();
        let result = revoke(&pk(99)).await;
        assert!(result.is_err(), "unknown key revoke must error");
        assert!(list().await.is_empty(), "no tombstone fabricated");
    }

    /// VAL-REGISTRY-014: get_principal found / unknown / malformed branches.
    #[tokio::test]
    async fn get_principal_found_unknown_and_malformed() {
        let _cozo = temp_cozo();
        let active = register(&pk(12), "active").await.expect("register active");
        let revoked = register(&pk(13), "revoked")
            .await
            .expect("register revoked");
        revoke(&pk(13)).await.expect("revoke");

        let found = get(principal_id_of(&active)).await;
        assert_eq!(
            found["principal"]["display_alias"].as_str().unwrap(),
            "active"
        );

        let revoked_found = get(principal_id_of(&revoked)).await;
        assert!(
            revoked_found["principal"]["revoked_at"].as_i64().unwrap() > 0,
            "revoked records remain fetchable by id"
        );

        assert!(
            get("00000000-0000-0000-0000-000000000000").await["principal"].is_null(),
            "unknown id is not-found"
        );
        assert!(
            get("not-a-uuid").await["principal"].is_null(),
            "malformed id is not-found, no panic"
        );
    }

    /// VAL-REGISTRY-015: list_principals returns all principals including
    /// revoked; empty registry lists empty.
    #[tokio::test]
    async fn list_principals_includes_revoked() {
        let _cozo = temp_cozo();
        assert!(list().await.is_empty(), "empty registry lists empty");

        register(&pk(14), "one").await.expect("register one");
        register(&pk(15), "two").await.expect("register two");
        revoke(&pk(15)).await.expect("revoke two");

        let all = list().await;
        assert_eq!(all.len(), 2, "revoked records are not hidden");
        let one = all
            .iter()
            .find(|p| p["display_alias"].as_str() == Some("one"))
            .expect("one present");
        let two = all
            .iter()
            .find(|p| p["display_alias"].as_str() == Some("two"))
            .expect("two present");
        assert!(one["revoked_at"].is_null());
        assert!(two["revoked_at"].as_i64().unwrap() > 0);
    }

    /// VAL-REGISTRY-016: set_alias updates the alias, changes nothing else,
    /// and preserves active-alias uniqueness.
    #[tokio::test]
    async fn set_alias_updates_and_preserves_active_uniqueness() {
        let _cozo = temp_cozo();
        let a = register(&pk(16), "a1").await.expect("register A");
        let a_id = principal_id_of(&a).to_string();
        let b = register(&pk(17), "b1").await.expect("register B");
        let b_id = principal_id_of(&b).to_string();

        let updated = set_alias(&a_id, "a2").await.expect("legal update");
        assert_eq!(
            updated["principal"]["display_alias"].as_str().unwrap(),
            "a2"
        );
        // Nothing else changed.
        assert_eq!(updated["principal"]["principal_id"].as_str().unwrap(), a_id);
        assert_eq!(
            updated["principal"]["human_pubkey"].as_str().unwrap(),
            a["principal"]["human_pubkey"].as_str().unwrap()
        );
        assert_eq!(
            updated["principal"]["registered_at"].as_i64().unwrap(),
            a["principal"]["registered_at"].as_i64().unwrap()
        );
        assert_eq!(
            resolve(&pk(16)).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "a2",
            "update visible via resolve_key"
        );

        let collision = set_alias(&b_id, "a2").await;
        assert!(collision.is_err(), "active-alias collision rejected");
        assert_eq!(
            get(&b_id).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "b1",
            "rejected collision leaves B unchanged"
        );

        // Alias held only by a revoked principal succeeds.
        revoke(&pk(16)).await.expect("revoke A");
        set_alias(&b_id, "a2")
            .await
            .expect("alias held only by a revoked principal is free");
    }

    /// VAL-REGISTRY-017: alias is never authoritative — an alias in the
    /// pubkey argument resolves to not-found.
    #[tokio::test]
    async fn alias_is_never_authoritative() {
        let _cozo = temp_cozo();
        register(&pk(18), "alice").await.expect("register");
        let resolved = resolve("alice").await;
        assert!(
            resolved["principal"].is_null(),
            "alias must never resolve to a principal"
        );
    }

    /// VAL-REGISTRY-019: register flows through MutationEngine::mutate's
    /// human_principal branch; resolve flows through dispatch_method_call's
    /// human_principal arm. Removing either touch-point breaks this test.
    #[tokio::test]
    async fn plugin_service_round_trip_both_touch_points() {
        let _cozo = temp_cozo();
        let _shm = temp_shm();
        let engine = test_engine();
        let pubkey = pk(31);

        let mutation = engine
            .mutate(
                "human_principal".to_string(),
                "/org/opdbus/v1/plugins/human_principal".to_string(),
                ChangeType::MethodCall,
                Some("register_key".to_string()),
                simd_json::json!([{ "human_pubkey": pubkey, "display_alias": "wiring" }]),
                "did:op:human:wiring".to_string(),
                Some("human_principal.write".to_string()),
            )
            .await
            .expect("register through MutationEngine::mutate");
        assert!(mutation.success);
        let derived = op_identity::session::derive_principal_id(&pubkey);
        let mutation_result = mutation.result.expect("caller result");
        let mutation_result =
            serde_json::to_value(&mutation_result).expect("caller result serializes");
        assert_eq!(
            mutation_result["principal"]["principal_id"]
                .as_str()
                .unwrap(),
            derived
        );

        let envelope = engine
            .dispatch_method_call(
                "human_principal",
                "resolve_key",
                &serde_json::json!({ "human_pubkey": pubkey }).to_string(),
                Some("human_principal.read"),
                "did:op:human:wiring",
            )
            .await
            .expect("resolve through dispatch_method_call");
        assert!(envelope["success"].as_bool().unwrap());
        assert_eq!(
            envelope["result"]["principal"]["principal_id"]
                .as_str()
                .unwrap(),
            derived,
            "round-trip through both touch-points"
        );
        assert_eq!(
            envelope["result"]["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "wiring"
        );
    }

    /// VAL-REGISTRY-020: through the bridge's enforce_bridge_capability gate,
    /// every one of the six methods is denied without its schema-declared
    /// capability grant and permitted with it. Capability strings come from
    /// the schema, never hardcoded.
    #[tokio::test]
    async fn capability_gate_matrix() {
        let schema = op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .expect("human_principal schema");
        let mut schema_json = serde_json::to_value(&schema).expect("schema serializes");
        let methods = schema_json["methods"]
            .as_object()
            .expect("methods object")
            .clone();
        assert_eq!(methods.len(), 6, "exactly the six contract methods");

        let principal_id = "did:op:human:gate-matrix-test".to_string();
        let identity = GhostbridgeIdentity {
            principal_id: principal_id.clone(),
            session_id: "gate-matrix-test".to_string(),
            session_genesis: "ab".repeat(32),
        };
        let grants_dir = tempfile::tempdir().expect("grants dir");
        let grants_path = grants_dir.path().join("capability-grants.json");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };

        for (method, decl) in &methods {
            let capability = decl["required_capability"]
                .as_str()
                .unwrap_or_else(|| panic!("{method} declares a capability"));

            // Embedded schema grants never authorize. With an empty external
            // exact-principal store, no header value may open the method.
            schema_json["capability_grants"] =
                serde_json::json!({ principal_id.clone(): [capability] });
            std::fs::write(&grants_path, b"{}").expect("write empty grants");
            for header in [None, Some("unrelated.capability"), Some(capability)] {
                assert!(
                    crate::grpc_server::enforce_bridge_capability_with_schema(
                        Some(&schema_json),
                        "human_principal",
                        method,
                        header,
                        Some(&identity),
                    )
                    .is_err(),
                    "{method} must be denied without its grant (header {header:?})"
                );
            }

            // The authoritative exact-principal store permits the matching
            // declaration even when the embedded schema contains only `*`.
            schema_json["capability_grants"] = serde_json::json!({ "*": [capability] });
            std::fs::write(
                &grants_path,
                serde_json::to_vec(&serde_json::json!({
                    principal_id.clone(): { "capabilities": [capability] }
                }))
                .expect("serialize grants"),
            )
            .expect("write exact grant");
            assert!(
                crate::grpc_server::enforce_bridge_capability_with_schema(
                    Some(&schema_json),
                    "human_principal",
                    method,
                    Some(capability),
                    Some(&identity),
                )
                .is_ok(),
                "{method} must be permitted with its grant"
            );
        }
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    /// VAL-REGISTRY-021: malformed pubkeys are rejected with zero state change.
    #[tokio::test]
    async fn register_key_rejects_malformed_pubkeys() {
        let _cozo = temp_cozo();
        let sixteen_bytes = base64::engine::general_purpose::STANDARD.encode([7u8; 16]);
        let thirty_three_bytes = base64::engine::general_purpose::STANDARD.encode([7u8; 33]);
        for bad in ["", "not-base64!!", &sixteen_bytes, &thirty_three_bytes] {
            let result = register(bad, "mal").await;
            assert!(result.is_err(), "malformed pubkey {bad:?} must error");
        }
        assert!(list().await.is_empty(), "zero state change");
    }

    /// VAL-REGISTRY-022: revocation is a permanent tombstone — the key can
    /// never be re-registered.
    #[tokio::test]
    async fn revocation_is_permanent_tombstone() {
        let _cozo = temp_cozo();
        let pubkey = pk(19);
        register(&pubkey, "doomed").await.expect("register");
        let revoked = revoke(&pubkey).await.expect("revoke");
        let revoked_at = revoked["principal"]["revoked_at"].as_i64().unwrap();

        let reregister = register(&pubkey, "phoenix").await;
        assert!(reregister.is_err(), "revoked key can never re-register");

        let all = list().await;
        assert_eq!(all.len(), 1, "tombstone not duplicated");
        assert_eq!(
            all[0]["revoked_at"].as_i64().unwrap(),
            revoked_at,
            "tombstone revoked_at intact"
        );
    }

    /// VAL-REGISTRY-023: set_alias edge states — self no-op, empty clears,
    /// revoked targets are unconstrained display-only retitles.
    #[tokio::test]
    async fn set_alias_edge_states() {
        let _cozo = temp_cozo();
        let a = register(&pk(20), "same").await.expect("register A");
        let a_id = principal_id_of(&a).to_string();

        // (a) Setting the alias the principal already holds is a no-op, not
        // a self-collision.
        set_alias(&a_id, "same")
            .await
            .expect("self-alias is a no-op");
        assert_eq!(
            get(&a_id).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "same"
        );

        // (c) Clearing to the empty alias succeeds and collides with nothing.
        set_alias(&a_id, "").await.expect("clearing alias succeeds");
        assert_eq!(
            get(&a_id).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            ""
        );
        let b = register(&pk(21), "").await.expect("another empty alias");
        let b_id = principal_id_of(&b).to_string();

        // (d) set_alias on a REVOKED principal is allowed (display-only) —
        // it may even take an alias held by an active principal.
        let c = register(&pk(22), "held-by-active")
            .await
            .expect("register C");
        let c_id = principal_id_of(&c).to_string();
        revoke(&pk(22)).await.expect("revoke C");
        set_alias(&c_id, "held-by-active")
            .await
            .expect("revoked self-alias no-op");
        set_alias(&b_id, "active-held")
            .await
            .expect("B takes its own new alias");
        set_alias(&c_id, "active-held")
            .await
            .expect("a revoked principal may take an alias held by an active principal");
        assert_eq!(
            get(&c_id).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "active-held"
        );
    }

    /// VAL-REGISTRY-024: schema input validation at the PluginService
    /// surface — missing arg, wrong-typed arg, nonexistent method all error
    /// with zero state change.
    #[tokio::test]
    async fn plugin_service_input_validation() {
        let _cozo = temp_cozo();
        let _shm = temp_shm();
        // The test-only dispatcher supplies an explicit caller principal; grant
        // only that identity the capabilities required by this validation test.
        let grants_dir = tempfile::tempdir().expect("grants dir");
        let grants_path = grants_dir.path().join("capability-grants.json");
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&serde_json::json!({
                (SchemaBackedInterface::TEST_CALLER_PRINCIPAL): {
                    "capabilities": ["human_principal.write", "human_principal.read"]
                }
            }))
            .expect("serialize grants"),
        )
        .expect("write grants");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };

        let engine = test_engine();
        let iface = SchemaBackedInterface::with_engine(
            "human_principal".to_string(),
            human_principal_route(),
            Some(engine),
        );

        let list_before = iface
            .call_in_test("list_principals".to_string(), "{}".to_string())
            .await
            .expect("list before");

        let missing = iface
            .call_in_test("register_key".to_string(), "{}".to_string())
            .await;
        assert!(missing.is_err(), "missing required argument must error");

        let wrong_typed = iface
            .call_in_test(
                "register_key".to_string(),
                r#"{"human_pubkey": 42}"#.to_string(),
            )
            .await;
        assert!(wrong_typed.is_err(), "wrong-typed argument must error");

        let nonexistent = iface
            .call_in_test("nonexistent_method".to_string(), "{}".to_string())
            .await;
        assert!(nonexistent.is_err(), "nonexistent method must error");

        let list_after = iface
            .call_in_test("list_principals".to_string(), "{}".to_string())
            .await
            .expect("list after");
        let before: JsonValue = serde_json::from_str(&list_before).unwrap();
        let after: JsonValue = serde_json::from_str(&list_after).unwrap();
        assert_eq!(
            before["result"], after["result"],
            "list_principals byte-identical: zero state change"
        );
        assert_eq!(before["result"]["principals"].as_array().unwrap().len(), 0);

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    fn human_principal_route() -> PluginRoute {
        let schema = op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .expect("human_principal schema");
        let schema_json = serde_json::to_value(&schema).expect("schema serializes");
        let methods: HashMap<String, JsonValue> = schema_json["methods"]
            .as_object()
            .expect("methods object")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        PluginRoute {
            plugin_id: "human_principal".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/human_principal".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods,
            signals: vec![],
            properties: HashMap::new(),
        }
    }

    /// VAL-REGISTRY-028 implementation (pinned to the crate root as
    /// `set_alias_cross_state_collision`): both cross-state collision
    /// branches are allowed.
    pub(crate) async fn set_alias_cross_state_collision_impl() {
        let _cozo = temp_cozo();

        // Branch 1: a REVOKED principal may take an alias held by an ACTIVE
        // principal (uniqueness holds among active principals only).
        let active = register(&pk(23), "taken").await.expect("register active");
        let active_id = principal_id_of(&active).to_string();
        let revoked = register(&pk(24), "other")
            .await
            .expect("register to-revoke");
        let revoked_id = principal_id_of(&revoked).to_string();
        revoke(&pk(24)).await.expect("revoke");
        set_alias(&revoked_id, "taken")
            .await
            .expect("revoked principal may take an active-held alias");
        assert_eq!(
            get(&active_id).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "taken",
            "active principal's alias untouched"
        );

        // Branch 2: an ACTIVE principal may take an alias held only by a
        // REVOKED principal.
        let third = register(&pk(25), "third").await.expect("register third");
        let third_id = principal_id_of(&third).to_string();
        set_alias(&third_id, "other")
            .await
            .expect("active principal may take a revoked-held alias");
        assert_eq!(
            resolve(&pk(25)).await["principal"]["display_alias"]
                .as_str()
                .unwrap(),
            "other"
        );
        // The revoked tombstone keeps its own alias (historical record).
        let tombstone = get(&revoked_id).await;
        assert!(tombstone["principal"]["revoked_at"].as_i64().unwrap() > 0);
        assert_eq!(
            tombstone["principal"]["display_alias"].as_str().unwrap(),
            "taken"
        );
    }

    /// VAL-REGISTRY-029 implementation (pinned to the crate root as
    /// `register_key_unwritable_cozo_fails_clean`): an unwritable Cozo path
    /// fails register_key cleanly — no partial record, no panic, unchanged
    /// list.
    pub(crate) async fn register_key_unwritable_cozo_fails_clean_impl() {
        let _cozo = temp_cozo();
        // A path UNDER A FILE can never become a directory (ENOTDIR) — a
        // deterministic unwritable location on any host.
        let blocker = tempfile::NamedTempFile::new().expect("blocker file");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
                blocker.path().join("cozo"),
            )
        };

        let result = register(&pk(26), "blocked").await;
        assert!(result.is_err(), "unwritable store must error, not panic");
        assert!(
            list().await.is_empty(),
            "no partial record; list unchanged (degraded read is empty)"
        );
        assert!(
            resolve(&pk(26)).await["principal"].is_null(),
            "resolve against the broken store is not-found"
        );
    }

    /// VAL-CROSS-020 implementation (pinned to the crate root as
    /// `registry_mutations_are_audit_recorded`): register_key, set_alias and
    /// revoke_key through MutationEngine::mutate each append an event
    /// carrying the method subid, actor_id and capability_id, in call order.
    pub(crate) async fn registry_mutations_are_audit_recorded_impl() {
        let _cozo = temp_cozo();
        let _shm = temp_shm();
        let engine = test_engine();
        let actor = "did:op:human:audit";
        let capability = "human_principal.write";
        let pubkey = pk(41);
        let path = "/org/opdbus/v1/plugins/human_principal".to_string();

        engine
            .mutate(
                "human_principal".to_string(),
                path.clone(),
                ChangeType::MethodCall,
                Some("register_key".to_string()),
                simd_json::json!([{ "human_pubkey": pubkey, "display_alias": "audit-a" }]),
                actor.to_string(),
                Some(capability.to_string()),
            )
            .await
            .expect("register through mutate");
        let principal_id = op_identity::session::derive_principal_id(&pubkey);
        engine
            .mutate(
                "human_principal".to_string(),
                path.clone(),
                ChangeType::MethodCall,
                Some("set_alias".to_string()),
                simd_json::json!([{ "principal_id": principal_id, "display_alias": "audit-b" }]),
                actor.to_string(),
                Some(capability.to_string()),
            )
            .await
            .expect("set_alias through mutate");
        engine
            .mutate(
                "human_principal".to_string(),
                path,
                ChangeType::MethodCall,
                Some("revoke_key".to_string()),
                simd_json::json!([{ "human_pubkey": pubkey }]),
                actor.to_string(),
                Some(capability.to_string()),
            )
            .await
            .expect("revoke through mutate");

        let chain = engine.event_chain.read().await;
        let events: Vec<_> = chain
            .events_for_plugin("human_principal")
            .into_iter()
            .filter(|event| !event.tags_touched.is_empty())
            .collect();
        assert_eq!(events.len(), 3, "one tagged event per mutation");
        let expected_subids = [
            "mut.service.human-principal.key.register@v1",
            "mut.service.human-principal.alias.set@v1",
            "mut.service.human-principal.key.revoke@v1",
        ];
        for (event, subid) in events.iter().zip(expected_subids) {
            assert!(
                event.tags_touched.iter().any(|tag| tag == subid),
                "event must carry subid {subid} in tags_touched, got {:?}",
                event.tags_touched
            );
            assert_eq!(event.actor_id, actor, "actor_id carried");
            assert_eq!(
                event.capability_id.as_deref(),
                Some(capability),
                "capability_id carried"
            );
        }
    }
}
