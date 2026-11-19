//! Redacted execution checkpoints share the existing append owner. The core
//! validates original scoped method proofs before accepting a materialization.
//!
//! Control is a new canonical action: older binaries cannot read a ledger once
//! one is appended. Rollback requires a Control-aware reader, never deleting or
//! rewriting the authoritative history to make an older binary accept it.
//! Counters cover durable final admitted executions and durable canonical-method
//! denials/cancellations. Unknown ingress methods and unavailable append owners
//! are not fabricated into that aggregate. A dropped waiter is not cancellation.
//! Terminal histories live in the original ledger; bounded active/unresolved
//! memory and disposable same-store locators are derived only by append/replay.
use op_execution_tracker::{ExecutionRecord, ExecutionStats};
use super::*;

use op_cozo_store::{DataValue, NamedRows, Num, ScriptMutability};

fn scope_parameters(scope: &StateScope) -> Result<BTreeMap<String, DataValue>> {
    Ok(BTreeMap::from([("scope".into(), DataValue::Str(scope_key(scope)?.into()))]))
}

fn locator_id(row: &[DataValue], column: usize) -> Result<u64> {
    match row.get(column) {
        Some(DataValue::Num(Num::Int(value))) if *value > 0 => Ok(u64::try_from(*value)?),
        _ => anyhow::bail!("checkpoint locator event identity invalid"),
    }
}

fn locator_text(row: &[DataValue], column: usize) -> Result<&str> {
    match row.get(column) {
        Some(DataValue::Str(value)) => Ok(value.as_str()),
        _ => anyhow::bail!("checkpoint locator encoding invalid"),
    }
}

fn latest_control(owner: &PersistentCore, scope: &StateScope, operation_id: &str) -> Result<Option<ExecutionRecord>> {
    if let Some(record) = owner.active_controls.get(&(scope.clone(), operation_id.to_owned())) {
        return Ok(Some(record.clone()));
    }
    let mut parameters = scope_parameters(scope)?;
    parameters.insert("operation".into(), DataValue::Str(operation_id.into()));
    let rows = owner.control_query(
        "?[event_id,event_hash] := *bridge_control_latest[scope,operation_id,event_id,event_hash], \
         scope = $scope, operation_id = $operation", parameters, false)?;
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] if row.len() == 2 => owner.control_record(scope, operation_id,
            locator_id(row, 0)?, locator_text(row, 1)?).map(Some),
        _ => anyhow::bail!("checkpoint latest locator is not unique"),
    }
}

impl PersistentCore {
    pub(in crate::core) fn control_query(&self, script: &str, parameters: BTreeMap<String, DataValue>, write: bool) -> Result<NamedRows> {
        self.db.db().run_script(script, parameters,
            if write { ScriptMutability::Mutable } else { ScriptMutability::Immutable })
            .map_err(|_| anyhow::anyhow!("checkpoint locator query unavailable"))
    }

    /// Locators are disposable indexes, never another journal or record body.
    /// Core open clears them before validated canonical replay rebuilds them.
    pub(super) fn reset_control_index(&self) -> Result<()> {
        self.control_query(
            "{ ?[scope,operation_id,event_id,event_hash] <- [] \
               :replace bridge_control_latest {scope: String,operation_id: String => event_id: Int,event_hash: String} } \
             { ?[scope,event_id,operation_id,event_hash] <- [] \
               :replace bridge_control_events {scope: String,event_id: Int => operation_id: String,event_hash: String} }",
            BTreeMap::new(), true)?;
        Ok(())
    }

    pub(super) fn index_execution_control(&mut self, entry: &LedgerRecord) -> Result<()> {
        let LedgerAction::Control { record, .. } = &entry.action else { return Ok(()); };
        let mut parameters = scope_parameters(&entry.owner)?;
        parameters.insert("operation".into(), DataValue::Str(record.context.operation_id.as_str().into()));
        parameters.insert("event".into(), DataValue::Num(Num::Int(i64::try_from(entry.event.event_id)?)));
        parameters.insert("hash".into(), DataValue::Str(entry.event.event_hash.as_str().into()));
        self.control_query(
            "{ ?[scope,operation_id,event_id,event_hash] <- [[$scope,$operation,$event,$hash]] \
               :put bridge_control_latest {scope,operation_id => event_id,event_hash} } \
             { ?[scope,event_id,operation_id,event_hash] <- [[$scope,$event,$operation,$hash]] \
               :put bridge_control_events {scope,event_id => operation_id,event_hash} }",
            parameters, true)?;
        let key = (entry.owner.clone(), record.context.operation_id.clone());
        if record.status.is_terminal() {
            self.active_controls.remove(&key);
        } else {
            ensure!(self.active_controls.contains_key(&key)
                || self.active_controls.len() < BridgeCore::MAX_ACTIVE_EXECUTION_RECORDS,
                "active or unresolved checkpoint capacity exhausted; explicit reconciliation required");
            let mut cached = record.clone();
            cached.cursor = Some(entry.event.event_id);
            self.active_controls.insert(key, cached);
        }
        Ok(())
    }

