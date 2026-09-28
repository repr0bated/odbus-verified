//! Schema-Driven Dynamic gRPC Router
//!
//! Reads the sealed blob catalog from `/dev/shm/opdbus/plugin-blobs`.
//! Dynamically routes incoming gRPC calls to their corresponding D-Bus objects
//! at `/org/opdbus/v1/plugins/<plugin>`.
//!
//! Every plugin defined in the schema catalog is automatically exposed as a
//! gRPC-callable service through the bridge. Unknown methods are rejected by
//! schema validation — no hand-rolling required.
//!
//! ## Schema Access Path
//!
//! **Sealed blob catalog:** `/dev/shm/opdbus/plugin-blobs` — a blob in the catalog IS the plugin.
//!
//! The manifest hash is read, never re-computed. Consumers trust the manifest.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::dbus_identity::DbusIdentityResolver;
// Import exact-principal capability enforcement (VAL-ENFORCE-002).
use crate::interceptor::{load_capability_grants, GhostbridgeIdentity};
use zbus::{message::Header, Connection, Proxy};

use crate::mutation_engine::MutationEngine;

use op_plugins::canonical::{self, BASE_SERVICE_NAME};

// The blob catalog path is resolved by sealed_schema_reader, which also
// supplies the exact manifest-pinned schemas used by method admission.

/// A schema-derived route entry for a plugin.
#[derive(Debug, Clone)]
pub struct PluginRoute {
    /// Plugin identifier (e.g. "unix_socket", "wireguard").
    pub plugin_id: String,
    /// Canonical D-Bus object path: `/org/opdbus/v1/plugins/<plugin_id>`.
    pub dbus_path: String,
    /// Canonical D-Bus destination (bus name): `org.opdbus.v1.plugins`.
    pub dbus_destination: String,
    /// Canonical D-Bus interface: `org.opdbus.v1.PluginV1`.
    pub dbus_interface: String,
    /// Methods declared in the schema (name → `MethodDecl` JSON).
    pub methods: HashMap<String, JsonValue>,
    /// Signals declared in the schema (each entry a `SignalDecl` JSON object).
    pub signals: Vec<JsonValue>,
    /// Properties declared in the schema (name → field schema).
    pub properties: HashMap<String, JsonValue>,
}

/// The dynamic schema router that maps plugin_id → D-Bus route.
pub struct SchemaRouter {
    /// Plugin routes keyed by plugin_id.
    routes: Arc<RwLock<HashMap<String, PluginRoute>>>,
    /// D-Bus system connection (shared with MutationEngine).
    dbus_connection: Arc<tokio::sync::OnceCell<Connection>>,
    /// Catalog path: blob-catalog dir in production, or a monolith file in tests.
    monolith_path: PathBuf,
    /// Directory containing MutationEngine's committed public projections.
    state_dir: PathBuf,
    /// Producer manifest path within the configured catalog.
    manifest_path: PathBuf,
    /// Last-seen `catalog_hash` read from the manifest. Compared on each
    /// inbound connection; a change triggers a reload. `None` when no manifest
    /// has been observed yet.
    cached_hash: Arc<RwLock<Option<String>>>,
    /// The MutationEngine used for real method dispatch (Requirement 5).
    /// `None` in unit tests that do not exercise the dispatch path.
    engine: Option<Arc<MutationEngine>>,
    /// Shared sender/session resolver for every PluginV1 object. Production
    /// installs it only after the NameOwnerChanged monitor is live.
    identity_resolver: Option<Arc<DbusIdentityResolver>>,
}

impl SchemaRouter {
    /// Create a new SchemaRouter and load routes from both schema sources.
    ///
    /// Loads synchronously at startup — the schema in /dev/shm is authoritative
    /// present-state and is always available. No polling or watchers.
    ///
    /// The last-seen `catalog_hash` is read from the manifest at construction
    /// so the first inbound connection only reloads if the catalog actually
    /// changed since startup.
    pub fn new(dbus_connection: Arc<tokio::sync::OnceCell<Connection>>) -> Self {
        let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        Self::with_paths(
            dbus_connection,
            catalog_dir.clone(),
            PathBuf::from(DEFAULT_SHM_STATE_DIR),
            catalog_dir.join(".manifest.json"),
        )
    }

    /// Create a new SchemaRouter wired to a MutationEngine for real dispatch.
    ///
    /// This is the production constructor: the bridge passes its
    /// `Arc<MutationEngine>` so every registered `SchemaBackedInterface` can
    /// dispatch method calls through `MutationEngine::dispatch_method_call`
    /// (Requirement 5 / VAL-DISPATCH-001).
    pub fn with_engine(
        dbus_connection: Arc<tokio::sync::OnceCell<Connection>>,
        engine: Arc<MutationEngine>,
    ) -> Self {
        let mut router = Self::new(dbus_connection);
        router.engine = Some(engine);
        router
    }

    pub fn with_identity_resolver(mut self, resolver: Arc<DbusIdentityResolver>) -> Self {
        self.identity_resolver = Some(resolver);
        self
    }

    /// Create a SchemaRouter with explicit SHM paths.
    ///
    /// In production, use [`new`](Self::new) which targets the canonical
    /// `/dev/shm` layout. Tests supply temporary paths to exercise the
    /// manifest-hash change-detection path without touching live SHM.
    pub fn with_paths(
        dbus_connection: Arc<tokio::sync::OnceCell<Connection>>,
        monolith_path: PathBuf,
        state_dir: PathBuf,
        manifest_path: PathBuf,
    ) -> Self {
        let routes = Self::load_routes_from(&monolith_path);
        let cached_hash = Self::read_manifest_hash_at(&manifest_path);
        info!(
            count = routes.len(),
            "SchemaRouter initialized from schema catalog"
        );
        Self {
            routes: Arc::new(RwLock::new(routes)),
            dbus_connection,
            monolith_path,
            state_dir,
            manifest_path,
            cached_hash: Arc::new(RwLock::new(cached_hash)),
            engine: None,
            identity_resolver: None,
        }
    }

    /// Register D-Bus objects for all plugins in the catalog.
    ///
    /// This makes the bridge the authoritative creator of the D-Bus objects
    /// defined in the schema.
    pub async fn register_objects(&self) -> anyhow::Result<()> {
        let conn = self
            .dbus_connection
            .get()
            .ok_or_else(|| anyhow::anyhow!("D-Bus connection not initialized"))?;

        let routes = self.routes.read().await;
        for (plugin_id, route) in routes.iter() {
            let mut interface = SchemaBackedInterface::with_engine_and_storage(
                plugin_id.clone(),
                route.clone(),
                self.engine.clone(),
                &self.state_dir,
                &self.monolith_path,
            );
            interface.identity_resolver = self.identity_resolver.clone();
            let path = route.dbus_path.clone();

            debug!(plugin_id, path, "Registering authoritative D-Bus object");
            if let Err(e) = conn.object_server().at(path.as_str(), interface).await {
                debug!(plugin_id, path, error = %e, "Failed to register D-Bus object (likely already registered)");
            }
        }
        Ok(())
    }

    /// Reload routes and re-register objects.
    pub async fn reload(&self) -> anyhow::Result<()> {
        let count = self.reload_routes_only().await;
        info!(count, "SchemaRouter reloaded plugin routes");
        self.register_objects().await?;
        if let Some(engine) = &self.engine {
            engine.notify_catalog_changed();
        }
        Ok(())
    }

    /// Reload routes from the SHM catalog into the in-memory route table
    /// without touching the D-Bus object server. Returns the route count.
    ///
    /// This is a direct read of the live schema catalog — no polling, no watchers.
    async fn reload_routes_only(&self) -> usize {
        let routes = Self::load_routes_from(&self.monolith_path);
        let count = routes.len();
        let mut w = self.routes.write().await;
        *w = routes;
        count
    }

    /// Reload the SHM catalog and re-register D-Bus objects **iff** the
    /// producer's `catalog_hash` changed since the last observation.
    ///
    /// This is the no-polling change-detection hook: it is invoked on inbound
    /// connection arrival (before dispatching a method call), never on a timer.
    /// The current `catalog_hash` is read directly from `.manifest.json` and
    /// compared to the cached value. On a change, routes are reloaded from SHM,
    /// the cached hash is advanced, and changed objects are re-registered on the
    /// bus without restarting the bridge. Returns `true` when a reload occurred.
    ///
    /// The hash is read, never re-computed — the producer is the sole authority
    /// for `catalog_hash`.
    pub async fn refresh_if_changed(&self) -> anyhow::Result<bool> {
        let current = self.read_manifest_hash();
        let changed = {
            let cached = self.cached_hash.read().await;
            match (cached.as_deref(), current.as_deref()) {
                // No manifest present → no hash to compare against; do nothing.
                (_, None) => false,
                // First observed hash, or a differing hash → reload.
                (None, Some(_)) => true,
                (Some(cached_hash), Some(current_hash)) => cached_hash != current_hash,
            }
        };

        if !changed {
            return Ok(false);
        }

        let count = self.reload_routes_only().await;
        {
            let mut w = self.cached_hash.write().await;
            *w = current;
        }
        info!(
            count,
            "manifest catalog_hash changed on inbound — reloaded SHM, re-registering objects"
        );

        // Re-register the (possibly changed) object set on the bus. Skipped when
        // the D-Bus connection is not yet initialized (e.g. unit tests), so the
        // route reload and hash advance remain observable without a live bus.
        if self.dbus_connection.get().is_some() {
            self.register_objects().await?;
        }
        if let Some(engine) = &self.engine {
            engine.notify_catalog_changed();
        }
        Ok(true)
    }

    /// The currently cached `catalog_hash`, or `None` if no manifest has been
    /// observed.
    pub async fn cached_catalog_hash(&self) -> Option<String> {
        self.cached_hash.read().await.clone()
    }

    /// List all currently routable plugin IDs.
    pub async fn list_plugin_ids(&self) -> Vec<String> {
        let r = self.routes.read().await;
        r.keys().cloned().collect()
    }

    /// Get the route for a specific plugin.
    pub async fn get_route(&self, plugin_id: &str) -> Option<PluginRoute> {
        let r = self.routes.read().await;
        r.get(plugin_id).cloned()
    }

