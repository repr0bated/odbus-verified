//! Thin tonic translation of the three core proto services onto [`BridgeCore`].
//!
//! `EventChainService` proof/snapshot RPCs are honestly `UNIMPLEMENTED`;
//! everything else is real. `OvsdbMirror`, `RuntimeMirror` and
//! `DbusPassthrough` are generated from the same file but not served: the
//! fresh bridge owns state, plugins and audit only.

use crate::core::{
    json_map_to_proto_struct, json_to_proto_value, proto_value_to_json, BridgeCore, CoreMutation,
    CoreOperation, CoreRejection, JournalEvent, MutationOutcome,
};
use crate::proto::event_chain_service_server::EventChainService;
use crate::proto::plugin_service_server::PluginService;
use crate::proto::state_sync_server::StateSync;
use crate::proto::{
    BatchMutateRequest, BatchMutateResponse, CallMethodRequest, CallMethodResponse, ChainEvent,
    ChangeType, CreateSnapshotRequest, CreateSnapshotResponse, Decision, ErrorCode,
    GetEventsRequest, GetEventsResponse, GetProofRequest, GetProofResponse, GetPropertyRequest,
    GetPropertyResponse, GetSchemaRequest, GetSchemaResponse, GetSnapshotRequest,
    GetSnapshotResponse, GetStateRequest, GetStateResponse, ListPluginsResponse, MutateRequest,
    MutateResponse, MutationError, OperationType, PluginInfo, ProveTagImmutabilityRequest,
    ProveTagImmutabilityResponse, SearchSemanticTraceRequest, SearchSemanticTraceResponse,
    SemanticTraceMatch, SetPropertyRequest, SetPropertyResponse, Signal, StateChange,
    SubscribeEventsRequest, SubscribeRequest, SubscribeSignalsRequest, VerifyChainRequest,
    VerifyChainResponse,
};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// The three core services, backed by one [`BridgeCore`].
#[derive(Clone)]
pub struct BridgeService {
    core: BridgeCore,
}

impl BridgeService {
    /// Serve `core` over tonic.
    pub fn new(core: BridgeCore) -> Self {
        Self { core }
    }
}

fn mutation_error(code: ErrorCode, message: String) -> MutationError {
    MutationError {
        code: code as i32,
        message,
        deny_reason: None,
    }
}

/// Translate one wire mutation. Infallible: anything malformed becomes a
/// [`CoreOperation`] the core rejects at admission, so batch alignment
/// never depends on translation order.
fn core_mutation(req: &MutateRequest, actor_default: &str) -> CoreMutation {
    let operation = if req.operation == OperationType::SetProperty as i32 {
        CoreOperation::SetProperty
    } else if req.operation == OperationType::CallMethod as i32 {
        CoreOperation::CallMethod
    } else if req.operation == OperationType::ApplyPatch as i32 {
        CoreOperation::ApplyPatch
    } else {
        CoreOperation::Unknown
    };
    CoreMutation {
        plugin_id: req.plugin_id.clone(),
        object_path: req.object_path.clone(),
        operation,
        member_name: req.member_name.clone(),
        value: req
            .value
            .as_ref()
            .map(proto_value_to_json)
            .unwrap_or(serde_json::Value::Null),
        actor_id: if req.actor_id.is_empty() {
            actor_default.to_string()
        } else {
            req.actor_id.clone()
        },
        capability_id: req.capability_id.clone(),
        idempotency_key: req.idempotency_key.clone(),
    }
}

fn mutate_response(result: Result<MutationOutcome, CoreRejection>) -> MutateResponse {
    match result {
        Ok(outcome) => MutateResponse {
            success: true,
            event_id: outcome.event.event_id,
            event_hash: outcome.event.event_hash,
            result: outcome.result.as_ref().map(json_to_proto_value),
            error: None,
            effective_hash: outcome.event.result_effective_hash,
        },
        Err(rejection) => MutateResponse {
            success: false,
            event_id: 0,
            event_hash: String::new(),
            result: None,
            error: Some(mutation_error(rejection.code, rejection.message)),
            effective_hash: String::new(),
        },
    }
}

