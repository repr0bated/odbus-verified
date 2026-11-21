//! Tiers, toolsets and the 50-tool cap (Requirements 5 and 6.2–6.4).

use crate::catalog::{Catalog, TIER_HOT, TIER_MEMORY};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub const FORMAT: &str = "op-mcp-door/toolsets@1";
/// Cursor's per-server tool limit.
pub const CAP: usize = 50;
/// Meta-tools listed on `/mcp/full`.
pub const FULL_META: &[&str] = &["list_toolsets", "load_toolset"];
/// The five meta-tools of `/mcp/loader`.
pub const LOADER_META: &[&str] = &[
    "list_toolsets",
    "search_tools",
    "describe_tool",
    "call_tool",
    "load_toolset",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tiers {
    pub memory: Vec<String>,
    pub hot: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolset {
    pub id: String,
    pub title: String,
    pub temperature: String,
    /// Stable `<plugin>.<method>` keys.
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolsetFile {
    pub format: String,
    #[serde(default)]
    pub catalog_view_id: Option<String>,
    pub cap: usize,
    pub tiers: Tiers,
    /// Tier names absent from the catalog (Requirement 5.4).
    #[serde(default)]
    pub missing: Vec<String>,
    pub toolsets: Vec<Toolset>,
}

fn plugin_sets() -> Vec<(&'static str, &'static str, &'static [&'static str])> {
    vec![
        ("memory_store", "Memory plugin CRUD", &["memory"]),
        (
            "knowledge",
            "Cozo, Qdrant, embeddings, models, Snowball",
            &[
                "cozo",
                "qdrant",
                "embedding_model",
                "model_registry",
                "snowball",
            ],
        ),
        ("storage", "Btrfs, Incus, OCI", &["btrfs", "incus", "oci"]),
        ("lxd", "LXD", &["lxd"]),
        ("fuse", "FUSE plugin", &["fuse"]),
        (
            "network",
            "Routing, tunnels, VPN, OVS",
            &[
                "net",
                "rtnetlink",
                "openflow",
                "openflow_obfuscation",
                "ovsdb_bridge",
                "rovs_commands",
                "wireguard",
                "wg_opdbus",
                "wgcf",
                "xray",
                "dnsresolver",
                "privacy_routes",
                "routing_decision",
                "proxy_server",
                "tunnel",
            ],
        ),
        (
            "chat",
            "Chat routing and projects",
            &[
                "chat_router",
                "chat_response",
                "ctl_plane_chatbot",
                "project",
                "persona",
            ],
        ),
        (
            "system",
            "Host system",
            &[
                "login1",
                "freedesktop",
                "service",
                "procfs",
                "hardware",
                "packagekit",
                "users",
                "cron",
                "fail2ban",
                "software",
                "full_system",
                "pcidecl",
            ],
        ),
        (
            "identity",
            "Identity, keys, elevation",
            &[
                "identity_sled",
                "human_principal",
                "keyring",
                "keypair",
                "sess_decl",
                "elevation",
            ],
        ),
    ]
}

/// The single lazy toolset holding every `cognitive` operation that is
/// neither memory (tier 1) nor hot (tier 2). Owner decision 2026-11-19:
/// cognitive is grouped into ONE set; memory is never grouped.
pub const COGNITIVE_SET: &str = "cognitive";
/// Temperature of a set that is projected only on request (FUSE
/// `toolsets/<id>/` in the identity's mount, grant-gated).
pub const LAZY: &str = "lazy";
/// The only toolset allowed to contain tier-1 memory tools.
pub const MEMORY_SET: &str = "memory";

const SCHEMA_PLUGINS: &[&str] = &[
    "schema_tag_registry",
    "schema_renderer",
    "oscal_subid_registry",
];
const NOTEBOOK_CORE: &[&str] = &[
    "notebook_list",
    "notebook_get",
    "notebook_query",
    "notebook_query_start",
    "notebook_query_status",
    "source_add",
    "source_list_drive",
    "source_get_content",
    "cross_notebook_query",
    "get_health",
];

/// Generate the toolsets file from a catalog (Requirement 5.2).
pub fn generate(catalog: &Catalog) -> ToolsetFile {
    let present =
        |plugin: &str, method: &str| catalog.by_key(&format!("{plugin}.{method}")).is_some();
    let resolve = |names: &[&str], missing: &mut Vec<String>| -> Vec<String> {
        names
            .iter()
            .filter_map(|name| match catalog.by_name(name) {
                Some(tool) if !tool.class.never_default() => Some(tool.key()),
                _ => {
                    missing.push((*name).to_owned());
                    None
                }
            })
            .collect()
    };
    let mut missing = Vec::new();
    let tiers = Tiers {
        memory: resolve(TIER_MEMORY, &mut missing),
        hot: resolve(TIER_HOT, &mut missing),
    };
    let mut sets: Vec<Toolset> = Vec::new();
    let mut push = |id: &str, title: &str, temperature: &str, tools: Vec<String>| {
        if !tools.is_empty() {
            sets.push(Toolset {
                id: id.into(),
                title: title.into(),
                temperature: temperature.into(),
                tools,
            });
        }
    };
    let pick = |plugin: &str, names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter(|m| present(plugin, m))
            .map(|m| format!("{plugin}.{m}"))
            .collect()
    };
    let whole = |plugins: &[&str]| -> Vec<String> {
        catalog
            .tools
            .iter()
            .filter(|t| plugins.contains(&t.plugin.as_str()))
            .map(|t| t.key())
            .collect()
    };
    push(
        "memory",
        "Cognitive memory (tier 1)",
        "hot",
        tiers.memory.clone(),
    );
    // Schema plugins only; the cognitive schema methods are hot
    // (`schema_read`) or in the single `cognitive` set.
    push(
        "schema",
        "Schema server plugins (sealed blobs)",
        "warm",
        whole(SCHEMA_PLUGINS),
    );
    let core_nb = pick("notebooklm", NOTEBOOK_CORE);
    let admin_nb: Vec<String> = whole(&["notebooklm"])
        .into_iter()
        .filter(|k| !core_nb.contains(k))
        .collect();
    push("notebooklm_core", "NotebookLM core", "warm", core_nb);
    push(
        "notebooklm_admin",
        "NotebookLM administration",
        "cold",
        admin_nb,
    );
    for (id, title, plugins) in plugin_sets() {
        push(id, title, "cold", whole(plugins));
    }
    let tiered: BTreeSet<&String> = tiers.memory.iter().chain(&tiers.hot).collect();
    let memory_names: BTreeSet<&str> = TIER_MEMORY.iter().copied().collect();
    push(
        COGNITIVE_SET,
        "Cognitive (everything but memory and the hot tools), lazy-loaded",
        LAZY,
        catalog
            .tools
            .iter()
            .filter(|t| t.plugin == "cognitive")
            .filter(|t| !memory_names.contains(t.method.as_str()))
            .map(|t| t.key())
            .filter(|k| !tiered.contains(k))
            .collect(),
    );
    for plugin in catalog.plugins() {
        // `cognitive` is covered by tier 1 + tier 2 + the one `cognitive`
        // set; a whole-plugin set would group memory with everything else.
        if plugin == "cognitive" {
            continue;
        }
        push(
            &format!("plugin:{plugin}"),
            &format!("Every {plugin} operation"),
            "cold",
            whole(&[plugin]),
        );
    }
    ToolsetFile {
        format: FORMAT.into(),
        catalog_view_id: catalog.view_id.clone(),
        cap: CAP,
        tiers,
        missing,
        toolsets: sets,
    }
}

/// Toolsets resolved against the live catalog.
#[derive(Debug, Clone)]
pub struct Toolsets {
    pub file: ToolsetFile,
    index: HashMap<String, usize>,
}

impl Toolsets {
    /// Use the file when configured and readable, else generate (Req 5.3).
    pub fn load_or_generate(path: &Path, catalog: &Catalog) -> (Self, Option<String>) {
        match read_file(path) {
            Ok(file) => (Self::new(file, catalog), None),
            Err(error) => (
                Self::new(generate(catalog), catalog),
                Some(format!("{error:#}")),
            ),
        }
    }

    /// Drop members the catalog does not have.
    pub fn new(mut file: ToolsetFile, catalog: &Catalog) -> Self {
        let keep = |keys: &mut Vec<String>| keys.retain(|k| catalog.by_key(k).is_some());
        keep(&mut file.tiers.memory);
        keep(&mut file.tiers.hot);
        file.tiers
            .memory
            .retain(|k| !catalog.by_key(k).is_some_and(|t| t.class.never_default()));
        file.tiers
            .hot
            .retain(|k| !catalog.by_key(k).is_some_and(|t| t.class.never_default()));
        // Memory is never grouped: tier-1 tools live only in tier 1 and the
        // `memory` set, whatever a hand-edited file says.
        let memory: BTreeSet<String> = file
            .tiers
            .memory
            .iter()
            .cloned()
            .chain(
                TIER_MEMORY
                    .iter()
                    .filter_map(|n| catalog.by_name(n).map(|t| t.key())),
            )
            .collect();
        for set in &mut file.toolsets {
            keep(&mut set.tools);
            if set.id != MEMORY_SET {
                set.tools.retain(|k| !memory.contains(k));
            }
        }
        file.toolsets.retain(|s| !s.tools.is_empty());
        let index = file
            .toolsets
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.clone(), i))
            .collect();
        Self { file, index }
    }

    pub fn get(&self, id: &str) -> Option<&Toolset> {
        self.index.get(id).map(|&i| &self.file.toolsets[i])
    }

    /// Tier 1 then tier 2, deduplicated, in priority order.
    pub fn core_keys(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.file
            .tiers
            .memory
            .iter()
            .chain(&self.file.tiers.hot)
            .filter(|k| seen.insert((*k).clone()))
            .cloned()
            .collect()
    }
}

