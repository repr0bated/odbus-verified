//! The container's own browser door: UI assets and the four RPCs the UI uses.
//!
//! The container is the identity. Every call here is one write to the
//! container's own 3tchedFS `call` file and every read is a read of that mount
//! (docs/FUSEFS.md §12), so identity is the mount's and never a value from the
//! request. This module holds no caller, grant, credential or `Runtime`: a call
//! file is admitted, tracked and receipted by the mount's normal path.
//!
//! FuseFS has no push. A `StateNotice` from this container's MutationEngine,
//! the writer of the SHM state this mount projects, is only the wakeup; each
//! value is re-read from `value.json`.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use _3tched_fs::model::{MethodDecl, PluginSchema};
use _3tched_fs::mounted::{IdentityMode, MountedDiagnostic, MountedNamespace};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tower::Layer;

use crate::core::{BridgeCore, StateNotice, StateScope, json_to_proto_value, proto_value_to_json};
use crate::proto::plugin_service_server::{PluginService, PluginServiceServer};
use crate::proto::state_sync_server::{StateSync, StateSyncServer};
use crate::proto::{
    BatchMutateRequest, BatchMutateResponse, CallMethodRequest, CallMethodResponse, ChangeType,
    ErrorCode, GetPropertyRequest, GetPropertyResponse, GetSchemaRequest, GetSchemaResponse,
    GetStateRequest, GetStateResponse, ListPluginsResponse, MutateRequest, MutateResponse,
    PluginInfo, SetPropertyRequest, SetPropertyResponse, Signal, StateChange, StateFrameKind,
    SubscribeRequest, SubscribeSignalsRequest,
};

const MAX_BLOCKING_MOUNT_WORK: usize = 32;
const HEARTBEAT: Duration = Duration::from_secs(15);

/// The identity door's address, resolved as `runtime_bootstrap` resolves it.
pub(crate) fn enabled() -> Result<bool> {
    let identity_door = std::env::var("OP_GRPC_BRIDGE_LISTEN")
        .unwrap_or_else(|_| "10.0.0.2:8090".into())
        .parse()?;
    Ok(listen_address(identity_door)?.is_some())
}

/// The container's own address for this door, or `None` when it stays off:
/// `OP_UI_LISTEN` absent, or equal to the identity door.
pub(crate) fn listen_address(identity_door: SocketAddr) -> Result<Option<SocketAddr>> {
    parse_listen(
        crate::container_ui::setting("OP_UI_LISTEN")?.as_deref(),
        identity_door,
    )
}

fn parse_listen(value: Option<&str>, identity_door: SocketAddr) -> Result<Option<SocketAddr>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let addr: SocketAddr = value.parse().context("OP_UI_LISTEN is not an address")?;
    if addr == identity_door {
        return Ok(None);
    }
    // Guest-local only: op-tunnel-door splices to it from inside the container.
    ensure!(
        addr.port() != 0
            && (addr.ip().is_loopback() || addr.ip() == IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))),
        "OP_UI_LISTEN must be a guest-local address"
    );
    Ok(Some(addr))
}

/// The mounted operations this door uses; a seam for tests without FUSE.
pub(crate) trait UiNamespace: Send {
    fn view(&self) -> Result<Value>;
    fn schema(&self, plugin: &str) -> Result<PluginSchema>;
    fn state(&self, plugin: &str) -> Result<Value>;
    fn invoke(&self, path: &str, arguments: Value) -> Result<Value>;
}

impl UiNamespace for MountedNamespace {
    fn view(&self) -> Result<Value> {
        self.catalog_view().cloned()
    }
    fn schema(&self, plugin: &str) -> Result<PluginSchema> {
        MountedNamespace::schema(self, plugin)
    }
    fn state(&self, plugin: &str) -> Result<Value> {
        MountedNamespace::state(self, plugin)
    }
    fn invoke(&self, path: &str, arguments: Value) -> Result<Value> {
        // Approval is its own call in the chat flow, before this one.
        MountedNamespace::invoke(self, path, arguments, true)
    }
}

