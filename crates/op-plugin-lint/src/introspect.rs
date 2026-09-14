//! Resolve `--introspect <repomix|file|binary>` into coverage inputs.
//!
//! Prefer **Repomix XML** (universal: any upstream repo root → `repomix` →
//! struct/enum element paths). CLI help-walk is secondary. Authoring our own
//! sealed schema and then "introspecting" it is circular — plugin ids that
//! fetch ui-model are kept only as an optional drift check, not discovery.

use crate::audit::CoverageInputs;
use crate::binary::{
    introspect_binary, resolve_binary_target, surface_as_instance_json, surface_to_json,
    BinaryIntrospectOpts,
};
use crate::repomix::{introspect_repomix, looks_like_repomix, surface_to_coverage_json};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct IntrospectOpts {
    pub max_depth: usize,
    pub ssh: Option<String>,
}

impl Default for IntrospectOpts {
    fn default() -> Self {
        Self {
            max_depth: 2,
            ssh: None,
        }
    }
}

pub fn resolve_introspect_target(target: &str) -> Result<CoverageInputs> {
    resolve_introspect_target_with(target, None, &IntrospectOpts::default())
}

pub fn resolve_introspect_target_for_plugin(
    target: &str,
    plugin_rs: Option<&Path>,
) -> Result<CoverageInputs> {
    resolve_introspect_target_with(target, plugin_rs, &IntrospectOpts::default())
}

pub fn resolve_introspect_target_with(
    target: &str,
    plugin_rs: Option<&Path>,
    opts: &IntrospectOpts,
) -> Result<CoverageInputs> {
    let mut cov = resolve_target(target, opts)?;
    if let Some(plugin) = plugin_rs {
        cov.extra_rust_sources = discover_type_sources(plugin);
    }
    Ok(cov)
}

fn resolve_target(target: &str, opts: &IntrospectOpts) -> Result<CoverageInputs> {
    // 1) Existing data file (JSON dump, Repomix XML) before binary heuristics.
    let path = Path::new(target);
    if path.is_file()
        && (path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "json" | "xml" | "md" | "rs"))
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_ascii_lowercase().contains("repomix")))
    {
        return resolve_file(path);
    }

    // 2) CLI binary (local path, binary:PATH, or absolute path + --ssh)
    if let Some(bin) = resolve_binary_target(target) {
        let bopts = BinaryIntrospectOpts {
            max_depth: opts.max_depth,
            ssh: opts.ssh.clone(),
        };
        eprintln!(
            "introspect binary: {} (depth={}, ssh={:?})",
            bin.display(),
            bopts.max_depth,
            bopts.ssh
        );
        let surface = introspect_binary(&bin, &bopts)?;
        eprintln!(
            "  discovered: {} nodes, {} element paths{}",
            surface.nodes.len(),
            surface.element_paths.len(),
            surface
                .version
                .as_ref()
                .map(|v| format!(", version={v}"))
                .unwrap_or_default()
        );
        return Ok(CoverageInputs {
            instance_json: Some(surface_as_instance_json(&surface)?),
            binary_surface_json: Some(surface_to_json(&surface)?),
            ..Default::default()
        });
    }

    if path.is_file() {
        return resolve_file(path);
    }

    match target {
        "gcloud" => resolve_gcloud(),
        // Drift-only: fetch sealed schema we already published (not discovery).
        name if looks_like_plugin_id(name) => {
            eprintln!(
                "warn: --introspect {name} fetches our sealed schema (drift check), \
                 not an external SDK — prefer a binary path for discovery"
            );
            resolve_plugin_object(name)
        }
        other => bail!(
            "unknown --introspect target `{other}`\n\
             examples:\n\
               --introspect /fast/zeroclaw/bin/zeroclaw --ssh root@192.168.1.1\n\
               --introspect binary:/usr/bin/gemini\n\
               --introspect ./external-sdk-dump.json\n\
               --introspect /path/to/repomix-output.xml"
        ),
    }
}

fn discover_type_sources(plugin_rs: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Some(dir) = plugin_rs.parent() else {
        return out;
    };
    let common = dir.join("common");
    if common.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&common) {
            let mut paths: Vec<_> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("rs"))
                .filter(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| n != "mod.rs")
                })
                .collect();
            paths.sort();
            for p in paths {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push(text);
                }
            }
        }
    }
    out
}

fn looks_like_plugin_id(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.contains('.')
        && !name.contains('/')
}

