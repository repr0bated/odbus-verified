//! gRPC Server - Implements the Operation gRPC services (shared-server topology)

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use async_stream::stream;
use chrono::{DateTime, Utc};
use op_cognitive_mcp::QdrantSemanticShuttle;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use prost_types::{Struct as ProstStruct, Timestamp as ProstTimestamp, Value as ProstValue};
use serde::de::DeserializeSeed;
use serde_json::Value as JsonValue;
use simd_json::prelude::{ValueAsContainer, ValueAsScalar};
use tokio::sync::{RwLock, broadcast};
use tokio_stream::Stream;
use tonic::transport::{Identity, ServerTlsConfig};
use tonic::{Request, Response, Status};
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::interceptor::{self, GhostbridgeIdentity, load_capability_grants};
use crate::oracle_assertion::{AssertionValidator, DecoyTrustStore};

use crate::dynamic_reflection::{ActiveReflectionCatalog, DynamicReflectionService};
use crate::mutation_engine::{ChangeType, MutationEngine};
use crate::plugin_grpc_gen::MethodServiceRegistry;
use crate::proto::{
    BatchMutateRequest, BatchMutateResponse, CallMethodRequest, CallMethodResponse,
    CapabilityMissing as ProtoCapabilityMissing, ChainEvent as ProtoChainEvent,
    ChangeType as ProtoChangeType, ConstraintFail as ProtoConstraintFail, CreateSnapshotRequest,
    CreateSnapshotResponse, Decision as ProtoDecision, DenyReason as ProtoDenyReason,
    ErrorCode as ProtoErrorCode, GetEventsRequest, GetEventsResponse, GetProofRequest,
    GetProofResponse, GetPropertyRequest, GetPropertyResponse, GetSchemaRequest, GetSchemaResponse,
    GetSnapshotRequest, GetSnapshotResponse, GetStateRequest, GetStateResponse,
    ListPluginsResponse, MerkleProofSibling, MutateRequest, MutateResponse,
    MutationError as ProtoMutationError, NumaNode as ProtoNumaNode,
    OperationType as ProtoOperationType, OvsdbAttachControllerSafeRequest,
    OvsdbAttachControllerSafeResponse, OvsdbBridge as ProtoOvsdbBridge, OvsdbDelControllerRequest,
    OvsdbDelControllerResponse, OvsdbDumpDbRequest, OvsdbDumpDbResponse, OvsdbEchoRequest,
    OvsdbEchoResponse, OvsdbEnsureFallbackNormalRequest, OvsdbEnsureFallbackNormalResponse,
    OvsdbGetBridgeStateRequest, OvsdbGetBridgeStateResponse, OvsdbGetDatapathHealthRequest,
    OvsdbGetDatapathHealthResponse, OvsdbGetSchemaRequest, OvsdbGetSchemaResponse,
    OvsdbInterface as ProtoOvsdbInterface, OvsdbListDbsResponse, OvsdbMonitorRequest,
    OvsdbPort as ProtoOvsdbPort, OvsdbTransactRequest, OvsdbTransactResponse, OvsdbUpdate,
    PluginInfo, ProveTagImmutabilityRequest, ProveTagImmutabilityResponse,
    ReadOnlyViolation as ProtoReadOnlyViolation, RuntimeGetNumaTopologyResponse,
    RuntimeGetServiceRequest, RuntimeGetSystemInfoResponse, RuntimeListInterfacesResponse,
    RuntimeListServicesRequest, RuntimeListServicesResponse, RuntimeMetricUpdate,
    RuntimeNetworkInterface as ProtoRuntimeNetworkInterface,
    RuntimeServiceInfo as ProtoRuntimeServiceInfo, RuntimeStreamMetricsRequest,
    SearchSemanticTraceRequest, SearchSemanticTraceResponse, SetPropertyRequest,
    SetPropertyResponse, Signal, StateChange as ProtoStateChange,
    StateFrameKind as ProtoStateFrameKind, SubscribeEventsRequest, SubscribeRequest,
    SubscribeSignalsRequest, TagLock as ProtoTagLock, VerifyChainRequest, VerifyChainResponse,
    event_chain_service_server::EventChainService, ovsdb_mirror_server::OvsdbMirror,
    plugin_service_server::PluginService, runtime_mirror_server::RuntimeMirror,
    state_sync_server::StateSync,
};
use op_state_store::{Decision, DenyReason, PluginSchema};
use zbus::Proxy;

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
    required_capability: String,
}

fn read_plugin_schema_json(plugin_id: &str) -> Option<JsonValue> {
    let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
    let document =
        crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog_dir, plugin_id)
            .ok()?;
    serde_json::from_str(&document.blob.schema_json).ok()
}

/// Request metadata key by which a caller declares the capability it is exercising.
///
/// `enforce_bridge_capability` requires the declaration to match the method's
/// `required_capability`; a mismatch or an absent header denies. This keeps the gRPC
/// check explicit: the caller states intent, carries an authenticated identity,
/// and that exact principal must be present in the independent grant store.
pub const DECLARED_CAPABILITY_HEADER: &str = "x-opdbus-capability";

