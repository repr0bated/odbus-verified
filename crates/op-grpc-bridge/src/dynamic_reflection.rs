//! Active blob-backed gRPC reflection.
//!
//! `tonic_reflection::server::Builder` freezes its descriptor index at build
//! time. This module keeps the descriptor files indexed, but advertises service
//! names from the active plugin object blobs so discovery follows D-Bus object
//! lifetime.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use prost::Message;
use prost_types::{
    DescriptorProto, EnumDescriptorProto, FieldDescriptorProto, FileDescriptorProto,
    FileDescriptorSet,
};
use tokio::sync::{RwLock, mpsc};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::{Request, Response, Status, Streaming};
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
use tonic_reflection::pb::v1::server_reflection_server::{
    ServerReflection, ServerReflectionServer,
};
use tonic_reflection::pb::v1::{
    ExtensionNumberResponse, FileDescriptorResponse, ListServiceResponse, ServerReflectionRequest,
    ServerReflectionResponse, ServiceResponse,
};

use crate::plugin_object_blob::PluginObjectBlob;

#[derive(Debug, Clone)]
pub struct ActiveReflectionCatalog {
    inner: Arc<RwLock<ActiveReflectionInner>>,
    shm_dir: PathBuf,
}

impl Default for ActiveReflectionCatalog {
    fn default() -> Self {
        Self::with_dir(crate::sealed_schema_reader::blob_catalog_dir_from_env())
    }
}

#[derive(Debug, Clone)]
struct ActiveReflectionInner {
    descriptor_sets: Vec<Vec<u8>>,
    blobs: BTreeMap<String, PluginObjectBlob>,
    index: ReflectionIndex,
    /// Last stored catalog hash, used only for change logging. Schema hashes
    /// do not cover descriptor/manifest producer revisions, so this must not
    /// suppress arrival-triggered replacement of the pinned artifacts.
    catalog_hash: Option<String>,
}

impl Default for ActiveReflectionInner {
    fn default() -> Self {
        let mut inner = Self {
            descriptor_sets: Vec::new(),
            blobs: BTreeMap::new(),
            index: ReflectionIndex::default(),
            catalog_hash: None,
        };
        inner
            .descriptor_sets
            .push(crate::proto::FILE_DESCRIPTOR_SET.to_vec());
        inner
            .descriptor_sets
            .push(tonic_reflection::pb::v1::FILE_DESCRIPTOR_SET.to_vec());
        inner.rebuild_index();
        inner
    }
}

impl ActiveReflectionCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dir(shm_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ActiveReflectionInner::default())),
            shm_dir,
        }
    }

    pub(crate) fn catalog_dir(&self) -> &std::path::Path {
        &self.shm_dir
    }

    /// Arrival-triggered resync reads the complete manifest-pinned artifacts.
    /// The sealer commits its manifest last. Every arrival replaces the index,
    /// even when schema/catalog hashes are unchanged: a producer revision can
    /// change descriptor or manifest bytes without changing schema JSON. No
    /// watcher, polling loop, consumer hash, or parallel revision authority.
    pub async fn sync_from_shm(&self) -> usize {
        // Serialize snapshot loads so an older concurrent reader cannot
        // overwrite newer advertisements. Even an unchanged manifest is
        // checked for missing/corrupt pinned files; no directory is scanned.
        let mut inner = self.inner.write().await;
        let directory = self.shm_dir.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            crate::sealed_schema_reader::read_manifest_pinned_catalog(&directory)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|result| result);
        let catalog = match loaded {
            Ok(catalog) => catalog,
            Err(error) => {
                tracing::warn!(%error, "sealed reflection catalog unavailable or invalid; clearing plugin advertisements");
                inner.blobs.clear();
                inner.catalog_hash = None;
                inner.rebuild_index();
                return 0;
            }
        };
        let count = catalog.plugins.len();
        if inner.catalog_hash.as_deref() != Some(catalog.catalog_hash.as_str()) {
            tracing::info!(count, "reflection loaded committed sealed catalog");
        }
        inner.blobs = catalog
            .plugins
            .into_iter()
            .map(|(id, document)| (id, document.blob))
            .collect();
        inner.catalog_hash = Some(catalog.catalog_hash);
        inner.rebuild_index();
        count
    }

    pub async fn list_services(&self) -> Vec<String> {
        self.inner.read().await.index.service_names.clone()
    }

    async fn file_by_filename(&self, filename: &str) -> Result<Vec<u8>, Status> {
        self.inner.read().await.index.file_by_filename(filename)
    }

    async fn symbol_by_name(&self, symbol: &str) -> Result<Vec<u8>, Status> {
        self.inner.read().await.index.symbol_by_name(symbol)
    }
}

