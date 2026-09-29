//! Durable method adapter for the `memory` plugin.
//!
//! The plugin schema remains in `memory_plugin.rs`. This module stores only
//! caller-scoped mutable domain data in the existing Cozo runtime-document
//! relation and never publishes entry contents to projections or logs.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use op_cozo_store::CozoGraphShuttle;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[cfg(test)]
use serde_json::json;

use super::memory_plugin::{
    ClearNamespaceInput, ClearNamespaceOutput, CompactOutput, CreateNamespaceInput,
    CreateNamespaceOutput, DeleteEntryInput, DeleteEntryOutput, DeleteNamespaceInput,
    DeleteNamespaceOutput, EntryInfo, GetEntryInput, GetEntryOutput, GetStatsOutput,
    InsertEntryInput, InsertEntryOutput, NamespaceInfo, NamespaceStats, QueryNamespaceInput,
    QueryNamespaceOutput, UpdateEntryInput, UpdateEntryOutput,
};
use super::runtime_documents::{RuntimeDocument, RuntimeDocuments};

type Compactor = Arc<dyn Fn() -> Result<()> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAccessContext {
    pub principal_id: String,
    pub session_id: String,
}

impl MemoryAccessContext {
    fn validate(&self) -> Result<()> {
        validate_identity_component("principal_id", &self.principal_id)?;
        validate_identity_component("session_id", &self.session_id)
    }
}

/// Long-lived method service. Callers should share one instance per process so
/// every update uses the same [`RuntimeDocuments`] writer lock.
pub struct MemoryMethods {
    documents: Arc<RuntimeDocuments>,
    compactor: Compactor,
}

impl MemoryMethods {
    /// Create the one process-wide memory service over the bridge's injected
    /// Cozo handle. Both document writes and compaction use this exact handle.
    pub fn from_shared(store: Arc<CozoGraphShuttle>) -> Self {
        let documents = Arc::new(RuntimeDocuments::from_shared(store.clone()));
        let compaction_store = store;
        Self {
            documents,
            compactor: Arc::new(move || {
                compaction_store
                    .run_query("::compact", None)
                    .context("Cozo compaction failed")?;
                Ok(())
            }),
        }
    }

    #[cfg(test)]
    fn with_compactor(documents: Arc<RuntimeDocuments>, compactor: Compactor) -> Self {
        Self {
            documents,
            compactor,
        }
    }

    pub async fn dispatch(
        &self,
        context: &MemoryAccessContext,
        method: &str,
        args: &Value,
    ) -> Result<Value> {
        context.validate()?;
        let owner = owner_document_key(context);
        match method {
            "create_namespace" => {
                let input: CreateNamespaceInput = serde_json::from_value(args.clone())?;
                self.create_namespace(&owner, input).await
            }
            "delete_namespace" => {
                let input: DeleteNamespaceInput = serde_json::from_value(args.clone())?;
                self.delete_namespace(&owner, input).await
            }
            "query_namespace" => {
                let input: QueryNamespaceInput = serde_json::from_value(args.clone())?;
                self.query_namespace(&owner, input).await
            }
            "insert_entry" => {
                let input: InsertEntryInput = serde_json::from_value(args.clone())?;
                self.insert_entry(&owner, input).await
            }
            "update_entry" => {
                let input: UpdateEntryInput = serde_json::from_value(args.clone())?;
                self.update_entry(&owner, input).await
            }
            "get_entry" => {
                let input: GetEntryInput = serde_json::from_value(args.clone())?;
                self.get_entry(&owner, input).await
            }
            "delete_entry" => {
                let input: DeleteEntryInput = serde_json::from_value(args.clone())?;
                self.delete_entry(&owner, input).await
            }
            "clear_namespace" => {
                let input: ClearNamespaceInput = serde_json::from_value(args.clone())?;
                self.clear_namespace(&owner, input).await
            }
            "get_stats" => {
                validate_empty_input(args)?;
                self.get_stats(&owner).await
            }
            "compact" => {
                validate_empty_input(args)?;
                self.compact().await
            }
            _ => bail!("unsupported memory method"),
        }
    }

