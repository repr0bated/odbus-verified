//! Admitted dispatch for private schema-tag and shadow-decision documents.
//!
//! This first slice records proposals only. It never installs forwarding,
//! claims an execution receipt, or treats a recorded allow as a credential.

use anyhow::{Context, Result, bail, ensure};
use op_plugins::state_plugins::{
    routing_decision::build_shadow_record, routing_decision_methods as decision,
    schema_tag_registry_methods as registry,
};
use op_state_store::PluginSchema;

use crate::{
    mutation_engine::MutationEngine,
    routing_resources::RoutingReservation,
    routing_store::{RegistryOwner, RoutingGuard, RoutingOwner, RoutingStore},
};

const DECISIONS: &str = "routing_decision";
const REGISTRY: &str = "schema_tag_registry";
const REGISTRY_OWNER_ENV: &str = "OP_ROUTING_POLICY_OWNER";

#[derive(Clone)]
struct CallContext {
    actor: String,
    capability: String,
    session: String,
    genesis: String,
    admitted: PluginSchema,
    engine: MutationEngine,
    plugin: &'static str,
    method: String,
    operation_id: String,
}

impl CallContext {
    fn final_admission(
        &self,
        operations: Vec<decision::ResolvedOperationRef>,
        associations: Vec<(
            u32,
            Vec<registry::SchemaTagAssociationRequest>,
            Vec<registry::SchemaTagAssociation>,
        )>,
    ) -> impl FnOnce() -> Result<()> + Send + 'static {
        let actor = self.actor.clone();
        let capability = self.capability.clone();
        let session = self.session.clone();
        let genesis = self.genesis.clone();
        let admitted = self.admitted.clone();
        let method = self.method.clone();
        let engine = self.engine.clone();
        let plugin = self.plugin;
        let runtime = tokio::runtime::Handle::current();
        move || {
            runtime.block_on(async move {
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor.clone(),
                    capability: Some(capability),
                    session: Some(session),
                    genesis: Some(genesis),
                };
                crate::tched_router_admission::readmit_call(
                    &engine, &admitted, plugin, &method, &caller,
                )
                .await?;
                for operation in operations {
                    crate::routing_catalog::recheck(operation).await?;
                }
                for (tag_id, requests, expected) in associations {
                    let current =
                        crate::routing_catalog::resolve_associations(requests, tag_id).await?;
                    ensure!(
                        current == expected,
                        "sealed operation association changed while queued"
                    );
                }
                Ok(())
            })
        }
    }
}

