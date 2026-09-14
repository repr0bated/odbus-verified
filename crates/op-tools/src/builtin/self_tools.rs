//! Self-Repository Tools
//!
//! These tools allow the chatbot to read, modify, and commit changes to its OWN
//! source code repository. These tools ONLY work within the self-repository
//! defined by the OP_SELF_REPO_PATH environment variable.
//!
//! ## Security Model
//!
//! All operations are strictly scoped to the self-repository:
//! - Path traversal outside the repo is blocked
//! - Only files within OP_SELF_REPO_PATH can be accessed
//! - Git operations only affect the self-repository

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{info, warn};
use zbus::Connection;

use crate::tool::{SecurityLevel, Tool};

/// Get the self-repository path from environment
fn get_self_repo_path() -> Option<PathBuf> {
    std::env::var("OP_SELF_REPO_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Helper to ensure a path is within the self-repository
fn validate_self_path(relative_path: &str) -> Result<PathBuf> {
    let repo_path = get_self_repo_path()
        .ok_or_else(|| anyhow::anyhow!("OP_SELF_REPO_PATH environment variable is not set"))?;
    
    // Clean the path - remove leading slashes and normalize
    let clean_path = relative_path.trim_start_matches('/');
    let full_path = repo_path.join(clean_path);
    
    // Canonicalize to resolve .. and .
    let canonical = full_path.canonicalize().unwrap_or_else(|_| full_path.clone());
    
    // Ensure it's still within the repo
    if !canonical.starts_with(&repo_path) {
        return Err(anyhow::anyhow!(
            "Path '{}' would escape the self-repository. Access denied.",
            relative_path
        ));
    }
    
    Ok(canonical)
}

/// Run a git command in the self-repository
async fn run_git_command(args: &[&str]) -> Result<(String, String, i32)> {
    let repo_path = get_self_repo_path()
        .ok_or_else(|| anyhow::anyhow!("OP_SELF_REPO_PATH environment variable is not set"))?;
    
    let output = Command::new("git")
        .args(args)
        .current_dir(&repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    
    Ok((stdout, stderr, code))
}

// =============================================================================
// SELF READ FILE TOOL
// =============================================================================

pub struct SelfReadFileTool;

#[async_trait]
impl Tool for SelfReadFileTool {
    fn name(&self) -> &str {
        "self_read_file"
    }

    fn description(&self) -> &str {
        "Read a file from YOUR OWN source code repository. Use relative paths from the repository root."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path from repository root (e.g., 'crates/op-core/src/lib.rs')"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional: Start reading from this line (1-indexed)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional: Stop reading at this line (1-indexed, inclusive)"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "file".to_string(), "read".to_string(), "source".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'path' argument"))?;
        
        let full_path = validate_self_path(path)?;
        
        if !full_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", path));
        }
        
        if !full_path.is_file() {
            return Err(anyhow::anyhow!("Path is not a file: {}", path));
        }
        
        let start_line = input.get("start_line").and_then(|v| v.as_u64()).map(|v| v as usize);
        let end_line = input.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);
        
        let content = tokio::fs::read_to_string(&full_path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        
        let (selected_lines, shown_range) = match (start_line, end_line) {
            (Some(s), Some(e)) => {
                let s = s.saturating_sub(1).min(total_lines);
                let e = e.min(total_lines);
                (lines[s..e].to_vec(), format!("{}-{}", s + 1, e))
            }
            (Some(s), None) => {
                let s = s.saturating_sub(1).min(total_lines);
                (lines[s..].to_vec(), format!("{}-{}", s + 1, total_lines))
            }
            (None, Some(e)) => {
                let e = e.min(total_lines);
                (lines[..e].to_vec(), format!("1-{}", e))
            }
            (None, None) => {
                (lines.clone(), format!("1-{}", total_lines))
            }
        };
        
        Ok(json!({
            "path": path,
            "content": selected_lines.join("\n"),
            "lines_shown": selected_lines.len(),
            "total_lines": total_lines,
            "line_range": shown_range
        }))
    }
}

// =============================================================================
// SELF WRITE FILE TOOL
// =============================================================================

pub struct SelfWriteFileTool;

#[async_trait]
impl Tool for SelfWriteFileTool {
    fn name(&self) -> &str {
        "self_write_file"
    }

    fn description(&self) -> &str {
        "Write to a file in YOUR OWN source code. This modifies your capabilities. Use relative paths from repository root."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path from repository root (e.g., 'crates/op-tools/src/new_tool.rs')"
                },
                "content": {
                    "type": "string",
                    "description": "Full content to write to the file"
                },
                "create_dirs": {
                    "type": "boolean",
                    "description": "Create parent directories if they don't exist (default: true)"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "file".to_string(), "write".to_string(), "modify".to_string()]
    }

    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::Elevated
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'path' argument"))?;
        
        let content = input.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'content' argument"))?;
        
        let create_dirs = input.get("create_dirs").and_then(|v| v.as_bool()).unwrap_or(true);
        
        let repo_path = get_self_repo_path()
            .ok_or_else(|| anyhow::anyhow!("OP_SELF_REPO_PATH is not set"))?;
        
        // Clean and join path
        let clean_path = path.trim_start_matches('/');
        let full_path = repo_path.join(clean_path);
        
        // Security check - ensure we're still in repo
        let canonical_repo = repo_path.canonicalize().unwrap_or(repo_path.clone());
        
        // For new files, check parent exists or will be created
        let parent = full_path.parent();
        if let Some(p) = parent {
            if p.exists() {
                let canonical_parent = p.canonicalize().unwrap_or(p.to_path_buf());
                if !canonical_parent.starts_with(&canonical_repo) {
                    return Err(anyhow::anyhow!(
                        "Path '{}' would escape the self-repository. Access denied.",
                        path
                    ));
                }
            } else if !create_dirs {
                return Err(anyhow::anyhow!("Parent directory does not exist: {:?}", p));
            }
        }
        
        // Create parent directories if needed
        if create_dirs {
            if let Some(parent) = full_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        
        // Write the file
        tokio::fs::write(&full_path, content).await?;
        
        info!("Self-modification: Wrote {} bytes to {}", content.len(), path);
        
        Ok(json!({
            "path": path,
            "bytes_written": content.len(),
            "success": true,
            "message": "File written successfully. Remember to commit your changes!"
        }))
    }
}

// =============================================================================
// SELF LIST DIRECTORY TOOL
// =============================================================================

pub struct SelfListDirectoryTool;

#[async_trait]
impl Tool for SelfListDirectoryTool {
    fn name(&self) -> &str {
        "self_list_directory"
    }

    fn description(&self) -> &str {
        "List files and directories in YOUR source code repository. Use to explore your own codebase structure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path from repository root (e.g., 'crates/op-tools/src' or '.' for root)"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "directory".to_string(), "list".to_string(), "explore".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let full_path = validate_self_path(path)?;
        
        if !full_path.is_dir() {
            return Err(anyhow::anyhow!("'{}' is not a directory", path));
        }
        
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&full_path).await?;
        
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata().await.ok();
            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            
            entries.push(json!({
                "name": name,
                "is_directory": is_dir,
                "size": if is_dir { Value::Null } else { json!(size) }
            }));
        }
        
        // Sort: directories first, then alphabetically
        entries.sort_by(|a, b| {
            let a_dir = a.get("is_directory").and_then(|v| v.as_bool()).unwrap_or(false);
            let b_dir = b.get("is_directory").and_then(|v| v.as_bool()).unwrap_or(false);
            match (a_dir, b_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    a_name.cmp(b_name)
                }
            }
        });
        
        Ok(json!({
            "path": path,
            "entries": entries,
            "count": entries.len()
        }))
    }
}