/// Tiny glob over bytes: `*` matches any run, `?` matches one byte.
fn glob_match(pattern: &str, text: &str) -> bool {
    fn walk(pattern: &[u8], text: &[u8]) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        match pattern[0] {
            b'*' => (0..=text.len()).any(|split| walk(&pattern[1..], &text[split..])),
            b'?' => !text.is_empty() && walk(&pattern[1..], &text[1..]),
            byte => !text.is_empty() && text[0] == byte && walk(&pattern[1..], &text[1..]),
        }
    }
    walk(pattern.as_bytes(), text.as_bytes())
}

fn state_matches(req: &SubscribeRequest, frame: &StateChange) -> bool {
    (req.plugin_ids.is_empty() || req.plugin_ids.contains(&frame.plugin_id))
        && (req.path_patterns.is_empty()
            || req
                .path_patterns
                .iter()
                .any(|pattern| glob_match(pattern, &frame.object_path)))
        && (req.tags.is_empty()
            || frame
                .tags_touched
                .iter()
                .any(|tag| req.tags.contains(tag)))
}

fn chain_matches(req: &SubscribeEventsRequest, event: &ChainEvent) -> bool {
    (req.plugin_id.is_empty() || event.plugin_id == req.plugin_id)
        && (req.tags.is_empty()
            || event
                .tags_touched
                .iter()
                .any(|tag| req.tags.contains(tag)))
}

/// Event summary carried in each trace-match payload.
fn event_payload(event: &JournalEvent) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([
        ("event_id".to_string(), serde_json::json!(event.event_id)),
        (
            "event_hash".to_string(),
            serde_json::json!(event.event_hash),
        ),
        ("plugin_id".to_string(), serde_json::json!(event.plugin_id)),
        (
            "operation_type".to_string(),
            serde_json::json!(event.operation_type),
        ),
        ("target".to_string(), serde_json::json!(event.target)),
        ("actor_id".to_string(), serde_json::json!(event.actor_id)),
    ])
}

