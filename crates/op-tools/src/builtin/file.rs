//! File Tools with Access Level Security
//!
//! Provides file operations with access level based security:
//! - Unrestricted (Admin): Full read/write access
//! - Restricted: Limited read-only access
//!
//! ## Security Model
//!
//! The chatbot is a FULL SYSTEM ADMINISTRATOR.
//! Admin users can read/write any file (except path traversal).
//! Audit logging is handled by the snowball plugin.

use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::path::Path;
use std::sync::Arc;

use crate::registry::ToolDefinition;
use crate::security::get_security_validator;
use crate::Tool;

/// Register all file tools
pub async fn register_file_tools(registry: &crate::ToolRegistry) -> anyhow::Result<()> {
    let tools = vec![
        SecureFileTool::read(),
        SecureFileTool::write(),
        SecureFileTool::list(),
        SecureFileTool::exists(),
        SecureFileTool::stat(),
    ];

    for tool in tools {
        let name = tool.name().to_string();
        let definition = ToolDefinition {
            name: name.clone(),
            description: tool.description().to_string(),
            input_schema: tool.input_schema(),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: tool.category().to_string(),
            namespace: "system.v1".to_string(),
            tags: tool.tags(),
        };
        registry
            .register(name.into(), Arc::new(tool), definition)
            .await?;
    }

    Ok(())
}

// ============================================================================
// SECURE FILE TOOL
// ============================================================================

pub struct SecureFileTool {
    name: String,
    description: String,
}

impl SecureFileTool {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }

    pub fn read() -> Self {
        Self::new(
            "file_read",
            "Read file contents. Full access for admin users.",
        )
    }

    pub fn write() -> Self {
        Self::new(
            "file_write",
            "Write content to a file. Full access for admin users.",
        )
    }

    pub fn list() -> Self {
        Self::new(
            "file_list",
            "List directory contents. Full access for admin users.",
        )
    }

    pub fn exists() -> Self {
        Self::new("file_exists", "Check if a file exists.")
    }

    pub fn stat() -> Self {
        Self::new("file_stat", "Get file metadata (size, type, permissions).")
    }
}

#[async_trait]
impl Tool for SecureFileTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> &str {
        "filesystem"
    }

    fn tags(&self) -> Vec<String> {
        vec!["file".to_string(), "filesystem".to_string()]
    }

    fn input_schema(&self) -> Value {
        match self.name.as_str() {
            "file_read" => json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to read"
                    },
                    "max_lines": {
                        "type": "integer",
                        "description": "Maximum lines to return (default: 1000)",
                        "default": 1000
                    }
                },
                "required": ["path"]
            }),
            "file_write" => json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    },
                    "append": {
                        "type": "boolean",
                        "description": "Append instead of overwrite (default: false)",
                        "default": false
                    }
                },
                "required": ["path", "content"]
            }),
            "file_list" => json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list"
                    },
                    "max_entries": {
                        "type": "integer",
                        "description": "Maximum entries to return (default: 100)",
                        "default": 100
                    }
                },
                "required": ["path"]
            }),
            "file_exists" => json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to check"
                    }
                },
                "required": ["path"]
            }),
            "file_stat" => json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to get metadata for"
                    }
                },
                "required": ["path"]
            }),
            _ => json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let validator = get_security_validator();

        match self.name.as_str() {
            "file_read" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

                // Validate path for reading
                validator
                    .validate_read_path(path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                let max_lines = args
                    .get("max_lines")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000) as usize;

                let content = tokio::fs::read_to_string(path).await?;
                let lines: Vec<&str> = content.lines().take(max_lines).collect();
                let truncated = content.lines().count() > max_lines;

                Ok(json!({
                    "path": path,
                    "content": lines.join("\n"),
                    "lines": lines.len(),
                    "truncated": truncated
                }))
            }

            "file_write" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: content"))?;

                let append = args
                    .get("append")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Validate path for writing
                validator
                    .validate_write_path(path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                if append {
                    use tokio::io::AsyncWriteExt;
                    let mut file = tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .await?;
                    file.write_all(content.as_bytes()).await?;
                } else {
                    tokio::fs::write(path, content).await?;
                }

                Ok(json!({
                    "path": path,
                    "written": content.len(),
                    "append": append,
                    "success": true
                }))
            }

            "file_list" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

                // Validate path for reading
                validator
                    .validate_read_path(path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                let max_entries = args
                    .get("max_entries")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .min(1000) as usize;

                let mut entries = tokio::fs::read_dir(path).await?;
                let mut files = Vec::new();
                let mut count = 0;

                while let Some(entry) = entries.next_entry().await? {
                    if count >= max_entries {
                        break;
                    }

                    let file_type = entry.file_type().await.ok();
                    let metadata = entry.metadata().await.ok();

                    files.push(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "is_dir": file_type.map(|t| t.is_dir()).unwrap_or(false),
                        "is_file": file_type.map(|t| t.is_file()).unwrap_or(false),
                        "size": metadata.map(|m| m.len()).unwrap_or(0)
                    }));

                    count += 1;
                }

                Ok(json!({
                    "path": path,
                    "entries": files,
                    "count": files.len(),
                    "truncated": count >= max_entries
                }))
            }

            "file_exists" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

                // For exists check, still validate against path traversal
                validator
                    .validate_read_path(path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                let exists = Path::new(path).exists();

                Ok(json!({
                    "path": path,
                    "exists": exists
                }))
            }

            "file_stat" => {
                let path = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

                // Validate path for reading
                validator
                    .validate_read_path(path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                let metadata = tokio::fs::metadata(path).await?;

                Ok(json!({
                    "path": path,
                    "size": metadata.len(),
                    "is_file": metadata.is_file(),
                    "is_dir": metadata.is_dir(),
                    "is_symlink": metadata.file_type().is_symlink(),
                    "readonly": metadata.permissions().readonly()
                }))
            }

            _ => Err(anyhow::anyhow!("Unknown file operation: {}", self.name)),
        }
    }
}

/// Legacy alias
pub type FileTool = SecureFileTool;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_file() {
        let tool = SecureFileTool::read();
        let temp_path = "/tmp/test_read_file.txt";
        tokio::fs::write(temp_path, "test content").await.unwrap();

        let result = tool.execute(json!({"path": temp_path})).await;
        assert!(result.is_ok());

        let _ = tokio::fs::remove_file(temp_path).await;
    }

    #[tokio::test]
    async fn test_write_file() {
        let tool = SecureFileTool::write();
        let temp_path = "/tmp/test_write_file.txt";

        let result = tool
            .execute(json!({
                "path": temp_path,
                "content": "hello world"
            }))
            .await;

        assert!(result.is_ok());
        let content = tokio::fs::read_to_string(temp_path).await.unwrap();
        assert_eq!(content, "hello world");

        let _ = tokio::fs::remove_file(temp_path).await;
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let tool = SecureFileTool::read();
        let result = tool.execute(json!({"path": "/tmp/../etc/passwd"})).await;
        assert!(result.is_err());
    }
}