    /// Call a method on a plugin via its schema-derived D-Bus route.
    pub async fn call_method(
        &self,
        plugin_id: &str,
        method_name: &str,
        json_args: &str,
    ) -> Result<String, SchemaRouterError> {
        let route = self
            .get_route(plugin_id)
            .await
            .ok_or_else(|| SchemaRouterError::PluginNotFound(plugin_id.to_string()))?;

        if !route.methods.contains_key(method_name) {
            return Err(SchemaRouterError::MethodNotFound {
                plugin_id: plugin_id.to_string(),
                method: method_name.to_string(),
                available: route.methods.keys().cloned().collect(),
            });
        }

        let conn = self
            .dbus_connection
            .get()
            .ok_or(SchemaRouterError::DbusUnavailable)?;

        let proxy = Proxy::new(
            conn,
            route.dbus_destination.as_str(),
            route.dbus_path.as_str(),
            route.dbus_interface.as_str(),
        )
        .await
        .map_err(|e| SchemaRouterError::ProxyBuildFailed(e.to_string()))?;

        let result: String = proxy
            .call("Call", &(method_name.to_string(), json_args.to_string()))
            .await
            .map_err(|e| SchemaRouterError::DbusCallFailed {
                method: method_name.to_string(),
                error: e.to_string(),
            })?;

        Ok(result)
    }

    /// Get a property from a plugin via its schema-derived D-Bus route.
    pub async fn get_property(
        &self,
        plugin_id: &str,
        property_name: &str,
    ) -> Result<String, SchemaRouterError> {
        let route = self
            .get_route(plugin_id)
            .await
            .ok_or_else(|| SchemaRouterError::PluginNotFound(plugin_id.to_string()))?;

        if !route.properties.contains_key(property_name) {
            return Err(SchemaRouterError::PropertyNotFound {
                plugin_id: plugin_id.to_string(),
                property: property_name.to_string(),
                available: route.properties.keys().cloned().collect(),
            });
        }

        let conn = self
            .dbus_connection
            .get()
            .ok_or(SchemaRouterError::DbusUnavailable)?;

        let proxy = Proxy::new(
            conn,
            route.dbus_destination.as_str(),
            route.dbus_path.as_str(),
            route.dbus_interface.as_str(),
        )
        .await
        .map_err(|e| SchemaRouterError::ProxyBuildFailed(e.to_string()))?;

        proxy
            .call("GetProperty", &(property_name.to_string(),))
            .await
            .map_err(|e| SchemaRouterError::DbusCallFailed {
                method: format!("GetProperty({property_name})"),
                error: e.to_string(),
            })
    }

    /// Set a property on a plugin via its schema-derived D-Bus route.
    pub async fn set_property(
        &self,
        plugin_id: &str,
        property_name: &str,
        json_value: &str,
    ) -> Result<(), SchemaRouterError> {
        let route = self
            .get_route(plugin_id)
            .await
            .ok_or_else(|| SchemaRouterError::PluginNotFound(plugin_id.to_string()))?;

        if !route.properties.contains_key(property_name) {
            return Err(SchemaRouterError::PropertyNotFound {
                plugin_id: plugin_id.to_string(),
                property: property_name.to_string(),
                available: route.properties.keys().cloned().collect(),
            });
        }

        let conn = self
            .dbus_connection
            .get()
            .ok_or(SchemaRouterError::DbusUnavailable)?;

        let proxy = Proxy::new(
            conn,
            route.dbus_destination.as_str(),
            route.dbus_path.as_str(),
            route.dbus_interface.as_str(),
        )
        .await
        .map_err(|e| SchemaRouterError::ProxyBuildFailed(e.to_string()))?;

        proxy
            .call::<_, _, ()>(
                "SetProperty",
                &(property_name.to_string(), json_value.to_string()),
            )
            .await
            .map_err(|e| SchemaRouterError::DbusCallFailed {
                method: format!("SetProperty({property_name})"),
                error: e.to_string(),
            })?;

        Ok(())
    }

    // ── Private helpers ─────────────────────────────────────────────────────

    /// Reads the `catalog_hash` from this router's manifest path.
    fn read_manifest_hash(&self) -> Option<String> {
        Self::read_manifest_hash_at(&self.manifest_path)
    }

    /// Reads `{ "catalog_hash": "<hex>" }` from the manifest at `path`.
    ///
    /// Returns `None` if the file is absent, unparsable, or lacks a string
    /// `catalog_hash`. This is a direct synchronous SHM read — no polling.
    fn read_manifest_hash_at(path: &Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let value: JsonValue = serde_json::from_str(&text).ok()?;
        value
            .get("catalog_hash")
            .and_then(|h| h.as_str())
            .map(String::from)
    }

    /// Loads plugin routes from the combined monolith. If the monolith is
    /// unavailable or invalid, no routes are exposed.
    fn load_routes_from(catalog_path: &Path) -> HashMap<String, PluginRoute> {
        if catalog_path.is_dir() {
            Self::load_from_blob_catalog(catalog_path).unwrap_or_default()
        } else {
            Self::load_from_monolith(catalog_path).unwrap_or_default()
        }
    }

    /// Loads plugin routes from the sealed blob catalog. Each blob's
    /// SCHEMA_JSON section is the canonical full `PluginSchema`; a blob in
    /// the catalog IS the plugin. Corrupt blobs are skipped by the catalog
    /// loader (hash-verified) and therefore expose no route.
    fn load_from_blob_catalog(dir: &Path) -> Option<HashMap<String, PluginRoute>> {
        let catalog = op_blob::ActiveReflectionCatalog::open(dir).ok()?;
        let mut routes = HashMap::new();
        for blob in catalog.plugin_object_blobs() {
            let plugin_id = blob.manifest.plugin_id.clone();
            if op_plugins::default_registry::is_retired_plugin(&plugin_id) {
                continue;
            }
            let schema: JsonValue = match serde_json::from_str(&blob.schema_json) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let route = Self::build_route(&plugin_id, &schema);
            routes.insert(plugin_id, route);
        }
        Some(routes)
    }

    fn load_from_monolith(path: &Path) -> Option<HashMap<String, PluginRoute>> {
        let bytes = std::fs::read(path).ok()?;
        let root: JsonValue = serde_json::from_slice(&bytes).ok()?;
        let catalog = root.as_object()?;

        let mut routes = HashMap::new();
        for (plugin_id, schema_value) in catalog {
            let schema = match schema_value {
                JsonValue::Array(arr) => arr.first()?,
                JsonValue::Object(_) => schema_value,
                _ => continue,
            };
            let route = Self::build_route(plugin_id, schema);
            routes.insert(plugin_id.clone(), route);
        }
        Some(routes)
    }

    fn build_route(plugin_id: &str, schema: &JsonValue) -> PluginRoute {
        let dbus_path = canonical::plugin_path(plugin_id);
        let dbus_destination = BASE_SERVICE_NAME.to_string();
        let dbus_interface = canonical::plugin_interface(plugin_id);
        let methods = Self::extract_methods(schema);
        let signals = Self::extract_signals(schema);
        let properties = Self::extract_properties(schema);
        PluginRoute {
            plugin_id: plugin_id.to_string(),
            dbus_path,
            dbus_destination,
            dbus_interface,
            methods,
            signals,
            properties,
        }
    }

    fn extract_methods(schema: &JsonValue) -> HashMap<String, JsonValue> {
        let mut methods = HashMap::new();
        if let Some(method_map) = schema.get("methods").and_then(|m| m.as_object()) {
            for (name, def) in method_map {
                methods.insert(name.clone(), def.clone());
            }
        }
        methods
    }

    /// Extracts the `signals` array from a plugin schema. Each entry is a
    /// `SignalDecl` JSON object (`name`, `payload`, `subid`). Absent or
    /// non-array `signals` yields an empty list.
    fn extract_signals(schema: &JsonValue) -> Vec<JsonValue> {
        schema
            .get("signals")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default()
    }

    fn extract_properties(schema: &JsonValue) -> HashMap<String, JsonValue> {
        let mut properties = HashMap::new();
        if let Some(props) = schema
            .get("fields")
            .or_else(|| schema.get("properties"))
            .and_then(|p| p.as_object())
        {
            for (name, def) in props {
                properties.insert(name.clone(), def.clone());
            }
        }
        properties
    }
}

/// Default SHM state directory: `/dev/shm/opdbus/state`.
const DEFAULT_SHM_STATE_DIR: &str = "/dev/shm/opdbus/state";

/// A generic D-Bus interface backed by a plugin schema.
///
/// This struct is registered on the bus for every plugin, making the bridge
/// the authoritative owner of the D-Bus objects defined in the schema.
///
/// Present-state is read from MutationEngine's committed SHM transaction on
/// each property access. The sealed catalog is pinned before and after that
/// transaction so a stale in-memory route cannot expose removed plugin state.
pub struct SchemaBackedInterface {
    plugin_id: String,
    route: PluginRoute,
    /// Directory containing per-plugin present-state JSON files.
    shm_state_dir: PathBuf,
    /// Sealer-owned plugin catalog used only to prove current membership.
    blob_catalog_dir: PathBuf,
    /// The MutationEngine for real method dispatch (Requirement 5).
    /// `None` when no engine is wired (unit tests that only exercise
    /// method-existence / property-read paths).
    engine: Option<Arc<MutationEngine>>,
    /// Exact D-Bus sender binding. `None` keeps production calls fail-closed.
    identity_resolver: Option<Arc<DbusIdentityResolver>>,
}

impl SchemaBackedInterface {
    /// Creates a new `SchemaBackedInterface` using the default SHM state
    /// directory (`/dev/shm/opdbus/state`) and no engine.
    ///
    /// Prefer [`with_engine`](Self::with_engine) in production so the
    /// interface can dispatch method calls through `MutationEngine`.
    pub fn new(plugin_id: String, route: PluginRoute) -> Self {
        Self::with_shm_state_dir(plugin_id, route, DEFAULT_SHM_STATE_DIR)
    }

    /// Creates a new `SchemaBackedInterface` wired to a MutationEngine for
    /// real method dispatch.
    ///
    /// The engine is used by [`call`](Self::call) to dispatch method
    /// invocations through `MutationEngine::dispatch_method_call`
    /// (Requirement 5 / VAL-DISPATCH-001).
    pub fn with_engine(
        plugin_id: String,
        route: PluginRoute,
        engine: Option<Arc<MutationEngine>>,
    ) -> Self {
        let mut iface = Self::with_shm_state_dir(plugin_id, route, DEFAULT_SHM_STATE_DIR);
        iface.engine = engine;
        iface
    }

    fn with_engine_and_storage(
        plugin_id: String,
        route: PluginRoute,
        engine: Option<Arc<MutationEngine>>,
        shm_state_dir: impl AsRef<Path>,
        blob_catalog_dir: impl AsRef<Path>,
    ) -> Self {
        let mut iface = Self::with_storage_paths(plugin_id, route, shm_state_dir, blob_catalog_dir);
        iface.engine = engine;
        iface
    }

    /// Creates a new `SchemaBackedInterface` with a custom SHM state directory.
    ///
    /// In production, use [`new`](Self::new) which targets `/dev/shm/opdbus/state`.
    /// Tests should supply a temporary directory.
    pub fn with_shm_state_dir(
        plugin_id: String,
        route: PluginRoute,
        shm_state_dir: impl AsRef<Path>,
    ) -> Self {
        Self::with_storage_paths(
            plugin_id,
            route,
            shm_state_dir,
            crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        )
    }

