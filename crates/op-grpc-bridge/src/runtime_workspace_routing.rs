//! Owner-bound routing records over the retained RuntimeDocuments format.
//! Proposals never become credentials or network effects. Missing external
//! mapping/traffic evidence is rejected, as in the retained native contract.
use std::sync::Arc;
use anyhow::{Context, Result, ensure, bail};
use op_plugins::state_plugins::{routing_decision::build_shadow_record, routing_decision_methods as decision, schema_tag_registry_methods as registry, runtime_documents::{RuntimeDocument, RuntimeDocuments}};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard};
use crate::{catalog::Catalog, core::BackendStateUpdate, runtime::{BackendContext, BackendOutcome, RuntimeError}};

#[path = "routing_resources.rs"]
mod resources;
use resources::{RoutingLimits, RoutingReservation, RoutingResources};

pub(super) struct Routing { gate: Arc<Mutex<()>>, resources: Arc<RoutingResources> }
impl Routing {
    pub(super) fn new() -> Result<Self> { Ok(Self { gate: Arc::new(Mutex::new(())), resources: RoutingResources::new(RoutingLimits::default())? }) }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingOwner { principal_id: String, session_id: String, session_genesis: String, owner_id: String }
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryOwner { owner_id: String }
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "scope", rename_all = "snake_case", deny_unknown_fields)]
enum Owner { Decisions(RoutingOwner), Registry(RegistryOwner) }
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "state", rename_all = "snake_case", deny_unknown_fields)]
enum StoredState { Decisions(decision::DecisionState), Registry(registry::RegistryState) }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument { format: u32, owner: Owner, state: StoredState }

impl Owner {
    fn registry() -> Result<Self> {
        let owner_id = std::env::var("OP_ROUTING_POLICY_OWNER").context("OP_ROUTING_POLICY_OWNER must select stable policy owner")?;
        registry::validate_text(&owner_id, "stable policy owner", registry::MAX_ID_BYTES)?;
        Ok(Self::Registry(RegistryOwner { owner_id }))
    }
    fn decisions(ctx: &BackendContext<'_>) -> Result<Self> {
        let principal_id = ctx.caller.principal_id.clone();
        let session_id = ctx.caller.session_id.clone();
        let session_genesis = ctx.caller.session_genesis.clone();
        for (field, value) in [("principal", &principal_id), ("session", &session_id), ("genesis", &session_genesis)] { registry::validate_text(value, field, registry::MAX_ID_BYTES)?; }
        let owner_id = hex::encode(Sha256::digest(serde_json::to_vec(&("routing-owner-v1", &principal_id, &session_id, &session_genesis))?));
        Ok(Self::Decisions(RoutingOwner { principal_id, session_id, session_genesis, owner_id }))
    }
    fn id(&self) -> &str { match self { Self::Decisions(owner) => &owner.owner_id, Self::Registry(owner) => &owner.owner_id } }
    fn plugin(&self) -> &'static str { match self { Self::Decisions(_) => "routing_decision", Self::Registry(_) => "schema_tag_registry" } }
    fn selector(&self) -> Result<String> { Ok(hex::encode(Sha256::digest(serde_json::to_vec(&("routing-private-v1", self))?))) }
    fn key(&self) -> Result<String> { Ok(format!("__routing_private_v1:{}:{}", self.plugin(), self.selector()?)) }
    fn budget(&self) -> Result<String> {
        let bytes = match self { Self::Decisions(owner) => serde_json::to_vec(&("routing-principal-budget-v1", &owner.principal_id))?, Self::Registry(owner) => serde_json::to_vec(&("routing-registry-budget-v1", &owner.owner_id))? };
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    fn empty(&self) -> StoredState {
        match self { Self::Decisions(_) => StoredState::Decisions(decision::DecisionState { revision: 0, records: vec![] }), Self::Registry(_) => StoredState::Registry(registry::RegistryState { revision: 0, tags: vec![], policies: vec![], protected_methods: vec![] }) }
    }
}

impl StoredState {
    fn revision(&self) -> u64 { match self { Self::Decisions(state) => state.revision, Self::Registry(state) => state.revision } }
    fn advance(&mut self) -> Result<()> {
        let next = self.revision().checked_add(1).context("routing revision exhausted")?;
        match self { Self::Decisions(state) => state.revision = next, Self::Registry(state) => state.revision = next }
        Ok(())
    }
    fn validate(&self, owner: &Owner, limits: &RoutingLimits) -> Result<()> {
        match (self, owner) {
            (Self::Decisions(state), Owner::Decisions(owner)) => { state.validate_for_owner(&owner.owner_id)?; ensure!(state.records.len() <= limits.max_records, "routing record capacity"); }
            (Self::Registry(state), Owner::Registry(owner)) => { state.validate_for_owner(&owner.owner_id)?; ensure!(state.tags.len() <= limits.max_tags && state.policies.len() <= limits.max_policies && state.protected_methods.len() <= limits.max_markers, "routing registry capacity"); }
            _ => bail!("routing document owner/type mismatch"),
        }
        ensure!(serde_json::to_vec(self)?.len() <= limits.max_document_bytes, "routing document capacity");
        Ok(())
    }
    fn value(&self) -> Result<Value> { match self { Self::Decisions(state) => Ok(serde_json::to_value(state)?), Self::Registry(state) => Ok(serde_json::to_value(state)?) } }
}

fn decode(owner: &Owner, row: Option<RuntimeDocument>, limits: &RoutingLimits) -> Result<StoredState> {
    let Some(row) = row else { return Ok(owner.empty()) };
    ensure!(serde_json::to_vec(&row.document)?.len() <= limits.max_document_bytes, "routing document capacity");
    let document: StoredDocument = serde_json::from_value(row.document)?;
    ensure!(document.format == 1 && document.owner == *owner && document.state.revision() == row.revision, "routing durable owner/revision mismatch");
    document.state.validate(owner, limits)?;
    Ok(document.state)
}

pub(super) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "routing_decision" => matches!(method, "list_decisions" | "get_decision" | "record_shadow_decision"),
        "schema_tag_registry" => matches!(method, "list_tags" | "get_tag" | "get_protected_method_marker" | "list_policies" | "get_policy" | "register_tag_semantics" | "set_protected_method_marker" | "author_policy" | "update_policy" | "revoke_policy"),
        _ => false,
    }
}

