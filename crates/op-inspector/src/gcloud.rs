//! GCloud CLI Introspection Adapter
//!
//! Introspects the complete gcloud command hierarchy, discovering:
//! - Command groups and subcommands
//! - Flags and arguments for each command
//! - Command descriptions
//!
//! # Usage
//!
//! ```ignore
//! // NOTE: this example does not compile against the current code. It
//! // documents the adapter API described in ADAPTER-WORKFLOW.md —
//! // `register_parser`, a no-arg `IntrospectiveGadget::new()`, and an
//! // `InspectionSource::GCloud` variant — none of which exist. The trait
//! // `ObjectParser` is also private, so no adapter outside the gadget
//! // module can implement it. Marked `ignore` rather than deleted so the
//! // intended contract stays visible until the gadget is wired up.
//! use op_inspector::{IntrospectiveGadget, InspectionInput, InspectionSource};
//!
//! let gadget = IntrospectiveGadget::new();
//! gadget.register_parser("gcloud", Arc::new(GCloudParser::new()));
//!
//! let input = InspectionInput {
//!     source: InspectionSource::GCloud {
//!         command_path: vec![],  // Start from root
//!         max_depth: 3,
//!     },
//!     data: None,
//!     metadata: Default::default(),
//! };
//!
//! let result = gadget.inspect_object(input).await?;
//! ```

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// GCloud command hierarchy schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudSchema {
    /// Schema version
    pub schema_version: String,
    /// GCloud SDK version
    pub gcloud_version: String,
    /// Target account (if authenticated)
    pub account: Option<String>,
    /// Root command hierarchy
    pub hierarchy: GCloudCommand,
    /// Statistics
    pub statistics: GCloudStats,
}

/// Statistics about the introspection
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GCloudStats {
    pub total_groups: usize,
    pub total_commands: usize,
    pub total_flags: usize,
    pub introspection_time_ms: u128,
}

/// Represents a gcloud command or command group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudCommand {
    /// Command name (e.g., "compute", "instances", "list")
    pub name: String,
    /// Full command path (e.g., "gcloud compute instances list")
    pub full_path: String,
    /// Command description
    pub description: String,
    /// Whether this is a command group (has subcommands)
    pub is_group: bool,
    /// Available flags
    pub flags: Vec<GCloudFlag>,
    /// Positional arguments
    pub positional_args: Vec<GCloudArg>,
    /// Subcommands (if this is a group)
    pub subcommands: HashMap<String, GCloudCommand>,
}

impl Default for GCloudCommand {
    fn default() -> Self {
        Self {
            name: "gcloud".to_string(),
            full_path: "gcloud".to_string(),
            description: String::new(),
            is_group: true,
            flags: vec![],
            positional_args: vec![],
            subcommands: HashMap::new(),
        }
    }
}

/// GCloud command flag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudFlag {
    pub name: String,
    pub short_name: Option<String>,
    pub description: String,
    pub required: bool,
    pub value_type: String,
    pub default: Option<String>,
    pub choices: Vec<String>,
}

/// GCloud positional argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudArg {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// GCloud CLI introspection parser
pub struct GCloudParser {
    cache: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for GCloudParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GCloudParser {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the current gcloud version
    pub async fn get_version(&self) -> Result<String> {
        let output = tokio::process::Command::new("gcloud")
            .arg("--version")
            .output()
            .await
            .context("Failed to run gcloud --version")?;

        let version_str = String::from_utf8_lossy(&output.stdout);
        Ok(version_str.lines().next().unwrap_or("unknown").to_string())
    }

    /// Get the current authenticated account
    pub async fn get_account(&self) -> Result<Option<String>> {
        let output = tokio::process::Command::new("gcloud")
            .args(["config", "get-value", "account"])
            .output()
            .await
            .context("Failed to get gcloud account")?;

        let account = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if account.is_empty() || account == "(unset)" {
            Ok(None)
        } else {
            Ok(Some(account))
        }
    }

    /// Run gcloud help for a command path
    async fn run_help(&self, command_path: &[String]) -> Result<String> {
        let cache_key = command_path.join(".");

        // Check cache
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let mut cmd = tokio::process::Command::new("gcloud");
        for part in command_path {
            cmd.arg(part);
        }
        cmd.arg("--help");
        cmd.env("CLOUDSDK_CORE_DISABLE_PROMPTS", "1");

        let output = cmd.output().await.context("Failed to run gcloud help")?;

        let help_text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Cache the result
        {
            let mut cache = self.cache.lock().await;
            cache.insert(cache_key, help_text.clone());
        }

        Ok(help_text)
    }

    /// Parse command groups from help output
    fn parse_groups(&self, help: &str) -> Vec<String> {
        let mut groups = Vec::new();
        let mut in_groups_section = false;
        let group_regex = Regex::new(r"^\s{4,8}(\w[\w-]*)\s").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            if trimmed == "GROUPS" || trimmed.starts_with("GROUPS") {
                in_groups_section = true;
                continue;
            }

            if in_groups_section {
                // New section starts
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    break;
                }

                // Parse group name (indented, followed by description)
                if let Some(caps) = group_regex.captures(line) {
                    if let Some(name) = caps.get(1) {
                        groups.push(name.as_str().to_string());
                    }
                }
            }
        }

