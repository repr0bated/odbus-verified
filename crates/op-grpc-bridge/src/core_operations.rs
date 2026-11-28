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
    attribution: Option<op_execution_tracker::DelegationAttribution>,
    request_digest: Option<String>,
    result_digest: Option<String>,
    attempt: MethodEventProof,
    terminal: Option<MethodEventProof>,
    status: MethodRecoveryStatus,
    /// Counted only by the ordered core ledger/replay, never cache delivery.
    control_terminal: Option<op_execution_tracker::ExecutionStatus>,
    definition_version: Option<(String, u64, MethodEventProof)>,
}

/// Opaque admitted attempt. Constructed only after its Cozo durability barrier.
/// Dropping this handle does not mark a backend operation failed or replayable.
#[derive(Clone)]
pub struct MethodOperation {
    operation_id: String,
    owner: StateScope,
    state: OperationState,
}

impl MethodOperation {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn attempt(&self) -> &MethodEventProof {
        &self.state.attempt
    }
}

fn operation_id(value: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(value)?;
    ensure!(
        id.to_string() == value,
        "operation id must be a canonical UUID"
    );
    Ok(())
}

fn owner_scope(principal_id: &str, session_id: &str) -> Result<StateScope> {
    let owner = StateScope::Private {
        principal_id: principal_id.into(),
        session_id: session_id.into(),
    };
    validate_scope(&owner)?;
    Ok(owner)
}

fn argument_digest(value: &serde_json::Value) -> Result<String> {
    validate_json_size(value, MAX_METHOD_ARGUMENT_BYTES)?;
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}

