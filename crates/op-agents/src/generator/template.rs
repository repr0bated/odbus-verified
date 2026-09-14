//! Agent code template generator
//!
//! Generates Rust source code for D-Bus agents based on:
//! - Agent definitions from markdown
//! - Security profiles
//! - Operation specifications

use crate::generator::md_parser::{determine_category, AgentDefinition};
use crate::security::ProfileCategory;
use std::collections::HashSet;

/// Agent template for code generation
#[derive(Debug, Clone)]
pub struct AgentTemplate {
    /// Agent type identifier (e.g., "python-pro")
    pub agent_type: String,

    /// Rust struct name (e.g., "PythonProAgent")
    pub struct_name: String,

    /// D-Bus interface name (e.g., "org.dbusmcp.Agent.PythonPro")
    pub interface_name: String,

    /// D-Bus path (e.g., "/org/dbusmcp/Agent/PythonPro")
    pub dbus_path: String,

    /// Agent description
    pub description: String,

    /// Security profile category
    pub category: ProfileCategory,

    /// Allowed commands
    pub allowed_commands: HashSet<String>,

    /// Operations this agent supports
    pub operations: Vec<AgentOperation>,
}

/// Agent operation definition
#[derive(Debug, Clone)]
pub struct AgentOperation {
    /// Operation name (e.g., "execute", "test")
    pub name: String,

    /// Operation description
    pub description: String,

    /// Primary command to run
    pub command: String,

    /// Default arguments
    pub default_args: Vec<String>,

    /// Whether path is required
    pub requires_path: bool,

    /// Whether this operation requires approval
    pub requires_approval: bool,
}

impl AgentTemplate {
    /// Create a template from an agent definition
    pub fn from_definition(def: &AgentDefinition) -> Self {
        let category = determine_category(def);
        let agent_type = def.name.clone();

        // Generate Rust-safe names
        let struct_name = to_pascal_case(&agent_type) + "Agent";
        let interface_name = format!("org.dbusmcp.Agent.{}", to_pascal_case(&agent_type));
        let dbus_path = format!("/org/dbusmcp/Agent/{}", to_pascal_case(&agent_type));

        // Determine allowed commands based on agent type
        let allowed_commands = infer_commands(&agent_type, &category);

        // Generate operations
        let operations = generate_operations(
            &agent_type,
            &category,
            &def.capabilities.detected_operations,
        );

        Self {
            agent_type,
            struct_name,
            interface_name,
            dbus_path,
            description: def.description.clone(),
            category,
            allowed_commands,
            operations,
        }
    }
}

/// Convert kebab-case to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Convert kebab-case to snake_case
fn to_snake_case(s: &str) -> String {
    s.replace('-', "_")
}

/// Infer allowed commands from agent type
fn infer_commands(agent_type: &str, category: &ProfileCategory) -> HashSet<String> {
    let mut commands = HashSet::new();

    match category {
        ProfileCategory::CodeExecution => {
            match agent_type {
                "python-pro" => {
                    commands.extend(
                        [
                            "python", "python3", "pip", "pip3", "uv", "ruff", "pytest", "mypy",
                            "black", "isort", "flake8",
                        ]
                        .map(String::from),
                    );
                }
                "rust-pro" => {
                    commands.extend(
                        ["cargo", "rustc", "rustfmt", "clippy-driver", "rustup"].map(String::from),
                    );
                }
                "golang-pro" => {
                    commands.extend(
                        ["go", "gofmt", "golint", "staticcheck", "gopls"].map(String::from),
                    );
                }
                "javascript-pro" | "typescript-pro" => {
                    commands.extend(
                        [
                            "node", "npm", "npx", "yarn", "pnpm", "eslint", "prettier", "jest",
                            "vitest", "tsc",
                        ]
                        .map(String::from),
                    );
                }
                "java-pro" => {
                    commands.extend(["java", "javac", "mvn", "gradle", "ant"].map(String::from));
                }
                "csharp-pro" => {
                    commands.extend(["dotnet", "csc", "msbuild", "nuget"].map(String::from));
                }
                "ruby-pro" => {
                    commands.extend(
                        ["ruby", "gem", "bundle", "rake", "rspec", "rubocop"].map(String::from),
                    );
                }
                "php-pro" => {
                    commands.extend(
                        ["php", "composer", "phpunit", "phpstan", "psalm"].map(String::from),
                    );
                }
                "c-pro" => {
                    commands.extend(
                        ["gcc", "clang", "make", "cmake", "gdb", "valgrind"].map(String::from),
                    );
                }
                "cpp-pro" => {
                    commands.extend(
                        ["g++", "clang++", "make", "cmake", "gdb", "valgrind"].map(String::from),
                    );
                }
                "scala-pro" => {
                    commands.extend(["scala", "scalac", "sbt", "mill"].map(String::from));
                }
                "julia-pro" => {
                    commands.extend(["julia"].map(String::from));
                }
                "elixir-pro" => {
                    commands.extend(["elixir", "mix", "iex"].map(String::from));
                }
                "bash-pro" | "posix-shell-pro" => {
                    commands.extend(["bash", "sh", "shellcheck"].map(String::from));
                }
                "sql-pro" => {
                    commands.extend(["psql", "mysql", "sqlite3", "sqlfluff"].map(String::from));
                }
                _ => {
                    // Default code execution commands
                    commands.insert("echo".to_string());
                }
            }
        }
        ProfileCategory::ReadOnlyAnalysis => {
            commands.extend(["rg", "grep", "wc", "cloc", "tokei", "diff", "git"].map(String::from));
        }
        ProfileCategory::ContentGeneration => {
            commands.extend(["cat", "echo", "wc"].map(String::from));
        }
        ProfileCategory::Orchestration => {
            // Orchestration agents typically don't run commands directly
        }
    }

    commands
}