struct CommitGuard { _operation: OwnedMutexGuard<()>, _reservation: RoutingReservation }

pub(super) async fn dispatch(ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let runtime = &ctx.runtime.workspace().routing;
    let owner = if plugin == "routing_decision" { Owner::decisions(ctx)? } else { Owner::registry()? };
    let limits = runtime.resources.limits().clone();
    super::require(serde_json::to_vec(args)?.len() <= limits.max_document_bytes, "routing input capacity")?;
    let mut reservation = runtime.resources.reserve(&owner.budget()?, limits.max_document_bytes * 6, runtime.resources.audit_work_bytes())?;
    let guard = tokio::time::timeout(std::time::Duration::from_millis(limits.queue_wait_ms), runtime.gate.clone().lock_owned()).await
        .map_err(|_| RuntimeError::backend(anyhow::anyhow!("routing queue wait exceeded")))?;
    reservation.begin_work()?;
    ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
    let documents = ctx.runtime.documents().clone();
    let row = documents.read_durable(&owner.key()?).await?;
    let state = decode(&owner, row, &limits)?;
    let (result, state) = if plugin == "routing_decision" {
        decision_call(ctx, method, args, &owner, state, documents, &limits).await?
    } else {
        registry_call(ctx, method, args, &owner, state, documents, &limits).await?
    };
    let mutation = matches!(ctx.declaration.side_effect, op_state_store::SideEffect::Mutation);
    let state = state.value().map_err(|error| super::domain_error(error, mutation))?;
    if let Err(error) = reservation.retain_document(&owner.selector()?, serde_json::to_vec(&state)?.len()) {
        return Err(super::domain_error(error, mutation).with_state(BackendStateUpdate::Replace(state), super::private_scope(ctx)));
    }
    let mut outcome = super::outcome(result, BackendStateUpdate::Replace(state), super::private_scope(ctx));
    outcome.guard = Some(Box::new(CommitGuard { _operation: guard, _reservation: reservation }));
    Ok(outcome)
}

fn resolve(catalog: &Catalog, plugin: &str, method: &str) -> Result<decision::ResolvedOperationRef> {
    let entry = catalog.plugin(plugin).context("routing target is not sealed")?;
    let schema = super::schema(catalog, plugin)?;
    let declaration = schema.methods.get(method).context("routing target method is not sealed")?;
    let value = decision::ResolvedOperationRef { plugin_id: plugin.into(), method_key: method.into(), transport_name: declaration.name.clone(), schema_hash: entry.schema_hash.clone(), schema_version: schema.version.clone(), catalog_hash: catalog.hash().into(), subid: declaration.subid.clone(), required_capability: declaration.required_capability.clone() };
    value.validate()?;
    Ok(value)
}