    pub(in crate::core) fn canonical_indexed_record(&self, scope: &StateScope, event_id: u64,
        event_hash: &str) -> Result<LedgerRecord> {
        ensure!(event_id > 0 && event_id <= self.at_event_id(), "checkpoint locator exceeds canonical tip");
        let sequence = i64::try_from(event_id.checked_sub(1).context("checkpoint event identity invalid")?)?;
        let rows = self.control_query(
            "?[kind,content] := *session_events[session_id,seq,kind,subid,content,created_at], \
             session_id = $session, seq = $sequence",
            BTreeMap::from([
                ("session".into(), DataValue::Str(LEDGER.into())),
                ("sequence".into(), DataValue::Num(Num::Int(sequence))),
            ]), false)?;
        let [row] = rows.rows.as_slice() else { anyhow::bail!("checkpoint canonical row missing or duplicated"); };
        ensure!(row.len() == 2 && locator_text(row, 0)? == RECORD_KIND, "checkpoint canonical row kind differs");
        let content = locator_text(row, 1)?;
        ensure!(content.len() <= MAX_RECORD_BYTES, "checkpoint canonical row exceeds byte bound");
        let entry: LedgerRecord = serde_json::from_str(content).context("decode canonical checkpoint")?;
        ensure!(entry.version == 1 && entry.owner == *scope && entry.event.event_id == event_id
            && entry.event.event_hash == event_hash && entry.event.verify(), "checkpoint canonical locator binding differs");
        let payload = entry.event.input_payload.as_ref().context("checkpoint canonical binding missing")?;
        ensure!(serde_json::to_value(payload)? == serde_json::json!({"record_digest": binding(&entry.owner, &entry.action)?}),
            "checkpoint canonical payload binding differs");
        Ok(entry)
    }

    fn control_record(&self, scope: &StateScope, operation_id: &str, event_id: u64,
        event_hash: &str) -> Result<ExecutionRecord> {
        let entry = self.canonical_indexed_record(scope, event_id, event_hash)?;
        let LedgerAction::Control { mut record, .. } = entry.action else { anyhow::bail!("checkpoint locator points to another action"); };
        ensure!(record.context.operation_id == operation_id, "checkpoint locator operation differs");
        record.cursor = Some(event_id);
        Ok(record)
    }

    pub(in crate::core) fn current_definition_control(&self, scope: &StateScope, operation_id: &str)
        -> Result<Option<ExecutionRecord>> {
        latest_control(self, scope, operation_id)
    }


    /// Immutable decisions and their contexts are checked before append, and
    /// again against the immediately preceding canonical event during replay.
    pub(crate) fn validate_control_predecessor(&self, scope: &StateScope,
        record: &ExecutionRecord, allow_idempotent: bool) -> Result<Option<ExecutionRecord>> {
        if let Some(previous) = latest_control(self, scope, &record.context.operation_id)? {
            ensure!(previous.context == record.context, "checkpoint attribution is immutable");
            if allow_idempotent && previous.status == record.status && previous.proof == record.proof {
                return Ok(Some(previous));
            }
            if previous.status.is_terminal() {
                anyhow::bail!("terminal checkpoint decision is immutable");
            }
        }
        Ok(None)
    }
}

/// Constructed only at the preappend reservation check, never by metadata
/// reconciliation after a possibly durable append.
#[derive(Debug, thiserror::Error)]
#[error("execution checkpoint capacity refused before admission")]
struct ControlAdmissionCapacityRefusal;

impl BridgeCore {
    pub(crate) const MAX_ACTIVE_EXECUTION_RECORDS: usize = 1024;

    pub(crate) fn is_control_admission_capacity_refusal(error: &anyhow::Error) -> bool {
        error.is::<ControlAdmissionCapacityRefusal>()
    }