type Open = Arc<dyn Fn() -> Result<Box<dyn UiNamespace>> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct UiService {
    core: BridgeCore,
    open: Open,
    permits: Arc<Semaphore>,
}

impl UiService {
    pub(crate) fn new(core: BridgeCore, mount: PathBuf) -> Self {
        Self::with_opener(
            core,
            Arc::new(move || {
                let namespace =
                    MountedNamespace::open_for_identity_mode(&mount, IdentityMode::GuestKernel, None)?;
                Ok(Box::new(namespace) as Box<dyn UiNamespace>)
            }),
        )
    }

    fn with_opener(core: BridgeCore, open: Open) -> Self {
        Self {
            core,
            open,
            permits: Arc::new(Semaphore::new(MAX_BLOCKING_MOUNT_WORK)),
        }
    }

    /// Open the mount afresh for each request: no schema or state is cached,
    /// and a remounted view is picked up. Kernel FUSE I/O never runs on an
    /// async worker.
    async fn on_mount<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn UiNamespace) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("container mount is busy"))?;
        let open = self.open.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let namespace = open().map_err(|error| {
                tracing::warn!(error = %format!("{error:#}"), "container mount unavailable");
                Status::unavailable("container mount is unavailable")
            })?;
            work(namespace.as_ref())
        })
        .await
        .map_err(|_| Status::internal("container mount worker failed"))?
    }
}

fn read_failed(error: anyhow::Error) -> Status {
    tracing::warn!(error = %format!("{error:#}"), "container mount read failed");
    Status::unavailable("container mount read failed")
}

fn view_plugins(view: &Value) -> Result<Vec<String>, Status> {
    Ok(view["objects"]
        .as_object()
        .ok_or_else(|| Status::unavailable("mounted catalog is absent"))?
        .keys()
        .cloned()
        .collect())
}

fn mounted_schema(
    namespace: &dyn UiNamespace,
    view: &Value,
    plugin: &str,
) -> Result<PluginSchema, Status> {
    if view["objects"].get(plugin).is_none() {
        return Err(Status::not_found("plugin is absent from the container mount"));
    }
    namespace.schema(plugin).map_err(read_failed)
}

/// The sealed JSON omits defaults the mount writes out. Decode it the way the
/// mount does (`from_sealed`) and compare in the mount's typed form.
pub(crate) fn enforces(sealed: Option<&Value>, mounted: &Value) -> bool {
    sealed
        .and_then(|sealed| {
            serde_json::from_value::<op_state_store::PluginSchema>(sealed.clone()).ok()
        })
        .and_then(|sealed| _3tched_fs::model::from_sealed(sealed).ok())
        .and_then(|sealed| serde_json::to_value(sealed).ok())
        .is_some_and(|sealed| sealed == *mounted)
}

fn discloses_state(schema: &PluginSchema) -> bool {
    schema.public_state["paths"]
        .as_array()
        .is_some_and(|paths| !paths.is_empty())
}

/// The browser carries no identity, so any identity metadata is refused: a
/// request can never choose who it runs as.
#[allow(clippy::result_large_err)]
fn refuse_identity(request: Request<()>) -> Result<Request<()>, Status> {
    let carries_identity = request.metadata().keys().any(|key| {
        let name = match key {
            tonic::metadata::KeyRef::Ascii(key) => key.as_str(),
            tonic::metadata::KeyRef::Binary(key) => key.as_str(),
        };
        name == op_identity::sealed_id::HTTP_HEADER_NAME
            || name == "authorization"
            || name == "x-ghostbridge-genesis"
            || name == "x-wireguard-pubkey"
            || name.starts_with("x-oracle-")
            || name.starts_with("x-opdbus-identity-")
            || name.starts_with("x-opdbus-verified-")
    });
    if carries_identity {
        return Err(Status::unauthenticated(
            "the container is the identity; identity metadata is refused",
        ));
    }
    Ok(request)
}