// =============================================================================
// SELF SEARCH CODE TOOL
// =============================================================================

pub struct SelfSearchCodeTool;

#[async_trait]
impl Tool for SelfSearchCodeTool {
    fn name(&self) -> &str {
        "self_search_code"
    }

    fn description(&self) -> &str {
        "Search YOUR source code for patterns. Uses ripgrep if available, falls back to grep."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Search pattern (regex supported)"
                },
                "path": {
                    "type": "string",
                    "description": "Subdirectory to search in (default: entire repository)"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Case-sensitive search (default: false)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 50)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "search".to_string(), "grep".to_string(), "find".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let pattern = input.get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'pattern' argument"))?;
        
        let repo_path = get_self_repo_path()
            .ok_or_else(|| anyhow::anyhow!("OP_SELF_REPO_PATH is not set"))?;
        
        let search_path = if let Some(subpath) = input.get("path").and_then(|v| v.as_str()) {
            validate_self_path(subpath)?
        } else {
            repo_path.clone()
        };
        
        let case_sensitive = input.get("case_sensitive").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_results = input.get("max_results").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        
        let search_path_str = search_path.to_string_lossy().to_string();
        
        // Try ripgrep first
        let mut rg_args = vec!["--line-number", "--no-heading", "--max-count", "100"];
        if !case_sensitive {
            rg_args.push("-i");
        }
        rg_args.push(pattern);
        rg_args.push(&search_path_str);
        
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("rg")
                .args(&rg_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
        ).await;
        