    /// Creates an interface with isolated committed-state and sealed-catalog
    /// roots. Production registration supplies the router's configured roots;
    /// tests use this constructor to avoid consulting live host state.
    pub fn with_storage_paths(
        plugin_id: String,
        route: PluginRoute,
        shm_state_dir: impl AsRef<Path>,
        blob_catalog_dir: impl AsRef<Path>,
    ) -> Self {
        Self {
            plugin_id,
            route,
            shm_state_dir: shm_state_dir.as_ref().to_path_buf(),
            blob_catalog_dir: blob_catalog_dir.as_ref().to_path_buf(),
            engine: None,
            identity_resolver: None,
        }
    }

    /// Reads one complete committed projection through the exact sealed
    /// schema's public-state policy. A missing projection is projected from an
    /// empty object; an absent policy returns `None`. Catalog, policy,
    /// transaction and JSON errors remain errors.
    pub async fn read_present_state(&self) -> anyhow::Result<Option<JsonValue>> {
        self.read_present_state_for(None).await
    }

    async fn read_present_state_for(
        &self,
        property: Option<String>,
    ) -> anyhow::Result<Option<JsonValue>> {
        let plugin_id = self.plugin_id.clone();
        let state_dir = self.shm_state_dir.clone();
        let catalog_dir = self.blob_catalog_dir.clone();
        tokio::task::spawn_blocking(move || {
            let before = crate::sealed_schema_reader::manifest_plugins(&catalog_dir)?;
            anyhow::ensure!(
                before.contains_key(&plugin_id),
                "plugin '{plugin_id}' is not a current sealed catalog member"
            );
            // Validate the member's manifest-pinned blob as well as its name.
            let schema = crate::sealed_schema_reader::read_manifest_pinned_plugin_schema(
                &catalog_dir,
                &plugin_id,
            )?;
            anyhow::ensure!(
                crate::sealed_schema_reader::manifest_plugins(&catalog_dir)? == before,
                "sealed catalog changed before state read"
            );
            if let Some(property) = property.as_deref()
                && !schema.public_state_exposes_top_level(property)?
            {
                return Ok(None);
            }
            let state_root = state_dir
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("projection path is not valid UTF-8"))?;
            let snapshot = op_core::projection_shm::read_projection_snapshot_in(
                state_root,
                std::slice::from_ref(&plugin_id),
            )?;
            anyhow::ensure!(
                crate::sealed_schema_reader::manifest_plugins(&catalog_dir)? == before,
                "sealed catalog changed during state read"
            );
            let mut bytes = snapshot
                .states
                .get(&plugin_id)
                .cloned()
                .unwrap_or_else(|| b"{}".to_vec());
            let private = simd_json::to_owned_value(&mut bytes)?;
            let Some(projected) = schema.project_public_state(&private)? else {
                return Ok(None);
            };
            let public = crate::mutation_engine::public_plugin_value(&plugin_id, &projected);
            let state = serde_json::to_value(public)?;
            anyhow::ensure!(
                state.is_object(),
                "committed plugin state is not a JSON object"
            );
            Ok(Some(state))
        })
        .await
        .map_err(|error| anyhow::anyhow!("committed state reader task failed: {error}"))?
    }

    /// The canonical D-Bus object path this interface is registered at
    /// (`/org/opdbus/v1/plugins/<plugin_id>`).
    pub fn dbus_path(&self) -> &str {
        &self.route.dbus_path
    }

    /// The canonical D-Bus interface name for this plugin
    /// (`org.opdbus.v1.PluginV1`).
    pub fn dbus_interface(&self) -> &str {
        &self.route.dbus_interface
    }

    /// Names of every method declared in the plugin's capability surface.
    ///
    /// Every declared `MethodDecl` is exposed as a callable D-Bus method via
    /// [`call`](Self::call).
    pub fn declared_methods(&self) -> Vec<String> {
        self.route.methods.keys().cloned().collect()
    }

    /// Returns `true` if `method` is declared in the plugin's capability
    /// surface (and is therefore callable).
    pub fn exposes_method(&self, method: &str) -> bool {
        self.route.methods.contains_key(method)
    }

    /// Names of every signal declared in the plugin's capability surface.
    ///
    /// Every declared `SignalDecl` is exposed as an emittable D-Bus signal via
    /// [`emit_signal`](Self::emit_signal).
    pub fn declared_signals(&self) -> Vec<String> {
        self.route
            .signals
            .iter()
            .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(String::from))
            .collect()
    }

    /// Returns `true` if `signal` is declared in the plugin's capability
    /// surface (and is therefore emittable).
    pub fn exposes_signal(&self, signal: &str) -> bool {
        self.route
            .signals
            .iter()
            .any(|s| s.get("name").and_then(|n| n.as_str()) == Some(signal))
    }

    /// Emits a declared D-Bus signal for this plugin on the canonical object
    /// path and interface, carrying `payload_json` as the signal body.
    ///
    /// The signal name is validated against the plugin's capability surface;
    /// emitting a signal that is not declared is rejected. This is the runtime
    /// counterpart to [`declared_signals`](Self::declared_signals): every
    /// `SignalDecl` in the schema is emittable, and nothing else is.
    pub async fn emit_signal(
        &self,
        conn: &Connection,
        signal: &str,
        payload_json: &str,
    ) -> zbus::fdo::Result<()> {
        if !self.exposes_signal(signal) {
            return Err(zbus::fdo::Error::UnknownMethod(format!(
                "signal '{}' is not declared for plugin '{}'",
                signal, self.plugin_id
            )));
        }
        conn.emit_signal(
            Option::<&str>::None,
            self.route.dbus_path.as_str(),
            self.route.dbus_interface.as_str(),
            signal,
            &(payload_json.to_string(),),
        )
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Validates that json_args conforms to the JSON Schema in MethodDecl.args.
    ///
    /// Returns Ok(()) if validation passes, Err(String) with details if it fails.
    /// This validation happens before dispatch, so invalid args cause no mutation
    /// (Requirement 6.3 / VAL-VALIDATE-003).
    fn validate_json_args(&self, method_decl: &JsonValue, json_args: &str) -> Result<(), String> {
        let args_schema = method_decl
            .get("args")
            .ok_or_else(|| "missing args schema".to_string())?;

        // Parse the args schema as a validator
        let validator =
            Validator::new(args_schema).map_err(|e| format!("invalid schema: {}", e))?;

        // Parse the args JSON
        let args_value: JsonValue =
            serde_json::from_str(json_args).map_err(|e| format!("invalid JSON: {}", e))?;

        // Validate against the schema (jsonschema uses draft-07 by default)
        if let Err(e) = validator.validate(&args_value) {
            return Err(format!("validation error: {}", e));
        }

        Ok(())
    }

    async fn dispatch_call_for_identity(
        &self,
        method: String,
        json_args: String,
        identity: Option<&GhostbridgeIdentity>,
    ) -> zbus::fdo::Result<String> {
        let method_decl = match self.route.methods.get(&method) {
            Some(decl) => decl,
            None => return Err(zbus::fdo::Error::UnknownMethod(method)),
        };

        info!(plugin_id = %self.plugin_id, method, "D-Bus method call received");

        if let Err(error) = self.validate_json_args(method_decl, &json_args) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "arguments do not conform to schema for method '{}': {}",
                method, error
            )));
        }

        let identity = identity.ok_or_else(|| {
            zbus::fdo::Error::AccessDenied(
                "PluginV1.Call has no authoritative sender-to-principal identity binding"
                    .to_string(),
            )
        })?;
        if identity.principal_id.is_empty() || identity.session_id.is_empty() {
            return Err(zbus::fdo::Error::AccessDenied(
                "PluginV1.Call requires a non-empty authenticated identity".to_string(),
            ));
        }

        let required_cap = method_decl
            .get("required_capability")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|capability| !capability.is_empty());
        if let Some(required_cap) = required_cap
            && !load_capability_grants(&identity.principal_id).contains(required_cap)
        {
            return Err(zbus::fdo::Error::AccessDenied(format!(
                "capability '{}' is not granted to the authenticated caller of method '{}'",
                required_cap, method
            )));
        }

        let engine = self.engine.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed(
                "MutationEngine not wired to this interface — dispatch unavailable".to_string(),
            )
        })?;
        let result = engine
            .dispatch_method_call_with_identity(
                &self.plugin_id,
                &method,
                &json_args,
                required_cap,
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
            )
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;

        serde_json::to_string(&result).map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

/// Test-only access to the zbus `call` dispatcher: the interface method is
/// private to the macro-generated impl, and bridge-internal tests outside
/// this module drive the real D-Bus-surface validation pipeline (method
/// lookup → arg validation → capability gate → dispatch) through this door.
#[cfg(test)]
impl SchemaBackedInterface {
    pub(crate) const TEST_CALLER_PRINCIPAL: &'static str = "schema-router-test-principal";

    pub(crate) async fn call_in_test(
        &self,
        method: String,
        json_args: String,
    ) -> zbus::fdo::Result<String> {
        self.dispatch_call_for_identity(
            method,
            json_args,
            Some(&GhostbridgeIdentity {
                principal_id: Self::TEST_CALLER_PRINCIPAL.to_string(),
                session_id: "schema-router-test-session".to_string(),
                session_genesis: "schema-router-test-genesis".to_string(),
            }),
        )
        .await
    }
}

#[zbus::interface(name = "org.opdbus.v1.PluginV1")]
impl SchemaBackedInterface {
    /// Generic method call dispatcher.
    ///
    /// All schema-defined methods are funneled through this call, allowing
    /// dynamic dispatch without compile-time traits for every plugin.
    ///
    /// Dispatch pipeline (Requirement 7):
    ///   1. Method lookup → reject if not declared (UnknownMethod)
    ///   2. Arg validation → reject if invalid (InvalidArgs)
    ///   3. Capability check → reject if required_capability not granted (AccessDenied)
    ///   4. MutationEngine.mutate → record in event chain (Requirement 5)
    async fn call(
        &self,
        method: String,
        json_args: String,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let resolver = self.identity_resolver.as_ref().ok_or_else(|| {
            zbus::fdo::Error::AccessDenied("D-Bus identity resolver is not available".to_string())
        })?;
        let identity = resolver
            .resolve(&header)
            .await
            .map_err(|error| zbus::fdo::Error::AccessDenied(error.to_string()))?;
        self.dispatch_call_for_identity(method, json_args, Some(&identity))
            .await
    }

