//! Mandatory private post-FUSE execution control. Prepared semantic evidence is
//! read here; this module never starts a provider/child operation under a root.
use super::*;
use op_execution_tracker::{DelegationAttribution, ExecutionContext, ExecutionProof, ExecutionRecord, ExecutionStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckpointStage { Preflight, Postflight, AgentFinal }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroundedOutcome { Succeeded, DomainFailed, Unverified, NeedsReview }

#[derive(Clone, Debug)]
pub(crate) enum CheckpointPolicy {
    /// The trusted native owner explicitly supplies deterministic task/schema/
    /// receipt/source checks sufficient for this exact operation and release.
    DeterministicOnly,
    RequireJudgment { policy_revision: u64, question_version: String },
}

#[derive(Clone, Debug)]
pub(crate) enum CallClass {
    Ordinary,
    /// Trusted final-claim release; source/task support is checked by the owner.
    AgentFinal,
    /// Selected by trusted infrastructure, never a flag in operation arguments.
    JudgmentInfrastructure { parent_operation_id: String, depth: u8 },
}

#[derive(Clone, Debug)]
pub(crate) struct CheckpointTarget {
    pub operation_id: Uuid,
    pub plugin_id: String,
    pub method: String,
    pub schema_hash: String,
    pub arguments_digest: String,
    pub result_digest: Option<String>,
    pub attribution: Option<DelegationAttribution>,
}

#[async_trait::async_trait]
pub(crate) trait CheckpointEvidenceOwner: Send + Sync {
    /// Recheck actual role, policy, resource scope, pins, budget and prepared
    /// continuation. Correlation fields never supply authority.
    async fn revalidate_binding(&self, runtime: &Runtime, caller: &VerifiedCaller,
        target: &CheckpointTarget, stage: CheckpointStage) -> Result<(), RuntimeError>;
    /// Compare the actual selected native route with the approved role pin
    /// immediately before provider bytes. An alias alone is not a route pin.
    async fn revalidate_provider_binding(&self, runtime: &Runtime, caller: &VerifiedCaller,
        target: &CheckpointTarget, profile: &op_agents::inference::NativeProviderProfile)
        -> Result<(), RuntimeError>;
    /// Capture the actual native Completed response before current disclosure
    /// can withhold it. Verify original scope/request/result/proofs and bound
    /// the private transient frame; no SHM publication or provider/child calls.
    async fn capture_completed(&self, runtime: &Runtime, caller: &VerifiedCaller,
        target: &CheckpointTarget, result: &Value, receipt: &MethodExecutionReceipt)
        -> Result<(), RuntimeError>;
    /// Read exact prepared core-backed request/response and apply the atomic
    /// judgment policy. No provider call, recursive judgment or heuristic here.
    async fn validate_committed_judgment(&self, runtime: &Runtime, caller: &VerifiedCaller,
        target: &CheckpointTarget, stage: CheckpointStage) -> Result<(), RuntimeError>;
    /// Native typed domain/source grounding, distinct from method completion.
    /// Raw result stays private; this owner must not send it to a model by default.
    async fn ground_response(&self, runtime: &Runtime, caller: &VerifiedCaller,
        target: &CheckpointTarget, result: &Value, receipt: &MethodExecutionReceipt)
        -> Result<GroundedOutcome, RuntimeError>;
}

#[derive(Clone)]
pub(crate) struct CallCheckpoint {
    pub policy: CheckpointPolicy,
    pub class: CallClass,
    /// Owner-approved native agent configuration alias; never model selection
    /// from arguments, a global dashboard or an ambient provider fallback.
    pub native_agent_alias: Option<String>,
    pub attribution: DelegationAttribution,
    pub owner: Arc<dyn CheckpointEvidenceOwner>,
}

impl CallCheckpoint {
    pub(crate) fn validate_caller(&self, caller: &VerifiedCaller) -> Result<(), RuntimeError> {
        if self.attribution.executor_principal_id != caller.principal_id
            || self.attribution.executor_session_id != caller.session_id {
            return Err(RuntimeError::denied("checkpoint executor differs from the verified container"));
        }
        if let CheckpointPolicy::RequireJudgment { policy_revision, question_version } = &self.policy {
            if *policy_revision == 0 || question_version.is_empty() || question_version.len() > 128 {
                return Err(RuntimeError::denied("required judgment policy is invalid"));
            }
        }
        if let CallClass::JudgmentInfrastructure { parent_operation_id, depth } = &self.class {
            if *depth != 1 || Uuid::parse_str(parent_operation_id).is_err()
                || self.native_agent_alias.as_deref().is_none_or(|alias|
                    alias.is_empty() || alias.len() > 128 || alias.chars().any(char::is_control)) {
                return Err(RuntimeError::denied("judgment infrastructure binding is invalid"));
            }
        }
        Ok(())
    }
}

impl Runtime {
    pub(super) fn execution_context(&self, caller: &VerifiedCaller, plugin: &str, method: &str, operation_id: Uuid)
        -> Result<ExecutionContext, RuntimeError> {
        let parent_operation_id = caller.checkpoint.as_ref().and_then(|checkpoint| match &checkpoint.class {
            CallClass::JudgmentInfrastructure { parent_operation_id, .. } => Some(parent_operation_id.clone()),
            CallClass::Ordinary | CallClass::AgentFinal => None,
        });
        let context = ExecutionContext {
            operation_id: operation_id.to_string(), trace_id: caller.trace_id.clone(), parent_operation_id,
            tool_name: format!("{plugin}.{method}"), executor_principal_id: Some(caller.principal_id.clone()),
            executor_session_id: Some(caller.session_id.clone()),
            attribution: caller.checkpoint.as_ref().map(|checkpoint| checkpoint.attribution.clone()),
        };
        context.validate().map_err(|_| RuntimeError::denied("execution checkpoint attribution is invalid"))?;
        if let Some(checkpoint) = &caller.checkpoint { checkpoint.validate_caller(caller)?; }
        Ok(context)
    }

    pub(super) fn checkpoint_target(&self, caller: &VerifiedCaller, plugin: &str, method: &str,
        args: &Value, operation_id: Uuid) -> Result<CheckpointTarget, RuntimeError> {
        let (pinned, _) = self.pinned_method(plugin, method)?;
        op_state_store::method_receipt::validate_json_size(args,
            op_state_store::method_receipt::MAX_METHOD_ARGUMENT_BYTES)?;
        Ok(CheckpointTarget {
            operation_id, plugin_id: plugin.to_owned(), method: method.to_owned(), schema_hash: pinned.schema_hash,
            arguments_digest: BridgeCore::method_value_digest(args)?, result_digest: None,
            attribution: caller.checkpoint.as_ref().map(|checkpoint| checkpoint.attribution.clone()),
        })
    }

    pub(super) async fn checkpoint_preflight(&self, caller: &VerifiedCaller,
        target: &CheckpointTarget, args: &Value) -> Result<(), RuntimeError> {
        let (_, declaration) = self.pinned_method(&target.plugin_id, &target.method)?;
        op_state_store::method_receipt::validate_json_size(args,
            op_state_store::method_receipt::MAX_METHOD_ARGUMENT_BYTES)?;
        crate::method_contract_admission::validate_admitted_arguments(&declaration, args)
            .map_err(|_| RuntimeError::denied("method input contract is unavailable or not satisfied"))?;
        self.authorize_nested(caller, &target.plugin_id, &target.method).await?;
        if caller.requires_checkpoint && caller.checkpoint.is_none() {
            return Err(RuntimeError::denied("delegated operation has no trusted checkpoint binding"));
        }
        let checkpoint = caller.checkpoint.as_ref()
            .ok_or_else(|| RuntimeError::denied("operation has no trusted grounding policy owner"))?;
        checkpoint.validate_caller(caller)?;
        checkpoint.owner.revalidate_binding(self, caller, target, CheckpointStage::Preflight).await?;
        if matches!(&checkpoint.policy, CheckpointPolicy::RequireJudgment { .. })
            && !matches!(&checkpoint.class, CallClass::JudgmentInfrastructure { .. }) {
            checkpoint.owner.validate_committed_judgment(self, caller, target, CheckpointStage::Preflight).await?;
        }
        Ok(())
    }

    /// Runs after the native effect root is released. Withholding cannot erase
    /// a committed effect or authorize another attempt with a new UUID.
    pub(super) async fn checkpoint_postflight(&self, caller: &VerifiedCaller,
        target: &CheckpointTarget, completed: &RuntimeResult) -> Result<GroundedOutcome, RuntimeError> {
        completed.receipt.validate_attribution(&target.plugin_id, &target.method)?;
        if completed.receipt.operation_id != target.operation_id.to_string() {
            return Err(RuntimeError::unknown(anyhow::anyhow!("checkpoint receipt operation differs")));
        }
        let (_, declaration) = self.pinned_method(&target.plugin_id, &target.method)?;
        crate::method_contract_admission::validate_admitted_output(&declaration, &completed.result)?;
        self.revalidate_result(completed, &target.plugin_id, &target.method).await?;
        let mut target = target.clone();
        target.result_digest = Some(BridgeCore::method_value_digest(&completed.result)?);
        self.core().verify_operation_result(&target.operation_id.to_string(),
            &caller.principal_id, &caller.session_id, &completed.result)?;
        let checkpoint = caller.checkpoint.as_ref()
            .ok_or_else(|| RuntimeError::unknown(anyhow::anyhow!("response grounding policy owner unavailable")))?;
        let stage = if matches!(&checkpoint.class, CallClass::AgentFinal) {
            CheckpointStage::AgentFinal
        } else { CheckpointStage::Postflight };
        // The original receipt remains scoped to the immutable principal and
        // session; only the native refresh owner may replace disclosure authority.
        let disclosure_caller = &completed.disclosure_caller;
        checkpoint.owner.revalidate_binding(self, disclosure_caller, &target, stage).await?;
        let outcome = checkpoint.owner.ground_response(self, disclosure_caller, &target,
            &completed.result, &completed.receipt).await?;
        if matches!(&checkpoint.policy, CheckpointPolicy::RequireJudgment { .. })
            && !matches!(&checkpoint.class, CallClass::JudgmentInfrastructure { .. }) {
            checkpoint.owner.validate_committed_judgment(self, disclosure_caller, &target, stage).await?;
        }
        // Grounding does not replace the final current disclosure boundary.
        self.revalidate_result(completed, &target.plugin_id, &target.method).await?;
        Ok(outcome)
    }

    pub(super) async fn checkpoint_transition(&self, context: &ExecutionContext,
        target: &CheckpointTarget, status: ExecutionStatus, proof: Option<ExecutionProof>)
        -> Result<ExecutionRecord, RuntimeError> {
        self.checkpoint_record(context, &target.plugin_id, &target.method, Some(&target.schema_hash), status, proof).await
    }

    /// Invalid input has no accepted request digest or attempt. Only resolvable
    /// canonical method metadata is persisted; no body/hash is fabricated.
    pub(super) async fn checkpoint_rejection(&self, context: &ExecutionContext,
        plugin: &str, method: &str) -> Result<ExecutionRecord, RuntimeError> {
        self.checkpoint_record(context, plugin, method, None, ExecutionStatus::Denied, None).await
    }

    async fn checkpoint_record(&self, context: &ExecutionContext, plugin: &str,
        method: &str, expected_schema: Option<&str>, status: ExecutionStatus,
        proof: Option<ExecutionProof>) -> Result<ExecutionRecord, RuntimeError> {
        let mut record = self.tracker().get_execution(context, &context.operation_id).await
            .ok_or_else(|| RuntimeError::unknown(anyhow::anyhow!("checkpoint reservation unavailable")))?;
        record.status = status;
        record.proof = proof;
        record.updated_at = chrono::Utc::now();
        record.duration_ms = (record.updated_at - record.created_at).num_milliseconds().max(0) as u64;
        let (pinned, declaration) = self.pinned_method(plugin, method)?;
        if expected_schema.is_some_and(|expected| expected != pinned.schema_hash) {
            return Err(RuntimeError::denied("checkpoint sealed method changed"));
        }
        let capability = declaration.required_capability.as_deref()
            .ok_or_else(|| RuntimeError::denied("sealed checkpoint capability absent"))?;
        let record = self.core().record_execution_control(record, plugin, method, &pinned.schema_hash, capability)
            .map_err(|error| {
                if BridgeCore::is_control_admission_capacity_refusal(&error) {
                    RuntimeError::denied("execution checkpoint capacity refused before admission")
                } else {
                    RuntimeError::unknown(error)
                }
            })?;
        self.tracker().apply_committed(record.clone()).await?;
        let (cursor, stats) = self.core().execution_control_stats(&StateScope::Private {
            principal_id: context.executor_principal_id.clone().expect("validated checkpoint scope"),
            session_id: context.executor_session_id.clone().expect("validated checkpoint scope"),
        })?;
        self.tracker().materialize_stats(context, cursor, stats).await;
        Ok(record)
    }

    /// Lagged subscribers recover the original scoped core cursor, never a
    /// broadcast history or a private domain result from another executor.
    pub(crate) async fn execution_checkpoint_page(&self, caller: &VerifiedCaller, after: u64, limit: usize)
        -> Result<Vec<ExecutionRecord>, RuntimeError> {
        caller.authority.revalidate().await.map_err(|_| RuntimeError::denied("checkpoint disclosure authority expired"))?;
        let page = self.core().execution_control_page(&caller.private_scope(), after, limit)?;
        for record in &page {
            let (plugin, method) = record.context.tool_name.split_once('.')
                .ok_or_else(|| RuntimeError::denied("checkpoint method reference invalid"))?;
            self.authorize_nested(caller, plugin, method).await?;
        }
        Ok(page)
    }
}

impl Runtime {
    /// Verify the result already read from the SAME mounted call descriptor.
    /// This performs no namespace/socket/transport I/O and is not caller ingress.
    pub(crate) async fn result_from_mounted(
        &self, caller: &VerifiedCaller, plugin: &str, method: &str,
        arguments: &Value, receipt: MethodExecutionReceipt,
    ) -> Result<RuntimeResult, RuntimeError> {
        receipt.validate_attribution(plugin, method)?;
        let operation_id = Uuid::parse_str(&receipt.operation_id)
            .map_err(|_| RuntimeError::denied("mounted receipt operation is not canonical"))?;
        let mut target = self.checkpoint_target(caller, plugin, method, arguments, operation_id)?;
        self.core().verify_operation_binding(&receipt.operation_id, &caller.principal_id,
            &caller.session_id, plugin, method, &target.schema_hash, arguments,
            caller.checkpoint.as_ref().map(|checkpoint| &checkpoint.attribution))?;
        self.core().verify_operation_result(&receipt.operation_id, &caller.principal_id,
            &caller.session_id, &receipt.result)?;
        target.result_digest = Some(BridgeCore::method_value_digest(&receipt.result)?);
        let mut disclosure_caller = caller.for_background();
        disclosure_caller.checkpoint = caller.checkpoint.clone();
        let completed = RuntimeResult {
            result: receipt.result.clone(), receipt,
            // A mounted method receipt is not proof of the current object's
            // effective hash; leave unavailable rather than invent one.
            effective_hash: String::new(), disclosure_caller, checkpoint_target: target,
        };
        match self.grounded_outcome(&completed).await? {
            GroundedOutcome::Succeeded | GroundedOutcome::DomainFailed => Ok(completed),
            GroundedOutcome::Unverified | GroundedOutcome::NeedsReview =>
                Err(RuntimeError::unknown(anyhow::anyhow!("mounted response remains unverified")).at_operation(operation_id)),
        }
    }

    /// Native consumers use the actual postflight decision, not receipt.success
    /// or guessed domain exit/status fields, to advance a task.
    pub(crate) async fn grounded_outcome(&self, completed: &RuntimeResult)
        -> Result<GroundedOutcome, RuntimeError> {
        self.revalidate_result(completed, &completed.receipt.plugin_id, &completed.receipt.method).await?;
        let record = self.core().execution_control_latest(&completed.disclosure_caller.private_scope(),
            &completed.receipt.operation_id)?.ok_or_else(|| RuntimeError::review("postflight decision unavailable"))?;
        let proof = record.proof.as_ref().ok_or_else(|| RuntimeError::review("postflight proof unavailable"))?;
        if proof.operation_id != completed.receipt.operation_id
            || proof.attempt.event_id != completed.receipt.event_id
            || proof.attempt.event_hash != completed.receipt.event_hash
            || proof.terminal.as_ref().is_none_or(|terminal|
                terminal.event_id != completed.receipt.terminal_event_id
                    || terminal.event_hash != completed.receipt.terminal_event_hash) {
            return Err(RuntimeError::unknown(anyhow::anyhow!("postflight receipt proof differs")));
        }
        Ok(match record.status {
            ExecutionStatus::Completed => GroundedOutcome::Succeeded,
            ExecutionStatus::DomainFailed => GroundedOutcome::DomainFailed,
            ExecutionStatus::NeedsReview => GroundedOutcome::NeedsReview,
            _ => GroundedOutcome::Unverified,
        })
    }

    /// Reconcile a privately captured ORIGINAL response after deferred judgment.
    /// This never dispatches a method, reserves a new attempt or replays effects.
    /// A missing/lost body or missing trusted evidence remains NeedsReview.
    pub(crate) async fn reconcile_checkpoint(
        &self, caller: &VerifiedCaller, plugin: &str, method: &str, arguments: &Value,
        receipt: MethodExecutionReceipt, checkpoint: CallCheckpoint,
    ) -> Result<RuntimeResult, RuntimeError> {
        if caller.execution.is_some() {
            return Err(RuntimeError::denied("checkpoint reconciliation requires ancestor-root release"));
        }
        let mut caller = caller.for_background();
        caller.attach_checkpoint(Arc::new(checkpoint))?;
        receipt.validate_attribution(plugin, method)?;
        let operation_id = Uuid::parse_str(&receipt.operation_id)
            .map_err(|_| RuntimeError::review("original checkpoint operation is invalid"))?;
        let mut target = self.checkpoint_target(&caller, plugin, method, arguments, operation_id)?;
        self.core().verify_operation_binding(&receipt.operation_id, &caller.principal_id,
            &caller.session_id, plugin, method, &target.schema_hash, arguments,
            caller.checkpoint.as_ref().map(|checkpoint| &checkpoint.attribution))?;
        let original = self.core().verify_operation_result(&receipt.operation_id,
            &caller.principal_id, &caller.session_id, &receipt.result)?;
        let attempt = original.attempt.ok_or_else(|| RuntimeError::review("original attempt proof unavailable"))?;
        let terminal = original.terminal.ok_or_else(|| RuntimeError::review("original terminal proof unavailable"))?;
        if attempt.event_id != receipt.event_id || attempt.event_hash != receipt.event_hash
            || terminal.event_id != receipt.terminal_event_id || terminal.event_hash != receipt.terminal_event_hash {
            return Err(RuntimeError::review("captured response differs from the original native proof"));
        }
        let context = self.execution_context(&caller, plugin, method, operation_id)?;
        let previous = self.core().execution_control_latest(&caller.private_scope(), &receipt.operation_id)?
            .ok_or_else(|| RuntimeError::review("original control reference unavailable"))?;
        if previous.context != context {
            return Err(RuntimeError::review("original checkpoint policy or task attribution differs"));
        }
        target.result_digest = Some(BridgeCore::method_value_digest(&receipt.result)?);
        let completed = RuntimeResult {
            result: receipt.result.clone(), receipt, effective_hash: String::new(),
            disclosure_caller: caller, checkpoint_target: target,
        };
        if previous.status.is_terminal() {
            // A final decision cannot be reclassified by later evidence. Current
            // policy/disclosure still applies before any original body is returned.
            return match self.grounded_outcome(&completed).await? {
                GroundedOutcome::Succeeded | GroundedOutcome::DomainFailed => Ok(completed),
                _ => Err(RuntimeError::review("original final checkpoint is not releasable")),
            };
        }
        // Only original core-backed records may hydrate a cold tracker. Neither
        // the private body cache nor supplied JSON supplies a control decision.
        self.tracker().apply_committed(previous).await?;
        let outcome = self.checkpoint_postflight(&completed.disclosure_caller,
            &completed.checkpoint_target, &completed).await?;
        let status = match outcome {
            GroundedOutcome::Succeeded => ExecutionStatus::Completed,
            GroundedOutcome::DomainFailed => ExecutionStatus::DomainFailed,
            GroundedOutcome::Unverified | GroundedOutcome::NeedsReview => ExecutionStatus::NeedsReview,
        };
        self.checkpoint_transition(&context, &completed.checkpoint_target, status,
            Some(ExecutionProof { operation_id: context.operation_id.clone(), attempt, terminal: Some(terminal) })).await?;
        if status == ExecutionStatus::NeedsReview {
            return Err(RuntimeError::review("original response still requires trusted grounding"));
        }
        self.revalidate_result(&completed, plugin, method).await?;
        Ok(completed)
    }
}