pub fn read_file(path: &Path) -> Result<ToolsetFile> {
    let value = crate::catalog::read_json(path)?;
    let file: ToolsetFile = serde_json::from_value(value).context("toolsets file shape")?;
    if file.format != FORMAT {
        bail!("unsupported toolsets format {}", file.format);
    }
    Ok(file)
}

pub fn write_file(path: &Path, file: &ToolsetFile) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(file)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Union of path (`a+b`), query (`a,b`) and header (`a,b`) selections.
/// `full` and `core` are implicit and ignored.
pub fn parse_selection(
    path: Option<&str>,
    query: Option<&str>,
    header: Option<&str>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let parts = path
        .into_iter()
        .flat_map(|p| p.split('+'))
        .chain(query.into_iter().flat_map(|q| q.split(',')))
        .chain(header.into_iter().flat_map(|h| h.split(',')));
    for part in parts {
        let id = part.trim();
        if !id.is_empty() && id != "full" && id != "core" && !out.iter().any(|o| o == id) {
            out.push(id.to_owned());
        }
    }
    out
}

/// Refuse anything over the cap with both numbers (Requirement 6.3).
pub fn ensure_within_cap(count: usize) -> std::result::Result<(), String> {
    if count > CAP {
        Err(format!("selection lists {count} tools; the cap is {CAP}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::fixture::write_op;
    use serde_json::json;

    fn args() -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    fn full_catalog() -> (tempfile::TempDir, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for m in TIER_MEMORY {
            write_op(
                root,
                "cognitive",
                m,
                m.contains("store") || m.contains("delete"),
                args(),
                "cognitive.read",
            );
        }
        for m in TIER_HOT
            .iter()
            .filter(|m| !matches!(**m, "notebook_query" | "search_own_events"))
        {
            write_op(
                root,
                "cognitive",
                m,
                *m == "code_index",
                args(),
                "cognitive.read",
            );
        }
        write_op(
            root,
            "snowball",
            "search_own_events",
            false,
            args(),
            "cap.obs.snowball.events.self.search@v1",
        );
        // Not hot any more: lands in the lazy `cognitive` set.
        write_op(
            root,
            "cognitive",
            "oscal_subids",
            false,
            args(),
            "cognitive.read",
        );
        write_op(
            root,
            "notebooklm",
            "notebook_query",
            false,
            args(),
            "notebooklm.invoke",
        );
        write_op(
            root,
            "notebooklm",
            "notebook_delete",
            true,
            args(),
            "notebooklm.invoke",
        );
        write_op(
            root,
            "cognitive",
            "agent_rust_pro_advise",
            false,
            args(),
            "cognitive.agent.rust_pro",
        );
        write_op(
            root,
            "cognitive",
            "agent_rust_pro_exec",
            true,
            json!({"type":"object","properties":{"command":{"type":"string"}}}),
            "cognitive.agent.rust_pro",
        );
        write_op(
            root,
            "cognitive",
            "agent_bash_pro_review",
            false,
            args(),
            "cognitive.agent.bash_pro",
        );
        write_op(root, "memory", "get_entry", false, args(), "memory.read");
        write_op(root, "btrfs", "list", false, args(), "btrfs.read");
        let catalog = Catalog::scan(root).unwrap();
        (dir, catalog)
    }

    #[test]
    fn core_is_nineteen_on_a_full_catalog() {
        let (_dir, catalog) = full_catalog();
        let sets = Toolsets::new(generate(&catalog), &catalog);
        assert_eq!(sets.core_keys().len(), 17);
        assert_eq!(sets.core_keys().len() + FULL_META.len(), 19);
        assert!(sets.file.missing.is_empty());
    }

    #[test]
    fn hot_ten_is_the_owner_list_with_knowledge_and_coding() {
        assert_eq!(TIER_HOT.len(), 10);
        for name in [
            "code_search",
            "code_context",
            "code_index",
            "search_own_events",
        ] {
            assert!(TIER_HOT.contains(&name), "{name} must be hot");
        }
        assert!(TIER_HOT.iter().all(|n| !TIER_MEMORY.contains(n)));
        let (_dir, catalog) = full_catalog();
        let file = generate(&catalog);
        assert_eq!(file.tiers.hot.len(), 10);
        assert!(
            file.tiers
                .hot
                .contains(&"snowball.search_own_events".to_owned())
        );
        assert!(
            file.tiers
                .hot
                .contains(&"notebooklm.notebook_query".to_owned())
        );
    }

    #[test]
    fn cognitive_is_one_lazy_set_and_memory_is_never_grouped() {
        let (_dir, catalog) = full_catalog();
        let file = generate(&catalog);
        let sets = Toolsets::new(file, &catalog);
        let cognitive = sets.get(COGNITIVE_SET).unwrap();
        assert_eq!(cognitive.temperature, LAZY);
        assert_eq!(
            cognitive.tools,
            vec![
                "cognitive.agent_bash_pro_review",
                "cognitive.agent_rust_pro_advise",
                "cognitive.agent_rust_pro_exec",
                "cognitive.oscal_subids",
            ]
        );
        // Exactly one cognitive grouping: no persona, coding, reasoning or
        // whole-plugin cognitive sets.
        for id in ["coding", "reasoning", "plugin:cognitive", "agents:rust_pro"] {
            assert!(sets.get(id).is_none(), "{id}");
        }
        let memory = sets.get(MEMORY_SET).unwrap();
        assert_eq!(memory.tools.len(), TIER_MEMORY.len());
        for set in sets.file.toolsets.iter().filter(|s| s.id != MEMORY_SET) {
            for key in &memory.tools {
                assert!(!set.tools.contains(key), "{} groups {key}", set.id);
            }
        }
        // Hot tools are tier 2, not members of the lazy set.
        for key in &sets.file.tiers.hot {
            assert!(!cognitive.tools.contains(key), "{key}");
        }
    }

    #[test]
    fn a_hand_edited_file_cannot_group_memory() {
        let (_dir, catalog) = full_catalog();
        let mut file = generate(&catalog);
        let index = file
            .toolsets
            .iter()
            .position(|s| s.id == COGNITIVE_SET)
            .unwrap();
        file.toolsets[index]
            .tools
            .push("cognitive.memory_store".into());
        let sets = Toolsets::new(file, &catalog);
        assert!(
            !sets
                .get(COGNITIVE_SET)
                .unwrap()
                .tools
                .contains(&"cognitive.memory_store".to_owned())
        );
    }

    #[test]
    fn domain_and_plugin_sets_are_generated() {
        let (_dir, catalog) = full_catalog();
        let file = generate(&catalog);
        let sets = Toolsets::new(file, &catalog);
        assert_eq!(
            sets.get("memory_store").unwrap().tools,
            vec!["memory.get_entry"]
        );
        assert_eq!(
            sets.get("notebooklm_core").unwrap().tools,
            vec!["notebooklm.notebook_query"]
        );
        assert_eq!(
            sets.get("notebooklm_admin").unwrap().tools,
            vec!["notebooklm.notebook_delete"]
        );
        assert!(sets.get("plugin:btrfs").is_some());
        // Every tool is in a tier or at least one set.
        for tool in &catalog.tools {
            assert!(
                sets.core_keys().contains(&tool.key())
                    || sets
                        .file
                        .toolsets
                        .iter()
                        .any(|s| s.tools.contains(&tool.key())),
                "{}",
                tool.key()
            );
        }
    }

    #[test]
    fn missing_tier_tools_are_reported_and_shell_never_enters_core() {
        let dir = tempfile::tempdir().unwrap();
        write_op(
            dir.path(),
            "cognitive",
            "memory_recall",
            false,
            args(),
            "cognitive.read",
        );
        // A hot name that classifies as shell must not enter core.
        write_op(
            dir.path(),
            "cognitive",
            "specialist",
            true,
            json!({"type":"object","properties":{"command":{"type":"string"}}}),
            "cognitive.read",
        );
        let catalog = Catalog::scan(dir.path()).unwrap();
        let file = generate(&catalog);
        assert_eq!(file.tiers.memory, vec!["cognitive.memory_recall"]);
        assert!(file.missing.contains(&"memory_store".to_owned()));
        assert!(file.missing.contains(&"specialist".to_owned()));
        let sets = Toolsets::new(file, &catalog);
        assert!(
            !sets
                .core_keys()
                .contains(&"cognitive.specialist".to_owned())
        );
    }

    #[test]
    fn file_round_trip_and_fallback() {
        let (dir, catalog) = full_catalog();
        let path = dir.path().join("toolsets.json");
        let (_, warning) = Toolsets::load_or_generate(&path, &catalog);
        assert!(warning.is_some(), "absent file falls back to generation");
        let mut file = generate(&catalog);
        file.toolsets[0].tools.push("ghost.method".into());
        write_file(&path, &file).unwrap();
        let (sets, warning) = Toolsets::load_or_generate(&path, &catalog);
        assert!(warning.is_none());
        assert!(
            !sets.file.toolsets[0]
                .tools
                .contains(&"ghost.method".to_owned())
        );
        std::fs::write(&path, r#"{"format":"other"}"#).unwrap();
        assert!(read_file(&path).is_err());
    }

    #[test]
    fn selection_parsing_and_cap() {
        assert_eq!(
            parse_selection(
                Some("full+cognitive+schema"),
                Some("schema, cognitive"),
                Some("notebooklm_core")
            ),
            vec!["cognitive", "schema", "notebooklm_core"]
        );
        assert!(parse_selection(None, None, None).is_empty());
        assert!(ensure_within_cap(50).is_ok());
        let error = ensure_within_cap(51).unwrap_err();
        assert!(error.contains("51") && error.contains("50"));
    }
}
