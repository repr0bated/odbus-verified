use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Runtime capture of `/usr/bin/zeroclaw` schema and command surface.
#[derive(Debug, Clone)]
pub struct ZeroclawSurface {
    binary_path: PathBuf,
    schema_bytes: Vec<u8>,
    schema_json: Value,
    schema_sha256: String,
    version: Option<String>,
    commands: BTreeMap<String, CommandSurface>,
}

#[derive(Debug, Clone)]
struct CommandSurface {
    usage: String,
    command_help: Option<String>,
}

impl ZeroclawSurface {
    pub fn capture(binary: impl AsRef<Path>) -> Result<Self> {
        let binary_path = binary.as_ref().to_path_buf();
        let schema_bytes = run_capture(&binary_path, &["config", "schema"])?.into_bytes();
        let schema_json: Value =
            serde_json::from_slice(&schema_bytes).context("zeroclaw config schema is not JSON")?;
        let mut hasher = Sha256::new();
        hasher.update(&schema_bytes);
        let schema_sha256 = hex::encode(hasher.finalize());

        let help = run_capture(&binary_path, &["--help"])?;
        let commands = parse_top_level_commands(&help);
        if commands.is_empty() {
            bail!("zeroclaw --help listed no commands");
        }

        let mut command_surfaces = BTreeMap::new();
        for (name, usage) in commands {
            let command_help = match run_capture(&binary_path, &[&name, "--help"]) {
                Ok(text) => Some(text),
                Err(_) => None,
            };
            command_surfaces.insert(
                name,
                CommandSurface {
                    usage,
                    command_help,
                },
            );
        }

        let version = match run_capture(&binary_path, &["--version"]) {
            Ok(text) => Some(text.trim().to_string()),
            Err(_) => None,
        };

        Ok(Self {
            binary_path,
            schema_bytes,
            schema_json,
            schema_sha256,
            version,
            commands: command_surfaces,
        })
    }

    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }

    pub fn schema_bytes(&self) -> &[u8] {
        &self.schema_bytes
    }

    pub fn schema_json(&self) -> &Value {
        &self.schema_json
    }

    pub fn schema_sha256(&self) -> &str {
        &self.schema_sha256
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.commands.keys().map(String::as_str)
    }

    pub fn input_schema(&self, command: &str) -> Result<Value> {
        let surface = self
            .commands
            .get(command)
            .with_context(|| format!("unknown zeroclaw command '{command}'"))?;
        if command == "config" {
            return Ok(self.schema_json.clone());
        }
        if let Some(help) = &surface.command_help {
            let properties = parse_option_flags(help);
            if !properties.is_empty() {
                return Ok(json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": properties,
                }));
            }
        }
        Ok(json!({
            "type": "object",
            "additionalProperties": true,
        }))
    }

    pub fn output_schema(&self, command: &str) -> Result<Value> {
        self.commands
            .get(command)
            .with_context(|| format!("unknown zeroclaw command '{command}'"))?;
        Ok(output_envelope_schema())
    }

    pub fn metadata(&self, command: &str) -> Result<Value> {
        let surface = self
            .commands
            .get(command)
            .with_context(|| format!("unknown zeroclaw command '{command}'"))?;
        Ok(json!({
            "name": command,
            "usage": surface.usage,
        }))
    }

    /// Binary words for one config section. A section whose name is a command
    /// returns that command's subcommands. A section that exists only in the
    /// config schema returns the `config` subcommands that address a path.
    pub fn lookup_section_methods(&self, section: &str) -> Vec<String> {
        if let Some(surface) = self.commands.get(section) {
            return command_words(surface.command_help.as_deref().unwrap_or(""));
        }
        if !self.schema_has_section(section) {
            return Vec::new();
        }
        let Some(config) = self.commands.get("config") else {
            return Vec::new();
        };
        command_words(config.command_help.as_deref().unwrap_or(""))
            .into_iter()
            .filter(|word| CONFIG_PATH_COMMANDS.contains(&word.as_str()))
            .collect()
    }

    /// One method per lookup element. Each method's argv is the binary command
    /// that element names. `--config-dir` stays on the invocation, outside the array.
    pub fn explode_section_methods(&self, section: &str) -> Vec<SectionMethod> {
        let command = if self.commands.contains_key(section) {
            section
        } else if self.schema_has_section(section) {
            "config"
        } else {
            return Vec::new();
        };
        self.lookup_section_methods(section)
            .into_iter()
            .map(|name| {
                let mut argv = vec![command.to_string(), name.clone()];
                if command == "config" {
                    argv.push(section.to_string());
                }
                SectionMethod { name, argv }
            })
            .collect()
    }

    fn schema_has_section(&self, section: &str) -> bool {
        self.schema_json
            .get("properties")
            .and_then(|properties| properties.get(section))
            .is_some()
    }
}