    pub(crate) fn record_execution_control(
        &self, mut record: ExecutionRecord, plugin: &str, method: &str,
        schema_hash: &str, capability_id: &str,
    ) -> Result<ExecutionRecord> {
        record.context.validate()?;
        ensure!(record.context.tool_name == format!("{plugin}.{method}"), "checkpoint method attribution differs");
        let scope = StateScope::Private {
            principal_id: record.context.executor_principal_id.clone().context("checkpoint executor missing")?,
            session_id: record.context.executor_session_id.clone().context("checkpoint session missing")?,
        };
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let schema = owner.pin(&self.inner.catalog, plugin, schema_hash)?;
        ensure!(schema.methods.get(method).is_some_and(|declaration|
            declaration.required_capability.as_deref() == Some(capability_id)), "checkpoint sealed method capability differs");
        owner.validate_execution_control(&scope, &record)?;
        if let Some(previous) = owner.validate_control_predecessor(&scope, &record, true)? {
            return Ok(previous);
        }
        if !record.status.is_terminal()
            && !owner.active_controls.contains_key(&(scope.clone(), record.context.operation_id.clone()))
            && owner.active_controls.len() >= Self::MAX_ACTIVE_EXECUTION_RECORDS {
            return Err(ControlAdmissionCapacityRefusal.into());
        }
        let final_transition = record.status.is_terminal();
        record.cursor = None;
        let mutation = CoreMutation {
            plugin_id: plugin.to_owned(), object_path: op_plugins::canonical::plugin_path(plugin),
            operation: CoreOperation::CallMethod, member_name: method.to_owned(),
            value: Value::Null,
            actor_id: record.context.executor_principal_id.clone().expect("validated executor"),
            capability_id: capability_id.to_owned(), idempotency_key: record.context.operation_id.clone(),
        };
        let event = owner.append(&self.inner.catalog, scope.clone(), &mutation, schema_hash,
            LedgerAction::Control { record: record.clone(), final_transition })?;
        record.cursor = Some(event.event_id);
        let _ = self.notices().send(StateNotice { scope, plugin_id: plugin.to_owned(),
            event_id: event.event_id, state_committed: false });
        Ok(record)
    }

    /// Exclusive durable event cursor; every returned entry has already passed
    /// full ledger linkage, private scope and original-operation proof checks.
    pub(crate) fn execution_control_page(&self, scope: &StateScope, after: u64, limit: usize)
        -> Result<Vec<ExecutionRecord>> {
        validate_scope(scope)?;
        ensure!(matches!(scope, StateScope::Private { .. }), "execution recovery requires private scope");
        ensure!(limit > 0 && limit <= 256, "checkpoint page limit must be 1..=256");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        if after >= owner.at_event_id() { return Ok(Vec::new()); }
        let mut parameters = scope_parameters(scope)?;
        parameters.insert("after".into(), DataValue::Num(Num::Int(i64::try_from(after)?)));
        let rows = owner.control_query(&format!(
            "?[event_id,operation_id,event_hash] := *bridge_control_events[scope,event_id,operation_id,event_hash], \
             scope = $scope, event_id > $after :order event_id :limit {limit}"), parameters, false)?;
        ensure!(rows.rows.len() <= limit, "checkpoint locator page exceeds bound");
        let mut page = Vec::with_capacity(rows.rows.len());
        let mut cursor = after;
        for row in &rows.rows {
            ensure!(row.len() == 3, "checkpoint locator page encoding invalid");
            let event_id = locator_id(row, 0)?;
            ensure!(event_id > cursor, "checkpoint locator page is not exclusive and ordered");
            page.push(owner.control_record(scope, locator_text(row, 1)?, event_id, locator_text(row, 2)?)?);
            cursor = event_id;
        }
        Ok(page)
    }

    pub(crate) fn execution_control_latest(&self, scope: &StateScope, operation_id: &str)
        -> Result<Option<ExecutionRecord>> {
        validate_scope(scope)?;
        ensure!(matches!(scope, StateScope::Private { .. }), "execution recovery requires private scope");
        ensure!(uuid::Uuid::parse_str(operation_id)?.to_string() == operation_id, "noncanonical operation id");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        latest_control(&owner, scope, operation_id)
    }

    /// Cumulative counters are folded by the ordered core owner/replay, never
    /// incremented by transport completion or out-of-order tracker cache pages.
    pub(crate) fn execution_control_stats(&self, scope: &StateScope) -> Result<(u64, ExecutionStats)> {
        validate_scope(scope)?;
        ensure!(matches!(scope, StateScope::Private { .. }), "execution counters require private scope");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        Ok((owner.at_event_id(), owner.control_stats.get(scope).cloned().unwrap_or_default()))
    }
}
