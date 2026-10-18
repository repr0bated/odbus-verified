//! The identity's built-in MCP server.
//!
//! One identity, fully loaded: every sealed plugin method in the projected
//! view is a tool, every plugin's schema and live state is a resource, and
//! each call runs as that identity. With `--mount`, tools, resources and
//! calls use only that identity's FUSE namespace: a tool call writes the
//! mounted `call` file and reads its receipt. The store-and-lungs path remains
//! for a schema-only guest that has no mount. Nothing here grants authority.
//! The server speaks stdio through the reference Rust SDK (`rmcp`), so any
//! IDE that launches an MCP server can use it.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, CompleteRequestParams, CompleteResult,
    CompletionInfo, ContentBlock, GetPromptRequestParams, GetPromptResponse, GetPromptResult,
    Implementation, JsonObject, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument,
    PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Reference,
    Resource, ResourceContents, ResourceTemplate, Role, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};

use crate::dispatch::{self, LungsError};
use crate::fuse_fs::Lungs;
use crate::model::{PluginSchema, SideEffect, StateValueScope, TypedObjectBlob};
use crate::mounted::{MountedDiagnostic, MountedNamespace};
use crate::namespace::ProjectionTarget;
use crate::store::ObjectStore;

const URI_PREFIX: &str = "3tched://";
const PAGE_SIZE: usize = 200;
/// Model APIs cap function names at 64 characters of `[A-Za-z0-9_-]`, and
/// IDE agents pass MCP tool names straight through, so names stay inside it.
const MAX_TOOL_NAME: usize = 64;

fn graph_notice(objects: &BTreeMap<String, CatalogDeclaration>) -> &'static str {
    if objects.get("cozo").is_some_and(|object| {
        object
            .schema
            .methods
            .get("list_system_graphs")
            .is_some_and(|method| method.side_effect == SideEffect::Read)
    }) {
        " System graph: discover cozo.list_system_graphs through identity-bound 3tchedFS; check provenance and coverage; grants apply."
    } else {
        ""
    }
}

/// The two ways to present the surface: every method as its own tool, or four
/// meta-tools that search and execute them (for small-context clients).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Every method as its own tool (can be too many for a client).
    Full,
    /// Four meta-tools that search and execute every method.
    Compact,
    /// A `toolsets` tool plus the tools of the sets this connection loaded;
    /// sets come from the sealed catalog, one per plugin.
    Toolsets,
}

/// The plugin whose sealed `publish_toolsets` method feeds the broker.
const EMQX_PLUGIN: &str = "emqx";
const PUBLISH_TOOLSETS: &str = "publish_toolsets";
const TOOLSETS_TOOL: &str = "toolsets";

/// One toolset: a plugin's methods, derived from its sealed schema.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Toolset {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
struct ToolRef {
    plugin: String,
    method: String,
}

/// Sealed declarations only. Mounted delivery keeps its provenance in the
/// pinned namespace; lungs delivery also needs the source hash for state reads.
#[derive(Clone)]
struct CatalogDeclaration {
    schema: PluginSchema,
    source_schema_hash: Option<String>,
}

/// One captured catalog: the sealed declarations of the projected view, and the
/// tool names derived from them.
struct Catalog {
    objects: BTreeMap<String, CatalogDeclaration>,
    tools: Vec<Tool>,
    by_name: HashMap<String, ToolRef>,
    sets: Vec<Toolset>,
}

impl Catalog {
    fn load(store: &ObjectStore, target: &ProjectionTarget) -> Result<Self> {
        let objects = load_objects(store, target)?;
        // Retain only sealed declarations and locators between operations.
        // Resource values are read from their authority for each request.
        let objects = objects
            .into_iter()
            .map(|(plugin, object)| {
                (
                    plugin,
                    CatalogDeclaration {
                        schema: object.schema,
                        source_schema_hash: Some(object.source_schema_hash),
                    },
                )
            })
            .collect();
        Ok(Self::from_declared(objects))
    }

    fn load_mount(mount: &MountedNamespace) -> Result<Self> {
        let mut objects = BTreeMap::new();
        for plugin in mount.plugins()? {
            let schema = mount.schema(&plugin)?;
            objects.insert(
                plugin,
                CatalogDeclaration {
                    schema,
                    source_schema_hash: None,
                },
            );
        }
        mount.verify()?;
        Ok(Self::from_declared(objects))
    }

    fn from_declared(objects: BTreeMap<String, CatalogDeclaration>) -> Self {
        let mut tools = Vec::new();
        let mut by_name = HashMap::new();
        for (plugin, object) in &objects {
            for (method, decl) in &object.schema.methods {
                let name = tool_name(plugin, method);
                let description = format!(
                    "{plugin}.{method} ({}{}). {}",
                    match decl.side_effect {
                        SideEffect::Read => "read",
                        SideEffect::Mutation => "mutation",
                    },
                    decl.required_capability
                        .as_deref()
                        .map(|capability| format!(", needs {capability}"))
                        .unwrap_or_default(),
                    object.schema.description
                );
                let annotations = ToolAnnotations::new()
                    .read_only(decl.side_effect == SideEffect::Read)
                    .destructive(decl.side_effect == SideEffect::Mutation)
                    .idempotent(decl.idempotent);
                tools.push(
                    Tool::new(name.clone(), description, input_schema(&decl.args))
                        .with_title(format!("{plugin} · {method}"))
                        .with_annotations(annotations),
                );
                by_name.insert(
                    name,
                    ToolRef {
                        plugin: plugin.clone(),
                        method: method.clone(),
                    },
                );
            }
        }
        let sets = objects
            .iter()
            .filter(|(_, object)| !object.schema.methods.is_empty())
            .map(|(plugin, object)| Toolset {
                id: plugin.clone(),
                title: object
                    .schema
                    .display_name
                    .clone()
                    .unwrap_or_else(|| plugin.clone()),
                description: object.schema.description.clone(),
                tools: object
                    .schema
                    .methods
                    .keys()
                    .map(|method| tool_name(plugin, method))
                    .collect(),
            })
            .collect();
        Self {
            objects,
            tools,
            by_name,
            sets,
        }
    }
}

