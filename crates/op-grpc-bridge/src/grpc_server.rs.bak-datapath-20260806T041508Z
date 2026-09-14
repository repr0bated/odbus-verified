//! gRPC Server - Implements the Operation gRPC services (shared-server topology)

use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use async_stream::stream;
use chrono::{DateTime, Utc};
use futures::StreamExt as _;
use op_cognitive_mcp::QdrantSemanticShuttle;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage};
use prost_types::{Struct as ProstStruct, Timestamp as ProstTimestamp, Value as ProstValue};
use serde::de::DeserializeSeed;
use serde_json::Value as JsonValue;
use simd_json::prelude::{ValueAsContainer, ValueAsScalar};
use tokio::sync::{broadcast, RwLock};
use tokio_stream::Stream;
use tonic::transport::{Identity, ServerTlsConfig};
use tonic::{Request, Response, Status};
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::interceptor::{self, load_capability_grants, GhostbridgeIdentity};
use crate::oracle_assertion::{AssertionValidator, DecoyTrustStore};

use crate::dynamic_reflection::{ActiveReflectionCatalog, DynamicReflectionService};
use crate::mutation_engine::{ChangeType, MutationEngine};
use crate::plugin_grpc_gen::MethodServiceRegistry;
use crate::plugin_object_blob::blobify_plugin_schema;
use crate::proto::{
    event_chain_service_server::EventChainService, ovsdb_mirror_server::OvsdbMirror,
    plugin_service_server::PluginService, runtime_mirror_server::RuntimeMirror,
    state_sync_server::StateSync, BatchMutateRequest, BatchMutateResponse, CallMethodRequest,
    CallMethodResponse, CapabilityMissing as ProtoCapabilityMissing, ChainEvent as ProtoChainEvent,
    ChangeType as ProtoChangeType, ConstraintFail as ProtoConstraintFail, CreateSnapshotRequest,
    CreateSnapshotResponse, Decision as ProtoDecision, DenyReason as ProtoDenyReason,
    ErrorCode as ProtoErrorCode, GetEventsRequest, GetEventsResponse, GetProofRequest,
    GetProofResponse, GetPropertyRequest, GetPropertyResponse, GetSchemaRequest, GetSchemaResponse,
    GetSnapshotRequest, GetSnapshotResponse, GetStateRequest, GetStateResponse,
    ListPluginsResponse, MerkleProofSibling, MutateRequest, MutateResponse,
    MutationError as ProtoMutationError, NumaNode as ProtoNumaNode,
    OperationType as ProtoOperationType, OvsdbBridge as ProtoOvsdbBridge, OvsdbDumpDbRequest,
    OvsdbDumpDbResponse, OvsdbEchoRequest, OvsdbEchoResponse, OvsdbGetBridgeStateRequest,
    OvsdbGetBridgeStateResponse, OvsdbGetSchemaRequest, OvsdbGetSchemaResponse,
    OvsdbInterface as ProtoOvsdbInterface, OvsdbListDbsResponse, OvsdbMonitorRequest,
    OvsdbPort as ProtoOvsdbPort, OvsdbTransactRequest, OvsdbTransactResponse, OvsdbUpdate,
    PluginInfo, ProveTagImmutabilityRequest, ProveTagImmutabilityResponse,
    ReadOnlyViolation as ProtoReadOnlyViolation, RuntimeGetNumaTopologyResponse,
    RuntimeGetServiceRequest, RuntimeGetSystemInfoResponse, RuntimeListInterfacesResponse,
    RuntimeListServicesRequest, RuntimeListServicesResponse, RuntimeMetricUpdate,
    RuntimeNetworkInterface as ProtoRuntimeNetworkInterface,
    RuntimeServiceInfo as ProtoRuntimeServiceInfo, RuntimeStreamMetricsRequest,
    SearchSemanticTraceRequest, SearchSemanticTraceResponse, SemanticTraceMatch,
    SetPropertyRequest, SetPropertyResponse, Signal, StateChange as ProtoStateChange,
    SubscribeEventsRequest, SubscribeRequest, SubscribeSignalsRequest, TagLock as ProtoTagLock,
    VerifyChainRequest, VerifyChainResponse,
};
use op_state_store::{Decision, DenyReason, MerkleProof, PluginSchema};
use zbus::zvariant::{Array as ZArray, OwnedValue as ZOwnedValue, Str as ZStr, Value as ZValue};
use zbus::{Connection, Proxy};

/// Plugin schema provider.
///
/// The provider reads only from the sealed blob catalog. A blob in the
/// catalog IS the plugin; nothing else is a valid source of truth.
#[tonic::async_trait]
pub trait PluginSchemaProvider: Send + Sync {
    async fn list_plugins(&self) -> Vec<PluginInfo>;
    async fn get_schema(&self, plugin_id: &str) -> Option<(String, String, String)>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchemaMethodCapability {
    required_capability: Option<String>,
    grants_declared: bool,
    granted_by_footprint: bool,
}

fn read_plugin_schema_json(plugin_id: &str) -> Option<JsonValue> {
    if let Some(schema) = op_blob::catalog::read_plugin_schema_shm(plugin_id) {
        return serde_json::to_value(&schema).ok();
    }
    inventory_plugin_schema_json(plugin_id)
}

fn inventory_plugin_schema_json(plugin_id: &str) -> Option<JsonValue> {
    use op_state::StatePlugin;
    let schema = match plugin_id {
        "human_principal" => op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()?,
        "identity_sled" => op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()?,
        _ => return None,
    };
    serde_json::to_value(&schema).ok()
}

/// Request metadata key by which a caller declares the capability it is exercising.
///
/// `enforce_bridge_capability` requires the declaration to match the method's
/// `required_capability`; a mismatch or an absent header denies. This keeps the gRPC
/// check two-factor — the caller states intent, and must also carry a Ghostbridge
/// identity — which is stricter than the D-Bus path, where possession of the grant
/// alone suffices.
pub const DECLARED_CAPABILITY_HEADER: &str = "x-opdbus-capability";

fn method_capability_from_schema(
    schema: &JsonValue,
    method_name: &str,
    footprint: Option<&str>,
) -> Option<SchemaMethodCapability> {
    let method = schema.get("methods")?.as_object()?.get(method_name)?;
    let required_capability = match method.get("required_capability") {
        Some(JsonValue::String(capability)) if !capability.is_empty() => Some(capability.clone()),
        _ => None,
    };

    let Some(capability) = required_capability.as_deref() else {
        return Some(SchemaMethodCapability {
            required_capability: None,
            grants_declared: false,
            granted_by_footprint: true,
        });
    };

    let mut grants_declared = false;
    let mut granted_by_footprint = false;
    if let Some(grants) = schema
        .get("capability_grants")
        .and_then(JsonValue::as_object)
    {
        grants_declared = true;
        if let Some(footprint) = footprint {
            granted_by_footprint = grants
                .get(footprint)
                .or_else(|| grants.get("*"))
                .and_then(JsonValue::as_array)
                .is_some_and(|caps| caps.iter().any(|cap| cap.as_str() == Some(capability)));
        }
    } else if let Some(footprint) = footprint {
        grants_declared = true;
        granted_by_footprint = load_capability_grants(footprint).contains(capability);
    }

    Some(SchemaMethodCapability {
        required_capability,
        grants_declared,
        granted_by_footprint,
    })
}

fn enforce_bridge_capability(
    plugin_id: &str,
    method_name: &str,
    capability_id: Option<&str>,
    identity: Option<&GhostbridgeIdentity>,
) -> Result<(), ProtoMutationError> {
    let schema = read_plugin_schema_json(plugin_id);
    enforce_bridge_capability_with_schema(
        schema.as_ref(),
        plugin_id,
        method_name,
        capability_id,
        identity,
    )
}

/// The decision half of [`enforce_bridge_capability`], split out so tests can
/// drive the gate against an explicit schema document; the production half
/// reads the sealed schema from the SHM blob catalog. Behavior is identical:
/// `None` schema means the plugin is not sealed and the gate stays open.
pub(crate) fn enforce_bridge_capability_with_schema(
    schema: Option<&JsonValue>,
    plugin_id: &str,
    method_name: &str,
    capability_id: Option<&str>,
    identity: Option<&GhostbridgeIdentity>,
) -> Result<(), ProtoMutationError> {
    let footprint = identity.map(|ctx| ctx.footprint.as_str());
    let Some(method_capability) =
        schema.and_then(|schema| method_capability_from_schema(schema, method_name, footprint))
    else {
        return Ok(());
    };
    let Some(required) = method_capability.required_capability else {
        return Ok(());
    };

    let capability_matches = capability_id == Some(required.as_str());
    let footprint_grants = if method_capability.grants_declared {
        method_capability.granted_by_footprint
    } else {
        identity.is_some() && capability_matches
    };

    if capability_matches && footprint_grants {
        Ok(())
    } else {
        Err(ProtoMutationError {
            code: ProtoErrorCode::PermissionDenied as i32,
            message: format!(
                "AccessDenied: method {plugin_id}.{method_name} requires capability {required}"
            ),
            deny_reason: Some(ProtoDenyReason {
                reason: Some(crate::proto::deny_reason::Reason::CapabilityMissing(
                    ProtoCapabilityMissing {
                        capability: required,
                    },
                )),
            }),
        })
    }
}

// tonic::Status is 176 bytes; Boxing the error would ripple through the
// shared bridge surface for zero runtime benefit, so the large Err is the
// deliberate disposition.
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_schema_method(
    plugin_id: &str,
    method_name: &str,
    capability_id: Option<&str>,
    identity: Option<&GhostbridgeIdentity>,
) -> Result<(), Status> {
    enforce_bridge_capability(plugin_id, method_name, capability_id, identity)
        .map_err(|error| Status::permission_denied(error.message))
}

fn plugin_id_from_dbus_path(path: &str) -> Option<String> {
    let prefix = "/org/opdbus/v1/plugins/";
    path.strip_prefix(prefix)
        .and_then(|rest| rest.split('/').next())
        .filter(|id| !id.is_empty())
        .map(|id| id.replace('/', "."))
}

#[tonic::async_trait]
impl PluginSchemaProvider for SchemaCatalogPluginProvider {
    async fn list_plugins(&self) -> Vec<PluginInfo> {
        read_schema_catalog_plugins()
    }

    async fn get_schema(&self, plugin_id: &str) -> Option<(String, String, String)> {
        read_schema(plugin_id)
    }
}

struct SchemaCatalogPluginProvider;

fn read_schema_catalog_plugins() -> Vec<PluginInfo> {
    // A blob in the catalog IS the plugin: enumerate the sealed catalog.
    let Ok(store) = op_blob::BlobStore::open_default() else {
        return Vec::new();
    };
    store
        .plugin_object_blobs()
        .iter()
        .filter_map(|blob| {
            let schema: JsonValue = serde_json::from_str(&blob.schema_json).ok()?;
            Some(plugin_info_from_schema(&blob.manifest.plugin_id, &schema))
        })
        .collect()
}

fn read_schema(plugin_id: &str) -> Option<(String, String, String)> {
    let schema = read_plugin_schema_json(plugin_id)?;
    schema_response_from_value(&schema)
}

fn schema_response_from_value(schema: &JsonValue) -> Option<(String, String, String)> {
    let version = schema
        .get("version")
        .and_then(JsonValue::as_str)
        .unwrap_or("1.0.0")
        .to_string();
    Some((
        serde_json::to_string(schema).ok()?,
        "operation.pluginschema+json".to_string(),
        version,
    ))
}

fn plugin_info_from_schema(id: &str, schema: &JsonValue) -> PluginInfo {
    let name = schema
        .get("name")
        .and_then(JsonValue::as_str)
        .unwrap_or(id)
        .to_string();
    let version = schema
        .get("version")
        .and_then(JsonValue::as_str)
        .unwrap_or("1.0.0")
        .to_string();
    let description = schema
        .get("description")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let mut tags = Vec::new();
    if let Some(category) = schema.get("category").and_then(JsonValue::as_str) {
        tags.push(category.to_string());
    }

    PluginInfo {
        id: id.to_string(),
        name,
        version,
        description,
        dbus_path: format!("/org/opdbus/v1/plugins/{}", id.replace('.', "/")),
        interfaces: vec!["org.opdbus.v1.PluginV1".to_string()],
        tags,
    }
}

// =============================================================================
// Registry State
// =============================================================================

/// In-memory component registry backing ComponentRegistry gRPC service.
/// Shared via Arc across all clones of OperationGrpcServer.
struct RegistryInner {
    /// component_id → ComponentInfo
    components: HashMap<String, crate::proto::registry::ComponentInfo>,
    /// component_id → lease_token
    leases: HashMap<String, String>,
    /// Broadcast channel for Watch stream
    watch_tx: broadcast::Sender<crate::proto::registry::RegistryEvent>,
}

impl RegistryInner {
    fn new() -> (
        Self,
        broadcast::Sender<crate::proto::registry::RegistryEvent>,
    ) {
        let (tx, _) = broadcast::channel(256);
        (
            Self {
                components: HashMap::new(),
                leases: HashMap::new(),
                watch_tx: tx.clone(),
            },
            tx,
        )
    }
}

// =============================================================================
// gRPC server implementation for operation services
// =============================================================================

use crate::per_plugin_reflection::PerMethodReflectionRegistry;
use crate::plugin_grpc_gen::PerMethodGrpcServices;

#[derive(Clone)]
pub struct OperationGrpcServer {
    mutation_engine: Arc<MutationEngine>,
    plugin_provider: Arc<dyn PluginSchemaProvider>,
    semantic_shuttle: Option<Arc<QdrantSemanticShuttle>>,
    /// Broadcast channel for chain events
    chain_events: broadcast::Sender<ProtoChainEvent>,
    /// Component registry state (shared across clones)
    registry: Arc<RwLock<RegistryInner>>,
    /// Per-method gRPC services (frozen at D-Bus object creation)
    per_method_services: Arc<PerMethodGrpcServices>,
    /// Per-method reflection registry
    per_method_reflection: Arc<PerMethodReflectionRegistry>,
    /// Active D-Bus object blobs backing gRPC reflection discovery.
    active_reflection: ActiveReflectionCatalog,
}

impl OperationGrpcServer {
    pub fn new(mutation_engine: Arc<MutationEngine>) -> Self {
        let (chain_tx, _) = broadcast::channel(1024);
        let (registry, _) = RegistryInner::new();
        Self {
            mutation_engine,
            plugin_provider: Arc::new(SchemaCatalogPluginProvider),
            semantic_shuttle: None,
            chain_events: chain_tx,
            registry: Arc::new(RwLock::new(registry)),
            per_method_services: PerMethodGrpcServices::new(),
            per_method_reflection: Arc::new(PerMethodReflectionRegistry::new()),
            active_reflection: ActiveReflectionCatalog::new(),
        }
    }

    pub fn with_plugin_provider(
        mutation_engine: Arc<MutationEngine>,
        plugin_provider: Arc<dyn PluginSchemaProvider>,
    ) -> Self {
        let (chain_tx, _) = broadcast::channel(1024);
        let (registry, _) = RegistryInner::new();
        Self {
            mutation_engine,
            plugin_provider,
            semantic_shuttle: None,
            chain_events: chain_tx,
            registry: Arc::new(RwLock::new(registry)),
            per_method_services: PerMethodGrpcServices::new(),
            per_method_reflection: Arc::new(PerMethodReflectionRegistry::new()),
            active_reflection: ActiveReflectionCatalog::new(),
        }
    }

    pub fn with_semantic_shuttle(mut self, semantic_shuttle: Arc<QdrantSemanticShuttle>) -> Self {
        self.semantic_shuttle = Some(semantic_shuttle);
        self
    }

