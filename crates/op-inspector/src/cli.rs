//! Generic CLI Introspection Adapter
//!
//! Generalizes the gcloud introspection pattern into a reusable framework
//! for ANY CLI program (incus, docker, kubectl, helm, etc.).
//!
//! Handles both Go/cobra-style and Python/click-style help output formats,
//! as well as gcloud's ALL-CAPS section headers.
//!
//! # Usage
//!
//! ```rust,no_run
//! use op_inspector::cli::{CliParser, introspect_cli};
//!
//! // Quick introspection
//! let schema = introspect_cli("incus", 3).await?;
//!
//! // Custom help flag (e.g., for programs that use "-h" only)
//! let parser = CliParser::with_help_flag("mytool", "-h");
//! let schema = parser.introspect_full(2).await?;
//! ```

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Root schema for a CLI program's command hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSchema {
    /// The program name (e.g., "incus", "docker", "kubectl")
    pub program: String,
    /// Version string as reported by the program
    pub version: String,
    /// Root command hierarchy
    pub hierarchy: CliCommand,
    /// Introspection statistics
    pub statistics: CliStats,
}

/// A command or command group within the CLI hierarchy (recursive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliCommand {
    /// Command name (e.g., "list", "config", "admin")
    pub name: String,
    /// Full command path (e.g., "incus config edit")
    pub full_path: String,
    /// Human-readable description
    pub description: String,
    /// Whether this node is a group (has subcommands)
    pub is_group: bool,
    /// Available flags / options
    pub flags: Vec<CliFlag>,
    /// Positional arguments
    pub positional_args: Vec<CliArg>,
    /// Subcommands keyed by name
    pub subcommands: HashMap<String, CliCommand>,
}

impl Default for CliCommand {
    fn default() -> Self {
        Self {
            name: "root".to_string(),
            full_path: String::new(),
            description: String::new(),
            is_group: true,
            flags: vec![],
            positional_args: vec![],
            subcommands: HashMap::new(),
        }
    }
}

/// A CLI flag / option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliFlag {
    /// Long flag name including dashes (e.g., "--format")
    pub name: String,
    /// Optional short flag (e.g., "-f")
    pub short_name: Option<String>,
    /// Description text
    pub description: String,
    /// Whether the flag is required
    pub required: bool,
    /// Inferred value type ("string", "integer", "boolean")
    pub value_type: String,
    /// Default value if any
    pub default: Option<String>,
    /// Allowed choices if any
    pub choices: Vec<String>,
}

/// A positional argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliArg {
    /// Argument name
    pub name: String,
    /// Description text
    pub description: String,
    /// Whether the argument is required
    pub required: bool,
}

/// Statistics gathered during introspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliStats {
    pub total_groups: usize,
    pub total_commands: usize,
    pub total_flags: usize,
    pub introspection_time_ms: u128,
}

