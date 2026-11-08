//! Native method accountability in the same append ledger as committed state.
//! An attempt is durable before dispatch; missing terminals remain unknown.
use anyhow::{Context, Result, ensure};
use op_state_store::method_receipt::{
    MAX_METHOD_ARGUMENT_BYTES, MAX_METHOD_RECEIPT_BYTES, MethodEventProof, MethodExecutionReceipt,
    MethodReceiptRecovery, MethodRecoveryStatus, UnsupportedBeforeEffect, validate_json_size,
    validate_method_result,
};

use super::persistence::{LedgerAction, LedgerRecord, PersistentCore};
use super::projection::validate_scope;
use super::*;

#[derive(Clone)]
pub(super) struct OperationState {
    plugin_id: String,
    method: String,
    object_path: String,
    capability_id: String,
    schema_hash: String,
    arguments_digest: String,
    attempt: MethodEventProof,
    terminal: Option<MethodEventProof>,
    status: MethodRecoveryStatus,
}

/// Opaque admitted attempt. Constructed only after its Cozo durability barrier.
/// Dropping this handle does not mark a backend operation failed or replayable.
pub struct MethodOperation {
    operation_id: String,
    owner: StateScope,
    state: OperationState,
}

impl MethodOperation {
    pub fn operation_id(&self) -> &str { &self.operation_id }
    pub fn attempt(&self) -> &MethodEventProof { &self.state.attempt }
}

fn operation_id(value: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(value)?;
    ensure!(id.to_string() == value, "operation id must be a canonical UUID");
    Ok(())
}

fn owner_scope(principal_id: &str, session_id: &str) -> Result<StateScope> {
    let owner = StateScope::Private { principal_id: principal_id.into(), session_id: session_id.into() };
    validate_scope(&owner)?;
    Ok(owner)
}

fn argument_digest(value: &serde_json::Value) -> Result<String> {
    validate_json_size(value, MAX_METHOD_ARGUMENT_BYTES)?;
    Ok(blake3::hash(&serde_json::to_vec(value)?).to_hex().to_string())
}

fn recovery(id: &str, state: Option<&OperationState>) -> MethodReceiptRecovery {
    MethodReceiptRecovery {
        operation_id: id.into(),
        status: state.map(|state| state.status.clone()).unwrap_or(MethodRecoveryStatus::NotFound),
        plugin_id: state.map(|state| state.plugin_id.clone()),
        method: state.map(|state| state.method.clone()),
        attempt: state.map(|state| state.attempt.clone()),
        terminal: state.and_then(|state| state.terminal.clone()),
        // No target-owned safe result-redaction contract exists here. Domain
        // results and even low-entropy result digests stay out of recovery.
        result_digest: None,
    }
}

impl PersistentCore {
    pub(super) fn accept_operation(&mut self, record: &LedgerRecord) -> Result<()> {
        match &record.action {
            LedgerAction::Attempt { operation_id: id, schema_hash, arguments_digest } => {
                operation_id(id)?;
                ensure!(matches!(record.owner, StateScope::Private { .. }), "method attempt lacks caller scope");
                let method = record.event.method_name.as_ref().context("method attempt name missing")?;
                let object_path = record.event.target.strip_suffix(&format!(":{method}"))
                    .context("method attempt target invalid")?;
                let key = (record.owner.clone(), id.clone());
                ensure!(!self.operations.contains_key(&key), "duplicate method operation attempt");
                self.operations.insert(key, OperationState {
                    plugin_id: record.event.plugin_id.clone(), method: method.clone(),
                    object_path: object_path.into(), capability_id: record.event.capability_id.clone().context("method attempt capability missing")?,
                    schema_hash: schema_hash.clone(), arguments_digest: arguments_digest.clone(),
                    attempt: MethodEventProof { event_id: record.event.event_id, event_hash: record.event.event_hash.clone() },
                    terminal: None, status: MethodRecoveryStatus::Unknown,
                });
            }
            LedgerAction::Terminal { operation_id: id, status, publications, .. } => {
                let state = self.operations.get_mut(&(record.owner.clone(), id.clone()))
                    .context("terminal event has no scoped attempt")?;
                ensure!(state.terminal.is_none() && record.event.event_id > state.attempt.event_id
                    && record.event.plugin_id == state.plugin_id && record.event.method_name.as_ref() == Some(&state.method)
                    && *status != MethodRecoveryStatus::NotFound,
                    "method terminal attribution or order invalid");
                for publication in publications {
                    ensure!(publication.plugin_id != state.plugin_id || publication.schema_hash == state.schema_hash,
                        "method terminal state attribution invalid");
                    if matches!(publication.scope, StateScope::Private { .. }) {
                        ensure!(publication.scope == record.owner, "method terminal private state scope mismatch");
                    }
                }
                state.terminal = Some(MethodEventProof { event_id: record.event.event_id, event_hash: record.event.event_hash.clone() });
                state.status = status.clone();
            }
            LedgerAction::State { .. } => {}
        }
        Ok(())
    }

