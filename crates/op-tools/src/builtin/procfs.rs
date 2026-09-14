//! /proc and /sys tools with read/write support.

use async_trait::async_trait;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::Tool;

const MAX_READ_DEPTH: usize = 3;
const MAX_INLINE_SIZE: usize = 64 * 1024;

fn validate_relative_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty() || path.starts_with('/') || path.contains("..") || path.contains('\\') {
        return Err(anyhow::anyhow!("Invalid path"));
    }
    Ok(())
}

fn make_full_path(root: &str, path: &str) -> PathBuf {
    Path::new(root).join(path)
}

async fn read_file_value(path: &Path) -> Value {
    match fs::read_to_string(path).await {
        Ok(content) => {
            let trimmed = content.trim();
            if let Ok(num) = trimmed.parse::<i64>() {
                return json!(num);
            }
            if let Ok(num) = trimmed.parse::<f64>() {
                return json!(num);
            }
            if trimmed.len() > MAX_INLINE_SIZE {
                return json!(format!(
                    "[content too large: {} bytes]",
                    trimmed.len()
                ));
            }
            json!(trimmed)
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn read_path(path: &Path) -> Value {
    if path.is_file() {
        read_file_value(path).await
    } else if path.is_dir() {
        let mut entries = Vec::new();
        if let Ok(mut dir) = fs::read_dir(path).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    let is_dir = entry.path().is_dir();
                    entries.push(json!({
                        "name": name,
                        "type": if is_dir { "dir" } else { "file" }
                    }));
                }
            }
        }
        entries.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        json!({ "entries": entries, "count": entries.len() })
    } else {
        json!({ "error": "path not found" })
    }
}

async fn fs_to_json(path: &Path, max_depth: usize, current_depth: usize) -> Value {
    if current_depth > max_depth {
        return Value::Null;
    }

    if path.is_file() {
        return read_file_value(path).await;
    }

    if path.is_dir() {
        let mut obj = Map::new();
        if let Ok(mut entries) = fs::read_dir(path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with('.') || name == "fd" || name == "task" {
                        continue;
                    }
                    let child_path = entry.path();
                    let value =
                        Box::pin(fs_to_json(&child_path, max_depth, current_depth + 1)).await;
                    if !value.is_null() {
                        obj.insert(name.to_string(), value);
                    }
                }
            }
        }
        if obj.is_empty() {
            Value::Null
        } else {
            Value::Object(obj)
        }
    } else {
        Value::Null
    }
}

async fn read_with_depth(root: &str, path: &str, depth: usize) -> Value {
    let full_path = make_full_path(root, path);
    if depth > 1 {
        fs_to_json(&full_path, depth, 0).await
    } else {
        read_path(&full_path).await
    }
}

async fn write_value(root: &str, path: &str, content: &str, append: bool) -> anyhow::Result<()> {
    let full_path = make_full_path(root, path);
    if full_path.is_dir() {
        return Err(anyhow::anyhow!("Path is a directory"));
    }

    if append {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&full_path)
            .await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;
    } else {
        fs::write(&full_path, content).await?;
    }

    Ok(())
}

pub struct ProcFsReadTool;

impl ProcFsReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ProcFsReadTool {
    fn name(&self) -> &str {
        "procfs_read"
    }

