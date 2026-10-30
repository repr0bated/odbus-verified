//! Validate the self-contained filesystem contract and all source references.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn visit(value: &Value, visitor: &mut impl FnMut(&str, &Value) -> Result<()>) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                visitor(key, value)?;
                visit(value, visitor)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                visit(value, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/threetched-fs/docs/typed-filesystem.schema.json".to_owned());
    let schema: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    jsonschema::draft202012::meta::validate(&schema)
        .map_err(|error| anyhow::anyhow!("invalid JSON Schema 2020-12: {error}"))?;
    let validator = jsonschema::Validator::new(&schema)?;
    validator.validate(&schema).map_err(|error| {
        anyhow::anyhow!("contract does not match its structural schema: {error}")
    })?;

    let types = schema["types"].as_object().context("types")?;
    let names: BTreeSet<_> = types.keys().map(String::as_str).collect();
    let type_names = regex::Regex::new("[A-Za-z_][A-Za-z_0-9]*")?;
    let check_type = |name: &str| -> Result<()> {
        for token in type_names.find_iter(name).map(|m| m.as_str()) {
            ensure!(
                names.contains(token)
                    || names.contains(format!("{token}<T>").as_str())
                    || ["Vec", "Option", "String", "bool", "u64", "usize"].contains(&token),
                "undefined type {token} in {name}"
            );
        }
        Ok(())
    };
    let classes = schema["bindings"]["classes"]
        .as_object()
        .context("classes")?;
    let invariants: BTreeSet<_> = schema["invariants"]["required"]
        .as_array()
        .context("invariants")?
        .iter()
        .filter_map(|value| value["id"].as_str())
        .collect();
    let mut node_ids = BTreeSet::new();
    for node in schema["nodes"].as_array().context("nodes")? {
        ensure!(node_ids.insert(node["id"].as_str()), "duplicate node ID");
        ensure!(
            classes.contains_key(node["class"].as_str().context("class")?),
            "node class has no generated trait"
        );
        for param in node["params"].as_array().context("params")? {
            check_type(param["type"].as_str().context("param type")?)?;
        }
        check_type(
            node["backing"]["payload_type"]
                .as_str()
                .context("payload")?,
        )?;
    }
    let mut method_ids = BTreeSet::new();
    for method in schema["methods"].as_array().context("methods")? {
        ensure!(
            method_ids.insert(method["id"].as_str()),
            "duplicate method ID"
        );
        check_type(method["returns"].as_str().context("return")?)?;
        check_type(method["errors"].as_str().context("errors")?)?;
        for param in method["params"].as_array().context("method params")? {
            check_type(param["type"].as_str().context("method param type")?)?;
        }
        let class = &classes[method["receiver"].as_str().context("receiver")?];
        let declaration = class["operations"]
            .as_array()
            .context("operations")?
            .iter()
            .find(|op| op["name"] == method["name"])
            .context("method absent from generator class")?;
        ensure!(
            declaration["args"] == method["params"]
                && declaration["return"] == method["returns"]
                && declaration["receiver"] == method["borrow"],
            "method and generator signature disagree: {}",
            method["id"]
        );
        ensure!(
            class["routes"]
                .as_array()
                .context("routes")?
                .contains(&method["route"]),
            "method route has no generated implementation"
        );
        for stage in ["pre", "post"] {
            for id in method["required_steps"][stage]
                .as_array()
                .context("steps")?
            {
                ensure!(
                    invariants.contains(id.as_str().context("step")?),
                    "unknown invariant"
                );
            }
        }
    }

    let citation = regex::Regex::new(r"^(.+):([0-9]+)(?:-([0-9]+))?$")?;
    let mut citations = BTreeSet::new();
    let mut line_counts = BTreeMap::new();
    visit(&schema, &mut |key, value| {
        if key == "$ref" {
            let reference = value.as_str().context("$ref is not a string")?;
            if let Some(pointer) = reference.strip_prefix('#') {
                ensure!(schema.pointer(pointer).is_some(), "unresolved {reference}");
            }
        }
        if ["source_refs", "sources", "evidence", "x-source-refs"].contains(&key)
            && let Some(values) = value.as_array()
        {
            for source in values.iter().filter_map(Value::as_str) {
                let Some(parts) = citation.captures(source) else {
                    continue;
                };
                let file = parts[1].to_owned();
                if !Path::new(&file).is_file() {
                    continue;
                }
                let count = *line_counts.entry(file.clone()).or_insert_with(|| {
                    std::fs::read_to_string(&file)
                        .map(|s| s.lines().count())
                        .unwrap_or(0)
                });
                let start: usize = parts[2].parse()?;
                let end: usize = parts.get(3).map_or(Ok(start), |m| m.as_str().parse())?;
                ensure!(
                    start > 0 && start <= end && end <= count,
                    "out-of-bounds source {source}"
                );
                citations.insert(source.to_owned());
            }
        }
        Ok(())
    })?;
    for source in schema["versioning"]["source_files"]
        .as_array()
        .context("source hashes")?
    {
        let path = source["path"].as_str().context("source path")?;
        let actual = format!("{:x}", Sha256::digest(std::fs::read(path)?));
        ensure!(
            source["sha256"] == actual,
            "source changed since schema assembly: {path}"
        );
    }
    println!(
        "JSON Schema 2020-12 + self-contract valid; {} types, {} nodes, {} methods, {} source citations; source digests match",
        types.len(),
        node_ids.len(),
        method_ids.len(),
        citations.len()
    );
    Ok(())
}