    fn require_operation(&self, operation: &MethodOperation) -> Result<&OperationState> {
        self.require_available()?;
        let state = self.operations.get(&(operation.owner.clone(), operation.operation_id.clone()))
            .context("admitted method operation absent")?;
        ensure!(state.attempt == operation.state.attempt && state.terminal.is_none(),
            "method operation already has a terminal outcome; do not dispatch again");
        Ok(state)
    }
}

impl BridgeCore {
    /// Persist the scoped attempt before any backend execution. Duplicate IDs
    /// never execute again, even after restart or a transport cancellation.
    pub fn begin_operation(
        &self,
        operation_id: &str,
        principal_id: &str,
        session_id: &str,
        mutation: &CoreMutation,
        schema_hash: &str,
    ) -> Result<MethodOperation> {
        self.begin_operation_inner(operation_id, principal_id, session_id, mutation, schema_hash)
            .map_err(|_| anyhow::anyhow!("method_attempt_unavailable; operation_id={operation_id}; recover before retry"))
    }

    fn begin_operation_inner(
        &self,
        id: &str,
        principal_id: &str,
        session_id: &str,
        mutation: &CoreMutation,
        schema_hash: &str,
    ) -> Result<MethodOperation> {
        operation_id(id)?;
        let scope = owner_scope(principal_id, session_id)?;
        ensure!(mutation.operation == CoreOperation::CallMethod && mutation.actor_id == principal_id,
            "method attempt requires receiver-authenticated principal");
        let digest = argument_digest(&mutation.value)?;
        super::projection::path_segments(&mutation.plugin_id, &mutation.object_path)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let schema = owner.pin(&self.inner.catalog, &mutation.plugin_id, schema_hash)?;
        let method = schema.methods.get(&mutation.member_name).context("method absent from sealed declaration")?;
        ensure!(method.required_capability.as_deref().is_some_and(|capability| capability == mutation.capability_id),
            "method attempt capability differs from sealed declaration");
        ensure!(!owner.operations.contains_key(&(scope.clone(), id.to_owned())),
            "operation already reserved; recover the original attempt, do not repeat execution");
        let event = owner.append(&self.inner.catalog, scope.clone(), mutation, schema_hash, LedgerAction::Attempt {
            operation_id: id.into(), schema_hash: schema_hash.into(), arguments_digest: digest,
        })?;
        let state = owner.operations.get(&(scope.clone(), id.to_owned())).context("durable attempt metadata absent")?.clone();
        let _ = self.notices().send(StateNotice { scope: scope.clone(), plugin_id: event.plugin_id, event_id: event.event_id, state_committed: false });
        Ok(MethodOperation { operation_id: id.into(), owner: scope, state })
    }

    /// Commit the terminal proof and authoritative state in ONE ledger append,
    /// then publish SHM, then wake subscribers and return the native receipt.
    pub fn complete_operation(&self, operation: &MethodOperation, commit: BackendCommit) -> Result<MethodExecutionReceipt> {
        self.complete_operation_with_updates(operation, commit, Vec::new())
    }

    /// Native multi-plugin observations share one terminal proof. Each
    /// namespace commits atomically; all namespaces publish before any wake.
    pub fn complete_operation_with_updates(
        &self,
        operation: &MethodOperation,
        mut commit: BackendCommit,
        updates: Vec<BackendProjection>,
    ) -> Result<MethodExecutionReceipt> {
        let result = commit.result.take();
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let admitted = owner.require_operation(operation)?;
        ensure!(commit.mutation.plugin_id == admitted.plugin_id && commit.mutation.member_name == admitted.method
            && commit.mutation.object_path == admitted.object_path && commit.mutation.capability_id == admitted.capability_id
            && commit.schema_hash == admitted.schema_hash && argument_digest(&commit.mutation.value)? == admitted.arguments_digest,
            "completed method differs from admitted attempt");
        let StateScope::Private { principal_id, .. } = &operation.owner else { unreachable!("admitted owner") };
        ensure!(commit.mutation.actor_id == *principal_id, "completed method actor mismatch");
        if matches!(commit.scope, StateScope::Private { .. }) {
            ensure!(commit.scope == operation.owner, "completed private state scope mismatch");
        }
        let schema = owner.pin(&self.inner.catalog, &admitted.plugin_id, &admitted.schema_hash)?;
        let method = schema.methods.get(&admitted.method).context("admitted method absent")?;
        let returns = method.returns.as_ref().map(serde_json::to_value).transpose()?;
        let result = result.context("completed method has no backend result").and_then(|result| {
            validate_json_size(&result, MAX_METHOD_RECEIPT_BYTES)?;
            validate_method_result(returns.as_ref(), &result)?;
            Ok(result)
        });
        let (status, code) = if result.is_ok() {
            (MethodRecoveryStatus::Completed, "completed")
        } else {
            (MethodRecoveryStatus::Unknown, "invalid_backend_result")
        };
        let state = owner.commit_with_updates(&self.inner.catalog, operation.owner.clone(), commit, updates,
            Some((operation.operation_id.clone(), status, code.into())))?;
        let receipt = result.and_then(|result| {
            let receipt = MethodExecutionReceipt {
                success: true, operation_id: operation.operation_id.clone(), outcome: "completed".into(),
                event_id: operation.state.attempt.event_id, event_hash: operation.state.attempt.event_hash.clone(),
                terminal_event_id: state.event.event_id, terminal_event_hash: state.event.event_hash.clone(),
                plugin_id: operation.state.plugin_id.clone(), method: operation.state.method.clone(), result,
            };
            receipt.validate_attribution(&operation.state.plugin_id, &operation.state.method)?;
            Ok(receipt)
        });
        self.notify_committed(&state);
        let _ = self.notices().send(StateNotice { scope: operation.owner.clone(),
            plugin_id: state.event.plugin_id, event_id: state.event.event_id, state_committed: false });
        receipt.map_err(|error| error.context("backend reply rejected; observed state retained with unknown outcome"))
    }

