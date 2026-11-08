//! Production RPC helpers. Fixture services never enter these paths.
use super::*;
use crate::core::{CommittedStateSnapshot, StateScope};
use serde_json::{Value, json};
use std::collections::HashMap;

impl BridgeService {
    async fn selected_snapshot(&self, runtime: &Runtime, caller: &VerifiedCaller, plugin: &str, path: &str) -> Result<Option<CommittedStateSnapshot>, Status> {
        if !runtime.can_read_plugin(caller, plugin).await.map_err(runtime_status)? {
            return Err(Status::permission_denied("current sealed read capability is required"));
        }
        // Identity metadata has a dedicated paired owner; its private credential
        // registry must never be read through the generic private projection.
        let private = if plugin == "identity_sled" { None } else {
            self.core.committed_snapshot(&caller.private_scope(), plugin, path)
                .map_err(|_| Status::unavailable("private committed state is unavailable"))?
        };
        let snapshot = match private {
            Some(snapshot) => Some(snapshot),
            None => self.core.committed_snapshot(&StateScope::Public, plugin, path)
                .map_err(|_| Status::unavailable("public committed state is unavailable"))?,
        };
        if !runtime.can_read_plugin(caller, plugin).await.map_err(runtime_status)? {
            return Err(Status::permission_denied("read capability changed"));
        }
        Ok(snapshot)
    }

    pub(super) async fn production_get_state(&self, runtime: &Runtime, request: Request<GetStateRequest>) -> Result<Response<GetStateResponse>, Status> {
        let caller = self.authenticate(&request).await?;
        let req = request.into_inner();
        let snapshot = self.selected_snapshot(runtime, &caller, &req.plugin_id, &req.object_path).await?
            .ok_or_else(|| Status::not_found("committed state is absent"))?;
        Ok(Response::new(GetStateResponse {
            state: snapshot.state.as_object().map(json_map_to_proto_struct),
            value: Some(json_to_proto_value(&snapshot.state)),
            effective_hash: snapshot.effective_hash, at_event_id: snapshot.event_id,
        }))
    }

    pub(super) async fn production_get_property(&self, runtime: &Runtime, request: Request<GetPropertyRequest>) -> Result<Response<GetPropertyResponse>, Status> {
        let caller = self.authenticate(&request).await?;
        let req = request.into_inner();
        let snapshot = self.selected_snapshot(runtime, &caller, &req.plugin_id, &req.object_path).await?
            .ok_or_else(|| Status::not_found("committed object is absent"))?;
        let value = snapshot.state.get(&req.property_name).ok_or_else(|| Status::not_found("committed property is absent"))?;
        let entry = runtime.catalog().plugin(&req.plugin_id).ok_or_else(|| Status::not_found("plugin absent"))?;
        let schema: op_state_store::PluginSchema = serde_json::from_value(entry.schema.clone()
            .ok_or_else(|| Status::failed_precondition("plugin schema absent"))?)
            .map_err(|_| Status::failed_precondition("invalid plugin schema"))?;
        Ok(Response::new(GetPropertyResponse { value: Some(json_to_proto_value(value)), read_only: !schema.field_writes.contains_key(&req.property_name) }))
    }

    fn native_mutation(&self, req: &MutateRequest) -> Result<(String, Value, uuid::Uuid), Status> {
        let entry = self.core.catalog().plugin(&req.plugin_id).ok_or_else(|| Status::not_found("plugin absent"))?;
        let value = req.value.as_ref().map(proto_value_to_json).unwrap_or_else(|| json!({}));
        let (method, args) = match OperationType::try_from(req.operation) {
            Ok(OperationType::CallMethod) => (req.member_name.clone(), value),
            Ok(OperationType::SetProperty) => {
                if !req.object_path.is_empty() && req.object_path != entry.dbus_path {
                    return Err(Status::failed_precondition("property writes require an explicit native root-field binding"));
                }
                let schema: op_state_store::PluginSchema = serde_json::from_value(entry.schema.clone()
                    .ok_or_else(|| Status::failed_precondition("plugin schema absent"))?)
                    .map_err(|_| Status::failed_precondition("invalid plugin schema"))?;
                let binding = schema.field_writes.get(&req.member_name)
                    .ok_or_else(|| Status::permission_denied("property has no sealed native write binding"))?;
                (binding.method.clone(), json!({binding.argument.clone(): value}))
            }
            _ => return Err(Status::failed_precondition("raw state patches cannot bypass a native plugin method")),
        };
        self.check_method_target(&req.plugin_id, &method, &req.object_path, &req.capability_id)?;
        let operation_id = if req.idempotency_key.is_empty() { uuid::Uuid::new_v4() } else {
            let id = uuid::Uuid::parse_str(&req.idempotency_key).map_err(|_| Status::invalid_argument("idempotency key must be a canonical operation UUID"))?;
            if id.to_string() != req.idempotency_key { return Err(Status::invalid_argument("noncanonical operation UUID")); }
            id
        };
        Ok((method, args, operation_id))
    }

