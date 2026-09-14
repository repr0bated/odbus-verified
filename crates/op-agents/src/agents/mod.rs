#![allow(dead_code)]
//! Agent implementations organized by category
//!
//! Categories:
//! - language: Programming language specific agents (python-pro, rust-pro, etc.)
//! - infrastructure: DevOps and infrastructure agents
//! - analysis: Code review and analysis agents
//! - database: Database-related agents
//! - content: Documentation and content generation agents
//! - orchestration: Meta-agents that coordinate others
//! - architecture: Software architecture agents (backend, frontend, graphql)
//! - operations: SRE and operations agents (incident response, troubleshooting)
//! - aiml: AI/ML specialized agents (ai-engineer, ml-engineer, etc.)
//! - webframeworks: Web framework agents (django, fastapi, temporal)
//! - mobile: Mobile development agents (flutter, ios, android)
//! - security: Security-focused coding agents
//! - business: Business and operations agents
//! - seo: SEO and content marketing agents
//! - specialty: Niche domain agents (snowball, gaming, finance, etc.)

pub mod advise;
pub mod aiml;
pub mod analysis;
pub mod architecture;
pub mod base;
pub mod business;
pub mod compliance;
pub mod content;
pub mod database;
pub mod infrastructure;
pub mod language;
pub mod mobile;
pub mod operations;
pub mod orchestration;
pub mod security;
pub mod seo;
pub mod specialty;
pub mod webframeworks;

// Re-export common types
pub use base::{AgentContext, AgentTask, AgentTrait, TaskResult};
