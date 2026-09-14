//! Markdown agent definition parser
//!
//! Parses markdown files with YAML frontmatter containing:
//! - name: agent identifier
//! - description: agent description
//! - model: LLM model to use
//! - Markdown content with capabilities, purpose, etc.

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Parsed agent definition from markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Agent name (from frontmatter)
    pub name: String,

    /// Description (from frontmatter)
    pub description: String,

    /// Model to use (from frontmatter)
    pub model: String,

    /// Purpose section content
    pub purpose: Option<String>,

    /// Parsed capabilities
    pub capabilities: ParsedCapabilities,

    /// Behavioral traits
    pub behavioral_traits: Vec<String>,

    /// Knowledge base items
    pub knowledge_base: Vec<String>,

    /// Example interactions
    pub examples: Vec<String>,

    /// Raw markdown content
    pub raw_content: String,
}

/// Parsed capabilities from markdown
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedCapabilities {
    /// Category groups (e.g., "Modern Python Features", "Testing & QA")
    pub categories: HashMap<String, Vec<String>>,

    /// All capability items flattened
    pub items: Vec<String>,

    /// Detected operations based on capabilities
    pub detected_operations: Vec<DetectedOperation>,
}

/// An operation detected from capability analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedOperation {
    /// Operation name
    pub name: String,

    /// Description
    pub description: String,

    /// Associated commands
    pub commands: Vec<String>,

    /// Risk level (inferred)
    pub risk: String,
}

/// YAML frontmatter structure
#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default = "default_model")]
    model: String,
}

fn default_model() -> String {
    "sonnet".to_string()
}

/// Parse an agent markdown file
pub fn parse_agent_markdown(content: &str) -> Result<AgentDefinition> {
    // Extract YAML frontmatter
    let frontmatter_re = Regex::new(r"(?s)^---\n(.*?)\n---\n(.*)$").unwrap();

    let captures = frontmatter_re
        .captures(content)
        .context("No YAML frontmatter found")?;

    let yaml_content = captures.get(1).unwrap().as_str();
    let markdown_content = captures.get(2).unwrap().as_str();

    // Parse frontmatter
    let frontmatter: Frontmatter =
        serde_yaml::from_str(yaml_content).context("Failed to parse YAML frontmatter")?;

    // Parse markdown sections
    let purpose = extract_section(markdown_content, "Purpose");
    let capabilities = parse_capabilities(markdown_content);
    let behavioral_traits = extract_list_section(markdown_content, "Behavioral Traits");
    let knowledge_base = extract_list_section(markdown_content, "Knowledge Base");
    let examples = extract_list_section(markdown_content, "Example Interactions");

    Ok(AgentDefinition {
        name: frontmatter.name,
        description: frontmatter.description,
        model: frontmatter.model,
        purpose,
        capabilities,
        behavioral_traits,
        knowledge_base,
        examples,
        raw_content: content.to_string(),
    })
}

/// Parse a markdown file from disk
pub async fn parse_agent_file(path: &Path) -> Result<AgentDefinition> {
    let content = tokio::fs::read_to_string(path)
        .await
        .context(format!("Failed to read file: {:?}", path))?;

    parse_agent_markdown(&content)
}

