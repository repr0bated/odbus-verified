//! External CLI binary introspection via recursive `--help` walks.
//!
//! Discover flags/commands from an upstream binary (e.g. ZeroClaw) — never from
//! a schema we already authored. Tuned for clap-style help (`Commands:` /
//! `Arguments:` / `Options:`).
//!
//! Local: spawn `binary … --help`.
//! Remote: `--ssh user@host` wraps each help call in `ssh host -- binary … --help`.

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct CliFlag {
    pub name: String,
    pub short: Option<String>,
    pub value_name: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliNode {
    pub path: Vec<String>,
    pub usage: String,
    pub about: String,
    pub commands: Vec<String>,
    pub flags: Vec<CliFlag>,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BinarySurface {
    pub binary: String,
    pub version: Option<String>,
    pub ssh: Option<String>,
    pub nodes: Vec<CliNode>,
    pub element_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BinaryIntrospectOpts {
    pub max_depth: usize,
    pub ssh: Option<String>,
}

impl Default for BinaryIntrospectOpts {
    fn default() -> Self {
        Self {
            max_depth: 2,
            ssh: None,
        }
    }
}

pub fn introspect_binary(binary: &Path, opts: &BinaryIntrospectOpts) -> Result<BinarySurface> {
    if opts.ssh.is_none() && !binary.exists() {
        bail!("binary not found: {}", binary.display());
    }
    let version = run_version(binary, opts.ssh.as_deref());
    let mut nodes = Vec::new();
    let mut visited = HashSet::new();
    walk(
        binary,
        &[],
        opts.max_depth,
        opts.ssh.as_deref(),
        &mut nodes,
        &mut visited,
    )?;

    let mut element_paths = BTreeSet::new();
    for node in &nodes {
        if node.path.is_empty() {
            element_paths.insert("cmd".to_string());
        } else {
            element_paths.insert(format!("cmd.{}", node.path.join(".")));
        }
        for c in &node.commands {
            let mut p = node.path.clone();
            p.push(c.clone());
            element_paths.insert(format!("cmd.{}", p.join(".")));
        }
        for f in &node.flags {
            let scope = if node.path.is_empty() {
                "root".to_string()
            } else {
                node.path.join(".")
            };
            let raw = f.name.trim_start_matches('-');
            let fname = raw.replace('-', "_");
            element_paths.insert(format!("flag.{scope}.{fname}"));
        }
        for a in &node.arguments {
            let scope = if node.path.is_empty() {
                "root".to_string()
            } else {
                node.path.join(".")
            };
            element_paths.insert(format!("arg.{scope}.{}", a.replace('-', "_")));
        }
    }

    Ok(BinarySurface {
        binary: binary.display().to_string(),
        version,
        ssh: opts.ssh.clone(),
        nodes,
        element_paths: element_paths.into_iter().collect(),
    })
}

pub fn surface_as_instance_json(surface: &BinarySurface) -> Result<String> {
    // Flat path → marker only. Do NOT embed the full surface tree here —
    // walking nested help text would invent thousands of bogus element paths.
    // Full tree lives in CoverageInputs.binary_surface_json / --surface-out.
    let mut map = serde_json::Map::new();
    for path in &surface.element_paths {
        // Scalar true — nested objects would invent fake `.present` child paths.
        map.insert(path.clone(), Value::Bool(true));
    }
    Ok(serde_json::to_string_pretty(&Value::Object(map))?)
}

pub fn surface_to_json(surface: &BinarySurface) -> Result<String> {
    Ok(serde_json::to_string_pretty(surface)?)
}

fn walk(
    binary: &Path,
    path: &[String],
    depth_left: usize,
    ssh: Option<&str>,
    out: &mut Vec<CliNode>,
    visited: &mut HashSet<Vec<String>>,
) -> Result<()> {
    if !visited.insert(path.to_vec()) {
        return Ok(());
    }
    let help = run_help(binary, path, ssh)?;
    let node = parse_help(path, &help);
    let children = node.commands.clone();
    out.push(node);

    if depth_left == 0 {
        return Ok(());
    }
    for child in children {
        if matches!(child.as_str(), "help" | "completions" | "locales") {
            continue;
        }
        let mut next = path.to_vec();
        next.push(child);
        walk(binary, &next, depth_left - 1, ssh, out, visited)?;
    }
    Ok(())
}

fn run_help(binary: &Path, path: &[String], ssh: Option<&str>) -> Result<String> {
    let output = if let Some(host) = ssh {
        let mut parts = vec![shell_escape(&binary.display().to_string())];
        for p in path {
            parts.push(shell_escape(p));
        }
        parts.push("--help".into());
        let remote = parts.join(" ");
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                host,
                &remote,
            ])
            .output()
            .with_context(|| format!("ssh {host} -- help {:?}", path))?
    } else {
        let mut cmd = Command::new(binary);
        for p in path {
            cmd.arg(p);
        }
        cmd.arg("--help");
        cmd.output()
            .with_context(|| format!("spawn {} {:?} --help", binary.display(), path))?
    };
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if text.trim().is_empty() {
        bail!(
            "empty help from {} {:?} via ssh={:?} (status={})",
            binary.display(),
            path,
            ssh,
            output.status
        );
    }
    Ok(text)
}

fn run_version(binary: &Path, ssh: Option<&str>) -> Option<String> {
    let output = if let Some(host) = ssh {
        let remote = format!("{} --version", shell_escape(&binary.display().to_string()));
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                host,
                &remote,
            ])
            .output()
            .ok()?
    } else {
        Command::new(binary).arg("--version").output().ok()?
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|s| s.trim().to_string())
}

fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+@:=,".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Parse CLI `--help` text (clap-style or Go `flag` / Antigravity root help).
pub fn parse_help(path: &[String], help: &str) -> CliNode {
    parse_clap_help(path, help)
}

pub fn parse_clap_help(path: &[String], help: &str) -> CliNode {
    let about = help
        .lines()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && !l.starts_with("Usage:")
                && !l.starts_with("Usage of ")
                && *l != "Available subcommands:"
        })
        .unwrap_or("")
        .to_string();

    let usage = help
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("Usage:") || t.starts_with("Usage of ")
        })
        .map(|l| l.trim().to_string())
        .unwrap_or_default();

    let mut commands = section_names(help, &["Commands:", "Available subcommands:"]);
    // Go-style root help: subcommands listed under "Available subcommands:" with
    // two-column layout (already handled by section_names). Dedup aliases later.
    commands.sort();
    commands.dedup();

    let arguments = section_names(help, &["Arguments:", "Args:"]);
    let mut flags = parse_options(help);
    // Go `flag` package dumps flags at top level without an "Options:" header.
    if flags.is_empty() {
        flags = parse_loose_flags(help);
    }

    CliNode {
        path: path.to_vec(),
        usage,
        about,
        commands,
        flags,
        arguments,
    }
}

fn section_names(help: &str, headers: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_section = false;
    let name_re = cmd_name_re();

    for line in help.lines() {
        let trimmed = line.trim();
        if headers.contains(&trimmed) {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.is_empty() {
                continue;
            }
            if !line.starts_with(' ') && !line.starts_with('\t') && trimmed.ends_with(':') {
                break;
            }
            if let Some(caps) = name_re.captures(line) {
                let name = caps.get(1).unwrap().as_str();
                if name != "help" {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

fn parse_options(help: &str) -> Vec<CliFlag> {
    let mut flags = Vec::new();
    let mut in_section = false;
    let mut current: Option<CliFlag> = None;

    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed == "Options:" || trimmed == "Global Options:" || trimmed == "Flags:" {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if !line.starts_with(' ')
            && !line.starts_with('\t')
            && !trimmed.is_empty()
            && trimmed.ends_with(':')
            && !trimmed.starts_with('-')
        {
            if let Some(f) = current.take() {
                flags.push(f);
            }
            break;
        }

        // Flag definition lines start with optional short + long.
        if let Some(flag) = parse_flag_line(line) {
            if let Some(f) = current.take() {
                flags.push(f);
            }
            current = Some(flag);
        } else if let Some(ref mut f) = current {
            if !trimmed.is_empty() {
                if !f.description.is_empty() {
                    f.description.push(' ');
                }
                f.description.push_str(trimmed);
            }
        }
    }
    if let Some(f) = current {
        flags.push(f);
    }
    flags
}

/// Go / Antigravity root help: flags appear as indented `--name` lines with no
/// `Options:` header (before `Available subcommands:`).
fn parse_loose_flags(help: &str) -> Vec<CliFlag> {
    let mut flags = Vec::new();
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed == "Available subcommands:" || trimmed.starts_with("Commands:") {
            break;
        }
        if let Some(flag) = parse_flag_line(line) {
            flags.push(flag);
        }
    }
    flags
}

fn parse_flag_line(line: &str) -> Option<CliFlag> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('-') {
        return None;
    }
    // clap: -v, --verbose   desc
    // clap: --config-dir <CONFIG_DIR>
    // Go/Antigravity: --add-dir                       Add a directory...
    // Go short: -c                              Short alias...
    let caps = flag_line_re().captures(trimmed)?;
    let short = caps
        .name("short1")
        .or_else(|| caps.name("short2"))
        .map(|m| format!("-{}", m.as_str()));
    let long = caps
        .name("long1")
        .or_else(|| caps.name("long2"))
        .map(|m| format!("--{}", m.as_str()));
    let value_name = caps.name("val").map(|m| m.as_str().to_string());
    let desc = caps
        .name("desc")
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let name = long.or_else(|| short.clone())?;
    Some(CliFlag {
        name,
        short,
        value_name,
        description: desc,
    })
}

fn cmd_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s{2,8}([a-zA-Z][\w-]*)\s{2,}").unwrap())
}

fn flag_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            ^
            (?:
                # clap: -v, --verbose
                -(?P<short1>[a-zA-Z0-9]),\s*--(?P<long1>[\w-]+)
              | # clap / go long: --verbose
                --(?P<long2>[\w-]+)
              | # go short alone: -c  (must not be start of --long; long2 already tried)
                -(?P<short2>[a-zA-Z0-9])
            )
            (?:\s+<(?P<val>[^>]+)>)?
            (?:\s{2,}(?P<desc>\S.*))?
            \s*$
            ",
        )
        .unwrap()
    })
}

