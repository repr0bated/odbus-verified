//! Agent code generation infrastructure
//!
//! Provides tools for:
//! - Parsing markdown agent definitions
//! - Generating Rust D-Bus agent code
//! - Creating agent specifications

pub mod md_parser;
pub mod template;

pub use md_parser::{AgentDefinition, ParsedCapabilities, parse_agent_markdown};
pub use template::{AgentOperation, AgentTemplate, generate_agent_code};