    /// Get a property value from SHM present-state.
    ///
    /// Reads the current committed projection on each call. If the sealed
    /// plugin has no committed projection, the requested property is `null`.
    async fn get_property(&self, name: String) -> zbus::fdo::Result<String> {
        let state = self
            .read_present_state_for(Some(name.clone()))
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?
            .ok_or_else(|| {
                zbus::fdo::Error::AccessDenied(format!(
                    "plugin '{}' does not expose public state",
                    self.plugin_id
                ))
            })?;
        let val = state.get(&name).unwrap_or(&JsonValue::Null);
        serde_json::to_string(val).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Get all present-state properties as a JSON object.
    ///
    /// Reads the current committed projection on each call. If the sealed
    /// plugin has no committed projection, returns `{}`.
    async fn get_all_properties(&self) -> zbus::fdo::Result<String> {
        let state = self
            .read_present_state()
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?
            .ok_or_else(|| {
                zbus::fdo::Error::AccessDenied(format!(
                    "plugin '{}' does not expose public state",
                    self.plugin_id
                ))
            })?;
        serde_json::to_string(&state).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Set a property value.
    ///
    /// Present-state is managed by the mutation engine via SHM.
    /// The bridge is a reader, not a writer, of present-state. This method
    /// returns an error indicating that property writes must go through the
    /// producer's mutation pipeline.
    async fn set_property(&self, _name: String, _json_value: String) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::Failed(
            "Present-state is managed via SHM; \
             use the mutation pipeline to change properties"
                .to_string(),
        ))
    }

