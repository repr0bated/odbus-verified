//! Tool catalog generated from the mounted operation schemas (Requirement 4).
//!
//! Only `plugins/*/operations/*/metadata.json` is read (plus a `call`
//! existence check). Its `args` and `returns` are the documents mounted as
//! `input.schema.json` and `output.schema.json`, so reading them again would
//! triple the FUSE round trips of a scan. A plugin-level `schema.json` is never
//! parsed for methods: the `threetched` surface publishes a JSON Schema there,
//! not a PluginSchema (audit §2). Plugins are scanned on a few threads because
//! each mounted read is a separate FUSE request.

use crate::canonical::sha256_hex;
use crate::classify::{Class, Facts, classify};
use crate::encode::encode_component;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

/// Bound on any single document the catalog reads.
pub const MAX_DOCUMENT: u64 = 16 * 1024 * 1024;
pub const MAX_TOOL_NAME: usize = 64;

/// Tier 1: the seven memory tools, always hot.
pub const TIER_MEMORY: &[&str] = &[
    "memory_store",
    "memory_recall",
    "memory_retrieve",
    "memory_query",
    "memory_delete",
    "memory_list_namespaces",
    "memory_semantic_query",
];

/// Tier 2: the ten other always-hot tools.
pub const TIER_HOT: &[&str] = &[
    "code_search",
    "code_context",
    "code_index",
    "sequential_thinking",
    "context_manager",
    "specialist",
    "schema_read",
    "oscal_subids",
    "notebook_query",
    "workflow_query",
];

/// Door meta-tools. No generated tool may take one of these names.
pub const META_TOOLS: &[&str] = &[
    "list_toolsets",
    "search_tools",
    "describe_tool",
    "call_tool",
    "load_toolset",
];

/// Plugins searched, in order, for a bare hot-tool name.
const HOT_PLUGIN_ORDER: &[&str] = &["cognitive", "notebooklm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
}

#[derive(Debug, Clone, Deserialize)]
struct RawMeta {
    #[serde(default)]
    args: Value,
    #[serde(default)]
    returns: Option<Value>,
    side_effect: SideEffect,
    idempotent: bool,
    #[serde(default)]
    required_capability: Option<String>,
    subid: String,
}

/// One callable operation, projected as an MCP tool.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub plugin: String,
    pub method: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    /// `metadata.json` exactly as mounted; receipts are checked against it.
    pub metadata: Value,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    pub required_capability: Option<String>,
    pub subid: String,
    pub class: Class,
    pub destructive: bool,
}

impl Tool {
    /// Stable `<plugin>.<method>` key used in toolsets and grants.
    pub fn key(&self) -> String {
        format!("{}.{}", self.plugin, self.method)
    }

    pub fn is_mutation(&self) -> bool {
        self.side_effect == SideEffect::Mutation
    }

    pub fn has_command_arg(&self) -> bool {
        crate::classify::has_command_arg(&arg_names(&self.input_schema))
    }

    /// The MCP `inputSchema`: the sealed input schema, forced to an object,
    /// plus an optional `confirm_side_effects` flag for mutations.
    pub fn mcp_input_schema(&self) -> Value {
        let mut schema = match &self.input_schema {
            Value::Object(_) => self.input_schema.clone(),
            _ => json!({}),
        };
        let object = schema.as_object_mut().expect("object");
        object.entry("type").or_insert_with(|| json!("object"));
        if self.is_mutation() {
            let properties = object.entry("properties").or_insert_with(|| json!({}));
            if let Some(properties) = properties.as_object_mut() {
                properties.insert(
                    "confirm_side_effects".into(),
                    json!({"type":"boolean","description":"Must be true: this tool mutates state."}),
                );
            }
        }
        schema
    }

    /// The MCP tool descriptor. No `outputSchema`: receipts wrap the result,
    /// and clients that validate structured content would reject the
    /// wrapper. The schema is still available through `describe_tool`.
    pub fn descriptor(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.mcp_input_schema(),
            "annotations": {
                "title": self.key(),
                "readOnlyHint": !self.is_mutation(),
                "destructiveHint": self.destructive,
                "idempotentHint": self.idempotent,
                "openWorldHint": false,
            },
        })
    }
}

/// The catalog projected from one mount.
#[derive(Debug, Default)]
pub struct Catalog {
    pub tools: Vec<Tool>,
    by_name: HashMap<String, usize>,
    by_key: HashMap<String, usize>,
    /// Operations that could not be projected, with the reason.
    pub skipped: Vec<String>,
    pub view_id: Option<String>,
}

impl Catalog {
    pub fn by_name(&self, name: &str) -> Option<&Tool> {
        self.by_name.get(name).map(|&i| &self.tools[i])
    }