fn load_objects(
    store: &ObjectStore,
    target: &ProjectionTarget,
) -> Result<BTreeMap<String, TypedObjectBlob>> {
    let mut objects = BTreeMap::new();
    match target {
        ProjectionTarget::View(id) => {
            let view = store.read_view(id)?;
            for plugin in view.objects.keys() {
                objects.insert(plugin.clone(), store.object_for_view(&view, plugin)?);
            }
        }
        ProjectionTarget::Workspace(id) => {
            let workspace = store.read_workspace(id)?;
            let view = store.read_view(&workspace.base_view)?;
            for plugin in view.objects.keys().chain(workspace.staged_objects.keys()) {
                if !objects.contains_key(plugin) {
                    objects.insert(plugin.clone(), store.workspace_object(&workspace, plugin)?);
                }
            }
        }
    }
    Ok(objects)
}

/// `plugin__method`, kept within the model-API name rules. A name that would
/// be too long keeps a readable prefix and a stable hash suffix.
fn tool_name(plugin: &str, method: &str) -> String {
    let raw: String = format!("{plugin}__{method}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if raw.len() <= MAX_TOOL_NAME {
        return raw;
    }
    let digest = blake3::hash(raw.as_bytes()).to_hex();
    format!("{}_{}", &raw[..MAX_TOOL_NAME - 13], &digest[..12])
}

/// MCP requires an object schema for tool input. Sealed argument schemas are
/// objects already; anything else is wrapped so the tool stays listable.
fn input_schema(args: &Value) -> Arc<JsonObject> {
    match args {
        Value::Object(schema) if schema.get("type").and_then(Value::as_str) == Some("object") => {
            Arc::new(schema.clone())
        }
        Value::Object(schema) if !schema.contains_key("type") => {
            let mut schema = schema.clone();
            schema.insert("type".into(), json!("object"));
            Arc::new(schema)
        }
        _ => Arc::new(
            json!({"type": "object", "additionalProperties": true})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        ),
    }
}

fn page<T: Clone>(items: &[T], cursor: Option<&str>) -> Result<(Vec<T>, Option<String>), McpError> {
    let start = match cursor {
        None => 0,
        Some(cursor) => cursor
            .parse::<usize>()
            .ok()
            .filter(|start| *start <= items.len())
            .ok_or_else(|| McpError::invalid_params("unknown cursor", None))?,
    };
    let end = (start + PAGE_SIZE).min(items.len());
    let next = (end < items.len()).then(|| end.to_string());
    Ok((items[start..end].to_vec(), next))
}

fn cursor(request: &Option<PaginatedRequestParams>) -> Option<&str> {
    request
        .as_ref()
        .and_then(|request| request.cursor.as_deref())
}

fn mounted_call_error(error: anyhow::Error) -> CallToolResult {
    let stage = error
        .downcast_ref::<LungsError>()
        .map(LungsError::stage)
        .or_else(|| error.downcast_ref::<MountedDiagnostic>().map(|_| "call"))
        .unwrap_or("mount");
    text_result(&json!({"error": error.to_string(), "stage": stage}), true)
}

fn text_result(value: &Value, error: bool) -> CallToolResult {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    match (value.is_object(), error) {
        (true, false) => {
            let mut result = CallToolResult::structured(value.clone());
            result.content = vec![ContentBlock::text(text)];
            result
        }
        (_, false) => CallToolResult::success(vec![ContentBlock::text(text)]),
        (_, true) => CallToolResult::error(vec![ContentBlock::text(text)]),
    }
}

/// Where this MCP server reads declarations and submits calls.
#[derive(Clone)]
enum Delivery {
    /// Schema-only guest: object store plus a direct lungs connection.
    Lungs {
        store: ObjectStore,
        catalog_dir: PathBuf,
        target: ProjectionTarget,
        state_dir: PathBuf,
        lungs: Arc<Lungs>,
        view_id: String,
    },
    /// Identity mount: every read and call stays inside that namespace.
    Mount {
        namespace: Arc<MountedNamespace>,
        view_id: String,
    },
}

#[derive(Clone)]
pub struct IdentityMcp {
    delivery: Delivery,
    surface: Surface,
    catalog: Arc<RwLock<Arc<Catalog>>>,
    /// Sets this connection has loaded (toolset surface only).
    loaded: Arc<RwLock<std::collections::BTreeSet<String>>>,
}

impl IdentityMcp {
    pub fn new(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
        lungs: Lungs,
        surface: Surface,
    ) -> Result<Self> {
        let catalog = Catalog::load(&store, &target).context("load the projected catalog")?;
        let view_id = match &target {
            ProjectionTarget::View(id) => id.clone(),
            ProjectionTarget::Workspace(id) => store.read_workspace(id)?.base_view,
        };
        Ok(Self {
            delivery: Delivery::Lungs {
                store,
                catalog_dir: crate::source::DEFAULT_CATALOG_DIR.into(),
                target,
                state_dir,
                lungs: Arc::new(lungs),
                view_id,
            },
            surface,
            catalog: Arc::new(RwLock::new(Arc::new(catalog))),
            loaded: Arc::new(RwLock::new(Default::default())),
        })
    }