    /// Push signal emitted after every `write_projection()` in the mutation engine.
    ///
    /// `data_json` is a JSON object identifying what changed:
    /// `{"plugin": "<name>", "key": "<key>"}` (single) or
    /// `{"plugin": "<name>", "keys": ["k1","k2"]}` (batch).
    ///
    /// Subscribers can act on the signal payload alone without an additional
    /// query — this is the no-polling guarantee (REQ-1.5, REQ-7).
    #[zbus(signal)]
    pub async fn updated(
        signal_emitter: &zbus::object_server::SignalEmitter<'_>,
        data_json: &str,
    ) -> zbus::Result<()>;
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaRouterError {
    #[error("Plugin not found: {0}")]
    PluginNotFound(String),
    #[error("Method {method} not found for plugin {plugin_id}. Available: {available:?}")]
    MethodNotFound {
        plugin_id: String,
        method: String,
        available: Vec<String>,
    },
    #[error("Property {property} not found for plugin {plugin_id}. Available: {available:?}")]
    PropertyNotFound {
        plugin_id: String,
        property: String,
        available: Vec<String>,
    },
    #[error("D-Bus unavailable")]
    DbusUnavailable,
    #[error("D-Bus proxy build failed: {0}")]
    ProxyBuildFailed(String),
    #[error("D-Bus call failed: {method}: {error}")]
    DbusCallFailed { method: String, error: String },
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
}

impl From<SchemaRouterError> for tonic::Status {
    fn from(err: SchemaRouterError) -> Self {
        match err {
            SchemaRouterError::PluginNotFound(id) => {
                tonic::Status::not_found(format!("Plugin not found: {}", id))
            }
            SchemaRouterError::MethodNotFound {
                plugin_id,
                method,
                available,
            } => tonic::Status::not_found(format!(
                "Method {} not found for plugin {}. Available: {:?}",
                method, plugin_id, available
            )),
            SchemaRouterError::PropertyNotFound {
                plugin_id,
                property,
                available,
            } => tonic::Status::not_found(format!(
                "Property {} not found for plugin {}. Available: {:?}",
                property, plugin_id, available
            )),
            SchemaRouterError::DbusUnavailable => {
                tonic::Status::unavailable("D-Bus connection unavailable")
            }
            SchemaRouterError::ProxyBuildFailed(msg) => {
                tonic::Status::internal(format!("D-Bus proxy build failed: {}", msg))
            }
            SchemaRouterError::DbusCallFailed { method, error } => {
                tonic::Status::internal(format!("D-Bus call failed: {}: {}", method, error))
            }
            SchemaRouterError::SerializationFailed(msg) => {
                tonic::Status::internal(format!("Serialization failed: {}", msg))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)] // test_registry_guard holds Mutex across await to serialize tests
mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::test_registry_guard;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    /// Builds a test `PluginRoute` with a single method and two properties.
    fn test_route() -> PluginRoute {
        let mut methods = HashMap::new();
        methods.insert(
            "GetStatus".to_string(),
            json!({ "type": "object", "properties": {} }),
        );
        let mut properties = HashMap::new();
        properties.insert("status".to_string(), json!({ "type": "string" }));
        properties.insert("uptime".to_string(), json!({ "type": "integer" }));
        let signals = vec![json!({
            "name": "StatusChanged",
            "payload": { "type": "object" },
            "subid": "evt.service.test.status-changed@v1"
        })];
        PluginRoute {
            plugin_id: "test_plugin".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/test_plugin".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods,
            signals,
            properties,
        }
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    struct StateFixture {
        state: tempfile::TempDir,
        catalog: tempfile::TempDir,
    }

    impl StateFixture {
        fn sealed(plugin_id: &str) -> Self {
            let schema = if plugin_id == "identity_sled" {
                let mut identity = HashMap::new();
                identity.insert(
                    "principal_id".to_string(),
                    op_state_store::FieldSchema {
                        field_type: op_state_store::FieldType::String,
                        required: false,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: Vec::new(),
                        read_only: false,
                        read_only_when: None,
                    },
                );
                identity.insert(
                    "sealed_id".to_string(),
                    op_state_store::FieldSchema {
                        field_type: op_state_store::FieldType::String,
                        required: false,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: Vec::new(),
                        read_only: false,
                        read_only_when: None,
                    },
                );
                op_state_store::PluginSchema::builder(plugin_id)
                    .object_field("identity", identity, false, "public identity")
                    .public_state_path("/identity")
                    .build()
            } else {
                op_state_store::PluginSchema::builder(plugin_id)
                    .string_field("status", false, "status")
                    .integer_field("uptime", false, "uptime")
                    .string_field("extra_field", false, "private fixture field")
                    .public_state_paths(&["/status", "/uptime"])
                    .build()
            };
            Self::sealed_schema(schema)
        }

        fn sealed_schema(schema: op_state_store::PluginSchema) -> Self {
            let state = tempfile::tempdir().unwrap();
            let catalog = tempfile::tempdir().unwrap();
            let plugin_id = schema.name.clone();
            op_blob::BlobStore::open(catalog.path())
                .unwrap()
                .write(&op_blob::blobify_plugin_schema(&plugin_id, schema))
                .unwrap();
            Self { state, catalog }
        }

        fn interface(&self, plugin_id: &str) -> SchemaBackedInterface {
            SchemaBackedInterface::with_storage_paths(
                plugin_id.to_string(),
                test_route(),
                self.state.path(),
                self.catalog.path(),
            )
        }

        fn commit(&self, plugin_id: &str, value: JsonValue) {
            let bytes = serde_json::to_vec(&value).unwrap();
            op_core::projection_shm::update_projection_in(
                self.state.path().to_str().unwrap(),
                plugin_id,
                |_| Ok(Some(bytes)),
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn missing_committed_state_is_an_explicit_empty_object() {
        let fixture = StateFixture::sealed("test_plugin");
        let state = fixture
            .interface("test_plugin")
            .read_present_state()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state, json!({}));
    }

    #[tokio::test]
    async fn reads_only_committed_projection_updates() {
        let fixture = StateFixture::sealed("test_plugin");
        let iface = fixture.interface("test_plugin");
        fixture.commit(
            "test_plugin",
            json!({"status":"active","uptime":42,"extra_field":"preserved"}),
        );
        assert_eq!(
            iface.read_present_state().await.unwrap().unwrap(),
            json!({"status":"active","uptime":42})
        );
        fixture.commit("test_plugin", json!({"status":"down","uptime":43}));
        assert_eq!(
            iface.read_present_state().await.unwrap().unwrap()["status"],
            "down"
        );
    }

    #[tokio::test]
    async fn dbus_property_reads_use_the_committed_snapshot() {
        let fixture = StateFixture::sealed("test_plugin");
        let iface = fixture.interface("test_plugin");
        fixture.commit(
            "test_plugin",
            json!({"status":"running","uptime":100,"extra_field":"private"}),
        );

        let property: JsonValue =
            serde_json::from_str(&iface.get_property("status".into()).await.unwrap()).unwrap();
        assert_eq!(property, json!("running"));
        let all: JsonValue =
            serde_json::from_str(&iface.get_all_properties().await.unwrap()).unwrap();
        assert_eq!(all, json!({"status":"running","uptime":100}));
    }

    #[tokio::test]
    async fn missing_committed_property_is_null() {
        let fixture = StateFixture::sealed("test_plugin");
        let value: JsonValue = serde_json::from_str(
            &fixture
                .interface("test_plugin")
                .get_property("status".into())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value, JsonValue::Null);
    }

    #[tokio::test]
    async fn declared_but_unexposed_property_is_access_denied() {
        let fixture = StateFixture::sealed_schema(
            op_state_store::PluginSchema::builder("partial_policy")
                .string_field("status", false, "public status")
                .integer_field("uptime", false, "private uptime")
                .public_state_path("/status")
                .build(),
        );
        fixture.commit("partial_policy", json!({"status":"running","uptime":100}));
        let iface = fixture.interface("partial_policy");
        assert!(matches!(
            iface.get_property("uptime".into()).await,
            Err(zbus::fdo::Error::AccessDenied(_))
        ));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&iface.get_all_properties().await.unwrap()).unwrap(),
            json!({"status":"running"})
        );
    }

    #[tokio::test]
    async fn malformed_or_non_object_committed_state_is_a_dbus_failure() {
        let fixture = StateFixture::sealed("test_plugin");
        let root = fixture.state.path().to_str().unwrap();
        op_core::projection_shm::update_projection_in(root, "test_plugin", |_| {
            Ok(Some(b"{bad-json".to_vec()))
        })
        .unwrap();
        assert!(fixture
            .interface("test_plugin")
            .get_all_properties()
            .await
            .is_err());

        fixture.commit("test_plugin", json!(["not", "an", "object"]));
        assert!(fixture
            .interface("test_plugin")
            .get_property("status".into())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn public_policy_type_drift_is_a_dbus_failure() {
        let fixture = StateFixture::sealed("test_plugin");
        fixture.commit("test_plugin", json!({"status": 7}));
        assert!(matches!(
            fixture.interface("test_plugin").get_all_properties().await,
            Err(zbus::fdo::Error::Failed(_))
        ));
    }

    #[tokio::test]
    async fn invalid_public_policy_is_a_dbus_failure() {
        let mut schema = op_state_store::PluginSchema::builder("invalid_policy")
            .string_field("status", false, "status")
            .build();
        schema.public_state = op_state_store::PublicStatePolicy {
            paths: vec!["/undeclared".into()],
        };
        let fixture = StateFixture::sealed_schema(schema);
        fixture.commit("invalid_policy", json!({"status":"private"}));
        assert!(matches!(
            fixture
                .interface("invalid_policy")
                .get_all_properties()
                .await,
            Err(zbus::fdo::Error::Failed(_))
        ));
    }

    #[tokio::test]
    async fn incomplete_projection_transaction_is_a_dbus_failure() {
        let fixture = StateFixture::sealed("test_plugin");
        fixture.commit("test_plugin", json!({"status":"committed"}));
        // Fault injection models a writer that crashed after publishing its
        // transaction marker. Production writes still use the core API.
        fs::write(
            fixture.state.path().join(".manifest.json"),
            br#"{"generation":2,"publishing":true}"#,
        )
        .unwrap();
        assert!(fixture
            .interface("test_plugin")
            .get_all_properties()
            .await
            .is_err());
    }

    #[tokio::test]
    async fn stale_route_cannot_read_state_after_sealed_membership_removal() {
        let fixture = StateFixture::sealed("another_plugin");
        fixture.commit("test_plugin", json!({"status":"must-not-leak"}));
        let error = fixture
            .interface("test_plugin")
            .read_present_state()
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("not a current sealed catalog member"));
    }

    #[tokio::test]
    async fn identity_state_is_redacted_at_the_public_read_boundary() {
        let fixture = StateFixture::sealed("identity_sled");
        fixture.commit(
            "identity_sled",
            json!({
                "identity": {"principal_id":"alice", "sealed_id":"SID1-secret"},
                "sealed_id":"SID1-root-secret"
            }),
        );
        let state = fixture
            .interface("identity_sled")
            .read_present_state()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state, json!({"identity":{"principal_id":"alice"}}));
    }

    #[tokio::test]
    async fn absent_public_policy_withholds_state_and_dbus_denies_access() {
        let fixture = StateFixture::sealed_schema(
            op_state_store::PluginSchema::builder("closed_plugin")
                .string_field("status", false, "private status")
                .build(),
        );
        fixture.commit("closed_plugin", json!({"status":"private"}));
        let iface = fixture.interface("closed_plugin");
        assert_eq!(iface.read_present_state().await.unwrap(), None);
        assert!(matches!(
            iface.get_all_properties().await,
            Err(zbus::fdo::Error::AccessDenied(_))
        ));
    }

    #[tokio::test]
    async fn set_property_remains_rejected() {
        let fixture = StateFixture::sealed("test_plugin");
        assert!(fixture
            .interface("test_plugin")
            .set_property("status".into(), "\"x\"".into())
            .await
            .is_err());
    }

    // ── R4.1-R4.4 / VAL-BRIDGE-003/004/005 — Bridge registration from SHM ──

    /// Creates a unique temp base directory under `/dev/shm` for SHM files.
    fn test_base_dir() -> PathBuf {
        let id = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = PathBuf::from(format!("/dev/shm/opdbus-test-bridge-{}-{}", id, nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test base dir");
        dir
    }

    /// Writes a combined monolith `live-schema.json` under `base` with two
    /// plugins ("alpha", "beta") each carrying methods and signals in the exact
    /// SHM format the producer writes (a map of `plugin_id → PluginSchema`).
    fn write_test_monolith(base: &Path) -> PathBuf {
        let monolith = json!({
            "alpha": {
                "name": "alpha",
                "methods": {
                    "GetStatus": {
                        "name": "GetStatus",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "object" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.alpha.get-status@v1"
                    }
                },
                "signals": [
                    {
                        "name": "StatusChanged",
                        "payload": { "type": "object" },
                        "subid": "evt.service.alpha.status-changed@v1"
                    }
                ],
                "properties": { "status": { "type": "string" } }
            },
            "beta": {
                "name": "beta",
                "methods": {
                    "SetKey": {
                        "name": "SetKey",
                        "args": {
                            "type": "object",
                            "properties": { "key": { "type": "string" } },
                            "required": ["key"]
                        },
                        "returns": null,
                        "side_effect": "mutation",
                        "idempotent": false,
                        "required_capability": "beta.set_key",
                        "subid": "mut.service.beta.set-key@v1"
                    },
                    "GetKey": {
                        "name": "GetKey",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "string" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.beta.get-key@v1"
                    }
                },
                "signals": [
                    { "name": "KeyRotated", "payload": null, "subid": "evt.service.beta.key-rotated@v1" },
                    { "name": "KeyExpired", "payload": null, "subid": "evt.service.beta.key-expired@v1" }
                ],
                "properties": {}
            }
        });
        let path = base.join("live-schema.json");
        fs::write(&path, serde_json::to_vec_pretty(&monolith).unwrap()).unwrap();
        path
    }

    // ── VAL-BRIDGE-003: one SchemaBackedInterface per plugin from SHM ──

    #[test]
    fn bridge_registers_object_per_plugin_from_shm() {
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);
        let routes = SchemaRouter::load_routes_from(&monolith_path);
        assert_eq!(
            routes.len(),
            2,
            "one route per plugin declared in the monolith"
        );
        assert!(routes.contains_key("alpha"));
        assert!(routes.contains_key("beta"));

        // Each plugin route mounts at the canonical per-plugin object path.
        let alpha = &routes["alpha"];
        assert_eq!(alpha.dbus_path, canonical::plugin_path("alpha"));
        assert_eq!(alpha.dbus_path, "/org/opdbus/v1/plugins/alpha");
        assert_eq!(alpha.dbus_destination, "org.opdbus.v1.plugins");
        assert_eq!(alpha.dbus_interface, canonical::plugin_interface("alpha"));

        // One SchemaBackedInterface is constructed per plugin route, mounted at
        // the canonical path the bridge registers it on.
        for (id, route) in routes.iter() {
            let iface = SchemaBackedInterface::new(id.clone(), route.clone());
            assert_eq!(iface.dbus_path(), canonical::plugin_path(id));
        }

        cleanup(&base);
    }

    // ── Fail closed: no per-plugin schema fallback ──

    #[test]
    fn bridge_does_not_load_per_plugin_files_when_monolith_missing() {
        let base = test_base_dir();
        let schemas_dir = base.join("opdbus/schemas");
        fs::create_dir_all(&schemas_dir).unwrap();

        let alpha_schema = json!({
            "name": "alpha",
            "methods": {
                "Ping": {
                    "name": "Ping",
                    "args": { "type": "object" },
                    "returns": null,
                    "side_effect": "read",
                    "idempotent": true,
                    "required_capability": null,
                    "subid": "obs.service.alpha.ping@v1"
                }
            },
            "signals": [],
            "properties": {}
        });
        fs::write(
            schemas_dir.join("alpha.json"),
            serde_json::to_vec(&alpha_schema).unwrap(),
        )
        .unwrap();

        // Point at a monolith path that does not exist.
        let missing_monolith = base.join("nonexistent-live-schema.json");
        let routes = SchemaRouter::load_routes_from(&missing_monolith);

        assert_eq!(
            routes.len(),
            0,
            "missing live-schema catalog must expose no routes"
        );

        cleanup(&base);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    // ── VAL-BRIDGE-004: every MethodDecl exposed as a callable method ──

    #[tokio::test]
    async fn bridge_exposes_all_declared_methods() {
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);
        let routes = SchemaRouter::load_routes_from(&monolith_path);

        // beta declares SetKey and GetKey — both must be exposed.
        let beta = routes["beta"].clone();
        let iface = SchemaBackedInterface::new("beta".to_string(), beta);

        let declared = iface.declared_methods();
        assert!(declared.contains(&"SetKey".to_string()));
        assert!(declared.contains(&"GetKey".to_string()));
        assert_eq!(declared.len(), 2, "every declared MethodDecl is exposed");

        assert!(iface.exposes_method("SetKey"));
        assert!(iface.exposes_method("GetKey"));
        assert!(!iface.exposes_method("Nonexistent"));

        // A declared method is callable (passes the method-existence gate);
        // an undeclared method is rejected as UnknownMethod.
        let unknown = iface
            .call_in_test("Nonexistent".to_string(), "{}".to_string())
            .await;
        assert!(
            matches!(unknown, Err(zbus::fdo::Error::UnknownMethod(_))),
            "undeclared method must be rejected as UnknownMethod"
        );
        let known = iface
            .call_in_test("GetKey".to_string(), "{}".to_string())
            .await;
        assert!(
            !matches!(known, Err(zbus::fdo::Error::UnknownMethod(_))),
            "a declared method must be exposed (not rejected as UnknownMethod)"
        );

        cleanup(&base);
    }

    // ── VAL-BRIDGE-005: every SignalDecl exposed as an emittable signal ──

    #[test]
    fn bridge_exposes_all_declared_signals() {
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);
        let routes = SchemaRouter::load_routes_from(&monolith_path);

        // beta declares KeyRotated and KeyExpired signals.
        let beta = routes["beta"].clone();
        let iface = SchemaBackedInterface::new("beta".to_string(), beta);

        let signals = iface.declared_signals();
        assert!(signals.contains(&"KeyRotated".to_string()));
        assert!(signals.contains(&"KeyExpired".to_string()));
        assert_eq!(signals.len(), 2, "every declared SignalDecl is exposed");

        assert!(iface.exposes_signal("KeyRotated"));
        assert!(iface.exposes_signal("KeyExpired"));
        assert!(!iface.exposes_signal("Nonexistent"));

        // alpha declares exactly one signal: StatusChanged.
        let alpha = routes["alpha"].clone();
        let alpha_iface = SchemaBackedInterface::new("alpha".to_string(), alpha);
        assert_eq!(
            alpha_iface.declared_signals(),
            vec!["StatusChanged".to_string()]
        );

        cleanup(&base);
    }

    // ── Route extraction preserves the full method + signal surface ──

    #[test]
    fn route_extracts_methods_and_signals_from_schema() {
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);
        let routes = SchemaRouter::load_routes_from(&monolith_path);

        let beta = &routes["beta"];
        // Methods preserved with full MethodDecl bodies.
        assert_eq!(beta.methods.len(), 2);
        assert_eq!(beta.methods["SetKey"]["side_effect"], json!("mutation"));
        assert_eq!(
            beta.methods["SetKey"]["required_capability"],
            json!("beta.set_key")
        );
        // Signals preserved with full SignalDecl bodies.
        assert_eq!(beta.signals.len(), 2);
        let names: Vec<&str> = beta
            .signals
            .iter()
            .filter_map(|s| s["name"].as_str())
            .collect();
        assert!(names.contains(&"KeyRotated"));
        assert!(names.contains(&"KeyExpired"));

        cleanup(&base);
    }

    // ── R4.5 / NFR1.2 — Manifest hash change detection (no polling) ──────

    /// Writes a combined monolith at `path` containing one plugin per id in
    /// `plugin_ids`, each declaring a single read method. Mirrors the SHM
    /// format the producer emits (`plugin_id → PluginSchema`).
    fn write_monolith_plugins(path: &Path, plugin_ids: &[&str]) {
        let mut catalog = serde_json::Map::new();
        for id in plugin_ids {
            catalog.insert(
                (*id).to_string(),
                json!({
                    "name": id,
                    "methods": {
                        "Ping": {
                            "name": "Ping",
                            "args": { "type": "object" },
                            "returns": null,
                            "side_effect": "read",
                            "idempotent": true,
                            "required_capability": null,
                            "subid": format!("obs.service.{}.ping@v1", id)
                        }
                    },
                    "signals": [],
                    "properties": {}
                }),
            );
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(
            path,
            serde_json::to_vec_pretty(&JsonValue::Object(catalog)).unwrap(),
        )
        .unwrap();
    }

    /// Writes `{ "catalog_hash": "<hash>" }` to the manifest at `path`.
    fn write_manifest(path: &Path, hash: &str) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(
            path,
            serde_json::to_vec_pretty(&json!({ "catalog_hash": hash })).unwrap(),
        )
        .unwrap();
    }

    // ── VAL-BRIDGE-006: reload on manifest hash change, no restart ──

    #[tokio::test]
    async fn bridge_reloads_on_manifest_hash_change() {
        let base = test_base_dir();
        let monolith_path = base.join("live-schema.json");
        let manifest_path = base.join(".manifest.json");

        // Producer writes initial catalog (alpha only) + manifest hash-1.
        write_monolith_plugins(&monolith_path, &["alpha"]);
        write_manifest(&manifest_path, "hash-1");

        // Bridge caches the last-seen catalog_hash from .manifest.json at startup.
        let conn = Arc::new(tokio::sync::OnceCell::new());
        let router = SchemaRouter::with_paths(
            conn,
            monolith_path.clone(),
            base.join("state"),
            manifest_path.clone(),
        );
        assert_eq!(
            router.cached_catalog_hash().await.as_deref(),
            Some("hash-1"),
            "bridge caches the catalog_hash from .manifest.json at startup"
        );
        assert_eq!(router.list_plugin_ids().await.len(), 1);

        // First inbound connection — cached hash equals current → no reload.
        let reloaded = router.refresh_if_changed().await.unwrap();
        assert!(!reloaded, "unchanged hash must not trigger a reload");
        assert_eq!(router.list_plugin_ids().await.len(), 1);

        // Producer changes the catalog: adds beta and writes a new hash.
        write_monolith_plugins(&monolith_path, &["alpha", "beta"]);
        write_manifest(&manifest_path, "hash-2");

        // Second inbound connection — hash changed → reload SHM + re-register.
        let reloaded = router.refresh_if_changed().await.unwrap();
        assert!(
            reloaded,
            "a hash change between two inbound calls must trigger a reload"
        );
        let ids = router.list_plugin_ids().await;
        assert_eq!(ids.len(), 2, "reload picks up the newly added plugin");
        assert!(
            ids.contains(&"beta".to_string()),
            "newly added plugin is registered after reload, without restart"
        );
        assert_eq!(
            router.cached_catalog_hash().await.as_deref(),
            Some("hash-2"),
            "cached hash advances to the new catalog_hash after reload"
        );

        cleanup(&base);
    }

    #[tokio::test]
    async fn sealed_catalog_change_wakes_state_consumers_once() {
        let catalog = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let alpha = op_state_store::PluginSchema::builder("alpha")
            .string_field("status", false, "status")
            .public_state_path("/status")
            .build();
        let mut store = op_blob::BlobStore::open(catalog.path()).unwrap();
        store
            .write(&op_blob::blobify_plugin_schema("alpha", alpha))
            .unwrap();

        let engine = test_engine();
        let mut wakeups = engine.change_tx().subscribe();
        let mut router = SchemaRouter::with_paths(
            Arc::new(tokio::sync::OnceCell::new()),
            catalog.path().to_path_buf(),
            state.path().to_path_buf(),
            catalog.path().join(".manifest.json"),
        );
        router.engine = Some(engine);

        assert!(!router.refresh_if_changed().await.unwrap());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), wakeups.recv())
                .await
                .is_err()
        );