    /// Hydrate in-memory reflection from the sealed SHM blob catalog.
    ///
    /// The blob catalog IS the plugin registry: a restart must advertise the
    /// same plugin set that was sealed, without waiting for every plugin to
    /// re-register at runtime. Call once at server startup.
    pub async fn hydrate_reflection_from_shm(&self) -> usize {
        let store = match op_blob::BlobStore::open_default() {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%error, "cannot open SHM blob catalog; reflection starts empty");
                return 0;
            }
        };
        let blobs = store.plugin_object_blobs();
        let count = blobs.len();
        for blob in blobs {
            self.active_reflection.upsert_blob(blob).await;
        }
        // Record the manifest hash so the arrival-triggered resync in the
        // reflection service doesn't reload what startup just loaded.
        let hash = op_blob::catalog::ActiveReflectionCatalog::read_catalog_hash(
            std::path::Path::new(op_blob::catalog::DEFAULT_SHM_DIR),
        );
        self.active_reflection.set_catalog_hash(hash).await;
        tracing::info!(count, "hydrated reflection from SHM blob catalog");
        count
    }

    /// Freeze all plugin method descriptors from the configured schema provider.
    ///
    /// tonic-reflection is immutable once mounted. This must run before
    /// `build_operation_routes` creates the reflection service.
    pub async fn freeze_plugin_method_reflection(&self) {
        for plugin in self.plugin_provider.list_plugins().await {
            let Some((schema_json, _, _)) = self.plugin_provider.get_schema(&plugin.id).await
            else {
                warn!(
                    plugin_id = %plugin.id,
                    "plugin schema missing while freezing gRPC reflection methods"
                );
                continue;
            };
            let Ok(schema) = serde_json::from_str::<PluginSchema>(&schema_json) else {
                warn!(
                    plugin_id = %plugin.id,
                    "plugin schema could not be parsed while freezing gRPC reflection methods"
                );
                continue;
            };
            if schema.methods.is_empty() {
                continue;
            }
            if let Err(error) = self
                .register_plugin_methods(plugin.id.clone(), &schema)
                .await
            {
                warn!(
                    plugin_id = %plugin.id,
                    %error,
                    "failed to freeze plugin method descriptors for gRPC reflection"
                );
            }
        }
    }

    /// Snapshot of all registered components plus a live-update receiver.
    ///
    /// The receiver fires a `RegistryEvent` every time a component registers,
    /// updates, or deregisters.  Use this to mirror the registry into the D-Bus
    /// tree without polling.
    pub async fn registry_watch(
        &self,
    ) -> (
        Vec<crate::proto::registry::ComponentInfo>,
        tokio::sync::broadcast::Receiver<crate::proto::registry::RegistryEvent>,
    ) {
        let inner = self.registry.read().await;
        let snapshot: Vec<_> = inner.components.values().cloned().collect();
        let rx = inner.watch_tx.subscribe();
        (snapshot, rx)
    }

    /// Register a plugin's methods as per-method gRPC services.
    ///
    /// This is called at D-Bus object creation time. Each method gets its own
    /// gRPC service generated and frozen. The reflection service is updated
    /// to include these new method services.
    pub async fn register_plugin_methods(
        &self,
        plugin_id: String,
        schema: &PluginSchema,
    ) -> Result<Vec<MethodServiceRegistry>, anyhow::Error> {
        // Create frozen services for all methods
        let registries = self
            .per_method_services
            .create_frozen_services(plugin_id.clone(), schema)
            .await?;

        // Register each method with the reflection registry
        for registry in &registries {
            self.per_method_reflection
                .register(
                    plugin_id.clone(),
                    registry.method_name.clone(),
                    registry.subid.clone(),
                    registry.required_capability.clone(),
                    registry.schema_version.clone(),
                )
                .await;
        }

        let blob = blobify_plugin_schema(&plugin_id, schema.clone());
        // Persist the sealed per-object blob to shm (zero-copy read-path for
        // other processes) before handing it to the reflection catalog.
        match op_blob::BlobStore::open_default() {
            Ok(mut store) => {
                if let Err(error) = store.write(&blob) {
                    tracing::warn!(%plugin_id, %error, "failed to persist plugin object blob");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to open plugin object blob store");
            }
        }
        self.active_reflection.upsert_blob(blob).await;

        Ok(registries)
    }

    /// Get the per-method services registry.
    pub fn per_method_services(&self) -> Arc<PerMethodGrpcServices> {
        self.per_method_services.clone()
    }

    /// Get the per-method reflection registry.
    pub fn per_method_reflection(&self) -> Arc<PerMethodReflectionRegistry> {
        self.per_method_reflection.clone()
    }

    pub fn active_reflection(&self) -> ActiveReflectionCatalog {
        self.active_reflection.clone()
    }

    pub(crate) async fn call_generated_plugin_method_typed<Req, Res>(
        &self,
        plugin_id: &'static str,
        method_name: &'static str,
        request_message_name: &'static str,
        response_message_name: &'static str,
        request: Request<Req>,
    ) -> Result<Response<Res>, Status>
    where
        Req: prost::Message + Default,
        Res: prost::Message + Default,
    {
        let identity = interceptor::bridge_capability_identity(request.extensions());

        // The caller declares which capability it is exercising, and the bridge
        // verifies that declaration against the method's `required_capability`.
        //
        // Passing `None` here (as this previously did) made `capability_matches` in
        // `enforce_bridge_capability` compare `None == Some(required)`, which is
        // always false — and since the decision is `capability_matches &&
        // footprint_grants`, every capability-gated method was permanently denied
        // over gRPC/gRPC-Web. All 16 cognitive_mcp methods are gated, so none were
        // reachable despite the services being mounted.
        //
        // Declaration is deliberate rather than derived from the schema: it keeps the
        // check two-factor (the caller must state its intent AND hold an identity)
        // and makes the exercised capability visible in traces. An absent or
        // malformed header still denies.
        let declared_capability = request
            .metadata()
            .get(DECLARED_CAPABILITY_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if let Err(error) = enforce_bridge_capability(
            plugin_id,
            method_name,
            declared_capability.as_deref(),
            identity.as_ref(),
        ) {
            return Err(Status::permission_denied(error.message));
        }

        let request_bytes = request.into_inner().encode_to_vec();
        let request_desc = plugin_descriptor_pool()
            .get_message_by_name(request_message_name)
            .ok_or_else(|| {
                Status::internal(format!(
                    "missing request descriptor for {plugin_id}.{method_name}"
                ))
            })?;
        let request_dynamic = DynamicMessage::decode(request_desc, request_bytes.as_slice())
            .map_err(|error| {
                Status::internal(format!(
                    "failed to decode typed request for {plugin_id}.{method_name}: {error}"
                ))
            })?;
        let json_args = serde_json::to_string(&request_dynamic).map_err(|error| {
            Status::internal(format!(
                "failed to serialize typed request for {plugin_id}.{method_name}: {error}"
            ))
        })?;

        let result = self
            .mutation_engine
            .dispatch_method_call(
                plugin_id,
                method_name,
                &json_args,
                None,
                "grpc:operation.plugin.v1",
            )
            .await
            .map_err(|error| {
                Status::internal(format!(
                    "plugin method {}.{} failed: {}",
                    plugin_id, method_name, error
                ))
            })?;

        let result_json = serde_json::to_string(&result).map_err(|error| {
            Status::internal(format!(
                "failed to serialize method result for {plugin_id}.{method_name}: {error}"
            ))
        })?;
        let response_desc = plugin_descriptor_pool()
            .get_message_by_name(response_message_name)
            .ok_or_else(|| {
                Status::internal(format!(
                    "missing response descriptor for {plugin_id}.{method_name}"
                ))
            })?;
        let mut deserializer = serde_json::de::Deserializer::from_str(&result_json);
        let response_dynamic = response_desc
            .deserialize(&mut deserializer)
            .map_err(|error| {
                Status::internal(format!(
                    "failed to deserialize typed response for {plugin_id}.{method_name}: {error}"
                ))
            })?;
        deserializer.end().map_err(|error| {
            Status::internal(format!(
                "failed to finish typed response decode for {plugin_id}.{method_name}: {error}"
            ))
        })?;

        let mut bytes = Vec::new();
        response_dynamic.encode(&mut bytes).map_err(|error| {
            Status::internal(format!(
                "failed to encode typed response for {plugin_id}.{method_name}: {error}"
            ))
        })?;
        let response = Res::decode(bytes.as_slice()).map_err(|error| {
            Status::internal(format!(
                "failed to decode typed response for {plugin_id}.{method_name}: {error}"
            ))
        })?;
        Ok(Response::new(response))
    }
}

fn plugin_descriptor_pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(crate::proto::FILE_DESCRIPTOR_SET)
            .expect("decode combined plugin descriptor pool")
    })
}

include!(concat!(env!("OUT_DIR"), "/plugin_method_routes.rs"));

/// Mount the COMPLETE Operation gRPC service surface onto a `tonic::service::Routes`.
///
/// This is the SINGLE source of the backplane's service set. Every endpoint that
/// serves the backplane — the op-dbus bridge (`run_grpc_server`, TCP `:50051`) and
/// the zeroclaw bridge (`run_zeroclaw_server`, `:8090` + `container.sock`) — builds
/// its routes from here, so reflection can never advertise a service that isn't
/// actually mounted (the bug the gRPC-Web probe surfaced).
///
/// Services (all behind the Ghostbridge interceptor, all gRPC-Web enabled):
///   StateSync, PluginService, EventChainService, OvsdbMirror, RuntimeMirror,
///   ComponentRegistry, MailService, PrivacyNetworkService, RegistrationService,
///   DbusPassthrough, ChatService, plus gRPC server reflection.
///
/// The caller supplies a fully-configured `OperationGrpcServer` (plugin provider /
/// semantic shuttle already attached) and adds endpoint-specific extras
/// afterward (e.g. the zeroclaw bridge adds `ZeroclawService`; `run_grpc_server`
/// adds the gRPC health service).
///
/// Adding a new domain service: add one `.add_service(...)` here and it appears on
/// BOTH endpoints at once.
pub fn build_operation_routes(server: OperationGrpcServer) -> tonic::service::Routes {
    let validator = Arc::new(AssertionValidator::new(DecoyTrustStore::load()));
    build_operation_routes_with_validator(server, validator)
}

pub fn build_operation_routes_with_validator(
    server: OperationGrpcServer,
    validator: Arc<AssertionValidator>,
) -> tonic::service::Routes {
    use crate::proto::dbus_passthrough_server::DbusPassthroughServer;
    use crate::proto::event_chain_service_server::EventChainServiceServer;
    use crate::proto::mail::mail_service_server::MailServiceServer;
    use crate::proto::ovsdb_mirror_server::OvsdbMirrorServer;
    use crate::proto::plugin_service_server::PluginServiceServer;
    use crate::proto::privacy::privacy_network_service_server::PrivacyNetworkServiceServer;
    use crate::proto::registration::registration_service_server::RegistrationServiceServer;
    use crate::proto::registry::component_registry_server::ComponentRegistryServer;
    use crate::proto::runtime_mirror_server::RuntimeMirrorServer;
    use crate::proto::state_sync_server::StateSyncServer;

    // Reflection — DynamicReflectionService serves from the live
    // ActiveReflectionCatalog, which resyncs from the sealed SHM blob catalog
    // on each incoming reflection request (arrival-triggered via the
    // manifest's catalog_hash; no watcher, no restart needed when the
    // op-blob sealer commits new blobs).
    // Give the interceptor a handle to the engine so it can look up
    // per-identity sled records (Cozo-backed) instead of only the shared
    // host legacy sled. Called exactly once, before any interceptor runs.
    interceptor::set_engine(server.mutation_engine.clone());

    let reflection_v1 = DynamicReflectionService::new(server.active_reflection()).into_v1_server();

    let intercept = interceptor::make_ghostbridge_interceptor(validator.clone());
    let registration_intercept = interceptor::make_registration_interceptor(validator);

    let routes = tonic::service::Routes::new(crate::grpc_web::enable(
        StateSyncServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        PluginServiceServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        EventChainServiceServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        OvsdbMirrorServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        RuntimeMirrorServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        ComponentRegistryServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        MailServiceServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        PrivacyNetworkServiceServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        RegistrationServiceServer::with_interceptor(
            server.clone(),
            registration_intercept,
        ),
    ))
    .add_service(crate::grpc_web::enable(
        DbusPassthroughServer::with_interceptor(server.clone(), intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        crate::proto::chat::chat_service_server::ChatServiceServer::with_interceptor(
            crate::chat_service::ChatServiceImpl::new(server.mutation_engine.clone()),
            intercept,
        ),
    ))
    // EMQX is the gRPC client and does not carry Ghostbridge identity headers.
    // ExHook is an audit tap, not an authorization or mutation surface.
    .add_service(
        crate::proto::emqx_exhook::hook_provider_server::HookProviderServer::new(
            crate::emqx_hook_provider::HookProviderService::new(server.mutation_engine.clone()),
        ),
    );

    let routes = add_routes(routes, server.clone());
    routes.add_service(crate::grpc_web::enable(reflection_v1))
}

/// Run gRPC server for all Operation services.
///
/// Mounts the shared [`build_operation_routes`] surface plus the gRPC health
/// protocol (liveness for deploy verification and load balancers), with optional
/// TLS. Adding a domain service is done in [`build_operation_routes`], not here.
pub async fn run_grpc_server(
    addr: std::net::SocketAddr,
    mutation_engine: Arc<MutationEngine>,
    plugin_provider: Option<Arc<dyn PluginSchemaProvider>>,
    tls_identity: Option<Identity>,
) -> Result<(), tonic::transport::Error> {
    // Imports here back only the `health_reporter.set_serving::<T>()` type args;
    // the service *instances* are mounted in `build_operation_routes`.
    use crate::proto::event_chain_service_server::EventChainServiceServer;
    use crate::proto::mail::mail_service_server::MailServiceServer;
    use crate::proto::ovsdb_mirror_server::OvsdbMirrorServer;
    use crate::proto::plugin_service_server::PluginServiceServer;
    use crate::proto::privacy::privacy_network_service_server::PrivacyNetworkServiceServer;
    use crate::proto::registration::registration_service_server::RegistrationServiceServer;
    use crate::proto::registry::component_registry_server::ComponentRegistryServer;
    use crate::proto::runtime_mirror_server::RuntimeMirrorServer;
    use crate::proto::state_sync_server::StateSyncServer;

    // Restore the durable audit trail before serving: a query issued right
    // after a restart must see pre-restart events.
    let replayed = mutation_engine.init_audit_durability().await;
    if replayed > 0 {
        info!(replayed, "audit trail restored from durable storage");
    }

    match mutation_engine.seed_missing_plugin_projections().await {
        Ok(count) => info!(count, "Plugin projections seeded"),
        Err(error) => warn!(%error, "Plugin projection seeding failed"),
    }

    let server = if let Some(provider) = plugin_provider {
        OperationGrpcServer::with_plugin_provider(mutation_engine, provider)
    } else {
        OperationGrpcServer::new(mutation_engine)
    };
    // The sealed SHM blob catalog IS the plugin set: hydrate reflection from
    // it so a restart advertises every sealed plugin immediately.
    server.hydrate_reflection_from_shm().await;
    let server = match QdrantSemanticShuttle::new().await {
        Ok(shuttle) => server.with_semantic_shuttle(Arc::new(shuttle)),
        Err(error) => {
            warn!(%error, "semantic shuttle unavailable; SearchSemanticTrace will return failed_precondition");
            server
        }
    };
    server.freeze_plugin_method_reflection().await;

    // Health — standard gRPC health protocol for deploy verification and LB probes.
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<StateSyncServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<PluginServiceServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<EventChainServiceServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<OvsdbMirrorServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<RuntimeMirrorServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<ComponentRegistryServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<MailServiceServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<PrivacyNetworkServiceServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<RegistrationServiceServer<OperationGrpcServer>>()
        .await;
    health_reporter
        .set_serving::<crate::proto::chat::chat_service_server::ChatServiceServer<
            crate::chat_service::ChatServiceImpl,
        >>()
        .await;

    info!(addr = %addr, "gRPC bridge listening");

    // Wrap services with gRPC-Web support and allow JSON encoding.
    // The GhostbridgeInterceptor enforces the Absolute Base rule on every
    // inbound request before it reaches a service handler.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers([
            "grpc-status".parse().unwrap(),
            "grpc-message".parse().unwrap(),
            "grpc-status-details-bin".parse().unwrap(),
        ]);

    let mut builder = tonic::transport::Server::builder()
        .accept_http1(true)
        .layer(cors);

    if let Some(identity) = tls_identity {
        builder = builder.tls_config(ServerTlsConfig::new().identity(identity))?;
    }

    // The complete service surface comes from the shared builder; the health
    // service is the only extra mounted here (it is endpoint-local, not part of
    // the reflected descriptor).
    builder
        .add_routes(build_operation_routes(server))
        .add_service(crate::grpc_web::enable(health_service))
        .serve(addr)
        .await
}

// =============================================================================
// StateSync Service
// =============================================================================

#[tonic::async_trait]
impl StateSync for OperationGrpcServer {
    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<ProtoStateChange, Status>> + Send>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        info!("gRPC Subscribe: plugins={:?}", req.plugin_ids);

        let mut rx = self.mutation_engine.change_tx().subscribe();
        let plugin_filters = req.plugin_ids;
        let path_filters = req.path_patterns;
        let tag_filters = req.tags;