    /// Tools, resources and calls come only from this identity's FUSE mount.
    pub fn from_mount(namespace: MountedNamespace, surface: Surface) -> Result<Self> {
        let view_id = namespace.view_id()?.to_owned();
        let catalog = Catalog::load_mount(&namespace).context("load the mounted catalog")?;
        Ok(Self {
            delivery: Delivery::Mount {
                namespace: Arc::new(namespace),
                view_id,
            },
            surface,
            catalog: Arc::new(RwLock::new(Arc::new(catalog))),
            loaded: Arc::new(RwLock::new(Default::default())),
        })
    }

    pub fn with_catalog_dir(mut self, catalog_dir: PathBuf) -> Self {
        if let Delivery::Lungs {
            catalog_dir: slot, ..
        } = &mut self.delivery
        {
            *slot = catalog_dir;
        }
        self
    }

    fn view_id(&self) -> &str {
        match &self.delivery {
            Delivery::Lungs { view_id, .. } | Delivery::Mount { view_id, .. } => view_id,
        }
    }

    /// Load sets before any client connects (`--load`).
    pub fn preload(&self, ids: &[String]) -> Result<()> {
        let catalog = self.catalog();
        for id in ids {
            anyhow::ensure!(
                catalog.sets.iter().any(|set| &set.id == id),
                "no toolset {id} in this view"
            );
        }
        self.loaded.write().extend(ids.iter().cloned());
        Ok(())
    }

    /// The toolset manifest derived from the sealed catalog.
    pub fn toolsets(&self) -> Vec<Toolset> {
        self.catalog().sets.clone()
    }

    /// Feed the broker: publish the manifest through the sealed
    /// `emqx.publish_toolsets` method, as this identity. The broker then
    /// holds it retained on `opdbus/toolsets` for every live identity.
    pub async fn announce_toolsets(&self) -> Result<Value> {
        let arguments = json!({
            "view_id": self.view_id(),
            "sets": self.toolsets(),
        });
        let result = self
            .dispatch(EMQX_PLUGIN, PUBLISH_TOOLSETS, arguments)
            .await;
        let body = result
            .structured_content
            .clone()
            .or_else(|| {
                result
                    .content
                    .first()
                    .and_then(|content| content.as_text())
                    .and_then(|text| serde_json::from_str(&text.text).ok())
            })
            .unwrap_or(Value::Null);
        if result.is_error == Some(true) {
            anyhow::bail!("publish toolsets: {body}");
        }
        Ok(body)
    }

