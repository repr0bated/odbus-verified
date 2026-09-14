//! Filesystem Tools

use crate::tool_registry::{BoxedTool, Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(ReadFileTool)).await?;
    registry.register(Arc::new(WriteFileTool)).await?;
    registry.register(Arc::new(ListDirectoryTool)).await?;
    Ok(3)
}

/// Normalize a path without requiring it to exist, rejecting traversal attempts.
/// Returns Err if the path contains `..` components or is not absolute.
fn normalize_path(raw: &str) -> Result<PathBuf, &'static str> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("path must be absolute");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => return Err("path traversal not allowed"),
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

const READ_BLOCKED: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/sudoers",
    "/etc/sudoers.d",
    "/etc/ssh",
    "/root",
    "/proc/self",
    "/proc/1",
];

const WRITE_BLOCKED: &[&str] = &[
    "/etc/",
    "/boot/",
    "/bin/",
    "/sbin/",
    "/usr/bin/",
    "/usr/sbin/",
    "/lib/",
    "/lib64/",
    "/usr/lib/",
    "/proc/",
    "/sys/",
    "/dev/",
    "/root/",
];

fn is_read_blocked(path: &Path) -> bool {
    let s = path.to_string_lossy();
    READ_BLOCKED.iter().any(|blocked| s.starts_with(blocked))
}

fn is_write_blocked(path: &Path) -> bool {
    let s = path.to_string_lossy();
    WRITE_BLOCKED.iter().any(|blocked| s.starts_with(blocked))
}

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Read the contents of a file." }
    fn category(&self) -> &str { "filesystem" }
    fn tags(&self) -> Vec<String> { vec!["filesystem".into(), "read".into()] }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Absolute path to the file"}
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let raw = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let path = normalize_path(raw)
            .map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?;

        if is_read_blocked(&path) {
            return Ok(json!({"success": false, "error": "Access denied"}));
        }

        // Resolve symlinks after normalization to catch symlink-based bypasses
        let canonical = tokio::fs::canonicalize(&path).await
            .map_err(|e| anyhow::anyhow!("Cannot resolve path: {}", e))?;

        if is_read_blocked(&canonical) {
            return Ok(json!({"success": false, "error": "Access denied"}));
        }

        match tokio::fs::read_to_string(&canonical).await {
            Ok(content) => Ok(json!({"success": true, "path": raw, "content": content})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()}))
        }
    }
}

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str { "Write content to a file." }
    fn category(&self) -> &str { "filesystem" }
    fn tags(&self) -> Vec<String> { vec!["filesystem".into(), "write".into()] }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let raw = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let content = input.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;

        let path = normalize_path(raw)
            .map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?;

        if is_write_blocked(&path) {
            return Ok(json!({"success": false, "error": "Access denied"}));
        }

        // Canonicalize parent dir to resolve symlinks before writing
        if let Some(parent) = path.parent() {
            if parent.exists() {
                let canonical_parent = tokio::fs::canonicalize(parent).await
                    .map_err(|e| anyhow::anyhow!("Cannot resolve parent: {}", e))?;
                let canonical_path = canonical_parent.join(path.file_name().unwrap_or_default());
                if is_write_blocked(&canonical_path) {
                    return Ok(json!({"success": false, "error": "Access denied"}));
                }
            }
        }

        match tokio::fs::write(&path, content).await {
            Ok(_) => Ok(json!({"success": true, "path": raw, "bytes_written": content.len()})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()}))
        }
    }
}

pub struct ListDirectoryTool;

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str { "list_directory" }
    fn description(&self) -> &str { "List contents of a directory." }
    fn category(&self) -> &str { "filesystem" }
    fn tags(&self) -> Vec<String> { vec!["filesystem".into(), "list".into()] }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let raw = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let path = normalize_path(raw)
            .map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?;

        if is_read_blocked(&path) {
            return Ok(json!({"success": false, "error": "Access denied"}));
        }

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&path).await?;

        while let Some(entry) = dir.next_entry().await? {
            let meta = entry.metadata().await?;
            entries.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "is_dir": meta.is_dir(),
                "size": meta.len()
            }));
        }

        Ok(json!({"success": true, "path": raw, "entries": entries}))
    }
}