    async fn read_document(&self, owner: &str) -> Result<MemoryDocument> {
        self.documents
            .read(owner)
            .await?
            .map(parse_document)
            .transpose()
            .map(|document| document.unwrap_or_default())
    }

    async fn create_namespace(&self, owner: &str, input: CreateNamespaceInput) -> Result<Value> {
        validate_namespace(&input.name)?;
        if let Some(max_entries) = input.max_entries {
            ensure!(max_entries > 0, "max_entries must be positive when set");
        }
        validate_domain_value(&input.config)?;
        let name = input.name;
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let (row, (namespace, created)) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                if let Some(existing) = document.namespaces.get(&name) {
                    ensure!(
                        existing.max_entries == input.max_entries
                            && existing.read_only == input.read_only
                            && existing.config == input.config,
                        "namespace already exists with different settings"
                    );
                    let existing = existing.clone();
                    return Ok((serde_json::to_value(document)?, (existing, false)));
                }
                let namespace = StoredNamespace {
                    id,
                    name: name.clone(),
                    max_entries: input.max_entries,
                    read_only: input.read_only,
                    config: input.config,
                    entries: BTreeMap::new(),
                    created_at: now.clone(),
                    updated_at: now,
                };
                document.namespaces.insert(name, namespace.clone());
                Ok((serde_json::to_value(document)?, (namespace, true)))
            })
            .await?;
        let _revision = row.revision;
        Ok(serde_json::to_value(CreateNamespaceOutput {
            namespace: namespace_info(&namespace)?,
            created,
        })?)
    }

    async fn delete_namespace(&self, owner: &str, input: DeleteNamespaceInput) -> Result<Value> {
        validate_namespace(&input.name)?;
        let name = input.name;
        let (_, receipt) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                if let Some(namespace) = document.namespaces.get(&name) {
                    ensure!(!namespace.read_only, "namespace is read-only");
                }
                let removed = document.namespaces.remove(&name);
                let deleted_entries = removed
                    .as_ref()
                    .map(|namespace| u64::try_from(namespace.entries.len()))
                    .transpose()?
                    .unwrap_or(0);
                Ok((
                    serde_json::to_value(document)?,
                    (removed.is_some(), deleted_entries),
                ))
            })
            .await?;
        Ok(serde_json::to_value(DeleteNamespaceOutput {
            deleted: receipt.0,
            deleted_entries: receipt.1,
        })?)
    }

    async fn query_namespace(&self, owner: &str, input: QueryNamespaceInput) -> Result<Value> {
        validate_namespace(&input.name)?;
        let document = self.read_document(owner).await?;
        let namespace = document
            .namespaces
            .get(&input.name)
            .map(namespace_info)
            .transpose()?;
        Ok(serde_json::to_value(QueryNamespaceOutput { namespace })?)
    }

    async fn insert_entry(&self, owner: &str, input: InsertEntryInput) -> Result<Value> {
        validate_namespace(&input.namespace)?;
        validate_entry_key(&input.key)?;
        validate_domain_value(&input.value)?;
        let namespace_name = input.namespace;
        let key = input.key;
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let (_, (namespace, entry)) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                let namespace = document
                    .namespaces
                    .get_mut(&namespace_name)
                    .context("namespace not found")?;
                ensure!(!namespace.read_only, "namespace is read-only");
                ensure!(
                    !namespace.entries.contains_key(&key),
                    "entry already exists"
                );
                if let Some(max_entries) = namespace.max_entries {
                    ensure!(
                        u64::try_from(namespace.entries.len())? < max_entries,
                        "namespace capacity reached"
                    );
                }
                let entry = StoredEntry {
                    id,
                    key: key.clone(),
                    value: input.value,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                namespace.entries.insert(key, entry.clone());
                namespace.updated_at = now;
                let namespace = namespace.clone();
                Ok((serde_json::to_value(document)?, (namespace, entry)))
            })
            .await?;
        Ok(serde_json::to_value(InsertEntryOutput {
            entry: entry_info(&namespace.name, &entry),
            entry_count: u64::try_from(namespace.entries.len())?,
        })?)
    }

    async fn update_entry(&self, owner: &str, input: UpdateEntryInput) -> Result<Value> {
        validate_namespace(&input.namespace)?;
        validate_entry_key(&input.key)?;
        validate_domain_value(&input.value)?;
        let namespace_name = input.namespace;
        let key = input.key;
        let now = chrono::Utc::now().to_rfc3339();
        let (_, (namespace_name, entry, previous_value)) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                let namespace = document
                    .namespaces
                    .get_mut(&namespace_name)
                    .context("namespace not found")?;
                ensure!(!namespace.read_only, "namespace is read-only");
                let entry = namespace.entries.get_mut(&key).context("entry not found")?;
                let previous_value = std::mem::replace(&mut entry.value, input.value);
                entry.updated_at = now.clone();
                let entry = entry.clone();
                namespace.updated_at = now;
                let namespace_name = namespace_name.clone();
                Ok((
                    serde_json::to_value(document)?,
                    (namespace_name, entry, previous_value),
                ))
            })
            .await?;
        Ok(serde_json::to_value(UpdateEntryOutput {
            entry: entry_info(&namespace_name, &entry),
            previous_value,
        })?)
    }

    async fn get_entry(&self, owner: &str, input: GetEntryInput) -> Result<Value> {
        validate_namespace(&input.namespace)?;
        validate_entry_key(&input.key)?;
        let document = self.read_document(owner).await?;
        let entry = document
            .namespaces
            .get(&input.namespace)
            .and_then(|namespace| namespace.entries.get(&input.key))
            .map(|entry| entry_info(&input.namespace, entry));
        Ok(serde_json::to_value(GetEntryOutput { entry })?)
    }

    async fn delete_entry(&self, owner: &str, input: DeleteEntryInput) -> Result<Value> {
        validate_namespace(&input.namespace)?;
        validate_entry_key(&input.key)?;
        let namespace_name = input.namespace;
        let key = input.key;
        let now = chrono::Utc::now().to_rfc3339();
        let (_, deleted_entry_id) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                let namespace = document
                    .namespaces
                    .get_mut(&namespace_name)
                    .context("namespace not found")?;
                ensure!(!namespace.read_only, "namespace is read-only");
                let removed = namespace.entries.remove(&key).map(|entry| entry.id);
                if removed.is_some() {
                    namespace.updated_at = now;
                }
                Ok((serde_json::to_value(document)?, removed))
            })
            .await?;
        Ok(serde_json::to_value(DeleteEntryOutput {
            deleted_entry_id,
        })?)
    }

    async fn clear_namespace(&self, owner: &str, input: ClearNamespaceInput) -> Result<Value> {
        validate_namespace(&input.name)?;
        let name = input.name;
        let now = chrono::Utc::now().to_rfc3339();
        let (_, deleted_entries) = self
            .documents
            .update_with_result(owner, move |old| {
                let mut document = parse_optional_document(old)?;
                let namespace = document
                    .namespaces
                    .get_mut(&name)
                    .context("namespace not found")?;
                ensure!(!namespace.read_only, "namespace is read-only");
                let deleted_entries = u64::try_from(namespace.entries.len())?;
                namespace.entries.clear();
                if deleted_entries > 0 {
                    namespace.updated_at = now;
                }
                Ok((serde_json::to_value(document)?, deleted_entries))
            })
            .await?;
        Ok(serde_json::to_value(ClearNamespaceOutput {
            deleted_entries,
        })?)
    }

    async fn get_stats(&self, owner: &str) -> Result<Value> {
        let document = self.read_document(owner).await?;
        let namespaces = document
            .namespaces
            .values()
            .map(|namespace| {
                Ok(NamespaceStats {
                    name: namespace.name.clone(),
                    entries: u64::try_from(namespace.entries.len())?,
                    max_entries: namespace.max_entries,
                    read_only: namespace.read_only,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let total_entries = namespaces.iter().try_fold(0_u64, |total, namespace| {
            total
                .checked_add(namespace.entries)
                .context("entry count overflow")
        })?;
        Ok(serde_json::to_value(GetStatsOutput {
            total_namespaces: u64::try_from(namespaces.len())?,
            total_entries,
            namespaces,
        })?)
    }

    async fn compact(&self) -> Result<Value> {
        let compactor = self.compactor.clone();
        tokio::task::spawn_blocking(move || compactor()).await??;
        Ok(serde_json::to_value(CompactOutput {
            scope: "cozo_store".to_string(),
            compacted_at: chrono::Utc::now().to_rfc3339(),
            reclaimed_bytes: None,
        })?)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MemoryDocument {
    namespaces: BTreeMap<String, StoredNamespace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredNamespace {
    id: String,
    name: String,
    max_entries: Option<u64>,
    read_only: bool,
    config: Value,
    entries: BTreeMap<String, StoredEntry>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEntry {
    id: String,
    key: String,
    value: Value,
    created_at: String,
    updated_at: String,
}

fn parse_optional_document(value: Option<Value>) -> Result<MemoryDocument> {
    value
        .map(|document| serde_json::from_value(document).context("invalid memory document"))
        .transpose()
        .map(|document| document.unwrap_or_default())
}

fn parse_document(row: RuntimeDocument) -> Result<MemoryDocument> {
    serde_json::from_value(row.document).context("invalid memory document")
}

fn namespace_info(namespace: &StoredNamespace) -> Result<NamespaceInfo> {
    Ok(NamespaceInfo {
        id: namespace.id.clone(),
        name: namespace.name.clone(),
        max_entries: namespace.max_entries,
        read_only: namespace.read_only,
        entry_count: u64::try_from(namespace.entries.len())?,
        config: namespace.config.clone(),
        created_at: namespace.created_at.clone(),
        updated_at: namespace.updated_at.clone(),
    })
}

fn entry_info(namespace: &str, entry: &StoredEntry) -> EntryInfo {
    EntryInfo {
        id: entry.id.clone(),
        namespace: namespace.to_string(),
        key: entry.key.clone(),
        value: entry.value.clone(),
        created_at: entry.created_at.clone(),
        updated_at: entry.updated_at.clone(),
    }
}

fn validate_identity_component(field: &str, value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "verified {field} is required");
    ensure!(value.len() <= 512, "verified {field} is too long");
    ensure!(
        !value.chars().any(char::is_control),
        "verified {field} contains control characters"
    );
    Ok(())
}

fn validate_namespace(value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "namespace name is required");
    ensure!(value.len() <= 128, "namespace name is too long");
    ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.:/".contains(ch)),
        "namespace name contains unsupported characters"
    );
    Ok(())
}

fn validate_entry_key(value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "entry key is required");
    ensure!(value.len() <= 512, "entry key is too long");
    ensure!(
        !value.chars().any(char::is_control),
        "entry key contains control characters"
    );
    Ok(())
}

fn validate_domain_value(value: &Value) -> Result<()> {
    ensure!(
        serde_json::to_vec(value)?.len() <= 1024 * 1024,
        "entry value exceeds 1 MiB"
    );
    reject_credential_fields(value)?;
    Ok(())
}

fn reject_credential_fields(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                ensure!(
                    !matches!(
                        normalized.as_str(),
                        "password"
                            | "passwd"
                            | "secret"
                            | "token"
                            | "access_token"
                            | "refresh_token"
                            | "api_key"
                            | "private_key"
                            | "psk"
                            | "sealed_id"
                            | "credential"
                            | "credentials"
                    ),
                    "memory values cannot store credential fields"
                );
                reject_credential_fields(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                reject_credential_fields(nested)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_empty_input(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .context("method input must be an object")?;
    ensure!(object.is_empty(), "method does not accept input fields");
    Ok(())
}

fn owner_document_key(context: &MemoryAccessContext) -> String {
    let mut hash = Sha256::new();
    hash.update(b"op-dbus.memory.owner.v1\0");
    hash.update(
        u64::try_from(context.principal_id.len())
            .expect("validated identity length fits u64")
            .to_be_bytes(),
    );
    hash.update(context.principal_id.as_bytes());
    hash.update(
        u64::try_from(context.session_id.len())
            .expect("validated identity length fits u64")
            .to_be_bytes(),
    );
    hash.update(context.session_id.as_bytes());
    format!("memory:{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(principal: &str, session: &str) -> MemoryAccessContext {
        MemoryAccessContext {
            principal_id: principal.to_string(),
            session_id: session.to_string(),
        }
    }

    fn fixture() -> (MemoryMethods, MemoryAccessContext) {
        let store = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        (
            MemoryMethods::from_shared(store),
            context("principal-a", "session-a"),
        )
    }

    #[tokio::test]
    async fn retained_crud_methods_enforce_capacity_readonly_and_stable_ids() {
        let (service, owner) = fixture();
        let created = service
            .dispatch(
                &owner,
                "create_namespace",
                &json!({"name":"notes","max_entries":1,"read_only":false,"config":{"purpose":"test"}}),
            )
            .await
            .unwrap();
        assert_eq!(created["created"], true);
        assert_eq!(created["namespace"]["max_entries"], 1);
        let namespace_id = created["namespace"]["id"].as_str().unwrap().to_string();
        let existing = service
            .dispatch(
                &owner,
                "create_namespace",
                &json!({"name":"notes","max_entries":1,"read_only":false,"config":{"purpose":"test"}}),
            )
            .await
            .unwrap();
        assert_eq!(existing["created"], false);
        assert_eq!(existing["namespace"]["id"], namespace_id);

        let inserted = service
            .dispatch(
                &owner,
                "insert_entry",
                &json!({"namespace":"notes","key":"one","value":{"note":"kept-private"}}),
            )
            .await
            .unwrap();
        let entry_id = inserted["entry"]["id"].as_str().unwrap().to_string();
        assert!(service
            .dispatch(
                &owner,
                "insert_entry",
                &json!({"namespace":"notes","key":"two","value":2}),
            )
            .await
            .is_err());

        let updated = service
            .dispatch(
                &owner,
                "update_entry",
                &json!({"namespace":"notes","key":"one","value":3}),
            )
            .await
            .unwrap();
        assert_eq!(updated["entry"]["id"], entry_id);
        assert_eq!(updated["previous_value"], json!({"note":"kept-private"}));

        let fetched = service
            .dispatch(
                &owner,
                "get_entry",
                &json!({"namespace":"notes","key":"one"}),
            )
            .await
            .unwrap();
        assert_eq!(fetched["entry"]["value"], 3);
        let queried = service
            .dispatch(&owner, "query_namespace", &json!({"name":"notes"}))
            .await
            .unwrap();
        assert_eq!(queried["namespace"]["entry_count"], 1);
        let stats = service
            .dispatch(&owner, "get_stats", &json!({}))
            .await
            .unwrap();
        assert_eq!(stats["total_entries"], 1);

        let deleted = service
            .dispatch(
                &owner,
                "delete_entry",
                &json!({"namespace":"notes","key":"one"}),
            )
            .await
            .unwrap();
        assert_eq!(deleted["deleted_entry_id"], entry_id);
        let cleared = service
            .dispatch(&owner, "clear_namespace", &json!({"name":"notes"}))
            .await
            .unwrap();
        assert_eq!(cleared["deleted_entries"], 0);
        let removed = service
            .dispatch(&owner, "delete_namespace", &json!({"name":"notes"}))
            .await
            .unwrap();
        assert_eq!(removed["deleted"], true);

        service
            .dispatch(
                &owner,
                "create_namespace",
                &json!({"name":"locked","read_only":true}),
            )
            .await
            .unwrap();
        let locked_owner = owner_document_key(&owner);
        service
            .documents
            .update(&locked_owner, |old| {
                let mut document = parse_optional_document(old)?;
                let namespace = document.namespaces.get_mut("locked").unwrap();
                namespace.entries.insert(
                    "seed".to_string(),
                    StoredEntry {
                        id: "fixture-entry".to_string(),
                        key: "seed".to_string(),
                        value: json!(1),
                        created_at: "t0".to_string(),
                        updated_at: "t0".to_string(),
                    },
                );
                Ok(serde_json::to_value(document)?)
            })
            .await
            .unwrap();
        assert!(service
            .dispatch(
                &owner,
                "insert_entry",
                &json!({"namespace":"locked","key":"no","value":1}),
            )
            .await
            .is_err());
        assert!(validate_domain_value(&json!({"api_key":"forbidden"})).is_err());
        for (method, input) in [
            (
                "update_entry",
                json!({"namespace":"locked","key":"seed","value":2}),
            ),
            ("delete_entry", json!({"namespace":"locked","key":"seed"})),
            ("clear_namespace", json!({"name":"locked"})),
            ("delete_namespace", json!({"name":"locked"})),
        ] {
            assert!(service.dispatch(&owner, method, &input).await.is_err());
        }
        let still_locked = service
            .dispatch(&owner, "query_namespace", &json!({"name":"locked"}))
            .await
            .unwrap();
        assert_eq!(still_locked["namespace"]["entry_count"], 1);
    }

    #[tokio::test]
    async fn exact_context_is_required_and_other_sessions_cannot_read_entries() {
        let (service, owner) = fixture();
        service
            .dispatch(&owner, "create_namespace", &json!({"name":"private"}))
            .await
            .unwrap();
        service
            .dispatch(
                &owner,
                "insert_entry",
                &json!({"namespace":"private","key":"one","value":"secret"}),
            )
            .await
            .unwrap();

        let other_session = context("principal-a", "session-b");
        let hidden = service
            .dispatch(
                &other_session,
                "get_entry",
                &json!({"namespace":"private","key":"one"}),
            )
            .await
            .unwrap();
        assert!(hidden["entry"].is_null());
        let other_principal = context("principal-b", "session-a");
        let hidden = service
            .dispatch(
                &other_principal,
                "get_entry",
                &json!({"namespace":"private","key":"one"}),
            )
            .await
            .unwrap();
        assert!(hidden["entry"].is_null());
        assert!(service
            .dispatch(&context("", "session-a"), "get_stats", &json!({}),)
            .await
            .is_err());
        assert_ne!(
            owner_document_key(&context("ab", "c")),
            owner_document_key(&context("a", "bc"))
        );
    }

    #[tokio::test]
    async fn compaction_errors_are_not_reported_as_completion() {
        let store = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        let documents = Arc::new(RuntimeDocuments::from_shared(store));
        let service = MemoryMethods::with_compactor(
            documents,
            Arc::new(|| bail!("unsupported fixture backend")),
        );
        assert!(service
            .dispatch(&context("principal", "session"), "compact", &json!({}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn persistent_cozo_executes_real_store_compaction_without_invented_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("memory-cozo");
        let owner = context("principal", "session");
        {
            let store = Arc::new(CozoGraphShuttle::new_persistent(path.clone()).unwrap());
            let service = MemoryMethods::from_shared(store);
            service
                .dispatch(&owner, "create_namespace", &json!({"name":"durable"}))
                .await
                .unwrap();
            service
                .dispatch(
                    &owner,
                    "insert_entry",
                    &json!({"namespace":"durable","key":"entry","value":7}),
                )
                .await
                .unwrap();
            let output = service
                .dispatch(&owner, "compact", &json!({}))
                .await
                .unwrap();
            assert_eq!(output["scope"], "cozo_store");
            assert!(output["reclaimed_bytes"].is_null());
            assert!(output["compacted_at"].as_str().is_some());
        }
        let reopened = Arc::new(CozoGraphShuttle::new_persistent(path).unwrap());
        let service = MemoryMethods::from_shared(reopened);
        let entry = service
            .dispatch(
                &owner,
                "get_entry",
                &json!({"namespace":"durable","key":"entry"}),
            )
            .await
            .unwrap();
        assert_eq!(entry["entry"]["value"], 7);
    }
}