    fn toolsets_tool() -> Tool {
        Tool::new(
            TOOLSETS_TOOL,
            "List the toolsets (one per plugin) and load or unload them. Loading adds a set's \
             tools to this connection; the list is short until you load what you need.",
            Arc::new(
                json!({
                    "type": "object",
                    "properties": {
                        "operation": {"type": "string", "enum": ["list", "load", "unload"]},
                        "ids": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["operation"]
                })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            ),
        )
        .with_annotations(ToolAnnotations::new().read_only(true).idempotent(true))
    }

    fn loaded_tools(&self) -> Vec<Tool> {
        let catalog = self.catalog();
        let loaded = self.loaded.read();
        let mut tools = vec![Self::toolsets_tool()];
        tools.extend(
            catalog
                .tools
                .iter()
                .filter(|tool| {
                    catalog
                        .by_name
                        .get(tool.name.as_ref())
                        .is_some_and(|reference| loaded.contains(&reference.plugin))
                })
                .cloned(),
        );
        tools
    }

    async fn call_toolsets(
        &self,
        arguments: &JsonObject,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResult {
        let catalog = self.catalog();
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("list");
        let ids: Vec<String> = arguments
            .get("ids")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        match operation {
            "list" => {
                let loaded = self.loaded.read();
                let sets: Vec<Value> = catalog
                    .sets
                    .iter()
                    .map(|set| {
                        json!({
                            "id": set.id,
                            "title": set.title,
                            "description": set.description,
                            "tools": set.tools.len(),
                            "loaded": loaded.contains(&set.id),
                        })
                    })
                    .collect();
                text_result(&json!({"toolsets": sets}), false)
            }
            "load" | "unload" => {
                let unknown: Vec<&String> = ids
                    .iter()
                    .filter(|id| !catalog.sets.iter().any(|set| &set.id == *id))
                    .collect();
                if ids.is_empty() || !unknown.is_empty() {
                    return text_result(
                        &json!({"error": "name one or more toolset ids from list", "unknown": unknown}),
                        true,
                    );
                }
                let changed = {
                    let mut loaded = self.loaded.write();
                    let mut changed = false;
                    for id in &ids {
                        changed |= if operation == "load" {
                            loaded.insert(id.clone())
                        } else {
                            loaded.remove(id)
                        };
                    }
                    changed
                };
                if changed {
                    // Clients re-list on this notification; a client that
                    // does not still sees the new tools on its next list.
                    let _ = context.peer.notify_tool_list_changed().await;
                }
                let loaded: Vec<String> = self.loaded.read().iter().cloned().collect();
                text_result(&json!({"loaded": loaded, "changed": changed}), false)
            }
            other => text_result(
                &json!({"error": format!("unknown operation {other}")}),
                true,
            ),
        }
    }

    /// Serve MCP on stdin/stdout until the client closes it.
    pub async fn serve_stdio(self) -> Result<()> {
        let running = self
            .serve(rmcp::transport::stdio())
            .await
            .context("start the MCP stdio session")?;
        running.waiting().await.context("MCP session ended")?;
        Ok(())
    }

    fn catalog(&self) -> Arc<Catalog> {
        self.catalog.read().clone()
    }

    fn live_state(&self, plugin: &str, object: &CatalogDeclaration) -> Result<Value, McpError> {
        let read = || -> Result<Value> {
            let Delivery::Lungs {
                store,
                catalog_dir,
                target,
                state_dir,
                view_id,
                ..
            } = &self.delivery
            else {
                let Delivery::Mount { namespace, .. } = &self.delivery else {
                    unreachable!("delivery is mount when it is not lungs");
                };
                return namespace.state(plugin);
            };
            if let ProjectionTarget::Workspace(id) = target {
                let workspace = store.read_workspace(id)?;
                let object = store
                    .workspace_object(&workspace, plugin)?
                    .public_projection()?;
                anyhow::ensure!(
                    object.value_scope != StateValueScope::SchemaOnly,
                    "workspace state unavailable"
                );
                return Ok(object.value);
            }
            let view = store.read_view(view_id)?;
            let hash = object
                .source_schema_hash
                .as_ref()
                .context("source schema hash absent")?;
            let hashes = BTreeMap::from([(plugin.to_owned(), hash.clone())]);
            let schemas = BTreeMap::from([(plugin.to_owned(), object.schema.clone())]);
            let verify = || {
                crate::source::verify_pinned_catalog(
                    catalog_dir,
                    &view.source_catalog_hash,
                    view.source_catalog_generation,
                    &hashes,
                    &schemas,
                )
            };
            verify()?;
            let committed = crate::source::read_committed_public_value(
                state_dir,
                plugin,
                &object.schema,
                16 * 1024 * 1024,
            )?;
            verify()?;
            Ok(committed.value)
        };
        read().map_err(|_| McpError::internal_error("committed public state unavailable", None))
    }

    async fn dispatch(&self, plugin: &str, method: &str, arguments: Value) -> CallToolResult {
        let decl = match self
            .catalog()
            .objects
            .get(plugin)
            .and_then(|object| object.schema.methods.get(method).cloned())
        {
            Some(decl) => decl,
            None => {
                return text_result(
                    &json!({"error": format!("{plugin}.{method} is not in this view")}),
                    true,
                );
            }
        };
        if let Delivery::Mount { namespace, .. } = &self.delivery {
            let namespace = Arc::clone(namespace);
            let plugin = plugin.to_owned();
            let method = method.to_owned();
            let outcome = tokio::task::spawn_blocking(move || {
                namespace.call(&plugin, &method, &decl, arguments)
            })
            .await;
            return match outcome {
                Ok(Ok(receipt)) => text_result(&receipt["result"], false),
                Ok(Err(error)) => mounted_call_error(error),
                Err(error) => {
                    text_result(&json!({"error": error.to_string(), "stage": "mount"}), true)
                }
            };
        }
        let Delivery::Lungs { lungs, .. } = &self.delivery else {
            return text_result(
                &json!({"error": "mount delivery was not dispatched", "stage": "mount"}),
                true,
            );
        };
        let outcome: Result<_, LungsError> = async {
            dispatch::validate_arguments(&decl, &arguments)?;
            match lungs.as_ref() {
                Lungs::Socket { path, session } => {
                    dispatch::call_over_socket(path, session, plugin, method, &decl, arguments)
                        .await
                }
                Lungs::Credential {
                    bus_address,
                    credential,
                } => {
                    let credential = dispatch::read_credential(credential)?;
                    dispatch::call_as_identity(
                        bus_address,
                        plugin,
                        method,
                        &decl,
                        arguments,
                        credential,
                    )
                    .await
                }
            }
        }
        .await;
        match outcome {
            Ok(receipt) => text_result(&receipt.result, false),
            Err(error) => text_result(
                &json!({"error": error.to_string(), "stage": error.stage()}),
                true,
            ),
        }
    }

    fn compact_tools() -> Vec<Tool> {
        let schema = |value: Value| Arc::new(value.as_object().cloned().unwrap_or_default());
        vec![
            Tool::new(
                "list_plugins",
                "List every plugin this identity sees, with its description and method count.",
                schema(json!({"type": "object", "properties": {}})),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "search_tools",
                "Search every method by plugin, method name or description.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 200}
                    },
                    "required": ["query"]
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "get_tool_schema",
                "The sealed input schema and side effect of one method.",
                schema(json!({
                    "type": "object",
                    "properties": {"name": {"type": "string"}},
                    "required": ["name"]
                })),
            )
            .with_annotations(ToolAnnotations::new().read_only(true)),
            Tool::new(
                "execute_tool",
                "Run one method as this identity. Mutations change the system.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "arguments": {"type": "object"}
                    },
                    "required": ["name"]
                })),
            )
            .with_annotations(ToolAnnotations::new().destructive(true)),
        ]
    }

    async fn call_compact(&self, name: &str, arguments: JsonObject) -> CallToolResult {
        let catalog = self.catalog();
        match name {
            "list_plugins" => {
                let plugins: Vec<Value> = catalog
                    .objects
                    .iter()
                    .map(|(plugin, object)| {
                        json!({
                            "plugin": plugin,
                            "description": object.schema.description,
                            "methods": object.schema.methods.len(),
                        })
                    })
                    .collect();
                text_result(&json!({"plugins": plugins}), false)
            }
            "search_tools" => {
                let query = arguments
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_lowercase();
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .clamp(1, 200) as usize;
                let hits: Vec<Value> = catalog
                    .tools
                    .iter()
                    .filter(|tool| {
                        tool.name.to_lowercase().contains(&query)
                            || tool
                                .description
                                .as_deref()
                                .is_some_and(|text| text.to_lowercase().contains(&query))
                    })
                    .take(limit)
                    .map(|tool| json!({"name": tool.name, "description": tool.description}))
                    .collect();
                text_result(&json!({"tools": hits}), false)
            }
            "get_tool_schema" => {
                let wanted = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match catalog.tools.iter().find(|tool| tool.name == wanted) {
                    Some(tool) => text_result(
                        &json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.input_schema.as_ref(),
                            "annotations": tool.annotations,
                        }),
                        false,
                    ),
                    None => text_result(&json!({"error": format!("no tool {wanted}")}), true),
                }
            }
            "execute_tool" => {
                let wanted = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let inner = arguments
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match catalog.by_name.get(wanted) {
                    Some(tool) => self.dispatch(&tool.plugin, &tool.method, inner).await,
                    None => text_result(&json!({"error": format!("no tool {wanted}")}), true),
                }
            }
            _ => text_result(&json!({"error": format!("no tool {name}")}), true),
        }
    }

    fn resources(&self) -> Vec<Resource> {
        let catalog = self.catalog();
        let mut resources = vec![
            Resource::new(format!("{URI_PREFIX}view"), "view")
                .with_title("Projected view")
                .with_description("The captured view: its id, source catalog and plugins.")
                .with_mime_type("application/json"),
        ];
        for (plugin, object) in &catalog.objects {
            resources.push(
                Resource::new(
                    format!("{URI_PREFIX}plugins/{plugin}/schema"),
                    format!("{plugin} schema"),
                )
                .with_description(format!(
                    "Sealed schema of {plugin}. {}",
                    object.schema.description
                ))
                .with_mime_type("application/json"),
            );
            resources.push(
                Resource::new(
                    format!("{URI_PREFIX}plugins/{plugin}/state"),
                    format!("{plugin} state"),
                )
                .with_description(format!("Present state of {plugin}."))
                .with_mime_type("application/json"),
            );
        }
        resources
    }

    fn read(&self, uri: &str) -> Result<Value, McpError> {
        let not_found = || McpError::resource_not_found(format!("no resource {uri}"), None);
        let path = uri.strip_prefix(URI_PREFIX).ok_or_else(not_found)?;
        let catalog = self.catalog();
        if path == "view" {
            let target = match &self.delivery {
                Delivery::Lungs { target, .. } => match target {
                    ProjectionTarget::View(id) => json!({"view": id}),
                    ProjectionTarget::Workspace(id) => json!({"workspace": id}),
                },
                Delivery::Mount { view_id, .. } => json!({"mount": true, "view": view_id}),
            };
            return Ok(json!({
                "target": target,
                "plugins": catalog.objects.keys().collect::<Vec<_>>(),
            }));
        }
        let rest = path.strip_prefix("plugins/").ok_or_else(not_found)?;
        let (plugin, rest) = rest.split_once('/').ok_or_else(not_found)?;
        let object = catalog.objects.get(plugin).ok_or_else(not_found)?;
        match rest.split_once('/') {
            None if rest == "schema" => serde_json::to_value(&object.schema).map_err(|error| {
                McpError::internal_error(format!("schema of {plugin}: {error}"), None)
            }),
            None if rest == "state" => self.live_state(plugin, object),
            Some(("state", pointer)) => self
                .live_state(plugin, object)?
                .pointer(&format!("/{pointer}"))
                .cloned()
                .ok_or_else(not_found),
            _ => Err(not_found()),
        }
    }

    fn plugin_prompt(&self, plugin: &str, task: Option<&str>) -> Result<GetPromptResult, McpError> {
        let catalog = self.catalog();
        let object = catalog
            .objects
            .get(plugin)
            .ok_or_else(|| McpError::invalid_params(format!("no plugin {plugin}"), None))?;
        let mut text = format!(
            "Plugin `{plugin}`: {}\n\nMethods (tool names in brackets):\n",
            object.schema.description
        );
        for (method, decl) in &object.schema.methods {
            text.push_str(&format!(
                "- {method} [{}], {}: args {}\n",
                tool_name(plugin, method),
                match decl.side_effect {
                    SideEffect::Read => "read",
                    SideEffect::Mutation => "mutation",
                },
                decl.args
            ));
        }
        text.push_str(&format!(
            "\nState fields: {}\nRead `{URI_PREFIX}plugins/{plugin}/state` for present values.\n",
            object
                .schema
                .fields
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if let Some(task) = task.filter(|task| !task.trim().is_empty()) {
            text.push_str(&format!("\nTask: {task}\n"));
        }
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
                .with_description(format!("Work with {plugin} as this identity")),
        )
    }
}