        let stream = stream! {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        let matches_plugin = plugin_filters.is_empty()
                            || plugin_filters.contains(&update.plugin_id);
                        let matches_path = path_filters.is_empty()
                            || path_filters.iter().any(|p| update.object_path.starts_with(p));
                        let matches_tag = tag_filters.is_empty()
                            || update.tags_touched.iter().any(|t| tag_filters.contains(t));

                        if matches_plugin && matches_path && matches_tag {
                            yield Ok(proto_state_change(&update));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Subscriber lagged, missed {} updates", n);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn mutate(
        &self,
        request: Request<MutateRequest>,
    ) -> Result<Response<MutateResponse>, Status> {
        let identity = interceptor::bridge_capability_identity(request.extensions());
        let req = request.into_inner();
        let value = prost_value_to_simd(&req.value.unwrap_or_else(|| ProstValue::from(0)));
        let change_type = match req.operation {
            x if x == ProtoOperationType::SetProperty as i32 => ChangeType::PropertySet,
            x if x == ProtoOperationType::CallMethod as i32 => ChangeType::MethodCall,
            x if x == ProtoOperationType::ApplyPatch as i32 => ChangeType::ObjectAdded,
            _ => ChangeType::PropertySet,
        };
        let capability_id = if req.capability_id.is_empty() {
            None
        } else {
            Some(req.capability_id.clone())
        };

        if change_type == ChangeType::MethodCall {
            if let Some(method_name) = req.member_name.strip_prefix("method:").or({
                if req.member_name.is_empty() {
                    None
                } else {
                    Some(req.member_name.as_str())
                }
            }) {
                if let Err(error) = enforce_bridge_capability(
                    &req.plugin_id,
                    method_name,
                    capability_id.as_deref(),
                    identity.as_ref(),
                ) {
                    return Ok(Response::new(MutateResponse {
                        success: false,
                        event_id: 0,
                        event_hash: String::new(),
                        result: None,
                        error: Some(error),
                        effective_hash: String::new(),
                    }));
                }
            }
        }

        let result = self
            .mutation_engine
            .mutate(
                req.plugin_id.clone(),
                req.object_path.clone(),
                change_type,
                if req.member_name.is_empty() {
                    None
                } else {
                    Some(req.member_name.clone())
                },
                value,
                req.actor_id.clone(),
                capability_id,
            )
            .await;

        match result {
            Ok(ok) => Ok(Response::new(MutateResponse {
                success: ok.success,
                event_id: ok.event_id,
                event_hash: ok.event_hash,
                result: ok.result.map(|v| simd_to_prost_value(&v)),
                error: None,
                effective_hash: String::new(),
            })),
            Err(e) => Ok(Response::new(MutateResponse {
                success: false,
                event_id: 0,
                event_hash: String::new(),
                result: None,
                error: Some(ProtoMutationError {
                    code: ProtoErrorCode::Internal as i32,
                    message: e.to_string(),
                    deny_reason: None,
                }),
                effective_hash: String::new(),
            })),
        }
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let req = request.into_inner();
        let state = self.mutation_engine.get_state(&req.plugin_id).await;

        let state_struct = state
            .map(|v| simd_to_prost_struct(&v))
            .unwrap_or_else(ProstStruct::default);

        Ok(Response::new(GetStateResponse {
            state: Some(state_struct),
            effective_hash: String::new(),
            at_event_id: 0,
        }))
    }

    async fn batch_mutate(
        &self,
        request: Request<BatchMutateRequest>,
    ) -> Result<Response<BatchMutateResponse>, Status> {
        let req = request.into_inner();
        let mut results = Vec::new();
        let mut failed_index = -1;

        for (idx, m) in req.mutations.into_iter().enumerate() {
            let mut_req = Request::new(m);
            let resp = self.mutate(mut_req).await?.into_inner();
            if !resp.success && failed_index < 0 && req.atomic {
                failed_index = idx as i32;
                break;
            }
            results.push(resp);
        }

        Ok(Response::new(BatchMutateResponse {
            success: failed_index < 0,
            results,
            failed_index,
        }))
    }
}

// =============================================================================
// PluginService
// =============================================================================

#[tonic::async_trait]
impl PluginService for OperationGrpcServer {
    type SubscribeSignalsStream = Pin<Box<dyn Stream<Item = Result<Signal, Status>> + Send>>;

    async fn list_plugins(
        &self,
        _request: Request<()>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        Ok(Response::new(ListPluginsResponse {
            plugins: self.plugin_provider.list_plugins().await,
        }))
    }

    async fn get_schema(
        &self,
        request: Request<GetSchemaRequest>,
    ) -> Result<Response<GetSchemaResponse>, Status> {
        let req = request.into_inner();
        if let Some((schema_json, dialect, version)) =
            self.plugin_provider.get_schema(&req.plugin_id).await
        {
            Ok(Response::new(GetSchemaResponse {
                schema_json,
                dialect,
                version,
            }))
        } else {
            Ok(Response::new(GetSchemaResponse {
                schema_json: String::new(),
                dialect: String::new(),
                version: String::new(),
            }))
        }
    }

    async fn call_method(
        &self,
        request: Request<CallMethodRequest>,
    ) -> Result<Response<CallMethodResponse>, Status> {
        let identity = interceptor::bridge_capability_identity(request.extensions());
        let req = request.into_inner();
        let capability_id = if req.capability_id.is_empty() {
            None
        } else {
            Some(req.capability_id.clone())
        };
        if let Err(error) = enforce_bridge_capability(
            &req.plugin_id,
            &req.method_name,
            capability_id.as_deref(),
            identity.as_ref(),
        ) {
            return Ok(Response::new(CallMethodResponse {
                success: false,
                result: None,
                event_id: 0,
                event_hash: String::new(),
                error: Some(error),
            }));
        }
        let mut args: Vec<simd_json::OwnedValue> = req
            .arguments
            .into_iter()
            .map(|v| prost_value_to_simd(&v))
            .collect();
        let args = match args.len() {
            0 => simd_json::json!({}),
            1 => args.pop().expect("one argument was counted"),
            _ => simd_json::json!(args),
        };

        let json_args = simd_json::to_string(&args)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;

        // Route through the same schema dispatcher as PluginV1.Call. That
        // dispatcher records the method event before returning its audited
        // envelope; gRPC is a transport adapter, not a second implementation.
        let result = self
            .mutation_engine
            .dispatch_method_call(
                &req.plugin_id,
                &req.method_name,
                &json_args,
                capability_id.as_deref(),
                &req.actor_id,
            )
            .await;

        match result {
            Ok(envelope) => Ok(Response::new(CallMethodResponse {
                success: true,
                result: Some(simd_to_prost_value(
                    &simd_json::serde::to_owned_value(&envelope)
                        .map_err(|error| Status::internal(error.to_string()))?,
                )),
                event_id: envelope
                    .get("event_id")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
                event_hash: envelope
                    .get("event_hash")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                error: None,
            })),
            Err(e) => Ok(Response::new(CallMethodResponse {
                success: false,
                result: None,
                event_id: 0,
                event_hash: String::new(),
                error: Some(ProtoMutationError {
                    code: ProtoErrorCode::Internal as i32,
                    message: e.to_string(),
                    deny_reason: None,
                }),
            })),
        }
    }

    async fn get_property(
        &self,
        request: Request<GetPropertyRequest>,
    ) -> Result<Response<GetPropertyResponse>, Status> {
        let req = request.into_inner();
        let connection = Connection::system()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let proxy = zbus::fdo::PropertiesProxy::builder(&connection)
            .destination(format!("org.opdbus.{}.v1", req.plugin_id))
            .map_err(|e| Status::internal(e.to_string()))?
            .path(req.object_path.as_str())
            .map_err(|e| Status::internal(e.to_string()))?
            .build()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let iface = zbus::names::InterfaceName::try_from(req.interface_name.as_str())
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let val: ZOwnedValue = proxy
            .get(iface, req.property_name.as_str())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let json =
            simd_json::serde::to_owned_value(&val).map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(GetPropertyResponse {
            value: Some(simd_to_prost_value(&json)),
            read_only: false,
        }))
    }

    async fn set_property(
        &self,
        request: Request<SetPropertyRequest>,
    ) -> Result<Response<SetPropertyResponse>, Status> {
        let req = request.into_inner();
        let value = prost_value_to_simd(&req.value.unwrap_or_else(|| ProstValue::from(0)));

        match self
            .mutation_engine
            .mutate(
                req.plugin_id,
                req.object_path,
                ChangeType::PropertySet,
                Some(req.property_name),
                value,
                req.actor_id,
                if req.capability_id.is_empty() {
                    None
                } else {
                    Some(req.capability_id)
                },
            )
            .await
        {
            Ok(ok) => Ok(Response::new(SetPropertyResponse {
                success: ok.success,
                event_id: ok.event_id,
                event_hash: ok.event_hash,
                error: None,
            })),
            Err(e) => Ok(Response::new(SetPropertyResponse {
                success: false,
                event_id: 0,
                event_hash: String::new(),
                error: Some(ProtoMutationError {
                    code: ProtoErrorCode::Internal as i32,
                    message: e.to_string(),
                    deny_reason: None,
                }),
            })),
        }
    }

    async fn subscribe_signals(
        &self,
        request: Request<SubscribeSignalsRequest>,
    ) -> Result<Response<Self::SubscribeSignalsStream>, Status> {
        let req = request.into_inner();
        let plugin_filter = req.plugin_id;
        let signal_names = req.signal_names;
        let path_filter = req.object_path;

        // Subscribe to the schema engine's change broadcast and filter for signals
        let mut rx = self.mutation_engine.change_tx().subscribe();

        let stream = stream! {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        // Only emit Signal change types
                        if update.change_type != ChangeType::Signal {
                            continue;
                        }
                        // Plugin filter
                        if !plugin_filter.is_empty() && update.plugin_id != plugin_filter {
                            continue;
                        }
                        // Path filter
                        if !path_filter.is_empty() && !update.object_path.starts_with(&path_filter) {
                            continue;
                        }
                        // Signal name filter
                        let signal_name = update.member_name.clone().unwrap_or_default();
                        if !signal_names.is_empty() && !signal_names.contains(&signal_name) {
                            continue;
                        }

                        yield Ok(Signal {
                            plugin_id: update.plugin_id.clone(),
                            object_path: update.object_path.clone(),
                            interface_name: String::new(),
                            signal_name,
                            arguments: vec![simd_to_prost_value(&update.new_value)],
                            timestamp: Some(ProstTimestamp {
                                seconds: update.timestamp.timestamp(),
                                nanos: update.timestamp.timestamp_subsec_nanos() as i32,
                            }),
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Signal subscriber lagged, missed {} updates", n);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}

// =============================================================================
// EventChainService
// =============================================================================

#[tonic::async_trait]
impl EventChainService for OperationGrpcServer {
    type SubscribeEventsStream =
        Pin<Box<dyn Stream<Item = Result<ProtoChainEvent, Status>> + Send>>;

    async fn get_events(
        &self,
        request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let req = request.into_inner();
        let chain = self.mutation_engine.event_chain.clone();
        let chain = chain.read().await;

        let events: Vec<ProtoChainEvent> = chain
            .events()
            .iter()
            .filter(|e| req.from_event_id == 0 || e.event_id >= req.from_event_id)
            .filter(|e| req.to_event_id == 0 || e.event_id <= req.to_event_id)
            .filter(|e| req.plugin_id.is_empty() || e.plugin_id == req.plugin_id)
            .filter(|e| req.tags.is_empty() || e.tags_touched.iter().any(|t| req.tags.contains(t)))
            .filter(|e| match req.decision_filter {
                x if x == ProtoDecision::Allow as i32 => e.decision == Decision::Allow,
                x if x == ProtoDecision::Deny as i32 => e.decision == Decision::Deny,
                _ => true,
            })
            .take(if req.limit == 0 {
                usize::MAX
            } else {
                req.limit as usize
            })
            .map(proto_chain_event)
            .collect();

        let has_more = req.limit > 0 && (events.len() as u32) == req.limit;
        Ok(Response::new(GetEventsResponse { events, has_more }))
    }

    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let req = request.into_inner();
        let mut rx = self.chain_events.subscribe();
        let plugin_filter = req.plugin_id;
        let tag_filters = req.tags;

        let stream = stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let matches_plugin = plugin_filter.is_empty() || event.plugin_id == plugin_filter;
                        let matches_tag = tag_filters.is_empty() || event.tags_touched.iter().any(|t| tag_filters.contains(t));
                        if matches_plugin && matches_tag {
                            yield Ok(event);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn verify_chain(
        &self,
        _request: Request<VerifyChainRequest>,
    ) -> Result<Response<VerifyChainResponse>, Status> {
        let chain = self.mutation_engine.event_chain.clone();
        let chain = chain.read().await;
        let result = chain.verify_chain();
        Ok(Response::new(VerifyChainResponse {
            valid: result.valid,
            events_verified: result.events_verified as u64,
            batches_verified: result.batches_verified as u64,
            errors: result.errors,
        }))
    }

    async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let req = request.into_inner();
        let chain = self.mutation_engine.event_chain.clone();
        let chain = chain.read().await;
        let proof: Option<MerkleProof> =
            op_state_store::EventBatch::generate_proof(chain.events(), req.event_id);

        if let Some(proof) = proof {
            let siblings = proof
                .siblings
                .into_iter()
                .map(|(hash, is_right)| MerkleProofSibling { hash, is_right })
                .collect();
            Ok(Response::new(GetProofResponse {
                event_hash: proof.event_hash,
                siblings,
                root: proof.root,
                batch_first_event_id: 0,
                batch_last_event_id: 0,
            }))
        } else {
            Err(Status::not_found("proof not found"))
        }
    }

    async fn prove_tag_immutability(
        &self,
        request: Request<ProveTagImmutabilityRequest>,
    ) -> Result<Response<ProveTagImmutabilityResponse>, Status> {
        let req = request.into_inner();
        let chain = self.mutation_engine.event_chain.clone();
        let chain = chain.read().await;
        let proof = chain.prove_tag_immutability(&req.tag);
        Ok(Response::new(ProveTagImmutabilityResponse {
            tag: proof.tag,
            is_immutable: proof.is_immutable,
            violation_event_ids: proof.violations,
            total_events_checked: proof.total_events_checked as u64,
        }))
    }

    async fn get_snapshot(
        &self,
        request: Request<GetSnapshotRequest>,
    ) -> Result<Response<GetSnapshotResponse>, Status> {
        let req = request.into_inner();
        let chain = self.mutation_engine.event_chain.clone();
        let chain = chain.read().await;
        if let Some(snapshot) = chain.get_snapshot(&req.snapshot_id) {
            Ok(Response::new(GetSnapshotResponse {
                snapshot: Some(proto_snapshot(snapshot)),
            }))
        } else {
            Err(Status::not_found("snapshot not found"))
        }
    }

    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<CreateSnapshotResponse>, Status> {
        let req = request.into_inner();
        let state = self
            .mutation_engine
            .get_state(&req.plugin_id)
            .await
            .unwrap_or_else(|| simd_json::json!({}));
        let chain = self.mutation_engine.event_chain.clone();
        let mut chain = chain.write().await;
        let snapshot = chain.create_snapshot(req.plugin_id, "1.0.0".to_string(), state);
        Ok(Response::new(CreateSnapshotResponse {
            snapshot: Some(proto_snapshot(snapshot)),
        }))
    }

    async fn search_semantic_trace(
        &self,
        request: Request<SearchSemanticTraceRequest>,
    ) -> Result<Response<SearchSemanticTraceResponse>, Status> {
        let shuttle = self.semantic_shuttle.as_ref().ok_or_else(|| {
            Status::failed_precondition(
                "Qdrant Semantic Shuttle is not configured; check Voyage and Qdrant settings",
            )
        })?;
        let req = request.into_inner();
        let limit = u64::from(if req.limit == 0 { 5 } else { req.limit });
        let trace = shuttle.current_trace_context().map_err(internal_status)?;
        let matches = shuttle
            .search_semantic_trace(limit)
            .await
            .map_err(internal_status)?
            .into_iter()
            .map(proto_semantic_trace_match)
            .collect();

        Ok(Response::new(SearchSemanticTraceResponse {
            trace_id: trace.trace_id,
            mutation_index: trace.mutation_index,
            matches,
        }))
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn proto_state_change(change: &crate::mutation_engine::StateChange) -> ProtoStateChange {
    ProtoStateChange {
        change_id: change.change_id.clone(),
        event_id: change.event_id,
        plugin_id: change.plugin_id.clone(),
        object_path: change.object_path.clone(),
        change_type: proto_change_type(change.change_type) as i32,
        member_name: change.member_name.clone().unwrap_or_default(),
        old_value: change.old_value.as_ref().map(simd_to_prost_value),
        new_value: Some(simd_to_prost_value(&change.new_value)),
        tags_touched: change.tags_touched.clone(),
        event_hash: change.event_hash.clone(),
        timestamp: Some(proto_timestamp(change.timestamp)),
        actor_id: change.actor_id.clone(),
    }
}

fn proto_change_type(change_type: ChangeType) -> ProtoChangeType {
    match change_type {
        ChangeType::PropertySet => ProtoChangeType::PropertySet,
        ChangeType::PropertyDelete => ProtoChangeType::PropertyDelete,
        ChangeType::MethodCall => ProtoChangeType::MethodCall,
        ChangeType::Signal => ProtoChangeType::Signal,
        ChangeType::ObjectAdded => ProtoChangeType::ObjectAdded,
        ChangeType::ObjectRemoved => ProtoChangeType::ObjectRemoved,
        ChangeType::SchemaMigration => ProtoChangeType::SchemaMigration,
    }
}

fn proto_chain_event(event: &op_state_store::ChainEvent) -> ProtoChainEvent {
    ProtoChainEvent {
        event_id: event.event_id,
        prev_hash: event.prev_hash.clone(),
        event_hash: event.event_hash.clone(),
        timestamp: Some(proto_timestamp(event.timestamp)),
        actor_id: event.actor_id.clone(),
        capability_id: event.capability_id.clone().unwrap_or_default(),
        plugin_id: event.plugin_id.clone(),
        schema_version: event.schema_version.clone(),
        operation_type: format!("{:?}", event.op),
        target: event.target.clone(),
        tags_touched: event.tags_touched.clone(),
        decision: match event.decision {
            Decision::Allow => ProtoDecision::Allow as i32,
            Decision::Deny => ProtoDecision::Deny as i32,
        },
        deny_reason: event.deny_reason.as_ref().map(proto_deny_reason),
        input_patch_hash: event.input_patch_hash.clone(),
        result_effective_hash: event.result_effective_hash.clone().unwrap_or_default(),
    }
}

fn proto_deny_reason(reason: &DenyReason) -> ProtoDenyReason {
    match reason {
        DenyReason::TagLock { tag, wrapper_id } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::TagLock(ProtoTagLock {
                tag: tag.clone(),
                wrapper_id: wrapper_id.clone(),
            })),
        },
        DenyReason::ConstraintFail {
            constraint,
            message,
        } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::ConstraintFail(
                ProtoConstraintFail {
                    constraint: constraint.clone(),
                    message: message.clone(),
                },
            )),
        },
        DenyReason::CapabilityMissing { capability } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::CapabilityMissing(
                ProtoCapabilityMissing {
                    capability: capability.clone(),
                },
            )),
        },
        DenyReason::ReadOnlyViolation { field } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::ReadOnlyViolation(
                ProtoReadOnlyViolation {
                    field: field.clone(),
                },
            )),
        },
        DenyReason::SchemaValidation { errors } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::ConstraintFail(
                ProtoConstraintFail {
                    constraint: "schema_validation".to_string(),
                    message: errors.join("; "),
                },
            )),
        },
        DenyReason::Custom { reason } => ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::ConstraintFail(
                ProtoConstraintFail {
                    constraint: "custom".to_string(),
                    message: reason.clone(),
                },
            )),
        },
    }
}