impl Default for CliStats {
    fn default() -> Self {
        Self {
            total_groups: 0,
            total_commands: 0,
            total_flags: 0,
            introspection_time_ms: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// CliParser
// ---------------------------------------------------------------------------

/// Generic CLI introspection parser.
///
/// Runs a program's help output, parses it to discover command groups,
/// subcommands, flags, and arguments, then builds a [`CliSchema`].
pub struct CliParser {
    /// The program binary name (e.g., "incus")
    program: String,
    /// In-memory cache of help text keyed by command path
    cache: Arc<Mutex<HashMap<String, String>>>,
    /// The flag used to request help (usually "--help")
    help_flag: String,
}

impl CliParser {
    /// Create a new parser for the given program with the default `--help` flag.
    pub fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            cache: Arc::new(Mutex::new(HashMap::new())),
            help_flag: "--help".to_string(),
        }
    }

    /// Create a new parser with a custom help flag (e.g., "-h").
    pub fn with_help_flag(program: &str, help_flag: &str) -> Self {
        Self {
            program: program.to_string(),
            cache: Arc::new(Mutex::new(HashMap::new())),
            help_flag: help_flag.to_string(),
        }
    }

    /// Attempt to retrieve the program version.
    ///
    /// Tries `program --version` first; if that fails or returns empty output
    /// it falls back to `program version`. Returns the first non-empty line.
    pub async fn get_version(&self) -> Result<String> {
        // Try --version first (most common)
        let output = tokio::process::Command::new(&self.program)
            .arg("--version")
            .output()
            .await
            .context(format!("Failed to run {} --version", self.program))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(first_line) = stdout.lines().find(|l| !l.trim().is_empty()) {
            return Ok(first_line.trim().to_string());
        }

        // Fallback: some programs use a "version" subcommand (e.g., docker)
        let output = tokio::process::Command::new(&self.program)
            .arg("version")
            .output()
            .await
            .context(format!("Failed to run {} version", self.program))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("unknown");
        Ok(first_line.trim().to_string())
    }

    /// Run the help command for a given command path and cache the result.
    ///
    /// Executes: `program [path...] <help_flag>`
    /// Both stdout and stderr are captured and concatenated.
    async fn run_help(&self, command_path: &[String]) -> Result<String> {
        let cache_key = if command_path.is_empty() {
            "_root".to_string()
        } else {
            command_path.join(".")
        };

        // Check cache first
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let mut cmd = tokio::process::Command::new(&self.program);
        for part in command_path {
            cmd.arg(part);
        }
        cmd.arg(&self.help_flag);

        debug!(
            "Running: {} {} {}",
            self.program,
            command_path.join(" "),
            self.help_flag
        );

        let output = cmd.output().await.context(format!(
            "Failed to run {} {} {}",
            self.program,
            command_path.join(" "),
            self.help_flag
        ))?;

        // Combine stdout and stderr — many CLIs print help to stderr
        let help_text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);

        // Cache the result
        {
            let mut cache = self.cache.lock().await;
            cache.insert(cache_key, help_text.clone());
        }

        Ok(help_text)
    }

    /// Full introspection entry point.
    ///
    /// Recursively discovers the entire command hierarchy up to `max_depth`.
    pub async fn introspect_full(&self, max_depth: usize) -> Result<CliSchema> {
        let start = std::time::Instant::now();

        info!(
            "Starting CLI introspection for '{}' (max_depth={})",
            self.program, max_depth
        );

        let version = self
            .get_version()
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        info!("{} version: {}", self.program, version);

        let mut stats = CliStats::default();
        let hierarchy = self
            .introspect_command_inner(&[], 0, max_depth, &mut stats)
            .await?;

        stats.introspection_time_ms = start.elapsed().as_millis();

        info!(
            "Introspection complete for '{}': {} groups, {} commands, {} flags in {}ms",
            self.program,
            stats.total_groups,
            stats.total_commands,
            stats.total_flags,
            stats.introspection_time_ms
        );

        Ok(CliSchema {
            program: self.program.clone(),
            version,
            hierarchy,
            statistics: stats,
        })
    }