    pub fn by_key(&self, key: &str) -> Option<&Tool> {
        self.by_key.get(key).map(|&i| &self.tools[i])
    }

    pub fn plugins(&self) -> BTreeSet<&str> {
        self.tools.iter().map(|t| t.plugin.as_str()).collect()
    }

    /// Scan a mount root.
    pub fn scan(root: &Path) -> Result<Self> {
        let view_id = read_json(&root.join("view.json"))
            .ok()
            .and_then(|v| v["view_id"].as_str().map(str::to_owned));
        let plugins_dir = root.join("plugins");
        let entries =
            sorted_dir(&plugins_dir).with_context(|| format!("list {}", plugins_dir.display()))?;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: std::sync::Mutex<Vec<PluginScan>> = std::sync::Mutex::new(Vec::new());
        let workers = SCAN_THREADS.min(entries.len().max(1));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(entry) = entries.get(index) else {
                            break;
                        };
                        let (ops, skipped) = scan_plugin(&plugins_dir, entry);
                        results
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((index, ops, skipped));
                    }
                });
            }
        });
        let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
        results.sort_by_key(|r| r.0);
        let mut ops = Vec::new();
        let mut skipped = Vec::new();
        for (_, o, s) in results {
            ops.extend(o);
            skipped.extend(s);
        }
        let mut catalog = Self::from_operations(ops);
        catalog.skipped.extend(skipped);
        catalog.view_id = view_id;
        Ok(catalog)
    }

    /// Build names and indexes from loaded operations.
    pub fn from_operations(mut ops: Vec<Tool>) -> Self {
        ops.sort_by(|a, b| (&a.plugin, &a.method).cmp(&(&b.plugin, &b.method)));
        // Resolve bare hot names first.
        let mut bare: HashMap<usize, String> = HashMap::new();
        for hot in TIER_MEMORY.iter().chain(TIER_HOT) {
            let found = HOT_PLUGIN_ORDER
                .iter()
                .find_map(|p| ops.iter().position(|t| t.plugin == *p && t.method == *hot))
                .or_else(|| ops.iter().position(|t| t.method == *hot));
            if let Some(index) = found {
                bare.insert(index, (*hot).to_owned());
            }
        }
        let mut used: BTreeSet<String> = bare.values().cloned().collect();
        for name in META_TOOLS {
            used.insert((*name).to_owned());
        }
        for (index, tool) in ops.iter_mut().enumerate() {
            tool.name = match bare.get(&index) {
                Some(name) => name.clone(),
                None => {
                    let mut name = tool_name(&tool.plugin, &tool.method);
                    if used.contains(&name) {
                        name = hashed_name(&name, &tool.key());
                    }
                    used.insert(name.clone());
                    name
                }
            };
        }
        let by_name = ops
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.clone(), i))
            .collect();
        let by_key = ops.iter().enumerate().map(|(i, t)| (t.key(), i)).collect();
        Self {
            tools: ops,
            by_name,
            by_key,
            skipped: Vec::new(),
            view_id: None,
        }
    }
}

/// `<plugin>__<method>` restricted to `[A-Za-z0-9_-]`, ≤ 64 characters.
pub fn tool_name(plugin: &str, method: &str) -> String {
    let raw = format!("{plugin}__{method}");
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.len() > MAX_TOOL_NAME {
        hashed_name(&sanitized, &format!("{plugin}.{method}"))
    } else {
        sanitized
    }
}

/// First 55 characters, `_`, then 8 hex of SHA-256 of the stable key.
fn hashed_name(name: &str, key: &str) -> String {
    let prefix: String = name.chars().take(55).collect();
    format!("{prefix}_{}", &sha256_hex(key.as_bytes())[..8])
}

