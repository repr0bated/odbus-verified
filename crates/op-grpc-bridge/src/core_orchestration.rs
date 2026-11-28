//! Scoped orchestration records are projections of the existing core append
//! owner. A CAS holds the same commit boundary through durable append and SHM
//! publication; no episode/index database participates in acceptance.
use anyhow::{Context, Result, ensure};
use op_state_store::method_receipt::{MethodEventProof, validate_json_size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::*;

const MAX_VALUE_BYTES: usize = 128 * 1024;
const MAX_PAGE: usize = 256;

/// Private append metadata, not another record body or public receipt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrchestrationDelta {
    pub domain: String,
    pub key: String,
    pub revision: u64,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<OrchestrationVersionDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrchestrationVersionDelta {
    pub key: String,
    pub revision: u64,
}

#[path = "core_owner_definitions.rs"]
mod owner_definitions;
pub(crate) use owner_definitions::{OwnerDefinition, DefinitionApprovalState};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationRecord {
    pub revision: u64,
    pub value: Value,
    pub owner_operation_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrchestrationCommit {
    pub record: OrchestrationRecord,
    pub proof: MethodEventProof,
}

/// Opaque native ownership for one exact scoped record. It retains attribution,
/// not a Runtime gate or authority to initiate another operation.
#[derive(Clone)]
pub(crate) struct OrchestrationWriter {
    operation: MethodOperation,
    domain: String,
    key: String,
    definition_guard: Option<std::sync::Arc<owner_definitions::DefinitionGuard>>,
}

fn validate_address(scope: &StateScope, domain: &str, key: &str) -> Result<()> {
    projection::validate_scope(scope)?;
    ensure!(matches!(scope, StateScope::Private { .. }), "orchestration requires verified private scope");
    ensure!(matches!(domain, "workflows" | "workstacks"), "unknown orchestration domain");
    ensure!(!key.is_empty() && key.len() <= 512 && !key.chars().any(char::is_control),
        "invalid orchestration record key");
    Ok(())
}

fn decode_record(value: &Value) -> Result<OrchestrationRecord> {
    let record: OrchestrationRecord = serde_json::from_value(value.clone())?;
    ensure!(record.revision > 0, "invalid committed orchestration revision");
    Ok(record)
}

fn definition_address<'a>(domain: &str, key: &'a str) -> Option<(&'a str, &'a str)> {
    let (kind, id) = key.split_once(':')?;
    (!id.is_empty() && matches!((domain, kind),
        ("workstacks", "role" | "policy" | "recipe") | ("workflows", "graph"))).then_some((kind, id))
}

fn immutable_definition_key(domain: &str, key: &str) -> bool {
    key.split_once(':').is_some_and(|(kind, _)| matches!((domain, kind),
        ("workstacks", "role_version" | "policy_version" | "recipe_version") | ("workflows", "graph_version")))
}

impl BridgeCore {
    pub fn orchestration_get(&self, scope: &StateScope, domain: &str, key: &str)
        -> Result<Option<OrchestrationRecord>> {
        validate_address(scope, domain, key)?;
        let Some(snapshot) = self.backend_snapshot(scope, domain, &op_plugins::canonical::plugin_path(domain))? else {
            return Ok(None);
        };
        snapshot.state.get("records").and_then(|records| records.get(key))
            .map(decode_record).transpose()
    }

    /// Bounded, sorted projection page. Prefix is a domain-owned record kind,
    /// never an authorization scope or a filesystem path.
    pub fn orchestration_list(&self, scope: &StateScope, domain: &str, prefix: &str, after: Option<&str>, limit: usize)
        -> Result<Vec<(String, OrchestrationRecord)>> {
        validate_address(scope, domain, if prefix.is_empty() { "_" } else { prefix })?;
        if let Some(after) = after { validate_address(scope, domain, after)?; }
        ensure!(limit > 0 && limit <= MAX_PAGE, "orchestration page limit must be 1..=256");
        let Some(snapshot) = self.backend_snapshot(scope, domain, &op_plugins::canonical::plugin_path(domain))? else {
            return Ok(Vec::new());
        };
        let Some(records) = snapshot.state.get("records") else { return Ok(Vec::new()); };
        let records = records.as_object().context("committed orchestration records are not an object")?;
        let mut keys: Vec<_> = records.keys().filter(|key| key.starts_with(prefix)
            && after.is_none_or(|after| key.as_str() > after)).collect();
        keys.sort_unstable();
        keys.into_iter().take(limit)
            .map(|key| Ok((key.clone(), decode_record(&records[key])?))).collect()
    }

    pub(crate) fn orchestration_writer(&self, operation: &MethodOperation, domain: &str, key: &str)
        -> Result<OrchestrationWriter> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let (scope, schema_hash, method, capability) = owner.orchestration_attribution(operation, domain)?;
        ensure!(domain != "workstacks" || !owner_definitions::is_definition_key(key),
            "owner definition requires a canonical global-head guard");
        validate_address(&scope, domain, key)?;
        let schema = owner.pin(&self.inner.catalog, domain, &schema_hash)?;
        let declaration = schema.methods.get(&method).context("orchestration mutator absent from sealed owner")?;
        ensure!(declaration.side_effect == op_state_store::SideEffect::Mutation
            && declaration.required_capability.as_deref() == Some(capability.as_str()),
            "only an actual admitted mutation can own orchestration records");
        Ok(OrchestrationWriter { operation: operation.clone(), domain: domain.to_owned(), key: key.to_owned(), definition_guard: None })
    }

    /// Recover projection ownership, not execution authority. The original
    /// acceptance terminal and record creator must both match this scope.
    pub(crate) fn recover_orchestration_writer(
        &self, scope: &StateScope, operation_id: &str, acceptance: &MethodEventProof,
        domain: &str, key: &str,
    ) -> Result<OrchestrationWriter> {
        validate_address(scope, domain, key)?;
        let operation = {
            let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
            let owner = self.inner.persistent.as_ref().context("durable bridge required")?
                .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
            owner.require_available()?;
            let snapshot = owner.snapshot(&self.inner.catalog, scope, domain,
                &op_plugins::canonical::plugin_path(domain), false)?.context("orchestration record unavailable")?;
            let record = decode_record(snapshot.state.get("records").and_then(|records| records.get(key))
                .context("orchestration record unavailable")?)?;
            ensure!(record.owner_operation_id == operation_id, "orchestration record has another acceptance owner");
            owner.recover_orchestration_operation(scope, operation_id, acceptance)?
        };
        self.orchestration_writer(&operation, domain, key)
    }

    /// Retained owners may reconcile already-dispatched effects after revocation.
    /// This does not re-admit effects or let a read grant mutate domain records.
    pub(crate) fn orchestration_cas(
        &self, writer: &OrchestrationWriter, expected_revision: Option<u64>, value: Value,
    ) -> Result<OrchestrationCommit> {
        self.orchestration_commit(writer, expected_revision, value, None).map(|(commit, _)| commit)
    }

    /// One existing append publishes an immutable definition and its head.
    /// No observable orphan body or head can survive a crash between writes.
    pub(crate) fn orchestration_publish_definition(
        &self, writer: &OrchestrationWriter, expected_revision: Option<u64>, head: Value,
        version_key: &str, version: Value,
    ) -> Result<(OrchestrationCommit, OrchestrationRecord)> {
        ensure!(definition_address(&writer.domain, &writer.key).is_some(), "unknown definition head kind");
        let (commit, version) = self.orchestration_commit(writer, expected_revision, head, Some((version_key, version)))?;
        Ok((commit, version.context("atomic definition version record absent")?))
    }

    fn orchestration_commit(
        &self, writer: &OrchestrationWriter, expected_revision: Option<u64>, value: Value,
        version: Option<(&str, Value)>,
    ) -> Result<(OrchestrationCommit, Option<OrchestrationRecord>)> {
        validate_json_size(&value, MAX_VALUE_BYTES)?;
        if let Some((_, value)) = &version { validate_json_size(value, MAX_VALUE_BYTES)?; }
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let domain = writer.domain.as_str();
        let key = writer.key.as_str();
        let (scope, schema_hash, method, capability_id) = owner.orchestration_attribution(&writer.operation, domain)?;
        ensure!(!immutable_definition_key(domain, key), "immutable definitions require atomic head publication");
        let definition = definition_address(domain, key);
        if definition.is_some() { owner.require_live_definition_writer(&writer.operation)?; }
        if let Some(guard) = &writer.definition_guard {
            owner.validate_definition_guard(&writer.operation, guard, key, &value, expected_revision)?;
            if let Some((version_key, version)) = &version {
                owner.validate_definition_guard(&writer.operation, guard, version_key, version, None)?;
            }
        } else {
            ensure!(domain != "workstacks" || !owner_definitions::is_definition_key(key),
                "owner definition requires a canonical global-head guard");
        }
        let StateScope::Private { principal_id: actor_id, .. } = &scope else { anyhow::bail!("private orchestration owner required"); };
        let path = op_plugins::canonical::plugin_path(domain);
        let snapshot = owner.snapshot(&self.inner.catalog, &scope, domain, &path, false)?;
        let mut root = snapshot.map(|snapshot| snapshot.state).unwrap_or_else(|| json!({}));
        let mut records = match root.as_object_mut().context("committed orchestration root is not an object")?.remove("records") {
            Some(Value::Object(records)) => records,
            None => serde_json::Map::new(),
            Some(_) => anyhow::bail!("committed orchestration records are not an object"),
        };
        let previous = records.get(key).map(decode_record).transpose()?;
        ensure!(previous.as_ref().map(|record| record.revision) == expected_revision,
            "orchestration revision conflict; reload the committed record");
        let revision = expected_revision.unwrap_or(0).checked_add(1).context("orchestration revision exhausted")?;
        let mut version_delta = None;
        let version_record = if let Some((version_key, version_value)) = version {
            let (kind, id) = definition.context("atomic publication requires a known definition head")?;
            validate_address(&scope, domain, version_key)?;
            let number = version_value.get("version").and_then(Value::as_u64).filter(|number| *number > 0)
                .context("immutable definition version absent")?;
            ensure!(version_key == format!("{kind}_version:{id}:{number:020}")
                && value.get("version").and_then(Value::as_u64) == Some(number)
                && value.get("disabled").and_then(Value::as_bool) == Some(false),
                "definition head and immutable body do not identify the same enabled version");
            ensure!(number == previous.as_ref().map(|record| record.value.get("version").and_then(Value::as_u64)
                .context("previous definition head version absent")).transpose()?.unwrap_or(0)
                .checked_add(1).context("definition version exhausted")?, "definition publication requires next version");
            ensure!(!records.contains_key(version_key), "immutable definition version already exists");
            let record = OrchestrationRecord { revision: 1, value: version_value,
                owner_operation_id: writer.operation.operation_id().to_owned() };
            records.insert(version_key.to_owned(), serde_json::to_value(&record)?);
            version_delta = Some(OrchestrationVersionDelta { key: version_key.to_owned(), revision: 1 });
            Some(record)
        } else {
            if definition.is_some() {
                let number = value.get("version").and_then(Value::as_u64).context("definition head version absent")?;
                ensure!(previous.as_ref().and_then(|record| record.value.get("version")).and_then(Value::as_u64) == Some(number),
                    "definition creation or version change requires atomic publication");
            }
            None
        };
        let owner_operation_id = previous.as_ref().map(|record| record.owner_operation_id.clone())
            .unwrap_or_else(|| writer.operation.operation_id().to_owned());
        let record = OrchestrationRecord { revision, value, owner_operation_id };
        records.insert(key.to_owned(), serde_json::to_value(&record)?);
        let mutation = CoreMutation {
            plugin_id: domain.to_owned(), object_path: path, operation: CoreOperation::ApplyPatch,
            member_name: method,
            value: json!({"record_key":key,"expected_revision":expected_revision,"revision":revision}),
            actor_id: actor_id.clone(), capability_id,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        };
        let state = owner.commit_with_updates(&self.inner.catalog, scope.clone(), BackendCommit {
            mutation, scope: scope.clone(), schema_hash,
            state: BackendStateUpdate::Fields(json!({"records": records})), result: None,
        }, Vec::new(), None, Some(OrchestrationDelta {
            domain: domain.to_owned(), key: key.to_owned(), revision,
            operation_id: writer.operation.operation_id().to_owned(), version: version_delta,
        }))?;
        let proof = MethodEventProof { event_id: state.event.event_id, event_hash: state.event.event_hash.clone() };
        self.notify_committed(&state);
        Ok((OrchestrationCommit { record, proof }, version_record))
    }
}
