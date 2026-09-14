//! Universal Data Dump - Import data from introspected schemas into database
//!
//! This module executes commands discovered during introspection and imports
//! their output into the op-dbus database.
//!
//! # Workflow
//! 1. Read schema from discover phase
//! 2. Identify data-producing commands (list, describe, get, etc.)
//! 3. Execute commands with JSON output format
//! 4. Import results into database

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::gcloud::{GCloudCommand, GCloudSchema};

/// Result of a data dump operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDumpResult {
    /// Source (e.g., "gcloud")
    pub source: String,
    /// Commands that were executed
    pub commands_executed: Vec<String>,
    /// Total objects imported
    pub total_objects: usize,
    /// Objects by type (e.g., "compute.instances" -> 5)
    pub objects_by_type: HashMap<String, usize>,
    /// Errors encountered
    pub errors: Vec<DataDumpError>,
    /// Time taken in milliseconds
    pub duration_ms: u128,
}

/// Error during data dump
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDumpError {
    pub command: String,
    pub error: String,
}

/// Imported object from external system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedObject {
    /// Object type (e.g., "gcloud.compute.instances")
    pub object_type: String,
    /// Object ID (extracted from object data)
    pub object_id: String,
    /// Full command path that produced this object
    pub source_command: String,
    /// Raw JSON data from the command
    pub data: Value,
    /// Timestamp of import
    pub imported_at: String,
}

/// Data dump executor
pub struct DataDumper {
    /// Dry run mode - don't actually execute commands
    dry_run: bool,
    /// Filter: only dump these resource types (empty = all)
    resource_filter: Vec<String>,
}

impl DataDumper {
    pub fn new() -> Self {
        Self {
            dry_run: false,
            resource_filter: Vec::new(),
        }
    }

    pub fn dry_run(mut self, enabled: bool) -> Self {
        self.dry_run = enabled;
        self
    }

    pub fn filter_resources(mut self, resources: Vec<String>) -> Self {
        self.resource_filter = resources;
        self
    }

    /// Find all data-producing commands in a schema
    fn find_data_commands(&self, cmd: &GCloudCommand, prefix: &str) -> Vec<DataCommand> {
        let mut results = Vec::new();
        let current_path = if prefix.is_empty() {
            cmd.name.clone()
        } else {
            format!("{}.{}", prefix, cmd.name)
        };

        // Check if this command produces data
        if is_data_producing_command(&cmd.name) {
            // Check resource filter
            if self.resource_filter.is_empty()
                || self.resource_filter.iter().any(|f| current_path.contains(f))
            {
                results.push(DataCommand {
                    path: current_path.clone(),
                    full_command: cmd.full_path.clone(),
                    command_type: classify_command(&cmd.name),
                });
            }
        }

        // Recurse into subcommands
        for sub_cmd in cmd.subcommands.values() {
            results.extend(self.find_data_commands(sub_cmd, &current_path));
        }

        results
    }

    /// Execute a data-producing command and return its output
    async fn execute_command(&self, cmd: &DataCommand) -> Result<Vec<ImportedObject>> {
        if self.dry_run {
            info!("[DRY RUN] Would execute: {} --format=json", cmd.full_command);
            return Ok(Vec::new());
        }

        debug!("Executing: {} --format=json", cmd.full_command);

        // Parse the command into parts
        let parts: Vec<&str> = cmd.full_command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(Vec::new());
        }

        let mut command = Command::new(parts[0]);
        for part in &parts[1..] {
            command.arg(part);
        }
        command.arg("--format=json");
        command.env("CLOUDSDK_CORE_DISABLE_PROMPTS", "1");
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let output = command.output().await.context("Failed to execute command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Some commands fail legitimately (no resources, permission denied)
            if stderr.contains("Listed 0 items")
                || stderr.contains("PERMISSION_DENIED")
                || stderr.contains("API has not been used")
            {
                debug!("Command {} returned no data or access denied", cmd.full_command);
                return Ok(Vec::new());
            }
            warn!("Command failed: {} - {}", cmd.full_command, stderr);
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim() == "[]" {
            debug!("Command {} returned empty result", cmd.full_command);
            return Ok(Vec::new());
        }

        // Parse JSON output
        let json: Value = simd_json::from_str(&stdout)
            .with_context(|| format!("Failed to parse JSON from {}", cmd.full_command))?;

        let now = chrono::Utc::now().to_rfc3339();
        let mut objects = Vec::new();

        match json {
            Value::Array(items) => {
                for item in items {
                    let object_id = extract_object_id(&item);
                    objects.push(ImportedObject {
                        object_type: cmd.path.clone(),
                        object_id,
                        source_command: cmd.full_command.clone(),
                        data: item,
                        imported_at: now.clone(),
                    });
                }
            }
            Value::Object(_) => {
                let object_id = extract_object_id(&json);
                objects.push(ImportedObject {
                    object_type: cmd.path.clone(),
                    object_id,
                    source_command: cmd.full_command.clone(),
                    data: json,
                    imported_at: now,
                });
            }
            _ => {
                debug!("Unexpected JSON type from {}", cmd.full_command);
            }
        }

        info!(
            "Imported {} objects from {}",
            objects.len(),
            cmd.full_command
        );
        Ok(objects)
    }

