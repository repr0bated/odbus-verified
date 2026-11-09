//! Tool_Registry derivation (spec task 6.2): a pure function of both
//! views' discovery outputs, the held-capability result, the exclusion
//! set and the sticky-denied set. Task 6.1: no authoritative mounted
//! caller-capability reader exists yet (task 19.2); until then the held
//! result is explicitly `Undetermined` and no tools are offered. No code
//! path reads `/dev/shm/opdbus/capability-grants.json`.

use crate::types::{
    BlockerRecord, CatalogPin, HeldCapabilityResult, RegistryEntry, RegistrySnapshot, SideEffect,
    ToolKey, UnavailableReason, ViewKind,
};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredMethod {
    pub plugin_id: String,
    pub method: String,
    pub args: Option<serde_json::Value>,
    pub returns: Option<serde_json::Value>,
    pub required_capability: Option<String>,
    /// Raw side-effect string from discovery; anything not `read`
    /// is a mutation (D-15).
    pub side_effect: String,
    pub idempotent: bool,
    pub subid: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewDiscovery {
    pub view_kind: ViewKind,
    /// `None` means the view is absent.
    pub pin: Option<CatalogPin>,
    pub methods: Vec<DiscoveredMethod>,
}

#[derive(Debug, Clone)]
pub struct DeriveInput {
    pub views: Vec<ViewDiscovery>,
    pub held: HeldCapabilityResult,
    pub exclusions: HashSet<String>,
    pub sticky_denied: HashSet<ToolKey>,
}

#[derive(Debug, Clone)]
pub struct DeriveOutput {
    pub snapshot: RegistrySnapshot,
    pub blockers: Vec<BlockerRecord>,
    /// Sanitized tool name -> (plugin, method) lookup. Tool definitions
    /// carry no execute function; names are lookup keys only.
    pub tool_names: BTreeMap<String, ToolKey>,
}

fn is_wrong_view(view: ViewKind, plugin_id: &str) -> bool {
    // The threetched view exposes only the threetched surface plugin;
    // that plugin never belongs in the 3tched view (D-35). The plugin
    // name follows from the view name, not the exclusion config.
    match view {
        ViewKind::Threetched => plugin_id != "threetched",
        ViewKind::ThreeTched => plugin_id == "threetched",
    }
}

pub fn derive(input: &DeriveInput, blocker_stamp: (&str, &str)) -> DeriveOutput {
    let mut entries: Vec<RegistryEntry> = Vec::new();
    let mut blockers = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let held_set: Option<&Vec<String>> = match &input.held {
        HeldCapabilityResult::Held { capabilities } => Some(capabilities),
        _ => None,
    };
    for view in &input.views {
        for method in &view.methods {
            if input.exclusions.contains(&method.plugin_id) {
                continue;
            }
            if is_wrong_view(view.view_kind, &method.plugin_id) {
                blockers.push(BlockerRecord {
                    id: format!("BR-{}-{}", blocker_stamp.0, blockers.len() + 1),
                    at: blocker_stamp.0.to_string(),
                    branch_display_name: blocker_stamp.1.to_string(),
                    operation: format!("{}.{}", method.plugin_id, method.method),
                    caller: "unidentified".to_string(),
                    detail: "naming defect: plugin exposed in wrong view".to_string(),
                });
                continue;
            }
            if !seen.insert((method.plugin_id.clone(), method.method.clone())) {
                continue;
            }
            let tool_key = ToolKey::new(format!("{}::{}", method.plugin_id, method.method));
            let (offered, reason) = if view.pin.is_none() {
                (false, Some(UnavailableReason::ViewAbsent))
            } else if method.args.is_none() {
                (false, Some(UnavailableReason::Schema))
            } else if input.sticky_denied.contains(&tool_key) {
                (false, Some(UnavailableReason::StickyDenied))
            } else {
                match (&method.required_capability, held_set) {
                    (Some(cap), Some(held)) if held.contains(cap) => (true, None),
                    (Some(_), Some(_)) => (false, Some(UnavailableReason::Absent)),
                    _ => (false, Some(UnavailableReason::Undetermined)),
                }
            };
            entries.push(RegistryEntry {
                tool_key,
                plugin_id: method.plugin_id.clone(),
                method: method.method.clone(),
                args: method.args.clone().unwrap_or(serde_json::Value::Null),
                returns: method.returns.clone(),
                required_capability: method.required_capability.clone(),
                side_effect: if method.side_effect == "read" {
                    SideEffect::Read
                } else {
                    SideEffect::Mutation
                },
                idempotent: method.idempotent,
                subid: method.subid.clone(),
                view_kind: view.view_kind,
                offered,
                unavailable_reason: reason,
            });
        }
    }
    let pin = input
        .views
        .iter()
        .find_map(|v| v.pin.clone())
        .unwrap_or(CatalogPin {
            view_id: String::new(),
            source_catalog_hash: String::new(),
            source_catalog_generation: 0,
        });
    let tool_names = tool_names(&entries);
    DeriveOutput {
        snapshot: RegistrySnapshot { pin, entries },
        blockers,
        tool_names,
    }
}

/// Sanitized `${plugin}__${method}` names in `[A-Za-z0-9_-]`, with
/// `_${n}` suffixes on collision.
pub fn tool_names(entries: &[RegistryEntry]) -> BTreeMap<String, ToolKey> {
    let mut out = BTreeMap::new();
    for entry in entries {
        let base: String = format!("{}__{}", entry.plugin_id, entry.method)
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let mut name = base.clone();
        let mut n = 2;
        while out.contains_key(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        out.insert(name, entry.tool_key.clone());
    }
    out
}

/// Per-view re-derivation: replace only that view's entries; the other
/// view's entries are carried over identical.
pub fn rederive_view(
    previous: &RegistrySnapshot,
    view: &ViewDiscovery,
    input: &DeriveInput,
    blocker_stamp: (&str, &str),
) -> DeriveOutput {
    let mut single = input.clone();
    single.views = vec![view.clone()];
    let fresh = derive(&single, blocker_stamp);
    let mut entries: Vec<RegistryEntry> = previous
        .entries
        .iter()
        .filter(|e| e.view_kind != view.view_kind)
        .cloned()
        .collect();
    entries.extend(fresh.snapshot.entries);
    let tool_names = tool_names(&entries);
    DeriveOutput {
        snapshot: RegistrySnapshot {
            pin: fresh.snapshot.pin,
            entries,
        },
        blockers: fresh.blockers,
        tool_names,
    }
}