fn resolve_file(path: &Path) -> Result<CoverageInputs> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read introspect file {}", path.display()))?;

    if looks_like_repomix(path, &text)
        || (path.extension().and_then(|e| e.to_str()) == Some("xml")
            && text.contains("<file path="))
    {
        eprintln!("introspect repomix: {}", path.display());
        let surface = introspect_repomix(path, &text)?;
        eprintln!(
            "  discovered: {} files {:?}, {} element paths {:?}",
            surface.files_seen,
            surface.files_by_kind,
            surface.element_paths.len(),
            surface.by_kind,
        );
        let surface_json = surface_to_coverage_json(&surface)?;
        let mut map = serde_json::Map::new();
        for p in &surface.element_paths {
            map.insert(p.clone(), Value::Bool(true));
        }
        return Ok(CoverageInputs {
            instance_json: Some(serde_json::to_string_pretty(&Value::Object(map))?),
            binary_surface_json: Some(surface_json),
            ..Default::default()
        });
    }

    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("parse JSON {}", path.display()))?;

    // BinarySurface dump from a prior run
    if value.get("element_paths").is_some() && value.get("nodes").is_some() {
        let paths = value
            .get("element_paths")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut map = serde_json::Map::new();
        for p in paths {
            if let Some(s) = p.as_str() {
                map.insert(s.to_string(), Value::Bool(true));
            }
        }
        return Ok(CoverageInputs {
            instance_json: Some(serde_json::to_string_pretty(&Value::Object(map))?),
            binary_surface_json: Some(text),
            ..Default::default()
        });
    }

    if value.pointer("/schema/fields").is_some() || value.get("fields").is_some() {
        Ok(CoverageInputs {
            sealed_schema_json: Some(text),
            ..Default::default()
        })
    } else {
        Ok(CoverageInputs {
            instance_json: Some(text),
            ..Default::default()
        })
    }
}

fn resolve_plugin_object(plugin_id: &str) -> Result<CoverageInputs> {
    if let Ok(base) = std::env::var("OP_PLUGIN_LINT_SCHEMA_URL") {
        let url = if base.contains("{plugin}") {
            base.replace("{plugin}", plugin_id)
        } else if base.ends_with('/') {
            format!("{base}{plugin_id}")
        } else {
            format!("{base}/{plugin_id}")
        };
        if let Ok(text) = http_get(&url) {
            return Ok(CoverageInputs {
                sealed_schema_json: Some(text),
                ..Default::default()
            });
        }
    }

    let candidates = [
        format!("http://10.0.0.2:8080/api/ui-model/plugin-schema/{plugin_id}"),
        format!("http://127.0.0.1:8080/api/ui-model/plugin-schema/{plugin_id}"),
        format!("http://127.0.0.1:18080/api/ui-model/plugin-schema/{plugin_id}"),
    ];
    for url in &candidates {
        if let Ok(text) = http_get(url) {
            if text.contains("\"fields\"") {
                return Ok(CoverageInputs {
                    sealed_schema_json: Some(text),
                    ..Default::default()
                });
            }
        }
    }

    bail!(
        "could not resolve `{plugin_id}` — for discovery use a binary path, e.g.\n\
         --introspect /fast/zeroclaw/bin/zeroclaw --ssh root@192.168.1.1"
    )
}

fn resolve_gcloud() -> Result<CoverageInputs> {
    let output = Command::new("gcloud")
        .args(["--help"])
        .output()
        .context("spawn gcloud --help (is gcloud installed?)")?;
    if !output.status.success() {
        bail!(
            "gcloud --help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut groups = Vec::new();
    let mut in_groups = false;
    for line in stdout.lines() {
        let t = line.trim();
        if t == "GROUPS" || t.starts_with("GROUPS ") {
            in_groups = true;
            continue;
        }
        if in_groups {
            if t.is_empty() || t.starts_with("COMMANDS") {
                break;
            }
            let name = t.split_whitespace().next().unwrap_or("");
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                groups.push(name.to_string());
            }
        }
    }
    let mut map = serde_json::Map::new();
    for g in &groups {
        map.insert(format!("cmd.{g}"), json!({ "present": true }));
    }
    Ok(CoverageInputs {
        instance_json: Some(serde_json::to_string_pretty(&Value::Object(map))?),
        ..Default::default()
    })
}

fn http_get(url: &str) -> Result<String> {
    let output = Command::new("curl")
        .args(["-fsS", "--max-time", "8", url])
        .output()
        .with_context(|| format!("curl {url}"))?;
    if !output.status.success() {
        bail!("curl failed for {url}");
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn file_with_fields_is_sealed_schema() {
        let dir = std::env::temp_dir();
        let path = dir.join("op-plugin-lint-sealed-test2.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{"plugin":"demo","schema":{{"fields":{{"status":{{"field_type":"string"}}}}}}}}"#
        )
        .unwrap();
        let cov = resolve_introspect_target(path.to_str().unwrap()).unwrap();
        assert!(cov.sealed_schema_json.is_some());
        let _ = std::fs::remove_file(path);
    }
}