/// One exploded method. `argv` is what follows the `zeroclaw` binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionMethod {
    pub name: String,
    pub argv: Vec<String>,
}

const CONFIG_PATH_COMMANDS: &[&str] = &["get", "set", "init", "list", "patch"];

fn run_capture(binary: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {} {:?}", binary.display(), args))?;
    if !output.status.success() {
        bail!(
            "{} {:?} exited with {}: {}",
            binary.display(),
            args,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn command_words(help: &str) -> Vec<String> {
    parse_top_level_commands(help)
        .into_keys()
        .filter(|name| name != "help")
        .collect()
}

fn parse_top_level_commands(help: &str) -> BTreeMap<String, String> {
    let mut commands = BTreeMap::new();
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
        let trimmed = line.trim();
        let Some((name, rest)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        if name == "Options:" || name == "Usage:" {
            break;
        }
        commands.insert(name.to_string(), rest.trim().to_string());
    }
    commands
}

fn parse_option_flags(help: &str) -> BTreeMap<String, Value> {
    let mut properties = BTreeMap::new();
    let mut in_options = false;
    for line in help.lines() {
        if line.trim() == "Options:" {
            in_options = true;
            continue;
        }
        if !in_options {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("-h") && trimmed.contains("help") {
            continue;
        }
        let Some(flag) = long_flag_name(trimmed) else {
            continue;
        };
        let key = flag.trim_start_matches("--").replace('-', "_");
        let schema = if trimmed.contains("<") {
            json!({ "type": "string" })
        } else {
            json!({ "type": "boolean" })
        };
        properties.insert(key, schema);
    }
    properties
}

fn long_flag_name(line: &str) -> Option<&str> {
    for token in line.split_whitespace() {
        if token.starts_with("--") {
            return Some(token);
        }
    }
    None
}

fn output_envelope_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "stdout": { "type": "string" },
            "stderr": { "type": "string" },
            "exit": { "type": "integer" }
        },
        "required": ["stdout", "stderr", "exit"]
    })
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[cfg(test)]
pub fn test_fixture_binary(dir: &Path) -> PathBuf {
    write_fixture_script(dir)
}

#[cfg(test)]
fn write_fixture_script(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("zeroclaw-fixture.sh");
    let script = r#"#!/bin/sh
case "$1" in
--version)
  echo "zeroclaw test-fixture 0.0.1"
  exit 0
  ;;
--help)
  if [ -z "$2" ]; then
    cat <<'EOF'
Usage: zeroclaw [OPTIONS] <COMMAND>

Commands:
  self-test     Run diagnostic self-tests
  config        Manage ZeroClaw configuration
EOF
    exit 0
  fi
  case "$2" in
  self-test)
    cat <<'EOF'
Run diagnostic self-tests

Usage: zeroclaw self-test [OPTIONS]

Options:
  -v, --verbose  More output
EOF
    exit 0
    ;;
  config)
    cat <<'EOF'
Manage ZeroClaw configuration

Usage: zeroclaw config <SUBCOMMAND>
EOF
    exit 0
    ;;
  esac
  exit 1
  ;;
self-test)
  if [ "$2" = "--help" ]; then
    cat <<'EOF'
Run diagnostic self-tests

Usage: zeroclaw self-test [OPTIONS]

Options:
  -v, --verbose  More output