/// Called only after the MutationEngine has validated the sealed method and
/// payload. `reservation` was acquired before parsing and is transferred into
/// the owned store task exactly once.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    plugin: &str,
    method: &str,
    args: &serde_json::Value,
    actor: &str,
    capability: Option<&str>,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted: &PluginSchema,
    operation_id: &str,
    reservation: RoutingAdmission,
) -> Result<serde_json::Value> {
    ensure!(
        plugin == DECISIONS || plugin == REGISTRY,
        "unsupported routing plugin"
    );
    let declaration = admitted
        .methods
        .get(method)
        .context("routing method is absent from admitted schema")?;
    let required = declaration
        .required_capability
        .as_deref()
        .context("routing method has no declared capability")?;
    ensure!(
        capability == Some(required),
        "exact routing capability is required"
    );
    let session =
        crate::project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let genesis = genesis.context("routing requires the verified session genesis")?;
    let context = CallContext {
        actor: actor.to_owned(),
        capability: required.to_owned(),
        session: session.clone(),
        genesis: genesis.to_owned(),
        admitted: admitted.clone(),
        method: method.to_owned(),
        engine: engine.clone(),
        plugin: if plugin == DECISIONS {
            DECISIONS
        } else {
            REGISTRY
        },
        operation_id: operation_id.to_owned(),
    };
    let store = engine.routing_store()?.clone();
    let output = if plugin == DECISIONS {
        let owner = RoutingOwner::new(actor.to_owned(), session.clone(), genesis.to_owned())?;
        if method == "record_shadow_decision" {
            let input: decision::RecordDecisionInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let operation =
                crate::routing_catalog::resolve_candidate(input.candidate.clone()).await?;
            let policy_owner = reservation
                .registry_owner
                .clone()
                .context("stable registry owner was not reserved")?;
            // Lock ordering is registry then decisions. Store mutation keeps
            // both guards through cancellation cleanup and durable publication.
            let registry_guard = store
                .lock_registry_reserved(
                    policy_owner.clone(),
                    reservation
                        .registry
                        .context("registry reservation missing")?,
                )
                .await?;
            let (registry_state, registry_guard) = store
                .read_registry_retained(
                    registry_guard,
                    context.final_admission(vec![operation.clone()], vec![]),
                )
                .await?;
            let mut decision_guard = store
                .lock_decisions_reserved(
                    owner.clone(),
                    reservation
                        .decision
                        .context("decision reservation missing")?,
                )
                .await?;
            decision_guard.retain_registry(registry_guard)?;
            let prepared = PreparedShadow {
                input,
                operation,
                registry_state,
                policy_owner,
            };
            dispatch_decision(
                engine,
                &store,
                decision_guard,
                owner,
                Some(prepared),
                &context,
                args,
            )
            .await?
        } else {
            let guard = store
                .lock_decisions_reserved(
                    owner.clone(),
                    reservation
                        .decision
                        .context("decision reservation missing")?,
                )
                .await?;
            dispatch_decision(engine, &store, guard, owner, None, &context, args).await?
        }
    } else {
        let owner = reservation
            .registry_owner
            .context("stable registry owner was not reserved")?;
        dispatch_registry(
            &store,
            owner,
            reservation
                .registry
                .context("registry reservation missing")?,
            &context,
            args,
        )
        .await?
    };
    crate::method_contract_admission::validate_admitted_output(
        admitted
            .methods
            .get(method)
            .context("routing method disappeared from admitted schema")?,
        &output,
    )?;
    Ok(output)
}

struct PreparedShadow {
    input: decision::RecordDecisionInput,
    operation: decision::ResolvedOperationRef,
    registry_state: registry::RegistryState,
    policy_owner: RegistryOwner,
}

pub(crate) struct RoutingAdmission {
    pub decision: Option<RoutingReservation>,
    pub registry: Option<RoutingReservation>,
    pub registry_owner: Option<RegistryOwner>,
}