        let stdout = match result {
            Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout).to_string(),
            Ok(Err(_)) | Err(_) => {
                // Fall back to grep
                let mut grep_args = vec!["-rn"];
                if !case_sensitive {
                    grep_args.push("-i");
                }
                grep_args.push("--exclude-dir=target");
                grep_args.push("--exclude-dir=.git");
                grep_args.push(pattern);
                grep_args.push(&search_path_str);
                
                match Command::new("grep").args(&grep_args).output().await {
                    Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
                    Err(_) => String::new(),
                }
            }
        };
        
        let lines: Vec<&str> = stdout.lines().take(max_results).collect();
        let total_matches = stdout.lines().count();
        
        Ok(json!({
            "pattern": pattern,
            "matches": lines,
            "count": lines.len(),
            "total_matches": total_matches,
            "truncated": total_matches > max_results
        }))
    }
}

// =============================================================================
// GIT STATUS TOOL
// =============================================================================

pub struct SelfGitStatusTool;

#[async_trait]
impl Tool for SelfGitStatusTool {
    fn name(&self) -> &str {
        "self_git_status"
    }

    fn description(&self) -> &str {
        "Check the git status of YOUR source code repository. Shows modified, staged, and untracked files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "git".to_string(), "status".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let (porcelain, _, _) = run_git_command(&["status", "--porcelain=v2", "-b"]).await?;
        let (readable, _, _) = run_git_command(&["status", "-sb"]).await.unwrap_or_default();
        
        let clean = porcelain.lines().filter(|l| !l.starts_with("#")).count() == 0;
        
        Ok(json!({
            "status": readable.trim(),
            "porcelain": porcelain,
            "clean": clean
        }))
    }
}

// =============================================================================
// GIT DIFF TOOL
// =============================================================================

pub struct SelfGitDiffTool;

#[async_trait]
impl Tool for SelfGitDiffTool {
    fn name(&self) -> &str {
        "self_git_diff"
    }

    fn description(&self) -> &str {
        "View the git diff of pending changes in YOUR source code."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Show staged changes (default: false, shows unstaged)"
                },
                "path": {
                    "type": "string",
                    "description": "Optional: specific file or directory to diff"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "git".to_string(), "diff".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let path = input.get("path").and_then(|v| v.as_str());
        
        let mut args = vec!["diff"];
        if staged {
            args.push("--staged");
        }
        args.push("--color=never");
        
        let path_owned: String;
        if let Some(p) = path {
            validate_self_path(p)?;
            path_owned = p.to_string();
            args.push("--");
            args.push(&path_owned);
        }
        
        let (stdout, _, exit_code) = run_git_command(&args).await?;
        
        Ok(json!({
            "diff": stdout,
            "empty": stdout.is_empty(),
            "staged": staged,
            "exit_code": exit_code
        }))
    }
}

// =============================================================================
// GIT COMMIT TOOL
// =============================================================================

pub struct SelfGitCommitTool;

