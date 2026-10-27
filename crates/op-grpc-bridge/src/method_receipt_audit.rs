//! Durable accountability for admitted methods. No private domain payload is
//! placed in the ledger, and recovery never grants authority or replays work.
use std::sync::atomic::Ordering;

use anyhow::Context;
use futures::FutureExt;
use op_plugins::state_plugins::runtime_documents::RuntimeDocuments;
#[cfg(not(test))]
use op_plugins::state_plugins::runtime_documents::runtime_documents;
use op_state_store::method_receipt::{
    AuditedMethodRefusal, MAX_METHOD_RECEIPT_BYTES, MethodEventProof, MethodExecutionReceipt,
    MethodReceiptRecovery, MethodRecoveryStatus, UnsupportedBeforeEffect, validate_json_size,
    validate_method_result,
};
use op_state_store::{ChainEvent, Decision, OperationType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AdmittedMethodCall, MutationEngine, event_to_footprint, require_bound_oracle_grant};

const JOURNAL_NAMESPACE: &str = "__method_operation_v1";
const MAX_JOURNAL_BYTES: usize = 8192;

/// Error text, including a copied public refusal, cannot prove no effects.
fn before_effect_refusal(error: &anyhow::Error) -> Option<UnsupportedBeforeEffect> {
    error.downcast_ref::<UnsupportedBeforeEffect>().copied()
}

/// Closed diagnostic vocabulary. Backend error text can contain possession
/// material; only matched source conditions leave this boundary, never text.
fn safe_backend_failure_kind(error: &anyhow::Error) -> &'static str {
    for cause in error.chain() {
        if let Some(http) = cause.downcast_ref::<crate::chat_router_runtime::NativeHttpFailure>() {
            let catalog =
                http.operation.starts_with("list ") && http.operation.ends_with(" models");
            return match (catalog, http.operation.as_str(), http.status) {
                (true, _, 401 | 403) => "native_model_catalog_auth_rejected",
                (true, _, _) => "native_model_catalog_http_failed",
                (false, "chat", 401 | 403) => "native_agent_auth_rejected",
                (false, "chat", _) => "native_agent_http_failed",
                _ => "native_config_http_failed",
            };
        }
        let message = cause.to_string();
        for (condition, code) in [
            (
                "generation prompt is empty or exceeds its bound",
                "generation_prompt_invalid",
            ),
            (
                "config list response has no drifted array",
                "native_config_list_missing_drift",
            ),
            (
                "masked native configuration failed sealed-contract validation",
                "native_config_contract_mismatch",
            ),
            (
                "native configuration is unavailable",
                "native_config_unavailable",
            ),
            (
                "model catalog is unavailable",
                "native_model_catalog_unavailable",
            ),
            (
                "catalog model has no unique live ordinary native binding",
                "native_binding_not_unique",
            ),
            (
                "catalog binding profile inventory exceeds bound",
                "native_binding_inventory_bound",
            ),
            (
                "catalog binding native profile reference is invalid",
                "native_binding_reference_invalid",
            ),
            (
                "property display value is not a string",
                "native_property_shape_invalid",
            ),
            (
                "configured provider model is missing a string value",
                "native_provider_model_absent",
            ),
            (
                "config path is absent from pinned contract",
                "native_config_path_unclassified",
            ),
            (
                "config contract reference",
                "native_config_reference_unresolved",
            ),
            (
                "selected model is absent from its configured alias catalog",
                "native_selected_model_not_catalogued",
            ),
            ("model catalog is empty", "native_selected_catalog_empty"),
            (
                "native generation protocol rejected",
                "native_generation_rejected",
            ),
            ("model provider failed", "native_provider_failed"),
            (
                "model provider returned HTTP",
                "native_provider_http_failed",
            ),
            (
                "connection to the model provider failed",
                "native_provider_transport_failed",
            ),
            (
                "model provider returned an invalid response",
                "native_provider_response_invalid",
            ),
            (
                "model provider configuration is invalid",
                "native_provider_configuration_invalid",
            ),
            (
                "native generation transport failed",
                "native_generation_transport_failed",
            ),
            ("native generation HTTP", "native_generation_http_failed"),
            (
                "did not authenticate the forwarded SID1",
                "native_identity_ack_absent",
            ),
            (
                "native config changed after observation publication",
                "native_config_publication_changed",
            ),
            (
                "human message was not recorded",
                "conversation_record_failed",
            ),
            ("chat episode", "chat_episode_failed"),
            ("requires verified", "verified_context_missing"),
            ("compiled/sealed", "compiled_sealed_contract_mismatch"),
            ("chat_router list ", "native_model_catalog_failed"),
            (
                "chat_router read masked config",
                "native_config_read_failed",
            ),
            (
                "chat_router read config property",
                "native_property_read_failed",
            ),
            ("chat_router health", "native_health_failed"),
            (
                "ordinary native profile is owned",
                "ordinary_profile_agent_owned",
            ),
            (
                "ordinary native profile is reserved",
                "ordinary_profile_dedicated",
            ),
            (
                "episode publication requires recovery",
                "chat_episode_recovery_required",
            ),
            ("chat_router chat failed", "native_agent_rpc_failed"),
            (
                "chat_router chat returned invalid JSON",
                "native_agent_response_invalid",
            ),
            ("chat_router chat returned", "native_agent_http_failed"),
            (
                "chat_router chat request failed",
                "native_agent_transport_failed",
            ),
            ("requested provider", "native_selection_provider_mismatch"),
            ("requested model", "native_selection_model_mismatch"),
            (
                "canonical identity credential projection is unavailable",
                "native_identity_projection_unavailable",
            ),
            (
                "canonical MCP SID1 projection is unavailable",
                "native_identity_credential_unavailable",
            ),
            (
                "canonical SID1 claims are invalid",
                "native_identity_claims_invalid",
            ),
            (
                "SID1 actor does not match caller",
                "native_identity_principal_mismatch",
            ),
            (
                "SID1 session does not match caller",
                "native_identity_session_mismatch",
            ),
            (
                "SID1 genesis does not match caller",
                "native_identity_genesis_mismatch",
            ),
        ] {
            if message.contains(condition) {
                return code;
            }
        }
    }
    "unclassified_backend_failure"
}