        groups
    }

    /// Parse commands from help output
    fn parse_commands(&self, help: &str) -> Vec<String> {
        let mut commands = Vec::new();
        let mut in_commands_section = false;
        let cmd_regex = Regex::new(r"^\s{4,8}(\w[\w-]*)\s").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            if trimmed == "COMMANDS" || trimmed.starts_with("COMMANDS") {
                in_commands_section = true;
                continue;
            }

            if in_commands_section {
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    break;
                }

                if let Some(caps) = cmd_regex.captures(line) {
                    if let Some(name) = caps.get(1) {
                        commands.push(name.as_str().to_string());
                    }
                }
            }
        }

        commands
    }

    /// Parse flags from help output
    fn parse_flags(&self, help: &str) -> Vec<GCloudFlag> {
        let mut flags = Vec::new();
        let mut in_flags_section = false;
        let mut current_flag: Option<GCloudFlag> = None;

        let flag_regex = Regex::new(r"^\s+(--[\w-]+)(?:=(\w+))?(?:,\s+(-\w))?").unwrap();

        for line in help.lines() {
            let trimmed = line.trim();

            // Detect flags sections
            if trimmed.contains("FLAGS")
                && (trimmed == "FLAGS"
                    || trimmed.starts_with("OPTIONAL FLAGS")
                    || trimmed.starts_with("REQUIRED FLAGS")
                    || trimmed.starts_with("GLOBAL FLAGS")
                    || trimmed.starts_with("GCLOUD WIDE FLAGS"))
            {
                in_flags_section = true;
                continue;
            }

            if in_flags_section {
                // New section
                if !line.starts_with(' ') && !trimmed.is_empty() && !trimmed.starts_with("--") {
                    in_flags_section = false;
                    if let Some(flag) = current_flag.take() {
                        flags.push(flag);
                    }
                    continue;
                }

                // Flag definition
                if let Some(caps) = flag_regex.captures(line) {
                    if let Some(flag) = current_flag.take() {
                        flags.push(flag);
                    }

                    let flag_name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let value_hint = caps.get(2).map(|m| m.as_str());
                    let short_name = caps.get(3).map(|m| m.as_str().to_string());

                    current_flag = Some(GCloudFlag {
                        name: flag_name.to_string(),
                        short_name,
                        description: String::new(),
                        required: false,
                        value_type: self.infer_type(value_hint),
                        default: None,
                        choices: vec![],
                    });
                } else if let Some(ref mut flag) = current_flag {
                    // Description continuation
                    if !trimmed.is_empty() {
                        if !flag.description.is_empty() {
                            flag.description.push(' ');
                        }
                        flag.description.push_str(trimmed);
                    }
                }
            }
        }

        if let Some(flag) = current_flag {
            flags.push(flag);
        }

        flags
    }

    /// Parse description from help output
    fn parse_description(&self, help: &str) -> String {
        let mut in_description = false;
        let mut description_lines = Vec::new();

        for line in help.lines() {
            let trimmed = line.trim();

            if trimmed == "DESCRIPTION" {
                in_description = true;
                continue;
            }

            if in_description {
                if !line.starts_with(' ') && !trimmed.is_empty() {
                    break;
                }
                if !trimmed.is_empty() {
                    description_lines.push(trimmed.to_string());
                }
            }
        }

        description_lines
            .into_iter()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Infer type from hint
    fn infer_type(&self, hint: Option<&str>) -> String {
        match hint.map(|s| s.to_lowercase()).as_deref() {
            Some("int") | Some("integer") | Some("number") => "integer".to_string(),
            Some("bool") | Some("boolean") => "boolean".to_string(),
            Some("list") | Some("array") => "array".to_string(),
            Some(_) => "string".to_string(),
            None => "boolean".to_string(),
        }
    }

    /// Recursively introspect a command (non-recursive entry point)
    async fn introspect_command(
        &self,
        command_path: &[String],
        depth: usize,
        max_depth: usize,
    ) -> Result<(GCloudCommand, GCloudStats)> {
        let mut stats = GCloudStats::default();
        let cmd = self
            .introspect_command_inner(command_path, depth, max_depth, &mut stats)
            .await?;
        Ok((cmd, stats))
    }

    /// Recursively introspect a command (uses iteration to avoid async recursion)
    async fn introspect_command_inner(
        &self,
        command_path: &[String],
        depth: usize,
        max_depth: usize,
        stats: &mut GCloudStats,
    ) -> Result<GCloudCommand> {
        if depth > max_depth {
            return Ok(GCloudCommand::default());
        }

        let full_path = if command_path.is_empty() {
            "gcloud".to_string()
        } else {
            format!("gcloud {}", command_path.join(" "))
        };

        let name = command_path.last().map(|s| s.as_str()).unwrap_or("gcloud");

        debug!("Introspecting: {}", full_path);

        let help = self.run_help(command_path).await?;

        let groups = self.parse_groups(&help);
        let commands = self.parse_commands(&help);
        let flags = self.parse_flags(&help);
        let description = self.parse_description(&help);

        let is_group = !groups.is_empty() || !commands.is_empty();

        stats.total_flags += flags.len();
        if is_group {
            stats.total_groups += 1;
        } else {
            stats.total_commands += 1;
        }

        let mut cmd = GCloudCommand {
            name: name.to_string(),
            full_path,
            description,
            is_group,
            flags,
            positional_args: vec![],
            subcommands: HashMap::new(),
        };

        // Introspect subcommands (one level at a time to avoid deep recursion)
        if depth < max_depth {
            for group in groups {
                let mut sub_path = command_path.to_vec();
                sub_path.push(group.clone());

                match Box::pin(self.introspect_command_inner(
                    &sub_path,
                    depth + 1,
                    max_depth,
                    stats,
                ))
                .await
                {
                    Ok(sub_cmd) => {
                        cmd.subcommands.insert(group, sub_cmd);
                    }
                    Err(e) => {
                        warn!("Failed to introspect {}: {}", sub_path.join(" "), e);
                    }
                }
            }

            for command in commands {
                let mut sub_path = command_path.to_vec();
                sub_path.push(command.clone());

                match Box::pin(self.introspect_command_inner(
                    &sub_path,
                    depth + 1,
                    max_depth,
                    stats,
                ))
                .await
                {
                    Ok(sub_cmd) => {
                        cmd.subcommands.insert(command, sub_cmd);
                    }
                    Err(e) => {
                        warn!("Failed to introspect {}: {}", sub_path.join(" "), e);
                    }
                }
            }
        }

        Ok(cmd)
    }

    /// Full introspection of gcloud CLI
    pub async fn introspect_full(&self, max_depth: usize) -> Result<GCloudSchema> {
        let start = std::time::Instant::now();

        info!(
            "Starting gcloud CLI introspection (max_depth={})",
            max_depth
        );

        let version = self
            .get_version()
            .await
            .unwrap_or_else(|_| "unknown".to_string());
        let account = self.get_account().await.unwrap_or(None);

        info!("GCloud version: {}", version);
        if let Some(ref acc) = account {
            info!("Authenticated account: {}", acc);
        }

        let (hierarchy, mut stats) = self.introspect_command(&[], 0, max_depth).await?;

        stats.introspection_time_ms = start.elapsed().as_millis();

        info!(
            "Introspection complete: {} groups, {} commands, {} flags in {}ms",
            stats.total_groups,
            stats.total_commands,
            stats.total_flags,
            stats.introspection_time_ms
        );

        Ok(GCloudSchema {
            schema_version: "1.0.0".to_string(),
            gcloud_version: version,
            account,
            hierarchy,
            statistics: stats,
        })
    }
}

