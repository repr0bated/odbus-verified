//! One filter for every plugin, applied to a binary schema extract before seal.
//!
//! The installed `threetched` binary stays the schema source. This module does
//! not rewrite that binary, and it does not search-replace its text. It keeps
//! the one named namespace, then turns that namespace's command words into
//! methods on the plugin that already owns it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use op_state_store::{CapabilityDecl, MethodDecl, PluginSchema, SideEffect};
use serde_json::{Map, Value};
use simd_json::OwnedValue;

const NAMESPACE: &str = "model_routes";
/// Path commands from `threetched config --help`. The help text is the array.
const PATH_COMMANDS: &[&str] = &["get", "set", "init", "list", "patch"];

struct CapturedNamespace {
    section: Value,
    commands: Vec<String>,
}

fn captured() -> Option<&'static CapturedNamespace> {
    static CAPTURED: OnceLock<Option<CapturedNamespace>> = OnceLock::new();
    CAPTURED
        .get_or_init(|| match capture() {
            Ok(captured) => Some(captured),
            Err(error) => {
                tracing::warn!(%error, "model_routes namespace was not captured from the binary");
                None
            }
        })
        .as_ref()
}

fn capture() -> Result<CapturedNamespace> {
    let (binary, _) = super::tched_router_paths::capture_paths()?;
    let binary = binary.to_str().context("non-UTF8 tched-router binary path")?;
    let schema_text = run(&[binary, "config", "schema"])?;
    let document: Value = serde_json::from_str(&schema_text)
        .context("threetched config schema is not JSON")?;
    let section = filter_namespace(&document, NAMESPACE)?;
    let help = run(&[binary, "config", "--help"])?;
    let commands = filter_command_words(&help, PATH_COMMANDS);
    if commands.is_empty() {
        bail!("threetched config --help listed no path commands");
    }
    Ok(CapturedNamespace { section, commands })
}

/// Keep one namespace from a binary config schema, including the `$defs` it
/// references. The rest of the monolith stays out. The retained bytes are the
/// binary's own schema nodes.
pub fn filter_namespace(document: &Value, namespace: &str) -> Result<Value> {
    let section = document
        .pointer(&format!("/properties/{namespace}"))
        .with_context(|| format!("binary schema has no namespace '{namespace}'"))?
        .clone();
    let mut defs = Map::new();
    let mut seen = HashSet::new();
    collect_defs(&section, document, &mut defs, &mut seen);
    let mut filtered = section;
    if !defs.is_empty() {
        if let Value::Object(map) = &mut filtered {
            map.insert("$defs".to_string(), Value::Object(defs));
        }
    }
    Ok(filtered)
}

fn collect_defs(
    node: &Value,
    document: &Value,
    defs: &mut Map<String, Value>,
    seen: &mut HashSet<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = reference.strip_prefix("#/$defs/")
                && seen.insert(name.to_string())
                && let Some(def) = document.pointer(&format!("/$defs/{name}"))
            {
                defs.insert(name.to_string(), def.clone());
                collect_defs(def, document, defs, seen);
            }
            for value in map.values() {
                collect_defs(value, document, defs, seen);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_defs(item, document, defs, seen);
            }
        }
        _ => {}
    }
}

/// Command words from a binary help listing, limited to the words that address
/// a config path. `help` is not a method.
pub fn filter_command_words(help: &str, allowed: &[&str]) -> Vec<String> {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let mut words = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line.trim() == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with("  ") {
            break;
        }
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if allowed.contains(name) {
            words.push(name.to_string());
        }
    }
    words
}

/// Declare the filtered namespace on a plugin schema. Missing binary capture
/// leaves the plugin schema unchanged so a host without the binary still loads.
pub fn attach_model_routes(schema: &mut PluginSchema) {
    let Some(captured) = captured() else {
        return;
    };
    let read = "cap.software.3tched-router.routes.read@v1";
    let write = "cap.software.3tched-router.config.write@v1";
    ensure_capability(
        schema,
        read,
        "Read model_routes through the binary namespace.",
    );
    ensure_capability(
        schema,
        write,
        "Change model_routes through the binary namespace.",
    );
    for command in &captured.commands {
        let mutation = matches!(command.as_str(), "set" | "init" | "patch");
        schema.methods.insert(
            command.clone(),
            method_decl(
                command,
                &captured.section,
                if mutation {
                    SideEffect::Mutation
                } else {
                    SideEffect::Read
                },
                command != "patch",
                if mutation { write } else { read },
            ),
        );
    }
}

fn ensure_capability(schema: &mut PluginSchema, id: &str, description: &str) {
    schema.capabilities.entry(id.to_string()).or_insert_with(|| {
        CapabilityDecl {
            id: id.to_string(),
            description: description.to_string(),
        }
    });
}

fn method_decl(
    command: &str,
    section: &Value,
    side_effect: SideEffect,
    idempotent: bool,
    capability: &str,
) -> MethodDecl {
    let category = if matches!(side_effect, SideEffect::Read) {
        "obs"
    } else {
        "mut"
    };
    let args = if command == "set" {
        serde_json::json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "help": { "type": "boolean" },
                "value": section
            }
        })
    } else if command == "patch" {
        serde_json::json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "help": { "type": "boolean" },
                "operations": { "type": "array" }
            }
        })
    } else {
        serde_json::json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "help": { "type": "boolean" }
            }
        })
    };
    MethodDecl {
        name: command.to_string(),
        args: to_simd(&args),
        returns: Some(to_simd(&serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["stdout", "stderr", "exit"],
            "properties": {
                "stdout": { "type": "string" },
                "stderr": { "type": "string" },
                "exit": { "type": "integer" }
            }
        }))),
        side_effect,
        idempotent,
        required_capability: Some(capability.to_string()),
        subid: format!("{category}.service.3tched-router.model-routes.{command}@v1"),
    }
}