impl ActiveReflectionInner {
    fn rebuild_index(&mut self) {
        // A sealed blob proves plugin presence, not that every descriptor in
        // it has a mounted RPC handler. The blob's operation.method.* entries
        // are schema metadata; add_routes mounts operation.plugin.v1.* instead.
        // Advertise only those mounted services, gated by live blob presence.
        // This table is generated alongside add_routes, not an existence check
        // against the Rust plugin registry. Blob descriptors remain readable.
        let active_services = crate::grpc_server::LEGACY_PLUGIN_METHOD_SERVICES
            .iter()
            .filter(|(plugin_id, _)| self.blobs.contains_key(*plugin_id))
            .map(|(_, service)| service.to_string())
            .collect::<BTreeSet<_>>();

        let mut index = ReflectionIndex::new(active_services);

        for encoded in self
            .descriptor_sets
            .iter()
            .chain(self.blobs.values().map(|blob| &blob.descriptor_set))
        {
            let Ok(file_set) = FileDescriptorSet::decode(encoded.as_slice()) else {
                continue;
            };
            index.add_file_descriptor_set(file_set);
        }

        self.index = index;
    }
}

#[derive(Debug, Clone, Default)]
struct ReflectionIndex {
    active_services: BTreeSet<String>,
    service_names: Vec<String>,
    files: HashMap<String, Arc<FileDescriptorProto>>,
    symbols: HashMap<String, Arc<FileDescriptorProto>>,
}

impl ReflectionIndex {
    fn new(active_services: BTreeSet<String>) -> Self {
        Self {
            active_services,
            service_names: Vec::new(),
            files: HashMap::new(),
            symbols: HashMap::new(),
        }
    }

    fn add_file_descriptor_set(&mut self, file_set: FileDescriptorSet) {
        for file in file_set.file {
            let Some(name) = file.name.clone() else {
                continue;
            };
            if self.files.contains_key(&name) {
                continue;
            }

            let file = Arc::new(file);
            self.files.insert(name, file.clone());
            self.process_file(file);
        }

        self.service_names.sort();
        self.service_names.dedup();
    }

    fn process_file(&mut self, file: Arc<FileDescriptorProto>) {
        let prefix = file.package.clone().unwrap_or_default();

        for message in &file.message_type {
            self.process_message(file.clone(), &prefix, message);
        }
        for enum_type in &file.enum_type {
            self.process_enum(file.clone(), &prefix, enum_type);
        }
        for service in &file.service {
            let Some(service_name) = extract_name(&prefix, service.name.as_ref()) else {
                continue;
            };
            if self.active_services.contains(&service_name)
                || service_name == "grpc.reflection.v1.ServerReflection"
            {
                self.service_names.push(service_name.clone());
            }
            self.symbols.insert(service_name.clone(), file.clone());

            for method in &service.method {
                if let Some(method_name) = extract_name(&service_name, method.name.as_ref()) {
                    self.symbols.insert(method_name, file.clone());
                }
            }
        }
    }

    fn process_message(
        &mut self,
        file: Arc<FileDescriptorProto>,
        prefix: &str,
        message: &DescriptorProto,
    ) {
        let Some(message_name) = extract_name(prefix, message.name.as_ref()) else {
            return;
        };
        self.symbols.insert(message_name.clone(), file.clone());

        for nested in &message.nested_type {
            self.process_message(file.clone(), &message_name, nested);
        }
        for enum_type in &message.enum_type {
            self.process_enum(file.clone(), &message_name, enum_type);
        }
        for field in &message.field {
            self.process_field(file.clone(), &message_name, field);
        }
        for oneof in &message.oneof_decl {
            if let Some(oneof_name) = extract_name(&message_name, oneof.name.as_ref()) {
                self.symbols.insert(oneof_name, file.clone());
            }
        }
    }

