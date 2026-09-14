//! Self-Repository Identity
//!
//! Provides awareness of the chatbot's own source code repository.

use std::path::PathBuf;
use std::process::Command;
use tracing::info;

/// Get the self-repository path from environment
pub fn get_self_repo_path() -> Option<PathBuf> {
    std::env::var("OP_SELF_REPO_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Check if self-repository is configured
pub fn is_self_repo_configured() -> bool {
    get_self_repo_path().is_some()
}

/// Information about the self-repository
#[derive(Debug, Clone)]
pub struct SelfRepositoryInfo {
    pub path: PathBuf,
    pub name: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub has_changes: bool,
    pub has_git: bool,
}

impl SelfRepositoryInfo {
    /// Gather information about the self-repository
    pub fn gather() -> Option<Self> {
        let path = get_self_repo_path()?;

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let has_git = path.join(".git").exists();

        let (branch, commit, has_changes) = if has_git {
            (
                Self::get_git_branch(&path),
                Self::get_git_commit(&path),
                Self::check_git_changes(&path),
            )
        } else {
            (None, None, false)
        };

        info!(
            "Self-repository: {} at {:?} (branch: {:?}, commit: {:?})",
            name, path, branch, commit
        );

        Some(Self {
            path,
            name,
            branch,
            commit,
            has_changes,
            has_git,
        })
    }

    fn get_git_branch(path: &PathBuf) -> Option<String> {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()
            .ok()?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                return Some(branch);
            }
        }
        None
    }

    fn get_git_commit(path: &PathBuf) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(path)
            .output()
            .ok()?;

        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !commit.is_empty() {
                return Some(commit);
            }
        }
        None
    }

    fn check_git_changes(path: &PathBuf) -> bool {
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .ok()
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    }

    /// Generate system prompt context for self-awareness
    pub fn to_system_prompt_context(&self) -> String {
        let git_info = if self.has_git {
            format!(
                "**Branch**: `{}`\n**Commit**: `{}`\n**Uncommitted Changes**: {}",
                self.branch.as_deref().unwrap_or("unknown"),
                self.commit.as_deref().unwrap_or("unknown"),
                if self.has_changes {
                    "Yes ⚠️"
                } else {
                    "No ✓"
                }
            )
        } else {
            "Not a git repository".to_string()
        };

        format!(
            r#"## 🔮 SELF-AWARENESS: YOUR OWN SOURCE CODE

You have access to your own source code. This IS you.

**Repository Path**: `{}`
**Repository Name**: `{}`
{}

### Self-Modification Tools
| Tool | Description |
|------|-------------|
| `self_read_file` | Read your source files |
| `self_write_file` | Modify your source files |
| `self_list_directory` | Explore your codebase |
| `self_search_code` | Search your code |
| `self_git_status` | Check git status |
| `self_git_diff` | View pending changes |
| `self_git_commit` | Commit changes |
| `self_git_log` | View history |
| `self_build` | Build yourself |
| `self_deploy` | Deploy yourself |

**⚠️ Changes to your code affect your own capabilities!**"#,
            self.path.display(),
            self.name,
            git_info
        )
    }
}