    /// Transport/backend failure does not prove absence of effects. This closes
    /// only the audit outcome; it does not retry, roll back or invent state.
    pub fn outcome_unknown(&self, operation: &MethodOperation) -> Result<MethodReceiptRecovery> {
        self.outcome_unknown_with_updates(operation, Vec::new())
    }

    pub fn fail_operation_before_effect(&self, operation: &MethodOperation, refusal: UnsupportedBeforeEffect) -> Result<MethodReceiptRecovery> {
        self.fail_operation_before_effect_with_updates(operation, refusal, Vec::new())
    }

    /// Preserve observed intermediate state after a backend/provider failure
    /// without claiming a successful domain response or an effect-free refusal.
    pub fn outcome_unknown_with_updates(
        &self, operation: &MethodOperation, updates: Vec<BackendProjection>,
    ) -> Result<MethodReceiptRecovery> {
        self.record_terminal(operation, MethodRecoveryStatus::Unknown, "backend_or_publication_unknown", updates)
    }

    pub fn fail_operation_before_effect_with_updates(
        &self, operation: &MethodOperation, refusal: UnsupportedBeforeEffect, updates: Vec<BackendProjection>,
    ) -> Result<MethodReceiptRecovery> {
        self.record_terminal(operation, MethodRecoveryStatus::Failed, refusal.code(), updates)
    }

    fn record_terminal(&self, operation: &MethodOperation, status: MethodRecoveryStatus, code: &str, updates: Vec<BackendProjection>) -> Result<MethodReceiptRecovery> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_operation(operation)?;
        let StateScope::Private { principal_id, .. } = &operation.owner else { unreachable!("admitted owner") };
        let mutation = CoreMutation {
            plugin_id: operation.state.plugin_id.clone(), object_path: operation.state.object_path.clone(),
            operation: CoreOperation::CallMethod, member_name: operation.state.method.clone(),
            value: serde_json::Value::Null, actor_id: principal_id.clone(), capability_id: operation.state.capability_id.clone(),
            idempotency_key: operation.operation_id.clone(),
        };
        let commit = BackendCommit { mutation, scope: operation.owner.clone(), schema_hash: operation.state.schema_hash.clone(),
            state: BackendStateUpdate::Unchanged, result: None };
        let state = owner.commit_with_updates(&self.inner.catalog, operation.owner.clone(), commit, updates,
            Some((operation.operation_id.clone(), status, code.into())))?;
        self.notify_committed(&state);
        let result = recovery(&operation.operation_id, owner.operations.get(&(operation.owner.clone(), operation.operation_id.clone())));
        let _ = self.notices().send(StateNotice { scope: operation.owner.clone(), plugin_id: state.event.plugin_id, event_id: state.event.event_id, state_committed: false });
        Ok(result)
    }

    /// Same-principal AND same-session recovery. Missing and another caller's
    /// operation are indistinguishable. No private domain result is returned.
    pub fn recover_operation(&self, operation_id: &str, principal_id: &str, session_id: &str) -> Result<MethodReceiptRecovery> {
        self.recover_operation_inner(operation_id, principal_id, session_id)
    }

    fn recover_operation_inner(&self, id: &str, principal_id: &str, session_id: &str) -> Result<MethodReceiptRecovery> {
        operation_id(id)?;
        let scope = owner_scope(principal_id, session_id)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        Ok(recovery(id, owner.operations.get(&(scope, id.to_owned()))))
    }
}