/// Extract a section by heading
fn extract_section(content: &str, heading: &str) -> Option<String> {
    let pattern = format!(r"(?s)##\s*{}\s*\n(.*?)(?:\n##|\z)", regex::escape(heading));
    let re = Regex::new(&pattern).ok()?;

    re.captures(content)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// Extract a list section (bullet points)
fn extract_list_section(content: &str, heading: &str) -> Vec<String> {
    let section = extract_section(content, heading).unwrap_or_default();

    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('-') || trimmed.starts_with('*') {
                Some(trimmed[1..].trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Parse capabilities section
fn parse_capabilities(content: &str) -> ParsedCapabilities {
    let mut capabilities = ParsedCapabilities::default();

    // Find the Capabilities section
    let cap_section = extract_section(content, "Capabilities").unwrap_or_default();

    // Parse subsections (### headings)
    let subsection_re = Regex::new(r"(?s)###\s*([^\n]+)\n(.*?)(?:###|\z)").unwrap();

    for cap in subsection_re.captures_iter(&cap_section) {
        let category = cap
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let items_text = cap.get(2).map(|m| m.as_str()).unwrap_or("");

        let items: Vec<String> = items_text
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('-') || trimmed.starts_with('*') {
                    Some(trimmed[1..].trim().to_string())
                } else {
                    None
                }
            })
            .collect();

        capabilities.items.extend(items.clone());
        capabilities.categories.insert(category, items);
    }

    // Detect operations from capabilities
    capabilities.detected_operations = detect_operations(&capabilities);

    capabilities
}

/// Detect operations based on capability keywords
fn detect_operations(capabilities: &ParsedCapabilities) -> Vec<DetectedOperation> {
    let mut operations = Vec::new();
    let items_lower: Vec<String> = capabilities
        .items
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    // Check for common operation patterns
    let operation_patterns = [
        (
            "execute",
            vec!["execute", "run", "execute code", "script"],
            "execute code",
        ),
        (
            "test",
            vec!["test", "testing", "pytest", "jest", "cargo test"],
            "run tests",
        ),
        (
            "lint",
            vec!["lint", "linting", "eslint", "pylint", "clippy"],
            "run linters",
        ),
        (
            "format",
            vec!["format", "formatting", "black", "prettier", "rustfmt"],
            "format code",
        ),
        (
            "build",
            vec!["build", "compile", "compilation"],
            "build/compile code",
        ),
        (
            "analyze",
            vec!["analyze", "analysis", "static analysis"],
            "analyze code",
        ),
        (
            "check",
            vec!["check", "type check", "mypy", "pyright"],
            "type checking",
        ),
        ("deploy", vec!["deploy", "deployment"], "deploy code"),
        (
            "query",
            vec!["query", "sql", "database query"],
            "execute queries",
        ),
        ("review", vec!["review", "code review"], "review code"),
        (
            "scan",
            vec!["scan", "security scan", "vulnerability"],
            "security scanning",
        ),
        (
            "profile",
            vec!["profile", "profiling", "performance"],
            "performance profiling",
        ),
    ];

    for (op_name, keywords, description) in operation_patterns {
        let found = keywords
            .iter()
            .any(|kw| items_lower.iter().any(|item| item.contains(kw)));

        if found {
            operations.push(DetectedOperation {
                name: op_name.to_string(),
                description: description.to_string(),
                commands: Vec::new(), // Filled in by template generator
                risk: infer_risk(op_name),
            });
        }
    }

    operations
}

/// Infer risk level based on operation type
fn infer_risk(operation: &str) -> String {
    match operation {
        "execute" | "deploy" => "high".to_string(),
        "build" | "test" => "medium".to_string(),
        "lint" | "format" | "check" | "analyze" | "review" | "scan" => "low".to_string(),
        _ => "medium".to_string(),
    }
}

/// Determine security profile category from agent definition
pub fn determine_category(definition: &AgentDefinition) -> crate::security::ProfileCategory {
    let name_lower = definition.name.to_lowercase();
    let desc_lower = definition.description.to_lowercase();

    // Code execution agents (language-pro, shell)
    if name_lower.ends_with("-pro")
        && (name_lower.contains("python")
            || name_lower.contains("rust")
            || name_lower.contains("go")
            || name_lower.contains("javascript")
            || name_lower.contains("typescript")
            || name_lower.contains("java")
            || name_lower.contains("c-pro")
            || name_lower.contains("cpp")
            || name_lower.contains("php")
            || name_lower.contains("ruby")
            || name_lower.contains("bash")
            || name_lower.contains("shell"))
    {
        return crate::security::ProfileCategory::CodeExecution;
    }

    // Orchestration agents
    if name_lower.contains("orchestrat")
        || name_lower.contains("manager")
        || desc_lower.contains("coordinate")
        || desc_lower.contains("orchestrat")
    {
        return crate::security::ProfileCategory::Orchestration;
    }

    // Content generation agents
    if name_lower.contains("doc")
        || name_lower.contains("tutorial")
        || name_lower.contains("content")
        || name_lower.contains("mermaid")
        || desc_lower.contains("documentation")
        || desc_lower.contains("content generation")
    {
        return crate::security::ProfileCategory::ContentGeneration;
    }

    // Default to read-only analysis
    crate::security::ProfileCategory::ReadOnlyAnalysis
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MD: &str = r#"---
name: python-pro
description: Master Python 3.12+ development
model: sonnet
---

You are a Python expert.

## Purpose
Expert Python developer mastering Python 3.12+ features.

## Capabilities

### Modern Python Features
- Python 3.12+ features
- Advanced async/await patterns
- Type hints and generics

### Testing & QA
- Comprehensive testing with pytest
- Property-based testing with Hypothesis

## Behavioral Traits
- Follows PEP 8
- Uses type hints throughout
- Writes extensive tests

## Example Interactions
- "Help me migrate from pip to uv"
- "Optimize this Python code"
"#;

    #[test]
    fn test_parse_agent_markdown() {
        let result = parse_agent_markdown(SAMPLE_MD).unwrap();

        assert_eq!(result.name, "python-pro");
        assert_eq!(result.model, "sonnet");
        assert!(result.purpose.is_some());
        assert!(!result.capabilities.items.is_empty());
        assert!(!result.behavioral_traits.is_empty());
        assert!(!result.examples.is_empty());
    }

    #[test]
    fn test_determine_category() {
        let def = parse_agent_markdown(SAMPLE_MD).unwrap();
        let category = determine_category(&def);
        assert_eq!(category, crate::security::ProfileCategory::CodeExecution);
    }
}