impl ServerHandler for IdentityMcp {
    fn get_info(&self) -> ServerConfig {
        let mut instructions = String::from(
            "This server is one OP-DBUS identity. Every tool is a sealed plugin method and runs \
             as this identity under its own grants; mutations change the live system. Resources \
             hold each plugin's sealed schema and present state. The `plugin` prompt summarizes \
             one plugin's methods.",
        );
        instructions.push_str(graph_notice(&self.catalog().objects));
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_completions()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
        .with_server_info(
            Implementation::new("3tchedfs", env!("CARGO_PKG_VERSION"))
                .with_title("3tchedFS identity")
                .with_description("Every sealed OP-DBUS plugin, as this identity."),
        )
        .with_instructions(instructions)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = match self.surface {
            Surface::Full => self.catalog().tools.clone(),
            Surface::Compact => Self::compact_tools(),
            Surface::Toolsets => self.loaded_tools(),
        };
        let (tools, next) = page(&tools, cursor(&request))?;
        let mut result = ListToolsResult::with_all_items(tools);
        result.next_cursor = next;
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        match self.surface {
            Surface::Full => self
                .catalog()
                .tools
                .iter()
                .find(|tool| tool.name == name)
                .cloned(),
            Surface::Compact => Self::compact_tools()
                .into_iter()
                .find(|tool| tool.name == name),
            Surface::Toolsets => self
                .loaded_tools()
                .into_iter()
                .find(|tool| tool.name == name),
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name: Cow<'static, str> = request.name;
        let arguments = request.arguments.unwrap_or_default();
        let result = match self.surface {
            Surface::Compact => self.call_compact(&name, arguments).await,
            Surface::Toolsets if name == TOOLSETS_TOOL => {
                self.call_toolsets(&arguments, &context).await
            }
            // Loading is about what the client lists; the identity can still
            // call any method it is granted.
            Surface::Full | Surface::Toolsets => {
                let tool = self
                    .catalog()
                    .by_name
                    .get(name.as_ref())
                    .cloned()
                    .ok_or_else(|| McpError::invalid_params(format!("no tool {name}"), None))?;
                self.dispatch(&tool.plugin, &tool.method, Value::Object(arguments))
                    .await
            }
        };
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let (resources, next) = page(&self.resources(), cursor(&request))?;
        let mut result = ListResourcesResult::with_all_items(resources);
        result.next_cursor = next;
        Ok(result)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new(
                format!("{URI_PREFIX}plugins/{{plugin}}/schema"),
                "plugin schema",
            )
            .with_description("Sealed schema of one plugin.")
            .with_mime_type("application/json"),
            ResourceTemplate::new(
                format!("{URI_PREFIX}plugins/{{plugin}}/state"),
                "plugin state",
            )
            .with_description("Present state of one plugin.")
            .with_mime_type("application/json"),
            ResourceTemplate::new(
                format!("{URI_PREFIX}plugins/{{plugin}}/state/{{+pointer}}"),
                "plugin state value",
            )
            .with_description("One value in a plugin's present state, by JSON pointer path.")
            .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let value = self.read(&request.uri)?;
        let text = serde_json::to_string_pretty(&value)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri).with_mime_type("application/json"),
        ])
        .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(vec![
            Prompt::new(
                "plugin",
                Some("Brief the model on one plugin: its methods, their tools and its state."),
                Some(vec![
                    PromptArgument::new("plugin")
                        .with_description("Plugin id")
                        .with_required(true),
                    PromptArgument::new("task")
                        .with_description("What to do with it")
                        .with_required(false),
                ]),
            ),
            Prompt::new(
                "catalog",
                Some("Every plugin this identity sees, one line each."),
                None,
            ),
        ]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let arguments = request.arguments.unwrap_or_default();
        let argument = |name: &str| arguments.get(name).and_then(Value::as_str);
        let result = match request.name.as_str() {
            "plugin" => {
                let plugin = argument("plugin")
                    .ok_or_else(|| McpError::invalid_params("plugin is required", None))?;
                self.plugin_prompt(plugin, argument("task"))?
            }
            "catalog" => {
                let lines: Vec<String> = self
                    .catalog()
                    .objects
                    .iter()
                    .map(|(plugin, object)| {
                        format!(
                            "- {plugin} ({} methods): {}",
                            object.schema.methods.len(),
                            object.schema.description
                        )
                    })
                    .collect();
                GetPromptResult::new(vec![PromptMessage::new_text(
                    Role::User,
                    format!(
                        "OP-DBUS plugins available to this identity:\n{}",
                        lines.join("\n")
                    ),
                )])
            }
            other => {
                return Err(McpError::invalid_params(format!("no prompt {other}"), None));
            }
        };
        Ok(result.into())
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, McpError> {
        let wants_plugin = request.argument.name == "plugin"
            && match &request.r#ref {
                Reference::Prompt(prompt) => prompt.name == "plugin",
                Reference::Resource(resource) => resource.uri.starts_with(URI_PREFIX),
                _ => false,
            };
        let values: Vec<String> = if wants_plugin {
            self.catalog()
                .objects
                .keys()
                .filter(|plugin| plugin.starts_with(&request.argument.value))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let total = values.len();
        let shown: Vec<String> = values
            .into_iter()
            .take(CompletionInfo::MAX_VALUES)
            .collect();
        let info = CompletionInfo::with_pagination(
            shown,
            Some(total as u32),
            total > CompletionInfo::MAX_VALUES,
        )
        .map_err(|error| McpError::internal_error(error, None))?;
        Ok(CompleteResult::new(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_startup_notice_tracks_read_method_availability() {
        let (_directory, server) = fixture();
        let mut objects = server.catalog().objects.clone();
        assert!(graph_notice(&objects).is_empty());
        let mut cozo = objects["demo"].clone();
        let mut method = cozo.schema.methods["get_status"].clone();
        method.name = "list_system_graphs".into();
        cozo.schema
            .methods
            .insert("list_system_graphs".into(), method);
        objects.insert("cozo".into(), cozo);
        assert!(graph_notice(&objects).contains("cozo.list_system_graphs"));
        objects
            .get_mut("cozo")
            .unwrap()
            .schema
            .methods
            .get_mut("list_system_graphs")
            .unwrap()
            .side_effect = SideEffect::Mutation;
        assert!(graph_notice(&objects).is_empty());
    }

    #[test]
    fn tool_names_fit_model_api_rules() {
        assert_eq!(tool_name("net", "get_status"), "net__get_status");
        let long = tool_name(&"p".repeat(50), &"m".repeat(50));
        assert_eq!(long.len(), MAX_TOOL_NAME);
        assert!(
            long.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        assert_eq!(long, tool_name(&"p".repeat(50), &"m".repeat(50)));
        assert_eq!(tool_name("a.b", "c d"), "a_b__c_d");
    }

    #[test]
    fn input_schemas_are_objects() {
        assert_eq!(input_schema(&json!({"type": "object"}))["type"], "object");
        assert_eq!(input_schema(&json!({"properties": {}}))["type"], "object");
        assert_eq!(input_schema(&json!(true))["type"], "object");
    }

    #[test]
    fn pages_follow_cursors() {
        let items: Vec<u32> = (0..450).collect();
        let (first, next) = page(&items, None).unwrap();
        assert_eq!(first.len(), PAGE_SIZE);
        let (second, next) = page(&items, next.as_deref()).unwrap();
        assert_eq!(second[0], 200);
        let (third, next) = page(&items, next.as_deref()).unwrap();
        assert_eq!(third.len(), 50);
        assert!(next.is_none());
        assert!(page(&items, Some("bogus")).is_err());
    }

    /// One sealed `demo` plugin with a read and a mutation method, captured
    /// into a view, plus its live state file.
    fn fixture() -> (tempfile::TempDir, IdentityMcp) {
        fixture_with(Surface::Full)
    }

    fn fixture_with(surface: Surface) -> (tempfile::TempDir, IdentityMcp) {
        use crate::model::MethodDecl;

        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        op_core::projection_shm::update_projection_in(state.to_str().unwrap(), "demo", |_| {
            Ok(Some(
                br#"{"mode":"live","nested":{"n":7},"secret":"private"}"#.to_vec(),
            ))
        })
        .unwrap();
        let store = ObjectStore::open(directory.path().join("store")).unwrap();
        let method = |name: &str, side_effect| MethodDecl {
            name: name.into(),
            args: json!({
                "type": "object",
                "properties": {"event": {"type": "string"}},
                "required": ["event"],
                "additionalProperties": false
            }),
            returns: Some(json!({"type":"object"})),
            side_effect,
            idempotent: false,
            required_capability: Some("cap.demo@v1".into()),
            subid: format!("demo.{name}@v1"),
        };
        let mut schema = crate::model::from_sealed(serde_json::from_value(json!({
            "name":"demo", "version":"1", "description":"Demo plugin",
            "fields": {
                "mode":{"field_type":"string","required":true},
                "nested":{"field_type":{"object":{"n":{"field_type":"integer","required":true}}},"required":true},
                "secret":{"field_type":"string","required":true}
            },
            "public_state":{"paths":["/mode","/nested"]}
        })).unwrap()).unwrap();
        schema.methods = BTreeMap::from([
            ("get_status".into(), method("get_status", SideEffect::Read)),
            (
                "record_event".into(),
                method("record_event", SideEffect::Mutation),
            ),
        ]);
        let source = crate::source::tests::fixture_catalog(directory.path(), &schema);
        let view = store.capture(&source, true).unwrap();
        let lungs = Lungs::Socket {
            path: directory.path().join("no-lungs.sock"),
            session: "session".into(),
        };
        let server = IdentityMcp::new(
            store,
            ProjectionTarget::View(view.view_id),
            state,
            lungs,
            surface,
        )
        .unwrap()
        .with_catalog_dir(source.catalog_dir);
        (directory, server)
    }

    fn text(result: &CallToolResult) -> String {
        result.content[0].as_text().unwrap().text.clone()
    }

    #[test]
    fn resources_use_public_committed_values_without_fallback_or_retention() {
        let (_directory, server) = fixture();
        let value = server.read("3tched://plugins/demo/state").unwrap();
        assert_eq!(value, json!({"mode":"live","nested":{"n":7}}));
        assert!(server.read("3tched://plugins/demo/state/secret").is_err());
        let Delivery::Lungs { state_dir, .. } = &server.delivery else {
            panic!("fixture uses lungs delivery");
        };
        op_core::projection_shm::update_projection_in(state_dir.to_str().unwrap(), "demo", |_| {
            Ok(None)
        })
        .unwrap();
        assert!(server.read("3tched://plugins/demo/state").is_err());
    }

    #[test]
    fn resources_refuse_a_catalog_that_no_longer_authorizes_the_pin() {
        let (_directory, server) = fixture();
        let Delivery::Lungs { catalog_dir, .. } = &server.delivery else {
            panic!("fixture uses lungs delivery");
        };
        let path = catalog_dir.join(".manifest.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["generation"] = json!(2);
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(server.read("3tched://plugins/demo/state").is_err());
    }

    #[tokio::test]
    async fn an_ide_sees_every_surface_over_stdio() {
        use rmcp::model::{
            CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams,
        };

        let (_directory, server) = fixture();
        let (client_io, server_io) = tokio::io::duplex(1 << 16);
        let serving = tokio::spawn(async move {
            server
                .serve(server_io)
                .await
                .unwrap()
                .waiting()
                .await
                .unwrap();
        });
        let client = ().serve(client_io).await.unwrap();

        let info = client.peer_info().unwrap();
        assert_eq!(info.server_info.as_ref().unwrap().name, "3tchedfs");
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
        assert!(info.capabilities.prompts.is_some());
        assert!(info.capabilities.completions.is_some());

        let tools = client.list_all_tools().await.unwrap();
        let names: Vec<_> = tools.iter().map(|tool| tool.name.to_string()).collect();
        assert_eq!(names, ["demo__get_status", "demo__record_event"]);
        let mutation = tools[1].annotations.as_ref().unwrap();
        assert_eq!(mutation.destructive_hint, Some(true));
        assert_eq!(
            tools[0].annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );

        // Arguments are checked against the sealed schema before any dispatch.
        let refused = client
            .call_tool(
                CallToolRequestParams::new("demo__get_status")
                    .with_arguments(json!({"wrong": 1}).as_object().cloned().unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(refused.is_error, Some(true));
        assert!(
            text(&refused).contains("\"stage\": \"validate\""),
            "{}",
            text(&refused)
        );

        // A valid call goes to the identity's lungs; here there are none.
        let unreachable = client
            .call_tool(
                CallToolRequestParams::new("demo__get_status")
                    .with_arguments(json!({"event": "x"}).as_object().cloned().unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(unreachable.is_error, Some(true));
        assert!(text(&unreachable).contains("\"stage\": \"transport\""));

        let state = client
            .read_resource(ReadResourceRequestParams::new(
                "3tched://plugins/demo/state",
            ))
            .await
            .unwrap();
        let body = serde_json::to_value(&state.contents[0]).unwrap();
        assert!(
            body["text"].as_str().unwrap().contains("\"live\""),
            "{body}"
        );
        let pointer = client
            .read_resource(ReadResourceRequestParams::new(
                "3tched://plugins/demo/state/nested/n",
            ))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&pointer.contents[0]).unwrap()["text"],
            "7"
        );

        let prompt = client
            .get_prompt(
                GetPromptRequestParams::new("plugin")
                    .with_arguments(json!({"plugin": "demo"}).as_object().cloned().unwrap()),
            )
            .await
            .unwrap();
        let prompt = serde_json::to_string(&prompt.messages).unwrap();
        assert!(prompt.contains("demo__record_event"), "{prompt}");

        client.cancel().await.unwrap();
        serving.await.unwrap();
    }

    #[tokio::test]
    async fn toolsets_keep_the_list_short_until_loaded() {
        use rmcp::model::CallToolRequestParams;

        let (_directory, server) = fixture_with(Surface::Toolsets);
        let manifest = server.toolsets();
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].id, "demo");
        assert_eq!(
            manifest[0].tools,
            ["demo__get_status", "demo__record_event"]
        );

        let (client_io, server_io) = tokio::io::duplex(1 << 16);
        let serving = tokio::spawn(async move {
            server
                .serve(server_io)
                .await
                .unwrap()
                .waiting()
                .await
                .unwrap();
        });
        let client = ().serve(client_io).await.unwrap();
        let names = |tools: Vec<Tool>| {
            tools
                .into_iter()
                .map(|tool| tool.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(client.list_all_tools().await.unwrap()), ["toolsets"]);
        let args = |value: Value| value.as_object().cloned().unwrap();
        let loaded = client
            .call_tool(
                CallToolRequestParams::new("toolsets")
                    .with_arguments(args(json!({"operation": "load", "ids": ["demo"]}))),
            )
            .await
            .unwrap();
        assert_eq!(loaded.is_error, Some(false), "{}", text(&loaded));
        assert_eq!(
            names(client.list_all_tools().await.unwrap()),
            ["toolsets", "demo__get_status", "demo__record_event"]
        );
        let unknown = client
            .call_tool(
                CallToolRequestParams::new("toolsets")
                    .with_arguments(args(json!({"operation": "load", "ids": ["nope"]}))),
            )
            .await
            .unwrap();
        assert_eq!(unknown.is_error, Some(true));
        client
            .call_tool(
                CallToolRequestParams::new("toolsets")
                    .with_arguments(args(json!({"operation": "unload", "ids": ["demo"]}))),
            )
            .await
            .unwrap();
        assert_eq!(names(client.list_all_tools().await.unwrap()), ["toolsets"]);
        client.cancel().await.unwrap();
        serving.await.unwrap();
    }

    #[tokio::test]
    async fn announcing_needs_the_sealed_emqx_method() {
        // The demo view has no `emqx` plugin, so nothing reaches a broker.
        let (_directory, server) = fixture_with(Surface::Toolsets);
        let error = server.announce_toolsets().await.unwrap_err().to_string();
        assert!(error.contains("not in this view"), "{error}");
    }

    #[test]
    fn mounted_discovery_needs_no_state_payload_and_resources_remain_fresh() {
        let (directory, namespace, _) = crate::mounted::tests::fixture();
        let plugin = directory.path().join("plugins/demo");
        std::fs::remove_file(plugin.join("object.blob.json")).unwrap();
        std::fs::remove_file(plugin.join("value.json")).unwrap();
        let server = IdentityMcp::from_mount(namespace, Surface::Full).unwrap();
        assert_eq!(server.catalog().tools.len(), 1);
        assert_eq!(server.toolsets()[0].id, "demo");
        assert!(
            server.catalog().objects["demo"]
                .source_schema_hash
                .is_none()
        );
        assert_eq!(
            server.read("3tched://plugins/demo/schema").unwrap()["name"],
            "demo"
        );
        assert!(server.read("3tched://plugins/demo/state").is_err());
        for value in [json!({"mode":"first"}), json!({"mode":"second"})] {
            std::fs::write(plugin.join("value.json"), value.to_string()).unwrap();
            assert_eq!(server.read("3tched://plugins/demo/state").unwrap(), value);
        }
        std::fs::remove_file(plugin.join("value.json")).unwrap();
        assert!(server.read("3tched://plugins/demo/state").is_err());
    }

    #[tokio::test]
    async fn mounted_delivery_reads_the_namespace_and_writes_its_call_file() {
        let (directory, namespace, _) = crate::mounted::tests::fixture();
        let server = IdentityMcp::from_mount(namespace, Surface::Full).unwrap();
        let value = server.read("3tched://plugins/demo/state").unwrap();
        assert_eq!(value, json!({"mode": "mounted", "nested": {"n": 7}}));
        assert!(server.read("3tched://plugins/demo/state/secret").is_err());
        let refused = server
            .dispatch("demo", "get_status", json!({"wrong": 1}))
            .await;
        assert_eq!(refused.is_error, Some(true));
        assert!(
            text(&refused).contains("\"stage\": \"validate\""),
            "{}",
            text(&refused)
        );
        let call = directory
            .path()
            .join("plugins/demo/operations/get_status/call");
        assert_eq!(std::fs::read_to_string(&call).unwrap(), "");
        let submitted = server
            .dispatch("demo", "get_status", json!({"event": "x"}))
            .await;
        assert_eq!(submitted.is_error, Some(true));
        let written = std::fs::read_to_string(&call).unwrap();
        assert!(written.contains("\"event\""), "{written}");
        std::fs::write(directory.path().join(".3tchedfs.json"), b"{}").unwrap();
        assert!(server.read("3tched://plugins/demo/state").is_err());
    }
}