    async fn execute_mutation(&self, runtime: &Runtime, caller: &VerifiedCaller, req: &MutateRequest) -> Result<MutateResponse, Status> {
        let (method, args, operation_id) = self.native_mutation(req)?;
        let result = runtime.call(caller.clone(), &req.plugin_id, &method, args, operation_id).await.map_err(runtime_status)?;
        runtime.revalidate_result(&result, &req.plugin_id, &method).await.map_err(runtime_status)?;
        Ok(MutateResponse { success: true, event_id: result.receipt.terminal_event_id,
            event_hash: result.receipt.terminal_event_hash, result: Some(json_to_proto_value(&result.result)), error: None,
            effective_hash: result.effective_hash })
    }

    pub(super) async fn production_mutate(&self, runtime: &Runtime, request: Request<MutateRequest>) -> Result<Response<MutateResponse>, Status> {
        let caller = self.authenticate(&request).await?;
        let capability = self.requested_capability(&request)?;
        let mut req = request.into_inner();
        if !req.capability_id.is_empty() && req.capability_id != capability { return Err(Status::permission_denied("conflicting capability metadata")); }
        req.capability_id = capability;
        Ok(Response::new(self.execute_mutation(runtime, &caller, &req).await?))
    }

    pub(super) async fn production_set_property(&self, runtime: &Runtime, request: Request<SetPropertyRequest>) -> Result<Response<SetPropertyResponse>, Status> {
        let caller = self.authenticate(&request).await?;
        let capability = self.requested_capability(&request)?;
        let req = request.into_inner();
        if !req.capability_id.is_empty() && req.capability_id != capability { return Err(Status::permission_denied("conflicting capability metadata")); }
        let result = self.execute_mutation(runtime, &caller, &MutateRequest { plugin_id: req.plugin_id,
            object_path: req.object_path, operation: OperationType::SetProperty as i32,
            member_name: req.property_name, value: req.value, actor_id: caller.principal_id.clone(),
            capability_id: capability, idempotency_key: String::new() }).await?;
        Ok(Response::new(SetPropertyResponse { success: result.success, event_id: result.event_id,
            event_hash: result.event_hash, error: result.error }))
    }

    pub(super) async fn production_batch_mutate(&self, runtime: &Runtime, request: Request<BatchMutateRequest>) -> Result<Response<BatchMutateResponse>, Status> {
        let caller = self.authenticate(&request).await?;
        let req = request.into_inner();
        if req.atomic && req.mutations.len() > 1 {
            return Err(Status::failed_precondition("independent native owners cannot promise an atomic batch; use a declared transactional method"));
        }
        if req.mutations.len() > 128 { return Err(Status::resource_exhausted("native batch exceeds its operation bound")); }
        // Validate every shape before the first native effect. Authorization is
        // still rechecked separately at every actual effect boundary.
        for mutation in &req.mutations { self.native_mutation(mutation)?; }
        let mut results = Vec::with_capacity(req.mutations.len());
        let mut failed_index = -1;
        for (index, mutation) in req.mutations.iter().enumerate() {
            match self.execute_mutation(runtime, &caller, mutation).await {
                Ok(result) => results.push(result),
                Err(error) => {
                    if failed_index == -1 { failed_index = index as i32; }
                    results.push(MutateResponse { success: false, event_id: 0, event_hash: String::new(),
                        result: None, error: Some(mutation_error(ErrorCode::Internal, error.message().into())), effective_hash: String::new() });
                    // Do not run further native effects after uncertainty or an
                    // identity failure; caller can reconcile each existing id.
                    break;
                }
            }
        }
        Ok(Response::new(BatchMutateResponse { success: failed_index == -1, results, failed_index }))
    }