EOF
    exit 0
  fi
  echo "ok"
  exit 0
  ;;
config)
  if [ "$2" = "schema" ]; then
    printf '%s' '{"type":"object","title":"fixture-config"}'
    exit 0
  fi
  exit 1
  ;;
esac
exit 1
"#;
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeroclaw_surface_preserves_command_words_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let binary = test_fixture_binary(dir.path());
        let surface = ZeroclawSurface::capture(&binary).unwrap();
        let names: Vec<_> = surface.commands().collect();
        assert!(names.contains(&"self-test"));
        assert!(names.contains(&"config"));
        assert_eq!(
            surface.schema_json(),
            &json!({"type":"object","title":"fixture-config"})
        );
        let metadata = surface.metadata("self-test").unwrap();
        assert_eq!(metadata["name"], "self-test");
        assert_eq!(metadata["usage"], "Run diagnostic self-tests");
        assert!(metadata.get("side_effect").is_none());
        assert_eq!(surface.version(), Some("zeroclaw test-fixture 0.0.1"));
        assert!(!surface.schema_sha256().is_empty());
    }

    #[test]
    fn zeroclaw_surface_input_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let binary = test_fixture_binary(dir.path());
        let surface = ZeroclawSurface::capture(&binary).unwrap();
        assert_eq!(
            surface.input_schema("config").unwrap(),
            surface.schema_json().clone()
        );
        let self_test = surface.input_schema("self-test").unwrap();
        assert_eq!(self_test["properties"]["verbose"]["type"], "boolean");
        assert!(surface
            .output_schema("status")
            .unwrap_err()
            .to_string()
            .contains("unknown zeroclaw command 'status'"));
    }

    #[test]
    fn lookup_returns_an_array_and_explode_makes_one_method_each() {
        let dir = tempfile::tempdir().unwrap();
        let binary = section_fixture_binary(dir.path());
        let surface = ZeroclawSurface::capture(&binary).unwrap();

        let providers = surface.lookup_section_methods("providers");
        assert_eq!(providers, vec!["create".to_string(), "list".to_string()]);
        assert_eq!(
            surface.explode_section_methods("providers"),
            vec![
                SectionMethod {
                    name: "create".to_string(),
                    argv: vec!["providers".to_string(), "create".to_string()],
                },
                SectionMethod {
                    name: "list".to_string(),
                    argv: vec!["providers".to_string(), "list".to_string()],
                },
            ]
        );

        let routes = surface.lookup_section_methods("model_routes");
        assert_eq!(
            routes,
            vec!["get".to_string(), "patch".to_string(), "set".to_string()]
        );
        let exploded = surface.explode_section_methods("model_routes");
        assert_eq!(exploded.len(), 3);
        assert_eq!(
            exploded[0].argv,
            vec![
                "config".to_string(),
                "get".to_string(),
                "model_routes".to_string()
            ]
        );
        assert!(surface.lookup_section_methods("skills").is_empty());
    }
}

#[cfg(test)]
fn section_fixture_binary(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("zeroclaw-sections.sh");
    let script = r#"#!/bin/sh
case "$1" in
--version)
  echo "zeroclaw test-fixture 0.0.1"
  exit 0
  ;;
--help)
  cat <<'EOF'
Usage: zeroclaw [OPTIONS] <COMMAND>

Commands:
  providers     List supported AI providers
  config        Manage ZeroClaw configuration
EOF
  exit 0
  ;;
providers)
  cat <<'EOF'
Commands:
  list    List provider aliases
  create  Create a new provider alias
  help    Print this message
EOF
  exit 0
  ;;
config)
  if [ "$2" = "schema" ]; then
    printf '%s' '{"type":"object","title":"fixture-config","properties":{"model_routes":{"type":"array"},"providers":{"type":"object"}}}'
    exit 0
  fi
  cat <<'EOF'
Commands:
  schema    Dump the schema
  get       Get a config property value
  set       Set a config property
  patch     Apply a JSON Patch
  help      Print this message
EOF
  exit 0
  ;;
esac
exit 1
"#;
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}