/// Reserve the owner-scoped work before the engine copies or parses method
/// arguments. The reservation moves with AdmittedMethodCall into its owned task.
pub(crate) async fn reserve_before_parse(
    engine: &MutationEngine,
    plugin: &str,
    method: &str,
    capability: Option<&str>,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<Option<(PluginSchema, RoutingAdmission)>> {
    if plugin != DECISIONS && plugin != REGISTRY {
        return Ok(None);
    }
    let (_declaration, schema) =
        crate::method_contract_admission::admit_compiled_invocation(plugin, method, capability)?;
    let verified_session =
        crate::project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let genesis = genesis.context("routing requires verified session genesis")?;
    let store = engine.routing_store()?.clone();
    let mut admission = RoutingAdmission {
        decision: None,
        registry: None,
        registry_owner: None,
    };
    if plugin == DECISIONS {
        let owner = RoutingOwner::new(actor.to_owned(), verified_session, genesis.to_owned())?;
        admission.decision = Some(store.reserve_decisions(&owner)?);
        if method == "record_shadow_decision" {
            let owner = registry_owner()?;
            admission.registry = Some(store.reserve_registry(&owner)?);
            admission.registry_owner = Some(owner);
        }
    } else {
        let owner = registry_owner()?;
        admission.registry = Some(store.reserve_registry(&owner)?);
        admission.registry_owner = Some(owner);
    }
    Ok(Some((schema, admission)))
}

fn registry_owner() -> Result<RegistryOwner> {
    let owner_id = std::env::var(REGISTRY_OWNER_ENV).with_context(|| {
        format!("{REGISTRY_OWNER_ENV} must select the stable policy-owner scope")
    })?;
    registry::validate_text(&owner_id, "stable policy owner", registry::MAX_ID_BYTES)?;
    Ok(RegistryOwner { owner_id })
}

async fn read_registry(
    store: &RoutingStore,
    owner: RegistryOwner,
    reservation: RoutingReservation,
    final_admission: impl FnOnce() -> Result<()> + Send + 'static,
) -> Result<registry::RegistryState> {
    let guard = store.lock_registry_reserved(owner, reservation).await?;
    store.read_registry(guard, final_admission).await
}

async fn dispatch_decision(
    engine: &MutationEngine,
    store: &RoutingStore,
    guard: RoutingGuard,
    owner: RoutingOwner,
    prepared_shadow: Option<PreparedShadow>,
    context: &CallContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match context.method.as_str() {
        "list_decisions" => {
            let input: decision::ListDecisionsInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let page_size = usize::from(input.limit).min(store.resources().limits().max_page_size);
            let state = store
                .read_decisions(guard, context.final_admission(vec![], vec![]))
                .await?;
            let (offset, revision) = page_cursor(input.cursor.as_deref(), state.revision)?;
            let end = offset.saturating_add(page_size).min(state.records.len());
            let records = if offset <= state.records.len() {
                state.records[offset..end].to_vec()
            } else {
                bail!("routing page cursor offset is invalid")
            };
            let next_cursor = (end < state.records.len()).then(|| format!("{}:{end}", revision));
            Ok(serde_json::to_value(decision::ListDecisionsOutput {
                decisions: records,
                next_cursor,
            })?)
        }
        "get_decision" => {
            let input: decision::DecisionIdInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let state = store
                .read_decisions(guard, context.final_admission(vec![], vec![]))
                .await?;
            let decision = state
                .records
                .into_iter()
                .find(|record| record.decision_id == input.decision_id);
            Ok(serde_json::to_value(decision::GetDecisionOutput {
                decision,
            })?)
        }
        "record_shadow_decision" => {
            let PreparedShadow {
                input,
                operation,
                registry_state,
                policy_owner,
            } = prepared_shadow.context("shadow decision was not prepared")?;
            let taxonomy = engine
                .projected_state::<op_plugins::state_plugins::gemma_brain::GemmaBrainState>(
                    "gemma_brain",
                )
                .await?;
            let current_taxonomy_revision = taxonomy.and_then(|state| state.taxonomy_revision);
            let marker = registry_state
                .protected_methods
                .iter()
                .find(|marker| {
                    marker.plugin_id == operation.plugin_id
                        && marker.method_key == operation.method_key
                })
                .cloned();
            let associations: Vec<_> = registry_state
                .tags
                .iter()
                .flat_map(|tag| tag.associations.iter().cloned())
                .filter(|association| {
                    association.plugin_id == operation.plugin_id
                        && association.method_key == operation.method_key
                        && association.schema_hash == operation.schema_hash
                })
                .collect();
            let resolved_tag = input
                .classification_proposal
                .as_ref()
                .and_then(|proposal| proposal.tag.as_ref())
                .and_then(|name| registry_state.tags.iter().find(|tag| &tag.name == name))
                .map(|tag| registry::ResolvedTagRef {
                    tag_id: tag.tag_id,
                    revision: tag.revision,
                    name: tag.name.clone(),
                });
            let policy_snapshot = registry::PolicySnapshot {
                owner_id: policy_owner.owner_id.clone(),
                rule_revision: registry_state.revision,
                marker,
                records: registry_state
                    .policies
                    .into_iter()
                    .filter(|policy| policy.owner_id == policy_owner.owner_id)
                    .collect(),
            };
            let gate_input = decision::GateInput {
                caller: decision::VerifiedCaller {
                    principal_id: owner.principal_id.clone(),
                    session_id: owner.session_id.clone(),
                    owner_id: owner.owner_id.clone(),
                    container: None,
                    project_scope: None,
                    capabilities: crate::interceptor::load_capability_grants(&owner.principal_id)
                        .into_iter()
                        .collect(),
                },
                policy_owner_id: policy_owner.owner_id,
                operation: operation.clone(),
                classification: input.classification_proposal.clone(),
                current_taxonomy_revision,
                resolved_tag,
                tag_associations: associations,
                policy_snapshot: Some(policy_snapshot),
                policy_read_succeeded: true,
                marker_lookup_succeeded: true,
                requested_effect: decision::RequestedEffect::Local,
                residual_validation_passed: false,
                destination_class: None,
                provider_route: None,
            };
            let target_capability = gate_input.operation.required_capability.clone();
            let decision_id = uuid::Uuid::now_v7().to_string();
            let record = build_shadow_record(
                decision_id,
                input,
                gate_input.caller.clone(),
                operation.clone(),
                gate_input,
                None,
            )?;
            let operation_id = context.operation_id.clone();
            let store_copy = store.clone();
            let caller_context = context.clone();
            let operation_admission = caller_context.final_admission(vec![operation], vec![]);
            let final_actor = context.actor.clone();
            let final_engine = engine.clone();
            let expected_taxonomy_revision = current_taxonomy_revision;
            let runtime = tokio::runtime::Handle::current();
            let mut operation_admission = Some(operation_admission);
            let final_admission = move || {
                (operation_admission
                    .take()
                    .context("routing operation admission already consumed")?)()?;
                if let Some(required) = target_capability.as_deref() {
                    ensure!(
                        crate::interceptor::load_capability_grants(&final_actor).contains(required),
                        "current target operation capability is required"
                    );
                }
                let latest = runtime.block_on(final_engine.projected_state::<op_plugins::state_plugins::gemma_brain::GemmaBrainState>("gemma_brain"))?
                    .and_then(|state| state.taxonomy_revision);
                ensure!(
                    latest == expected_taxonomy_revision,
                    "routing taxonomy revision changed while queued"
                );
                Ok(())
            };
            let record = store_copy
                .record_decision(guard, operation_id, record, final_admission)
                .await?;
            Ok(serde_json::to_value(decision::RecordDecisionOutput {
                decision: record,
            })?)
        }
        _ => bail!("unsupported routing_decision method"),
    }
}

async fn dispatch_registry(
    store: &RoutingStore,
    owner: RegistryOwner,
    reservation: RoutingReservation,
    context: &CallContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match context.method.as_str() {
        "list_tags" => {
            let input: registry::ListTagsInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let state = read_registry(
                store,
                owner,
                reservation,
                context.final_admission(vec![], vec![]),
            )
            .await?;
            let size = usize::from(input.limit).min(store.resources().limits().max_page_size);
            let (offset, revision) = page_cursor(input.cursor.as_deref(), state.revision)?;
            let end = offset.saturating_add(size).min(state.tags.len());
            let tags = if offset <= state.tags.len() {
                state.tags[offset..end].to_vec()
            } else {
                bail!("routing page cursor offset is invalid")
            };
            let next_cursor = (end < state.tags.len()).then(|| format!("{}:{end}", revision));
            Ok(serde_json::to_value(registry::ListTagsOutput {
                tags,
                next_cursor,
            })?)
        }
        "get_tag" => {
            let input: registry::TagIdInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let state = read_registry(
                store,
                owner,
                reservation,
                context.final_admission(vec![], vec![]),
            )
            .await?;
            Ok(serde_json::to_value(registry::GetTagOutput {
                tag: state
                    .tags
                    .into_iter()
                    .find(|tag| tag.tag_id == input.tag_id),
            })?)
        }
        "get_protected_method_marker" => {
            let input: registry::ProtectedMethodMarkerKeyInput =
                serde_json::from_value(args.clone())?;
            registry::validate_text(&input.plugin_id, "plugin_id", registry::MAX_ID_BYTES)?;
            registry::validate_text(&input.method_key, "method_key", registry::MAX_ID_BYTES)?;
            registry::validate_text(&input.schema_hash, "schema_hash", registry::MAX_ID_BYTES)?;
            let state = read_registry(
                store,
                owner,
                reservation,
                context.final_admission(vec![], vec![]),
            )
            .await?;
            Ok(serde_json::to_value(
                registry::GetProtectedMethodMarkerOutput {
                    marker: state.protected_methods.into_iter().find(|marker| {
                        marker.plugin_id == input.plugin_id
                            && marker.method_key == input.method_key
                            && marker.schema_hash == input.schema_hash
                    }),
                },
            )?)
        }
        "list_policies" => {
            let input: registry::ListPoliciesInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let state = read_registry(
                store,
                owner,
                reservation,
                context.final_admission(vec![], vec![]),
            )
            .await?;
            let size = usize::from(input.limit).min(store.resources().limits().max_page_size);
            let (offset, revision) = page_cursor(input.cursor.as_deref(), state.revision)?;
            let end = offset.saturating_add(size).min(state.policies.len());
            let policies = if offset <= state.policies.len() {
                state.policies[offset..end].to_vec()
            } else {
                bail!("routing page cursor offset is invalid")
            };
            let next_cursor = (end < state.policies.len()).then(|| format!("{}:{end}", revision));
            Ok(serde_json::to_value(registry::ListPoliciesOutput {
                policies,
                next_cursor,
            })?)
        }
        "get_policy" => {
            let input: registry::PolicyIdInput = serde_json::from_value(args.clone())?;
            input.validate()?;
            let state = read_registry(
                store,
                owner,
                reservation,
                context.final_admission(vec![], vec![]),
            )
            .await?;
            Ok(serde_json::to_value(registry::GetPolicyOutput {
                policy: state
                    .policies
                    .into_iter()
                    .find(|policy| policy.policy_id == input.policy_id),
            })?)
        }
        "register_tag_semantics" => {
            let input: registry::RegisterTagInput = serde_json::from_value(args.clone())?;
            ensure!(input.tag_id > 0, "tag_id must be positive");
            registry::validate_text(&input.name, "tag name", registry::MAX_ID_BYTES)?;
            registry::validate_text(
                &input.description,
                "tag description",
                registry::MAX_TEXT_BYTES,
            )?;
            ensure!(
                input.associations.len() <= store.resources().limits().max_evidence_fanout,
                "routing_evidence_capacity"
            );
            ensure!(
                input.openflow_binding.is_none(),
                "OpenFlow traffic binding is unavailable until an authoritative identity-to-traffic binding is configured"
            );
            let associations = crate::routing_catalog::resolve_associations(
                input.associations.clone(),
                input.tag_id,
            )
            .await?;
            let expected = associations.clone();
            let requests = input.associations.clone();
            let tag_id = input.tag_id;
            let association_check = vec![(tag_id, requests, expected)];
            mutate_registry(
                store,
                owner,
                reservation,
                context,
                association_check,
                vec![],
                move |state| {
                    let tag = registry::apply_tag_registration(
                        &mut state.tags,
                        input,
                        associations,
                        None,
                    )?;
                    Ok(serde_json::to_value(registry::RegisterTagOutput {
                        tag,
                        registry_revision: state.revision,
                    })?)
                },
            )
            .await
        }
        "set_protected_method_marker" => {
            let input: registry::SetProtectedMethodMarkerInput =
                serde_json::from_value(args.clone())?;
            input.marker.validate()?;
            let candidate = decision::CandidateOperation {
                plugin_id: input.marker.plugin_id.clone(),
                method_key: input.marker.method_key.clone(),
                schema_hash: input.marker.schema_hash.clone(),
            };
            let operation = crate::routing_catalog::resolve_candidate(candidate).await?;
            mutate_registry(
                store,
                owner,
                reservation,
                context,
                vec![],
                vec![operation.clone()],
                move |state| {
                    let marker = registry::apply_protected_method_marker(
                        &mut state.protected_methods,
                        input,
                        &operation.plugin_id,
                        &operation.method_key,
                        &operation.schema_hash,
                    )?;
                    Ok(serde_json::to_value(
                        registry::ProtectedMethodMarkerMutationOutput { marker },
                    )?)
                },
            )
            .await
        }
        "author_policy" => {
            let input: registry::AuthorPolicyInput = serde_json::from_value(args.clone())?;
            reject_unavailable_policy_evidence(&input.rule, &input.canonical_mappings)?;
            let operations = resolve_policy_rule(&input.rule).await?;
            let owner_id = owner.owner_id.clone();
            mutate_registry(
                store,
                owner,
                reservation,
                context,
                vec![],
                operations,
                move |state| {
                    validate_policy_tag(&input.rule, &state.tags)?;
                    let policy =
                        registry::author_policy(&mut state.policies, &owner_id, input, vec![])?;
                    Ok(serde_json::to_value(registry::PolicyMutationOutput {
                        policy,
                        rule_revision: state.revision,
                    })?)
                },
            )
            .await
        }
        "update_policy" => {
            let input: registry::UpdatePolicyInput = serde_json::from_value(args.clone())?;
            reject_unavailable_policy_evidence(&input.rule, &input.canonical_mappings)?;
            let operations = resolve_policy_rule(&input.rule).await?;
            let owner_id = owner.owner_id.clone();
            mutate_registry(
                store,
                owner,
                reservation,
                context,
                vec![],
                operations,
                move |state| {
                    validate_policy_tag(&input.rule, &state.tags)?;
                    let policy = registry::apply_policy_update(
                        &mut state.policies,
                        &owner_id,
                        input,
                        vec![],
                    )?;
                    Ok(serde_json::to_value(registry::PolicyMutationOutput {
                        policy,
                        rule_revision: state.revision,
                    })?)
                },
            )
            .await
        }
        "revoke_policy" => {
            let input: registry::RevokePolicyInput = serde_json::from_value(args.clone())?;
            registry::validate_text(&input.policy_id, "policy_id", registry::MAX_ID_BYTES)?;
            registry::validate_text(&input.reason, "revocation reason", registry::MAX_TEXT_BYTES)?;
            let policy_id = input.policy_id.clone();
            let expected_revision = input.expected_revision;
            let reason = input.reason.clone();
            let owner_id = owner.owner_id.clone();
            mutate_registry(
                store,
                owner,
                reservation,
                context,
                vec![],
                vec![],
                move |state| {
                    let policy = registry::revoke_policy(
                        &mut state.policies,
                        &owner_id,
                        &policy_id,
                        expected_revision,
                        &reason,
                    )?;
                    Ok(serde_json::to_value(registry::PolicyMutationOutput {
                        policy,
                        rule_revision: state.revision,
                    })?)
                },
            )
            .await
        }
        _ => bail!("unsupported schema_tag_registry method"),
    }
}

async fn mutate_registry<R, F>(
    store: &RoutingStore,
    owner: RegistryOwner,
    reservation: RoutingReservation,
    context: &CallContext,
    associations: Vec<(
        u32,
        Vec<registry::SchemaTagAssociationRequest>,
        Vec<registry::SchemaTagAssociation>,
    )>,
    operations: Vec<decision::ResolvedOperationRef>,
    change: F,
) -> Result<R>
where
    R: Send + 'static,
    F: FnOnce(&mut registry::RegistryState) -> Result<R> + Send + 'static,
{
    mutate_registry_with_operations(
        store,
        owner,
        reservation,
        context,
        associations,
        operations,
        change,
    )
    .await
}

async fn mutate_registry_with_operations<R, F>(
    store: &RoutingStore,
    owner: RegistryOwner,
    reservation: RoutingReservation,
    context: &CallContext,
    associations: Vec<(
        u32,
        Vec<registry::SchemaTagAssociationRequest>,
        Vec<registry::SchemaTagAssociation>,
    )>,
    operations: Vec<decision::ResolvedOperationRef>,
    change: F,
) -> Result<R>
where
    R: Send + 'static,
    F: FnOnce(&mut registry::RegistryState) -> Result<R> + Send + 'static,
{
    let guard = store.lock_registry_reserved(owner, reservation).await?;
    let mut final_admission = Some(context.final_admission(operations, associations));
    store
        .mutate_registry(
            guard,
            context.operation_id.clone(),
            move |state| change(state),
            move || {
                final_admission
                    .take()
                    .context("routing final admission was already consumed")?()
            },
        )
        .await
}

fn reject_unavailable_policy_evidence(
    rule: &registry::PolicyRule,
    mappings: &[registry::CanonicalMappingRequest],
) -> Result<()> {
    ensure!(
        mappings.is_empty(),
        "canonical Qdrant mapping resolution is unavailable; caller-authored references are rejected"
    );
    if matches!(rule, registry::PolicyRule::EgressRule { .. }) {
        bail!(
            "egress policy is unavailable without canonical OSCAL mapping resolution and content-bound egress evidence"
        );
    }
    rule.validate()
}

async fn resolve_policy_rule(
    rule: &registry::PolicyRule,
) -> Result<Vec<decision::ResolvedOperationRef>> {
    let registry::PolicyRule::DenyMethod {
        plugin_id,
        method_key,
        ..
    } = rule
    else {
        return Ok(Vec::new());
    };
    // Resolve the key through the maintained sealed-catalog resolver; the
    // arbitrary numeric tag is only an association envelope for that API.
    let mut associations = crate::routing_catalog::resolve_associations(
        vec![registry::SchemaTagAssociationRequest {
            plugin_id: plugin_id.clone(),
            method_key: method_key.clone(),
        }],
        1,
    )
    .await?;
    let association = associations
        .pop()
        .context("sealed policy method resolution returned no association")?;
    Ok(vec![
        crate::routing_catalog::resolve_candidate(decision::CandidateOperation {
            plugin_id: plugin_id.clone(),
            method_key: method_key.clone(),
            schema_hash: association.schema_hash,
        })
        .await?,
    ])
}

fn validate_policy_tag(
    rule: &registry::PolicyRule,
    tags: &[registry::TagDescriptor],
) -> Result<()> {
    if let registry::PolicyRule::TagConstraint { tag_id, .. } = rule {
        ensure!(
            tags.iter().any(|tag| tag.tag_id == *tag_id),
            "policy tag_id is not registered"
        );
    }
    Ok(())
}

fn page_cursor(cursor: Option<&str>, revision: u64) -> Result<(usize, u64)> {
    let Some(cursor) = cursor else {
        return Ok((0, revision));
    };
    let (revision_text, offset_text) = cursor.split_once(':').context("invalid routing cursor")?;
    let cursor_revision: u64 = revision_text
        .parse()
        .context("invalid routing cursor revision")?;
    let offset: usize = offset_text
        .parse()
        .context("invalid routing cursor offset")?;
    ensure!(
        format!("{cursor_revision}:{offset}") == cursor,
        "noncanonical routing cursor"
    );
    ensure!(cursor_revision == revision, "stale routing cursor revision");
    Ok((offset, cursor_revision))
}