/// Top-level argument names of an input schema.
pub fn arg_names(schema: &Value) -> BTreeSet<String> {
    schema["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// One plugin's scan: its index in the sorted listing, tools, skip reasons.
type PluginScan = (usize, Vec<Tool>, Vec<String>);

/// Initial read buffer for mounted JSON documents.
const READ_CHUNK: usize = 64 * 1024;

/// Worker threads for a scan; FUSE serves requests concurrently.
const SCAN_THREADS: usize = 8;

/// Every operation of one plugin directory, plus skip reasons.
fn scan_plugin(plugins_dir: &Path, plugin_entry: &str) -> (Vec<Tool>, Vec<String>) {
    let mut ops = Vec::new();
    let mut skipped = Vec::new();
    let Some(plugin) = decode_component(plugin_entry) else {
        skipped.push(format!("{plugin_entry}: undecodable plugin directory"));
        return (ops, skipped);
    };
    let operations = plugins_dir.join(plugin_entry).join("operations");
    let Ok(methods) = sorted_dir(&operations) else {
        return (ops, skipped);
    };
    for method_entry in methods {
        let dir = operations.join(&method_entry);
        match load_operation(&dir, &plugin, &method_entry) {
            Ok(op) => ops.push(op),
            Err(error) => skipped.push(format!("{plugin}.{method_entry}: {error:#}")),
        }
    }
    (ops, skipped)
}

const DESTRUCTIVE: &[&str] = &[
    "delete",
    "remove",
    "kill",
    "purge",
    "destroy",
    "drop",
    "clear",
    "wipe",
    "revoke",
    "reset",
    "truncate",
    "unregister",
    "rm",
];

fn load_operation(dir: &Path, plugin: &str, method_entry: &str) -> Result<Tool> {
    let metadata = read_json(&dir.join("metadata.json"))?;
    let raw: RawMeta = serde_json::from_value(metadata.clone()).context("metadata.json shape")?;
    // The method id is the directory: 3tchedFS names it after the key of
    // `schema.methods`. `metadata.json`'s `name` is the declaration's own
    // (often D-Bus member) name, e.g. `AttachDevice` under `attach_device`.
    let method = decode_component(method_entry).context("undecodable method directory")?;
    if !dir.join("call").exists() {
        bail!("no call file");
    }
    let input_schema = raw.args.clone();
    let output_schema = raw.returns.clone();
    let description = input_schema["description"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_owned();
    let mutation = raw.side_effect == SideEffect::Mutation;
    let names = arg_names(&input_schema);
    let class = classify(&Facts {
        plugin,
        method: &method,
        mutation,
        idempotent: raw.idempotent,
        arg_names: &names,
    });
    let destructive = mutation
        && method
            .split(['_', '-', '.'])
            .any(|t| DESTRUCTIVE.contains(&t));
    Ok(Tool {
        name: String::new(),
        plugin: plugin.to_owned(),
        method,
        description: if description.is_empty() {
            format!("{plugin}.{method_entry}")
        } else {
            description
        },
        input_schema,
        output_schema,
        metadata,
        side_effect: raw.side_effect,
        idempotent: raw.idempotent,
        required_capability: raw.required_capability,
        subid: raw.subid,
        class,
        destructive,
    })
}

fn sorted_dir(dir: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// Bounded JSON read. The buffer starts at 64 KiB: `Take<File>` hides the
/// file size, and without spare capacity `read_to_end` probes with 32-byte
/// reads, each of which is a separate FUSE request on a mounted file.
pub fn read_json(path: &Path) -> Result<Value> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut bytes = Vec::with_capacity(READ_CHUNK);
    file.take(MAX_DOCUMENT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT {
        bail!("{} exceeds {MAX_DOCUMENT} bytes", path.display());
    }
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

/// Inverse of `encode_component`; `None` unless it round-trips exactly.
pub fn decode_component(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    (encode_component(&decoded) == encoded).then_some(decoded)
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;

    /// Write one operation into a mount tree.
    pub fn write_op(
        root: &Path,
        plugin: &str,
        method: &str,
        mutation: bool,
        args: Value,
        cap: &str,
    ) {
        let dir = root
            .join("plugins")
            .join(encode_component(plugin))
            .join("operations")
            .join(encode_component(method));
        std::fs::create_dir_all(&dir).unwrap();
        let meta = json!({
            "name": method, "args": args, "returns": {"type":"object"},
            "side_effect": if mutation {"mutation"} else {"read"},
            "idempotent": !mutation, "required_capability": cap,
            "subid": format!("{}.service.{plugin}.{method}@v1", if mutation {"mut"} else {"obs"}),
        });
        std::fs::write(dir.join("metadata.json"), meta.to_string()).unwrap();
        std::fs::write(dir.join("input.schema.json"), args.to_string()).unwrap();
        std::fs::write(
            dir.join("output.schema.json"),
            json!({"type":"object"}).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("call"), "").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::write_op;
    use super::*;

    fn obj(props: &[&str]) -> Value {
        let mut p = serde_json::Map::new();
        for name in props {
            p.insert((*name).into(), json!({"type":"string"}));
        }
        json!({"type":"object","description":"d","properties":p})
    }

    #[test]
    fn names_are_sanitized_capped_and_hashed() {
        assert_eq!(tool_name("cozo", "run_query"), "cozo__run_query");
        assert_eq!(tool_name("a.b", "c d"), "a_b__c_d");
        let long = tool_name("lxd", &"x".repeat(80));
        assert_eq!(long.len(), 64);
        assert!(
            long.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
        assert_eq!(long, tool_name("lxd", &"x".repeat(80)), "stable");
        assert_ne!(long, tool_name("lxd", &"x".repeat(81)));
    }

    #[test]
    fn decode_round_trips_only_canonical_encodings() {
        assert_eq!(decode_component("a%2Fb").as_deref(), Some("a/b"));
        assert_eq!(
            decode_component("memory_recall").as_deref(),
            Some("memory_recall")
        );
        assert_eq!(
            decode_component("%61"),
            None,
            "non-canonical encoding of 'a'"
        );
        assert_eq!(decode_component("%zz"), None);
    }

    #[test]
    fn scan_projects_operations_and_skips_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("view.json"), json!({"view_id":"v1"}).to_string()).unwrap();
        write_op(
            root,
            "cognitive",
            "memory_recall",
            false,
            obj(&["namespace"]),
            "cognitive.read",
        );
        write_op(
            root,
            "cognitive",
            "memory_store",
            true,
            obj(&["key"]),
            "cognitive.write",
        );
        write_op(
            root,
            "cognitive",
            "agent_bash_pro_exec",
            true,
            obj(&["command"]),
            "cognitive.agent.bash_pro",
        );
        write_op(
            root,
            "wireguard",
            "remove_peer",
            true,
            obj(&["peer"]),
            "cap.network.wireguard.peer.remove@v1",
        );
        // threetched-style plugin schema.json is ignored entirely.
        std::fs::write(
            root.join("plugins/cognitive/schema.json"),
            r#"{"$defs":{},"title":"Config"}"#,
        )
        .unwrap();
        // Broken operation: no metadata.
        std::fs::create_dir_all(root.join("plugins/cognitive/operations/broken")).unwrap();

        let catalog = Catalog::scan(root).unwrap();
        assert_eq!(catalog.view_id.as_deref(), Some("v1"));
        assert_eq!(catalog.tools.len(), 4);
        assert_eq!(catalog.skipped.len(), 1);
        let recall = catalog.by_name("memory_recall").unwrap();
        assert_eq!(recall.description, "d");
        assert_eq!(recall.descriptor()["annotations"]["readOnlyHint"], true);
        assert!(
            recall.mcp_input_schema()["properties"]
                .get("confirm_side_effects")
                .is_none()
        );
        let store = catalog.by_key("cognitive.memory_store").unwrap();
        assert_eq!(store.name, "memory_store");
        assert_eq!(
            store.mcp_input_schema()["properties"]["confirm_side_effects"]["type"],
            "boolean"
        );
        let shell = catalog.by_key("cognitive.agent_bash_pro_exec").unwrap();
        assert_eq!(shell.name, "cognitive__agent_bash_pro_exec");
        assert_eq!(shell.class, Class::Shell);
        let peer = catalog.by_name("wireguard__remove_peer").unwrap();
        assert!(peer.destructive);
        assert_eq!(peer.descriptor()["annotations"]["destructiveHint"], true);
    }

    #[test]
    fn method_id_is_the_directory_not_the_declared_name() {
        // Live login1 shape: `operations/attach_device/metadata.json` declares
        // `"name": "AttachDevice"`; the call path and receipt use the key.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_op(
            root,
            "login1",
            "attach_device",
            true,
            obj(&["seat"]),
            "cap.login1.write",
        );
        let meta_path = root.join("plugins/login1/operations/attach_device/metadata.json");
        let mut meta = read_json(&meta_path).unwrap();
        meta["name"] = json!("AttachDevice");
        std::fs::write(&meta_path, meta.to_string()).unwrap();
        let catalog = Catalog::scan(root).unwrap();
        assert!(catalog.skipped.is_empty(), "{:?}", catalog.skipped);
        let tool = catalog.by_key("login1.attach_device").unwrap();
        assert_eq!(tool.method, "attach_device");
        assert_eq!(tool.name, "login1__attach_device");
    }

    #[test]
    fn hot_names_prefer_cognitive_then_notebooklm() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_op(
            root,
            "notebooklm",
            "notebook_query",
            false,
            obj(&[]),
            "notebooklm.invoke",
        );
        write_op(root, "aaa", "code_search", false, obj(&[]), "x");
        write_op(
            root,
            "cognitive",
            "code_search",
            false,
            obj(&[]),
            "cognitive.read",
        );
        let catalog = Catalog::scan(root).unwrap();
        assert_eq!(catalog.by_name("code_search").unwrap().plugin, "cognitive");
        assert_eq!(catalog.by_name("aaa__code_search").unwrap().plugin, "aaa");
        assert_eq!(
            catalog.by_name("notebook_query").unwrap().plugin,
            "notebooklm"
        );
    }
}