        let beta = op_state_store::PluginSchema::builder("beta")
            .string_field("status", false, "status")
            .public_state_path("/status")
            .build();
        store
            .write(&op_blob::blobify_plugin_schema("beta", beta))
            .unwrap();

        assert!(router.refresh_if_changed().await.unwrap());
        let wakeup = tokio::time::timeout(std::time::Duration::from_secs(1), wakeups.recv())
            .await
            .expect("catalog change wakeup timeout")
            .expect("catalog change wakeup channel");
        assert_eq!(
            wakeup.change_type,
            crate::mutation_engine::ChangeType::SchemaMigration
        );
        assert_eq!(wakeup.object_path, canonical::DBUS_ROOT_PATH);
        assert_eq!(wakeup.actor_id, "sealed-catalog");
        assert!(router.list_plugin_ids().await.contains(&"beta".to_string()));

        assert!(!router.refresh_if_changed().await.unwrap());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), wakeups.recv())
                .await
                .is_err()
        );
    }

    // ── VAL-NFR-002: hash compared on inbound arrival, not on a timer ──

    #[tokio::test]
    async fn hash_check_on_inbound_not_timer() {
        let base = test_base_dir();
        let monolith_path = base.join("live-schema.json");
        let manifest_path = base.join(".manifest.json");

        write_monolith_plugins(&monolith_path, &["alpha"]);
        write_manifest(&manifest_path, "h1");

        let conn = Arc::new(tokio::sync::OnceCell::new());
        let router = SchemaRouter::with_paths(
            conn,
            monolith_path.clone(),
            base.join("state"),
            manifest_path.clone(),
        );

        // Producer changes the catalog AFTER the router is constructed.
        write_monolith_plugins(&monolith_path, &["alpha", "beta"]);
        write_manifest(&manifest_path, "h2");

        // No inbound connection has arrived. There is NO background timer,
        // interval, or polling loop: the router must not have reloaded on its
        // own. The change is observable only when a connection arrives.
        assert_eq!(
            router.list_plugin_ids().await.len(),
            1,
            "no timer reloads the catalog; the change is picked up only on inbound arrival"
        );
        assert_eq!(router.cached_catalog_hash().await.as_deref(), Some("h1"));

        // Inbound connection arrives → catalog hash compared on arrival →
        // reload happens before dispatch.
        let reloaded = router.refresh_if_changed().await.unwrap();
        assert!(
            reloaded,
            "the catalog hash is compared on inbound connection arrival"
        );
        assert_eq!(router.list_plugin_ids().await.len(), 2);
        assert_eq!(router.cached_catalog_hash().await.as_deref(), Some("h2"));

        cleanup(&base);
    }

    // ── A missing manifest leaves the cache empty and does not reload ──

    #[tokio::test]
    async fn refresh_no_manifest_does_not_reload() {
        let base = test_base_dir();
        let monolith_path = base.join("live-schema.json");
        let manifest_path = base.join(".manifest.json");

        write_monolith_plugins(&monolith_path, &["alpha"]);
        // No manifest written.

        let conn = Arc::new(tokio::sync::OnceCell::new());
        let router = SchemaRouter::with_paths(
            conn,
            monolith_path.clone(),
            base.join("state"),
            manifest_path.clone(),
        );
        assert_eq!(router.cached_catalog_hash().await, None);

        let reloaded = router.refresh_if_changed().await.unwrap();
        assert!(
            !reloaded,
            "with no manifest there is no hash to compare; no reload occurs"
        );

        cleanup(&base);
    }

    // ── R5.1 / VAL-DISPATCH-001: Bridge dispatches to MutationEngine.mutate ──

    /// Builds a test `MutationEngine` with a real `EventChain` so dispatch
    /// tests can verify the event chain record.
    fn test_engine() -> Arc<MutationEngine> {
        use op_network::rovs_proxy::OvsdbDbusClient;
        use op_state_store::{ChainConfig, EventChain};

        let event_chain = Arc::new(RwLock::new(EventChain::new(ChainConfig::default())));
        let ovsdb = Arc::new(OvsdbDbusClient::new());
        Arc::new(MutationEngine::new(event_chain, ovsdb))
    }

    /// Builds a `PluginRoute` with a method that has a `required_capability`.
    /// Also includes ForceDispatchError for testing VAL-DISPATCH-003 error propagation.
    fn dispatch_test_route() -> PluginRoute {
        let mut methods = HashMap::new();
        methods.insert(
            "SetKey".to_string(),
            json!({
                "name": "SetKey",
                "args": { "type": "object", "properties": { "key": { "type": "string" } }, "required": ["key"] },
                "returns": null,
                "side_effect": "mutation",
                "idempotent": false,
                "required_capability": "beta.set_key",
                "subid": "mut.service.beta.set-key@v1"
            }),
        );
        methods.insert(
            "GetStatus".to_string(),
            json!({
                "name": "GetStatus",
                "args": { "type": "object", "properties": {} },
                "returns": { "type": "object" },
                "side_effect": "read",
                "idempotent": true,
                "required_capability": null,
                "subid": "obs.service.beta.get-status@v1"
            }),
        );
        // Test-only method to trigger dispatch errors for VAL-DISPATCH-003.
        // Accepts empty object as valid args, but dispatch_method_call returns an error.
        methods.insert(
            "ForceDispatchError".to_string(),
            json!({
                "name": "ForceDispatchError",
                "args": { "type": "object", "properties": {} },
                "returns": null,
                "side_effect": "read",
                "idempotent": true,
                "required_capability": null,
                "subid": "obs.service.beta.force-dispatch-error@v1"
            }),
        );
        PluginRoute {
            plugin_id: "beta".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/beta".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods,
            signals: vec![],
            properties: HashMap::new(),
        }
    }

    /// A sealed fixture has a real declaration but no compiled executor.
    /// Transport admission must reach the engine and then fail at that boundary.
    fn dispatch_test_catalog() -> crate::human_principal_dispatch::tests::TempShm {
        let route = dispatch_test_route();
        let mut schema = op_state_store::PluginSchema::builder("beta").build();
        for (name, declaration) in route.methods {
            schema
                .methods
                .insert(name, serde_json::from_value(declaration).unwrap());
        }
        let mut get_key = schema.methods["GetStatus"].clone();
        get_key.name = "GetKey".into();
        schema.methods.insert("GetKey".into(), get_key);
        crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[schema])
    }

    #[tokio::test]
    async fn bridge_dispatches_to_schema_engine_mutate() {
        let _catalog = dispatch_test_catalog();
        // VAL-DISPATCH-001: call() dispatches to MutationEngine, not a stub.
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("GetStatus".to_string(), r#"{"verbose":true}"#.to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        // The admitted call reaches the canonical engine and is audit-recorded
        // even though beta has no production backend dispatch.
        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "one event recorded for the dispatch"
        );
    }

    // ── R5.2 / VAL-DISPATCH-002: Stub removed ─────────────────────────────

    #[tokio::test]
    async fn stub_success_true_removed_from_call_path() {
        let _catalog = dispatch_test_catalog();
        // VAL-DISPATCH-002: the old stub returned {"success": true, ...}
        // without an event_id/event_hash. The admitted call now reaches the real
        // dispatch path and records accountability before failing closed.
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("GetStatus".to_string(), "{}".to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));
        assert_eq!(
            engine.event_chain.read().await.events().len(),
            1,
            "admitted unsupported call must still be audit-recorded"
        );
    }

    // ── R5.3 / VAL-DISPATCH-003: Errors propagated as fdo::Error::Failed ──

    #[tokio::test]
    async fn mutate_error_propagated_to_caller() {
        let _catalog = dispatch_test_catalog();
        // VAL-DISPATCH-003: if MutationEngine returns an error, the bridge
        // propagates it as zbus::fdo::Error::Failed.
        // Uses ForceDispatchError method with valid JSON args to trigger dispatch error
        // (without using malformed JSON, which would be rejected by arg-validation gate).
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        // Valid JSON args that pass validation, but ForceDispatchError triggers
        // an error in dispatch_method_call to test error propagation.
        let result = iface
            .call_in_test("ForceDispatchError".to_string(), "{}".to_string())
            .await;
        assert!(result.is_err(), "dispatch error must propagate to caller");
        let err = result.unwrap_err();
        assert!(
            matches!(err, zbus::fdo::Error::Failed(_)),
            "error must be zbus::fdo::Error::Failed, got {:?}",
            err
        );
    }

    // ── R5.4 / VAL-DISPATCH-004: json_args passed verbatim ─────────────────

    #[tokio::test]
    async fn mutate_receives_verbatim_json_args() {
        let _catalog = dispatch_test_catalog();
        // VAL-DISPATCH-004: the json_args argument is the verbatim string
        // from the caller. We verify the parsed value is carried directly in
        // the canonical event payload.
        // No capability check needed since GetStatus has null required_capability.
        let engine = test_engine();
        let route = PluginRoute {
            plugin_id: "beta".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/beta".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods: {
                let mut m = HashMap::new();
                m.insert(
                    "GetStatus".to_string(),
                    json!({
                        "name": "GetStatus",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "object" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.beta.get-status@v1"
                    }),
                );
                m
            },
            signals: vec![],
            properties: HashMap::new(),
        };
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let verbatim = r#"{"verbose":false}"#;
        let result = iface
            .call_in_test("GetStatus".to_string(), verbatim.to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        let chain = engine.event_chain.read().await;
        let event = chain.events().last().unwrap();
        let payload = event.input_payload.as_ref().expect("payload set");
        assert_eq!(payload["args"]["verbose"], false);
    }

    // ── R5.5 / VAL-DISPATCH-005 / VAL-NFR-003: Event chain record ────────

    #[tokio::test]
    async fn mutate_appends_event_chain_record() {
        let _catalog = dispatch_test_catalog();
        // VAL-DISPATCH-005: the event chain record includes actor_id,
        // plugin_id, method_name, capability_id, and canonical input payload.
        // Uses GetStatus (null required_capability) to avoid capability check.
        let engine = test_engine();
        let route = PluginRoute {
            plugin_id: "beta".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/beta".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods: {
                let mut m = HashMap::new();
                m.insert(
                    "GetStatus".to_string(),
                    json!({
                        "name": "GetStatus",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "object" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.beta.get-status@v1"
                    }),
                );
                m
            },
            signals: vec![],
            properties: HashMap::new(),
        };
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("GetStatus".to_string(), r#"{"verbose":false}"#.to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        let chain = engine.event_chain.read().await;
        let event = chain.events().last().unwrap();

        assert!(!event.actor_id.is_empty(), "event must carry actor_id");
        assert_eq!(event.plugin_id, "beta", "event must carry plugin_id");
        assert_eq!(
            event.method_name.as_deref(),
            Some("GetStatus"),
            "event must carry method_name"
        );
        // capability_id is null since the method has null required_capability
        assert_eq!(
            event.capability_id.as_deref(),
            None,
            "event must have null capability_id for methods without required_capability"
        );
        let payload = event.input_payload.as_ref().expect("payload set");
        assert_eq!(payload["args"]["verbose"], false);
    }

    #[tokio::test]
    async fn mutation_appended_before_dispatch_error() {
        let _catalog = dispatch_test_catalog();
        // VAL-NFR-003: the event chain append occurs before the call returns its
        // explicit unsupported-backend error.
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        {
            let chain = engine.event_chain.read().await;
            assert!(chain.events().is_empty(), "no events before dispatch");
        }

        let result = iface
            .call_in_test("GetStatus".to_string(), "{}".to_string())
            .await;

        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));
        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "event must be appended before the dispatch error is returned"
        );
    }

    // ── R7.4 / VAL-ENFORCE-004: capability_id threaded, null allowed ──────

    #[tokio::test]
    async fn null_required_capability_dispatched_without_capability() {
        let _catalog = dispatch_test_catalog();
        // VAL-ENFORCE-005: where required_capability is null, the dispatch
        // still proceeds and the event records capability_id as None.
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("GetStatus".to_string(), "{}".to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        let chain = engine.event_chain.read().await;
        let event = chain.events().last().unwrap();
        assert!(
            event.capability_id.is_none(),
            "null required_capability → event capability_id is None"
        );
    }

    // ── R6.2: Unknown method rejected without dispatch ─────────────────────

    #[tokio::test]
    async fn unknown_method_rejected_without_dispatch() {
        // VAL-VALIDATE-002: unknown method is rejected as UnknownMethod
        // without calling MutationEngine (no event recorded).
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("Nonexistent".to_string(), "{}".to_string())
            .await;
        assert!(
            matches!(result, Err(zbus::fdo::Error::UnknownMethod(_))),
            "unknown method must be rejected as UnknownMethod"
        );

        let chain = engine.event_chain.read().await;
        assert!(
            chain.events().is_empty(),
            "unknown method must not record an event (mutate not called)"
        );
    }

    // ── VAL-VALIDATE-001: Look up MethodDecl before dispatch ─────────────────

    #[tokio::test]
    async fn bridge_looks_up_method_decl_before_dispatch() {
        let _catalog = dispatch_test_catalog();
        // VAL-VALIDATE-001: the bridge must look up the MethodDecl in
        // PluginSchema.methods before dispatching to MutationEngine.
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);
        let routes = SchemaRouter::load_routes_from(&monolith_path);

        let beta = routes["beta"].clone();
        let engine = test_engine();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), beta, Some(engine.clone()));

        // The call should validate by looking up the method in the route's methods map
        // (which contains MethodDecl JSON) and then dispatch.
        // Use GetKey which has null required_capability (from write_test_monolith)
        let result = iface
            .call_in_test("GetKey".to_string(), "{}".to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "one event recorded - mutate was called after method lookup"
        );

        cleanup(&base);
    }

    // ── VAL-VALIDATE-003: Argument validation against JSON Schema ──────────────

    #[tokio::test]
    async fn invalid_args_rejected_before_mutate() {
        // VAL-VALIDATE-003: if json_args do not conform to MethodDecl.args
        // (validated via jsonschema crate), the bridge returns InvalidArgs
        // without calling mutate.
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        // SetKey requires {"key": "string"} with key in required array
        // Missing required key should be rejected
        let result = iface
            .call_in_test("SetKey".to_string(), "{}".to_string())
            .await;
        assert!(
            matches!(result, Err(zbus::fdo::Error::InvalidArgs(_))),
            "missing required key should be rejected as InvalidArgs"
        );

        // MutationEngine.mutate should NOT have been called
        let chain = engine.event_chain.read().await;
        assert!(
            chain.events().is_empty(),
            "invalid args must not trigger mutate (no event recorded)"
        );
    }

    #[tokio::test]
    async fn valid_args_pass_to_mutate() {
        let _catalog = dispatch_test_catalog();
        // Valid args conform to the schema and reach dispatch. The declared beta
        // method then fails explicitly because no canonical backend is wired.
        let engine = test_engine();
        let route = PluginRoute {
            plugin_id: "beta".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/beta".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods: {
                let mut m = HashMap::new();
                m.insert(
                    "GetStatus".to_string(),
                    json!({
                        "name": "GetStatus",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "object" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.beta.get-status@v1"
                    }),
                );
                m
            },
            signals: vec![],
            properties: HashMap::new(),
        };
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        let result = iface
            .call_in_test("GetStatus".to_string(), r#"{"verbose":false}"#.to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        // The valid call reached the engine and was audit-recorded.
        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "valid args should trigger mutate (one event recorded)"
        );
    }

    // ── VAL-VALIDATE-004: Uses loaded route schema, not per-call file read ────────────

    #[test]
    fn validation_uses_loaded_route_schema_not_per_call_file_read() {
        // VAL-VALIDATE-004: method validation uses the schema loaded from SHM
        // at bridge startup (refreshed on manifest hash change), not per-call
        // file reads. SchemaBackedInterface::validate_json_args reads from the
        // method_decl already stored in the route (which was loaded once at
        // startup), not from a file.
        let base = test_base_dir();
        let monolith_path = write_test_monolith(&base);

        // Load routes once - this simulates the one-time SHM read at startup
        let routes = SchemaRouter::load_routes_from(&monolith_path);
        let beta = routes["beta"].clone();

        // Create interface - it has no file reads inside validate_json_args
        let iface =
            SchemaBackedInterface::with_shm_state_dir("beta".to_string(), beta, base.join("state"));

        // The validate_json_args method operates on the loaded method_decl
        // (the JsonValue stored in the route), not on a file read.
        let result = iface.validate_json_args(
            iface.route.methods.get("SetKey").unwrap(),
            r#"{"key":"test"}"#,
        );
        assert!(
            result.is_ok(),
            "validation should work against loaded schema"
        );

        cleanup(&base);
    }

    // ── VAL-VALIDATE-005: Empty methods map rejects all invocations ───────────

    #[tokio::test]
    async fn empty_methods_rejects_all_invocations() {
        // VAL-VALIDATE-005: where PluginSchema.methods is empty, ALL method
        // invocations on that object are rejected as UnknownMethod.
        let base = test_base_dir();
        let empty_monolith = json!({
            "empty_plugin": {
                "name": "empty_plugin",
                "methods": {},
                "signals": [],
                "properties": {}
            }
        });
        let monolith_path = base.join("live-schema.json");
        fs::write(
            &monolith_path,
            serde_json::to_vec_pretty(&empty_monolith).unwrap(),
        )
        .unwrap();

        let routes = SchemaRouter::load_routes_from(&monolith_path);
        assert!(
            routes.contains_key("empty_plugin"),
            "empty_plugin should be registered"
        );
        assert!(
            routes["empty_plugin"].methods.is_empty(),
            "empty_plugin should have empty methods map"
        );

        let engine = test_engine();
        let route = routes["empty_plugin"].clone();
        let iface = SchemaBackedInterface::with_engine(
            "empty_plugin".to_string(),
            route,
            Some(engine.clone()),
        );

        // Any method call should be rejected as UnknownMethod
        let result = iface
            .call_in_test("AnyMethod".to_string(), "{}".to_string())
            .await;
        assert!(
            matches!(result, Err(zbus::fdo::Error::UnknownMethod(_))),
            "empty methods map must reject all invocations as UnknownMethod"
        );

        // mutate should NOT have been called
        let chain = engine.event_chain.read().await;
        assert!(
            chain.events().is_empty(),
            "empty methods must not trigger mutate"
        );

        cleanup(&base);
    }

    // ── VAL-ENFORCE-001/002: exact-principal capability grants ──

    #[tokio::test]
    async fn required_capability_check_denies_ungranted() {
        let _env_guard = test_registry_guard();
        // VAL-ENFORCE-002: When required_capability is declared, the caller's
        // principal must grant it. If not granted, the call is denied without
        // calling mutate (VAL-ENFORCE-003).
        let base = test_base_dir();

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };

        // Build the capability grants file with grants for one principal
        let grants_path = base.join("opdbus/capability-grants.json");
        std::fs::create_dir_all(grants_path.parent().unwrap()).unwrap();
        let grants_json = json!({
            "did:op:human:ungranted-test": {
                "capabilities": ["some.other.cap"]
            }
        });
        std::fs::write(&grants_path, serde_json::to_vec(&grants_json).unwrap()).unwrap();

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", grants_path.into_os_string()) };

        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        // SetKey has required_capability: "beta.set_key" which is NOT granted
        let result = iface
            .call_in_test("SetKey".to_string(), r#"{"key":"test"}"#.to_string())
            .await;

        // Should be denied as AccessDenied
        assert!(
            matches!(result, Err(zbus::fdo::Error::AccessDenied(_))),
            "ungranted capability should be denied as AccessDenied, got {:?}",
            result
        );

        // mutate should NOT have been called
        let chain = engine.event_chain.read().await;
        assert!(
            chain.events().is_empty(),
            "denied call must not trigger mutate"
        );

        cleanup(&base);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    #[tokio::test]
    async fn required_capability_check_allows_granted() {
        let _env_guard = test_registry_guard();
        let _catalog = dispatch_test_catalog();
        // VAL-ENFORCE-002: When required_capability is declared and the caller's
        // principal grants it, the call proceeds to mutate.
        let base = test_base_dir();

        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };

        // Build the capability grants file with grants for the test principal
        let grants_path_cloned = base.join("opdbus/capability-grants.json");
        std::fs::create_dir_all(grants_path_cloned.parent().unwrap()).unwrap();
        let grants_json = json!({
            (SchemaBackedInterface::TEST_CALLER_PRINCIPAL): {
                "capabilities": ["beta.set_key"]
            }
        });
        std::fs::write(
            &grants_path_cloned,
            serde_json::to_vec(&grants_json).unwrap(),
        )
        .unwrap();

        // Set OP_GRANTS_PATH to point to our test grants file
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", grants_path_cloned.into_os_string()) };

        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        // SetKey has required_capability: "beta.set_key" which IS granted
        let result = iface
            .call_in_test("SetKey".to_string(), r#"{"key":"test"}"#.to_string())
            .await;

        // The grant admits the call; beta then fails closed because it has no
        // canonical backend dispatch.
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        // Event should have been recorded after capability admission.
        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "granted capability should trigger mutate"
        );

        cleanup(&base);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    #[tokio::test]
    async fn required_capability_check_rejects_wildcard_operator_grant() {
        let _env_guard = test_registry_guard();
        let base = test_base_dir();
        let grants_path = base.join("opdbus/capability-grants.json");
        std::fs::create_dir_all(grants_path.parent().unwrap()).unwrap();
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&json!({
                "*": { "capabilities": ["beta.set_key"] }
            }))
            .unwrap(),
        )
        .unwrap();

        // Wildcard authority is never inherited by the explicit test identity.
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };

        let engine = test_engine();
        let iface = SchemaBackedInterface::with_engine(
            "beta".to_string(),
            dispatch_test_route(),
            Some(engine.clone()),
        );
        let result = iface
            .call_in_test("SetKey".to_string(), r#"{"key":"operator"}"#.to_string())
            .await;

        assert!(
            matches!(result, Err(zbus::fdo::Error::AccessDenied(_))),
            "wildcard operator grant must not dispatch: {result:?}"
        );
        assert!(engine.event_chain.read().await.events().is_empty());

        cleanup(&base);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    #[tokio::test]
    async fn dispatch_without_sender_principal_binding_fails_closed() {
        let engine = test_engine();
        let iface = SchemaBackedInterface::with_engine(
            "beta".to_string(),
            dispatch_test_route(),
            Some(engine.clone()),
        );

        let result = iface
            .dispatch_call_for_identity(
                "SetKey".to_string(),
                r#"{"key":"operator"}"#.to_string(),
                None,
            )
            .await;

        assert!(
            matches!(result, Err(zbus::fdo::Error::AccessDenied(_))),
            "dispatch must deny a call without authoritative identity: {result:?}"
        );
        assert!(engine.event_chain.read().await.events().is_empty());
    }

    // ── VAL-ENFORCE-005: Null required_capability allows any authenticated caller ──

    #[tokio::test]
    async fn null_required_capability_allows_authenticated_caller() {
        let _env_guard = test_registry_guard();
        let _catalog = dispatch_test_catalog();
        // VAL-ENFORCE-005: Where required_capability is null, the bridge allows
        // invocation by any authenticated caller without a capability check.
        let base = test_base_dir();

        // No grants file - set to empty JSON for this test path
        let grants_path = base.join("opdbus/capability-grants.json");
        std::fs::create_dir_all(grants_path.parent().unwrap()).unwrap();
        std::fs::write(&grants_path, serde_json::to_vec(&json!({})).unwrap()).unwrap();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", grants_path.into_os_string()) };

        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));

        // GetStatus has required_capability: null - should be allowed
        let result = iface
            .call_in_test("GetStatus".to_string(), r#"{"verbose":false}"#.to_string())
            .await;

        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));

        // The authenticated call was admitted and then failed closed at dispatch.
        let chain = engine.event_chain.read().await;
        assert_eq!(
            chain.events().len(),
            1,
            "null required_capability should trigger mutate"
        );

        cleanup(&base);
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    // ── VAL-CROSS-003: Full dispatch pipeline ordering ──

    #[tokio::test]
    async fn full_dispatch_pipeline_ordering() {
        let _env_guard = test_registry_guard();
        let _catalog = dispatch_test_catalog();
        // VAL-CROSS-003: Full pipeline ordering:
        // method lookup → arg validation → capability enforcement → mutate → event chain
        // This test verifies the ordering using the already-tested building blocks.

        // Test 1: Unknown method → UnknownMethod (method lookup is first)
        let engine = test_engine();
        let route = dispatch_test_route();
        let iface =
            SchemaBackedInterface::with_engine("beta".to_string(), route, Some(engine.clone()));
        let result = iface
            .call_in_test("NonexistentMethod".to_string(), "{}".to_string())
            .await;
        assert!(matches!(result, Err(zbus::fdo::Error::UnknownMethod(_))));
        assert!(
            engine.event_chain.read().await.events().is_empty(),
            "unknown method: no mutate"
        );

        // Test 2: Invalid args → InvalidArgs (arg validation is second)
        let engine2 = test_engine();
        let iface2 = SchemaBackedInterface::with_engine(
            "beta".to_string(),
            dispatch_test_route(),
            Some(engine2.clone()),
        );
        let result = iface2
            .call_in_test("SetKey".to_string(), "{}".to_string())
            .await; // Missing 'key' arg
        assert!(matches!(result, Err(zbus::fdo::Error::InvalidArgs(_))));
        assert!(
            engine2.event_chain.read().await.events().is_empty(),
            "invalid args: no mutate"
        );

        // Test 3: Missing capability → AccessDenied (capability check is third)
        // No grants file - capability check should fail before mutate
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
        let engine3 = test_engine();
        let iface3 = SchemaBackedInterface::with_engine(
            "beta".to_string(),
            dispatch_test_route(),
            Some(engine3.clone()),
        );
        let result = iface3
            .call_in_test("SetKey".to_string(), r#"{"key":"test"}"#.to_string())
            .await;
        // Should fail because no explicit operator grant is available.
        assert!(
            result.is_err(),
            "missing grant should cause error: {:?}",
            result
        );
        assert!(
            engine3.event_chain.read().await.events().is_empty(),
            "missing capability: no mutate"
        );

        // Test 4: Null required_capability → dispatch is admitted, then the
        // declared-but-unwired backend fails explicitly.
        let engine4 = test_engine();
        let route_no_cap = PluginRoute {
            plugin_id: "beta".to_string(),
            dbus_path: "/org/opdbus/v1/plugins/beta".to_string(),
            dbus_destination: "org.opdbus.v1.plugins".to_string(),
            dbus_interface: "org.opdbus.v1.PluginV1".to_string(),
            methods: {
                let mut m = HashMap::new();
                m.insert(
                    "GetStatus".to_string(),
                    json!({
                        "name": "GetStatus",
                        "args": { "type": "object", "properties": {} },
                        "returns": { "type": "object" },
                        "side_effect": "read",
                        "idempotent": true,
                        "required_capability": null,
                        "subid": "obs.service.beta.get-status@v1"
                    }),
                );
                m
            },
            signals: vec![],
            properties: HashMap::new(),
        };
        let iface4 = SchemaBackedInterface::with_engine(
            "beta".to_string(),
            route_no_cap,
            Some(engine4.clone()),
        );
        let result = iface4
            .call_in_test("GetStatus".to_string(), "{}".to_string())
            .await;
        assert!(matches!(
            result,
            Err(zbus::fdo::Error::Failed(message)) if message.contains("no compiled method compatibility evidence for sealed plugin beta")
        ));
        assert_eq!(
            engine4.event_chain.read().await.events().len(),
            1,
            "null required_capability: admitted call reached mutate"
        );
    }
}