fn proto_snapshot(snapshot: &op_state_store::StateSnapshot) -> crate::proto::Snapshot {
    crate::proto::Snapshot {
        snapshot_id: snapshot.snapshot_id.clone(),
        at_event_id: snapshot.at_event_id,
        plugin_id: snapshot.plugin_id.clone(),
        schema_version: snapshot.schema_version.clone(),
        stub_hash: snapshot.stub_hash.clone(),
        immutable_wrappers_hash: snapshot.immutable_wrappers_hash.clone(),
        tunable_patch_hash: snapshot.tunable_patch_hash.clone(),
        effective_hash: snapshot.effective_hash.clone(),
        timestamp: Some(proto_timestamp(snapshot.timestamp)),
        state: Some(simd_to_prost_struct(&snapshot.state)),
    }
}

fn proto_timestamp(ts: DateTime<Utc>) -> ProstTimestamp {
    ProstTimestamp {
        seconds: ts.timestamp(),
        nanos: ts.timestamp_subsec_nanos() as i32,
    }
}

fn proto_semantic_trace_match(point: qdrant_client::qdrant::ScoredPoint) -> SemanticTraceMatch {
    SemanticTraceMatch {
        point_id: qdrant_point_id_to_string(point.id),
        score: point.score,
        payload: Some(qdrant_payload_to_prost_struct(point.payload)),
    }
}

fn qdrant_point_id_to_string(point_id: Option<qdrant_client::qdrant::PointId>) -> String {
    use qdrant_client::qdrant::point_id::PointIdOptions;

    match point_id.and_then(|id| id.point_id_options) {
        Some(PointIdOptions::Num(value)) => value.to_string(),
        Some(PointIdOptions::Uuid(value)) => value,
        None => String::new(),
    }
}

fn qdrant_payload_to_prost_struct(
    payload: std::collections::HashMap<String, qdrant_client::qdrant::Value>,
) -> ProstStruct {
    ProstStruct {
        fields: payload
            .into_iter()
            .map(|(key, value)| (key, qdrant_value_to_prost_value(value)))
            .collect(),
    }
}

fn qdrant_value_to_prost_value(value: qdrant_client::qdrant::Value) -> ProstValue {
    use prost_types::value::Kind as ProstKind;
    use qdrant_client::qdrant::value::Kind as QdrantKind;

    let kind = match value.kind {
        Some(QdrantKind::NullValue(_)) => ProstKind::NullValue(0),
        Some(QdrantKind::DoubleValue(number)) => ProstKind::NumberValue(number),
        Some(QdrantKind::IntegerValue(number)) => ProstKind::NumberValue(number as f64),
        Some(QdrantKind::StringValue(text)) => ProstKind::StringValue(text),
        Some(QdrantKind::BoolValue(flag)) => ProstKind::BoolValue(flag),
        Some(QdrantKind::StructValue(struct_value)) => {
            ProstKind::StructValue(qdrant_payload_to_prost_struct(struct_value.fields))
        }
        Some(QdrantKind::ListValue(list_value)) => ProstKind::ListValue(prost_types::ListValue {
            values: list_value
                .values
                .into_iter()
                .map(qdrant_value_to_prost_value)
                .collect(),
        }),
        None => ProstKind::NullValue(0),
    };

    ProstValue { kind: Some(kind) }
}

fn internal_status(error: anyhow::Error) -> Status {
    Status::internal(error.to_string())
}

fn simd_to_prost_struct(value: &simd_json::OwnedValue) -> ProstStruct {
    match value.as_object() {
        Some(map) => {
            let fields = map
                .iter()
                .map(|(k, v)| (k.to_string(), simd_to_prost_value(v)))
                .collect();
            ProstStruct { fields }
        }
        None => ProstStruct {
            fields: BTreeMap::new(),
        },
    }
}

fn simd_to_prost_value(value: &simd_json::OwnedValue) -> ProstValue {
    use prost_types::value::Kind;
    if value.as_null().is_some() {
        return ProstValue {
            kind: Some(Kind::NullValue(0)),
        };
    }
    if let Some(b) = value.as_bool() {
        return ProstValue {
            kind: Some(Kind::BoolValue(b)),
        };
    }
    // `as_f64()` is strict — it only matches values already stored as f64,
    // so a u64/i64 field (mutation_index, session_started_at, ...) silently
    // fell through every branch to NullValue. `cast_f64()` coerces integers
    // too; google.protobuf.Struct only has a single f64 number_value anyway,
    // so this is the correct behavior, not a precision-losing shortcut.
    if let Some(n) = value.cast_f64() {
        return ProstValue {
            kind: Some(Kind::NumberValue(n)),
        };
    }
    if let Some(s) = value.as_str() {
        return ProstValue {
            kind: Some(Kind::StringValue(s.to_string())),
        };
    }
    if let Some(arr) = value.as_array() {
        let vals = arr.iter().map(simd_to_prost_value).collect();
        return ProstValue {
            kind: Some(Kind::ListValue(prost_types::ListValue { values: vals })),
        };
    }
    if let Some(obj) = value.as_object() {
        let fields = obj
            .iter()
            .map(|(k, v)| (k.to_string(), simd_to_prost_value(v)))
            .collect();
        return ProstValue {
            kind: Some(Kind::StructValue(ProstStruct { fields })),
        };
    }
    ProstValue {
        kind: Some(Kind::NullValue(0)),
    }
}

fn prost_value_to_simd(value: &ProstValue) -> simd_json::OwnedValue {
    use prost_types::value::Kind;
    match &value.kind {
        None => simd_json::json!(null),
        Some(Kind::NullValue(_)) => simd_json::json!(null),
        Some(Kind::BoolValue(b)) => simd_json::json!(*b),
        Some(Kind::NumberValue(n)) => simd_json::json!(*n),
        Some(Kind::StringValue(s)) => simd_json::json!(s),
        Some(Kind::StructValue(s)) => {
            let mut map = simd_json::value::owned::Object::new();
            for (k, v) in &s.fields {
                map.insert(k.clone(), prost_value_to_simd(v));
            }
            simd_json::OwnedValue::Object(Box::new(map))
        }
        Some(Kind::ListValue(l)) => {
            let arr = l.values.iter().map(prost_value_to_simd).collect::<Vec<_>>();
            simd_json::OwnedValue::from(arr)
        }
    }
}

fn simd_json_to_zvariant(value: &simd_json::OwnedValue) -> Result<ZOwnedValue, anyhow::Error> {
    if let Some(obj) = value.as_object() {
        if let (Some(sig_val), Some(inner)) = (obj.get("sig"), obj.get("value")) {
            if let Some(sig) = sig_val.as_str() {
                return zvariant_from_sig(sig, inner);
            }
        }
    }

    if let Some(s) = value.as_str() {
        return Ok(ZOwnedValue::from(ZStr::from(s)));
    }
    if let Some(b) = value.as_bool() {
        return Ok(ZOwnedValue::from(b));
    }
    if let Some(i) = value.as_i64() {
        return Ok(ZOwnedValue::from(i));
    }
    if let Some(u) = value.as_u64() {
        return Ok(ZOwnedValue::from(u));
    }
    if let Some(f) = value.as_f64() {
        return Ok(ZOwnedValue::from(f));
    }

    Err(anyhow::anyhow!(
        "Unsupported argument type; use tagged {{sig,value}} or primitives"
    ))
}

fn zvariant_from_sig(
    sig: &str,
    value: &simd_json::OwnedValue,
) -> Result<ZOwnedValue, anyhow::Error> {
    match sig {
        "s" => value
            .as_str()
            .map(|v| ZOwnedValue::from(ZStr::from(v)))
            .ok_or_else(|| anyhow::anyhow!("Expected string for sig 's'")),
        "b" => value
            .as_bool()
            .map(ZOwnedValue::from)
            .ok_or_else(|| anyhow::anyhow!("Expected bool for sig 'b'")),
        "i" => value
            .as_i64()
            .map(|v| ZOwnedValue::from(v as i32))
            .ok_or_else(|| anyhow::anyhow!("Expected i32 for sig 'i'")),
        "u" => value
            .as_u64()
            .map(|v| ZOwnedValue::from(v as u32))
            .ok_or_else(|| anyhow::anyhow!("Expected u32 for sig 'u'")),
        "x" => value
            .as_i64()
            .map(ZOwnedValue::from)
            .ok_or_else(|| anyhow::anyhow!("Expected i64 for sig 'x'")),
        "t" => value
            .as_u64()
            .map(ZOwnedValue::from)
            .ok_or_else(|| anyhow::anyhow!("Expected u64 for sig 't'")),
        "d" => value
            .as_f64()
            .map(ZOwnedValue::from)
            .ok_or_else(|| anyhow::anyhow!("Expected f64 for sig 'd'")),
        "ay" => {
            let arr = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Expected array for sig 'ay'"))?;
            let bytes: Result<Vec<u8>, anyhow::Error> = arr
                .iter()
                .map(|v| {
                    v.as_u64()
                        .map(|n| n as u8)
                        .ok_or_else(|| anyhow::anyhow!("Expected u8 in ay array"))
                })
                .collect();
            ZOwnedValue::try_from(ZValue::Array(ZArray::from(bytes?)))
                .map_err(|e| anyhow::anyhow!("Array conversion error: {}", e))
        }
        _ => Err(anyhow::anyhow!("Unsupported signature '{}'", sig)),
    }
}

// =============================================================================
// OvsdbMirror Service — RFC 7047 gRPC bridge
// =============================================================================

#[tonic::async_trait]
impl OvsdbMirror for OperationGrpcServer {
    type MonitorStream = Pin<Box<dyn Stream<Item = Result<OvsdbUpdate, Status>> + Send>>;

    async fn list_dbs(
        &self,
        _request: Request<()>,
    ) -> Result<Response<OvsdbListDbsResponse>, Status> {
        let result = self.ovsdb_call("list_dbs", "[]").await?;
        let dbs: Vec<String> = serde_json::from_str(&result)
            .map_err(|e| Status::internal(format!("Parse error: {}", e)))?;
        Ok(Response::new(OvsdbListDbsResponse { databases: dbs }))
    }

    async fn get_schema(
        &self,
        request: Request<OvsdbGetSchemaRequest>,
    ) -> Result<Response<OvsdbGetSchemaResponse>, Status> {
        let db = &request.get_ref().database;
        let db_arg = if db.is_empty() { "Open_vSwitch" } else { db };
        let result = self
            .ovsdb_call("get_schema", &format!("[\"{}\"]", db_arg))
            .await?;
        Ok(Response::new(OvsdbGetSchemaResponse {
            schema_json: result,
            name: db_arg.to_string(),
            version: String::new(),
        }))
    }