/// Convenience function to introspect gcloud and return schema
pub async fn introspect_gcloud(max_depth: usize) -> Result<GCloudSchema> {
    let parser = GCloudParser::new();
    parser.introspect_full(max_depth).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gcloud_parser_creation() {
        let parser = GCloudParser::new();
        assert!(parser.cache.lock().await.is_empty());
    }

    #[test]
    fn test_parse_groups() {
        let parser = GCloudParser::new();
        let help = r#"
NAME
    gcloud - manage Google Cloud resources

GROUPS
    compute       Create and manage Compute Engine resources
    storage       Create and manage Cloud Storage resources
    container     Deploy and manage containers

COMMANDS
    init          Initialize gcloud
"#;

        let groups = parser.parse_groups(help);
        assert_eq!(groups, vec!["compute", "storage", "container"]);
    }

    #[test]
    fn test_parse_commands() {
        let parser = GCloudParser::new();
        let help = r#"
COMMANDS
    init          Initialize gcloud
    version       Print version information
    help          Display help
"#;

        let commands = parser.parse_commands(help);
        assert_eq!(commands, vec!["init", "version", "help"]);
    }

    #[test]
    fn test_parse_flags() {
        let parser = GCloudParser::new();
        let help = r#"
FLAGS
    --project=PROJECT
        The Google Cloud project ID.

    --zone=ZONE, -z
        The zone of the resource.

    --quiet
        Disable interactive prompts.
"#;

        let flags = parser.parse_flags(help);
        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.name == "--project"));
        assert!(flags.iter().any(|f| f.name == "--zone"));
        assert!(flags.iter().any(|f| f.name == "--quiet"));
    }
}
