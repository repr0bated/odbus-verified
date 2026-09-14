//! Skills System - Knowledge augmentation for tool execution
//!
//! Skills provide domain-specific knowledge and capabilities that can be
//! activated to enhance tool execution. They inject context, validate inputs,
//! and post-process outputs.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::prelude::{ValueAsContainer, ValueAsMutContainer, ValueAsScalar, ValueObjectAccess};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use tracing::{info, warn};

/// Skill metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Human-readable description
    pub description: String,
    /// Category (e.g., "debugging", "optimization", "security")
    pub category: String,
    /// Tags for discovery
    #[serde(default)]
    pub tags: Vec<String>,
    /// Required tools for this skill
    #[serde(default)]
    pub required_tools: Vec<String>,
    /// Version
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl Default for SkillMetadata {
    fn default() -> Self {
        Self {
            description: String::new(),
            category: "general".to_string(),
            tags: Vec::new(),
            required_tools: Vec::new(),
            version: default_version(),
        }
    }
}

/// Context injected by an active skill
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillContext {
    /// Additional system prompt content
    #[serde(default)]
    pub system_prompt_additions: Vec<String>,
    /// Pre-execution hooks (tool transformations)
    #[serde(default)]
    pub input_transformations: HashMap<String, Value>,
    /// Post-execution hooks (output transformations)
    #[serde(default)]
    pub output_transformations: HashMap<String, Value>,
    /// Variables available to the skill
    #[serde(default)]
    pub variables: HashMap<String, Value>,
    /// Constraints to enforce
    #[serde(default)]
    pub constraints: Vec<SkillConstraint>,
}

/// Constraint enforced by a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConstraint {
    /// Constraint type
    pub constraint_type: ConstraintType,
    /// Target (tool name, path pattern, etc.)
    pub target: String,
    /// Constraint value
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    /// Require specific argument values
    RequireArgument,
    /// Forbid certain argument values
    ForbidArgument,
    /// Require tool to be called before another
    RequireBefore,
    /// Require tool to be called after another
    RequireAfter,
    /// Limit execution count
    MaxExecutions,
    /// Require confirmation
    RequireConfirmation,
}

/// A skill definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Unique skill name
    pub name: String,
    /// Metadata
    pub metadata: SkillMetadata,
    /// Context provided when skill is active
    pub context: SkillContext,
    /// Whether skill is currently active
    #[serde(default)]
    pub active: bool,
    /// Priority (higher = applied first)
    #[serde(default)]
    pub priority: i32,
}

impl Skill {
    /// Create a new skill
    pub fn new(name: &str, description: &str, category: &str) -> Self {
        Self {
            name: name.to_string(),
            metadata: SkillMetadata {
                description: description.to_string(),
                category: category.to_string(),
                ..Default::default()
            },
            context: SkillContext::default(),
            active: false,
            priority: 0,
        }
    }