    async fn transact(
        &self,
        request: Request<OvsdbTransactRequest>,
    ) -> Result<Response<OvsdbTransactResponse>, Status> {
        let req = request.get_ref();
        let db = if req.database.is_empty() {
            "Open_vSwitch"
        } else {
            &req.database
        };
        let ops = &req.operations_json;
        let call_arg = format!("[\"{}\", {}]", db, ops);
        match self.ovsdb_call("transact", &call_arg).await {
            Ok(result) => Ok(Response::new(OvsdbTransactResponse {
                success: true,
                results_json: result,
                event_id: 0,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(OvsdbTransactResponse {
                success: false,
                results_json: String::new(),
                event_id: 0,
                error: e.message().to_string(),
            })),
        }
    }

    async fn monitor(
        &self,
        request: Request<OvsdbMonitorRequest>,
    ) -> Result<Response<Self::MonitorStream>, Status> {
        let req = request.into_inner();
        let _database = if req.database.is_empty() {
            "Open_vSwitch".to_string()
        } else {
            req.database
        };

        // Subscribe to the schema engine's change broadcast and filter for OVSDB paths
        let mut rx = self.mutation_engine.change_tx().subscribe();

        let stream = stream! {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        // Filter to only OVSDB-related path changes
                        if !update.object_path.starts_with("/org/opdbus/v1/ovsdb") {
                            continue;
                        }

                        // Extract table name and UUID from path:
                        //   /org/opdbus/v1/ovsdb/{table_name}/{uuid}
                        let parts: Vec<&str> = update.object_path.split('/').collect();
                        let (table, uuid) = if parts.len() >= 7 {
                            (parts[5].to_string(), parts[6].to_string())
                        } else {
                            continue;
                        };

                        let new_row = Some(simd_to_prost_struct(&update.new_value));
                        let old_row = update.old_value.as_ref().map(simd_to_prost_struct);

                        yield Ok(OvsdbUpdate {
                            table,
                            uuid,
                            old_row,
                            new_row,
                            timestamp: Some(ProstTimestamp {
                                seconds: update.timestamp.timestamp(),
                                nanos: update.timestamp.timestamp_subsec_nanos() as i32,
                            }),
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("OVSDB monitor subscriber lagged, missed {} updates", n);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn echo(
        &self,
        request: Request<OvsdbEchoRequest>,
    ) -> Result<Response<OvsdbEchoResponse>, Status> {
        Ok(Response::new(OvsdbEchoResponse {
            payload: request.into_inner().payload,
        }))
    }

    async fn dump_db(
        &self,
        request: Request<OvsdbDumpDbRequest>,
    ) -> Result<Response<OvsdbDumpDbResponse>, Status> {
        let db = &request.get_ref().database;
        let db_arg = if db.is_empty() { "Open_vSwitch" } else { db };
        let result = self
            .ovsdb_call("dump", &format!("[\"{}\"]", db_arg))
            .await?;
        Ok(Response::new(OvsdbDumpDbResponse { dump_json: result }))
    }

    async fn get_bridge_state(
        &self,
        request: Request<OvsdbGetBridgeStateRequest>,
    ) -> Result<Response<OvsdbGetBridgeStateResponse>, Status> {
        let filter = &request.get_ref().bridge_name;

        // Query via D-Bus mirror's OvsdbV1 interface
        let dump = self.ovsdb_call("dump", "[\"Open_vSwitch\"]").await?;

        // Parse and build bridge hierarchy
        let bridges = self
            .parse_bridge_hierarchy(&dump, filter)
            .map_err(|e| Status::internal(format!("Parse error: {}", e)))?;

        Ok(Response::new(OvsdbGetBridgeStateResponse { bridges }))
    }
}

impl OperationGrpcServer {
    /// Call an OVSDB method via the D-Bus mirror interface
    async fn ovsdb_call(&self, method: &str, args: &str) -> Result<String, Status> {
        let conn = self
            .mutation_engine
            .dbus_connection()
            .await
            .map_err(|e| Status::unavailable(format!("D-Bus not available: {}", e)))?;

        let proxy = Proxy::new(
            &conn,
            op_core::config::MIRROR_BUS_NAME,
            "/org/opdbus/v1/ovsdb",
            "org.opdbus.OvsdbV1",
        )
        .await
        .map_err(|e| Status::internal(format!("Proxy error: {}", e)))?;

        let result: String = proxy
            .call(method, &(args.to_string(),))
            .await
            .map_err(|e| Status::internal(format!("OVSDB call '{}' failed: {}", method, e)))?;

        Ok(result)
    }

    /// Parse OVSDB dump into Bridge→Port→Interface hierarchy
    fn parse_bridge_hierarchy(
        &self,
        dump_json: &str,
        filter: &str,
    ) -> Result<Vec<ProtoOvsdbBridge>, anyhow::Error> {
        let dump: serde_json::Value = serde_json::from_str(dump_json)?;
        let mut bridges = Vec::new();

        // Get Bridge table rows
        let empty_map = serde_json::Map::new();
        let bridge_rows = dump
            .get("Bridge")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty_map);

        for (_uuid, row) in bridge_rows {
            let name = row
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if !filter.is_empty() && name != filter {
                continue;
            }

            let datapath_type = row
                .get("datapath_type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let fail_mode = row
                .get("fail_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let stp_enable = row
                .get("stp_enable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mcast_snooping_enable = row
                .get("mcast_snooping_enable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Parse ports (OVSDB set format: ["set", [...]] or ["uuid", "..."])
            let port_uuids = self.extract_uuid_set(row.get("ports"));

            let mut ports = Vec::new();
            let empty_port_map = serde_json::Map::new();
            let port_rows = dump
                .get("Port")
                .and_then(|v| v.as_object())
                .unwrap_or(&empty_port_map);

            for port_uuid in &port_uuids {
                if let Some(port_row) = port_rows.get(port_uuid) {
                    let port_name = port_row
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tag = port_row.get("tag").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                    // Parse interfaces
                    let iface_uuids = self.extract_uuid_set(port_row.get("interfaces"));
                    let empty_iface_map = serde_json::Map::new();
                    let iface_rows = dump
                        .get("Interface")
                        .and_then(|v| v.as_object())
                        .unwrap_or(&empty_iface_map);

                    let mut interfaces = Vec::new();
                    for iface_uuid in &iface_uuids {
                        if let Some(iface_row) = iface_rows.get(iface_uuid) {
                            interfaces.push(ProtoOvsdbInterface {
                                name: iface_row
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                r#type: iface_row
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                mac_in_use: iface_row
                                    .get("mac_in_use")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                mac: iface_row
                                    .get("mac")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                admin_state: iface_row
                                    .get("admin_state")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                link_state: iface_row
                                    .get("link_state")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                options: self.extract_map(iface_row.get("options")),
                            });
                        }
                    }

                    ports.push(ProtoOvsdbPort {
                        name: port_name,
                        tag,
                        trunks: vec![],
                        vlan_mode: port_row
                            .get("vlan_mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        bond_mode: port_row
                            .get("bond_mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        interfaces,
                    });
                }
            }

            bridges.push(ProtoOvsdbBridge {
                name,
                datapath_type,
                fail_mode,
                stp_enable,
                mcast_snooping_enable,
                other_config: self.extract_map(row.get("other_config")),
                ports,
            });
        }

        Ok(bridges)
    }

    /// Extract UUID set from OVSDB value (["set", [...]] or ["uuid", "..."])
    fn extract_uuid_set(&self, value: Option<&serde_json::Value>) -> Vec<String> {
        let Some(v) = value else {
            return vec![];
        };
        if let Some(arr) = v.as_array() {
            if arr.len() == 2 {
                if arr[0].as_str() == Some("uuid") {
                    return vec![arr[1].as_str().unwrap_or("").to_string()];
                }
                if arr[0].as_str() == Some("set") {
                    if let Some(items) = arr[1].as_array() {
                        return items
                            .iter()
                            .filter_map(|item| {
                                if let Some(inner) = item.as_array() {
                                    if inner.len() == 2 && inner[0].as_str() == Some("uuid") {
                                        return inner[1].as_str().map(|s| s.to_string());
                                    }
                                }
                                None
                            })
                            .collect();
                    }
                }
            }
        }
        vec![]
    }

    /// Extract OVSDB map (["map", [[k,v], ...]]) to HashMap
    fn extract_map(
        &self,
        value: Option<&serde_json::Value>,
    ) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        let Some(v) = value else {
            return map;
        };
        if let Some(arr) = v.as_array() {
            if arr.len() == 2 && arr[0].as_str() == Some("map") {
                if let Some(pairs) = arr[1].as_array() {
                    for pair in pairs {
                        if let Some(kv) = pair.as_array() {
                            if kv.len() == 2 {
                                let k = kv[0].as_str().unwrap_or("").to_string();
                                let v = kv[1].as_str().unwrap_or("").to_string();
                                map.insert(k, v);
                            }
                        }
                    }
                }
            }
        }
        map
    }
}

// =============================================================================
// RuntimeMirror Service — Live operational state
// =============================================================================

const RUNIT_ACTIVE_DIR: &str = "/etc/runit/runsvdir/default";

fn valid_runit_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

fn parse_runit_pid(status: &str) -> u32 {
    status
        .split("(pid ")
        .nth(1)
        .and_then(|tail| tail.split(')').next())
        .and_then(|pid| pid.parse().ok())
        .unwrap_or(0)
}

async fn runit_service_info(
    name: String,
    path: PathBuf,
) -> Result<ProtoRuntimeServiceInfo, Status> {
    let output = tokio::process::Command::new("sv")
        .arg("status")
        .arg(&path)
        .output()
        .await
        .map_err(|e| Status::internal(format!("sv status {} failed: {}", name, e)))?;
    let description = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let running = description.starts_with("run:");

    Ok(ProtoRuntimeServiceInfo {
        name,
        state: if running { "STARTED" } else { "STOPPED" }.to_string(),
        pid: parse_runit_pid(&description),
        enabled: path.exists(),
        description,
        dependencies: vec![],
        started_at: None,
    })
}

#[tonic::async_trait]
impl RuntimeMirror for OperationGrpcServer {
    type StreamMetricsStream =
        Pin<Box<dyn Stream<Item = Result<RuntimeMetricUpdate, Status>> + Send>>;

    async fn get_system_info(
        &self,
        _request: Request<()>,
    ) -> Result<Response<RuntimeGetSystemInfoResponse>, Status> {
        let hostname = tokio::fs::read_to_string("/etc/hostname")
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let kernel_version = tokio::fs::read_to_string("/proc/version")
            .await
            .unwrap_or_default()
            .split_whitespace()
            .nth(2)
            .unwrap_or("")
            .to_string();
        let uptime_str = tokio::fs::read_to_string("/proc/uptime")
            .await
            .unwrap_or_default();
        let uptime_seconds = uptime_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0) as u64;

        let meminfo = tokio::fs::read_to_string("/proc/meminfo")
            .await
            .unwrap_or_default();
        let mem_total = Self::parse_meminfo_kb(&meminfo, "MemTotal") * 1024;
        let mem_available = Self::parse_meminfo_kb(&meminfo, "MemAvailable") * 1024;
        let mem_used = mem_total.saturating_sub(mem_available);

        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1);
        let arch = std::env::consts::ARCH.to_string();

        let init_system = if Path::new(RUNIT_ACTIVE_DIR).is_dir() {
            "runit"
        } else {
            "unknown"
        }
        .to_string();

        Ok(Response::new(RuntimeGetSystemInfoResponse {
            hostname,
            kernel_version,
            uptime_seconds,
            boot_timestamp: 0,
            cpu_count,
            memory_total_bytes: mem_total,
            memory_available_bytes: mem_available,
            memory_used_bytes: mem_used,
            init_system,
            arch,
            queried_at: Some(ProstTimestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
        }))
    }

    async fn list_services(
        &self,
        _request: Request<RuntimeListServicesRequest>,
    ) -> Result<Response<RuntimeListServicesResponse>, Status> {
        let mut entries = tokio::fs::read_dir(RUNIT_ACTIVE_DIR)
            .await
            .map_err(|e| Status::internal(format!("cannot read runit service directory: {}", e)))?;
        let mut services = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Status::internal(format!("cannot enumerate runit services: {}", e)))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if !valid_runit_service_name(&name) {
                continue;
            }
            services.push(runit_service_info(name, entry.path()).await?);
        }
        services.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(Response::new(RuntimeListServicesResponse { services }))
    }

    async fn get_service(
        &self,
        request: Request<RuntimeGetServiceRequest>,
    ) -> Result<Response<ProtoRuntimeServiceInfo>, Status> {
        let name = request.into_inner().service_name;
        if !valid_runit_service_name(&name) {
            return Err(Status::invalid_argument("invalid runit service name"));
        }
        let path = Path::new(RUNIT_ACTIVE_DIR).join(&name);
        if !path.exists() {
            return Err(Status::not_found(format!(
                "runit service '{}' is not enabled",
                name
            )));
        }
        Ok(Response::new(runit_service_info(name, path).await?))
    }

    async fn stream_metrics(
        &self,
        request: Request<RuntimeStreamMetricsRequest>,
    ) -> Result<Response<Self::StreamMetricsStream>, Status> {
        let interval = std::cmp::max(request.get_ref().interval_seconds, 1) as u64;

        let stream = stream! {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
            loop {
                ticker.tick().await;

                // Read /proc/meminfo
                if let Ok(meminfo) = tokio::fs::read_to_string("/proc/meminfo").await {
                    let total = OperationGrpcServer::parse_meminfo_kb(&meminfo, "MemTotal") * 1024;
                    let available = OperationGrpcServer::parse_meminfo_kb(&meminfo, "MemAvailable") * 1024;
                    yield Ok(RuntimeMetricUpdate {
                        category: "memory".to_string(),
                        name: "used_bytes".to_string(),
                        value: (total - available) as f64,
                        unit: "bytes".to_string(),
                        labels: Default::default(),
                        timestamp: Some(ProstTimestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });
                }
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn list_interfaces(
        &self,
        _request: Request<()>,
    ) -> Result<Response<RuntimeListInterfacesResponse>, Status> {
        // Read from /sys/class/net
        let mut interfaces = Vec::new();
        let mut entries = tokio::fs::read_dir("/sys/class/net")
            .await
            .map_err(|e| Status::internal(format!("Cannot read /sys/class/net: {}", e)))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let base = format!("/sys/class/net/{}", name);

            let mac = tokio::fs::read_to_string(format!("{}/address", base))
                .await
                .unwrap_or_default()
                .trim()
                .to_string();
            let mtu: u32 = tokio::fs::read_to_string(format!("{}/mtu", base))
                .await
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0);
            let ifindex: u32 = tokio::fs::read_to_string(format!("{}/ifindex", base))
                .await
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0);
            let operstate = tokio::fs::read_to_string(format!("{}/operstate", base))
                .await
                .unwrap_or_default()
                .trim()
                .to_uppercase();

            interfaces.push(ProtoRuntimeNetworkInterface {
                name,
                index: ifindex,
                mac_address: mac,
                state: operstate,
                mtu,
                ipv4_addresses: vec![],
                ipv6_addresses: vec![],
                rx_bytes: 0,
                tx_bytes: 0,
                rx_packets: 0,
                tx_packets: 0,
                driver: String::new(),
                speed_mbps: 0,
            });
        }

        Ok(Response::new(RuntimeListInterfacesResponse { interfaces }))
    }

    async fn get_numa_topology(
        &self,
        _request: Request<()>,
    ) -> Result<Response<RuntimeGetNumaTopologyResponse>, Status> {
        let mut nodes = Vec::new();

        // Read /sys/devices/system/node/node*/
        if let Ok(mut entries) = tokio::fs::read_dir("/sys/devices/system/node").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("node") {
                    continue;
                }
                let node_id: u32 = name
                    .strip_prefix("node")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                let meminfo_path = format!("/sys/devices/system/node/{}/meminfo", name);
                let meminfo = tokio::fs::read_to_string(&meminfo_path)
                    .await
                    .unwrap_or_default();
                let mem_total = Self::parse_node_meminfo_kb(&meminfo, "MemTotal") * 1024;
                let mem_free = Self::parse_node_meminfo_kb(&meminfo, "MemFree") * 1024;

                nodes.push(ProtoNumaNode {
                    node_id,
                    cpus: vec![],
                    memory_total_bytes: mem_total,
                    memory_free_bytes: mem_free,
                    memory_used_bytes: mem_total.saturating_sub(mem_free),
                });
            }
        }

        Ok(Response::new(RuntimeGetNumaTopologyResponse { nodes }))
    }
}

impl OperationGrpcServer {
    fn parse_meminfo_kb(meminfo: &str, key: &str) -> u64 {
        meminfo
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .unwrap_or(0)
    }

    fn parse_node_meminfo_kb(meminfo: &str, key: &str) -> u64 {
        meminfo
            .lines()
            .find(|l| l.contains(key))
            .and_then(|l| {
                l.split_whitespace()
                    .rev()
                    .nth(1)
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .unwrap_or(0)
    }

    fn now_ts() -> prost_types::Timestamp {
        let now = Utc::now();
        prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        }
    }
}

// =============================================================================
// ComponentRegistry Service
// =============================================================================

use crate::proto::registry::{
    component_registry_server::ComponentRegistry, ComponentInfo, ComponentStatus,
    DeregisterRequest, DeregisterResponse, DiscoverRequest, DiscoverResponse, GetComponentRequest,
    GetComponentResponse, HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
    RegistryEvent, RegistryEventType, WatchRequest,
};

#[tonic::async_trait]
impl ComponentRegistry for OperationGrpcServer {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<RegistryEvent, Status>> + Send + 'static>>;

    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let req = request.into_inner();
        if req.component_id.is_empty() {
            return Err(Status::invalid_argument("component_id must not be empty"));
        }

        let lease_token = Uuid::new_v4().to_string();
        let now = OperationGrpcServer::now_ts();

        let mut inner = self.registry.write().await;

        let is_update = inner.components.contains_key(&req.component_id);
        let info = ComponentInfo {
            component_id: req.component_id.clone(),
            component_type: req.component_type.clone(),
            name: req.name.clone(),
            description: req.description.clone(),
            schema_json: req.schema_json.clone(),
            metadata: req.metadata.clone(),
            capabilities: req.capabilities.clone(),
            endpoint: req.endpoint.clone(),
            version: req.version.clone(),
            status: ComponentStatus::Active as i32,
            registered_at: Some(now),
            last_heartbeat: Some(now),
        };

        inner
            .components
            .insert(req.component_id.clone(), info.clone());
        inner
            .leases
            .insert(req.component_id.clone(), lease_token.clone());

        let event_type = if is_update {
            RegistryEventType::RegistryEventUpdated
        } else {
            RegistryEventType::RegistryEventRegistered
        };
        let event = RegistryEvent {
            event_type: event_type as i32,
            component: Some(info),
            timestamp: Some(now),
        };
        // Ignore send error — no active watchers is fine.
        let _ = inner.watch_tx.send(event);

        info!(
            component_id = %req.component_id,
            component_type = %req.component_type,
            update = is_update,
            "component registered"
        );

        Ok(Response::new(RegisterResponse {
            success: true,
            message: if is_update {
                "updated".to_string()
            } else {
                "registered".to_string()
            },
            lease_token,
            registered_at: Some(now),
        }))
    }

    async fn deregister(
        &self,
        request: Request<DeregisterRequest>,
    ) -> Result<Response<DeregisterResponse>, Status> {
        let req = request.into_inner();
        let mut inner = self.registry.write().await;

        match inner.leases.get(&req.component_id) {
            None => {
                return Ok(Response::new(DeregisterResponse {
                    success: false,
                    message: "component not found".to_string(),
                }))
            }
            Some(stored) if stored != &req.lease_token => {
                return Err(Status::permission_denied("invalid lease token"))
            }
            _ => {}
        }

        let info = inner.components.remove(&req.component_id);
        inner.leases.remove(&req.component_id);

        if let Some(mut component) = info {
            component.status = ComponentStatus::Deregistered as i32;
            let event = RegistryEvent {
                event_type: RegistryEventType::RegistryEventDeregistered as i32,
                component: Some(component),
                timestamp: Some(OperationGrpcServer::now_ts()),
            };
            let _ = inner.watch_tx.send(event);
        }

        info!(component_id = %req.component_id, "component deregistered");

        Ok(Response::new(DeregisterResponse {
            success: true,
            message: "deregistered".to_string(),
        }))
    }

    async fn discover(
        &self,
        request: Request<DiscoverRequest>,
    ) -> Result<Response<DiscoverResponse>, Status> {
        let req = request.into_inner();
        let inner = self.registry.read().await;

        let components: Vec<ComponentInfo> = inner
            .components
            .values()
            .filter(|c| {
                // Type filter
                if !req.component_type.is_empty() && c.component_type != req.component_type {
                    return false;
                }
                // Capability filter
                if !req.capability.is_empty() && !c.capabilities.contains(&req.capability) {
                    return false;
                }
                // Metadata filter
                if !req.metadata_key.is_empty() {
                    match c.metadata.get(&req.metadata_key) {
                        Some(v) if req.metadata_value.is_empty() || v == &req.metadata_value => {}
                        _ => return false,
                    }
                }
                // Stale filter
                if !req.include_stale && c.status == ComponentStatus::Stale as i32 {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        let total = components.len() as u32;
        Ok(Response::new(DiscoverResponse {
            components,
            total_count: total,
        }))
    }

    async fn get_component(
        &self,
        request: Request<GetComponentRequest>,
    ) -> Result<Response<GetComponentResponse>, Status> {
        let req = request.into_inner();
        let inner = self.registry.read().await;
        let component = inner.components.get(&req.component_id).cloned();
        let found = component.is_some();
        Ok(Response::new(GetComponentResponse { component, found }))
    }

    async fn watch(
        &self,
        request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let req = request.into_inner();
        let type_filter: Vec<String> = req.component_types.clone();

        let inner = self.registry.read().await;
        let mut rx = inner.watch_tx.subscribe();

        // Collect existing components to replay if requested.
        let existing: Vec<ComponentInfo> = if req.include_existing {
            inner.components.values().cloned().collect()
        } else {
            Vec::new()
        };
        drop(inner);

        let output = stream! {
            // Replay existing registrations first.
            for info in existing {
                if type_filter.is_empty() || type_filter.contains(&info.component_type) {
                    yield Ok(RegistryEvent {
                        event_type: RegistryEventType::RegistryEventRegistered as i32,
                        component: Some(info),
                        timestamp: Some(OperationGrpcServer::now_ts()),
                    });
                }
            }
            // Stream live events.
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let passes_filter = type_filter.is_empty()
                            || event
                                .component
                                .as_ref()
                                .map(|c| type_filter.contains(&c.component_type))
                                .unwrap_or(false);
                        if passes_filter {
                            yield Ok(event);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Watch stream lagged — skipping events");
                        // Continue rather than closing the stream.
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(Response::new(Box::pin(output)))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        let mut inner = self.registry.write().await;

        match inner.leases.get(&req.component_id) {
            None => {
                // Component not known — tell it to re-register.
                return Ok(Response::new(HeartbeatResponse {
                    acknowledged: false,
                    reregister_required: true,
                    server_time: Some(OperationGrpcServer::now_ts()),
                }));
            }
            Some(stored) if stored != &req.lease_token => {
                return Err(Status::permission_denied("invalid lease token"))
            }
            _ => {}
        }

        let now = OperationGrpcServer::now_ts();
        let was_stale;
        if let Some(info) = inner.components.get_mut(&req.component_id) {
            was_stale = info.status == ComponentStatus::Stale as i32;
            info.last_heartbeat = Some(now);
            if was_stale {
                info.status = ComponentStatus::Active as i32;
            }
        } else {
            return Ok(Response::new(HeartbeatResponse {
                acknowledged: false,
                reregister_required: true,
                server_time: Some(now),
            }));
        }

        if was_stale {
            if let Some(info) = inner.components.get(&req.component_id).cloned() {
                let event = RegistryEvent {
                    event_type: RegistryEventType::RegistryEventRecovered as i32,
                    component: Some(info),
                    timestamp: Some(now),
                };
                let _ = inner.watch_tx.send(event);
                info!(component_id = %req.component_id, "component recovered from stale");
            }
        }

        debug!(component_id = %req.component_id, "heartbeat acknowledged");

        Ok(Response::new(HeartbeatResponse {
            acknowledged: true,
            reregister_required: false,
            server_time: Some(now),
        }))
    }
}

// =============================================================================
// MailService — Email and Webmail Operations via D-Bus bridge
// =============================================================================

use crate::proto::mail::{
    mail_service_server::MailService, AdminMailActionRequest, AdminMailActionResponse,
    CheckMailServerRequest, CheckMailServerResponse, GetInboxRequest, GetInboxResponse,
    GetMailStatusRequest, GetMailStatusResponse, GetMessageRequest, GetMessageResponse,
    ListMailAccountsRequest, ListMailAccountsResponse, SendEmailRequest, SendEmailResponse,
};

impl OperationGrpcServer {
    /// Call a MailService method via the D-Bus mail interface.
    /// Falls back to MutationEngine state if the D-Bus service is unavailable.
    async fn mail_dbus_call(&self, method: &str, args: &str) -> Result<String, Status> {
        let conn = self
            .mutation_engine
            .dbus_connection()
            .await
            .map_err(|e| Status::unavailable(format!("D-Bus not available: {}", e)))?;

        let proxy = Proxy::new(
            &conn,
            op_core::config::MIRROR_BUS_NAME,
            "/org/opdbus/v1/mail",
            "org.opdbus.MailV1",
        )
        .await
        .map_err(|e| Status::internal(format!("Mail proxy error: {}", e)))?;

        let result: String = proxy
            .call(method, &(args.to_string(),))
            .await
            .map_err(|e| {
                Status::unavailable(format!(
                    "Mail D-Bus service unavailable for '{}': {}",
                    method, e
                ))
            })?;

        Ok(result)
    }
}

#[tonic::async_trait]
impl MailService for OperationGrpcServer {
    async fn send_email(
        &self,
        request: Request<SendEmailRequest>,
    ) -> Result<Response<SendEmailResponse>, Status> {
        let req = request.into_inner();
        info!(
            from = %req.from_email,
            to = %req.to_email,
            subject = %req.subject,
            "gRPC SendEmail"
        );

        // Try D-Bus mail service first
        let args = simd_json::json!({
            "from": req.from_email,
            "to": req.to_email,
            "subject": req.subject,
            "body": req.body,
            "is_html": req.is_html,
            "domain": req.domain
        });
        let args_str = args.to_string();

        match self.mail_dbus_call("send_email", &args_str).await {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(SendEmailResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("sent")
                        .to_string(),
                    message_id: parsed
                        .get("message_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    sent_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_dbus_err) => {
                // Record the send attempt in MutationEngine state
                let mutation_value = simd_json::json!({
                    "from": req.from_email,
                    "to": req.to_email,
                    "subject": req.subject,
                    "status": "queued_no_backend"
                });
                let message_id = Uuid::new_v4().to_string();
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "mail".to_string(),
                        format!("/org/opdbus/v1/mail/outbox/{}", message_id),
                        ChangeType::ObjectAdded,
                        Some("send_email".to_string()),
                        mutation_value,
                        req.from_email.clone(),
                        None,
                    )
                    .await;

                Ok(Response::new(SendEmailResponse {
                    success: false,
                    message: "Mail D-Bus service unavailable; email queued in state store"
                        .to_string(),
                    message_id,
                    sent_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
        }
    }

    async fn get_inbox(
        &self,
        request: Request<GetInboxRequest>,
    ) -> Result<Response<GetInboxResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "email": req.email,
            "domain": req.domain,
            "limit": req.limit,
            "offset": req.offset,
            "folder": req.folder
        });

        match self.mail_dbus_call("get_inbox", &args.to_string()).await {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let messages = parsed
                    .get("messages")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|m| crate::proto::mail::EmailMessage {
                                message_id: m
                                    .get("message_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                from: m
                                    .get("from")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                to: m
                                    .get("to")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                subject: m
                                    .get("subject")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                preview: m
                                    .get("preview")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                is_read: m
                                    .get("is_read")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                has_attachments: m
                                    .get("has_attachments")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                received_at: None,
                                size_bytes: m
                                    .get("size_bytes")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0)
                                    as i32,
                                folder: m
                                    .get("folder")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Response::new(GetInboxResponse {
                    messages,
                    total_count: parsed
                        .get("total_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    unread_count: parsed
                        .get("unread_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    folder: req.folder,
                }))
            }
            Err(_) => {
                // Return empty inbox when backend is unavailable
                Ok(Response::new(GetInboxResponse {
                    messages: vec![],
                    total_count: 0,
                    unread_count: 0,
                    folder: if req.folder.is_empty() {
                        "inbox".to_string()
                    } else {
                        req.folder
                    },
                }))
            }
        }
    }

    async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "message_id": req.message_id,
            "email": req.email,
            "domain": req.domain
        });

        match self.mail_dbus_call("get_message", &args.to_string()).await {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GetMessageResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    header: None,
                    body: parsed
                        .get("body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    is_html: parsed
                        .get("is_html")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    attachments: vec![],
                    raw_content: parsed
                        .get("raw_content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }))
            }
            Err(_) => Err(Status::unavailable(
                "Mail D-Bus service is not available; cannot retrieve message",
            )),
        }
    }

    async fn get_mail_status(
        &self,
        request: Request<GetMailStatusRequest>,
    ) -> Result<Response<GetMailStatusResponse>, Status> {
        let req = request.into_inner();

        // Try reading status from MutationEngine state first
        let state = self.mutation_engine.get_state("mail").await;
        if let Some(ref st) = state {
            if let Some(status_obj) = st.as_object().and_then(|o| o.get("status")) {
                return Ok(Response::new(GetMailStatusResponse {
                    is_configured: status_obj
                        .as_object()
                        .and_then(|o| o.get("is_configured"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    is_running: status_obj
                        .as_object()
                        .and_then(|o| o.get("is_running"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    mail_server_type: "maddy".to_string(),
                    webmail_url: format!("https://mail.{}", req.domain),
                    smtp_status: "unknown".to_string(),
                    imap_status: "unknown".to_string(),
                    total_accounts: 0,
                    total_messages: 0,
                    last_checked: Some(OperationGrpcServer::now_ts()),
                    message: "Status from state store".to_string(),
                }));
            }
        }

        // Attempt D-Bus call
        match self
            .mail_dbus_call(
                "get_status",
                &simd_json::json!({"domain": req.domain}).to_string(),
            )
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GetMailStatusResponse {
                    is_configured: parsed
                        .get("is_configured")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    is_running: parsed
                        .get("is_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    mail_server_type: parsed
                        .get("mail_server_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("maddy")
                        .to_string(),
                    webmail_url: parsed
                        .get("webmail_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    smtp_status: parsed
                        .get("smtp_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    imap_status: parsed
                        .get("imap_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    total_accounts: parsed
                        .get("total_accounts")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    total_messages: parsed
                        .get("total_messages")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    last_checked: Some(OperationGrpcServer::now_ts()),
                    message: "ok".to_string(),
                }))
            }
            Err(_) => Ok(Response::new(GetMailStatusResponse {
                is_configured: false,
                is_running: false,
                mail_server_type: String::new(),
                webmail_url: String::new(),
                smtp_status: "unavailable".to_string(),
                imap_status: "unavailable".to_string(),
                total_accounts: 0,
                total_messages: 0,
                last_checked: Some(OperationGrpcServer::now_ts()),
                message: "Mail D-Bus service is not available".to_string(),
            })),
        }
    }

    async fn list_mail_accounts(
        &self,
        request: Request<ListMailAccountsRequest>,
    ) -> Result<Response<ListMailAccountsResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "domain": req.domain,
            "include_inactive": req.include_inactive
        });

        match self
            .mail_dbus_call("list_accounts", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let accounts: Vec<_> = parsed
                    .get("accounts")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|a| crate::proto::mail::MailAccount {
                                email: a
                                    .get("email")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                user_id: a
                                    .get("user_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                is_admin: a
                                    .get("is_admin")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                is_active: a
                                    .get("is_active")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true),
                                created_at: None,
                                message_count: a
                                    .get("message_count")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32,
                                unread_count: a
                                    .get("unread_count")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32,
                                last_login: a
                                    .get("last_login")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let total = accounts.len() as u32;
                Ok(Response::new(ListMailAccountsResponse {
                    accounts,
                    total_count: total,
                }))
            }
            Err(_) => Ok(Response::new(ListMailAccountsResponse {
                accounts: vec![],
                total_count: 0,
            })),
        }
    }

    async fn admin_mail_action(
        &self,
        request: Request<AdminMailActionRequest>,
    ) -> Result<Response<AdminMailActionResponse>, Status> {
        let req = request.into_inner();
        info!(
            action = %req.action,
            email = %req.email,
            domain = %req.domain,
            "gRPC AdminMailAction"
        );

        let args = simd_json::json!({
            "action": req.action,
            "email": req.email,
            "domain": req.domain
        });

        match self.mail_dbus_call("admin_action", &args.to_string()).await {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(AdminMailActionResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ok")
                        .to_string(),
                    action_id: Uuid::new_v4().to_string(),
                    timestamp: Some(OperationGrpcServer::now_ts()),
                    result: req.parameters,
                }))
            }
            Err(_) => {
                // Record admin action in state store even without backend
                let mutation_value = simd_json::json!({
                    "action": req.action,
                    "email": req.email,
                    "domain": req.domain,
                    "status": "pending_no_backend"
                });
                let action_id = Uuid::new_v4().to_string();
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "mail".to_string(),
                        format!("/org/opdbus/v1/mail/admin_actions/{}", action_id),
                        ChangeType::MethodCall,
                        Some(req.action.clone()),
                        mutation_value,
                        "admin".to_string(),
                        None,
                    )
                    .await;

                Ok(Response::new(AdminMailActionResponse {
                    success: false,
                    message: "Mail D-Bus service unavailable; action recorded in state store"
                        .to_string(),
                    action_id,
                    timestamp: Some(OperationGrpcServer::now_ts()),
                    result: None,
                }))
            }
        }
    }

    async fn check_mail_server(
        &self,
        request: Request<CheckMailServerRequest>,
    ) -> Result<Response<CheckMailServerResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "domain": req.domain,
            "check_smtp": req.check_smtp,
            "check_imap": req.check_imap,
            "check_webmail": req.check_webmail
        });

        match self.mail_dbus_call("check_server", &args.to_string()).await {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(CheckMailServerResponse {
                    all_healthy: parsed
                        .get("all_healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    smtp_healthy: parsed
                        .get("smtp_healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    imap_healthy: parsed
                        .get("imap_healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    webmail_healthy: parsed
                        .get("webmail_healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    smtp_status: parsed
                        .get("smtp_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unchecked")
                        .to_string(),
                    imap_status: parsed
                        .get("imap_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unchecked")
                        .to_string(),
                    webmail_status: parsed
                        .get("webmail_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unchecked")
                        .to_string(),
                    message: "ok".to_string(),
                    issues: parsed
                        .get("issues")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                }))
            }
            Err(_) => Ok(Response::new(CheckMailServerResponse {
                all_healthy: false,
                smtp_healthy: false,
                imap_healthy: false,
                webmail_healthy: false,
                smtp_status: "unavailable".to_string(),
                imap_status: "unavailable".to_string(),
                webmail_status: "unavailable".to_string(),
                message: "Mail D-Bus service is not available".to_string(),
                issues: vec!["Mail D-Bus service (org.opdbus.MailV1) is not running".to_string()],
            })),
        }
    }
}

// =============================================================================
// PrivacyNetworkService — wgcf + OVS + Xray Privacy Infrastructure
// =============================================================================

use crate::proto::privacy::{
    privacy_network_service_server::PrivacyNetworkService, ConfigurePacketRoutingRequest,
    ConfigurePacketRoutingResponse, EnsurePrivacyNetworkRequest, EnsurePrivacyNetworkResponse,
    GenerateWireGuardKeyPairRequest, GenerateWireGuardKeyPairResponse, GetNetworkStatusRequest,
    GetNetworkStatusResponse, GetNetworkTopologyRequest, GetNetworkTopologyResponse,
    GetPrivacyWireGuardConfigRequest, GetPrivacyWireGuardConfigResponse, HealthCheckRequest,
    HealthCheckResponse, ManageComponentRequest, ManageComponentResponse, ProvisionUserRequest,
    ProvisionUserResponse,
};

impl OperationGrpcServer {
    /// Call a PrivacyNetwork method via the D-Bus privacy interface.
    async fn privacy_dbus_call(&self, method: &str, args: &str) -> Result<String, Status> {
        let conn = self
            .mutation_engine
            .dbus_connection()
            .await
            .map_err(|e| Status::unavailable(format!("D-Bus not available: {}", e)))?;

        let proxy = Proxy::new(
            &conn,
            op_core::config::MIRROR_BUS_NAME,
            "/org/opdbus/v1/privacy",
            "org.opdbus.PrivacyV1",
        )
        .await
        .map_err(|e| Status::internal(format!("Privacy proxy error: {}", e)))?;

        let result: String = proxy
            .call(method, &(args.to_string(),))
            .await
            .map_err(|e| {
                Status::unavailable(format!(
                    "Privacy D-Bus service unavailable for '{}': {}",
                    method, e
                ))
            })?;

        Ok(result)
    }
}

#[tonic::async_trait]
impl PrivacyNetworkService for OperationGrpcServer {
    async fn ensure_privacy_network(
        &self,
        request: Request<EnsurePrivacyNetworkRequest>,
    ) -> Result<Response<EnsurePrivacyNetworkResponse>, Status> {
        let req = request.into_inner();
        info!(
            domain = %req.domain,
            force = req.force_reprovision,
            "gRPC EnsurePrivacyNetwork"
        );

        let args = simd_json::json!({
            "domain": req.domain,
            "force_reprovision": req.force_reprovision
        });

        match self
            .privacy_dbus_call("ensure_network", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(EnsurePrivacyNetworkResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("provisioned")
                        .to_string(),
                    bridge_name: parsed
                        .get("bridge_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ovsbr0")
                        .to_string(),
                    wgcf_status: parsed
                        .get("wgcf_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    xray_status: parsed
                        .get("xray_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    active_ports: parsed
                        .get("active_ports")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    provisioned_at: Some(OperationGrpcServer::now_ts()),
                    topology_summary: parsed
                        .get("topology_summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }))
            }
            Err(_) => {
                // Record provisioning intent in state store
                let mutation_value = simd_json::json!({
                    "domain": req.domain,
                    "status": "pending_no_backend",
                    "force_reprovision": req.force_reprovision
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "privacy".to_string(),
                        "/org/opdbus/v1/privacy/network".to_string(),
                        ChangeType::MethodCall,
                        Some("ensure_network".to_string()),
                        mutation_value,
                        "grpc".to_string(),
                        None,
                    )
                    .await;

                Ok(Response::new(EnsurePrivacyNetworkResponse {
                    success: false,
                    message:
                        "Privacy D-Bus service unavailable; provisioning request recorded in state"
                            .to_string(),
                    bridge_name: String::new(),
                    wgcf_status: "unavailable".to_string(),
                    xray_status: "unavailable".to_string(),
                    active_ports: vec![],
                    provisioned_at: Some(OperationGrpcServer::now_ts()),
                    topology_summary: String::new(),
                }))
            }
        }
    }

    async fn get_network_status(
        &self,
        request: Request<GetNetworkStatusRequest>,
    ) -> Result<Response<GetNetworkStatusResponse>, Status> {
        let req = request.into_inner();

        // Check MutationEngine state first
        let state = self.mutation_engine.get_state("privacy").await;
        if let Some(ref st) = state {
            if let Some(net_status) = st.as_object().and_then(|o| o.get("network_status")) {
                let components: Vec<_> = net_status
                    .as_object()
                    .and_then(|o| o.get("components"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|c| crate::proto::privacy::NetworkComponent {
                                name: c
                                    .as_object()
                                    .and_then(|o| o.get("name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                status: c
                                    .as_object()
                                    .and_then(|o| o.get("status"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                r#type: c
                                    .as_object()
                                    .and_then(|o| o.get("type"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                ip_address: String::new(),
                                details: String::new(),
                                critical: false,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                return Ok(Response::new(GetNetworkStatusResponse {
                    healthy: !components.is_empty(),
                    overall_status: "from_state_store".to_string(),
                    components,
                    message: "Status from state store".to_string(),
                    last_updated: Some(OperationGrpcServer::now_ts()),
                }));
            }
        }

        let args = simd_json::json!({"component": req.component});
        match self
            .privacy_dbus_call("get_status", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let components = parsed
                    .get("components")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|c| crate::proto::privacy::NetworkComponent {
                                name: c
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                status: c
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                r#type: c
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                ip_address: c
                                    .get("ip_address")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                details: c
                                    .get("details")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                critical: c
                                    .get("critical")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Response::new(GetNetworkStatusResponse {
                    healthy: parsed
                        .get("healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    overall_status: parsed
                        .get("overall_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    components,
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    last_updated: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => Ok(Response::new(GetNetworkStatusResponse {
                healthy: false,
                overall_status: "unhealthy".to_string(),
                components: vec![],
                message: "Privacy D-Bus service is not available".to_string(),
                last_updated: Some(OperationGrpcServer::now_ts()),
            })),
        }
    }

    async fn provision_user(
        &self,
        request: Request<ProvisionUserRequest>,
    ) -> Result<Response<ProvisionUserResponse>, Status> {
        let req = request.into_inner();
        info!(
            email = %req.email,
            container_type = %req.container_type,
            "gRPC ProvisionUser"
        );

        let args = simd_json::json!({
            "email": req.email,
            "wireguard_public_key": req.wireguard_public_key,
            "is_admin": req.is_admin,
            "domain": req.domain,
            "container_type": req.container_type
        });

        match self
            .privacy_dbus_call("provision_user", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(ProvisionUserResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    user_id: parsed
                        .get("user_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    privacy_config: parsed
                        .get("privacy_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("provisioned")
                        .to_string(),
                    provisioned_at: Some(OperationGrpcServer::now_ts()),
                    xray_endpoint: parsed
                        .get("xray_endpoint")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }))
            }
            Err(_) => {
                // Record provisioning request in state
                let user_id = Uuid::new_v4().to_string();
                let mutation_value = simd_json::json!({
                    "email": req.email,
                    "wireguard_public_key": req.wireguard_public_key,
                    "is_admin": req.is_admin,
                    "domain": req.domain,
                    "container_type": req.container_type,
                    "status": "pending_no_backend"
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "privacy".to_string(),
                        format!("/org/opdbus/v1/privacy/users/{}", user_id),
                        ChangeType::ObjectAdded,
                        Some("provision_user".to_string()),
                        mutation_value,
                        req.email.clone(),
                        None,
                    )
                    .await;

                Ok(Response::new(ProvisionUserResponse {
                    success: false,
                    user_id,
                    assigned_ip: String::new(),
                    privacy_config: String::new(),
                    message: "Privacy D-Bus service unavailable; provisioning request recorded"
                        .to_string(),
                    provisioned_at: Some(OperationGrpcServer::now_ts()),
                    xray_endpoint: String::new(),
                }))
            }
        }
    }

    async fn get_privacy_wire_guard_config(
        &self,
        request: Request<GetPrivacyWireGuardConfigRequest>,
    ) -> Result<Response<GetPrivacyWireGuardConfigResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "email": req.email,
            "user_id": req.user_id,
            "include_xray": req.include_xray
        });

        match self
            .privacy_dbus_call("get_wireguard_config", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GetPrivacyWireGuardConfigResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    wireguard_config: parsed
                        .get("wireguard_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    public_key: parsed
                        .get("public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    endpoint: parsed
                        .get("endpoint")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    dns_servers: parsed
                        .get("dns_servers")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: "ok".to_string(),
                    generated_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => Err(Status::unavailable(
                "Privacy D-Bus service is not available; cannot retrieve WireGuard config",
            )),
        }
    }

    async fn manage_component(
        &self,
        request: Request<ManageComponentRequest>,
    ) -> Result<Response<ManageComponentResponse>, Status> {
        let req = request.into_inner();
        info!(
            action = %req.action,
            component = %req.component,
            "gRPC ManageComponent"
        );

        let args = simd_json::json!({
            "action": req.action,
            "component": req.component
        });

        match self
            .privacy_dbus_call("manage_component", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(ManageComponentResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ok")
                        .to_string(),
                    component: req.component,
                    status: parsed
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    output: parsed
                        .get("output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    completed_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => {
                let mutation_value = simd_json::json!({
                    "action": req.action,
                    "component": req.component,
                    "status": "pending_no_backend"
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "privacy".to_string(),
                        format!("/org/opdbus/v1/privacy/components/{}", req.component),
                        ChangeType::MethodCall,
                        Some("manage_component".to_string()),
                        mutation_value,
                        "grpc".to_string(),
                        None,
                    )
                    .await;

                Ok(Response::new(ManageComponentResponse {
                    success: false,
                    message: "Privacy D-Bus service unavailable; action recorded in state"
                        .to_string(),
                    component: req.component,
                    status: "unavailable".to_string(),
                    output: String::new(),
                    completed_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
        }
    }

    async fn get_network_topology(
        &self,
        request: Request<GetNetworkTopologyRequest>,
    ) -> Result<Response<GetNetworkTopologyResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({"include_details": req.include_details});

        match self
            .privacy_dbus_call("get_topology", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let routes = parsed
                    .get("routes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|r| crate::proto::privacy::NetworkRoute {
                                destination: r
                                    .get("destination")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                gateway: r
                                    .get("gateway")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                device: r
                                    .get("device")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                metric: r
                                    .get("metric")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let proxy_configs = parsed
                    .get("proxy_configs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|p| crate::proto::privacy::ProxyConfig {
                                container_name: p
                                    .get("container_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                container_type: p
                                    .get("container_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                http_proxy_enabled: p
                                    .get("http_proxy_enabled")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                grpc_proxy_enabled: p
                                    .get("grpc_proxy_enabled")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                http_port: p.get("http_port").and_then(|v| v.as_u64()).unwrap_or(0)
                                    as u32,
                                grpc_port: p.get("grpc_port").and_then(|v| v.as_u64()).unwrap_or(0)
                                    as u32,
                                proxy_mode: p
                                    .get("proxy_mode")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Response::new(GetNetworkTopologyResponse {
                    bridge_name: parsed
                        .get("bridge_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    wgcf_status: parsed
                        .get("wgcf_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    ports: parsed
                        .get("ports")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    management_ip: parsed
                        .get("management_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("10.200.0.1")
                        .to_string(),
                    xray_config: parsed
                        .get("xray_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    routes,
                    topology_data: None,
                    summary: parsed
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    proxy_configs,
                }))
            }
            Err(_) => Ok(Response::new(GetNetworkTopologyResponse {
                bridge_name: String::new(),
                wgcf_status: "unavailable".to_string(),
                ports: vec![],
                management_ip: String::new(),
                xray_config: String::new(),
                routes: vec![],
                topology_data: None,
                summary: "Privacy D-Bus service is not available".to_string(),
                proxy_configs: vec![],
            })),
        }
    }

    async fn health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "check_wgcf": req.check_wgcf,
            "check_ovs": req.check_ovs,
            "check_xray": req.check_xray,
            "check_ports": req.check_ports
        });

        match self
            .privacy_dbus_call("health_check", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let issues = parsed
                    .get("issues")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|i| crate::proto::privacy::HealthIssue {
                                component: i
                                    .get("component")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                severity: i
                                    .get("severity")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("warning")
                                    .to_string(),
                                message: i
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                suggested_fix: i
                                    .get("suggested_fix")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Response::new(HealthCheckResponse {
                    all_healthy: parsed
                        .get("all_healthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    healthy_components: parsed
                        .get("healthy_components")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    total_components: parsed
                        .get("total_components")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    issues,
                    overall_status: parsed
                        .get("overall_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    checked_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => Ok(Response::new(HealthCheckResponse {
                all_healthy: false,
                healthy_components: 0,
                total_components: 0,
                issues: vec![crate::proto::privacy::HealthIssue {
                    component: "dbus".to_string(),
                    severity: "critical".to_string(),
                    message: "Privacy D-Bus service (org.opdbus.PrivacyV1) is not running"
                        .to_string(),
                    suggested_fix: "Start the privacy-router runit service".to_string(),
                }],
                overall_status: "unhealthy".to_string(),
                checked_at: Some(OperationGrpcServer::now_ts()),
            })),
        }
    }

    async fn configure_packet_routing(
        &self,
        request: Request<ConfigurePacketRoutingRequest>,
    ) -> Result<Response<ConfigurePacketRoutingResponse>, Status> {
        let req = request.into_inner();
        info!(
            container = %req.container_name,
            container_type = %req.container_type,
            "gRPC ConfigurePacketRouting"
        );

        let args = simd_json::json!({
            "container_name": req.container_name,
            "container_type": req.container_type,
            "enable_http_proxy": req.enable_http_proxy,
            "enable_grpc_proxy": req.enable_grpc_proxy,
            "proxy_type": req.proxy_type,
            "socks_port": req.socks_port,
            "http_port": req.http_port,
            "enable_tproxy": req.enable_tproxy
        });

        match self
            .privacy_dbus_call("configure_routing", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(ConfigurePacketRoutingResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("configured")
                        .to_string(),
                    container_name: req.container_name,
                    proxy_config_summary: parsed
                        .get("proxy_config_summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    configured_at: Some(OperationGrpcServer::now_ts()),
                    applied_rules: parsed
                        .get("applied_rules")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                }))
            }
            Err(_) => {
                let mutation_value = simd_json::json!({
                    "container_name": req.container_name,
                    "container_type": req.container_type,
                    "proxy_type": req.proxy_type,
                    "status": "pending_no_backend"
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "privacy".to_string(),
                        format!("/org/opdbus/v1/privacy/routing/{}", req.container_name),
                        ChangeType::MethodCall,
                        Some("configure_routing".to_string()),
                        mutation_value,
                        "grpc".to_string(),
                        None,
                    )
                    .await;

                Ok(Response::new(ConfigurePacketRoutingResponse {
                    success: false,
                    message: "Privacy D-Bus service unavailable; routing config recorded in state"
                        .to_string(),
                    container_name: req.container_name,
                    proxy_config_summary: String::new(),
                    configured_at: Some(OperationGrpcServer::now_ts()),
                    applied_rules: vec![],
                }))
            }
        }
    }

    async fn generate_wire_guard_key_pair(
        &self,
        request: Request<GenerateWireGuardKeyPairRequest>,
    ) -> Result<Response<GenerateWireGuardKeyPairResponse>, Status> {
        let req = request.into_inner();
        info!(
            email = %req.user_email,
            container_type = %req.container_type,
            "gRPC GenerateWireGuardKeyPair"
        );

        let args = simd_json::json!({
            "user_token": req.user_token,
            "user_email": req.user_email,
            "is_admin": req.is_admin,
            "container_type": req.container_type
        });

        match self
            .privacy_dbus_call("generate_keypair", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GenerateWireGuardKeyPairResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    client_public_key: parsed
                        .get("client_public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    wireguard_config: parsed
                        .get("wireguard_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    key_id: parsed
                        .get("key_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: "ok".to_string(),
                    generated_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => Err(Status::unavailable(
                "Privacy D-Bus service is not available; cannot generate WireGuard keypair without backend",
            )),
        }
    }
}

// =============================================================================
// RegistrationService — Magic Link + WireGuard Identity Management
// =============================================================================

use crate::proto::registration::{
    registration_service_server::RegistrationService, AdminUserActionRequest,
    AdminUserActionResponse, GetUserStatusRequest, GetUserStatusResponse,
    GetWireGuardConfigRequest, GetWireGuardConfigResponse, ListUsersRequest, ListUsersResponse,
    RegisterUserRequest, RegisterUserResponse, SendMagicLinkRequest, SendMagicLinkResponse,
    VerifyMagicLinkRequest, VerifyMagicLinkResponse,
};

impl OperationGrpcServer {
    /// Call a RegistrationService method via the D-Bus registration interface.
    async fn registration_dbus_call(&self, method: &str, args: &str) -> Result<String, Status> {
        let conn = self
            .mutation_engine
            .dbus_connection()
            .await
            .map_err(|e| Status::unavailable(format!("D-Bus not available: {}", e)))?;

        let proxy = Proxy::new(
            &conn,
            op_core::config::MIRROR_BUS_NAME,
            "/org/opdbus/v1/registration",
            "org.opdbus.RegistrationV1",
        )
        .await
        .map_err(|e| Status::internal(format!("Registration proxy error: {}", e)))?;

        let result: String = proxy
            .call(method, &(args.to_string(),))
            .await
            .map_err(|e| {
                Status::unavailable(format!(
                    "Registration D-Bus service unavailable for '{}': {}",
                    method, e
                ))
            })?;

        Ok(result)
    }
}

#[tonic::async_trait]
impl RegistrationService for OperationGrpcServer {
    async fn send_magic_link(
        &self,
        request: Request<SendMagicLinkRequest>,
    ) -> Result<Response<SendMagicLinkResponse>, Status> {
        let req = request.into_inner();
        info!(
            email = %req.email,
            domain = %req.domain,
            is_admin = req.is_admin,
            "gRPC SendMagicLink"
        );

        let args = simd_json::json!({
            "email": req.email,
            "domain": req.domain,
            "is_admin": req.is_admin
        });

        match self
            .registration_dbus_call("send_magic_link", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(SendMagicLinkResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("magic link sent")
                        .to_string(),
                    token: parsed
                        .get("token")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    expires_at: Some(ProstTimestamp {
                        seconds: chrono::Utc::now().timestamp() + 3600, // 1 hour expiry
                        nanos: 0,
                    }),
                }))
            }
            Err(_) => {
                // Generate a token and record in state store for later verification
                let token = Uuid::new_v4().to_string();
                let mutation_value = simd_json::json!({
                    "email": req.email,
                    "domain": req.domain,
                    "is_admin": req.is_admin,
                    "token": token,
                    "status": "pending_no_backend",
                    "created_at": chrono::Utc::now().to_rfc3339()
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "registration".to_string(),
                        format!("/org/opdbus/v1/registration/magic_links/{}", token),
                        ChangeType::ObjectAdded,
                        Some("send_magic_link".to_string()),
                        mutation_value,
                        req.email.clone(),
                        None,
                    )
                    .await;

                Ok(Response::new(SendMagicLinkResponse {
                    success: false,
                    message:
                        "Registration D-Bus service unavailable; magic link recorded in state store"
                            .to_string(),
                    token: Some(token),
                    expires_at: Some(ProstTimestamp {
                        seconds: chrono::Utc::now().timestamp() + 3600,
                        nanos: 0,
                    }),
                }))
            }
        }
    }

    async fn verify_magic_link(
        &self,
        request: Request<VerifyMagicLinkRequest>,
    ) -> Result<Response<VerifyMagicLinkResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "token": req.token,
            "domain": req.domain
        });

        match self
            .registration_dbus_call("verify_magic_link", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(VerifyMagicLinkResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    user_id: parsed
                        .get("user_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    email: parsed
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    wireguard_public_key: parsed
                        .get("wireguard_public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    wireguard_config: parsed
                        .get("wireguard_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("verified")
                        .to_string(),
                    is_admin: parsed
                        .get("is_admin")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    verified_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => {
                // Check state store for magic link token
                let state = self.mutation_engine.get_state("registration").await;
                if let Some(ref st) = state {
                    if let Some(link_data) = st
                        .as_object()
                        .and_then(|o| o.get("magic_links"))
                        .and_then(|o| o.as_object())
                        .and_then(|o| o.get(&req.token))
                    {
                        let email = link_data
                            .as_object()
                            .and_then(|o| o.get("email"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_admin = link_data
                            .as_object()
                            .and_then(|o| o.get("is_admin"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        return Ok(Response::new(VerifyMagicLinkResponse {
                            success: true,
                            user_id: Uuid::new_v4().to_string(),
                            email,
                            wireguard_public_key: String::new(),
                            assigned_ip: String::new(),
                            wireguard_config: String::new(),
                            message: "Verified from state store (D-Bus unavailable)".to_string(),
                            is_admin,
                            verified_at: Some(OperationGrpcServer::now_ts()),
                        }));
                    }
                }

                Err(Status::unavailable(
                    "Registration D-Bus service is not available; token not found in state store",
                ))
            }
        }
    }

    async fn register_user(
        &self,
        request: Request<RegisterUserRequest>,
    ) -> Result<Response<RegisterUserResponse>, Status> {
        let req = request.into_inner();
        info!(
            email = %req.email,
            domain = %req.domain,
            is_admin = req.is_admin,
            "gRPC RegisterUser"
        );

        let args = simd_json::json!({
            "email": req.email,
            "wireguard_public_key": req.wireguard_public_key,
            "domain": req.domain,
            "is_admin": req.is_admin
        });

        match self
            .registration_dbus_call("register_user", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(RegisterUserResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    user_id: parsed
                        .get("user_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("registered")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    wireguard_config: parsed
                        .get("wireguard_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    registered_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => {
                let user_id = Uuid::new_v4().to_string();
                let mutation_value = simd_json::json!({
                    "email": req.email,
                    "wireguard_public_key": req.wireguard_public_key,
                    "domain": req.domain,
                    "is_admin": req.is_admin,
                    "status": "pending_no_backend"
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "registration".to_string(),
                        format!("/org/opdbus/v1/registration/users/{}", user_id),
                        ChangeType::ObjectAdded,
                        Some("register_user".to_string()),
                        mutation_value,
                        req.email.clone(),
                        None,
                    )
                    .await;

                Ok(Response::new(RegisterUserResponse {
                    success: false,
                    user_id,
                    message: "Registration D-Bus service unavailable; user recorded in state store"
                        .to_string(),
                    assigned_ip: String::new(),
                    wireguard_config: String::new(),
                    registered_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
        }
    }

    async fn get_user_status(
        &self,
        request: Request<GetUserStatusRequest>,
    ) -> Result<Response<GetUserStatusResponse>, Status> {
        let req = request.into_inner();

        // Check state store first
        let state = self.mutation_engine.get_state("registration").await;
        if let Some(ref st) = state {
            if let Some(users) = st.as_object().and_then(|o| o.get("users")) {
                // Search by email or user_id
                if let Some(user_data) = users.as_object().and_then(|o| {
                    // Try user_id first
                    if !req.user_id.is_empty() {
                        o.get(&req.user_id)
                    } else {
                        // Search by email
                        o.iter().find_map(|(_, v)| {
                            if v.as_object()
                                .and_then(|u| u.get("email"))
                                .and_then(|e| e.as_str())
                                == Some(&req.email)
                            {
                                Some(v)
                            } else {
                                None
                            }
                        })
                    }
                }) {
                    return Ok(Response::new(GetUserStatusResponse {
                        registered: true,
                        user_id: user_data
                            .as_object()
                            .and_then(|o| o.get("user_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(&req.user_id)
                            .to_string(),
                        email: user_data
                            .as_object()
                            .and_then(|o| o.get("email"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(&req.email)
                            .to_string(),
                        email_verified: user_data
                            .as_object()
                            .and_then(|o| o.get("email_verified"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        wireguard_public_key: user_data
                            .as_object()
                            .and_then(|o| o.get("wireguard_public_key"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        assigned_ip: user_data
                            .as_object()
                            .and_then(|o| o.get("assigned_ip"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        is_admin: user_data
                            .as_object()
                            .and_then(|o| o.get("is_admin"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        registered_at: None,
                        last_active: None,
                    }));
                }
            }
        }

        let args = simd_json::json!({
            "email": req.email,
            "user_id": req.user_id,
            "domain": req.domain
        });

        match self
            .registration_dbus_call("get_user_status", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GetUserStatusResponse {
                    registered: parsed
                        .get("registered")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    user_id: parsed
                        .get("user_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    email: parsed
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    email_verified: parsed
                        .get("email_verified")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    wireguard_public_key: parsed
                        .get("wireguard_public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    is_admin: parsed
                        .get("is_admin")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    registered_at: None,
                    last_active: None,
                }))
            }
            Err(_) => Ok(Response::new(GetUserStatusResponse {
                registered: false,
                user_id: String::new(),
                email: req.email,
                email_verified: false,
                wireguard_public_key: String::new(),
                assigned_ip: String::new(),
                is_admin: false,
                registered_at: None,
                last_active: None,
            })),
        }
    }

    async fn list_users(
        &self,
        request: Request<ListUsersRequest>,
    ) -> Result<Response<ListUsersResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "limit": req.limit,
            "offset": req.offset,
            "include_admins_only": req.include_admins_only,
            "domain_filter": req.domain_filter
        });

        match self
            .registration_dbus_call("list_users", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                let users: Vec<_> = parsed
                    .get("users")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|u| crate::proto::registration::UserInfo {
                                user_id: u
                                    .get("user_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                email: u
                                    .get("email")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                email_verified: u
                                    .get("email_verified")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                wireguard_public_key: u
                                    .get("wireguard_public_key")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                assigned_ip: u
                                    .get("assigned_ip")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                is_admin: u
                                    .get("is_admin")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                                registered_at: None,
                                last_active: None,
                                metadata: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let total = parsed
                    .get("total_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(users.len() as u64) as u32;
                let filtered = parsed
                    .get("filtered_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(users.len() as u64) as u32;

                Ok(Response::new(ListUsersResponse {
                    users,
                    total_count: total,
                    filtered_count: filtered,
                }))
            }
            Err(_) => Ok(Response::new(ListUsersResponse {
                users: vec![],
                total_count: 0,
                filtered_count: 0,
            })),
        }
    }

    async fn get_wire_guard_config(
        &self,
        request: Request<GetWireGuardConfigRequest>,
    ) -> Result<Response<GetWireGuardConfigResponse>, Status> {
        let req = request.into_inner();
        let args = simd_json::json!({
            "email": req.email,
            "user_id": req.user_id,
            "domain": req.domain
        });

        match self
            .registration_dbus_call("get_wireguard_config", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(GetWireGuardConfigResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    wireguard_config: parsed
                        .get("wireguard_config")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    public_key: parsed
                        .get("public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assigned_ip: parsed
                        .get("assigned_ip")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    message: "ok".to_string(),
                    generated_at: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => Err(Status::unavailable(
                "Registration D-Bus service is not available; cannot retrieve WireGuard config",
            )),
        }
    }

    async fn admin_user_action(
        &self,
        request: Request<AdminUserActionRequest>,
    ) -> Result<Response<AdminUserActionResponse>, Status> {
        let req = request.into_inner();
        info!(
            action = %req.action,
            user_id = %req.user_id,
            email = %req.email,
            "gRPC AdminUserAction"
        );

        let args = simd_json::json!({
            "action": req.action,
            "user_id": req.user_id,
            "email": req.email
        });

        match self
            .registration_dbus_call("admin_user_action", &args.to_string())
            .await
        {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
                Ok(Response::new(AdminUserActionResponse {
                    success: parsed
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    message: parsed
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ok")
                        .to_string(),
                    user_id: req.user_id.clone(),
                    action_timestamp: Some(OperationGrpcServer::now_ts()),
                }))
            }
            Err(_) => {
                let mutation_value = simd_json::json!({
                    "action": req.action,
                    "user_id": req.user_id,
                    "email": req.email,
                    "status": "pending_no_backend"
                });
                let _ = self
                    .mutation_engine
                    .process_grpc_mutation(
                        "registration".to_string(),
                        format!(
                            "/org/opdbus/v1/registration/admin_actions/{}",
                            Uuid::new_v4()
                        ),
                        ChangeType::MethodCall,
                        Some(req.action.clone()),
                        mutation_value,
                        "admin".to_string(),
                        None,
                    )
                    .await;

                Ok(Response::new(AdminUserActionResponse {
                    success: false,
                    message:
                        "Registration D-Bus service unavailable; action recorded in state store"
                            .to_string(),
                    user_id: req.user_id,
                    action_timestamp: Some(OperationGrpcServer::now_ts()),
                }))
            }
        }
    }
}

// =============================================================================
// D-Bus Passthrough Service
// =============================================================================

#[tonic::async_trait]
impl crate::proto::dbus_passthrough_server::DbusPassthrough for OperationGrpcServer {
    type WatchStream =
        Pin<Box<dyn Stream<Item = Result<crate::proto::DbusSignalEvent, Status>> + Send>>;

    async fn call(
        &self,
        request: Request<crate::proto::DbusCallRequest>,
    ) -> Result<Response<crate::proto::DbusCallResponse>, Status> {
        let identity = interceptor::bridge_capability_identity(request.extensions());
        let req = request.into_inner();
        if let Some(plugin_id) = plugin_id_from_dbus_path(&req.path) {
            if let Err(error) =
                enforce_bridge_capability(&plugin_id, &req.method, None, identity.as_ref())
            {
                return Err(Status::permission_denied(error.message));
            }
        }
        let conn = self
            .mutation_engine
            .dbus_connection()
            .await
            .map_err(|e| Status::unavailable(format!("D-Bus not available: {e}")))?;

        let bus_conn = match req.bus.as_str() {
            "session" => {
                let session = Connection::session()
                    .await
                    .map_err(|e| Status::unavailable(format!("session bus unavailable: {e}")))?;
                session
            }
            _ => conn,
        };

        let proxy = Proxy::new(
            &bus_conn,
            req.destination.clone(),
            req.path.clone(),
            req.interface.clone(),
        )
        .await
        .map_err(|e| Status::internal(format!("proxy build failed: {e}")))?;

        let result: String = proxy
            .call(req.method.clone(), &(req.json_body.clone(),))
            .await
            .map_err(|e| Status::internal(format!("D-Bus call '{}' failed: {e}", req.method)))?;

        Ok(Response::new(crate::proto::DbusCallResponse {
            success: true,
            json_result: result,
            error: String::new(),
        }))
    }

    async fn get(
        &self,
        request: Request<crate::proto::DbusGetPropertyRequest>,
    ) -> Result<Response<crate::proto::DbusGetPropertyResponse>, Status> {
        let req = request.into_inner();
        let bus_conn = match req.bus.as_str() {
            "session" => Connection::session()
                .await
                .map_err(|e| Status::unavailable(format!("session bus: {e}")))?,
            _ => Connection::system()
                .await
                .map_err(|e| Status::unavailable(format!("system bus: {e}")))?,
        };

        let props = zbus::fdo::PropertiesProxy::builder(&bus_conn)
            .destination(req.destination.clone())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .path(req.path.as_str())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .build()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let iface = zbus::names::InterfaceName::try_from(req.interface.as_str())
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let val: ZOwnedValue = props
            .get(iface, &req.property)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let json = serde_json::to_string(&val).map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(crate::proto::DbusGetPropertyResponse {
            success: true,
            json_value: json,
            error: String::new(),
        }))
    }

    async fn set(
        &self,
        request: Request<crate::proto::DbusSetPropertyRequest>,
    ) -> Result<Response<crate::proto::DbusSetPropertyResponse>, Status> {
        let identity = interceptor::bridge_capability_identity(request.extensions());
        let req = request.into_inner();
        if let Some(plugin_id) = plugin_id_from_dbus_path(&req.path) {
            let setter = format!("set_{}", req.property);
            if let Err(error) =
                enforce_bridge_capability(&plugin_id, &setter, None, identity.as_ref())
            {
                return Err(Status::permission_denied(error.message));
            }
        }
        let bus_conn = match req.bus.as_str() {
            "session" => Connection::session()
                .await
                .map_err(|e| Status::unavailable(format!("session bus: {e}")))?,
            _ => Connection::system()
                .await
                .map_err(|e| Status::unavailable(format!("system bus: {e}")))?,
        };

        let props = zbus::fdo::PropertiesProxy::builder(&bus_conn)
            .destination(req.destination.clone())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .path(req.path.as_str())
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .build()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let iface = zbus::names::InterfaceName::try_from(req.interface.as_str())
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let value: serde_json::Value = serde_json::from_str(&req.json_value)
            .map_err(|e| Status::invalid_argument(format!("bad JSON: {e}")))?;
        let zval = simd_json_to_zvariant(
            &simd_json::serde::to_owned_value(&value)
                .map_err(|e| Status::internal(e.to_string()))?,
        )
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

        props
            .set(iface, &req.property, zval.into())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(crate::proto::DbusSetPropertyResponse {
            success: true,
            error: String::new(),
        }))
    }

    async fn watch(
        &self,
        request: Request<crate::proto::DbusWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let req = request.into_inner();
        let bus_conn = match req.bus.as_str() {
            "session" => Connection::session()
                .await
                .map_err(|e| Status::unavailable(format!("session bus: {e}")))?,
            _ => Connection::system()
                .await
                .map_err(|e| Status::unavailable(format!("system bus: {e}")))?,
        };

        let proxy = Proxy::new(
            &bus_conn,
            req.destination.clone(),
            req.path.clone(),
            req.interface.clone(),
        )
        .await
        .map_err(|e| Status::internal(format!("proxy build failed: {e}")))?;

        // zbus v5: receive_all_signals() or receive_signal(name) -> SignalStream
        // SignalStream implements futures::Stream<Item = Message>
        let mut sig_stream = if req.signal_names.is_empty() {
            proxy
                .receive_all_signals()
                .await
                .map_err(|e| Status::internal(format!("signal subscribe failed: {e}")))?
        } else {
            // Subscribe to the first named signal; filter others in-stream
            let sig_name = zbus::names::MemberName::try_from(req.signal_names[0].clone())
                .map_err(|e| Status::invalid_argument(format!("invalid signal name: {e}")))?;
            proxy
                .receive_signal(sig_name)
                .await
                .map_err(|e| Status::internal(format!("signal subscribe failed: {e}")))?
        };

        let signal_names = req.signal_names.clone();
        let stream = stream! {
            while let Some(msg) = sig_stream.next().await {
                let hdr = msg.header();
                let member = hdr.member().map(|m| m.as_str().to_string()).unwrap_or_default();
                if !signal_names.is_empty() && !signal_names.contains(&member) { continue; }
                let iface = hdr.interface().map(|i| i.as_str().to_string()).unwrap_or_default();
                let path = hdr.path().map(|p| p.as_str().to_string()).unwrap_or_default();
                let body: String = msg.body()
                    .deserialize::<zbus::zvariant::OwnedValue>()
                    .map(|b| format!("{b:?}"))
                    .unwrap_or_default();
                yield Ok(crate::proto::DbusSignalEvent {
                    signal_name: member,
                    path,
                    interface: iface,
                    json_body: body,
                    timestamp: Some(ProstTimestamp {
                        seconds: Utc::now().timestamp(),
                        nanos: Utc::now().timestamp_subsec_nanos() as i32,
                    }),
                });
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }
}