#[async_trait]
impl Tool for SelfGitCommitTool {
    fn name(&self) -> &str {
        "self_git_commit"
    }

    fn description(&self) -> &str {
        "Commit changes to YOUR source code repository. This creates a permanent record of your self-modifications."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Commit message describing the changes"
                },
                "stage_all": {
                    "type": "boolean",
                    "description": "Stage all modified files before committing (default: true)"
                }
            },
            "required": ["message"]
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "git".to_string(), "commit".to_string()]
    }

    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::Elevated
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let message = input.get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'message' argument"))?;
        
        let stage_all = input.get("stage_all").and_then(|v| v.as_bool()).unwrap_or(true);
        
        // Stage files if requested
        if stage_all {
            run_git_command(&["add", "-A"]).await?;
        }
        
        // Commit
        let (stdout, stderr, exit_code) = run_git_command(&["commit", "-m", message]).await?;
        
        if exit_code != 0 {
            return Err(anyhow::anyhow!("Commit failed: {}", stderr));
        }
        
        // Get the commit hash
        let (hash, _, _) = run_git_command(&["rev-parse", "--short", "HEAD"]).await.unwrap_or_default();
        
        info!("Self-modification committed: {} - {}", hash.trim(), message);
        
        Ok(json!({
            "success": true,
            "message": message,
            "commit_hash": hash.trim(),
            "output": stdout
        }))
    }
}

// =============================================================================
// GIT LOG TOOL
// =============================================================================

pub struct SelfGitLogTool;

#[async_trait]
impl Tool for SelfGitLogTool {
    fn name(&self) -> &str {
        "self_git_log"
    }

    fn description(&self) -> &str {
        "View the git commit history of YOUR source code."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "count": {
                    "type": "integer",
                    "description": "Number of commits to show (default: 10)"
                },
                "oneline": {
                    "type": "boolean",
                    "description": "Show one-line format (default: true)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "git".to_string(), "log".to_string(), "history".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(10);
        let oneline = input.get("oneline").and_then(|v| v.as_bool()).unwrap_or(true);
        
        let count_str = format!("-{}", count);
        let mut args = vec!["log", &count_str];
        if oneline {
            args.push("--oneline");
        }
        
        let (stdout, _, exit_code) = run_git_command(&args).await?;
        
        Ok(json!({
            "log": stdout,
            "count": count,
            "exit_code": exit_code
        }))
    }
}

// =============================================================================
// BUILD TOOL
// =============================================================================

pub struct SelfBuildTool;

#[async_trait]
impl Tool for SelfBuildTool {
    fn name(&self) -> &str {
        "self_build"
    }

    fn description(&self) -> &str {
        "Build/compile YOUR source code. Runs 'cargo build' by default. Use to verify changes compile correctly."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "release": {
                    "type": "boolean",
                    "description": "Build in release mode (default: false)"
                },
                "package": {
                    "type": "string",
                    "description": "Specific package to build (default: workspace)"
                },
                "check_only": {
                    "type": "boolean",
                    "description": "Only check for compilation errors, don't produce binaries (faster)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "build".to_string(), "compile".to_string(), "cargo".to_string()]
    }

    fn estimated_duration_ms(&self) -> Option<u64> {
        Some(60000) // 1 minute typical
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let repo_path = get_self_repo_path()
            .ok_or_else(|| anyhow::anyhow!("OP_SELF_REPO_PATH is not set"))?;
        
        let release = input.get("release").and_then(|v| v.as_bool()).unwrap_or(false);
        let check_only = input.get("check_only").and_then(|v| v.as_bool()).unwrap_or(false);
        let package = input.get("package").and_then(|v| v.as_str());
        
        let mut args = vec![if check_only { "check" } else { "build" }];
        if release {
            args.push("--release");
        }
        
        let pkg_owned: String;
        if let Some(pkg) = package {
            args.push("-p");
            pkg_owned = pkg.to_string();
            args.push(&pkg_owned);
        }
        
        info!("Building self with: cargo {}", args.join(" "));
        
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            Command::new("cargo")
                .args(&args)
                .current_dir(&repo_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
        ).await;
        
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let success = output.status.success();
                
                if success {
                    info!("Self build succeeded");
                } else {
                    warn!("Self build failed");
                }
                
                Ok(json!({
                    "success": success,
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": output.status.code(),
                    "check_only": check_only,
                    "release": release
                }))
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Build failed: {}", e)),
            Err(_) => Err(anyhow::anyhow!("Build timed out after 5 minutes")),
        }
    }
}

// =============================================================================
// DEPLOY TOOL
// =============================================================================

pub struct SelfDeployTool;

#[async_trait]
impl Tool for SelfDeployTool {
    fn name(&self) -> &str {
        "self_deploy"
    }