fn candidate(catalog: &Catalog, proposal: &decision::CandidateOperation) -> Result<decision::ResolvedOperationRef> {
    proposal.validate()?;
    let operation = resolve(catalog, &proposal.plugin_id, &proposal.method_key)?;
    ensure!(operation.schema_hash == proposal.schema_hash, "routing candidate schema hash is stale");
    Ok(operation)
}

async fn commit(ctx: &BackendContext<'_>, documents: Arc<RuntimeDocuments>, owner: Owner, mut state: StoredState, result: Value, limits: &RoutingLimits) -> Result<(Value, StoredState), RuntimeError> {
    let expected = state.revision();
    state.advance()?;
    state.validate(&owner, limits)?;
    let key = owner.key()?;
    let stored = serde_json::to_value(StoredDocument { format: 1, owner: owner.clone(), state: state.clone() })?;
    super::require(serde_json::to_vec(&stored)?.len() <= limits.max_document_bytes, "routing document capacity")?;
    let authority = ctx.caller.authority.clone();
    let handle = tokio::runtime::Handle::current();
    let (row, result) = documents.update_at_revision_with_result(&key, expected, move |_| {
        handle.block_on(authority.revalidate())?;
        Ok((stored, result))
    }).await.map_err(|error| super::domain_error(error, true))?;
    let committed = decode(&owner, Some(row), limits).map_err(RuntimeError::unknown)?;
    Ok((result, committed))
}

fn page<T: Clone>(values: &[T], cursor: Option<&str>, revision: u64, limit: u16) -> Result<(Vec<T>, Option<String>)> {
    ensure!((1..=100).contains(&limit), "invalid routing page size");
    let offset = if let Some(cursor) = cursor {
        let (r, n) = cursor.split_once(':').context("invalid routing cursor")?;
        let r: u64 = r.parse()?; let n: usize = n.parse()?;
        ensure!(r == revision && format!("{r}:{n}") == cursor, "stale or noncanonical routing cursor");
        n
    } else { 0 };
    ensure!(offset <= values.len(), "routing cursor out of bounds");
    let end = offset.saturating_add(usize::from(limit)).min(values.len());
    Ok((values[offset..end].to_vec(), (end < values.len()).then(|| format!("{revision}:{end}"))))
}

