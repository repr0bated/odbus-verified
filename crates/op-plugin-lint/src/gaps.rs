//! Gap analysis: introspected surface paths not represented in the plugin.

use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct IntrospectGaps {
    pub surface_paths_total: usize,
    pub signal_paths_considered: usize,
    pub covered_approx: usize,
    pub missing_from_plugin: usize,
    pub missing_cli_commands: Vec<String>,
    pub missing_config_fields: Vec<String>,
    pub missing_by_group: BTreeMap<String, usize>,
    /// Paths owned by a delegated catalog (e.g. Gemini models on antigravity*).
    pub delegated_paths: Vec<String>,
    /// Full missing paths (capped for report size; see surface-out for catalog).
    pub missing_paths_sample: Vec<String>,
    pub note: String,
}

pub fn gaps_from_surface_json(
    surface_json: &str,
    plugin_field_leaves: &BTreeSet<String>,
    plugin_methods: &BTreeSet<String>,
) -> Option<IntrospectGaps> {
    gaps_from_surface_json_for_plugin(surface_json, plugin_field_leaves, plugin_methods, "")
}

pub fn gaps_from_surface_json_for_plugin(
    surface_json: &str,
    plugin_field_leaves: &BTreeSet<String>,
    plugin_methods: &BTreeSet<String>,
    plugin_name: &str,
) -> Option<IntrospectGaps> {
    let surface: Value = serde_json::from_str(surface_json).ok()?;
    let paths = surface
        .get("element_paths")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let plugin_text_leaves: BTreeSet<String> = plugin_field_leaves
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let method_snakes: BTreeSet<String> = plugin_methods
        .iter()
        .flat_map(|m| {
            let lower = m.to_ascii_lowercase();
            let snake = camel_to_snake(m);
            [lower, snake]
        })
        .collect();

    let delegate_gemini = is_antigravity_plugin(plugin_name);

    let mut covered = 0usize;
    let mut missing = Vec::new();
    let mut delegated = Vec::new();
    let mut by_group: BTreeMap<String, usize> = BTreeMap::new();

    for p in &paths {
        let Some(path) = p.as_str() else { continue };
        if is_noise(path) || !is_signal(path) {
            continue;
        }
        if surface_covered(path, &plugin_text_leaves, &method_snakes) {
            covered += 1;
            continue;
        }
        if delegate_gemini && is_delegated_gemini_path(path) {
            *by_group.entry("delegated_gemini".into()).or_default() += 1;
            delegated.push(path.to_string());
            continue;
        }
        *by_group.entry(group_of(path)).or_default() += 1;
        missing.push(path.to_string());
    }

    let missing_cli: Vec<String> = missing
        .iter()
        .filter(|p| {
            p.as_str() == "cmd"
                || p.starts_with("cmd.")
                || p.starts_with("flag.")
                || p.contains("Commands.")
                || p.contains(".Commands.")
        })
        .cloned()
        .collect();
    let missing_config: Vec<String> = missing
        .iter()
        .filter(|p| p.contains("zeroclaw_config") || p.contains(".config."))
        .cloned()
        .collect();

    const SAMPLE_CAP: usize = 500;
    let sample = if missing.len() > SAMPLE_CAP {
        missing[..SAMPLE_CAP].to_vec()
    } else {
        missing.clone()
    };

    let note = if delegate_gemini && !delegated.is_empty() {
        format!(
            "Introspected findings not represented in the plugin .rs — this is the gap list. \
             {} Gemini-model path(s) classified as delegated (owned by llm_plugin/provider_route, \
             not this plugin). Full catalog is --surface-out.",
            delegated.len()
        )
    } else {
        "Introspected findings not represented in the plugin .rs — this is the gap list. Full catalog is --surface-out.".into()
    };

    Some(IntrospectGaps {
        surface_paths_total: paths.len(),
        signal_paths_considered: covered + missing.len() + delegated.len(),
        covered_approx: covered,
        missing_from_plugin: missing.len(),
        missing_cli_commands: missing_cli,
        missing_config_fields: missing_config,
        missing_by_group: by_group,
        delegated_paths: delegated,
        missing_paths_sample: sample,
        note,
    })
}

fn is_antigravity_plugin(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "antigravity"
        || n == "antigravity_chat"
        || n.ends_with("/antigravity.rs")
        || n.ends_with("/antigravity_chat.rs")
        || n == "antigravity.rs"
        || n == "antigravity_chat.rs"
}