fn method_capability_from_schema(
    schema: &JsonValue,
    method_name: &str,
) -> Option<SchemaMethodCapability> {
    let method = schema.get("methods")?.as_object()?.get(method_name)?;
    let required_capability = method
        .get("required_capability")?
        .as_str()?
        .trim()
        .to_string();
    if required_capability.is_empty() {
        return None;
    }
    Some(SchemaMethodCapability {
        required_capability,
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
/// reads the sealed schema from the SHM blob catalog. Missing target schema,
/// method metadata, request identity, capability declaration, or exact grant
/// all deny. Embedded `PluginSchema.capability_grants` never confer runtime
/// authority.
pub(crate) fn enforce_bridge_capability_with_schema(
    schema: Option<&JsonValue>,
    plugin_id: &str,
    method_name: &str,
    capability_id: Option<&str>,
    identity: Option<&GhostbridgeIdentity>,
) -> Result<(), ProtoMutationError> {
    let method_capability = schema
        .and_then(|schema| method_capability_from_schema(schema, method_name))
        .ok_or_else(|| {
            capability_denied(
                plugin_id,
                method_name,
                capability_id,
                "authoritative method capability metadata is unavailable",
            )
        })?;
    let required = method_capability.required_capability;
    let identity = identity
        .filter(|identity| !identity.principal_id.is_empty() && !identity.session_id.is_empty())
        .ok_or_else(|| {
            capability_denied(
                plugin_id,
                method_name,
                Some(&required),
                "authenticated request identity is unavailable",
            )
        })?;
    let capability_matches = capability_id == Some(required.as_str());
    let principal_grants = load_capability_grants(&identity.principal_id).contains(&required);

    if capability_matches && principal_grants {
        Ok(())
    } else {
        Err(capability_denied(
            plugin_id,
            method_name,
            Some(&required),
            "matching request capability and exact-principal grant are required",
        ))
    }
}

#[allow(clippy::result_large_err)] // Preserve tonic status at the RPC boundary.
fn validate_generated_arguments(
    schema: &JsonValue,
    method_name: &str,
    args: &mut JsonValue,
) -> Result<(), Status> {
    let contract = schema
        .get("methods")
        .and_then(|methods| methods.get(method_name))
        .and_then(|method| method.get("args"))
        .ok_or_else(|| Status::failed_precondition("sealed method input schema is unavailable"))?;
    // A Rust unit input has a null JSON contract and an empty protobuf
    // message. Normalize only that exact case; other inputs retain their
    // required fields, nullability and domain constraints from the blob.
    if contract.get("type").and_then(JsonValue::as_str) == Some("null")
        && args.as_object().is_some_and(|object| object.is_empty())
    {
        *args = JsonValue::Null;
    }
    let validator = jsonschema::Validator::new(contract).map_err(|error| {
        Status::failed_precondition(format!("invalid sealed method input schema: {error}"))
    })?;
    validator.validate(args).map_err(|error| {
        Status::invalid_argument(format!(
            "arguments do not conform to {method_name}: {error}"
        ))
    })
}

#[cfg(test)]
mod generated_argument_tests {
    use super::*;

    #[test]
    fn typed_requests_obey_sealed_constraints_and_unit_contracts() {
        let schema = serde_json::json!({"methods":{
            "configure":{"args":{"type":"object","required":["port","enabled"],"properties":{
                "port":{"type":"integer","minimum":1,"maximum":65535},
                "enabled":{"type":"boolean"}
            }}},
            "list":{"args":{"type":"null"}}
        }});
        let mut valid = serde_json::json!({"port":443,"enabled":false});
        validate_generated_arguments(&schema, "configure", &mut valid).unwrap();
        let mut invalid = serde_json::json!({"port":0,"enabled":false});
        assert_eq!(
            validate_generated_arguments(&schema, "configure", &mut invalid)
                .unwrap_err()
                .code(),
            tonic::Code::InvalidArgument
        );
        let mut missing = serde_json::json!({"port":443});
        assert!(validate_generated_arguments(&schema, "configure", &mut missing).is_err());
        let mut empty = serde_json::json!({});
        validate_generated_arguments(&schema, "list", &mut empty).unwrap();
        assert!(empty.is_null());
        let mut supplied = serde_json::json!({"unexpected":true});
        assert!(validate_generated_arguments(&schema, "list", &mut supplied).is_err());
        assert!(validate_generated_arguments(&schema, "missing", &mut empty).is_err());
    }
}

fn capability_denied(
    plugin_id: &str,
    method_name: &str,
    capability: Option<&str>,
    detail: &str,
) -> ProtoMutationError {
    let capability = capability.unwrap_or("<undeclared>").to_string();
    ProtoMutationError {
        code: ProtoErrorCode::PermissionDenied as i32,
        message: format!(
            "AccessDenied: method {plugin_id}.{method_name} requires authoritative capability metadata and grant ({detail})"
        ),
        deny_reason: Some(ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::CapabilityMissing(
                ProtoCapabilityMissing { capability },
            )),
        }),
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

fn enforce_human_principal_self_service(
    plugin_id: &str,
    method_name: &str,
    args: &JsonValue,
    identity: Option<&crate::oracle_assertion::HumanPrincipalIdentity>,
) -> Result<(), ProtoMutationError> {
    if plugin_id != "human_principal" {
        return Ok(());
    }
    let Some(identity) = identity else {
        return Ok(());
    };
    let args = args
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(args);
    let authorized = match method_name {
        "register_key" | "revoke_key" => args
            .get("human_pubkey")
            .and_then(JsonValue::as_str)
            .is_none_or(|pubkey| pubkey == identity.human_pubkey),
        "set_alias" => args
            .get("principal_id")
            .and_then(JsonValue::as_str)
            .is_none_or(|principal_id| principal_id == identity.principal_id),
        _ => true,
    };
    if authorized {
        return Ok(());
    }
    Err(ProtoMutationError {
        code: ProtoErrorCode::PermissionDenied as i32,
        message: format!(
            "AccessDenied: human assertion may only mutate its own principal via \
             human_principal.{method_name}"
        ),
        deny_reason: Some(ProtoDenyReason {
            reason: Some(crate::proto::deny_reason::Reason::CapabilityMissing(
                ProtoCapabilityMissing {
                    capability: "human_principal.write:self".to_string(),
                },
            )),
        }),
    })
}

fn plugin_id_from_dbus_path(path: &str) -> Option<String> {
    let prefix = "/org/opdbus/v1/plugins/";
    path.strip_prefix(prefix)
        .filter(|id| !id.is_empty() && !id.contains('/'))
        .map(str::to_string)
}

/// Legacy D-Bus transport may address only the canonical plugin object on
/// the system bus. Check the complete tuple before constructing a proxy.
// This helper returns tonic's wire status directly to the legacy RPC boundary.
#[allow(clippy::result_large_err)]
fn passthrough_plugin_id(
    bus: &str,
    destination: &str,
    path: &str,
    interface: &str,
) -> Result<String, Status> {
    let plugin_id = plugin_id_from_dbus_path(path)
        .ok_or_else(|| Status::permission_denied("D-Bus target is not a canonical plugin"))?;
    if bus != "system"
        || destination != op_plugins::canonical::BASE_SERVICE_NAME
        || interface != op_plugins::canonical::PLUGIN_INTERFACE
        || path != op_plugins::canonical::plugin_path(&plugin_id)
    {
        return Err(Status::permission_denied(
            "D-Bus target is not a canonical plugin",
        ));
    }
    Ok(plugin_id)
}

fn declared_capability<T>(request: &Request<T>) -> Option<&str> {
    request
        .metadata()
        .get(DECLARED_CAPABILITY_HEADER)?
        .to_str()
        .ok()
}

// Component registration denies with tonic's unimplemented wire status.
#[allow(clippy::result_large_err)]
fn authorize_component_registration<T>(_: &Request<T>) -> Result<(), Status> {
    Err(Status::unimplemented(
        "component registration has no sealed method declaration; use PluginService",
    ))
}

#[cfg(test)]
mod legacy_route_admission_tests {
    use super::*;

    #[test]
    fn passthrough_requires_exact_canonical_target_before_proxy_creation() {
        let path = op_plugins::canonical::plugin_path("hardware");
        assert_eq!(
            passthrough_plugin_id(
                "system",
                op_plugins::canonical::BASE_SERVICE_NAME,
                &path,
                op_plugins::canonical::PLUGIN_INTERFACE
            )
            .unwrap(),
            "hardware"
        );
        for (bus, destination, path, interface) in [
            (
                "session",
                op_plugins::canonical::BASE_SERVICE_NAME,
                path.as_str(),
                op_plugins::canonical::PLUGIN_INTERFACE,
            ),
            (
                "system",
                "org.freedesktop.systemd1",
                path.as_str(),
                op_plugins::canonical::PLUGIN_INTERFACE,
            ),
            (
                "system",
                op_plugins::canonical::BASE_SERVICE_NAME,
                "/org/freedesktop/DBus",
                op_plugins::canonical::PLUGIN_INTERFACE,
            ),
            (
                "system",
                op_plugins::canonical::BASE_SERVICE_NAME,
                "/org/opdbus/v1/plugins/hardware/child",
                op_plugins::canonical::PLUGIN_INTERFACE,
            ),
            (
                "system",
                op_plugins::canonical::BASE_SERVICE_NAME,
                path.as_str(),
                "org.freedesktop.DBus.Properties",
            ),
        ] {
            assert_eq!(
                passthrough_plugin_id(bus, destination, path, interface)
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied
            );
        }
    }

    #[test]
    fn legacy_effect_routes_are_unsupported_before_handler_execution() {
        assert_eq!(
            unsupported_legacy_domain(Request::new(()))
                .unwrap_err()
                .code(),
            tonic::Code::Unimplemented,
        );
        let mut bootstrap = Request::new(());
        bootstrap.extensions_mut().insert(tonic::GrpcMethod::new(
            "operation.registration.v1.RegistrationService",
            "SendMagicLink",
        ));
        assert!(compatibility_registration_interceptor(bootstrap).is_ok());
        let mut admin = Request::new(());
        admin.extensions_mut().insert(tonic::GrpcMethod::new(
            "operation.registration.v1.RegistrationService",
            "ListUsers",
        ));
        assert_eq!(
            compatibility_registration_interceptor(admin)
                .unwrap_err()
                .code(),
            tonic::Code::Unimplemented,
        );
    }
}

// Tonic interceptors require this exact Status-bearing signature.
#[allow(clippy::result_large_err)]
fn unsupported_legacy_domain(_: Request<()>) -> Result<Request<()>, Status> {
    Err(Status::unimplemented(
        "legacy domain RPC has no sealed method admission; use PluginService",
    ))
}

// Tonic interceptors require this exact Status-bearing signature.
#[allow(clippy::result_large_err)]
fn compatibility_registration_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    match req.extensions().get::<tonic::GrpcMethod<'_>>() {
        Some(method)
            if method.service() == "operation.registration.v1.RegistrationService"
                && matches!(method.method(), "SendMagicLink" | "VerifyMagicLink") =>
        {
            Ok(req)
        }
        _ => unsupported_legacy_domain(req),
    }
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
    let directory = crate::sealed_schema_reader::blob_catalog_dir_from_env();
    let Ok(catalog) = crate::sealed_schema_reader::read_manifest_pinned_catalog(&directory) else {
        return Vec::new();
    };
    catalog
        .plugins
        .into_iter()
        .filter_map(|(id, document)| {
            let schema: JsonValue = serde_json::from_str(&document.blob.schema_json).ok()?;
            Some(plugin_info_from_schema(&id, &schema))
        })
        .collect()
}

fn read_schema(plugin_id: &str) -> Option<(String, String, String)> {
    if op_plugins::default_registry::is_retired_plugin(plugin_id) {
        return None;
    }
    let document = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        plugin_id,
    )
    .ok()?;
    let mut response =
        schema_response_from_value(&serde_json::from_str(&document.blob.schema_json).ok()?)?;
    response.0 = document.blob.schema_json;
    Some(response)
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
    // The registry handler returns this exact permission-denied Status to gRPC.
    #[allow(clippy::result_large_err)]
    fn ensure_registration_vacant(&self, component_id: &str) -> Result<(), Status> {
        if self.components.contains_key(component_id) {
            return Err(Status::permission_denied(
                "component is already registered; deregister with its lease before registering again",
            ));
        }
        Ok(())
    }

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

#[cfg(test)]
mod component_registration_tests {
    use super::*;

    #[test]
    fn occupied_component_id_requires_prior_lease_deregistration() {
        let (mut registry, _) = RegistryInner::new();
        let component_id = "component-a".to_string();
        let existing = crate::proto::registry::ComponentInfo {
            component_id: component_id.clone(),
            endpoint: "unix:///original.sock".into(),
            ..Default::default()
        };
        registry
            .components
            .insert(component_id.clone(), existing.clone());
        registry
            .leases
            .insert(component_id.clone(), "original-lease".into());

        let error = registry
            .ensure_registration_vacant(&component_id)
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            registry.components[&component_id].endpoint,
            existing.endpoint
        );
        assert_eq!(registry.leases[&component_id], "original-lease");
        registry
            .ensure_registration_vacant("new-component")
            .unwrap();
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
        let (registry, _) = RegistryInner::new();
        Self {
            mutation_engine,
            plugin_provider: Arc::new(SchemaCatalogPluginProvider),
            semantic_shuttle: None,
            registry: Arc::new(RwLock::new(registry)),
            per_method_services: PerMethodGrpcServices::new(),
            per_method_reflection: Arc::new(PerMethodReflectionRegistry::new()),
            active_reflection: ActiveReflectionCatalog::new(),
        }
    }

    pub(crate) fn mutation_engine(&self) -> Arc<MutationEngine> {
        self.mutation_engine.clone()
    }

    pub fn with_plugin_provider(
        mutation_engine: Arc<MutationEngine>,
        plugin_provider: Arc<dyn PluginSchemaProvider>,
    ) -> Self {
        let (registry, _) = RegistryInner::new();
        Self {
            mutation_engine,
            plugin_provider,
            semantic_shuttle: None,
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

    /// Supply an isolated sealed-blob catalog for embedded servers and tests.
    /// The default constructor continues to use the host SHM catalog.
    pub fn with_reflection_catalog(mut self, catalog: ActiveReflectionCatalog) -> Self {
        self.active_reflection = catalog;
        self
    }

    /// Hydrate in-memory reflection from the sealed SHM blob catalog.
    ///
    /// The blob catalog IS the plugin registry: a restart must advertise the
    /// same plugin set that was sealed, without waiting for every plugin to
    /// re-register at runtime. Call once at server startup.
    pub async fn hydrate_reflection_from_shm(&self) -> usize {
        let count = self.active_reflection.sync_from_shm().await;
        tracing::info!(count, "hydrated reflection from SHM blob catalog");
        count
    }

    /// Freeze all plugin method descriptors from the configured schema provider.
    ///
    /// Populate schema/authority metadata before building routes. This does
    /// not mount handlers; the generated `add_routes` does that separately.
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

    /// Register a plugin's frozen per-method schema and authority descriptors.
    ///
    /// This is called at D-Bus object creation time. Descriptor registration
    /// does not create an RPC route; service discovery advertises only the
    /// generated handlers that are actually mounted for active blobs.
    pub async fn register_plugin_methods(
        &self,
        plugin_id: String,
        schema: &PluginSchema,
    ) -> Result<Vec<MethodServiceRegistry>, anyhow::Error> {
        if op_plugins::default_registry::is_retired_plugin(&plugin_id) {
            anyhow::bail!("plugin '{}' is retired and cannot be registered", plugin_id);
        }
        // Registration cannot create plugin existence or replace the sealed
        // contract. A supplied schema must equal the manifest-pinned owner.
        let pinned = crate::sealed_schema_reader::read_registration_document(
            self.active_reflection.catalog_dir(),
            &plugin_id,
            schema,
        )?;
        let registries = self
            .per_method_services
            .register_sealed_services(
                plugin_id.clone(),
                &pinned.schema,
                &pinned.blob.descriptor_set,
            )
            .await?;

        // Register each method with the reflection registry
        self.per_method_reflection
            .unregister_plugin(&plugin_id)
            .await;
        for method in &pinned.blob.manifest.methods {
            self.per_method_reflection
                .register_sealed(plugin_id.clone(), method, pinned.schema.version.clone())
                .await;
        }

        self.active_reflection.sync_from_shm().await;

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
        let human_identity = request
            .extensions()
            .get::<crate::oracle_assertion::HumanPrincipalIdentity>()
            .cloned();
        let identity = interceptor::bridge_capability_identity(request.extensions());

        // The caller declares which capability it is exercising, and the bridge
        // verifies that declaration against the method's `required_capability`.
        //
        // Passing `None` here (as this previously did) made `capability_matches` in
        // `enforce_bridge_capability` compare `None == Some(required)`, which is
        // always false — and since the decision is `capability_matches &&
        // principal_grants`, every capability-gated method was permanently denied
        // over gRPC/gRPC-Web. Every cognitive_mcp method is gated, so none were
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

        let sealed_schema = read_plugin_schema_json(plugin_id);
        if let Err(error) = enforce_bridge_capability_with_schema(
            sealed_schema.as_ref(),
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
        let mut json_args = Vec::new();
        request_dynamic
            .serialize_with_options(
                &mut serde_json::Serializer::new(&mut json_args),
                &SerializeOptions::new()
                    .use_proto_field_name(true)
                    .stringify_64_bit_integers(false)
                    .skip_default_fields(false),
            )
            .map_err(|error| {
                Status::internal(format!(
                    "failed to serialize typed request for {plugin_id}.{method_name}: {error}"
                ))
            })?;
        let mut json_args = String::from_utf8(json_args).map_err(|error| {
            Status::internal(format!(
                "typed request JSON was not UTF-8 for {plugin_id}.{method_name}: {error}"
            ))
        })?;
        let mut args_value: JsonValue = serde_json::from_str(&json_args).map_err(|error| {
            Status::internal(format!(
                "failed to inspect typed request for {plugin_id}.{method_name}: {error}"
            ))
        })?;
        let sealed_schema = sealed_schema
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("sealed method schema is unavailable"))?;
        validate_generated_arguments(sealed_schema, method_name, &mut args_value)?;
        json_args = serde_json::to_string(&args_value)
            .map_err(|error| Status::internal(error.to_string()))?;
        enforce_human_principal_self_service(
            plugin_id,
            method_name,
            &args_value,
            human_identity.as_ref(),
        )
        .map_err(|error| Status::permission_denied(error.message))?;

        let authenticated_actor_id = identity
            .as_ref()
            .map(|identity| identity.principal_id.as_str())
            .ok_or_else(|| Status::unauthenticated("authenticated principal is required"))?;
        let result = self
            .mutation_engine
            .dispatch_method_call_with_identity(
                plugin_id,
                method_name,
                &json_args,
                declared_capability.as_deref(),
                authenticated_actor_id,
                identity
                    .as_ref()
                    .map(|identity| identity.session_id.as_str()),
                identity
                    .as_ref()
                    .map(|identity| identity.session_genesis.as_str()),
            )
            .await
            .map_err(|error| {
                Status::internal(format!(
                    "plugin method {}.{} failed: {}",
                    plugin_id, method_name, error
                ))
            })?;

        let method_result = result.get("result").unwrap_or(&result);
        let result_json = serde_json::to_string(method_result).map_err(|error| {
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
/// the tched_router bridge (`run_tched_router_server`, `:8090` + `container.sock`) — builds
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
/// afterward (e.g. the tched_router bridge adds `TchedRouterService`; `run_grpc_server`
/// adds the gRPC health service).
///
/// Adding a new domain service: add one `.add_service(...)` here and it appears on
/// BOTH endpoints at once.
pub fn build_operation_routes(server: OperationGrpcServer) -> tonic::service::Routes {
    let validator = Arc::new(AssertionValidator::from_env(DecoyTrustStore::load()));
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

    // Only PluginService and its canonical forwarding adapters remain live.
    // Domain implementations without a sealed PluginSchema method cannot
    // acquire an exact grant, so their compatibility routes fail closed.
    let assertion_intercept = interceptor::make_ghostbridge_interceptor(validator.clone());

    let routes = tonic::service::Routes::new(crate::grpc_web::enable(
        StateSyncServer::with_interceptor(server.clone(), assertion_intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        PluginServiceServer::with_interceptor(server.clone(), assertion_intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        EventChainServiceServer::with_interceptor(server.clone(), assertion_intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        OvsdbMirrorServer::with_interceptor(server.clone(), unsupported_legacy_domain),
    ))
    .add_service(crate::grpc_web::enable(
        RuntimeMirrorServer::with_interceptor(server.clone(), unsupported_legacy_domain),
    ))
    .add_service(crate::grpc_web::enable(
        ComponentRegistryServer::with_interceptor(server.clone(), unsupported_legacy_domain),
    ))
    .add_service(crate::grpc_web::enable(
        MailServiceServer::with_interceptor(server.clone(), unsupported_legacy_domain),
    ))
    .add_service(crate::grpc_web::enable(
        PrivacyNetworkServiceServer::with_interceptor(server.clone(), unsupported_legacy_domain),
    ))
    .add_service(crate::grpc_web::enable(
        RegistrationServiceServer::with_interceptor(
            server.clone(),
            compatibility_registration_interceptor,
        ),
    ))
    .add_service(crate::grpc_web::enable(
        DbusPassthroughServer::with_interceptor(server.clone(), assertion_intercept.clone()),
    ))
    .add_service(crate::grpc_web::enable(
        crate::proto::chat::chat_service_server::ChatServiceServer::with_interceptor(
            crate::chat_service::ChatServiceImpl::new(server.mutation_engine.clone()),
            unsupported_legacy_domain,
        ),
    ));

    let routes = add_routes(routes, server.clone(), validator);
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
) -> anyhow::Result<()> {
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

    mutation_engine.initialize_identity_sled_store().await?;
    crate::identity_sled_dispatch::recover_identity_startup(mutation_engine.as_ref()).await?;

    match mutation_engine.seed_missing_plugin_projections().await {
        Ok(count) => info!(count, "Plugin projections seeded"),
        Err(error) => warn!(%error, "Plugin projection seeding failed"),
    }

    if let Err(error) = mutation_engine.clone().start().await {
        warn!(%error, "Native plugin observation startup failed");
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
        .await?;
    Ok(())
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
        let verified_identity =
            crate::event_chain_access::verified_identity(&self.mutation_engine, &request).await?;
        if let Some(selection) = crate::chatbot_episode_stream::EpisodeSelection::from_subscription(
            &request.get_ref().plugin_ids,
            &request.get_ref().path_patterns,
        )
        .map_err(|_| Status::invalid_argument("invalid private chatbot episode selection"))?
        {
            return crate::chatbot_episode_stream::subscribe(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        if let Some(selection) =
            crate::generation_jobs_selection::GenerationSelection::from_subscription(
                &request.get_ref().plugin_ids,
                &request.get_ref().path_patterns,
            )
            .map_err(|_| Status::invalid_argument("invalid private generation selection"))?
        {
            return crate::generation_jobs_stream::subscribe(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        let private_selection =
            crate::ui_workspace_stream_selection::WorkspaceSelection::from_subscription(
                &request.get_ref().plugin_ids,
                &request.get_ref().path_patterns,
            )
            .map_err(|_| Status::invalid_argument("invalid private workspace selection"))?;
        if let Some(selection) = private_selection {
            return crate::ui_workspace_stream::subscribe(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        let req = request.into_inner();
        info!("gRPC Subscribe: plugins={:?}", req.plugin_ids);

        let mut rx = self.mutation_engine.change_tx().subscribe();
        let plugin_filters = req.plugin_ids;
        let path_filters = req.path_patterns;
        if !req.tags.is_empty() {
            return Err(Status::unimplemented(
                "StateSync tag filtering requires committed SHM tag provenance",
            ));
        }
        crate::state_stream::validate_public_path_patterns(&path_filters)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let initial = crate::state_stream::read_public_state_snapshot(&plugin_filters)
            .await
            .map_err(public_state_status)?;
        crate::state_stream::validate_path_patterns_against_snapshot(&initial, &path_filters)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let include_initial_state = req.include_initial_state;
        let include_schema = req.include_schema;

        let engine = self.mutation_engine.clone();
        let stream = stream! {
            let mut previous = initial;
            match state_snapshot_frames(
                &previous,
                None,
                include_schema,
                include_initial_state,
                ProtoStateFrameKind::InitialState,
                &path_filters,
                false,
            ) {
                Ok(frames) => for frame in frames {
                    if let Err(status) = crate::event_chain_access::validate_identity(&engine, &verified_identity).await {
                        yield Err(status); return;
                    }
                    yield Ok(frame);
                },
                Err(status) => { yield Err(status); return; }
            }
            let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            heartbeat.tick().await;
            loop {
                let force_rehydrate = tokio::select! {
                    received = rx.recv() => match received {
                    Ok(update) => {
                        let matches_plugin = plugin_filters.is_empty()
                            || plugin_filters.contains(&update.plugin_id);
                        let commits_state = !matches!(
                            update.change_type,
                            ChangeType::MethodCall | ChangeType::Signal
                        );

                        if update.change_type == ChangeType::SchemaMigration || (matches_plugin && commits_state) {
                            Some(false)
                        } else {
                            continue;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Subscriber lagged, missed {} updates", n);
                        Some(true)
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = heartbeat.tick() => None,
                };
                if let Err(status) = crate::event_chain_access::validate_identity(&engine, &verified_identity).await {
                    yield Err(status); break;
                }
                let Some(force_rehydrate) = force_rehydrate else {
                    yield Ok(state_heartbeat_frame(&previous));
                    continue;
                };
                let current = match crate::state_stream::read_public_state_snapshot_after_admission(&plugin_filters).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        yield Err(public_state_status(error));
                        break;
                    }
                };
                let changed = current.generation != previous.generation
                    || current.catalog_hash != previous.catalog_hash;
                if changed || force_rehydrate {
                    match state_snapshot_frames(
                        &current,
                        Some(&previous),
                        include_schema,
                        true,
                        ProtoStateFrameKind::Update,
                        &path_filters,
                        force_rehydrate,
                    ) {
                        Ok(frames) => for frame in frames {
                            if let Err(status) = crate::event_chain_access::validate_identity(&engine, &verified_identity).await {
                                yield Err(status); return;
                            }
                            yield Ok(frame);
                        },
                        Err(status) => { yield Err(status); break; }
                    }
                    previous = current;
                }
                yield Ok(state_heartbeat_frame(&previous));
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn mutate(
        &self,
        request: Request<MutateRequest>,
    ) -> Result<Response<MutateResponse>, Status> {
        let operation = request.get_ref().operation;
        if operation == ProtoOperationType::CallMethod as i32 {
            let request = request.map(|req| CallMethodRequest {
                plugin_id: req.plugin_id,
                object_path: req.object_path,
                interface_name: op_plugins::canonical::PLUGIN_INTERFACE.to_string(),
                method_name: req
                    .member_name
                    .strip_prefix("method:")
                    .unwrap_or(&req.member_name)
                    .to_string(),
                arguments: req.value.into_iter().collect(),
                actor_id: req.actor_id,
                capability_id: req.capability_id,
            });
            let response = <Self as PluginService>::call_method(self, request)
                .await?
                .into_inner();
            return Ok(Response::new(MutateResponse {
                success: response.success,
                event_id: response.event_id,
                event_hash: response.event_hash,
                result: response.result,
                error: response.error,
                effective_hash: String::new(),
            }));
        }
        if operation == ProtoOperationType::SetProperty as i32 {
            let request = request.map(|req| SetPropertyRequest {
                plugin_id: req.plugin_id,
                object_path: req.object_path,
                interface_name: op_plugins::canonical::PLUGIN_INTERFACE.to_string(),
                property_name: req.member_name,
                value: req.value,
                actor_id: req.actor_id,
                capability_id: req.capability_id,
            });
            let response = <Self as PluginService>::set_property(self, request)
                .await?
                .into_inner();
            return Ok(Response::new(MutateResponse {
                success: response.success,
                event_id: response.event_id,
                event_hash: response.event_hash,
                result: None,
                error: response.error,
                effective_hash: String::new(),
            }));
        }
        Err(Status::failed_precondition(
            "raw state replacement is not a backend mutation; use a schema-declared plugin method",
        ))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let verified_identity =
            crate::event_chain_access::verified_identity(&self.mutation_engine, &request).await?;
        if let Some(selection) = crate::chatbot_episode_stream::EpisodeSelection::from_get_state(
            &request.get_ref().plugin_id,
            &request.get_ref().object_path,
        )
        .map_err(|_| Status::invalid_argument("invalid private chatbot episode selection"))?
        {
            return crate::chatbot_episode_stream::get_state(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        if let Some(selection) =
            crate::generation_jobs_selection::GenerationSelection::from_get_state(
                &request.get_ref().plugin_id,
                &request.get_ref().object_path,
            )
            .map_err(|_| Status::invalid_argument("invalid private generation selection"))?
        {
            return crate::generation_jobs_stream::get_state(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        let private_selection =
            crate::ui_workspace_stream_selection::WorkspaceSelection::from_get_state(
                &request.get_ref().plugin_id,
                &request.get_ref().object_path,
            )
            .map_err(|_| Status::invalid_argument("invalid private workspace selection"))?;
        if let Some(selection) = private_selection {
            return crate::ui_workspace_stream::get_state(
                self.mutation_engine.clone(),
                request,
                verified_identity,
                selection,
            )
            .await
            .map(Response::new);
        }
        let req = request.into_inner();
        let snapshot =
            crate::state_stream::read_public_state_snapshot(std::slice::from_ref(&req.plugin_id))
                .await
                .map_err(public_state_status)?;
        let plugin = snapshot
            .plugins
            .get(&req.plugin_id)
            .ok_or_else(|| Status::not_found("plugin is absent from the sealed catalog"))?;
        let root_state = plugin
            .state
            .as_ref()
            .ok_or_else(|| Status::not_found("plugin has no committed public SHM state"))?;
        let requested_path = if req.object_path.is_empty() {
            crate::state_stream::plugin_object_path(&req.plugin_id)
        } else {
            req.object_path
        };
        crate::state_stream::validate_public_path_patterns(std::slice::from_ref(&requested_path))
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let root_path = crate::state_stream::plugin_object_path(&req.plugin_id);
        if requested_path != root_path && !requested_path.starts_with(&(root_path.clone() + "/")) {
            return Err(Status::invalid_argument(
                "GetState object path is outside the public plugin tree",
            ));
        }
        let state =
            crate::state_stream::state_at_object_path(&req.plugin_id, root_state, &requested_path)
                .map_err(|error| Status::not_found(error.to_string()))?;
        let state = simd_json::serde::to_owned_value(state)
            .map_err(|error| Status::data_loss(error.to_string()))?;
        let state_struct = simd_to_prost_struct(&state);

        Ok(Response::new(GetStateResponse {
            state: Some(state_struct),
            effective_hash: String::new(),
            at_event_id: 0,
            value: Some(simd_to_prost_value(&state)),
        }))
    }

    async fn batch_mutate(
        &self,
        request: Request<BatchMutateRequest>,
    ) -> Result<Response<BatchMutateResponse>, Status> {
        let (metadata, extensions, req) = request.into_parts();
        if req.atomic {
            return Err(Status::unimplemented(
                "atomic mutation across plugin backends is not supported",
            ));
        }
        let mut results = Vec::new();
        let mut failed_index = -1;

        for (idx, m) in req.mutations.into_iter().enumerate() {
            let mut_req = Request::from_parts(metadata.clone(), extensions.clone(), m);
            let resp = self.mutate(mut_req).await?.into_inner();
            if !resp.success && failed_index < 0 {
                failed_index = idx as i32;
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
        let human_identity = request
            .extensions()
            .get::<crate::oracle_assertion::HumanPrincipalIdentity>()
            .cloned();
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
        let authenticated_actor_id = identity
            .as_ref()
            .map(|identity| identity.principal_id.clone())
            .ok_or_else(|| Status::unauthenticated("authenticated request identity is required"))?;
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
        let args_value: JsonValue = serde_json::from_str(&json_args)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        if let Err(error) = enforce_human_principal_self_service(
            &req.plugin_id,
            &req.method_name,
            &args_value,
            human_identity.as_ref(),
        ) {
            return Ok(Response::new(CallMethodResponse {
                success: false,
                result: None,
                event_id: 0,
                event_hash: String::new(),
                error: Some(error),
            }));
        }

        // Route through the same schema dispatcher as PluginV1.Call. That
        // dispatcher records the method event before returning its audited
        // envelope; gRPC is a transport adapter, not a second implementation.
        let result = self
            .mutation_engine
            .dispatch_method_call_with_identity(
                &req.plugin_id,
                &req.method_name,
                &json_args,
                capability_id.as_deref(),
                &authenticated_actor_id,
                identity
                    .as_ref()
                    .map(|identity| identity.session_id.as_str()),
                identity
                    .as_ref()
                    .map(|identity| identity.session_genesis.as_str()),
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
        if interceptor::bridge_capability_identity(request.extensions()).is_none() {
            return Err(Status::unauthenticated(
                "verified session identity is required",
            ));
        }
        let req = request.into_inner();
        if req.object_path != op_plugins::canonical::plugin_path(&req.plugin_id)
            || req.interface_name != op_plugins::canonical::PLUGIN_INTERFACE
        {
            return Err(Status::invalid_argument(
                "property reads require the canonical plugin root and interface",
            ));
        }
        let arguments = serde_json::json!({
            "plugin_id": req.plugin_id, "interface_name": req.interface_name,
            "property_name": req.property_name,
        });
        let result = crate::freedesktop_dispatch::prepare(&self.mutation_engine, "Get", &arguments)
            .await
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let crate::freedesktop_dispatch::DispatchOutcome::Reply(value) = result else {
            return Err(Status::internal("property read resolved to a write"));
        };
        let output: op_plugins::state_plugins::freedesktop::GetPropertyOutput =
            serde_json::from_value(value).map_err(|error| Status::internal(error.to_string()))?;
        let json = simd_json::serde::to_owned_value(output.value)
            .map_err(|error| Status::internal(error.to_string()))?;
        Ok(Response::new(GetPropertyResponse {
            value: Some(simd_to_prost_value(&json)),
            read_only: output.read_only,
        }))
    }

    async fn set_property(
        &self,
        request: Request<SetPropertyRequest>,
    ) -> Result<Response<SetPropertyResponse>, Status> {
        if interceptor::bridge_capability_identity(request.extensions()).is_none() {
            return Err(Status::unauthenticated(
                "verified session identity is required",
            ));
        }
        let req = request.get_ref();
        if req.object_path != op_plugins::canonical::plugin_path(&req.plugin_id)
            || req.interface_name != op_plugins::canonical::PLUGIN_INTERFACE
        {
            return Err(Status::invalid_argument(
                "property writes require the canonical plugin root and interface",
            ));
        }
        let value = req
            .value
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("property value is required"))?;
        let arguments = serde_json::json!({
            "plugin_id": req.plugin_id, "interface_name": req.interface_name,
            "property_name": req.property_name, "value": prost_value_to_simd(value),
        });
        let plan = crate::freedesktop_dispatch::prepare(&self.mutation_engine, "Set", &arguments)
            .await
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let crate::freedesktop_dispatch::DispatchOutcome::Write(plan) = plan else {
            return Err(Status::internal(
                "property write did not resolve to a declared method",
            ));
        };
        if req.capability_id != plan.required_capability {
            return Err(Status::permission_denied(
                "property write requires its declared method capability",
            ));
        }
        let args = simd_json::serde::to_owned_value(&plan.args)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let request = request.map(|req| CallMethodRequest {
            plugin_id: plan.target_plugin,
            object_path: req.object_path,
            interface_name: req.interface_name,
            method_name: plan.method,
            arguments: vec![simd_to_prost_value(&args)],
            actor_id: req.actor_id,
            capability_id: plan.required_capability,
        });
        let response = <Self as PluginService>::call_method(self, request)
            .await?
            .into_inner();
        Ok(Response::new(SetPropertyResponse {
            success: response.success,
            event_id: response.event_id,
            event_hash: response.event_hash,
            error: response.error,
        }))
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

use crate::event_chain_access::{AuditAccess, AuditMethod};

#[tonic::async_trait]
impl EventChainService for OperationGrpcServer {
    type SubscribeEventsStream =
        Pin<Box<dyn Stream<Item = Result<ProtoChainEvent, Status>> + Send>>;

    async fn get_events(
        &self,
        request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::QueryEvents).await?;
        let chain = self.mutation_engine.event_chain.read().await;
        let response = crate::event_chain_access::get_events(chain.events(), request.get_ref())?;
        drop(chain);
        access.revalidate(&self.mutation_engine).await?;
        Ok(Response::new(response))
    }

    async fn subscribe_events(
        &self,
        request: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::QueryEvents).await?;
        Ok(Response::new(
            crate::event_chain_access::subscribe(
                self.mutation_engine.clone(),
                request.into_inner(),
                access,
            )
            .await?,
        ))
    }

    async fn verify_chain(
        &self,
        request: Request<VerifyChainRequest>,
    ) -> Result<Response<VerifyChainResponse>, Status> {
        let access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::VerifyChain).await?;
        if request.get_ref().from_event_id != 0 || request.get_ref().to_event_id != 0 {
            return Err(Status::invalid_argument(
                "only complete-chain verification is supported",
            ));
        }
        let chain = self.mutation_engine.event_chain.read().await;
        let result = chain.verify_chain();
        drop(chain);
        access.revalidate(&self.mutation_engine).await?;
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
        let access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::VerifyChain).await?;
        let chain = self.mutation_engine.event_chain.read().await;
        let proof =
            op_state_store::EventBatch::generate_proof(chain.events(), request.get_ref().event_id)
                .ok_or_else(|| Status::not_found("proof not found"))?;
        // This proof covers the current complete in-memory chain, not a fabricated batch.
        let first = chain.events().first().map_or(0, |event| event.event_id);
        let last = chain.events().last().map_or(0, |event| event.event_id);
        drop(chain);
        access.revalidate(&self.mutation_engine).await?;
        Ok(Response::new(GetProofResponse {
            event_hash: proof.event_hash,
            siblings: proof
                .siblings
                .into_iter()
                .map(|(hash, is_right)| MerkleProofSibling { hash, is_right })
                .collect(),
            root: proof.root,
            batch_first_event_id: first,
            batch_last_event_id: last,
        }))
    }

    async fn prove_tag_immutability(
        &self,
        request: Request<ProveTagImmutabilityRequest>,
    ) -> Result<Response<ProveTagImmutabilityResponse>, Status> {
        let access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::VerifyChain).await?;
        let chain = self.mutation_engine.event_chain.read().await;
        let proof = chain.prove_tag_immutability(&request.get_ref().tag);
        drop(chain);
        access.revalidate(&self.mutation_engine).await?;
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
        let _access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::GetSnapshot).await?;
        Err(Status::failed_precondition(
            "legacy EventChain snapshot hashes and state are not a canonical Snowball snapshot contract; use snowball.get_snapshot",
        ))
    }

    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<CreateSnapshotResponse>, Status> {
        let _access =
            AuditAccess::admit(&self.mutation_engine, &request, AuditMethod::CreateSnapshot)
                .await?;
        Err(Status::failed_precondition(
            "legacy EventChain snapshot creation bypasses canonical Snowball mutation; use snowball.create_snapshot",
        ))
    }

    async fn search_semantic_trace(
        &self,
        request: Request<SearchSemanticTraceRequest>,
    ) -> Result<Response<SearchSemanticTraceResponse>, Status> {
        crate::event_chain_access::verified_identity(&self.mutation_engine, &request).await?;
        // No Snowball declaration authorizes process-scoped semantic payloads.
        Err(Status::failed_precondition(
            "semantic trace search requires a canonical caller-scoped method contract",
        ))
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn public_state_status(error: anyhow::Error) -> Status {
    let message = error.to_string();
    if message.starts_with("invalid plugin id:") {
        Status::invalid_argument(message)
    } else if message.contains("absent from the sealed catalog") {
        Status::not_found(message)
    } else {
        Status::unavailable(message)
    }
}

#[allow(clippy::result_large_err)] // Native tonic error contract for the stream.
pub(crate) fn state_snapshot_frames(
    current: &crate::state_stream::StateSnapshot,
    previous: Option<&crate::state_stream::StateSnapshot>,
    include_schema: bool,
    include_state: bool,
    frame_kind: ProtoStateFrameKind,
    path_patterns: &[String],
    force_rehydrate: bool,
) -> Result<Vec<ProtoStateChange>, Status> {
    state_snapshot_frames_with_views(
        current,
        previous,
        include_schema,
        include_state,
        frame_kind,
        path_patterns,
        force_rehydrate,
        crate::state_stream::object_views,
    )
}

pub(crate) type StateViewSelector =
    fn(
        &str,
        &JsonValue,
        &[String],
    ) -> anyhow::Result<std::collections::BTreeMap<String, JsonValue>>;

// Views are already admitted by the caller's public or private owner boundary.
// This pure diff/bounds helper grants no read access and reads no filesystem.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
pub(crate) fn state_snapshot_frames_with_views(
    current: &crate::state_stream::StateSnapshot,
    previous: Option<&crate::state_stream::StateSnapshot>,
    include_schema: bool,
    include_state: bool,
    frame_kind: ProtoStateFrameKind,
    path_patterns: &[String],
    force_rehydrate: bool,
    select_views: StateViewSelector,
) -> Result<Vec<ProtoStateChange>, Status> {
    let mut frames = Vec::new();
    for (plugin_id, plugin) in &current.plugins {
        let previous_plugin = previous.and_then(|snapshot| snapshot.plugins.get(plugin_id));
        if include_schema
            && (force_rehydrate
                || previous_plugin.is_none()
                || previous_plugin.is_some_and(|old| old.schema_hash != plugin.schema_hash))
        {
            frames.push(state_frame(
                plugin_id,
                crate::state_stream::plugin_object_path(plugin_id),
                ProtoChangeType::SchemaMigration,
                plugin.schema_hash.clone(),
                None,
                Some(&plugin.schema),
                frame_kind,
                &plugin.schema_hash,
                current,
            )?);
        }
        if !include_state {
            continue;
        }
        let current_views = plugin
            .state
            .as_ref()
            .map(|state| select_views(plugin_id, state, path_patterns))
            .transpose()
            .map_err(|error| Status::resource_exhausted(error.to_string()))?
            .unwrap_or_default();
        let previous_views = previous_plugin
            .and_then(|old| old.state.as_ref())
            .map(|state| select_views(plugin_id, state, path_patterns))
            .transpose()
            .map_err(|error| Status::resource_exhausted(error.to_string()))?
            .unwrap_or_default();
        let paths: std::collections::BTreeSet<_> = current_views
            .keys()
            .chain(previous_views.keys())
            .cloned()
            .collect();
        for path in paths {
            let new_state = current_views.get(&path);
            let old_state = previous_views.get(&path);
            if !force_rehydrate && previous.is_some() && new_state == old_state {
                continue;
            }
            frames.push(state_frame(
                plugin_id,
                path,
                if new_state.is_some() {
                    ProtoChangeType::PropertySet
                } else {
                    ProtoChangeType::ObjectRemoved
                },
                String::new(),
                if force_rehydrate { None } else { old_state },
                new_state,
                frame_kind,
                &plugin.schema_hash,
                current,
            )?);
        }
    }
    if let Some(previous) = previous {
        for (plugin_id, plugin) in &previous.plugins {
            if current.plugins.contains_key(plugin_id) {
                continue;
            }
            if include_schema {
                frames.push(state_frame(
                    plugin_id,
                    crate::state_stream::plugin_object_path(plugin_id),
                    ProtoChangeType::SchemaMigration,
                    plugin.schema_hash.clone(),
                    None,
                    None,
                    frame_kind,
                    "",
                    current,
                )?);
            }
            if include_state {
                let Some(state) = plugin.state.as_ref() else {
                    continue;
                };
                for (path, old_state) in select_views(plugin_id, state, path_patterns)
                    .map_err(|error| Status::resource_exhausted(error.to_string()))?
                {
                    frames.push(state_frame(
                        plugin_id,
                        path,
                        ProtoChangeType::ObjectRemoved,
                        String::new(),
                        if force_rehydrate {
                            None
                        } else {
                            Some(&old_state)
                        },
                        None,
                        frame_kind,
                        "",
                        current,
                    )?);
                }
            }
        }
    }
    if frames.len() > 4096 {
        return Err(Status::resource_exhausted(
            "state snapshot exceeds 4096 stream frames",
        ));
    }
    let encoded_bytes = frames.iter().try_fold(0usize, |total, frame| {
        total.checked_add(frame.encoded_len())
    });
    if encoded_bytes.is_none_or(|bytes| bytes > 16 * 1024 * 1024) {
        return Err(Status::resource_exhausted(
            "state snapshot exceeds 16 MiB of stream frames",
        ));
    }
    Ok(frames)
}

#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn state_frame(
    plugin_id: &str,
    object_path: String,
    change_type: ProtoChangeType,
    member_name: String,
    old_value: Option<&JsonValue>,
    new_value: Option<&JsonValue>,
    frame_kind: ProtoStateFrameKind,
    schema_hash: &str,
    snapshot: &crate::state_stream::StateSnapshot,
) -> Result<ProtoStateChange, Status> {
    let old_value = old_value
        .map(simd_json::serde::to_owned_value)
        .transpose()
        .map_err(|error| Status::data_loss(error.to_string()))?;
    let new_value = new_value
        .map(simd_json::serde::to_owned_value)
        .transpose()
        .map_err(|error| Status::data_loss(error.to_string()))?;
    Ok(ProtoStateChange {
        change_id: String::new(),
        event_id: 0,
        plugin_id: plugin_id.to_string(),
        object_path,
        change_type: change_type as i32,
        member_name,
        old_value: old_value.as_ref().map(simd_to_prost_value),
        new_value: new_value.as_ref().map(simd_to_prost_value),
        tags_touched: Vec::new(),
        event_hash: String::new(),
        timestamp: None,
        actor_id: String::new(),
        frame_kind: frame_kind as i32,
        schema_hash: schema_hash.to_string(),
        catalog_hash: snapshot.catalog_hash.clone(),
        state_generation: snapshot.generation,
    })
}

pub(crate) fn state_heartbeat_frame(
    snapshot: &crate::state_stream::StateSnapshot,
) -> ProtoStateChange {
    ProtoStateChange {
        change_id: String::new(),
        event_id: 0,
        plugin_id: String::new(),
        object_path: String::new(),
        change_type: ProtoChangeType::Unspecified as i32,
        member_name: String::new(),
        old_value: None,
        new_value: None,
        tags_touched: Vec::new(),
        event_hash: String::new(),
        timestamp: None,
        actor_id: String::new(),
        frame_kind: ProtoStateFrameKind::Heartbeat as i32,
        schema_hash: String::new(),
        catalog_hash: snapshot.catalog_hash.clone(),
        state_generation: snapshot.generation,
    }
}

#[cfg(test)]
mod public_state_frame_tests {
    use super::*;
    use crate::state_stream::{PluginSnapshot, StateSnapshot};

    fn snapshot(generation: u64, state: JsonValue) -> StateSnapshot {
        StateSnapshot {
            generation,
            catalog_hash: "c".repeat(64),
            plugins: [(
                "demo".to_string(),
                PluginSnapshot {
                    schema_hash: "s".repeat(64),
                    schema: serde_json::json!({"name":"demo"}),
                    state: Some(state),
                },
            )]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn child_frames_are_snapshot_values_without_false_audit_correlation() {
        let current = snapshot(7, serde_json::json!({"children":{"one":{"value":2}}}));
        let frames = state_snapshot_frames(
            &current,
            None,
            false,
            true,
            ProtoStateFrameKind::InitialState,
            &["/org/opdbus/v1/plugins/demo/children/one".into()],
            false,
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(
            frame.object_path,
            "/org/opdbus/v1/plugins/demo/children/one"
        );
        assert_eq!(frame.state_generation, 7);
        assert_eq!(frame.event_id, 0);
        assert!(frame.event_hash.is_empty());
        assert!(frame.change_id.is_empty());
        assert!(frame.actor_id.is_empty());
        assert!(frame.timestamp.is_none());
    }

    #[test]
    fn schema_and_lag_rehydrate_frames_carry_current_generation() {
        let previous = snapshot(6, serde_json::json!({"value":1}));
        let current = snapshot(7, serde_json::json!({"value":1}));
        let frames = state_snapshot_frames(
            &current,
            Some(&previous),
            true,
            true,
            ProtoStateFrameKind::Update,
            &[],
            true,
        )
        .unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|frame| frame.state_generation == 7));
        assert!(frames.iter().any(|frame| {
            frame.change_type == ProtoChangeType::SchemaMigration as i32
                && frame.member_name == current.plugins["demo"].schema_hash
        }));
        assert!(frames.iter().any(|frame| {
            frame.change_type == ProtoChangeType::PropertySet as i32 && frame.old_value.is_none()
        }));
    }

    #[test]
    fn lag_rehydrate_keeps_removed_child_tombstone() {
        let previous = snapshot(6, serde_json::json!({"children":{"one":{"value":1}}}));
        let current = snapshot(7, serde_json::json!({"children":{}}));
        let frames = state_snapshot_frames(
            &current,
            Some(&previous),
            false,
            true,
            ProtoStateFrameKind::Update,
            &["/org/opdbus/v1/plugins/demo/children/*".into()],
            true,
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].change_type, ProtoChangeType::ObjectRemoved as i32);
        assert_eq!(
            frames[0].object_path,
            "/org/opdbus/v1/plugins/demo/children/one"
        );
        assert!(frames[0].new_value.is_none());
    }

    #[test]
    fn removed_schema_only_plugin_emits_tombstone_without_state_subscription() {
        let previous = snapshot(6, serde_json::json!({}));
        let current = StateSnapshot {
            generation: 7,
            catalog_hash: "new-catalog".into(),
            plugins: Default::default(),
        };
        let frames = state_snapshot_frames(
            &current,
            Some(&previous),
            true,
            false,
            ProtoStateFrameKind::Update,
            &[],
            false,
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].change_type,
            ProtoChangeType::SchemaMigration as i32
        );
        assert_eq!(frames[0].member_name, "s".repeat(64));
        assert!(frames[0].schema_hash.is_empty());
        assert!(frames[0].new_value.is_none());
    }
}

pub(crate) fn proto_chain_event(event: &op_state_store::ChainEvent) -> ProtoChainEvent {
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

fn proto_timestamp(ts: DateTime<Utc>) -> ProstTimestamp {
    ProstTimestamp {
        seconds: ts.timestamp(),
        nanos: ts.timestamp_subsec_nanos() as i32,
    }
}

pub(crate) fn simd_to_prost_struct(value: &simd_json::OwnedValue) -> ProstStruct {
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

pub(crate) fn simd_to_prost_value(value: &simd_json::OwnedValue) -> ProstValue {
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

    async fn get_datapath_health(
        &self,
        request: Request<OvsdbGetDatapathHealthRequest>,
    ) -> Result<Response<OvsdbGetDatapathHealthResponse>, Status> {
        let bridge = request.into_inner().bridge_name;
        let bridge = if bridge.is_empty() {
            "ovsbr0".to_string()
        } else {
            bridge
        };
        let h = op_network::get_datapath_health(&bridge)
            .await
            .map_err(|e| Status::internal(format!("get_datapath_health: {e:#}")))?;
        let detail_json = serde_json::to_string(&h).unwrap_or_default();
        Ok(Response::new(OvsdbGetDatapathHealthResponse {
            bridge: h.bridge,
            fail_mode: h.fail_mode,
            controllers: h.controllers,
            fallback_normal: h.fallback_normal,
            fallback_priority: h.fallback_priority as u32,
            detail_json,
        }))
    }

    async fn attach_controller_safe(
        &self,
        request: Request<OvsdbAttachControllerSafeRequest>,
    ) -> Result<Response<OvsdbAttachControllerSafeResponse>, Status> {
        let req = request.into_inner();
        let bridge = if req.bridge_name.is_empty() {
            "ovsbr0".to_string()
        } else {
            req.bridge_name
        };
        let endpoint = if req.endpoint.is_empty() {
            "tcp:10.200.0.1:6653".to_string()
        } else {
            req.endpoint
        };
        match op_network::attach_controller_safe(&bridge, &endpoint).await {
            Ok(h) => {
                let detail_json = serde_json::to_string(&h).unwrap_or_default();
                Ok(Response::new(OvsdbAttachControllerSafeResponse {
                    ok: true,
                    message: format!("AttachControllerSafe ok bridge={bridge}"),
                    health: Some(OvsdbGetDatapathHealthResponse {
                        bridge: h.bridge,
                        fail_mode: h.fail_mode,
                        controllers: h.controllers,
                        fallback_normal: h.fallback_normal,
                        fallback_priority: h.fallback_priority as u32,
                        detail_json,
                    }),
                }))
            }
            Err(e) => Ok(Response::new(OvsdbAttachControllerSafeResponse {
                ok: false,
                message: format!("{e:#}"),
                health: None,
            })),
        }
    }

    async fn ensure_fallback_normal(
        &self,
        request: Request<OvsdbEnsureFallbackNormalRequest>,
    ) -> Result<Response<OvsdbEnsureFallbackNormalResponse>, Status> {
        let bridge = request.into_inner().bridge_name;
        let bridge = if bridge.is_empty() {
            "ovsbr0".to_string()
        } else {
            bridge
        };
        match op_network::ensure_fallback_normal(&bridge).await {
            Ok(()) => Ok(Response::new(OvsdbEnsureFallbackNormalResponse {
                ok: true,
                message: format!("EnsureFallbackNormal ok bridge={bridge}"),
            })),
            Err(e) => Ok(Response::new(OvsdbEnsureFallbackNormalResponse {
                ok: false,
                message: format!("{e:#}"),
            })),
        }
    }

    async fn del_controller(
        &self,
        request: Request<OvsdbDelControllerRequest>,
    ) -> Result<Response<OvsdbDelControllerResponse>, Status> {
        let bridge = request.into_inner().bridge_name;
        let bridge = if bridge.is_empty() {
            "ovsbr0".to_string()
        } else {
            bridge
        };
        match op_network::del_controller(&bridge).await {
            Ok(()) => Ok(Response::new(OvsdbDelControllerResponse {
                ok: true,
                message: format!("DelController ok bridge={bridge}"),
            })),
            Err(e) => Ok(Response::new(OvsdbDelControllerResponse {
                ok: false,
                message: format!("{e:#}"),
            })),
        }
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
                fallback_normal: false,
                controllers: vec![],
            });
        }

        Ok(bridges)
    }

    /// Extract UUID set from OVSDB value (["set", [...]] or ["uuid", "..."])
    fn extract_uuid_set(&self, value: Option<&serde_json::Value>) -> Vec<String> {
        let Some(v) = value else {
            return vec![];
        };
        if let Some(arr) = v.as_array()
            && arr.len() == 2
        {
            if arr[0].as_str() == Some("uuid") {
                return vec![arr[1].as_str().unwrap_or("").to_string()];
            }
            if arr[0].as_str() == Some("set")
                && let Some(items) = arr[1].as_array()
            {
                return items
                    .iter()
                    .filter_map(|item| {
                        if let Some(inner) = item.as_array()
                            && inner.len() == 2
                            && inner[0].as_str() == Some("uuid")
                        {
                            return inner[1].as_str().map(|s| s.to_string());
                        }
                        None
                    })
                    .collect();
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
        if let Some(arr) = v.as_array()
            && arr.len() == 2
            && arr[0].as_str() == Some("map")
            && let Some(pairs) = arr[1].as_array()
        {
            for pair in pairs {
                if let Some(kv) = pair.as_array()
                    && kv.len() == 2
                {
                    let k = kv[0].as_str().unwrap_or("").to_string();
                    let v = kv[1].as_str().unwrap_or("").to_string();
                    map.insert(k, v);
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
        _request: Request<RuntimeStreamMetricsRequest>,
    ) -> Result<Response<Self::StreamMetricsStream>, Status> {
        Err(Status::unimplemented(
            "runtime metrics require a canonical state subscription",
        ))
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

    async fn check_unix_sockets(
        &self,
        request: Request<crate::proto::RuntimeCheckUnixSocketsRequest>,
    ) -> Result<Response<crate::proto::RuntimeCheckUnixSocketsResponse>, Status> {
        let mut sockets = Vec::new();
        for path in request.into_inner().paths {
            let exists = std::path::Path::new(&path).exists();
            let connectable = exists && std::os::unix::net::UnixStream::connect(&path).is_ok();
            sockets.push(crate::proto::RuntimeUnixSocketStatus {
                path,
                exists,
                connectable,
                detail: String::new(),
            });
        }
        Ok(Response::new(
            crate::proto::RuntimeCheckUnixSocketsResponse {
                sockets,
                queried_at: Some(proto_timestamp(Utc::now())),
            },
        ))
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
    ComponentInfo, ComponentStatus, DeregisterRequest, DeregisterResponse, DiscoverRequest,
    DiscoverResponse, GetComponentRequest, GetComponentResponse, HeartbeatRequest,
    HeartbeatResponse, RegisterRequest, RegisterResponse, RegistryEvent, RegistryEventType,
    WatchRequest, component_registry_server::ComponentRegistry,
};

#[tonic::async_trait]
impl ComponentRegistry for OperationGrpcServer {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<RegistryEvent, Status>> + Send + 'static>>;

    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        authorize_component_registration(&request)?;
        let req = request.into_inner();
        if req.component_id.is_empty() {
            return Err(Status::invalid_argument("component_id must not be empty"));
        }

        let mut inner = self.registry.write().await;

        // RegisterRequest carries no prior lease or verified owner grant. Do
        // not let another caller replace an existing endpoint advertisement.
        inner.ensure_registration_vacant(&req.component_id)?;
        let lease_token = Uuid::new_v4().to_string();
        let now = OperationGrpcServer::now_ts();
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

        let event = RegistryEvent {
            event_type: RegistryEventType::RegistryEventRegistered as i32,
            component: Some(info),
            timestamp: Some(now),
        };
        // Ignore send error — no active watchers is fine.
        let _ = inner.watch_tx.send(event);

        info!(
            component_id = %req.component_id,
            component_type = %req.component_type,
            "component registered"
        );

        Ok(Response::new(RegisterResponse {
            success: true,
            message: "registered".to_string(),
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
                }));
            }
            Some(stored) if stored != &req.lease_token => {
                return Err(Status::permission_denied("invalid lease token"));
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
                return Err(Status::permission_denied("invalid lease token"));
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

        if was_stale && let Some(info) = inner.components.get(&req.component_id).cloned() {
            let event = RegistryEvent {
                event_type: RegistryEventType::RegistryEventRecovered as i32,
                component: Some(info),
                timestamp: Some(now),
            };
            let _ = inner.watch_tx.send(event);
            info!(component_id = %req.component_id, "component recovered from stale");
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
    AdminMailActionRequest, AdminMailActionResponse, CheckMailServerRequest,
    CheckMailServerResponse, GetInboxRequest, GetInboxResponse, GetMailStatusRequest,
    GetMailStatusResponse, GetMessageRequest, GetMessageResponse, ListMailAccountsRequest,
    ListMailAccountsResponse, SendEmailRequest, SendEmailResponse,
    mail_service_server::MailService,
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
        if let Some(ref st) = state
            && let Some(status_obj) = st.as_object().and_then(|o| o.get("status"))
        {
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
    ConfigurePacketRoutingRequest, ConfigurePacketRoutingResponse, EnsurePrivacyNetworkRequest,
    EnsurePrivacyNetworkResponse, GenerateWireGuardKeyPairRequest,
    GenerateWireGuardKeyPairResponse, GetNetworkStatusRequest, GetNetworkStatusResponse,
    GetNetworkTopologyRequest, GetNetworkTopologyResponse, GetPrivacyWireGuardConfigRequest,
    GetPrivacyWireGuardConfigResponse, HealthCheckRequest, HealthCheckResponse,
    ManageComponentRequest, ManageComponentResponse, ProvisionUserRequest, ProvisionUserResponse,
    privacy_network_service_server::PrivacyNetworkService,
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
        if let Some(ref st) = state
            && let Some(net_status) = st.as_object().and_then(|o| o.get("network_status"))
        {
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
    AdminUserActionRequest, AdminUserActionResponse, GetUserStatusRequest, GetUserStatusResponse,
    GetWireGuardConfigRequest, GetWireGuardConfigResponse, ListUsersRequest, ListUsersResponse,
    RegisterUserRequest, RegisterUserResponse, SendMagicLinkRequest, SendMagicLinkResponse,
    VerifyMagicLinkRequest, VerifyMagicLinkResponse,
    registration_service_server::RegistrationService,
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
                if let Some(ref st) = state
                    && let Some(link_data) = st
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
        if let Some(ref st) = state
            && let Some(users) = st.as_object().and_then(|o| o.get("users"))
        {
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
        let target = request.get_ref();
        let plugin_id = passthrough_plugin_id(
            &target.bus,
            &target.destination,
            &target.path,
            &target.interface,
        )?;
        let capability = declared_capability(&request)
            .ok_or_else(|| Status::permission_denied("declared plugin capability is required"))?
            .to_string();
        let body: JsonValue = serde_json::from_str(&target.json_body)
            .map_err(|error| Status::invalid_argument(format!("invalid JSON body: {error}")))?;
        let body = simd_json::serde::to_owned_value(&body)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let request = request.map(|req| CallMethodRequest {
            plugin_id,
            object_path: req.path,
            interface_name: req.interface,
            method_name: req.method,
            arguments: vec![simd_to_prost_value(&body)],
            actor_id: String::new(),
            capability_id: capability,
        });
        let result = <Self as PluginService>::call_method(self, request)
            .await?
            .into_inner();
        if !result.success {
            return Err(Status::permission_denied(
                result
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "plugin method was denied".to_string()),
            ));
        }
        let value = result
            .result
            .ok_or_else(|| Status::internal("plugin result is missing"))?;
        let json_result = simd_json::to_string(&prost_value_to_simd(&value))
            .map_err(|error| Status::internal(error.to_string()))?;

        Ok(Response::new(crate::proto::DbusCallResponse {
            success: true,
            json_result,
            error: String::new(),
        }))
    }

    async fn get(
        &self,
        request: Request<crate::proto::DbusGetPropertyRequest>,
    ) -> Result<Response<crate::proto::DbusGetPropertyResponse>, Status> {
        let target = request.get_ref();
        let plugin_id = passthrough_plugin_id(
            &target.bus,
            &target.destination,
            &target.path,
            &target.interface,
        )?;
        let request = request.map(|req| GetPropertyRequest {
            plugin_id,
            object_path: req.path,
            interface_name: req.interface,
            property_name: req.property,
        });
        let result = <Self as PluginService>::get_property(self, request)
            .await?
            .into_inner();
        let value = result
            .value
            .ok_or_else(|| Status::internal("property value is missing"))?;
        let json = simd_json::to_string(&prost_value_to_simd(&value))
            .map_err(|error| Status::internal(error.to_string()))?;

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
        let target = request.get_ref();
        let plugin_id = passthrough_plugin_id(
            &target.bus,
            &target.destination,
            &target.path,
            &target.interface,
        )?;
        let capability = declared_capability(&request)
            .ok_or_else(|| Status::permission_denied("declared plugin capability is required"))?
            .to_string();
        let value: serde_json::Value = serde_json::from_str(&target.json_value)
            .map_err(|e| Status::invalid_argument(format!("bad JSON: {e}")))?;
        let value = simd_json::serde::to_owned_value(&value)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let request = request.map(|req| SetPropertyRequest {
            plugin_id,
            object_path: req.path,
            interface_name: req.interface,
            property_name: req.property,
            value: Some(simd_to_prost_value(&value)),
            actor_id: String::new(),
            capability_id: capability,
        });
        let result = <Self as PluginService>::set_property(self, request)
            .await?
            .into_inner();
        if !result.success {
            return Err(Status::permission_denied(
                result
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "property write was denied".to_string()),
            ));
        }

        Ok(Response::new(crate::proto::DbusSetPropertyResponse {
            success: true,
            error: String::new(),
        }))
    }

    async fn watch(
        &self,
        _request: Request<crate::proto::DbusWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        Err(Status::unimplemented(
            "legacy signal watch has no sealed capability declaration; use PluginService",
        ))
    }
}