    fn description(&self) -> &str {
        "Read /proc as JSON (files parse to numbers/strings; directories list or recurse)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to /proc (e.g., 'sys/net/ipv4/ip_forward', 'meminfo')"
                },
                "depth": {
                    "type": "integer",
                    "description": "Max recursion depth for directories (default: 1, max: 3)"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn tags(&self) -> Vec<String> {
        vec!["proc".to_string(), "system".to_string(), "filesystem".to_string()]
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let (path, depth) = match input {
            Value::Object(mut obj) => {
                let path = obj
                    .remove("path")
                    .and_then(|v| v.as_str().map(str::to_string))
                    .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
                let depth = obj
                    .remove("depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .min(MAX_READ_DEPTH as u64) as usize;
                (path, depth)
            }
            _ => return Err(anyhow::anyhow!("Invalid arguments")),
        };

        validate_relative_path(&path)?;
        let data = read_with_depth("/proc", &path, depth).await;
        Ok(json!({
            "path": format!("/proc/{}", path),
            "data": data
        }))
    }
}

pub struct SysFsReadTool;

impl SysFsReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SysFsReadTool {
    fn name(&self) -> &str {
        "sysfs_read"
    }

    fn description(&self) -> &str {
        "Read /sys as JSON (files parse to numbers/strings; directories list or recurse)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to /sys (e.g., 'class/net', 'devices/system/cpu')"
                },
                "depth": {
                    "type": "integer",
                    "description": "Max recursion depth for directories (default: 1, max: 3)"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn tags(&self) -> Vec<String> {
        vec!["sys".to_string(), "hardware".to_string(), "filesystem".to_string()]
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let (path, depth) = match input {
            Value::Object(mut obj) => {
                let path = obj
                    .remove("path")
                    .and_then(|v| v.as_str().map(str::to_string))
                    .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
                let depth = obj
                    .remove("depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .min(MAX_READ_DEPTH as u64) as usize;
                (path, depth)
            }
            _ => return Err(anyhow::anyhow!("Invalid arguments")),
        };

        validate_relative_path(&path)?;
        let data = read_with_depth("/sys", &path, depth).await;
        Ok(json!({
            "path": format!("/sys/{}", path),
            "data": data
        }))
    }
}

pub struct ProcFsWriteTool;

impl ProcFsWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ProcFsWriteTool {
    fn name(&self) -> &str {
        "procfs_write"
    }

    fn description(&self) -> &str {
        "Write to /proc files (used to change kernel parameters)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to /proc (e.g., 'sys/net/ipv4/ip_forward')"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "append": {
                    "type": "boolean",
                    "default": false,
                    "description": "Append instead of overwrite"
                },
                "ensure_newline": {
                    "type": "boolean",
                    "default": true,
                    "description": "Ensure trailing newline (common for /proc writes)"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn namespace(&self) -> &str {
        "control-agent"
    }

    fn tags(&self) -> Vec<String> {
        vec!["proc".to_string(), "system".to_string(), "write".to_string()]
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let mut obj = match input {
            Value::Object(obj) => obj,
            _ => return Err(anyhow::anyhow!("Invalid arguments")),
        };

        let path = obj
            .remove("path")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let mut content = obj
            .remove("content")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
        let append = obj
            .remove("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ensure_newline = obj
            .remove("ensure_newline")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if ensure_newline && !content.ends_with('\n') {
            content.push('\n');
        }

        validate_relative_path(&path)?;
        write_value("/proc", &path, &content, append).await?;

        Ok(json!({
            "path": format!("/proc/{}", path),
            "written_bytes": content.len(),
            "append": append
        }))
    }
}

pub struct SysFsWriteTool;

impl SysFsWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SysFsWriteTool {
    fn name(&self) -> &str {
        "sysfs_write"
    }

    fn description(&self) -> &str {
        "Write to /sys files (used to change device parameters)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to /sys (e.g., 'class/net/eth0/mtu')"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "append": {
                    "type": "boolean",
                    "default": false,
                    "description": "Append instead of overwrite"
                },
                "ensure_newline": {
                    "type": "boolean",
                    "default": true,
                    "description": "Ensure trailing newline (common for /sys writes)"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn namespace(&self) -> &str {
        "control-agent"
    }

    fn tags(&self) -> Vec<String> {
        vec!["sys".to_string(), "hardware".to_string(), "write".to_string()]
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let mut obj = match input {
            Value::Object(obj) => obj,
            _ => return Err(anyhow::anyhow!("Invalid arguments")),
        };

        let path = obj
            .remove("path")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let mut content = obj
            .remove("content")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
        let append = obj
            .remove("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ensure_newline = obj
            .remove("ensure_newline")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if ensure_newline && !content.ends_with('\n') {
            content.push('\n');
        }

        validate_relative_path(&path)?;
        write_value("/sys", &path, &content, append).await?;

        Ok(json!({
            "path": format!("/sys/{}", path),
            "written_bytes": content.len(),
            "append": append
        }))
    }
}