    fn process_enum(
        &mut self,
        file: Arc<FileDescriptorProto>,
        prefix: &str,
        enum_type: &EnumDescriptorProto,
    ) {
        let Some(enum_name) = extract_name(prefix, enum_type.name.as_ref()) else {
            return;
        };
        self.symbols.insert(enum_name.clone(), file.clone());

        for value in &enum_type.value {
            if let Some(value_name) = extract_name(&enum_name, value.name.as_ref()) {
                self.symbols.insert(value_name, file.clone());
            }
        }
    }

    fn process_field(
        &mut self,
        file: Arc<FileDescriptorProto>,
        prefix: &str,
        field: &FieldDescriptorProto,
    ) {
        if let Some(field_name) = extract_name(prefix, field.name.as_ref()) {
            self.symbols.insert(field_name, file);
        }
    }

    // tonic::Status is 176 bytes; Boxing the error would ripple through the
    // shared reflection surface for zero runtime benefit, so the large Err is
    // the deliberate disposition on these three lookup helpers.
    #[allow(clippy::result_large_err)]
    fn symbol_by_name(&self, symbol: &str) -> Result<Vec<u8>, Status> {
        self.encode_file(symbol, self.symbols.get(symbol), "symbol")
    }

    #[allow(clippy::result_large_err)]
    fn file_by_filename(&self, filename: &str) -> Result<Vec<u8>, Status> {
        self.encode_file(filename, self.files.get(filename), "file")
    }

    #[allow(clippy::result_large_err)]
    fn encode_file(
        &self,
        name: &str,
        file: Option<&Arc<FileDescriptorProto>>,
        kind: &str,
    ) -> Result<Vec<u8>, Status> {
        let Some(file) = file else {
            return Err(Status::not_found(format!("{kind} '{name}' not found")));
        };
        let mut encoded = Vec::new();
        file.encode(&mut encoded)
            .map_err(|_| Status::internal("failed to encode descriptor"))?;
        Ok(encoded)
    }
}

fn extract_name(prefix: &str, name: Option<&String>) -> Option<String> {
    let name = name?;
    if prefix.is_empty() {
        Some(name.to_string())
    } else {
        Some(format!("{prefix}.{name}"))
    }
}

#[derive(Debug, Clone)]
pub struct DynamicReflectionService {
    catalog: ActiveReflectionCatalog,
}

impl DynamicReflectionService {
    pub fn new(catalog: ActiveReflectionCatalog) -> Self {
        Self { catalog }
    }

    pub fn into_v1_server(self) -> ServerReflectionServer<Self> {
        ServerReflectionServer::new(self)
    }
}

#[tonic::async_trait]
impl ServerReflection for DynamicReflectionService {
    type ServerReflectionInfoStream = ReceiverStream<Result<ServerReflectionResponse, Status>>;