    /// Dump data from a gcloud schema
    pub async fn dump_gcloud(&self, schema: &GCloudSchema) -> Result<(DataDumpResult, Vec<ImportedObject>)> {
        let start = std::time::Instant::now();

        info!("Starting data dump from gcloud schema");

        // Find all data-producing commands
        let data_commands = self.find_data_commands(&schema.hierarchy, "");
        info!("Found {} data-producing commands", data_commands.len());

        let mut result = DataDumpResult {
            source: "gcloud".to_string(),
            commands_executed: Vec::new(),
            total_objects: 0,
            objects_by_type: HashMap::new(),
            errors: Vec::new(),
            duration_ms: 0,
        };

        let mut all_objects = Vec::new();

        for cmd in &data_commands {
            result.commands_executed.push(cmd.full_command.clone());

            match self.execute_command(cmd).await {
                Ok(objects) => {
                    let count = objects.len();
                    if count > 0 {
                        *result.objects_by_type.entry(cmd.path.clone()).or_insert(0) += count;
                        result.total_objects += count;
                        all_objects.extend(objects);
                    }
                }
                Err(e) => {
                    result.errors.push(DataDumpError {
                        command: cmd.full_command.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }

        result.duration_ms = start.elapsed().as_millis();

        info!(
            "Data dump complete: {} objects from {} commands in {}ms",
            result.total_objects,
            result.commands_executed.len(),
            result.duration_ms
        );

        Ok((result, all_objects))
    }
}

impl Default for DataDumper {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal representation of a data-producing command
#[derive(Debug, Clone)]
struct DataCommand {
    /// Dot-separated path (e.g., "compute.instances.list")
    path: String,
    /// Full command (e.g., "gcloud compute instances list")
    full_command: String,
    /// Type of command
    command_type: CommandType,
}

#[derive(Debug, Clone, Copy)]
enum CommandType {
    List,
    Describe,
    Get,
    Other,
}

/// Check if a command name produces data
fn is_data_producing_command(name: &str) -> bool {
    matches!(
        name,
        "list" | "describe" | "get" | "get-value" | "show" | "info"
    )
}

/// Classify a command by type
fn classify_command(name: &str) -> CommandType {
    match name {
        "list" => CommandType::List,
        "describe" => CommandType::Describe,
        "get" | "get-value" => CommandType::Get,
        _ => CommandType::Other,
    }
}

/// Extract an ID from an object
fn extract_object_id(obj: &Value) -> String {
    // Try common ID fields
    for field in &["id", "name", "selfLink", "resource_id", "uid", "ID", "Name"] {
        if let Some(id) = obj.get(field) {
            if let Some(s) = id.as_str() {
                return s.to_string();
            }
            if let Some(n) = id.as_u64() {
                return n.to_string();
            }
        }
    }

    // For selfLink, extract the last part
    if let Some(link) = obj.get("selfLink").and_then(|v| v.as_str()) {
        if let Some(last) = link.rsplit('/').next() {
            return last.to_string();
        }
    }

    // Fallback to hash of object
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    obj.to_string().hash(&mut hasher);
    format!("obj-{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_data_producing_command() {
        assert!(is_data_producing_command("list"));
        assert!(is_data_producing_command("describe"));
        assert!(is_data_producing_command("get"));
        assert!(!is_data_producing_command("create"));
        assert!(!is_data_producing_command("delete"));
    }

    #[test]
    fn test_extract_object_id() {
        let obj = simd_json::json!({"id": "12345", "name": "my-vm"});
        assert_eq!(extract_object_id(&obj), "12345");

        let obj = simd_json::json!({"name": "my-bucket"});
        assert_eq!(extract_object_id(&obj), "my-bucket");

        let obj = simd_json::json!({"selfLink": "https://compute.googleapis.com/compute/v1/projects/my-project/zones/us-central1-a/instances/my-vm"});
        assert_eq!(extract_object_id(&obj), "my-vm");
    }
}
