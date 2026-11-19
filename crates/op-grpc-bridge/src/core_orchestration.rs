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
        validate_address(&scope, domain, key)?;
        let schema = owner.pin(&self.inner.catalog, domain, &schema_hash)?;
        let declaration = schema.methods.get(&method).context("orchestration mutator absent from sealed owner")?;
        ensure!(declaration.side_effect == op_state_store::SideEffect::Mutation
            && declaration.required_capability.as_deref() == Some(capability.as_str()),
            "only an actual admitted mutation can own orchestration records");
        Ok(OrchestrationWriter { operation: operation.clone(), domain: domain.to_owned(), key: key.to_owned() })
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
        validate_json_size(&value, MAX_VALUE_BYTES)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let domain = writer.domain.as_str();
        let key = writer.key.as_str();
        let (scope, schema_hash, method, capability_id) = owner.orchestration_attribution(&writer.operation, domain)?;
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
        }, Vec::new(), None)?;
        let proof = MethodEventProof { event_id: state.event.event_id, event_hash: state.event.event_hash.clone() };
        self.notify_committed(&state);
        Ok(OrchestrationCommit { record, proof })
    }
}