async fn journal_documents(
    engine: &MutationEngine,
) -> anyhow::Result<std::sync::Arc<RuntimeDocuments>> {
    if let Some(documents) = engine.method_journal_documents.get() {
        return Ok(documents.clone());
    }
    // Unit fixtures never open the service's production Cozo path implicitly.
    #[cfg(test)]
    anyhow::bail!("method_journal_fixture_required");
    #[cfg(not(test))]
    {
        // The public Cozo plugin exposes arbitrary cognitive-owner queries. Never
        // allow operator path aliasing to place private journals in that database.
        let path = std::env::var_os("OP_PLUGIN_RUNTIME_COZO_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/var/lib/op-dbus/plugin-runtime-cozo".into());
        let cognitive = std::env::var_os("COGNITIVE_DB_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/var/lib/op-dbus/cognitive-memory.db".into());
        crate::chatbot_episode_state::distinct_databases(&path, &cognitive)?;
        engine
            .method_journal_documents
            .get_or_try_init(|| async {
                runtime_documents()
                    .await
                    .context("method_journal_unavailable")
            })
            .await
            .cloned()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationJournal {
    format: String,
    actor_id: String,
    session_id: Option<String>,
    schema_version: String,
    subid: String,
    sanitized_args_digest: String,
    receipt: MethodReceiptRecovery,
}

struct JournalCursor {
    documents: std::sync::Arc<RuntimeDocuments>,
    key: String,
    revision: u64,
    document: OperationJournal,
}

fn journal_key(
    actor_id: &str,
    session_id: Option<&str>,
    operation_id: &str,
) -> anyhow::Result<String> {
    let id = uuid::Uuid::parse_str(operation_id)?;
    anyhow::ensure!(id.to_string() == operation_id, "noncanonical operation id");
    // Receiver context only. JSON preserves tuple boundaries and distinguishes
    // an absent session from an empty one; the prefix domain-separates hashes.
    let scope = serde_json::to_vec(&(JOURNAL_NAMESPACE, actor_id, session_id))?;
    Ok(format!(
        "{JOURNAL_NAMESPACE}:{}:{operation_id}",
        blake3::hash(&scope).to_hex()
    ))
}

fn decode_journal(
    value: Value,
    actor_id: &str,
    session_id: Option<&str>,
    operation_id: &str,
) -> anyhow::Result<OperationJournal> {
    validate_json_size(&value, MAX_JOURNAL_BYTES)?;
    let document: OperationJournal = serde_json::from_value(value)?;
    anyhow::ensure!(
        document.format == JOURNAL_NAMESPACE
            && document.actor_id == actor_id
            && document.session_id.as_deref() == session_id
            && document.receipt.operation_id == operation_id,
        "method_journal_scope_invalid"
    );
    anyhow::ensure!(
        document.receipt.plugin_id.is_some()
            && document.receipt.method.is_some()
            && document.receipt.result_digest.is_none()
            && document.receipt.status != MethodRecoveryStatus::NotFound,
        "method_journal_metadata_invalid"
    );
    if let Some(attempt) = &document.receipt.attempt {
        anyhow::ensure!(
            attempt.event_id > 0
                && op_state_store::method_receipt::valid_event_hash(&attempt.event_hash),
            "method_journal_attempt_invalid"
        );
    }
    if let Some(terminal) = &document.receipt.terminal {
        let attempt = document
            .receipt
            .attempt
            .as_ref()
            .context("method_journal_attempt_absent")?;
        anyhow::ensure!(
            terminal.event_id > attempt.event_id
                && op_state_store::method_receipt::valid_event_hash(&terminal.event_hash),
            "method_journal_terminal_invalid"
        );
    }
    if matches!(
        document.receipt.status,
        MethodRecoveryStatus::Completed | MethodRecoveryStatus::Failed
    ) {
        anyhow::ensure!(
            document.receipt.terminal.is_some(),
            "method_journal_terminal_absent"
        );
    }
    Ok(document)
}

impl JournalCursor {
    async fn reserve(
        engine: &MutationEngine,
        call: &AdmittedMethodCall,
        args_digest: String,
    ) -> anyhow::Result<Self> {
        let operation_id = &call.change.change_id;
        let key = journal_key(&call.actor_id, call.session_id.as_deref(), operation_id)?;
        let documents = journal_documents(engine).await?;
        let document = OperationJournal {
            format: JOURNAL_NAMESPACE.into(),
            actor_id: call.actor_id.clone(),
            session_id: call.session_id.clone(),
            schema_version: call.admitted_schema.version.clone(),
            subid: call
                .admitted_schema
                .methods
                .get(&call.method)
                .context("admitted method declaration absent")?
                .subid
                .clone(),
            sanitized_args_digest: args_digest,
            receipt: MethodReceiptRecovery {
                operation_id: operation_id.clone(),
                status: MethodRecoveryStatus::Unknown,
                plugin_id: Some(call.plugin_id.clone()),
                method: Some(call.method.clone()),
                attempt: None,
                terminal: None,
                result_digest: None,
            },
        };
        let candidate = serde_json::to_value(&document)?;
        validate_json_size(&candidate, MAX_JOURNAL_BYTES)?;
        // Revision zero means absent. The existing native CAS and persistence
        // barrier establish a permanent tombstone before any audit/effect.
        let reserved = documents
            .update_at_revision_with_result(&key, 0, move |_| Ok((candidate, ())))
            .await;
        let revision = match reserved {
            Ok((row, ())) => row.revision,
            Err(_) => {
                // One bounded durable reconciliation; never retry reservation
                // or execute a backend after a conflict/uncertain submission.
                let previous = documents
                    .read_durable(&key)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|row| {
                        decode_journal(
                            row.document,
                            &call.actor_id,
                            call.session_id.as_deref(),
                            operation_id,
                        )
                        .ok()
                    });
                let code = match previous {
                    Some(previous)
                        if previous.receipt.plugin_id == document.receipt.plugin_id
                            && previous.receipt.method == document.receipt.method
                            && previous.sanitized_args_digest == document.sanitized_args_digest =>
                    {
                        "operation_already_submitted"
                    }
                    Some(_) => "operation_id_conflict",
                    None => "method_reservation_unknown",
                };
                anyhow::bail!(
                    "{code}; operation_id={operation_id}; use tched_fs.get_receipt; do not retry automatically"
                );
            }
        };
        Ok(Self {
            documents,
            key,
            revision,
            document,
        })
    }

    async fn persist(&mut self) -> anyhow::Result<()> {
        let candidate = serde_json::to_value(&self.document)?;
        validate_json_size(&candidate, MAX_JOURNAL_BYTES)?;
        let (row, ()) = self
            .documents
            .update_at_revision_with_result(&self.key, self.revision, move |_| Ok((candidate, ())))
            .await
            .context("method_journal_transition_unknown")?;
        self.revision = row.revision;
        Ok(())
    }
}

impl MutationEngine {
    /// Trusted service/fixture ownership injection, never a plugin argument or
    /// caller-selected database. Configure before listeners/admitted methods.
    pub fn configure_method_journal_documents(
        &self,
        documents: std::sync::Arc<RuntimeDocuments>,
    ) -> anyhow::Result<()> {
        self.method_journal_documents
            .set(documents)
            .map_err(|_| anyhow::anyhow!("method_journal_owner_already_configured"))
    }
    /// Unlike legacy observation records, a method cannot execute without a
    /// confirmed durable attempt. Failure can leave the disk head uncertain;
    /// require restart/replay rather than append behind a guessed head.
    async fn persist_required_method_event(&self, event: &ChainEvent) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.method_audit_uncertain.load(Ordering::Acquire),
            "method_audit_recovery_required"
        );
        let sink = self
            .audit_sink
            .get()
            .context("method_audit_durability_unavailable")?;
        if sink.add_footprint(event_to_footprint(event)).await.is_err() {
            self.method_audit_uncertain.store(true, Ordering::Release);
            anyhow::bail!("method_audit_durability_uncertain");
        }
        if let Some(projector) = self.vector_projector.get() {
            projector.notify();
        }
        Ok(())
    }

    pub(super) async fn audit_and_dispatch_admitted(
        &self,
        mut call: AdmittedMethodCall,
        sealed_id_digest: Option<String>,
    ) -> anyhow::Result<Value> {
        // The existing audit owner redacts private inputs before this point.
        // Never introduce a password oracle by hashing the unredacted request.
        let canonical_args =
            op_state_store::schema_validator::canonicalize_json(&call.change.new_value);
        let args_digest = blake3::hash(&simd_json::to_vec(&canonical_args)?)
            .to_hex()
            .to_string();
        anyhow::ensure!(
            self.audit_sink.get().is_some() && !self.method_audit_uncertain.load(Ordering::Acquire),
            "method_audit_durability_unavailable"
        );
        let mut journal = JournalCursor::reserve(self, &call, args_digest.clone()).await?;
        let attempt = {
            let mut chain = self.event_chain.write().await;
            let mut payload = serde_json::json!({
                "operation_id": call.change.change_id,
                "args_digest": args_digest,
                "method": call.method,
                "args": serde_json::to_value(&call.change.new_value)?,
            });
            if let Some(proof) = &call.bound_oracle {
                payload["_verified_transport"] = proof.audit_value();
            }
            let mut event = ChainEvent::new(
                chain.next_event_id(),
                chain.last_hash().to_owned(),
                call.actor_id.clone(),
                call.plugin_id.clone(),
                call.admitted_schema.version.clone(),
                OperationType::MethodCall,
                call.change.object_path.clone(),
                Vec::new(),
                Decision::Allow,
                &simd_json::serde::to_owned_value(payload)?,
            );
            event.session_id = call.session_id.clone();
            event.session_genesis = call.session_genesis.clone();
            event.sealed_id_digest = sealed_id_digest;
            event.capability_id = call.capability_id.clone();
            event.method_name = Some(call.method.clone());
            event.event_hash = event.compute_hash();
            self.persist_required_method_event(&event).await.map_err(|_| anyhow::anyhow!(
                "method_reservation_unknown; operation_id={}; attempt_audit_unavailable; do not retry automatically", call.change.change_id))?;
            chain.append(event).clone()
        };
        journal.document.receipt.attempt = Some(MethodEventProof {
            event_id: attempt.event_id,
            event_hash: attempt.event_hash.clone(),
        });
        journal.persist().await.map_err(|_| anyhow::anyhow!(
            "method_reservation_unknown; operation_id={}; attempt_journal_unavailable; do not retry automatically", call.change.change_id))?;
        call.change.event_id = attempt.event_id;
        call.change.event_hash = attempt.event_hash.clone();
        call.change.timestamp = attempt.timestamp;
        let _ = self.chain_tx.send(attempt.clone());
        let _ = self.change_tx.send(call.change.clone());

        let operation_id = call.change.change_id.clone();
        let declaration = call
            .admitted_schema
            .methods
            .get(&call.method)
            .context("admitted method declaration absent")?
            .clone();
        let result_schema = declaration
            .returns
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        // This check occurs before calling a backend. Later failures remain
        // unknown unless the backend explicitly certifies a pre-effect refusal.
        let before_effect = async {
            if let Some(proof) = &call.bound_oracle {
                crate::interceptor::revalidate_bound_oracle(
                    self,
                    proof,
                    &call.actor_id,
                    call.session_id.as_deref(),
                    call.session_genesis.as_deref(),
                    true,
                )
                .await?;
                require_bound_oracle_grant(&call.actor_id, call.capability_id.as_deref())?;
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if before_effect.is_err() {
            self.record_method_terminal(
                &attempt,
                &operation_id,
                "failed",
                None,
                "identity_changed_before_effect",
                &mut journal,
            )
            .await?;
            anyhow::bail!(
                "method_failed_before_effect; operation_id={operation_id}; identity_changed_before_effect"
            );
        }

        let outcome = std::panic::AssertUnwindSafe(self.dispatch_admitted_method(call))
            .catch_unwind()
            .await;
        let mut envelope = match outcome {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return self.record_method_failure(
                    &attempt, &operation_id, &mut journal, error,
                ).await;
            }
            Err(_) => {
                self.record_method_terminal(
                    &attempt,
                    &operation_id,
                    "unknown",
                    None,
                    "method_owner_panicked",
                    &mut journal,
                )
                .await?;
                anyhow::bail!(
                    "method_outcome_unknown; operation_id={operation_id}; do not retry automatically"
                );
            }
        };
        let verified = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                envelope["success"] == true
                    && envelope["plugin_id"] == attempt.plugin_id
                    && envelope["method"] == attempt.method_name.as_deref().unwrap_or_default(),
                "method envelope attribution mismatch"
            );
            anyhow::ensure!(
                envelope["event_id"] == attempt.event_id
                    && envelope["event_hash"] == attempt.event_hash,
                "method envelope attempt proof mismatch"
            );
            let result = envelope
                .get("result")
                .context("method envelope has no domain result")?;
            validate_method_result(result_schema.as_ref(), result)?;
            // Leave room for the fixed-size accountability envelope.
            validate_json_size(result, MAX_METHOD_RECEIPT_BYTES - 4096)?;
            Ok(())
        })();
        if verified.is_err() {
            self.record_method_terminal(
                &attempt,
                &operation_id,
                "unknown",
                None,
                "invalid_method_result",
                &mut journal,
            )
            .await?;
            anyhow::bail!(
                "method_outcome_unknown; operation_id={operation_id}; result_contract_failed; do not retry automatically"
            );
        }
        // A domain result can contain possession material or low-entropy
        // secrets. Without a target-owned safe redaction contract, even its
        // digest must stay out of the shared audit trail.
        let terminal = self.record_method_terminal(
            &attempt, &operation_id, "completed", None, "completed", &mut journal,
        ).await.map_err(|_| anyhow::anyhow!("method_outcome_unknown; operation_id={operation_id}; terminal_audit_unavailable; do not retry automatically"))?;
        envelope["operation_id"] = operation_id.into();
        envelope["outcome"] = "completed".into();
        envelope["terminal_event_id"] = terminal.event_id.into();
        envelope["terminal_event_hash"] = terminal.event_hash.into();
        let receipt: MethodExecutionReceipt = serde_json::from_value(envelope.clone())?;
        receipt.validate_attribution(
            &attempt.plugin_id,
            attempt.method_name.as_deref().unwrap_or_default(),
        )?;
        validate_json_size(&envelope, MAX_METHOD_RECEIPT_BYTES)?;
        Ok(envelope)
    }

    async fn record_method_failure(
        &self,
        attempt: &ChainEvent,
        operation_id: &str,
        journal: &mut JournalCursor,
        error: anyhow::Error,
    ) -> anyhow::Result<Value> {
        if let Some(refusal) = before_effect_refusal(&error) {
            // Only the typed, static diagnostic is public. Context and arbitrary
            // backend text cannot certify the absence of effects or leave here.
            let id = uuid::Uuid::parse_str(operation_id)?;
            self.record_method_terminal(
                attempt, operation_id, "failed", None, refusal.code(), journal,
            ).await?;
            return Err(AuditedMethodRefusal::new(id, refusal).into());
        }
        tracing::error!(
            operation_id = %operation_id,
            failure_kind = safe_backend_failure_kind(&error),
            "Admitted backend failed; outcome remains unknown"
        );
        self.record_method_terminal(
            attempt, operation_id, "unknown", None, "backend_or_publication_failed", journal,
        ).await?;
        anyhow::bail!(
            "method_outcome_unknown; operation_id={operation_id}; do not retry automatically"
        );
    }

    async fn record_method_terminal(
        &self,
        attempt: &ChainEvent,
        operation_id: &str,
        status: &str,
        result_digest: Option<&str>,
        code: &str,
        journal: &mut JournalCursor,
    ) -> anyhow::Result<ChainEvent> {
        let terminal = {
            let mut chain = self.event_chain.write().await;
            let payload = simd_json::json!({
                "operation_id": operation_id,
                "attempt_event_id": attempt.event_id,
                "attempt_event_hash": attempt.event_hash,
                "status": status,
                "result_digest": result_digest,
                "code": code,
            });
            let mut event = ChainEvent::new(
                chain.next_event_id(),
                chain.last_hash().to_owned(),
                attempt.actor_id.clone(),
                attempt.plugin_id.clone(),
                attempt.schema_version.clone(),
                OperationType::Custom("method_outcome".into()),
                attempt.target.clone(),
                Vec::new(),
                if status == "failed" {
                    Decision::Deny
                } else {
                    Decision::Allow
                },
                &payload,
            );
            event.session_id = attempt.session_id.clone();
            event.session_genesis = attempt.session_genesis.clone();
            event.sealed_id_digest = attempt.sealed_id_digest.clone();
            event.capability_id = attempt.capability_id.clone();
            event.method_name = attempt.method_name.clone();
            event.result_effective_hash = result_digest.map(str::to_owned);
            event.event_hash = event.compute_hash();
            self.persist_required_method_event(&event).await.map_err(|_| anyhow::anyhow!(
                "method_outcome_unknown; operation_id={operation_id}; terminal_audit_unavailable; do not retry automatically"))?;
            chain.append(event).clone()
        };
        let _ = self.chain_tx.send(terminal.clone());
        // No cross-store atomicity is claimed. Audit ordering is established
        // first, then the durable journal publishes only sanitized proof/state.
        journal.document.receipt.status = match status {
            "completed" => MethodRecoveryStatus::Completed,
            "failed" => MethodRecoveryStatus::Failed,
            _ => MethodRecoveryStatus::Unknown,
        };
        journal.document.receipt.terminal = Some(MethodEventProof {
            event_id: terminal.event_id,
            event_hash: terminal.event_hash.clone(),
        });
        journal.persist().await.map_err(|_| anyhow::anyhow!(
            "method_outcome_unknown; operation_id={operation_id}; terminal_journal_unavailable; do not retry automatically"))?;
        Ok(terminal)
    }

    /// The declared plugin method calls this only with receiver-authenticated
    /// actor/session context. Missing, another caller's, and unretained IDs
    /// deliberately return the same result. No argument can select a caller.
    pub(crate) async fn recover_method_receipt(
        &self,
        operation_id: &str,
        actor_id: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<MethodReceiptRecovery> {
        let key = journal_key(actor_id, session_id, operation_id)?;
        let recovery = MethodReceiptRecovery {
            operation_id: operation_id.into(),
            status: MethodRecoveryStatus::NotFound,
            plugin_id: None,
            method: None,
            attempt: None,
            terminal: None,
            result_digest: None,
        };
        let documents = journal_documents(self).await?;
        let Some(row) = documents
            .read_durable(&key)
            .await
            .context("method_journal_recovery_unavailable")?
        else {
            return Ok(recovery);
        };
        Ok(decode_journal(row.document, actor_id, session_id, operation_id)?.receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const OPERATION: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn generation_prompt_bound_has_a_safe_failure_classification() {
        let error = anyhow::anyhow!("generation prompt is empty or exceeds its bound")
            .context("private backend context must not be logged");
        assert_eq!(safe_backend_failure_kind(&error), "generation_prompt_invalid");
    }

    fn document() -> OperationJournal {
        OperationJournal {
            format: JOURNAL_NAMESPACE.into(),
            actor_id: "fixture-actor".into(),
            session_id: Some("fixture-session".into()),
            schema_version: "1.0.0".into(),
            subid: "obs.fixture.read@v1".into(),
            sanitized_args_digest: "a".repeat(64),
            receipt: MethodReceiptRecovery {
                operation_id: OPERATION.into(),
                status: MethodRecoveryStatus::Unknown,
                plugin_id: Some("fixture".into()),
                method: Some("read".into()),
                attempt: None,
                terminal: None,
                result_digest: None,
            },
        }
    }

    #[tokio::test]
    async fn typed_pre_effect_refusal_survives_worker_and_context() {
        for contextual in [false, true] {
            let backend = tokio::task::spawn_blocking(|| -> anyhow::Result<Value> {
                Err(UnsupportedBeforeEffect::new(
                    "threetched_skills_add_editor_unsupported", "omit --edit",
                ).into())
            }).await.context("execution worker failed").unwrap().unwrap_err();
            let backend = if contextual { backend.context("private backend context") } else { backend };
            let refusal = before_effect_refusal(&backend).expect("typed pre-effect refusal");
            assert_eq!(refusal.code(), "threetched_skills_add_editor_unsupported");
            assert_eq!(refusal.to_string(), "threetched_skills_add_editor_unsupported; omit --edit");
        }
    }

    #[tokio::test]
    async fn ordinary_nested_and_worker_failures_remain_unknown() {
        let cancelled = tokio::spawn(std::future::pending::<()>());
        cancelled.abort();
        let cancelled = anyhow::Error::new(cancelled.await.unwrap_err()).context("worker failed");
        let panicked = tokio::task::spawn_blocking(|| -> () { panic!("private worker failure") });
        let panicked = anyhow::Error::new(panicked.await.unwrap_err()).context("worker failed");
        let nested = AuditedMethodRefusal::new(
            uuid::Uuid::parse_str(OPERATION).unwrap(),
            UnsupportedBeforeEffect::new("editor_unsupported", "omit --edit"),
        );
        for backend in [
            anyhow::anyhow!("threetched_skills_add_editor_unsupported; omit --edit"),
            anyhow::anyhow!("threetched command exited with 1"),
            anyhow::anyhow!("operation completed but SHM publication failed"),
            anyhow::Error::new(nested).context("outer backend may already have effects"),
            cancelled,
            panicked,
        ] {
            assert!(before_effect_refusal(&backend).is_none());
        }
    }

    #[test]
    fn receiver_scopes_are_domain_separated_and_null_session_is_distinct() {
        let keys = [
            journal_key("ab", Some("c"), OPERATION).unwrap(),
            journal_key("a", Some("bc"), OPERATION).unwrap(),
            journal_key("a", None, OPERATION).unwrap(),
            journal_key("a", Some(""), OPERATION).unwrap(),
        ];
        assert_eq!(
            keys.iter().collect::<std::collections::BTreeSet<_>>().len(),
            4
        );
        assert!(journal_key("a", None, "00000000000040008000000000000001").is_err());
    }

    #[test]
    fn recovery_rejects_foreign_scope_and_unproven_completion() {
        let mut journal = document();
        let value = serde_json::to_value(&journal).unwrap();
        assert!(
            decode_journal(
                value.clone(),
                "other-actor",
                Some("fixture-session"),
                OPERATION
            )
            .is_err()
        );
        assert!(decode_journal(value, "fixture-actor", None, OPERATION).is_err());
        journal.receipt.status = MethodRecoveryStatus::Completed;
        assert!(
            decode_journal(
                serde_json::to_value(journal).unwrap(),
                "fixture-actor",
                Some("fixture-session"),
                OPERATION
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn durable_operation_tombstone_survives_reopen_and_rejects_reexecution() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("journal");
        let key = journal_key("fixture-actor", Some("fixture-session"), OPERATION).unwrap();
        let documents = std::sync::Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_persistent_private(path.clone()).unwrap(),
        ));
        let journal = document();
        let candidate = serde_json::to_value(&journal).unwrap();
        let (row, ()) = documents
            .update_at_revision_with_result(&key, 0, move |_| Ok((candidate, ())))
            .await
            .unwrap();
        let mut cursor = JournalCursor {
            documents,
            key: key.clone(),
            revision: row.revision,
            document: journal,
        };
        cursor.document.receipt.attempt = Some(MethodEventProof {
            event_id: 1,
            event_hash: "a".repeat(32),
        });
        cursor.persist().await.unwrap();
        cursor.document.receipt.status = MethodRecoveryStatus::Completed;
        cursor.document.receipt.terminal = Some(MethodEventProof {
            event_id: 2,
            event_hash: "b".repeat(32),
        });
        cursor.persist().await.unwrap();
        drop(cursor);
        let reopened = RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_persistent_private(path).unwrap(),
        );
        let row = reopened.read_durable(&key).await.unwrap().unwrap();
        let recovered = decode_journal(
            row.document,
            "fixture-actor",
            Some("fixture-session"),
            OPERATION,
        )
        .unwrap();
        assert_eq!(recovered.receipt.status, MethodRecoveryStatus::Completed);
        assert!(recovered.receipt.result_digest.is_none());
        let invoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observation = invoked.clone();
        assert!(
            reopened
                .update_at_revision_with_result(&key, 0, move |_| {
                    observation.store(true, Ordering::SeqCst);
                    Ok((serde_json::json!({}), ()))
                })
                .await
                .is_err()
        );
        assert!(!invoked.load(Ordering::SeqCst));
    }
}