    /// Add a system prompt addition
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.context
            .system_prompt_additions
            .push(prompt.to_string());
        self
    }

    /// Add a required tool
    pub fn with_required_tool(mut self, tool: &str) -> Self {
        self.metadata.required_tools.push(tool.to_string());
        self
    }

    /// Add a constraint
    pub fn with_constraint(mut self, constraint: SkillConstraint) -> Self {
        self.context.constraints.push(constraint);
        self
    }

    /// Add a variable
    pub fn with_variable(mut self, name: &str, value: Value) -> Self {
        self.context.variables.insert(name.to_string(), value);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Activate the skill
    pub fn activate(&mut self) {
        self.active = true;
        info!(skill = %self.name, "Skill activated");
    }

    /// Deactivate the skill
    pub fn deactivate(&mut self) {
        self.active = false;
        info!(skill = %self.name, "Skill deactivated");
    }

    /// Transform input arguments based on skill context
    pub fn transform_input(&self, tool_name: &str, mut args: Value) -> Value {
        if let Some(transform) = self.context.input_transformations.get(tool_name) {
            if let (Some(args_obj), Some(transform_obj)) =
                (args.as_object_mut(), transform.as_object())
            {
                for (key, value) in transform_obj {
                    if !args_obj.contains_key(key) {
                        args_obj.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        args
    }

    /// Transform output based on skill context
    pub fn transform_output(&self, tool_name: &str, output: Value) -> Value {
        if let Some(_transform) = self.context.output_transformations.get(tool_name) {
            // Apply output transformation logic
            // For now, just return as-is
            output
        } else {
            output
        }
    }

    /// Check if constraints are satisfied
    pub fn check_constraints(&self, tool_name: &str, args: &Value) -> Result<()> {
        for constraint in &self.context.constraints {
            if constraint.target != tool_name && constraint.target != "*" {
                continue;
            }

            match constraint.constraint_type {
                ConstraintType::RequireArgument => {
                    if let Some(required_key) = constraint.value.as_str() {
                        if args.get(required_key).is_none() {
                            anyhow::bail!(
                                "Skill '{}' requires argument '{}' for tool '{}'",
                                self.name,
                                required_key,
                                tool_name
                            );
                        }
                    }
                }
                ConstraintType::ForbidArgument => {
                    if let Some(forbidden_key) = constraint.value.as_str() {
                        if args.get(forbidden_key).is_some() {
                            anyhow::bail!(
                                "Skill '{}' forbids argument '{}' for tool '{}'",
                                self.name,
                                forbidden_key,
                                tool_name
                            );
                        }
                    }
                }
                ConstraintType::RequireConfirmation => {
                    warn!(
                        skill = %self.name,
                        tool = %tool_name,
                        "Tool requires confirmation (not implemented)"
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Registry of available skills
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Create registry with default skills
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_default_skills();
        registry
    }

    /// Register a skill
    pub fn register(&mut self, skill: Skill) {
        info!(skill = %skill.name, category = %skill.metadata.category, "Registering skill");
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Get a skill by name
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get a mutable skill by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Skill> {
        self.skills.get_mut(name)
    }

    /// List all skills
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// List active skills (sorted by priority)
    pub fn active_skills(&self) -> Vec<&Skill> {
        let mut active: Vec<_> = self.skills.values().filter(|s| s.active).collect();
        active.sort_by_key(|b| std::cmp::Reverse(b.priority));
        active
    }

    /// Activate a skill by name
    pub fn activate(&mut self, name: &str) -> Result<()> {
        if let Some(skill) = self.skills.get_mut(name) {
            skill.activate();
            Ok(())
        } else {
            anyhow::bail!("Skill not found: {}", name)
        }
    }

    /// Deactivate a skill by name
    pub fn deactivate(&mut self, name: &str) -> Result<()> {
        if let Some(skill) = self.skills.get_mut(name) {
            skill.deactivate();
            Ok(())
        } else {
            anyhow::bail!("Skill not found: {}", name)
        }
    }

    /// Get combined context from all active skills
    #[allow(dead_code)]
    pub fn combined_context(&self) -> SkillContext {
        let mut combined = SkillContext::default();

        for skill in self.active_skills() {
            combined
                .system_prompt_additions
                .extend(skill.context.system_prompt_additions.clone());
            combined
                .input_transformations
                .extend(skill.context.input_transformations.clone());
            combined
                .output_transformations
                .extend(skill.context.output_transformations.clone());
            combined.variables.extend(skill.context.variables.clone());
            combined
                .constraints
                .extend(skill.context.constraints.clone());
        }

        combined
    }

    /// Register default skills
    fn register_default_skills(&mut self) {
        // Python debugging skill
        self.register(
            Skill::new("python_debugging", "Enhanced Python debugging capabilities", "debugging")
                .with_prompt("When debugging Python code, use pdb breakpoints and inspect variables systematically.")
                .with_required_tool("agent_python_pro")
                .with_variable("debug_level", json!("verbose"))
                .with_priority(10)
        );

        // Rust optimization skill
        self.register(
            Skill::new(
                "rust_optimization",
                "Rust performance optimization guidance",
                "optimization",
            )
            .with_prompt(
                "Focus on zero-cost abstractions, avoid unnecessary allocations, use iterators.",
            )
            .with_required_tool("agent_rust_pro")
            .with_priority(10),
        );

        // Security audit skill
        self.register(
            Skill::new("security_audit", "Security-focused code review", "security")
                .with_prompt(
                    "Check for: SQL injection, XSS, CSRF, path traversal, secrets in code.",
                )
                .with_constraint(SkillConstraint {
                    constraint_type: ConstraintType::RequireConfirmation,
                    target: "*".to_string(),
                    value: json!(true),
                })
                .with_priority(20),
        );

        // TDD skill
        self.register(
            Skill::new("tdd_workflow", "Test-Driven Development workflow", "methodology")
                .with_prompt("Follow Red-Green-Refactor: 1) Write failing test, 2) Minimal code to pass, 3) Refactor.")
                .with_priority(15)
        );

        // Documentation skill
        self.register(
            Skill::new(
                "documentation",
                "Comprehensive documentation generation",
                "documentation",
            )
            .with_prompt("Generate clear docstrings, README sections, and API documentation.")
            .with_priority(5),
        );

        // OVS networking skill
        self.register(
            Skill::new(
                "ovs_networking",
                "Open vSwitch networking expertise",
                "networking",
            )
            .with_prompt("Use OVSDB JSON-RPC for bridge/port management. Never use ovs-vsctl CLI.")
            .with_required_tool("ovs_create_bridge")
            .with_required_tool("ovs_list_bridges")
            .with_priority(10),
        );

        // Systemd management skill
        self.register(
            Skill::new(
                "systemd_management",
                "Systemd service management expertise",
                "system",
            )
            .with_prompt("Use D-Bus for systemd operations. Check service status before changes.")
            .with_required_tool("systemd_status")
            .with_constraint(SkillConstraint {
                constraint_type: ConstraintType::RequireBefore,
                target: "systemd_restart".to_string(),
                value: json!("systemd_status"),
            })
            .with_priority(10),
        );
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_creation() {
        let skill = Skill::new("test_skill", "A test skill", "testing")
            .with_prompt("Test prompt")
            .with_priority(5);

        assert_eq!(skill.name, "test_skill");
        assert_eq!(skill.metadata.category, "testing");
        assert_eq!(skill.priority, 5);
        assert!(!skill.active);
    }

    #[test]
    fn test_skill_registry() {
        let mut registry = SkillRegistry::with_defaults();

        assert!(registry.get("python_debugging").is_some());
        assert!(registry.get("nonexistent").is_none());

        registry.activate("python_debugging").unwrap();
        assert_eq!(registry.active_skills().len(), 1);

        registry.deactivate("python_debugging").unwrap();
        assert_eq!(registry.active_skills().len(), 0);
    }

    #[test]
    fn test_constraint_checking() {
        let skill = Skill::new("test", "test", "test").with_constraint(SkillConstraint {
            constraint_type: ConstraintType::RequireArgument,
            target: "test_tool".to_string(),
            value: json!("required_arg"),
        });

        // Should fail - missing required argument
        let result = skill.check_constraints("test_tool", &json!({"other": "value"}));
        assert!(result.is_err());

        // Should pass - has required argument
        let result = skill.check_constraints("test_tool", &json!({"required_arg": "value"}));
        assert!(result.is_ok());
    }
}