// ---------------------------------------------------------------------------
// StateSync
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl StateSync for BridgeService {
    type SubscribeStream = ReceiverStream<Result<StateChange, Status>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        // Register before returning the RPC, and snapshot under the same
        // commit boundary. No updates can fall between hydration and live.
        let (initial, schemas, mut sub) = self.core.state_subscription(
            req.include_initial_state,
            req.include_schema,
        );
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            for frame in initial {
                if state_matches(&req, &frame) && tx.send(Ok(frame)).await.is_err() {
                    return;
                }
            }
            for frame in schemas {
                // Contracts carry no path or tags; the plugin filter still applies.
                if !req.plugin_ids.is_empty() && !req.plugin_ids.contains(&frame.plugin_id) {
                    continue;
                }
                if tx.send(Ok(frame)).await.is_err() {
                    return;
                }
            }
            loop {
                let received = tokio::select! {
                    _ = tx.closed() => return,
                    received = sub.recv() => received,
                };
                match received {
                    Ok(frame) => {
                        if state_matches(&req, &frame) && tx.send(Ok(frame)).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let _ = tx.send(Err(Status::data_loss(format!(
                            "state stream missed {skipped} updates; resubscribe with include_initial_state",
                        )))).await;
                        return;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn mutate(
        &self,
        request: Request<MutateRequest>,
    ) -> Result<Response<MutateResponse>, Status> {
        let req = request.into_inner();
        let op = core_mutation(&req, "");
        Ok(Response::new(mutate_response(self.core.mutate(op))))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let req = request.into_inner();
        match self.core.get_object_snapshot(&req.plugin_id, &req.object_path) {
            Some((map, hash, event_id)) => {
                let value = serde_json::Value::Object(map.clone());
                Ok(Response::new(GetStateResponse {
                    state: Some(json_map_to_proto_struct(&map)),
                    effective_hash: hash,
                    at_event_id: event_id,
                    value: Some(json_to_proto_value(&value)),
                }))
            }
            None => Err(Status::not_found(format!(
                "no state for '{}:{}'",
                req.plugin_id, req.object_path
            ))),
        }
    }

    async fn batch_mutate(
        &self,
        request: Request<BatchMutateRequest>,
    ) -> Result<Response<BatchMutateResponse>, Status> {
        let req = request.into_inner();
        let ops: Vec<CoreMutation> = req
            .mutations
            .iter()
            .map(|m| core_mutation(m, &req.actor_id))
            .collect();
        let outcome = self.core.batch_mutate(ops, req.atomic);
        let results = outcome.results.into_iter().map(mutate_response).collect();
        Ok(Response::new(BatchMutateResponse {
            success: outcome.failed_index.is_none(),
            results,
            failed_index: outcome.failed_index.map(|i| i as i32).unwrap_or(-1),
        }))
    }
}

// ---------------------------------------------------------------------------
// PluginService
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl PluginService for BridgeService {
    async fn list_plugins(
        &self,
        _request: Request<()>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        let plugins = self
            .core
            .catalog()
            .plugins()
            .iter()
            .map(|p| PluginInfo {
                id: p.id.clone(),
                name: p.name.clone(),
                version: p.version.clone(),
                description: p.description.clone(),
                dbus_path: p.dbus_path.clone(),
                interfaces: p.interfaces.clone(),
                tags: p.tags.clone(),
            })
            .collect();
        Ok(Response::new(ListPluginsResponse { plugins }))
    }

    async fn get_schema(
        &self,
        request: Request<GetSchemaRequest>,
    ) -> Result<Response<GetSchemaResponse>, Status> {
        let req = request.into_inner();
        let plugin = self.core.catalog().plugin(&req.plugin_id).ok_or_else(|| {
            Status::not_found(format!("unknown plugin '{}'", req.plugin_id))
        })?;
        let schema_json = match &plugin.schema {
            Some(value) => serde_json::to_string(value).unwrap_or_default(),
            None => String::new(),
        };
        let dialect = if req.format.is_empty() {
            "json-schema".to_string()
        } else {
            req.format.clone()
        };
        Ok(Response::new(GetSchemaResponse {
            schema_json,
            dialect,
            version: plugin.version.clone(),
        }))
    }

    async fn call_method(
        &self,
        request: Request<CallMethodRequest>,
    ) -> Result<Response<CallMethodResponse>, Status> {
        let req = request.into_inner();
        // Routes key on (plugin, method); interface_name is informational.
        let args: Vec<serde_json::Value> =
            req.arguments.iter().map(proto_value_to_json).collect();
        let op = CoreMutation {
            plugin_id: req.plugin_id.clone(),
            object_path: req.object_path.clone(),
            operation: CoreOperation::CallMethod,
            member_name: req.method_name.clone(),
            value: serde_json::Value::Array(args),
            actor_id: req.actor_id.clone(),
            capability_id: req.capability_id.clone(),
            idempotency_key: String::new(),
        };
        match self.core.mutate(op) {
            Ok(outcome) => Ok(Response::new(CallMethodResponse {
                success: true,
                result: outcome.result.as_ref().map(json_to_proto_value),
                event_id: outcome.event.event_id,
                event_hash: outcome.event.event_hash,
                error: None,
            })),
            Err(rejection) => Ok(Response::new(CallMethodResponse {
                success: false,
                result: None,
                event_id: 0,
                event_hash: String::new(),
                error: Some(mutation_error(rejection.code, rejection.message)),
            })),
        }
    }

    async fn get_property(
        &self,
        request: Request<GetPropertyRequest>,
    ) -> Result<Response<GetPropertyResponse>, Status> {
        let req = request.into_inner();
        if self.core.catalog().plugin(&req.plugin_id).is_none() {
            return Err(Status::not_found(format!(
                "unknown plugin '{}'",
                req.plugin_id
            )));
        }
        match self.core.get_property(
            &req.plugin_id,
            &req.object_path,
            &req.property_name,
        ) {
            Some(value) => Ok(Response::new(GetPropertyResponse {
                value: Some(json_to_proto_value(&value)),
                read_only: false,
            })),
            None => Err(Status::not_found(format!(
                "no property '{}' on '{}:{}'",
                req.property_name, req.plugin_id, req.object_path
            ))),
        }
    }

    async fn set_property(
        &self,
        request: Request<SetPropertyRequest>,
    ) -> Result<Response<SetPropertyResponse>, Status> {
        let req = request.into_inner();
        let op = CoreMutation {
            plugin_id: req.plugin_id.clone(),
            object_path: req.object_path.clone(),
            operation: CoreOperation::SetProperty,
            member_name: req.property_name.clone(),
            value: req
                .value
                .as_ref()
                .map(proto_value_to_json)
                .unwrap_or(serde_json::Value::Null),
            actor_id: req.actor_id.clone(),
            capability_id: req.capability_id.clone(),
            idempotency_key: String::new(),
        };
        match self.core.mutate(op) {
            Ok(outcome) => Ok(Response::new(SetPropertyResponse {
                success: true,
                event_id: outcome.event.event_id,
                event_hash: outcome.event.event_hash,
                error: None,
            })),
            Err(rejection) => Ok(Response::new(SetPropertyResponse {
                success: false,
                event_id: 0,
                event_hash: String::new(),
                error: Some(mutation_error(rejection.code, rejection.message)),
            })),
        }
    }

    type SubscribeSignalsStream = ReceiverStream<Result<Signal, Status>>;

    async fn subscribe_signals(
        &self,
        request: Request<SubscribeSignalsRequest>,
    ) -> Result<Response<Self::SubscribeSignalsStream>, Status> {
        // The fresh core has no signal producers yet, so this forwards
        // SIGNAL-kind frames (none today) over real plumbing. The stream
        // stays open until the client leaves.
        let req = request.into_inner();
        let mut sub = self.core.subscribe_state();
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            loop {
                let received = tokio::select! {
                    _ = tx.closed() => return,
                    received = sub.recv() => received,
                };
                match received {
                    Ok(frame) => {
                        if frame.change_type != ChangeType::Signal as i32 {
                            continue;
                        }
                        if !req.plugin_id.is_empty() && frame.plugin_id != req.plugin_id {
                            continue;
                        }
                        if !req.object_path.is_empty() && frame.object_path != req.object_path {
                            continue;
                        }
                        if !req.signal_names.is_empty()
                            && !req.signal_names.contains(&frame.member_name)
                        {
                            continue;
                        }
                        let signal = Signal {
                            plugin_id: frame.plugin_id.clone(),
                            object_path: frame.object_path.clone(),
                            interface_name: String::new(),
                            signal_name: frame.member_name.clone(),
                            arguments: frame.new_value.into_iter().collect(),
                            timestamp: frame.timestamp.clone(),
                        };
                        if tx.send(Ok(signal)).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let _ = tx.send(Err(Status::data_loss(format!(
                            "signal stream missed {skipped} state frames; resubscribe and refresh state",
                        )))).await;
                        return;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

// ---------------------------------------------------------------------------
// EventChainService
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl EventChainService for BridgeService {
    async fn get_events(
        &self,
        request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let req = request.into_inner();
        let limit = if req.limit == 0 { 100 } else { req.limit } as usize;
        let mut events: Vec<ChainEvent> = self
            .core
            .journal()
            .range(req.from_event_id, req.to_event_id)
            .iter()
            .filter(|e| req.plugin_id.is_empty() || e.plugin_id == req.plugin_id)
            .filter(|e| req.tags.is_empty() || e.tags_touched.iter().any(|t| req.tags.contains(t)))
            .filter(|e| {
                req.decision_filter == Decision::Unspecified as i32
                    || e.decision == req.decision_filter
            })
            .map(ChainEvent::from)
            .collect();
        let has_more = events.len() > limit;
        events.truncate(limit);
        Ok(Response::new(GetEventsResponse { events, has_more }))
    }

    type SubscribeEventsStream = ReceiverStream<Result<ChainEvent, Status>>;

    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let req = request.into_inner();
        let (history, mut sub) = self.core.event_subscription(req.from_event_id);
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(async move {
            // Replay from from_event_id (0 = live events only).
            for event in history {
                let event = ChainEvent::from(&event);
                if chain_matches(&req, &event) && tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
            loop {
                let received = tokio::select! {
                    _ = tx.closed() => return,
                    received = sub.recv() => received,
                };
                match received {
                    Ok(event) => {
                        if event.event_id < req.from_event_id {
                            continue;
                        }
                        if chain_matches(&req, &event) && tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let _ = tx.send(Err(Status::data_loss(format!(
                            "event stream missed {skipped} events; resume with from_event_id after the last received event",
                        )))).await;
                        return;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn verify_chain(
        &self,
        request: Request<VerifyChainRequest>,
    ) -> Result<Response<VerifyChainResponse>, Status> {
        let req = request.into_inner();
        let checked = self
            .core
            .journal()
            .verify(req.from_event_id, req.to_event_id);
        Ok(Response::new(VerifyChainResponse {
            valid: checked.valid,
            events_verified: checked.events_verified,
            batches_verified: 0, // the fresh core has no batch layer
            errors: checked.errors,
        }))
    }

    async fn get_proof(
        &self,
        _request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        Err(Status::unimplemented(
            "merkle proofs are not implemented by the fresh bridge",
        ))
    }

    async fn prove_tag_immutability(
        &self,
        _request: Request<ProveTagImmutabilityRequest>,
    ) -> Result<Response<ProveTagImmutabilityResponse>, Status> {
        Err(Status::unimplemented(
            "tag immutability proofs are not implemented by the fresh bridge",
        ))
    }

    async fn get_snapshot(
        &self,
        _request: Request<GetSnapshotRequest>,
    ) -> Result<Response<GetSnapshotResponse>, Status> {
        Err(Status::unimplemented(
            "snapshots are not implemented by the fresh bridge",
        ))
    }

    async fn create_snapshot(
        &self,
        _request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<CreateSnapshotResponse>, Status> {
        Err(Status::unimplemented(
            "snapshots are not implemented by the fresh bridge",
        ))
    }

    async fn search_semantic_trace(
        &self,
        request: Request<SearchSemanticTraceRequest>,
    ) -> Result<Response<SearchSemanticTraceResponse>, Status> {
        // The request carries no query, only a limit: return the most recent
        // journal events as matches. Scores are recency ranks; the fresh
        // core keeps no vector index.
        let req = request.into_inner();
        let limit = if req.limit == 0 { 10 } else { req.limit } as usize;
        let tip = self.core.journal().tip();
        let (trace_id, mutation_index) = match &tip {
            Some(t) => (t.event_hash.clone(), t.event_id),
            None => (String::new(), 0),
        };
        let matches: Vec<SemanticTraceMatch> = self
            .core
            .journal()
            .range(0, 0)
            .into_iter()
            .rev()
            .take(limit)
            .enumerate()
            .map(|(rank, event)| SemanticTraceMatch {
                point_id: event.event_id.to_string(),
                score: 1.0 / (1.0 + rank as f32),
                payload: Some(json_map_to_proto_struct(&event_payload(&event))),
            })
            .collect();
        Ok(Response::new(SearchSemanticTraceResponse {
            trace_id,
            mutation_index,
            matches,
        }))
    }
}