/// True when target names a binary path (or `binary:PATH`), not JSON/source.
pub fn resolve_binary_target(target: &str) -> Option<PathBuf> {
    let path = if let Some(rest) = target.strip_prefix("binary:") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(target)
    };
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = name.to_ascii_lowercase();
    // Data dumps / packs — never treat as executable binaries.
    if lower.ends_with(".json")
        || lower.ends_with(".rs")
        || lower.ends_with(".md")
        || lower.ends_with(".xml")
        || lower.contains("repomix")
    {
        return None;
    }
    // Explicit binary: prefix always wins (remote path need not exist locally).
    if target.starts_with("binary:") {
        return Some(path);
    }
    // Absolute path: only if it looks executable / has no dump extension.
    if path.is_absolute() {
        if path.is_file() {
            // Existing non-executable text dump? leave to resolve_file.
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = path.metadata() {
                if meta.permissions().mode() & 0o111 == 0 {
                    return None;
                }
            }
        }
        return Some(path);
    }
    if path.is_file() {
        return Some(path);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"Manage provider model catalogs

Usage: zeroclaw models [OPTIONS] <COMMAND>

Commands:
  refresh  Refresh and cache provider models
  list     List cached models for a provider
  set      Set the default model in config
  status   Show current model configuration
  help     Print this message or the help of the given subcommand(s)

Options:
      --config-dir <CONFIG_DIR>
      --log-level <LOG_LEVEL>    Lowest severity recorded
  -v, --verbose                  Surface recorded logs
  -h, --help                     Print help
"#;

    #[test]
    fn parses_clap_commands_and_flags() {
        let node = parse_clap_help(&["models".into()], SAMPLE);
        assert!(node.commands.contains(&"refresh".into()));
        assert!(node.commands.contains(&"list".into()));
        assert!(!node.commands.contains(&"help".into()));
        let names: Vec<_> = node.flags.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"--verbose"), "flags={names:?}");
        assert!(names.contains(&"--config-dir"), "flags={names:?}");
        assert!(names.contains(&"--help"), "flags={names:?}");
    }

    const AGY_ROOT: &str = r#"Usage of antigravity:
  --add-dir                       Add a directory to the workspace (repeatable) (default [])
  --agent                         Agent for the current CLI session
  -c                              Short alias for --continue
  --continue                      Continue the most recent conversation
  --model                         Model for the current CLI session
  --print                         Run a single prompt non-interactively

Available subcommands:
  agent           List available agents
  models          List available models
  plugin          Manage plugins (install, uninstall, list, enable, disable)
  help            Show help for subcommands
"#;

    #[test]
    fn parses_antigravity_go_style_root_help() {
        let node = parse_help(&[], AGY_ROOT);
        assert!(
            node.commands.contains(&"plugin".into()),
            "cmds={:?}",
            node.commands
        );
        assert!(node.commands.contains(&"models".into()));
        assert!(!node.commands.contains(&"help".into()));
        let names: Vec<_> = node.flags.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"--model"), "flags={names:?}");
        assert!(names.contains(&"--add-dir"), "flags={names:?}");
        assert!(
            names.contains(&"-c") || names.iter().any(|n| n.ends_with("continue")),
            "flags={names:?}"
        );
    }
}
