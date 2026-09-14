//! Agent code generation infrastructure
//!
//! Provides tools for:
//! - Parsing markdown agent definitions
//! - Generating Rust D-Bus agent code
//! - Creating agent specifications

pub mod md_parser;
pub mod template;

pub use md_parser::{parse_agent_markdown, AgentDefinition, ParsedCapabilities};
pub use template::{generate_agent_code, AgentOperation, AgentTemplate};