fn to_simd(value: &Value) -> OwnedValue {
    let mut bytes = serde_json::to_vec(value).expect("method schema is JSON");
    simd_json::to_owned_value(&mut bytes).expect("method schema is simd JSON")
}

/// Run a namespace method. `None` means the method belongs to another surface.
pub fn dispatch(
    method: &str,
    json_args: &str,
) -> Option<std::result::Result<serde_json::Value, super::common::errors::TchedRouterError>> {
    if !PATH_COMMANDS.contains(&method) {
        return None;
    }
    Some(call_namespace(method, json_args).map_err(|error| {
        super::common::errors::TchedRouterError::RouteUnavailable {
            hint: NAMESPACE.to_string(),
            reason: error.to_string(),
        }
    }))
}

fn call_namespace(method: &str, json_args: &str) -> Result<serde_json::Value> {
    let args: BTreeMap<String, Value> = if json_args.trim().is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(json_args).context("namespace method arguments are not an object")?
    };
    let help = matches!(args.get("help"), Some(Value::Bool(true)));
    let config_dir = super::tched_router_paths::config_dir()?;
    let mut argv = vec![
        "--config-dir".to_string(),
        config_dir.to_str().context("non-UTF8 tched-router config path")?.to_string(),
        "config".to_string(),
        method.to_string(),
    ];
    if help {
        argv.push("--help".to_string());
    } else {
        match method {
            "list" => {
                argv.push("--filter".to_string());
                argv.push(NAMESPACE.to_string());
            }
            "patch" => argv.push("-".to_string()),
            _ => argv.push(NAMESPACE.to_string()),
        }
        if method == "set" {
            let value = args
                .get("value")
                .context("set requires value")?
                .to_string();
            argv.push(value);
        }
        for (key, value) in &args {
            if key == "help" || key == "value" || key == "operations" {
                continue;
            }
            match value {
                Value::Bool(false) | Value::Null => {}
                Value::Bool(true) => argv.push(format!("--{}", key.replace('_', "-"))),
                Value::String(text) => {
                    argv.push(format!("--{}", key.replace('_', "-")));
                    argv.push(text.clone());
                }
                other => {
                    argv.push(format!("--{}", key.replace('_', "-")));
                    argv.push(other.to_string());
                }
            }
        }
    }
    let mut command = Command::new(super::tched_router_paths::binary()?);
    command.args(&argv);
    if method == "patch" && !help {
        let operations = args
            .get("operations")
            .context("patch requires operations")?;
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn().context("failed to start threetched")?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(serde_json::to_string(operations)?.as_bytes())?;
        }
        let output = child.wait_with_output().context("threetched patch failed")?;
        return Ok(envelope(&output));
    }
    let output = command.output().context("failed to run threetched")?;
    Ok(envelope(&output))
}

fn envelope(output: &std::process::Output) -> serde_json::Value {
    serde_json::json!({
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "exit": output.status.code().unwrap_or(1),
    })
}

fn run(argv: &[&str]) -> Result<String> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .with_context(|| format!("failed to run {}", argv[0]))?;
    if !output.status.success() {
        bail!(
            "{} exited with {}: {}",
            argv.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_keeps_one_namespace_and_its_defs() {
        let document = serde_json::json!({
            "type": "object",
            "properties": {
                "model_routes": { "$ref": "#/$defs/Routes" },
                "providers": { "type": "object" }
            },
            "$defs": {
                "Routes": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/Route" }
                },
                "Route": { "type": "object" },
                "Unused": { "type": "string" }
            }
        });
        let filtered = filter_namespace(&document, "model_routes").unwrap();
        assert_eq!(filtered["$ref"], "#/$defs/Routes");
        assert!(filtered["$defs"].get("Routes").is_some());
        assert!(filtered["$defs"].get("Route").is_some());
        assert!(filtered["$defs"].get("Unused").is_none());
        assert!(filtered.get("providers").is_none());
    }

    #[test]
    fn get_help_runs_the_installed_binary() {
        let Ok(binary) = super::super::tched_router_paths::binary() else {
            return;
        };
        if !binary.is_file() {
            return;
        }
        let outcome = dispatch("get", r#"{"help":true}"#).expect("get is a namespace method");
        let outcome = outcome.expect("binary help call");
        let stdout = outcome["stdout"].as_str().unwrap_or("");
        assert!(
            stdout.contains("Usage:"),
            "help stdout was {stdout}"
        );
        assert_eq!(outcome["exit"], 0);
    }

    #[test]
    fn command_filter_returns_the_path_words_only() {
        let help = "Commands:\n  schema    Dump\n  get       Get a value\n  set       Set a value\n  help      Print help\n\nOptions:\n";
        assert_eq!(
            filter_command_words(help, PATH_COMMANDS),
            vec!["get".to_string(), "set".to_string()]
        );
    }
}