/// A capability named by the caller may only restate the sealed one.
#[allow(clippy::result_large_err)]
fn check_capability<'a>(
    declaration: &MethodDecl,
    named: impl IntoIterator<Item = &'a str>,
) -> Result<(), Status> {
    for capability in named {
        if declaration.required_capability.as_deref() != Some(capability) {
            return Err(Status::permission_denied(
                "capability does not match sealed method",
            ));
        }
    }
    Ok(())
}

fn unknown_outcome(detail: Value) -> CallMethodResponse {
    // Neither success nor refusal: the effect may have happened.
    CallMethodResponse {
        success: false,
        result: Some(json_to_proto_value(&detail)),
        event_id: 0,
        event_hash: String::new(),
        error: None,
    }
}

/// Map a call file's outcome the way the identity door maps a runtime outcome.
fn call_response(outcome: Result<Value>) -> (CallMethodResponse, Option<String>) {
    match outcome {
        Ok(receipt) => {
            let engine = &receipt["result"];
            (
                CallMethodResponse {
                    success: engine["success"].as_bool().unwrap_or(false),
                    result: Some(json_to_proto_value(&engine["result"])),
                    event_id: engine["terminal_event_id"].as_u64().unwrap_or(0),
                    event_hash: engine["terminal_event_hash"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    error: None,
                },
                engine["operation_id"].as_str().map(str::to_owned),
            )
        }
        Err(error) => match error.downcast_ref::<MountedDiagnostic>() {
            Some(MountedDiagnostic(diagnostic)) => {
                let operation = diagnostic["operation_id"].as_str().map(str::to_owned);
                let code = match diagnostic["stage"].as_str() {
                    Some("parse" | "validate") => ErrorCode::ValidationFailed,
                    Some("call") => ErrorCode::PermissionDenied,
                    Some("register" | "credential" | "contract" | "admission") => {
                        ErrorCode::Internal
                    }
                    // outcome_unknown, transport, executor: not a proven refusal.
                    _ => return (unknown_outcome(diagnostic.clone()), operation),
                };
                (
                    CallMethodResponse {
                        success: false,
                        result: Some(json_to_proto_value(diagnostic)),
                        event_id: 0,
                        event_hash: String::new(),
                        error: Some(crate::service::mutation_error(
                            code,
                            diagnostic["error"].as_str().unwrap_or("call refused").to_owned(),
                        )),
                    },
                    operation,
                )
            }
            // The descriptor may already have dispatched; never a refusal.
            None => {
                tracing::warn!(error = %format!("{error:#}"), "call file outcome unknown");
                (
                    unknown_outcome(json!({
                        "stage": "outcome_unknown",
                        "error": "call file outcome is unknown; do not retry automatically",
                    })),
                    None,
                )
            }
        },
    }
}

#[async_trait::async_trait]
impl PluginService for UiService {
    async fn list_plugins(
        &self,
        _request: Request<()>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        let plugins = self
            .on_mount(|namespace| {
                let view = namespace.view().map_err(read_failed)?;
                view_plugins(&view)?
                    .into_iter()
                    .map(|id| {
                        let schema = mounted_schema(namespace, &view, &id)?;
                        Ok(PluginInfo {
                            name: schema.display_name.unwrap_or(schema.name),
                            version: schema.version,
                            description: schema.description,
                            dbus_path: op_plugins::canonical::plugin_path(&id),
                            interfaces: vec![op_plugins::canonical::plugin_interface(&id)],
                            tags: schema.tags,
                            id,
                        })
                    })
                    .collect::<Result<Vec<_>, Status>>()
            })
            .await?;
        Ok(Response::new(ListPluginsResponse { plugins }))
    }

    async fn get_schema(
        &self,
        request: Request<GetSchemaRequest>,
    ) -> Result<Response<GetSchemaResponse>, Status> {
        let req = request.into_inner();
        let schema = self
            .on_mount(move |namespace| {
                let view = namespace.view().map_err(read_failed)?;
                mounted_schema(namespace, &view, &req.plugin_id)
            })
            .await?;
        Ok(Response::new(GetSchemaResponse {
            schema_json: serde_json::to_string(&schema)
                .map_err(|_| Status::internal("mounted schema is not serializable"))?,
            dialect: if req.format.is_empty() {
                "json-schema".into()
            } else {
                req.format
            },
            version: schema.version,
        }))
    }

    async fn call_method(
        &self,
        request: Request<CallMethodRequest>,
    ) -> Result<Response<CallMethodResponse>, Status> {
        // The call file allocates its own operation id; do not accept one and
        // silently ignore it.
        if request.metadata().contains_key("x-opdbus-operation-id") {
            return Err(Status::invalid_argument(
                "the call file allocates the operation id",
            ));
        }
        let mut named = Vec::new();
        for value in request.metadata().get_all("x-opdbus-capability") {
            named.push(
                value
                    .to_str()
                    .map_err(|_| Status::permission_denied("invalid capability"))?
                    .to_owned(),
            );
        }
        let req = request.into_inner();
        if !req.capability_id.is_empty() {
            named.push(req.capability_id);
        }
        if !req.object_path.is_empty()
            && req.object_path != op_plugins::canonical::plugin_path(&req.plugin_id)
        {
            return Err(Status::invalid_argument(
                "method target is not the canonical plugin object",
            ));
        }
        let arguments = match req.arguments.as_slice() {
            [] => json!({}),
            [argument] => proto_value_to_json(argument),
            _ => {
                return Err(Status::invalid_argument(
                    "sealed methods require one named argument object",
                ));
            }
        };
        let (plugin, method) = (req.plugin_id, req.method_name);
        let (response, operation) = self
            .on_mount(move |namespace| {
                let view = namespace.view().map_err(read_failed)?;
                let schema = mounted_schema(namespace, &view, &plugin)?;
                let declaration = schema
                    .methods
                    .get(&method)
                    .ok_or_else(|| Status::not_found("method is absent"))?;
                check_capability(declaration, named.iter().map(String::as_str))?;
                _3tched_fs::dispatch::validate_arguments(declaration, &arguments)
                    .map_err(|error| Status::invalid_argument(error.to_string()))?;
                let path = format!(
                    "plugins/{}/operations/{}/call",
                    _3tched_fs::namespace::encode_component(&plugin),
                    _3tched_fs::namespace::encode_component(&method)
                );
                Ok(call_response(namespace.invoke(&path, arguments)))
            })
            .await?;
        let mut response = Response::new(response);
        if let Some(operation) = operation {
            response.metadata_mut().insert(
                "x-opdbus-operation-id",
                operation
                    .parse()
                    .map_err(|_| Status::internal("operation receipt metadata invalid"))?,
            );
        }
        Ok(response)
    }

    async fn get_property(
        &self,
        _request: Request<GetPropertyRequest>,
    ) -> Result<Response<GetPropertyResponse>, Status> {
        Err(not_served())
    }

    async fn set_property(
        &self,
        _request: Request<SetPropertyRequest>,
    ) -> Result<Response<SetPropertyResponse>, Status> {
        Err(not_served())
    }

    type SubscribeSignalsStream = ReceiverStream<Result<Signal, Status>>;
    async fn subscribe_signals(
        &self,
        _request: Request<SubscribeSignalsRequest>,
    ) -> Result<Response<Self::SubscribeSignalsStream>, Status> {
        Err(not_served())
    }
}

fn not_served() -> Status {
    Status::unimplemented("not served on the container UI door")
}

impl UiService {
    /// Replace a committed frame's value with the mount's own projection.
    /// `None` when the mount discloses nothing for that plugin.
    #[allow(clippy::result_large_err)]
    fn mounted_frame(
        namespace: &dyn UiNamespace,
        view: &Value,
        mut frame: StateChange,
    ) -> Result<Option<StateChange>, Status> {
        if view["objects"].get(&frame.plugin_id).is_none() {
            return Ok(None);
        }
        let schema = mounted_schema(namespace, view, &frame.plugin_id)?;
        if !discloses_state(&schema) {
            return Ok(None);
        }
        if frame.new_value.is_some() {
            let value = namespace.state(&frame.plugin_id).map_err(read_failed)?;
            frame.new_value = Some(json_to_proto_value(&value));
        }
        frame.old_value = None;
        Ok(Some(frame))
    }

    /// Schema frames carry the mounted schema, and only while it is the schema
    /// this bridge enforces.
    fn schema_frame(
        core: &BridgeCore,
        namespace: &dyn UiNamespace,
        view: &Value,
        plugin: &str,
    ) -> Result<StateChange, Status> {
        let schema = serde_json::to_value(mounted_schema(namespace, view, plugin)?)
            .map_err(|_| Status::internal("mounted schema is not serializable"))?;
        let pinned = core
            .catalog()
            .plugin(plugin)
            .filter(|pinned| enforces(pinned.schema.as_ref(), &schema))
            .ok_or_else(|| {
                Status::failed_precondition("mounted schema differs from the sealed catalog")
            })?;
        Ok(StateChange {
            plugin_id: plugin.to_owned(),
            object_path: pinned.dbus_path.clone(),
            change_type: ChangeType::SchemaMigration as i32,
            member_name: pinned.schema_hash.clone(),
            new_value: Some(json_to_proto_value(&schema)),
            frame_kind: StateFrameKind::InitialState as i32,
            schema_hash: pinned.schema_hash.clone(),
            catalog_hash: core.catalog().hash().to_string(),
            ..Default::default()
        })
    }

    async fn hydrate(&self, req: &SubscribeRequest) -> Result<Vec<StateChange>, Status> {
        let core = self.core.clone();
        let req = req.clone();
        self.on_mount(move |namespace| {
            let view = namespace.view().map_err(read_failed)?;
            let mut frames = Vec::new();
            if req.include_initial_state {
                let (committed, _) = core
                    .committed_subscription(&StateScope::Public, true)
                    .map_err(|_| Status::unavailable("public state hydration unavailable"))?;
                for frame in committed {
                    if crate::service::state_matches(&req, &frame)
                        && let Some(frame) = Self::mounted_frame(namespace, &view, frame)?
                    {
                        frames.push(frame);
                    }
                }
            }
            if req.include_schema {
                for plugin in view_plugins(&view)? {
                    if !req.plugin_ids.is_empty() && !req.plugin_ids.contains(&plugin) {
                        continue;
                    }
                    frames.push(Self::schema_frame(&core, namespace, &view, &plugin)?);
                }
            }
            Ok(frames)
        })
        .await
    }

    async fn live_frame(&self, notice: StateNotice) -> Result<Option<StateChange>, Status> {
        let core = self.core.clone();
        self.on_mount(move |namespace| {
            let Some(frame) = core
                .committed_frame(&StateScope::Public, &notice)
                .map_err(|_| Status::unavailable("committed state observation unavailable"))?
            else {
                return Ok(None);
            };
            let view = namespace.view().map_err(read_failed)?;
            Self::mounted_frame(namespace, &view, frame)
        })
        .await
    }

    async fn forward(
        self,
        req: SubscribeRequest,
        initial: Vec<StateChange>,
        mut notices: broadcast::Receiver<StateNotice>,
        tx: mpsc::Sender<Result<StateChange, Status>>,
    ) {
        let mut generations = std::collections::HashMap::<String, u64>::new();
        for frame in initial {
            if frame.change_type != ChangeType::SchemaMigration as i32 {
                generations.insert(frame.plugin_id.clone(), frame.state_generation);
            }
            if tx.send(Ok(frame)).await.is_err() {
                return;
            }
        }
        // Keepalive only: it reads neither the mount nor state.
        let mut heartbeat = tokio::time::interval(HEARTBEAT);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;
        loop {
            let notice = tokio::select! {
                _ = tx.closed() => return,
                _ = heartbeat.tick() => {
                    let frame = StateChange {
                        frame_kind: StateFrameKind::Heartbeat as i32,
                        catalog_hash: self.core.catalog().hash().to_string(),
                        ..Default::default()
                    };
                    if tx.send(Ok(frame)).await.is_err() {
                        return;
                    }
                    continue;
                }
                notice = notices.recv() => notice,
            };
            let notice = match notice {
                Ok(notice) => notice,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = tx
                        .send(Err(Status::data_loss(
                            "committed state stream lagged; hydrate again",
                        )))
                        .await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if !notice.state_committed || notice.scope != StateScope::Public {
                continue;
            }
            match self.live_frame(notice).await {
                Ok(Some(frame)) => {
                    if generations
                        .get(&frame.plugin_id)
                        .is_some_and(|seen| *seen >= frame.state_generation)
                    {
                        continue;
                    }
                    generations.insert(frame.plugin_id.clone(), frame.state_generation);
                    if crate::service::state_matches(&req, &frame) && tx.send(Ok(frame)).await.is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {}
                // A missed frame breaks the order; the client hydrates again.
                Err(status) => {
                    tracing::warn!(%status, "container state feed ended");
                    let _ = tx
                        .send(Err(Status::data_loss(
                            "committed state could not be read; hydrate again",
                        )))
                        .await;
                    return;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl StateSync for UiService {
    type SubscribeStream = ReceiverStream<Result<StateChange, Status>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        // Subscribe before reading so no commit falls between the two.
        let notices = self.core.subscribe_commits();
        let initial = self.hydrate(&req).await?;
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(self.clone().forward(req, initial, notices, tx));
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn mutate(
        &self,
        _request: Request<MutateRequest>,
    ) -> Result<Response<MutateResponse>, Status> {
        Err(not_served())
    }

    async fn get_state(
        &self,
        _request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        Err(not_served())
    }

    async fn batch_mutate(
        &self,
        _request: Request<BatchMutateRequest>,
    ) -> Result<Response<BatchMutateResponse>, Status> {
        Err(not_served())
    }
}

/// Every other gRPC path gets tonic's `UNIMPLEMENTED` fallback.
pub(crate) fn router(service: UiService) -> axum::Router {
    let web = tonic_web::GrpcWebLayer::new();
    let app = tonic::service::Routes::new(web.layer(StateSyncServer::with_interceptor(
        service.clone(),
        refuse_identity,
    )))
    .add_service(web.layer(PluginServiceServer::with_interceptor(
        service,
        refuse_identity,
    )))
    .prepare()
    .into_axum_router();
    // tonic's router is axum 0.8; mount it as a plain service under axum 0.7.
    axum::Router::new().fallback_service(app)
}

/// The container's UI door: its own TLS listener with the container's
/// certificate, reached only through the in-container `op-tunnel-door` splice.
/// The identity door's admission is unaffected.
pub(crate) struct UiDoor {
    listener: tokio::net::TcpListener,
    tls: Arc<tokio_rustls::rustls::ServerConfig>,
    app: axum::Router,
}

impl UiDoor {
    /// `None` when `OP_UI_LISTEN` leaves this door off.
    pub(crate) async fn bind(
        identity_door: SocketAddr,
        core: BridgeCore,
        identity: &crate::tls_door::PemIdentity,
    ) -> Result<Option<Self>> {
        let Some(addr) = listen_address(identity_door)? else {
            return Ok(None);
        };
        ensure!(
            crate::container_ui::setting("CHAT_UI_DIR")?.is_some(),
            "OP_UI_LISTEN requires CHAT_UI_DIR"
        );
        let service = UiService::new(core, crate::container_ui::mount_path()?);
        Ok(Some(Self {
            app: crate::container_ui::assets(router(service))?,
            tls: identity.server_config()?,
            listener: tokio::net::TcpListener::bind(addr).await?,
        }))
    }

    pub(crate) async fn serve<F: Future<Output = ()>>(self, shutdown: F) -> Result<()> {
        crate::tls_door::serve_upgradeable_tcp_with_shutdown(
            self.listener,
            self.tls,
            self.app,
            shutdown,
        )
        .await
    }
}

#[cfg(test)]
#[path = "container_ui_door_tests.rs"]
mod tests;