fn recovery(id: &str, state: Option<&OperationState>) -> MethodReceiptRecovery {
    MethodReceiptRecovery {
        operation_id: id.into(),
        status: state
            .map(|state| state.status.clone())
            .unwrap_or(MethodRecoveryStatus::NotFound),
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
            LedgerAction::Attempt {
                operation_id: id,
                schema_hash,
                arguments_digest,
                attribution,
                request_digest,
            } => {
                operation_id(id)?;
                ensure!(
                    matches!(record.owner, StateScope::Private { .. }),
                    "method attempt lacks caller scope"
                );
                let method = record
                    .event
                    .method_name
                    .as_ref()
                    .context("method attempt name missing")?;
                let object_path = record
                    .event
                    .target
                    .strip_suffix(&format!(":{method}"))
                    .context("method attempt target invalid")?;
                let key = (record.owner.clone(), id.clone());
                ensure!(
                    !self.operations.contains_key(&key),
                    "duplicate method operation attempt"
                );
                self.operations.insert(
                    key,
                    OperationState {
                        plugin_id: record.event.plugin_id.clone(),
                        method: method.clone(),
                        object_path: object_path.into(),
                        capability_id: record
                            .event
                            .capability_id
                            .clone()
                            .context("method attempt capability missing")?,
                        schema_hash: schema_hash.clone(),
                        arguments_digest: arguments_digest.clone(),
                        attribution: attribution.clone(),
                        request_digest: request_digest.clone(),
                        result_digest: None,
                        attempt: MethodEventProof {
                            event_id: record.event.event_id,
                            event_hash: record.event.event_hash.clone(),
                        },
                        terminal: None,
                        status: MethodRecoveryStatus::Unknown,
                        control_terminal: None,
                        definition_version: None,
                    },
                );
            }
            LedgerAction::Terminal {
                operation_id: id,
                status,
                publications,
                result_digest,
                ..
            } => {
                let state = self
                    .operations
                    .get_mut(&(record.owner.clone(), id.clone()))
                    .context("terminal event has no scoped attempt")?;
                ensure!(
                    state.terminal.is_none()
                        && record.event.event_id > state.attempt.event_id
                        && record.event.plugin_id == state.plugin_id
                        && record.event.method_name.as_ref() == Some(&state.method)
                        && *status != MethodRecoveryStatus::NotFound,
                    "method terminal attribution or order invalid"
                );
                for publication in publications {
                    ensure!(
                        publication.plugin_id != state.plugin_id
                            || publication.schema_hash == state.schema_hash,
                        "method terminal state attribution invalid"
                    );
                    if matches!(publication.scope, StateScope::Private { .. }) {
                        ensure!(
                            publication.scope == record.owner,
                            "method terminal private state scope mismatch"
                        );
                    }
                }
                state.terminal = Some(MethodEventProof {
                    event_id: record.event.event_id,
                    event_hash: record.event.event_hash.clone(),
                });
                state.status = status.clone();
                state.result_digest = result_digest.clone();
            }
            LedgerAction::State { orchestration, .. } => {
                if let Some((delta, version)) = orchestration.as_ref().filter(|delta| delta.domain == "workstacks")
                    .and_then(|delta| delta.version.as_ref().map(|version| (delta, version))) {
                    let kind = if delta.key.starts_with("role:") { Some("role") }
                        else if delta.key.starts_with("policy:") { Some("policy") } else { None };
                    if let Some(kind) = kind {
                        let publication = record.action.publications().iter().find(|publication|
                            publication.scope == record.owner && publication.plugin_id == "workstacks")
                            .context("immutable definition has no original publication")?;
                        self.validate_definition_delta(&record.owner, &delta.operation_id, kind, &record.event, &publication.schema_hash)?;
                        let state = self.operations.get_mut(&(record.owner.clone(), delta.operation_id.clone()))
                            .context("immutable definition has no management attempt")?;
                        let method = if kind == "role" { "put_execution_role" } else { "put_utility_policy" };
                        ensure!(state.method == method && version.revision == 1 && state.definition_version.is_none(),
                            "immutable definition must be created once by its actual put operation");
                        state.definition_version = Some((version.key.clone(), version.revision,
                            MethodEventProof { event_id: record.event.event_id, event_hash: record.event.event_hash.clone() }));
                    }
                }
            }
            LedgerAction::Control { record: control, final_transition } => {
                self.validate_execution_control(&record.owner, control)?;
                self.validate_control_predecessor(&record.owner, control, false)?;
                ensure!(*final_transition == control.status.is_terminal(), "invalid checkpoint terminal transition");
                if *final_transition {
                    let stats = self.control_stats.entry(record.owner.clone()).or_default();
                    if control.proof.is_some() {
                        let state = self.operations.get_mut(&(record.owner.clone(), control.context.operation_id.clone()))
                            .context("final checkpoint operation is absent")?;
                        ensure!(state.control_terminal.is_none(), "duplicate core terminal checkpoint");
                        state.control_terminal = Some(control.status);
                        stats.total_executions += 1;
                        stats.total_duration_ms += control.duration_ms;
                        *stats.executions_by_tool.entry(control.context.tool_name.clone()).or_default() += 1;
                        match control.status {
                            op_execution_tracker::ExecutionStatus::Completed => stats.successful_executions += 1,
                            status => {
                                stats.failed_executions += 1;
                                *stats.failures_by_tool.entry(control.context.tool_name.clone()).or_default() += 1;
                                if status == op_execution_tracker::ExecutionStatus::FailedBeforeEffect {
                                    stats.failed_before_effect_executions += 1;
                                }
                            }
                        }
                    } else {
                        match control.status {
                            op_execution_tracker::ExecutionStatus::Denied => stats.denied_requests += 1,
                            op_execution_tracker::ExecutionStatus::Cancelled => stats.cancelled_requests += 1,
                            _ => anyhow::bail!("final request decision has no original operation proof"),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Native projection ownership derived from an actual durable mutation
    /// attempt. A terminal transition may reconcile this original owner after
    /// grant revocation; it never admits another effect.
    pub(super) fn orchestration_attribution(
        &self, operation: &MethodOperation, domain: &str,
    ) -> Result<(StateScope, String, String, String)> {
        self.require_available()?;
        let state = self.operations.get(&(operation.owner.clone(), operation.operation_id.clone()))
            .context("orchestration owner has no durable operation")?;
        ensure!(state.attempt == operation.state.attempt && state.plugin_id == domain
            && state.schema_hash == operation.state.schema_hash
            && state.capability_id == operation.state.capability_id,
            "orchestration admission attribution differs from its durable attempt");
        Ok((operation.owner.clone(), state.schema_hash.clone(), state.method.clone(), state.capability_id.clone()))
    }

    pub(super) fn recover_orchestration_operation(
        &self, scope: &StateScope, id: &str, acceptance: &MethodEventProof,
    ) -> Result<MethodOperation> {
        operation_id(id)?;
        let state = self.operations.get(&(scope.clone(), id.to_owned()))
            .context("original orchestration operation is unavailable")?;
        ensure!(state.status == MethodRecoveryStatus::Completed
            && state.terminal.as_ref() == Some(acceptance),
            "orchestration writer recovery requires its original completed acceptance proof");
        Ok(MethodOperation { operation_id: id.to_owned(), owner: scope.clone(), state: state.clone() })
    }

    fn require_operation(&self, operation: &MethodOperation) -> Result<&OperationState> {
        self.require_available()?;
        let state = self
            .operations
            .get(&(operation.owner.clone(), operation.operation_id.clone()))
            .context("admitted method operation absent")?;
        ensure!(
            state.attempt == operation.state.attempt && state.terminal.is_none(),
            "method operation already has a terminal outcome; do not dispatch again"
        );
        Ok(state)
    }

    pub(super) fn definition_attribution(&self, operation: &MethodOperation, kind: &str, put: bool)
        -> Result<(StateScope, String, String, String)> {
        let state = self.require_operation(operation)?;
        let expected = match (kind, put) {
            ("role", true) => "put_execution_role",
            ("policy", true) => "put_utility_policy",
            ("role", false) => "disable_execution_role",
            ("policy", false) => "disable_utility_policy",
            _ => anyhow::bail!("unknown native owner definition mutation"),
        };
        ensure!(state.plugin_id == "workstacks" && state.method == expected,
            "definition requires its original exact admitted management method");
        Ok((operation.owner.clone(), state.schema_hash.clone(), state.method.clone(), state.capability_id.clone()))
    }

    pub(super) fn validate_definition_delta(&self, scope: &StateScope, id: &str, kind: &str,
        event: &op_state_store::ChainEvent, schema_hash: &str) -> Result<()> {
        let state = self.operations.get(&(scope.clone(), id.to_owned())).context("definition writer attempt absent")?;
        let methods = match kind {
            "role" => ["put_execution_role", "disable_execution_role"],
            "policy" => ["put_utility_policy", "disable_utility_policy"],
            _ => anyhow::bail!("unknown native owner definition kind"),
        };
        ensure!(state.plugin_id == "workstacks" && methods.contains(&state.method.as_str())
            && state.terminal.is_none() && state.schema_hash == schema_hash
            && event.event_id > state.attempt.event_id && event.plugin_id == state.plugin_id
            && event.method_name.as_deref() == Some(state.method.as_str())
            && event.capability_id.as_deref() == Some(state.capability_id.as_str()),
            "definition state delta differs from its original live management attempt");
        Ok(())
    }

    pub(super) fn definition_management_proof(&self, scope: &StateScope, id: &str, kind: &str, head: bool)
        -> Result<MethodReceiptRecovery> {
        operation_id(id)?;
        let state = self.operations.get(&(scope.clone(), id.to_owned())).context("original definition approval unavailable")?;
        let methods = match kind {
            "role" => ["put_execution_role", "disable_execution_role"],
            "policy" => ["put_utility_policy", "disable_utility_policy"],
            _ => anyhow::bail!("unknown owner definition kind"),
        };
        ensure!(state.plugin_id == "workstacks" && (state.method == methods[0] || head && state.method == methods[1]),
            "owner definition management approval remains unverified; needs review");
        Ok(recovery(id, Some(state)))
    }

    pub(super) fn require_live_definition_writer(&self, operation: &MethodOperation) -> Result<()> {
        self.require_operation(operation).map(|_| ())
    }

    pub(super) fn definition_version_source(&self, scope: &StateScope, id: &str, key: &str, revision: u64)
        -> Result<MethodEventProof> {
        let state = self.operations.get(&(scope.clone(), id.to_owned())).context("original immutable version creator unavailable")?;
        let (original_key, original_revision, proof) = state.definition_version.as_ref()
            .context("immutable version has no typed original source; needs review")?;
        ensure!(original_key == key && *original_revision == revision,
            "immutable version differs from original publication");
        Ok(proof.clone())
    }

    pub(super) fn definition_management_reader(&self, operation: &MethodOperation, owner: &str, kind: &str) -> Result<()> {
        let state = self.require_operation(operation)?;
        let methods = match kind {
            "role" => ["get_execution_role", "list_execution_roles", "put_execution_role", "disable_execution_role"],
            "policy" => ["get_utility_policy", "list_utility_policies", "put_utility_policy", "disable_utility_policy"],
            _ => anyhow::bail!("unknown management definition kind"),
        };
        ensure!(matches!(&operation.owner, StateScope::Private { principal_id, .. } if principal_id == owner)
            && state.plugin_id == "workstacks" && methods.contains(&state.method.as_str()),
            "management read requires actual canonical-owner actor and exact native management method");
        Ok(())
    }



    pub(super) fn validate_execution_control(
        &self, scope: &StateScope, record: &op_execution_tracker::ExecutionRecord,
    ) -> Result<()> {
        use op_execution_tracker::ExecutionStatus as Status;
        record.context.validate()?;
        let StateScope::Private { principal_id, session_id } = scope else {
            anyhow::bail!("execution control requires private scope");
        };
        ensure!(record.context.executor_principal_id.as_deref() == Some(principal_id.as_str())
            && record.context.executor_session_id.as_deref() == Some(session_id.as_str()),
            "execution control scope differs from verified executor");
        let state = self.operations.get(&(scope.clone(), record.context.operation_id.clone()));
        match (&record.proof, state) {
            (Some(proof), Some(state)) => {
                proof.validate(&record.context.operation_id)?;
                ensure!(state.control_terminal.is_none_or(|status| status == record.status),
                    "final execution checkpoint is immutable");
                ensure!(proof.attempt == state.attempt
                    && proof.terminal == state.terminal
                    && record.context.tool_name == format!("{}.{}", state.plugin_id, state.method)
                    && record.context.attribution == state.attribution,
                    "execution control differs from original operation proof");
                match record.status {
                    Status::ReceiptCompleted | Status::Completed | Status::DomainFailed => ensure!(state.status == MethodRecoveryStatus::Completed,
                        "execution control claims completion without a completed core operation"),
                    Status::FailedBeforeEffect => ensure!(state.status == MethodRecoveryStatus::Failed,
                        "execution control claims no effect without native refusal proof"),
                    Status::Denied | Status::Cancelled | Status::Proposed | Status::Checking | Status::ReadyForAdmission =>
                        anyhow::bail!("admitted operation cannot become an unattempted request"),
                    Status::Admitted | Status::Running => ensure!(state.terminal.is_none(),
                        "running execution already has a terminal method receipt"),
                    Status::CancelRequested | Status::Interrupted | Status::OutcomeUnknown | Status::NeedsReview => {}
                }
            }
            (None, None) => ensure!(!matches!(record.status,
                Status::Admitted | Status::Running | Status::ReceiptCompleted | Status::Completed | Status::DomainFailed | Status::FailedBeforeEffect),
                "execution control has no durable attempt"),
            _ => anyhow::bail!("execution control lost or invented its original attempt"),
        }
        Ok(())
    }
}

impl BridgeCore {
    pub(crate) fn method_value_digest(value: &serde_json::Value) -> Result<String> {
        validate_json_size(value, MAX_METHOD_RECEIPT_BYTES)?;
        super::projection::state_hash(Some(value))
    }

    pub(crate) fn verify_operation_result(
        &self, id: &str, principal_id: &str, session_id: &str, result: &serde_json::Value,
    ) -> Result<MethodReceiptRecovery> {
        operation_id(id)?;
        let scope = owner_scope(principal_id, session_id)?;
        let digest = Self::method_value_digest(result)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let state = owner.operations.get(&(scope, id.to_owned())).context("original operation unavailable")?;
        ensure!(state.status == MethodRecoveryStatus::Completed
            && state.terminal.is_some() && state.result_digest.as_deref() == Some(digest.as_str()),
            "original operation has no matching validated result binding");
        Ok(recovery(id, Some(state)))
    }

    pub(crate) fn verify_operation_binding(
        &self, id: &str, principal_id: &str, session_id: &str, plugin: &str, method: &str,
        schema_hash: &str, arguments: &serde_json::Value,
        attribution: Option<&op_execution_tracker::DelegationAttribution>,
    ) -> Result<MethodReceiptRecovery> {
        operation_id(id)?;
        let scope = owner_scope(principal_id, session_id)?;
        let digest = argument_digest(arguments)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let state = owner.operations.get(&(scope, id.to_owned())).context("original operation unavailable")?;
        owner
            .pin(&self.inner.catalog, plugin, schema_hash)
            .context("original operation request or orchestration binding differs")?;
        owner
            .pin(&self.inner.catalog, plugin, &state.schema_hash)
            .context("original operation request or orchestration binding differs")?;
        ensure!(state.plugin_id == plugin && state.method == method
            && state.request_digest.as_deref().unwrap_or(&state.arguments_digest) == digest.as_str()
            && state.attribution.as_ref() == attribution,
            "original operation request or orchestration binding differs");
        Ok(recovery(id, Some(state)))
    }

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
        self.begin_operation_inner(
            operation_id,
            principal_id,
            session_id,
            mutation,
            schema_hash,
            None,
            None,
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "method_attempt_unavailable; operation_id={operation_id}; recover before retry"
            )
        })
    }

    pub(crate) fn begin_operation_attributed(
        &self, operation_id: &str, principal_id: &str, session_id: &str,
        mutation: &CoreMutation, schema_hash: &str,
        attribution: Option<&op_execution_tracker::DelegationAttribution>,
        request_digest: &str,
    ) -> Result<MethodOperation> {
        self.begin_operation_inner(operation_id, principal_id, session_id, mutation, schema_hash, attribution, Some(request_digest))
            .map_err(|_| anyhow::anyhow!("method_attempt_unavailable; operation_id={operation_id}; recover before retry"))
    }

    fn begin_operation_inner(
        &self,
        id: &str,
        principal_id: &str,
        session_id: &str,
        mutation: &CoreMutation,
        schema_hash: &str,
        attribution: Option<&op_execution_tracker::DelegationAttribution>,
        request_digest: Option<&str>,
    ) -> Result<MethodOperation> {
        operation_id(id)?;
        if let Some(digest) = request_digest {
            ensure!(blake3::Hash::from_hex(digest).is_ok(), "actual method request binding is invalid");
        }
        let scope = owner_scope(principal_id, session_id)?;
        ensure!(
            mutation.operation == CoreOperation::CallMethod && mutation.actor_id == principal_id,
            "method attempt requires receiver-authenticated principal"
        );
        if let Some(attribution) = attribution {
            ensure!(attribution.executor_principal_id == principal_id
                && attribution.executor_session_id == session_id && attribution.policy_revision > 0,
                "operation delegation attribution differs from its verified executor");
        }
        let digest = argument_digest(&mutation.value)?;
        super::projection::path_segments(&mutation.plugin_id, &mutation.object_path)?;
        let _boundary = self
            .inner
            .commit
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self
            .inner
            .persistent
            .as_ref()
            .context("durable bridge required")?
            .lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let schema = owner.pin(&self.inner.catalog, &mutation.plugin_id, schema_hash)?;
        // Store the purpose-stable contract hash. The caller's presented hash
        // is still pinned above, so a stale blob hash is refused.
        let schema_hash = owner.contract_hash(&self.inner.catalog, &mutation.plugin_id)?;
        let method = schema
            .methods
            .get(&mutation.member_name)
            .context("method absent from sealed declaration")?;
        ensure!(
            method
                .required_capability
                .as_deref()
                .is_some_and(|capability| capability == mutation.capability_id),
            "method attempt capability differs from sealed declaration"
        );
        ensure!(
            !owner
                .operations
                .contains_key(&(scope.clone(), id.to_owned())),
            "operation already reserved; recover the original attempt, do not repeat execution"
        );
        let event = owner.append(
            &self.inner.catalog,
            scope.clone(),
            mutation,
            &schema_hash,
            LedgerAction::Attempt {
                operation_id: id.into(),
                schema_hash: schema_hash.clone(),
                arguments_digest: digest,
                attribution: attribution.cloned(),
                request_digest: request_digest.map(str::to_owned),
            },
        )?;
        let state = owner
            .operations
            .get(&(scope.clone(), id.to_owned()))
            .context("durable attempt metadata absent")?
            .clone();
        let _ = self.notices().send(StateNotice {
            scope: scope.clone(),
            plugin_id: event.plugin_id,
            event_id: event.event_id,
            state_committed: false,
        });
        Ok(MethodOperation {
            operation_id: id.into(),
            owner: scope,
            state,
        })
    }

    /// Commit the terminal proof and authoritative state in ONE ledger append,
    /// then publish SHM, then wake subscribers and return the native receipt.
    pub fn complete_operation(
        &self,
        operation: &MethodOperation,
        commit: BackendCommit,
    ) -> Result<MethodExecutionReceipt> {
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
        let _boundary = self
            .inner
            .commit
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self
            .inner
            .persistent
            .as_ref()
            .context("durable bridge required")?
            .lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let admitted = owner.require_operation(operation)?;
        owner
            .pin(
                &self.inner.catalog,
                &admitted.plugin_id,
                &commit.schema_hash,
            )
            .context("completed method differs from admitted attempt")?;
        owner
            .pin(
                &self.inner.catalog,
                &admitted.plugin_id,
                &admitted.schema_hash,
            )
            .context("completed method differs from admitted attempt")?;
        ensure!(
            commit.mutation.plugin_id == admitted.plugin_id
                && commit.mutation.member_name == admitted.method
                && commit.mutation.object_path == admitted.object_path
                && commit.mutation.capability_id == admitted.capability_id
                && argument_digest(&commit.mutation.value)? == admitted.arguments_digest,
            "completed method differs from admitted attempt"
        );
        let StateScope::Private { principal_id, .. } = &operation.owner else {
            unreachable!("admitted owner")
        };
        ensure!(
            commit.mutation.actor_id == *principal_id,
            "completed method actor mismatch"
        );
        if matches!(commit.scope, StateScope::Private { .. }) {
            ensure!(
                commit.scope == operation.owner,
                "completed private state scope mismatch"
            );
        }
        let schema = owner.pin(
            &self.inner.catalog,
            &admitted.plugin_id,
            &admitted.schema_hash,
        )?;
        let method = schema
            .methods
            .get(&admitted.method)
            .context("admitted method absent")?;
        let returns = method
            .returns
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let result = result
            .context("completed method has no backend result")
            .and_then(|result| {
                validate_json_size(&result, MAX_METHOD_RECEIPT_BYTES)?;
                validate_method_result(returns.as_ref(), &result)?;
                Ok(result)
            });
        let (status, code) = if result.is_ok() {
            (MethodRecoveryStatus::Completed, "completed")
        } else {
            (MethodRecoveryStatus::Unknown, "invalid_backend_result")
        };
        let result_digest = result.as_ref().ok().map(|result| Self::method_value_digest(result)).transpose()?;
        let state = owner.commit_with_updates(
            &self.inner.catalog,
            operation.owner.clone(),
            commit,
            updates,
            Some((operation.operation_id.clone(), status, code.into(), result_digest)),
            None,
        )?;
        let receipt = result.and_then(|result| {
            let receipt = MethodExecutionReceipt {
                success: true,
                operation_id: operation.operation_id.clone(),
                outcome: "completed".into(),
                event_id: operation.state.attempt.event_id,
                event_hash: operation.state.attempt.event_hash.clone(),
                terminal_event_id: state.event.event_id,
                terminal_event_hash: state.event.event_hash.clone(),
                plugin_id: operation.state.plugin_id.clone(),
                method: operation.state.method.clone(),
                result,
            };
            receipt.validate_attribution(&operation.state.plugin_id, &operation.state.method)?;
            Ok(receipt)
        });
        self.notify_committed(&state);
        let _ = self.notices().send(StateNotice {
            scope: operation.owner.clone(),
            plugin_id: state.event.plugin_id,
            event_id: state.event.event_id,
            state_committed: false,
        });
        receipt.map_err(|error| {
            error.context("backend reply rejected; observed state retained with unknown outcome")
        })
    }

    /// Transport/backend failure does not prove absence of effects. This closes
    /// only the audit outcome; it does not retry, roll back or invent state.
    pub fn outcome_unknown(&self, operation: &MethodOperation) -> Result<MethodReceiptRecovery> {
        self.outcome_unknown_with_updates(operation, Vec::new())
    }

    pub fn fail_operation_before_effect(
        &self,
        operation: &MethodOperation,
        refusal: UnsupportedBeforeEffect,
    ) -> Result<MethodReceiptRecovery> {
        self.fail_operation_before_effect_with_updates(operation, refusal, Vec::new())
    }

    /// Preserve observed intermediate state after a backend/provider failure
    /// without claiming a successful domain response or an effect-free refusal.
    pub fn outcome_unknown_with_updates(
        &self,
        operation: &MethodOperation,
        updates: Vec<BackendProjection>,
    ) -> Result<MethodReceiptRecovery> {
        self.record_terminal(
            operation,
            MethodRecoveryStatus::Unknown,
            "backend_or_publication_unknown",
            updates,
        )
    }

    pub fn fail_operation_before_effect_with_updates(
        &self,
        operation: &MethodOperation,
        refusal: UnsupportedBeforeEffect,
        updates: Vec<BackendProjection>,
    ) -> Result<MethodReceiptRecovery> {
        self.record_terminal(
            operation,
            MethodRecoveryStatus::Failed,
            refusal.code(),
            updates,
        )
    }

    fn record_terminal(
        &self,
        operation: &MethodOperation,
        status: MethodRecoveryStatus,
        code: &str,
        updates: Vec<BackendProjection>,
    ) -> Result<MethodReceiptRecovery> {
        let _boundary = self
            .inner
            .commit
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self
            .inner
            .persistent
            .as_ref()
            .context("durable bridge required")?
            .lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_operation(operation)?;
        let StateScope::Private { principal_id, .. } = &operation.owner else {
            unreachable!("admitted owner")
        };
        let mutation = CoreMutation {
            plugin_id: operation.state.plugin_id.clone(),
            object_path: operation.state.object_path.clone(),
            operation: CoreOperation::CallMethod,
            member_name: operation.state.method.clone(),
            value: serde_json::Value::Null,
            actor_id: principal_id.clone(),
            capability_id: operation.state.capability_id.clone(),
            idempotency_key: operation.operation_id.clone(),
        };
        let commit = BackendCommit {
            mutation,
            scope: operation.owner.clone(),
            schema_hash: operation.state.schema_hash.clone(),
            state: BackendStateUpdate::Unchanged,
            result: None,
        };
        let state = owner.commit_with_updates(
            &self.inner.catalog,
            operation.owner.clone(),
            commit,
            updates,
            Some((operation.operation_id.clone(), status, code.into(), None)),
            None,
        )?;
        self.notify_committed(&state);
        let result = recovery(
            &operation.operation_id,
            owner
                .operations
                .get(&(operation.owner.clone(), operation.operation_id.clone())),
        );
        let _ = self.notices().send(StateNotice {
            scope: operation.owner.clone(),
            plugin_id: state.event.plugin_id,
            event_id: state.event.event_id,
            state_committed: false,
        });
        Ok(result)
    }

    /// Same-principal AND same-session recovery. Missing and another caller's
    /// operation are indistinguishable. No private domain result is returned.
    pub fn recover_operation(
        &self,
        operation_id: &str,
        principal_id: &str,
        session_id: &str,
    ) -> Result<MethodReceiptRecovery> {
        self.recover_operation_inner(operation_id, principal_id, session_id)
    }

    fn recover_operation_inner(
        &self,
        id: &str,
        principal_id: &str,
        session_id: &str,
    ) -> Result<MethodReceiptRecovery> {
        operation_id(id)?;
        let scope = owner_scope(principal_id, session_id)?;
        let _boundary = self
            .inner
            .commit
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self
            .inner
            .persistent
            .as_ref()
            .context("durable bridge required")?
            .lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        Ok(recovery(id, owner.operations.get(&(scope, id.to_owned()))))
    }
}