    fn description(&self) -> &str {
        "Deploy YOUR code. This restarts the service with updated code. Use with caution!"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": {
                    "type": "string",
                    "description": "Systemd service to restart (default: op-web)"
                },
                "build_first": {
                    "type": "boolean",
                    "description": "Build release binary before deploying (default: true)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "self"
    }

    fn tags(&self) -> Vec<String> {
        vec!["self".to_string(), "deploy".to_string(), "restart".to_string()]
    }

    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::Elevated
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let service = input.get("service").and_then(|v| v.as_str()).unwrap_or("op-web");
        let build_first = input.get("build_first").and_then(|v| v.as_bool()).unwrap_or(true);
        
        // Build first if requested
        if build_first {
            let build_result = SelfBuildTool.execute(json!({"release": true})).await?;
            if !build_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err(anyhow::anyhow!("Build failed, not deploying"));
            }
        }
        
        info!("Deploying self: restarting {}", service);

        // Restart via systemd D-Bus (not systemctl)
        let restart_result = restart_unit_dbus(service).await;

        let success = restart_result.is_ok();

        Ok(json!({
            "success": success,
            "service": service,
            "built": build_first,
            "message": if success {
                "Deployed successfully"
            } else {
                format!("Deploy may have failed: {}", restart_result.unwrap_err())
            },
            "protocol": "D-Bus"
        }))
    }
}

// =============================================================================
// TOOL REGISTRATION
// =============================================================================

/// Restart a systemd unit via D-Bus (not systemctl)
async fn restart_unit_dbus(unit: &str) -> Result<String> {
    let connection = Connection::system().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await?;

    let job_path: zbus::zvariant::OwnedObjectPath = proxy
        .call("RestartUnit", &(unit, "replace"))
        .await?;

    Ok(job_path.to_string())
}

/// Create all self-repository tools
pub fn create_self_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SelfReadFileTool) as Arc<dyn Tool>,
        Arc::new(SelfWriteFileTool),
        Arc::new(SelfListDirectoryTool),
        Arc::new(SelfSearchCodeTool),
        Arc::new(SelfGitStatusTool),
        Arc::new(SelfGitDiffTool),
        Arc::new(SelfGitCommitTool),
        Arc::new(SelfGitLogTool),
        Arc::new(SelfBuildTool),
        Arc::new(SelfDeployTool),
    ]
}

/// Get information about the self-repository for the system prompt
pub fn get_self_repo_system_context() -> Option<String> {
    let path = get_self_repo_path()?;
    
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    Some(format!(
        r#"## 🔮 SELF-AWARENESS: YOUR OWN SOURCE CODE

You have access to your own source code at `{}`.

### Self-Modification Tools
- `self_read_file` - Read your source files
- `self_write_file` - Modify your source files  
- `self_list_directory` - Explore your codebase
- `self_search_code` - Search your code
- `self_git_status` - Check git status
- `self_git_diff` - View pending changes
- `self_git_commit` - Commit changes
- `self_git_log` - View history
- `self_build` - Build yourself
- `self_deploy` - Deploy yourself

**Warning**: Changes affect your own capabilities!"#,
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_validate_self_path_rejects_traversal() {
        // This should fail since OP_SELF_REPO_PATH is not set in tests
        let result = validate_self_path("../../../etc/passwd");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_create_self_tools() {
        let tools = create_self_tools();
        assert_eq!(tools.len(), 10);
    }
}