/// Gemini-model surface that antigravity* plugins deliberately do not own.
fn is_delegated_gemini_path(p: &str) -> bool {
    let pl = p.to_ascii_lowercase();
    if pl == "cmd.models" || pl.starts_with("cmd.models.") {
        return true;
    }
    // flags under the models subcommand
    if pl.starts_with("flag.models.") || pl == "flag.models" {
        return true;
    }
    // flag.*.model / flag.root.model (not mode/modeless)
    if pl.starts_with("flag.") {
        let leaf = pl.rsplit('.').next().unwrap_or("");
        if leaf == "model" || leaf.ends_with("_model") || leaf.starts_with("model_") {
            return true;
        }
    }
    if pl.contains("gemini") {
        return true;
    }
    if pl.contains("modelconfig")
        || pl.contains("model_config")
        || pl.contains(".modelconfig.")
        || pl.contains("geminiapiendpoint")
        || pl.contains("geminimodeloptions")
        || pl.contains("modelapi")
        || pl.contains("modeloutput")
        || pl.contains("modeltype")
        || pl.contains("modelendpoint")
    {
        return true;
    }
    // SDK python model classes under *.models.class.Gemini*
    if pl.contains(".models.class.gemini") || pl.contains(".models.gemini") {
        return true;
    }
    false
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn is_noise(p: &str) -> bool {
    let pl = p.to_ascii_lowercase();
    [
        "aardvark",
        "robot_kit",
        "firmware",
        "/tests.",
        ".tests.",
        "cargo.",
        "deny.",
        "clippy",
        "github.workflows",
        "docker-compose",
        "locales",
        "bench",
    ]
    .iter()
    .any(|n| pl.contains(n))
}

fn is_signal(p: &str) -> bool {
    p == "cmd"
        || p.starts_with("cmd.")
        || p.starts_with("flag.")
        || p.starts_with("struct.zeroclaw")
        || p.starts_with("enum.zeroclaw")
        || p.starts_with("struct.zeroclaw_")
        || p.starts_with("toml.")
        || p.starts_with("proto.")
        || p.starts_with("py.")
        || p.contains("zeroclaw_config")
        || p.contains("antigravity")
        || p.contains("Commands.")
}

fn group_of(p: &str) -> String {
    if p == "cmd" || p.starts_with("cmd.") {
        "cli_commands".into()
    } else if p.starts_with("flag.") {
        "cli_flags".into()
    } else if p.contains("Commands.") || p.contains(".Commands.") {
        "cli_commands".into()
    } else if p.contains("zeroclaw_config") {
        "zeroclaw_config".into()
    } else if p.starts_with("py.") {
        "python".into()
    } else if p.starts_with("toml.") {
        "toml".into()
    } else if p.starts_with("enum.") {
        "rust_enum".into()
    } else if p.starts_with("struct.") {
        "rust_struct".into()
    } else {
        "other".into()
    }
}

fn surface_covered(
    path: &str,
    plugin_leaves: &BTreeSet<String>,
    method_snakes: &BTreeSet<String>,
) -> bool {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    let leaf_l = leaf.to_ascii_lowercase();
    let snake = camel_to_snake(leaf);
    if plugin_leaves.contains(&leaf_l) || plugin_leaves.contains(&snake) {
        return true;
    }
    if method_snakes.contains(&leaf_l) || method_snakes.contains(&snake) {
        return true;
    }
    // ModelsCommands.List ↔ list_models
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() >= 2 {
        let variant = parts[parts.len() - 1].to_ascii_lowercase();
        let parent = parts[parts.len() - 2]
            .to_ascii_lowercase()
            .replace("commands", "")
            .replace("command", "");
        let g1 = format!("{variant}_{parent}").trim_matches('_').to_string();
        let g2 = format!("{parent}_{variant}").trim_matches('_').to_string();
        if method_snakes.contains(&g1) || method_snakes.contains(&g2) {
            return true;
        }
    }
    // selected_model covers flag.*.model for antigravity* product preference
    if (leaf_l == "model" || snake == "model") && plugin_leaves.contains("selected_model") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_delegates_gemini_cli_paths() {
        let surface = r#"{
          "element_paths": [
            "cmd.agent",
            "cmd.models",
            "cmd.models.list",
            "flag.root.model",
            "flag.root.mode",
            "flag.root.agent",
            "py.sdk.models.class.GeminiAPIEndpoint"
          ]
        }"#;
        let leaves = BTreeSet::from(["selected_model".into(), "bridge".into()]);
        let methods = BTreeSet::new();
        let gaps =
            gaps_from_surface_json_for_plugin(surface, &leaves, &methods, "antigravity_chat")
                .unwrap();
        assert!(
            gaps.delegated_paths.iter().any(|p| p == "cmd.models"),
            "delegated={:?}",
            gaps.delegated_paths
        );
        assert!(
            !gaps
                .missing_cli_commands
                .iter()
                .any(|p| p.starts_with("cmd.models")),
            "missing_cli={:?}",
            gaps.missing_cli_commands
        );
        // mode is product, not model
        assert!(
            gaps.missing_cli_commands
                .iter()
                .any(|p| p == "flag.root.mode")
                || gaps
                    .missing_paths_sample
                    .iter()
                    .any(|p| p == "flag.root.mode"),
            "mode should remain a product gap; missing={:?} sample={:?}",
            gaps.missing_cli_commands,
            gaps.missing_paths_sample
        );
        // selected_model covers flag.root.model
        assert!(
            !gaps
                .missing_cli_commands
                .iter()
                .any(|p| p == "flag.root.model"),
            "selected_model should cover flag.root.model"
        );
        assert!(gaps.missing_by_group.contains_key("delegated_gemini"));
    }
}
