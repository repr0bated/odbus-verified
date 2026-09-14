//! Debugger Agent

use async_trait::async_trait;
use std::fs;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt", "/var/log"];

pub struct DebuggerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DebuggerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "debugger",
                vec!["strace", "ltrace", "gdb"],
            ),
        }
    }

    fn read_logs(&self, path: Option<&str>, lines: Option<&str>) -> Result<String, String> {
        let num_lines_str = lines.unwrap_or("100");
        validation::validate_args(num_lines_str)?;
        let num_lines: usize = num_lines_str
            .parse()
            .map_err(|_| "Invalid line count".to_string())?;

        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            let content = fs::read_to_string(&validated_path)
                .map_err(|e| format!("Failed to read {}: {}", validated_path, e))?;
            let collected: Vec<&str> = content.lines().collect();
            let start = collected.len().saturating_sub(num_lines);
            let filtered = collected[start..].join("\n");
            Ok(format!(
                "Log output (last {} lines):\n{}",
                num_lines, filtered
            ))
        } else {
            Err("Log file path required".to_string())
        }
    }

    fn journalctl(&self, unit: Option<&str>) -> Result<String, String> {
        if let Some(u) = unit {
            validation::validate_args(u)?;
        }

        // Attempt direct file reads from common log paths since journalctl is a D-Bus bypass
        let fallback_paths = [
            "/var/log/syslog",
            "/var/log/messages",
            "/var/log/kern.log",
            "/var/log/auth.log",
        ];

        let mut output = String::new();
        for path in &fallback_paths {
            if let Ok(content) = fs::read_to_string(path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(100);
                output.push_str(&format!(
                    "--- {} (last 100 lines) ---\n{}\n",
                    path,
                    lines[start..].join("\n")
                ));
            }
        }

        if output.is_empty() {
            return Err(
                "No readable log files found in /var/log. journalctl is not available via /proc."
                    .to_string(),
            );
        }

        if let Some(u) = unit {
            let filtered: String = output
                .lines()
                .filter(|line| line.contains(u))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!(
                "Journal output (filtered by '{}'):\n{}",
                u, filtered
            ))
        } else {
            Ok(format!("Journal output (direct file reads):\n{}", output))
        }
    }

    fn process_info(&self, pid: Option<&str>) -> Result<String, String> {
        if let Some(p) = pid {
            validation::validate_args(p)?;
        }

        let mut entries = Vec::new();
        let proc_dir = fs::read_dir("/proc").map_err(|e| format!("Failed to read /proc: {}", e))?;

        for entry in proc_dir {
            let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.chars().all(|c| c.is_ascii_digit()) {
                let status_path = format!("/proc/{}/status", name_str);
                let cmdline_path = format!("/proc/{}/cmdline", name_str);
                let status = fs::read_to_string(&status_path).unwrap_or_default();
                let cmdline = fs::read_to_string(&cmdline_path)
                    .unwrap_or_default()
                    .replace('\0', " ");
                let proc_name = status
                    .lines()
                    .find(|l| l.starts_with("Name:"))
                    .map(|l| l.trim_start_matches("Name:").trim())
                    .unwrap_or("?");
                let entry_line = format!(
                    "{} {} {}",
                    name_str,
                    proc_name,
                    if cmdline.is_empty() {
                        "[kernel]"
                    } else {
                        &cmdline
                    }
                );
                if let Some(p) = pid {
                    if entry_line.contains(p) {
                        entries.push(entry_line);
                    }
                } else {
                    entries.push(entry_line);
                }
            }
        }

        let header = "PID NAME CMDLINE";
        entries.insert(0, header.to_string());
        Ok(format!("Process info:\n{}", entries.join("\n")))
    }
}

#[async_trait]
impl AgentTrait for DebuggerAgent {
    fn agent_type(&self) -> &str {
        "debugger"
    }
    fn name(&self) -> &str {
        "Debugger"
    }
    fn description(&self) -> &str {
        "Debug logs and process analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "logs".to_string(),
            "journal".to_string(),
            "process".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "logs" => self.read_logs(task.path.as_deref(), task.args.as_deref()),
            "journal" => self.journalctl(task.path.as_deref()),
            "process" => self.process_info(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