    async fn server_reflection_info(
        &self,
        request: Request<Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<Self::ServerReflectionInfoStream>, Status> {
        let mut request_rx = request.into_inner();
        let (response_tx, response_rx) =
            mpsc::channel::<Result<ServerReflectionResponse, Status>>(1);
        let catalog = self.catalog.clone();

        tokio::spawn(async move {
            while let Some(request) = request_rx.next().await {
                let Ok(request) = request else {
                    return;
                };

                // Arrival-triggered: pick up blobs sealed by the op-blob
                // binary since startup (no-op when the manifest is unchanged).
                catalog.sync_from_shm().await;

                let response = match request.message_request.clone() {
                    None => Err(Status::invalid_argument("invalid MessageRequest")),
                    Some(MessageRequest::FileByFilename(filename)) => catalog
                        .file_by_filename(&filename)
                        .await
                        .map(file_descriptor_response),
                    Some(MessageRequest::FileContainingSymbol(symbol)) => catalog
                        .symbol_by_name(&symbol)
                        .await
                        .map(file_descriptor_response),
                    Some(MessageRequest::FileContainingExtension(_)) => {
                        Err(Status::not_found("extensions are not supported"))
                    }
                    Some(MessageRequest::AllExtensionNumbersOfType(_)) => {
                        Ok(MessageResponse::AllExtensionNumbersResponse(
                            ExtensionNumberResponse::default(),
                        ))
                    }
                    Some(MessageRequest::ListServices(_)) => {
                        let services = catalog
                            .list_services()
                            .await
                            .into_iter()
                            .map(|name| ServiceResponse { name })
                            .collect();
                        Ok(MessageResponse::ListServicesResponse(ListServiceResponse {
                            service: services,
                        }))
                    }
                };

                let send_result = match response {
                    Ok(message_response) => {
                        response_tx
                            .send(Ok(ServerReflectionResponse {
                                valid_host: request.host.clone(),
                                original_request: Some(request),
                                message_response: Some(message_response),
                            }))
                            .await
                    }
                    Err(status) => response_tx.send(Err(status)).await,
                };

                if send_result.is_err() {
                    return;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(response_rx)))
    }
}

fn file_descriptor_response(file: Vec<u8>) -> MessageResponse {
    MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
        file_descriptor_proto: vec![file],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retired_plugin_pinned_by_a_stale_manifest_is_not_advertised() {
        let dir = tempfile::tempdir().unwrap();
        let seal = |id: &str| {
            let schema = op_state_store::PluginSchema::builder(id)
                .version("1.0.0")
                .method(op_state_store::MethodDecl {
                    name: "Read".into(),
                    args: simd_json::json!({"type":"object", "properties":{}}),
                    returns: Some(simd_json::json!({"type":"object", "properties":{}})),
                    side_effect: op_state_store::SideEffect::Read,
                    idempotent: true,
                    required_capability: Some("fixture.read".into()),
                    subid: "obs.service.fixture.read@v1".into(),
                })
                .build();
            op_blob::blobify_plugin_schema(id, schema)
        };
        let mut producer = op_blob::BlobStore::open(dir.path()).unwrap();
        producer.write(&seal("chat_router")).unwrap();
        // Simulates a catalog sealed before `factory` was retired.
        producer.write(&seal("factory")).unwrap();

        // The manifest pins both blobs; the manifest-pinned reader drops the
        // retired id before reflection ever sees it (sealed_schema_reader).
        let catalog = ActiveReflectionCatalog::with_dir(dir.path().into());
        assert_eq!(
            catalog.sync_from_shm().await,
            1,
            "only the live plugin is loaded"
        );
        let services = catalog.list_services().await;
        assert!(
            services.iter().any(|s| s.contains("ChatRouter")),
            "live plugin advertised: {services:?}"
        );
        assert!(
            !services
                .iter()
                .any(|s| s.to_ascii_lowercase().contains("factory")),
            "retired plugin must not be advertised: {services:?}"
        );
    }

    #[tokio::test]
    async fn same_schema_and_catalog_hash_refreshes_changed_valid_descriptor_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let schema = op_state_store::PluginSchema::builder("chat_router")
            .version("1.0.0")
            .method(op_state_store::MethodDecl {
                name: "Read".into(),
                args: simd_json::json!({"type":"object", "properties":{}}),
                returns: Some(simd_json::json!({"type":"object", "properties":{}})),
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: Some("fixture.read".into()),
                subid: "obs.service.fixture.read@v1".into(),
            })
            .build();
        let template = op_blob::blobify_plugin_schema("chat_router", schema);
        // Model two valid sealer code-generation revisions. The source schema
        // stays identical while a generated helper message changes. Only the
        // local producer fixture writes artifacts and the committed manifest.
        let revision = |helper: &str| {
            let mut blob = template.clone();
            let mut descriptors =
                FileDescriptorSet::decode(blob.descriptor_set.as_slice()).unwrap();
            let file = &mut descriptors.file[0];
            let symbol = format!("{}.{}", file.package.as_deref().unwrap(), helper);
            file.message_type.push(DescriptorProto {
                name: Some(helper.into()),
                ..Default::default()
            });
            blob.manifest.methods[0].symbols.push(symbol.clone());
            blob.descriptor_set = descriptors.encode_to_vec();
            (blob, symbol)
        };
        let (first, old_symbol) = revision("ArtifactRevisionOne");
        let (second, new_symbol) = revision("ArtifactRevisionTwo");
        assert_eq!(first.schema_json, second.schema_json);
        assert_eq!(first.manifest.schema_hash, second.manifest.schema_hash);
        assert_eq!(first.file_name(), second.file_name());
        assert_ne!(first.descriptor_set, second.descriptor_set);

        let mut producer = op_blob::BlobStore::open(dir.path()).unwrap();
        producer.write(&first).unwrap();
        let manifest_path = dir.path().join(op_blob::catalog::MANIFEST_FILENAME);
        let first_manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let catalog = ActiveReflectionCatalog::with_dir(dir.path().into());
        assert_eq!(catalog.sync_from_shm().await, 1);
        assert!(catalog.symbol_by_name(&old_symbol).await.is_ok());
        assert!(catalog.symbol_by_name(&new_symbol).await.is_err());

        producer.write(&second).unwrap();
        let second_manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        assert_eq!(
            first_manifest["catalog_hash"],
            second_manifest["catalog_hash"]
        );
        assert!(
            second_manifest["generation"].as_u64().unwrap()
                > first_manifest["generation"].as_u64().unwrap()
        );
        assert_eq!(catalog.sync_from_shm().await, 1);
        assert!(catalog.symbol_by_name(&old_symbol).await.is_err());
        let indexed = catalog.symbol_by_name(&new_symbol).await.unwrap();
        let expected = FileDescriptorSet::decode(second.descriptor_set.as_slice())
            .unwrap()
            .file
            .remove(0)
            .encode_to_vec();
        assert_eq!(indexed, expected);
        assert_eq!(
            catalog
                .file_by_filename(&second.manifest.methods[0].grpc_file)
                .await
                .unwrap(),
            expected
        );
        assert!(
            catalog
                .list_services()
                .await
                .iter()
                .any(|service| service == "operation.plugin.v1.ChatRouterPluginMethods")
        );
    }

    #[tokio::test]
    async fn active_reflection_advertises_only_active_blob_services() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = ActiveReflectionCatalog::with_dir(dir.path().into());
        let before = catalog.list_services().await;
        assert!(
            before
                .iter()
                .any(|service| service == "grpc.reflection.v1.ServerReflection")
        );
        assert!(
            !before
                .iter()
                .any(|service| service == "operation.plugin.v1.ChatRouterPluginMethods")
        );

        let mut sealer = op_blob::BlobStore::open(dir.path()).unwrap();
        sealer
            .write(&crate::chat_router_object_blob::from_plugin_schema())
            .unwrap();
        catalog.sync_from_shm().await;
        let after = catalog.list_services().await;
        // Legacy service (mounted via build.rs) is advertised.
        assert!(
            after
                .iter()
                .any(|service| service == "operation.plugin.v1.ChatRouterPluginMethods")
        );
        assert!(after.iter().all(|service| {
            service == "grpc.reflection.v1.ServerReflection"
                || service == "operation.plugin.v1.ChatRouterPluginMethods"
        }));

        sealer.remove_blob("chat_router").unwrap();
        catalog.sync_from_shm().await;
        let removed = catalog.list_services().await;
        assert!(
            !removed
                .iter()
                .any(|service| service == "operation.plugin.v1.ChatRouterPluginMethods")
        );
    }

    #[tokio::test]
    async fn sync_from_shm_picks_up_blobs_sealed_by_external_writer() {
        let dir = std::env::temp_dir().join(format!("op-reflection-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let catalog = ActiveReflectionCatalog::with_dir(dir.clone());
        let before = catalog.list_services().await;
        assert_eq!(
            before,
            vec!["grpc.reflection.v1.ServerReflection".to_string()]
        );

        // External sealer (the op-blob binary's role): seals the blob and
        // commits the manifest — no call into the bridge.
        let blob = crate::chat_router_object_blob::from_plugin_schema();
        let descriptor_only_services = blob.manifest.grpc.services.clone();
        assert!(!descriptor_only_services.is_empty());
        {
            let mut store = op_blob::BlobStore::open(&dir).unwrap();
            store.write(&blob).unwrap();
        }

        catalog.sync_from_shm().await;
        let after = catalog.list_services().await;
        assert!(
            after
                .iter()
                .any(|service| service == "operation.plugin.v1.ChatRouterPluginMethods")
        );
        for service in &descriptor_only_services {
            assert!(
                !after.iter().any(|s| s == service),
                "unmounted descriptor {service} advertised after SHM resync"
            );
        }

        // Removal by the external sealer propagates on the next arrival too.
        {
            let mut store = op_blob::BlobStore::open(&dir).unwrap();
            store.remove_blob("chat_router").unwrap();
        }
        catalog.sync_from_shm().await;
        let removed = catalog.list_services().await;
        assert_eq!(
            removed,
            vec!["grpc.reflection.v1.ServerReflection".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