async fn registry_call(ctx: &BackendContext<'_>, method: &str, args: &Value, owner: &Owner, state: StoredState, documents: Arc<RuntimeDocuments>, limits: &RoutingLimits) -> Result<(Value, StoredState), RuntimeError> {
    let StoredState::Registry(mut state) = state else { return Err(RuntimeError::backend(anyhow::anyhow!("registry state required"))) };
    let next_revision = state.revision.checked_add(1).context("routing revision overflow")?;
    let mut mutate = false;
    let result = match method {
        "list_tags" => { let input: registry::ListTagsInput = serde_json::from_value(args.clone())?; input.validate()?; let (tags, next_cursor) = page(&state.tags, input.cursor.as_deref(), state.revision, input.limit)?; serde_json::to_value(registry::ListTagsOutput { tags, next_cursor })? }
        "get_tag" => { let input: registry::TagIdInput = serde_json::from_value(args.clone())?; input.validate()?; serde_json::to_value(registry::GetTagOutput { tag: state.tags.iter().find(|tag| tag.tag_id == input.tag_id).cloned() })? }
        "list_policies" => { let input: registry::ListPoliciesInput = serde_json::from_value(args.clone())?; input.validate()?; let (policies, next_cursor) = page(&state.policies, input.cursor.as_deref(), state.revision, input.limit)?; serde_json::to_value(registry::ListPoliciesOutput { policies, next_cursor })? }
        "get_policy" => { let input: registry::PolicyIdInput = serde_json::from_value(args.clone())?; input.validate()?; serde_json::to_value(registry::GetPolicyOutput { policy: state.policies.iter().find(|policy| policy.policy_id == input.policy_id).cloned() })? }
        "get_protected_method_marker" => {
            let input: registry::ProtectedMethodMarkerKeyInput = serde_json::from_value(args.clone())?;
            for (key, value) in [("plugin", &input.plugin_id), ("method", &input.method_key), ("schema hash", &input.schema_hash)] { registry::validate_text(value, key, registry::MAX_ID_BYTES)?; }
            serde_json::to_value(registry::GetProtectedMethodMarkerOutput { marker: state.protected_methods.iter().find(|marker| marker.plugin_id == input.plugin_id && marker.method_key == input.method_key && marker.schema_hash == input.schema_hash).cloned() })?
        }
        "register_tag_semantics" => {
            let input: registry::RegisterTagInput = serde_json::from_value(args.clone())?;
            super::require(input.openflow_binding.is_none(), "authoritative identity-to-traffic binding is not configured")?;
            super::require(input.associations.len() <= limits.max_evidence_fanout, "routing evidence capacity")?;
            let mut seen = std::collections::BTreeSet::new();
            let associations = input.associations.iter().map(|request| {
                request.validate()?; ensure!(seen.insert((&request.plugin_id, &request.method_key)), "duplicate schema-tag association");
                let operation = resolve(ctx.runtime.catalog(), &request.plugin_id, &request.method_key)?;
                Ok(registry::SchemaTagAssociation { plugin_id: operation.plugin_id, method_key: operation.method_key, schema_hash: operation.schema_hash, tag_id: input.tag_id })
            }).collect::<Result<Vec<_>>>()?;
            let tag = registry::apply_tag_registration(&mut state.tags, input, associations, None)?;
            mutate = true; serde_json::to_value(registry::RegisterTagOutput { tag, registry_revision: next_revision })?
        }
        "set_protected_method_marker" => {
            let input: registry::SetProtectedMethodMarkerInput = serde_json::from_value(args.clone())?; input.marker.validate()?;
            let operation = candidate(ctx.runtime.catalog(), &decision::CandidateOperation { plugin_id: input.marker.plugin_id.clone(), method_key: input.marker.method_key.clone(), schema_hash: input.marker.schema_hash.clone() })?;
            let marker = registry::apply_protected_method_marker(&mut state.protected_methods, input, &operation.plugin_id, &operation.method_key, &operation.schema_hash)?;
            mutate = true; serde_json::to_value(registry::ProtectedMethodMarkerMutationOutput { marker })?
        }
        "author_policy" => {
            let input: registry::AuthorPolicyInput = serde_json::from_value(args.clone())?;
            policy_evidence(ctx.runtime.catalog(), &input.rule, &input.canonical_mappings, &state.tags)?;
            let policy = registry::author_policy(&mut state.policies, owner.id(), input, vec![])?;
            mutate = true; serde_json::to_value(registry::PolicyMutationOutput { policy, rule_revision: next_revision })?
        }
        "update_policy" => {
            let input: registry::UpdatePolicyInput = serde_json::from_value(args.clone())?;
            policy_evidence(ctx.runtime.catalog(), &input.rule, &input.canonical_mappings, &state.tags)?;
            let policy = registry::apply_policy_update(&mut state.policies, owner.id(), input, vec![])?;
            mutate = true; serde_json::to_value(registry::PolicyMutationOutput { policy, rule_revision: next_revision })?
        }
        "revoke_policy" => {
            let input: registry::RevokePolicyInput = serde_json::from_value(args.clone())?;
            let policy = registry::revoke_policy(&mut state.policies, owner.id(), &input.policy_id, input.expected_revision, &input.reason)?;
            mutate = true; serde_json::to_value(registry::PolicyMutationOutput { policy, rule_revision: next_revision })?
        }
        _ => return Err(RuntimeError::unsupported(format!("unknown registry method {method}"))),
    };
    let state = StoredState::Registry(state);
    if mutate { commit(ctx, documents, owner.clone(), state, result, limits).await } else { Ok((result, state)) }
}

fn policy_evidence(catalog: &Catalog, rule: &registry::PolicyRule, mappings: &[registry::CanonicalMappingRequest], tags: &[registry::TagDescriptor]) -> Result<()> {
    ensure!(mappings.is_empty(), "canonical OSCAL/Qdrant mapping resolver is unavailable; caller-authored provenance rejected");
    rule.validate()?;
    match rule {
        registry::PolicyRule::EgressRule { .. } => bail!("egress policy requires canonical mapping resolution and content-bound egress evidence"),
        registry::PolicyRule::DenyMethod { plugin_id, method_key, .. } => { resolve(catalog, plugin_id, method_key)?; }
        registry::PolicyRule::TagConstraint { tag_id, .. } => ensure!(tags.iter().any(|tag| tag.tag_id == *tag_id), "policy tag is not registered"),
    }
    Ok(())
}