    pub(super) async fn production_subscribe(&self, runtime: Arc<Runtime>, request: Request<SubscribeRequest>) -> Result<Response<ReceiverStream<Result<StateChange, Status>>>, Status> {
        let caller = self.authenticate(&request).await?;
        let req = request.into_inner();
        let mut notices = self.core.subscribe_commits();
        let mut initial = Vec::new();
        if req.include_initial_state {
            let (public, _) = self.core.committed_subscription(&StateScope::Public, true)
                .map_err(|_| Status::unavailable("public state hydration unavailable"))?;
            let (private, _) = self.core.committed_subscription(&caller.private_scope(), true)
                .map_err(|_| Status::unavailable("private state hydration unavailable"))?;
            let private_plugins: std::collections::HashSet<_> = private.iter().map(|frame| frame.plugin_id.clone()).collect();
            initial.extend(public.into_iter().filter(|frame| !private_plugins.contains(&frame.plugin_id)));
            initial.extend(private);
        }
        if req.include_schema {
            for plugin in runtime.catalog().plugins() {
                if !runtime.can_read_plugin(&caller, &plugin.id).await.map_err(runtime_status)? { continue; }
                initial.push(StateChange { plugin_id: plugin.id.clone(), object_path: plugin.dbus_path.clone(),
                    change_type: ChangeType::SchemaMigration as i32, member_name: plugin.schema_hash.clone(),
                    new_value: plugin.schema.as_ref().map(json_to_proto_value), schema_hash: plugin.schema_hash.clone(),
                    catalog_hash: runtime.catalog().hash().to_string(), ..Default::default() });
            }
        }
        let (tx, rx) = mpsc::channel(128);
        let core = self.core.clone();
        tokio::spawn(async move {
            let mut generations: HashMap<(String, StateScope), u64> = HashMap::new();
            for frame in initial {
                if !state_matches(&req, &frame) { continue; }
                let Ok(permit) = tx.reserve().await else { return; };
                match runtime.can_read_plugin(&caller, &frame.plugin_id).await {
                    Ok(true) => { permit.send(Ok(frame)); },
                    Ok(false) => {},
                    Err(error) => { permit.send(Err(runtime_status(error))); return; }
                }
            }
            loop {
                let notice = tokio::select! { _ = tx.closed() => return, notice = notices.recv() => notice };
                let notice = match notice {
                    Ok(notice) => notice,
                    Err(broadcast::error::RecvError::Lagged(_)) => { let _ = tx.send(Err(Status::data_loss("committed state stream lagged; hydrate again"))).await; return; },
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if !notice.state_committed || (notice.scope != StateScope::Public && notice.scope != caller.private_scope()) { continue; }
                // Wait for capacity before the disclosure check, not after it.
                let Ok(permit) = tx.reserve().await else { return; };
                match runtime.can_read_plugin(&caller, &notice.plugin_id).await {
                    Ok(true) => {}, Ok(false) => continue,
                    Err(error) => { permit.send(Err(runtime_status(error))); return; }
                }
                if notice.scope == StateScope::Public && notice.plugin_id != "identity_sled" {
                    let root = format!("/org/opdbus/v1/plugins/{}", notice.plugin_id);
                    match core.committed_snapshot(&caller.private_scope(), &notice.plugin_id, &root) {
                        Ok(Some(_)) => continue, Ok(None) => {},
                        Err(_) => { permit.send(Err(Status::unavailable("private state selection unavailable"))); return; }
                    }
                }
                match core.committed_frame(&notice.scope, &notice) {
                    Ok(Some(frame)) => {
                        // One terminal can publish several siblings. Deduplicate by
                        // plugin/scope generation, never by a global event cursor.
                        let key = (frame.plugin_id.clone(), notice.scope.clone());
                        if generations.get(&key).is_some_and(|old| *old >= frame.state_generation) { continue; }
                        generations.insert(key, frame.state_generation);
                        if state_matches(&req, &frame) { permit.send(Ok(frame)); }
                    }
                    Ok(None) => {},
                    Err(_) => { permit.send(Err(Status::unavailable("committed state observation unavailable"))); return; }
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

impl BridgeService {
    async fn audit_caller<T: Sync>(&self, runtime: &Runtime, request: &Request<T>, method: &str) -> Result<VerifiedCaller, Status> {
        let caller = self.authenticate(request).await?;
        let capability = self.requested_capability(request)?;
        self.check_method_target("snowball", method, "", &capability)?;
        runtime.authorize_nested(&caller, "snowball", method).await.map_err(runtime_status)?;
        Ok(caller)
    }

    pub(super) async fn production_get_events(&self, runtime: &Runtime, request: Request<GetEventsRequest>) -> Result<Response<GetEventsResponse>, Status> {
        let caller = self.audit_caller(runtime, &request, "query_events").await?;
        let req = request.into_inner();
        let limit = if req.limit == 0 { 100 } else { req.limit.min(4096) } as usize;
        let events = self.core.journal_events(&caller.private_scope(), req.from_event_id, req.to_event_id)
            .map_err(|_| Status::unavailable("durable audit ledger unavailable"))?;
        let mut selected = events.iter()
            .filter(|event| req.plugin_id.is_empty() || event.plugin_id == req.plugin_id)
            .filter(|event| req.tags.is_empty() || event.tags_touched.iter().any(|tag| req.tags.contains(tag)))
            .filter(|event| req.decision_filter == Decision::Unspecified as i32 || event.decision == req.decision_filter)
            .take(limit + 1).map(ChainEvent::from).collect::<Vec<_>>();
        let has_more = selected.len() > limit;
        selected.truncate(limit);
        runtime.authorize_nested(&caller, "snowball", "query_events").await.map_err(runtime_status)?;
        Ok(Response::new(GetEventsResponse { events: selected, has_more }))
    }

    pub(super) async fn production_verify_chain(&self, runtime: &Runtime, request: Request<VerifyChainRequest>) -> Result<Response<VerifyChainResponse>, Status> {
        let caller = self.audit_caller(runtime, &request, "verify_chain").await?;
        let req = request.into_inner();
        let checked = self.core.verify_journal(req.from_event_id, req.to_event_id)
            .map_err(|_| Status::unavailable("durable chain verification unavailable"))?;
        runtime.authorize_nested(&caller, "snowball", "verify_chain").await.map_err(runtime_status)?;
        Ok(Response::new(VerifyChainResponse { valid: checked.valid, events_verified: checked.events_verified,
            batches_verified: 0, errors: checked.errors }))
    }

    pub(super) async fn production_subscribe_events(&self, runtime: Arc<Runtime>, request: Request<SubscribeEventsRequest>) -> Result<Response<ReceiverStream<Result<ChainEvent, Status>>>, Status> {
        let caller = self.audit_caller(&runtime, &request, "query_events").await?;
        let req = request.into_inner();
        let scope = caller.private_scope();
        let mut notices = self.core.subscribe_commits();
        let history = self.core.journal_events(&scope, req.from_event_id, 0)
            .map_err(|_| Status::unavailable("durable audit hydration unavailable"))?;
        let mut next = history.last().map(|event| event.event_id.saturating_add(1)).unwrap_or(req.from_event_id.max(1));
        let core = self.core.clone();
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            if req.from_event_id != 0 {
                for event in history {
                    let event = ChainEvent::from(&event);
                    if !chain_matches(&req, &event) { continue; }
                    let Ok(permit) = tx.reserve().await else { return; };
                    if let Err(error) = runtime.authorize_nested(&caller, "snowball", "query_events").await {
                        permit.send(Err(runtime_status(error))); return;
                    }
                    permit.send(Ok(event));
                }
            }
            loop {
                let notice = tokio::select! { _ = tx.closed() => return, notice = notices.recv() => notice };
                match notice {
                    Ok(notice) if notice.scope == scope => {},
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(broadcast::error::RecvError::Closed) => return,
                }
                let events = match core.journal_events(&scope, next, 0) {
                    Ok(events) => events,
                    Err(_) => { let _ = tx.send(Err(Status::unavailable("durable audit resume unavailable"))).await; return; },
                };
                for event in events {
                    next = event.event_id.saturating_add(1);
                    let event = ChainEvent::from(&event);
                    if !chain_matches(&req, &event) { continue; }
                    let Ok(permit) = tx.reserve().await else { return; };
                    if let Err(error) = runtime.authorize_nested(&caller, "snowball", "query_events").await {
                        permit.send(Err(runtime_status(error))); return;
                    }
                    permit.send(Ok(event));
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