    /// Recursively introspect a single command node.
    ///
    /// Uses `Box::pin` to allow async recursion through the subcommand tree.
    async fn introspect_command_inner(
        &self,
        command_path: &[String],
        depth: usize,
        max_depth: usize,
        stats: &mut CliStats,
    ) -> Result<CliCommand> {
        if depth > max_depth {
            return Ok(CliCommand::default());
        }

        let full_path = if command_path.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, command_path.join(" "))
        };

        let name = command_path
            .last()
            .map(|s| s.as_str())
            .unwrap_or(&self.program);

        debug!("Introspecting: {}", full_path);

        let help = self.run_help(command_path).await?;

        // Parse groups (gcloud-style GROUPS section)
        let groups = self.parse_groups(&help);
        // Parse commands from all recognized section formats
        let commands = self.parse_commands_section(&help);
        let flags = self.parse_flags_section(&help);
        let description = self.parse_description(&help);

        let is_group = !groups.is_empty() || !commands.is_empty();

        stats.total_flags += flags.len();
        if is_group {
            stats.total_groups += 1;
        } else {
            stats.total_commands += 1;
        }

        let mut cmd = CliCommand {
            name: name.to_string(),
            full_path,
            description,
            is_group,
            flags,
            positional_args: vec![],
            subcommands: HashMap::new(),
        };

        // Recurse into subcommands if we haven't hit the depth limit
        if depth < max_depth {
            // Groups are subcommands that themselves contain more commands
            for (group_name, _desc) in &groups {
                let mut sub_path = command_path.to_vec();
                sub_path.push(group_name.clone());

                match Box::pin(self.introspect_command_inner(
                    &sub_path,
                    depth + 1,
                    max_depth,
                    stats,
                ))
                .await
                {
                    Ok(sub_cmd) => {
                        cmd.subcommands.insert(group_name.clone(), sub_cmd);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to introspect group '{} {}': {}",
                            self.program,
                            sub_path.join(" "),
                            e
                        );
                    }
                }
            }

            // Leaf commands (may still have further subcommands)
            for (cmd_name, _desc) in &commands {
                // Avoid duplicates if a name appeared in both GROUPS and COMMANDS
                if cmd.subcommands.contains_key(cmd_name) {
                    continue;
                }

                let mut sub_path = command_path.to_vec();
                sub_path.push(cmd_name.clone());

                match Box::pin(self.introspect_command_inner(
                    &sub_path,
                    depth + 1,
                    max_depth,
                    stats,
                ))
                .await
                {
                    Ok(sub_cmd) => {
                        cmd.subcommands.insert(cmd_name.clone(), sub_cmd);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to introspect command '{} {}': {}",
                            self.program,
                            sub_path.join(" "),
                            e
                        );
                    }
                }
            }
        }

        Ok(cmd)
    }

    // -----------------------------------------------------------------------
    // Parsing helpers
    // -----------------------------------------------------------------------

    /// Parse commands from help output.
    ///
    /// Recognizes multiple section header styles:
    /// - Cobra-style: "Available Commands:", "Additional Commands:"
    /// - Click-style: "Commands:"
    /// - Gcloud-style: "COMMANDS"
    /// - Misc: "Subcommands:", "SUBCOMMANDS"
    ///
    /// Returns `(name, description)` pairs.
    pub fn parse_commands_section(&self, help: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        let mut in_section = false;

        let cmd_name_re = Regex::new(r"^\s{2,8}(\w[\w-]*)\s{2,}(.*)$").unwrap();
        // Fallback for names with no multi-space gap (single description word)
        let cmd_name_simple_re = Regex::new(r"^\s{2,8}(\w[\w-]*)$").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            // Detect section headers
            if Self::is_commands_header(trimmed) {
                in_section = true;
                continue;
            }

            if in_section {
                // A non-indented, non-empty line signals end of section
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    in_section = false;
                    continue;
                }

                // Skip blank lines within the section
                if trimmed.is_empty() {
                    continue;
                }

                // Try to extract command name + description
                if let Some(caps) = cmd_name_re.captures(line) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    let desc = caps.get(2).unwrap().as_str().trim().to_string();
                    results.push((name, desc));
                } else if let Some(caps) = cmd_name_simple_re.captures(line) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    results.push((name, String::new()));
                }
            }
        }

        results
    }

    /// Parse flags from help output.
    ///
    /// Recognizes section headers:
    /// - Cobra-style: "Flags:", "Global Flags:", "Persistent Flags:"
    /// - Gcloud-style: "FLAGS", "OPTIONAL FLAGS", "REQUIRED FLAGS", "GLOBAL FLAGS",
    ///   "GCLOUD WIDE FLAGS"
    /// - Click/argparse-style: "Options:", "Optional arguments:"
    ///
    /// Handles flag formats:
    /// - `--flag-name string   Description text` (cobra)
    /// - `-f, --flag-name      Description text` (cobra short+long)
    /// - `--flag-name=VALUE    Description text` (gcloud)
    /// - `--flag-name          Description text` (boolean flags)
    pub fn parse_flags_section(&self, help: &str) -> Vec<CliFlag> {
        let mut flags = Vec::new();
        let mut in_flags_section = false;
        let mut is_required_section = false;
        let mut current_flag: Option<CliFlag> = None;

        // Pattern: -f, --flag-name value   Description
        // Pattern:     --flag-name value    Description
        // Pattern:     --flag-name=VALUE    Description
        let long_flag_re =
            Regex::new(r"^\s+(?:(-\w),\s+)?(--[\w-]+)(?:[=\s]\s*(\w+))?\s{2,}(.*)$").unwrap();
        // Simpler: just the flag with no description on this line
        let long_flag_simple_re =
            Regex::new(r"^\s+(?:(-\w),\s+)?(--[\w-]+)(?:[=\s]\s*(\w+))?\s*$").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            // Detect flags section headers
            if Self::is_flags_header(trimmed) {
                // Flush any pending flag
                if let Some(flag) = current_flag.take() {
                    flags.push(flag);
                }
                in_flags_section = true;
                is_required_section = trimmed.contains("REQUIRED");
                continue;
            }

            if in_flags_section {
                // Non-indented, non-empty line ends the section
                if !line.starts_with(' ') && !trimmed.is_empty() && !trimmed.starts_with('-') {
                    in_flags_section = false;
                    if let Some(flag) = current_flag.take() {
                        flags.push(flag);
                    }
                    continue;
                }

                // Skip empty lines
                if trimmed.is_empty() {
                    continue;
                }

                // Try to match a flag definition line
                let matched = if let Some(caps) = long_flag_re.captures(line) {
                    Some((
                        caps.get(1).map(|m| m.as_str().to_string()),
                        caps.get(2).unwrap().as_str().to_string(),
                        caps.get(3).map(|m| m.as_str()),
                        caps.get(4).map(|m| m.as_str().trim().to_string()),
                    ))
                } else if let Some(caps) = long_flag_simple_re.captures(line) {
                    Some((
                        caps.get(1).map(|m| m.as_str().to_string()),
                        caps.get(2).unwrap().as_str().to_string(),
                        caps.get(3).map(|m| m.as_str()),
                        None,
                    ))
                } else {
                    None
                };

                if let Some((short, long, value_hint, desc)) = matched {
                    // Flush previous flag
                    if let Some(flag) = current_flag.take() {
                        flags.push(flag);
                    }

                    // Extract default value from description if present
                    let (description, default_val) = if let Some(ref d) = desc {
                        Self::extract_default(d)
                    } else {
                        (String::new(), None)
                    };

                    current_flag = Some(CliFlag {
                        name: long,
                        short_name: short,
                        description,
                        required: is_required_section,
                        value_type: self.infer_type(value_hint),
                        default: default_val,
                        choices: vec![],
                    });
                } else if let Some(ref mut flag) = current_flag {
                    // Continuation line for the current flag's description
                    if !flag.description.is_empty() {
                        flag.description.push(' ');
                    }
                    flag.description.push_str(trimmed);
                }
            }
        }

        // Flush last flag
        if let Some(flag) = current_flag {
            flags.push(flag);
        }

        flags
    }

    /// Parse the description from help output.
    ///
    /// Looks for "Description:" (title-case) or "DESCRIPTION" (gcloud-style)
    /// sections and returns up to the first 3 lines joined with spaces.
    /// Falls back to the first non-header, non-empty line if no section found.
    pub fn parse_description(&self, help: &str) -> String {
        let mut in_description = false;
        let mut desc_lines: Vec<String> = Vec::new();

        for line in help.lines() {
            let trimmed = line.trim();

            if trimmed == "DESCRIPTION"
                || trimmed == "Description:"
                || trimmed.starts_with("DESCRIPTION")
            {
                in_description = true;
                continue;
            }

            if in_description {
                // Non-indented, non-empty → new section
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    break;
                }
                if !trimmed.is_empty() {
                    desc_lines.push(trimmed.to_string());
                }
            }
        }

        desc_lines
            .into_iter()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parse GROUPS section (gcloud-style).
    ///
    /// Returns `(name, description)` pairs for each group.
    pub fn parse_groups(&self, help: &str) -> Vec<(String, String)> {
        let mut groups = Vec::new();
        let mut in_groups_section = false;

        let name_re = Regex::new(r"^\s{2,8}(\w[\w-]*)\s{2,}(.*)$").unwrap();
        let name_simple_re = Regex::new(r"^\s{2,8}(\w[\w-]*)$").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            if trimmed == "GROUPS" || trimmed.starts_with("GROUPS") {
                in_groups_section = true;
                continue;
            }

            if in_groups_section {
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    break;
                }
                if trimmed.is_empty() {
                    continue;
                }

                if let Some(caps) = name_re.captures(line) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    let desc = caps.get(2).unwrap().as_str().trim().to_string();
                    groups.push((name, desc));
                } else if let Some(caps) = name_simple_re.captures(line) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    groups.push((name, String::new()));
                }
            }
        }

        groups
    }

    /// Infer the value type from an optional hint string.
    ///
    /// - "int", "integer", "number" -> "integer"
    /// - "bool", "boolean" -> "boolean"
    /// - None (no value hint) -> "boolean" (bare flags are typically toggles)
    /// - Anything else -> "string"
    pub fn infer_type(&self, hint: Option<&str>) -> String {
        match hint.map(|s| s.to_lowercase()).as_deref() {
            Some("int") | Some("integer") | Some("number") => "integer".to_string(),
            Some("bool") | Some("boolean") => "boolean".to_string(),
            Some("list") | Some("array") => "array".to_string(),
            Some(_) => "string".to_string(),
            None => "boolean".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Check if a trimmed line is a commands-section header.
    fn is_commands_header(trimmed: &str) -> bool {
        trimmed == "COMMANDS"
            || trimmed.starts_with("COMMANDS")
            || trimmed == "Available Commands:"
            || trimmed == "Additional Commands:"
            || trimmed == "Commands:"
            || trimmed == "Subcommands:"
            || trimmed == "SUBCOMMANDS"
            || trimmed == "Management Commands:"
    }

    /// Check if a trimmed line is a flags-section header.
    fn is_flags_header(trimmed: &str) -> bool {
        trimmed == "Flags:"
            || trimmed == "FLAGS"
            || trimmed == "Options:"
            || trimmed == "Global Flags:"
            || trimmed == "Persistent Flags:"
            || trimmed.starts_with("OPTIONAL FLAGS")
            || trimmed.starts_with("REQUIRED FLAGS")
            || trimmed.starts_with("GLOBAL FLAGS")
            || trimmed.starts_with("GCLOUD WIDE FLAGS")
            || trimmed == "Optional arguments:"
    }

    /// Extract a default value from a description string.
    ///
    /// Looks for patterns like `(default "table")` or `(default: 10)`.
    fn extract_default(desc: &str) -> (String, Option<String>) {
        let default_re =
            Regex::new(r#"\(default[:\s]+["']?([^"')]+)["']?\)"#).unwrap();
        if let Some(caps) = default_re.captures(desc) {
            let default_val = caps.get(1).unwrap().as_str().trim().to_string();
            let cleaned = default_re.replace(desc, "").trim().to_string();
            (cleaned, Some(default_val))
        } else {
            (desc.to_string(), None)
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// Convenience function to introspect any CLI program.
///
/// Creates a [`CliParser`] with default settings and runs a full introspection
/// up to the given depth.
pub async fn introspect_cli(program: &str, max_depth: usize) -> Result<CliSchema> {
    let parser = CliParser::new(program);
    parser.introspect_full(max_depth).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cli_parser_creation() {
        let parser = CliParser::new("incus");
        assert!(parser.cache.lock().await.is_empty());
        assert_eq!(parser.program, "incus");
        assert_eq!(parser.help_flag, "--help");
    }

    #[test]
    fn test_parse_cobra_commands() {
        let parser = CliParser::new("incus");
        let help = r#"
Usage:
  incus [command]

Available Commands:
  admin       Manage incus daemon
  config      Manage instance and server configuration options
  list        List instances

Flags:
  -h, --help   Print help
"#;

        let commands = parser.parse_commands_section(help);
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].0, "admin");
        assert_eq!(commands[0].1, "Manage incus daemon");
        assert_eq!(commands[1].0, "config");
        assert_eq!(
            commands[1].1,
            "Manage instance and server configuration options"
        );
        assert_eq!(commands[2].0, "list");
        assert_eq!(commands[2].1, "List instances");
    }

    #[test]
    fn test_parse_cobra_flags() {
        let parser = CliParser::new("incus");
        let help = r#"
Usage:
  incus [command]

Flags:
      --debug          Show all debug messages
  -h, --help           Print help
  -q, --quiet          Don't show progress information
      --format string  Output format (default "table")
"#;

        let flags = parser.parse_flags_section(help);
        assert_eq!(flags.len(), 4);

        // --debug: boolean (no value hint)
        let debug_flag = flags.iter().find(|f| f.name == "--debug").unwrap();
        assert_eq!(debug_flag.value_type, "boolean");
        assert!(debug_flag.short_name.is_none());
        assert!(debug_flag.description.contains("debug"));

        // -h, --help
        let help_flag = flags.iter().find(|f| f.name == "--help").unwrap();
        assert_eq!(help_flag.short_name.as_deref(), Some("-h"));
        assert_eq!(help_flag.value_type, "boolean");

        // -q, --quiet
        let quiet_flag = flags.iter().find(|f| f.name == "--quiet").unwrap();
        assert_eq!(quiet_flag.short_name.as_deref(), Some("-q"));

        // --format string (default "table")
        let format_flag = flags.iter().find(|f| f.name == "--format").unwrap();
        assert_eq!(format_flag.value_type, "string");
        assert_eq!(format_flag.default.as_deref(), Some("table"));
    }

    #[test]
    fn test_parse_gcloud_style() {
        let parser = CliParser::new("gcloud");
        let help = r#"
NAME
    gcloud - manage Google Cloud resources

GROUPS
    compute       Create and manage Compute Engine resources
    storage       Create and manage Cloud Storage resources
    container     Deploy and manage containers

COMMANDS
    init          Initialize gcloud
    version       Print version information
    help          Display detailed help

FLAGS
    --project=PROJECT
        The Google Cloud project ID.

    --quiet
        Disable interactive prompts.

DESCRIPTION
    The Google Cloud CLI manages authentication, local configuration,
    developer workflow, and interactions with the Cloud Platform APIs.
"#;

        // Groups
        let groups = parser.parse_groups(help);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "compute");
        assert!(groups[0].1.contains("Compute Engine"));
        assert_eq!(groups[1].0, "storage");
        assert_eq!(groups[2].0, "container");

        // Commands
        let commands = parser.parse_commands_section(help);
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].0, "init");
        assert_eq!(commands[1].0, "version");
        assert_eq!(commands[2].0, "help");

        // Flags
        let flags = parser.parse_flags_section(help);
        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.name == "--project"));
        assert!(flags.iter().any(|f| f.name == "--quiet"));

        // Description
        let desc = parser.parse_description(help);
        assert!(desc.contains("Google Cloud CLI"));
    }

    #[test]
    fn test_parse_description_title_case() {
        let parser = CliParser::new("mytool");
        let help = r#"
Description:
    This is a great tool that does many things.
    It supports multiple formats.
    Very useful indeed.
    This fourth line should be ignored.

Usage:
    mytool [flags]
"#;

        let desc = parser.parse_description(help);
        assert!(desc.contains("great tool"));
        assert!(desc.contains("multiple formats"));
        assert!(desc.contains("Very useful"));
        // Only first 3 lines
        assert!(!desc.contains("fourth line"));
    }

    #[test]
    fn test_infer_type() {
        let parser = CliParser::new("test");
        assert_eq!(parser.infer_type(Some("int")), "integer");
        assert_eq!(parser.infer_type(Some("integer")), "integer");
        assert_eq!(parser.infer_type(Some("number")), "integer");
        assert_eq!(parser.infer_type(Some("bool")), "boolean");
        assert_eq!(parser.infer_type(Some("boolean")), "boolean");
        assert_eq!(parser.infer_type(Some("list")), "array");
        assert_eq!(parser.infer_type(Some("string")), "string");
        assert_eq!(parser.infer_type(Some("PATH")), "string");
        assert_eq!(parser.infer_type(None), "boolean");
    }

    #[test]
    fn test_extract_default() {
        let (desc, default) = CliParser::extract_default(r#"Output format (default "table")"#);
        assert_eq!(default.as_deref(), Some("table"));
        assert!(desc.contains("Output format"));
        assert!(!desc.contains("default"));

        let (desc, default) = CliParser::extract_default("Simple description");
        assert_eq!(desc, "Simple description");
        assert!(default.is_none());
    }

    #[test]
    fn test_with_help_flag() {
        let parser = CliParser::with_help_flag("mytool", "-h");
        assert_eq!(parser.help_flag, "-h");
        assert_eq!(parser.program, "mytool");
    }

    #[test]
    fn test_parse_required_flags_section() {
        let parser = CliParser::new("gcloud");
        let help = r#"
REQUIRED FLAGS
    --zone=ZONE
        The zone of the resource.

OPTIONAL FLAGS
    --quiet
        Disable prompts.
"#;

        let flags = parser.parse_flags_section(help);
        let zone = flags.iter().find(|f| f.name == "--zone").unwrap();
        assert!(zone.required);

        let quiet = flags.iter().find(|f| f.name == "--quiet").unwrap();
        assert!(!quiet.required);
    }

    #[test]
    fn test_default_cli_command() {
        let cmd = CliCommand::default();
        assert_eq!(cmd.name, "root");
        assert!(cmd.is_group);
        assert!(cmd.flags.is_empty());
        assert!(cmd.positional_args.is_empty());
        assert!(cmd.subcommands.is_empty());
    }

    #[test]
    fn test_default_cli_stats() {
        let stats = CliStats::default();
        assert_eq!(stats.total_groups, 0);
        assert_eq!(stats.total_commands, 0);
        assert_eq!(stats.total_flags, 0);
        assert_eq!(stats.introspection_time_ms, 0);
    }

    #[test]
    fn test_parse_empty_help() {
        let parser = CliParser::new("empty");
        let help = "";
        assert!(parser.parse_commands_section(help).is_empty());
        assert!(parser.parse_flags_section(help).is_empty());
        assert!(parser.parse_groups(help).is_empty());
        assert!(parser.parse_description(help).is_empty());
    }

    #[test]
    fn test_parse_docker_style_management_commands() {
        let parser = CliParser::new("docker");
        let help = r#"
Management Commands:
  container   Manage containers
  image       Manage images
  network     Manage networks

Commands:
  run         Create and run a new container
  ps          List containers
"#;

        // Management Commands are treated as commands too
        let mgmt = parser.parse_commands_section(help);
        // Should capture both Management Commands and Commands sections
        assert!(mgmt.len() >= 5);
        assert!(mgmt.iter().any(|c| c.0 == "container"));
        assert!(mgmt.iter().any(|c| c.0 == "run"));
    }
}