async fn decision_call(ctx: &BackendContext<'_>, method: &str, args: &Value, owner: &Owner, state: StoredState, documents: Arc<RuntimeDocuments>, limits: &RoutingLimits) -> Result<(Value, StoredState), RuntimeError> {
    let StoredState::Decisions(mut state) = state else { return Err(RuntimeError::backend(anyhow::anyhow!("decision state required"))) };
    let result = match method {
        "list_decisions" => {
            let input: decision::ListDecisionsInput = serde_json::from_value(args.clone())?; input.validate()?;
            let (decisions, next_cursor) = page(&state.records, input.cursor.as_deref(), state.revision, input.limit)?;
            return Ok((serde_json::to_value(decision::ListDecisionsOutput { decisions, next_cursor })?, StoredState::Decisions(state)));
        }
        "get_decision" => {
            let input: decision::DecisionIdInput = serde_json::from_value(args.clone())?; input.validate()?;
            let decision = state.records.iter().find(|record| record.decision_id == input.decision_id).cloned();
            return Ok((serde_json::to_value(decision::GetDecisionOutput { decision })?, StoredState::Decisions(state)));
        }
        "record_shadow_decision" => {
            let input: decision::RecordDecisionInput = serde_json::from_value(args.clone())?; input.validate()?;
            let operation = candidate(ctx.runtime.catalog(), &input.candidate)?;
            ctx.runtime.authorize_nested(ctx.caller, &operation.plugin_id, &operation.method_key).await?;
            let registry_owner = Owner::registry()?;
            let registry = decode(&registry_owner, documents.read_durable(&registry_owner.key()?).await?, limits)?;
            let StoredState::Registry(registry) = registry else { return Err(RuntimeError::backend(anyhow::anyhow!("policy state required"))) };
            let ui = ctx.runtime.core().committed_snapshot(&super::private_scope(ctx), "gemma_brain", &op_plugins::canonical::plugin_path("gemma_brain"))?;
            let current_taxonomy_revision = ui.as_ref().and_then(|snapshot| snapshot.state.get("taxonomy_revision")).and_then(Value::as_u64);
            let caller = decision::VerifiedCaller { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone(), owner_id: owner.id().into(), container: None, project_scope: None, capabilities: ctx.runtime.current_grants(ctx.caller).await?.into_iter().collect() };
            let resolved_tag = input.classification_proposal.as_ref().and_then(|proposal| proposal.tag.as_ref()).and_then(|name| registry.tags.iter().find(|tag| &tag.name == name)).map(|tag| registry::ResolvedTagRef { tag_id: tag.tag_id, revision: tag.revision, name: tag.name.clone() });
            let associations = registry.tags.iter().flat_map(|tag| tag.associations.iter()).filter(|association| association.plugin_id == operation.plugin_id && association.method_key == operation.method_key && association.schema_hash == operation.schema_hash).cloned().collect();
            let marker = registry.protected_methods.iter().find(|marker| marker.plugin_id == operation.plugin_id && marker.method_key == operation.method_key).cloned();
            let gate = decision::GateInput { caller: caller.clone(), policy_owner_id: registry_owner.id().into(), operation: operation.clone(), classification: input.classification_proposal.clone(), current_taxonomy_revision, resolved_tag, tag_associations: associations, policy_snapshot: Some(registry::PolicySnapshot { owner_id: registry_owner.id().into(), rule_revision: registry.revision, marker, records: registry.policies }), policy_read_succeeded: true, marker_lookup_succeeded: true, requested_effect: decision::RequestedEffect::Local, residual_validation_passed: false, destination_class: None, provider_route: None };
            let record = build_shadow_record(uuid::Uuid::now_v7().to_string(), input, caller, operation, gate, None)?;
            state.records.push(record.clone());
            serde_json::to_value(decision::RecordDecisionOutput { decision: record })?
        }
        _ => return Err(RuntimeError::unsupported(format!("unknown decision method {method}"))),
    };
    commit(ctx, documents, owner.clone(), StoredState::Decisions(state), result, limits).await
}