/// Generate operations based on agent type
fn generate_operations(
    agent_type: &str,
    category: &ProfileCategory,
    detected: &[super::md_parser::DetectedOperation],
) -> Vec<AgentOperation> {
    let mut operations = Vec::new();

    match category {
        ProfileCategory::CodeExecution => {
            match agent_type {
                "python-pro" => {
                    operations.push(AgentOperation {
                        name: "run".to_string(),
                        description: "Execute Python script".to_string(),
                        command: "python3".to_string(),
                        default_args: vec![],
                        requires_path: true,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "test".to_string(),
                        description: "Run pytest".to_string(),
                        command: "pytest".to_string(),
                        default_args: vec!["-v".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "lint".to_string(),
                        description: "Run ruff linter".to_string(),
                        command: "ruff".to_string(),
                        default_args: vec!["check".to_string()],
                        requires_path: true,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "format".to_string(),
                        description: "Format with black".to_string(),
                        command: "black".to_string(),
                        default_args: vec!["--check".to_string(), "--diff".to_string()],
                        requires_path: true,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "typecheck".to_string(),
                        description: "Run mypy type checker".to_string(),
                        command: "mypy".to_string(),
                        default_args: vec![],
                        requires_path: true,
                        requires_approval: false,
                    });
                }
                "rust-pro" => {
                    operations.push(AgentOperation {
                        name: "check".to_string(),
                        description: "Run cargo check".to_string(),
                        command: "cargo".to_string(),
                        default_args: vec!["check".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "build".to_string(),
                        description: "Build the project".to_string(),
                        command: "cargo".to_string(),
                        default_args: vec!["build".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "test".to_string(),
                        description: "Run tests".to_string(),
                        command: "cargo".to_string(),
                        default_args: vec!["test".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "clippy".to_string(),
                        description: "Run clippy linter".to_string(),
                        command: "cargo".to_string(),
                        default_args: vec![
                            "clippy".to_string(),
                            "--".to_string(),
                            "-D".to_string(),
                            "warnings".to_string(),
                        ],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "format".to_string(),
                        description: "Check formatting".to_string(),
                        command: "cargo".to_string(),
                        default_args: vec!["fmt".to_string(), "--check".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                }
                "golang-pro" => {
                    operations.push(AgentOperation {
                        name: "build".to_string(),
                        description: "Build Go project".to_string(),
                        command: "go".to_string(),
                        default_args: vec!["build".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "test".to_string(),
                        description: "Run Go tests".to_string(),
                        command: "go".to_string(),
                        default_args: vec!["test".to_string(), "./...".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "fmt".to_string(),
                        description: "Format Go code".to_string(),
                        command: "gofmt".to_string(),
                        default_args: vec!["-l".to_string(), ".".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "vet".to_string(),
                        description: "Run Go vet".to_string(),
                        command: "go".to_string(),
                        default_args: vec!["vet".to_string(), "./...".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                }
                "javascript-pro" | "typescript-pro" => {
                    operations.push(AgentOperation {
                        name: "run".to_string(),
                        description: "Execute script".to_string(),
                        command: "node".to_string(),
                        default_args: vec![],
                        requires_path: true,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "test".to_string(),
                        description: "Run tests".to_string(),
                        command: "npm".to_string(),
                        default_args: vec!["test".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "lint".to_string(),
                        description: "Run ESLint".to_string(),
                        command: "npx".to_string(),
                        default_args: vec!["eslint".to_string(), ".".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                    operations.push(AgentOperation {
                        name: "build".to_string(),
                        description: "Build project".to_string(),
                        command: "npm".to_string(),
                        default_args: vec!["run".to_string(), "build".to_string()],
                        requires_path: false,
                        requires_approval: false,
                    });
                }
                _ => {
                    // Generate generic operations from detected ones
                    for detected_op in detected {
                        operations.push(AgentOperation {
                            name: detected_op.name.clone(),
                            description: detected_op.description.clone(),
                            command: "echo".to_string(),
                            default_args: vec![format!("Operation: {}", detected_op.name)],
                            requires_path: false,
                            requires_approval: detected_op.risk == "high",
                        });
                    }
                }
            }
        }
        ProfileCategory::ReadOnlyAnalysis => {
            operations.push(AgentOperation {
                name: "analyze".to_string(),
                description: "Analyze code".to_string(),
                command: "rg".to_string(),
                default_args: vec![],
                requires_path: true,
                requires_approval: false,
            });
            operations.push(AgentOperation {
                name: "count".to_string(),
                description: "Count lines of code".to_string(),
                command: "wc".to_string(),
                default_args: vec!["-l".to_string()],
                requires_path: true,
                requires_approval: false,
            });
        }
        ProfileCategory::ContentGeneration => {
            operations.push(AgentOperation {
                name: "generate".to_string(),
                description: "Generate content".to_string(),
                command: "echo".to_string(),
                default_args: vec![],
                requires_path: false,
                requires_approval: false,
            });
        }
        ProfileCategory::Orchestration => {
            operations.push(AgentOperation {
                name: "coordinate".to_string(),
                description: "Coordinate subagents".to_string(),
                command: "echo".to_string(),
                default_args: vec![],
                requires_path: false,
                requires_approval: false,
            });
        }
    }

    operations
}

/// Generate Rust source code for a D-Bus agent
pub fn generate_agent_code(template: &AgentTemplate) -> String {
    let _snake_name = to_snake_case(&template.agent_type);
    let allowed_cmds: Vec<&str> = template
        .allowed_commands
        .iter()
        .map(|s| s.as_str())
        .collect();

    let mut operations_impl = String::new();
    let mut match_arms = String::new();

    for op in &template.operations {
        let op_snake = to_snake_case(&op.name);
        let default_args_str = if op.default_args.is_empty() {
            "vec![]".to_string()
        } else {
            format!(
                "vec![{}]",
                op.default_args
                    .iter()
                    .map(|a| format!("\"{}\".to_string()", a))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        operations_impl.push_str(&format!(
            r#"
    fn {op_snake}(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {{
        let mut cmd = Command::new("{command}");
        let default_args = {default_args};
        
        for arg in default_args {{
            cmd.arg(arg);
        }}
        
        if let Some(a) = args {{
            self.validate_args(a)?;
            for arg in a.split_whitespace() {{
                cmd.arg(arg);
            }}
        }}
        
        {path_handling}
        
        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run {command}: {{}}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        if output.status.success() {{
            Ok(format!("{op_name} succeeded\nstdout: {{}}\nstderr: {{}}", stdout, stderr))
        }} else {{
            Ok(format!("{op_name} failed\nstdout: {{}}\nstderr: {{}}", stdout, stderr))
        }}
    }}
"#,
            op_snake = op_snake,
            command = op.command,
            default_args = default_args_str,
            op_name = op.name,
            path_handling = if op.requires_path {
                r#"if let Some(p) = path {
            let validated_path = self.validate_path(p)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required".to_string());
        }"#
            } else {
                r#"if let Some(p) = path {
            let validated_path = self.validate_path(p)?;
            cmd.current_dir(validated_path);
        }"#
            }
        ));

        match_arms.push_str(&format!(
            r#"            "{}" => self.{op_snake}(task.path.as_deref(), task.args.as_deref()),
"#,
            op.name,
            op_snake = op_snake
        ));
    }

    format!(
        r#"//! {description}
//! 
//! Auto-generated D-Bus agent for {agent_type}

use serde::Deserialize;
use std::process::Command;
use uuid::Uuid;
use zbus::{{connection::Builder, interface, object_server::SignalEmitter}};

// Security configuration
const ALLOWED_DIRECTORIES: &[&str] = &["/tmp", "/home", "/opt"];
const FORBIDDEN_CHARS: &[char] = &[
    '$', '`', ';', '&', '|', '>', '<', '(', ')', '{{', '}}', '\n', '\r',
];
const MAX_PATH_LENGTH: usize = 4096;
const ALLOWED_COMMANDS: &[&str] = &[{allowed_commands}];

#[derive(Debug, Deserialize)]
struct {struct_name}Task {{
    #[serde(rename = "type")]
    task_type: String,
    operation: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    args: Option<String>,
}}

struct {struct_name} {{
    agent_id: String,
}}

#[interface(name = "{interface_name}")]
impl {struct_name} {{
    /// Execute a task safely
    async fn execute(&self, mut task_json: String) -> zbus::fdo::Result<String> {{
        println!("[{{}}] Received task: {{}}", self.agent_id, task_json);

        let task: {struct_name}Task = match serde_json::from_str(&task_json) {{
            Ok(t) => t,
            Err(e) => {{
                return Err(zbus::fdo::Error::InvalidArgs(format!(
                    "Failed to parse task: {{}}",
                    e
                )));
            }}
        }};

        if task.task_type != "{agent_type}" {{
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Unknown task type: {{}}",
                task.task_type
            )));
        }}

        println!(
            "[{{}}] Operation: {{}} on path: {{:?}}",
            self.agent_id, task.operation, task.path
        );

        let result = match task.operation.as_str() {{
{match_arms}            _ => Err(format!("Unknown operation: {{}}", task.operation)),
        }};

        match result {{
            Ok(data) => {{
                let response = simd_json::json!({{
                    "success": true,
                    "operation": task.operation,
                    "data": data,
                }});
                Ok(response.to_string())
            }}
            Err(e) => Err(zbus::fdo::Error::Failed(e)),
        }}
    }}

    /// Get agent status
    async fn get_status(&self) -> zbus::fdo::Result<String> {{
        Ok(format!("{struct_name} {{}} is running", self.agent_id))
    }}

    /// List supported operations
    async fn list_operations(&self) -> zbus::fdo::Result<String> {{
        let ops = simd_json::json!({{
            "operations": [{operations_list}]
        }});
        Ok(ops.to_string())
    }}

    /// Signal emitted when task completes
    #[zbus(signal)]
    async fn task_completed(signal_emitter: &SignalEmitter<'_>, result: String)
        -> zbus::Result<()>;
}}

impl {struct_name} {{
    fn new(agent_id: String) -> Self {{
        Self {{ agent_id }}
    }}

    fn validate_path(&self, path: &str) -> Result<String, String> {{
        if path.len() > MAX_PATH_LENGTH {{
            return Err("Path exceeds maximum length".to_string());
        }}

        for forbidden_char in FORBIDDEN_CHARS {{
            if path.contains(*forbidden_char) {{
                return Err(format!(
                    "Path contains forbidden character: {{:?}}",
                    forbidden_char
                ));
            }}
        }}

        let mut is_allowed = false;
        for allowed in ALLOWED_DIRECTORIES {{
            if path.starts_with(allowed) {{
                is_allowed = true;
                break;
            }}
        }}

        if !is_allowed {{
            return Err(format!(
                "Path must be within allowed directories: {{:?}}",
                ALLOWED_DIRECTORIES
            ));
        }}

        Ok(path.to_string())
    }}

    fn validate_args(&self, args: &str) -> Result<(), String> {{
        if args.len() > 256 {{
            return Err("Args string too long".to_string());
        }}

        for forbidden_char in FORBIDDEN_CHARS {{
            if args.contains(*forbidden_char) {{
                return Err(format!(
                    "Args contains forbidden character: {{:?}}",
                    forbidden_char
                ));
            }}
        }}

        Ok(())
    }}
{operations_impl}
}}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    let args: Vec<String> = std::env::args().collect();

    let agent_id = if args.len() > 1 {{
        args[1].clone()
    }} else {{
        format!("{agent_type}-{{}}", Uuid::new_v4().to_string()[..8].to_string())
    }};

    println!("Starting {struct_name}: {{}}", agent_id);

    let agent = {struct_name}::new(agent_id.clone());

    let path = format!("{dbus_path}/{{}}", agent_id.replace('-', "_"));
    let service_name = format!("{interface_name}.{{}}", agent_id.replace('-', "_"));

    let _conn = Builder::system()?
        .name(service_name.as_str())?
        .serve_at(path.as_str(), agent)?
        .build()
        .await?;

    println!("{struct_name} {{}} ready on D-Bus", agent_id);
    println!("Service: {{}}", service_name);
    println!("Path: {{}}", path);

    std::future::pending::<()>().await;

    Ok(())
}}
"#,
        description = template.description,
        agent_type = template.agent_type,
        struct_name = template.struct_name,
        interface_name = template.interface_name,
        dbus_path = template.dbus_path,
        allowed_commands = allowed_cmds
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", "),
        match_arms = match_arms,
        operations_impl = operations_impl,
        operations_list = template
            .operations
            .iter()
            .map(|o| format!("\"{}\"", o.name))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("python-pro"), "PythonPro");
        assert_eq!(to_pascal_case("code-reviewer"), "CodeReviewer");
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("python-pro"), "python_pro");
        assert_eq!(to_snake_case("code-reviewer"), "code_reviewer");
    }
}
