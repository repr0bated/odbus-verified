//! Orchestration Module - Unified execution coordination
//!
//! This module provides:
//! - **Error handling**: Comprehensive, typed errors with retry support
//! - **gRPC Agent Pool**: Production-ready persistent connections
//! - **Workstack Executor**: Multi-phase execution with rollback
//! - **Skills**: Knowledge/capability augmentation for tools
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                 ChatActor (Brain)                                │
//! │                                                                  │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │              WorkstackExecutor                             │  │
//! │  │                                                            │  │
//! │  │  Phases → Tools + Agents (via GrpcAgentPool)               │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! │                           │                                      │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │                 GrpcAgentPool                              │  │
//! │  │                                                            │  │
//! │  │  Run-on-connection:                                        │  │
//! │  │  rust_pro | backend_architect | sequential_thinking        │  │
//! │  │  memory | context_manager                                  │  │
//! │  │                                                            │  │
//! │  │  On-demand (lazy):                                         │  │
//! │  │  python_pro | debugger | mem0 | search_specialist | ...   │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use op_chat::orchestration::{GrpcAgentPool, WorkstackExecutor, Workstack};
//!
//! // Create agent pool
//! let pool = Arc::new(GrpcAgentPool::from_env());
//!
//! // Initialize session
//! pool.init_session("session-123", "cursor", None).await?;
//!
//! // Execute operations
//! let result = pool.execute("session-123", "rust_pro", "check", json!({"path": "."})).await?;
//!
//! // Use workstack executor for multi-phase tasks
//! let executor = WorkstackExecutor::new(pool.clone(), tool_executor);
//! executor.register_all(builtin_workstacks()).await;
//! let result = executor.execute("session-123", "full_stack_feature", variables, event_tx).await?;
//! ```

pub mod error;
pub mod grpc_pool;
pub mod proto;
pub mod services;
pub mod skills;
pub mod workstack_executor;

// Re-exports
pub use error::{ErrorCode, OrchestrationError, OrchestrationResult, ResultExt, RetryInfo};
pub use grpc_pool::{
    AgentHealth, AgentOperation, AgentOperationResult, AgentPoolConfig, CircuitState,
    GrpcAgentPool, PoolStatus, StreamChunk, StreamType,
};
pub use skills::{Skill, SkillMetadata, SkillRegistry};
pub use workstack_executor::{
    AgentResult, ExecutionStatus, PhaseResult, PhaseStatus, PhaseToolCall, ToolExecutor,
    ToolResult, Workstack, WorkstackEvent, WorkstackExecutor, WorkstackInfo, WorkstackPhase,
    WorkstackResult,
};

// ============================================================================
// BUILTIN WORKSTACKS
// ============================================================================

use simd_json::json;

/// Get all builtin workstacks
pub fn builtin_workstacks() -> Vec<Workstack> {
    vec![
        // Full Stack Feature Development
        Workstack::new(
            "full_stack_feature",
            "Full Stack Feature",
            "Develop a full-stack feature with analysis, design, implementation, and testing",
        )
        .with_category("development")
        .with_timeout(600)
        .with_phase(WorkstackPhase {
            id: "analyze".to_string(),
            name: "Analyze Requirements".to_string(),
            description: "Analyze feature requirements".to_string(),
            tools: vec![PhaseToolCall {
                tool: "file_read".to_string(),
                arguments: json!({ "path": "${requirements_file}" }),
                store_as: Some("requirements".to_string()),
                retries: 1,
            }],
            agents: vec!["backend_architect".to_string()],
            agent_operation: Some("analyze".to_string()),
            agent_arguments: Some(json!({ "context": "${requirements}" })),
            depends_on: vec![],
            condition: None,
            rollback: vec![],
            continue_on_failure: false,
            timeout_secs: 120,
        })
        .with_phase(WorkstackPhase {
            id: "design".to_string(),
            name: "Design Solution".to_string(),
            description: "Design solution using sequential thinking".to_string(),
            tools: vec![],
            agents: vec!["sequential_thinking".to_string()],
            agent_operation: Some("think_stream".to_string()),
            agent_arguments: Some(
                json!({ "problem": "Design implementation for feature", "max_steps": 5 }),
            ),
            depends_on: vec!["analyze".to_string()],
            condition: Some("analyze.success".to_string()),
            rollback: vec![],
            continue_on_failure: false,
            timeout_secs: 180,
        })
        .with_phase(WorkstackPhase {
            id: "implement".to_string(),
            name: "Implement Feature".to_string(),
            description: "Generate and write code".to_string(),
            tools: vec![],
            agents: vec!["rust_pro".to_string()],
            agent_operation: Some("build".to_string()),
            agent_arguments: Some(json!({ "path": ".", "release": false })),
            depends_on: vec!["design".to_string()],
            condition: None,
            rollback: vec![],
            continue_on_failure: false,
            timeout_secs: 300,
        })
        .with_phase(WorkstackPhase {
            id: "test".to_string(),
            name: "Test Implementation".to_string(),
            description: "Run tests".to_string(),
            tools: vec![],
            agents: vec!["rust_pro".to_string()],
            agent_operation: Some("test".to_string()),
            agent_arguments: Some(json!({ "path": "." })),
            depends_on: vec!["implement".to_string()],
            condition: None,
            rollback: vec![],
            continue_on_failure: true,
            timeout_secs: 180,
        })
        .with_phase(WorkstackPhase {
            id: "save_context".to_string(),
            name: "Save Context".to_string(),
            description: "Save progress to context manager".to_string(),
            tools: vec![],
            agents: vec!["context_manager".to_string()],
            agent_operation: Some("save".to_string()),
            agent_arguments: Some(json!({
                "name": "feature-${feature_name}",
                "content": "Feature implementation complete",
                "tags": ["feature", "implementation"]
            })),
            depends_on: vec!["test".to_string()],
            condition: None,
            rollback: vec![],
            continue_on_failure: true,
            timeout_secs: 30,
        }),
        // Code Review Workstack
        Workstack::new(
            "code_review",
            "Code Review",
            "Comprehensive code review with multiple perspectives",
        )
        .with_category("review")
        .with_timeout(300)
        .with_phase(WorkstackPhase {
            id: "clippy".to_string(),
            name: "Run Clippy".to_string(),
            description: "Run Clippy lints".to_string(),
            tools: vec![],
            agents: vec!["rust_pro".to_string()],
            agent_operation: Some("clippy".to_string()),
            agent_arguments: Some(json!({ "path": "${path}" })),
            depends_on: vec![],
            condition: None,
            rollback: vec![],
            continue_on_failure: true,
            timeout_secs: 120,
        })
        .with_phase(WorkstackPhase {
            id: "arch_review".to_string(),
            name: "Architecture Review".to_string(),
            description: "Review architecture".to_string(),
            tools: vec![],
            agents: vec!["backend_architect".to_string()],
            agent_operation: Some("review".to_string()),
            agent_arguments: Some(json!({ "path": "${path}", "scope": "crate" })),
            depends_on: vec![],
            condition: None,
            rollback: vec![],
            continue_on_failure: true,
            timeout_secs: 120,
        })
        .with_phase(WorkstackPhase {
            id: "consolidate".to_string(),
            name: "Consolidate Reviews".to_string(),
            description: "Combine all review feedback".to_string(),
            tools: vec![],
            agents: vec!["sequential_thinking".to_string()],
            agent_operation: Some("conclude".to_string()),
            agent_arguments: None,
            depends_on: vec!["clippy".to_string(), "arch_review".to_string()],
            condition: None,
            rollback: vec![],
            continue_on_failure: false,
            timeout_secs: 60,
        }),
        // OVS Network Setup (example with tools + rollback)
        Workstack::new(
            "ovs_network_setup",
            "OVS Network Setup",
            "Set up OVS bridge with ports and flows",
        )
        .with_category("network")
        .with_timeout(120)
        .with_phase(WorkstackPhase {
            id: "create_bridge".to_string(),
            name: "Create Bridge".to_string(),
            description: "Create the OVS bridge".to_string(),
            tools: vec![PhaseToolCall {
                tool: "ovs_create_bridge".to_string(),
                arguments: json!({ "bridge": "${bridge_name}" }),
                store_as: Some("bridge_result".to_string()),
                retries: 1,
            }],
            agents: vec![],
            agent_operation: None,
            agent_arguments: None,
            depends_on: vec![],
            condition: None,
            rollback: vec![PhaseToolCall {
                tool: "ovs_delete_bridge".to_string(),
                arguments: json!({ "bridge": "${bridge_name}" }),
                store_as: None,
                retries: 0,
            }],
            continue_on_failure: false,
            timeout_secs: 60,
        })
        .with_phase(WorkstackPhase {
            id: "verify".to_string(),
            name: "Verify Setup".to_string(),
            description: "Verify the bridge configuration".to_string(),
            tools: vec![PhaseToolCall {
                tool: "ovs_list_bridges".to_string(),
                arguments: json!({}),
                store_as: Some("final_bridges".to_string()),
                retries: 0,
            }],
            agents: vec![],
            agent_operation: None,
            agent_arguments: None,
            depends_on: vec!["create_bridge".to_string()],
            condition: None,
            rollback: vec![],
            continue_on_failure: false,
            timeout_secs: 30,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_workstacks() {
        let workstacks = builtin_workstacks();
        assert!(!workstacks.is_empty());

        // Check that full_stack_feature exists
        let full_stack = workstacks.iter().find(|w| w.id == "full_stack_feature");
        assert!(full_stack.is_some());

        let ws = full_stack.unwrap();
        assert!(!ws.phases.is_empty());
        assert!(!ws.required_agents.is_empty());
    }

    #[test]
    fn test_error_types() {
        let err = OrchestrationError::agent_timeout(
            "rust_pro",
            "build",
            std::time::Duration::from_secs(30),
        );
        assert_eq!(err.code, ErrorCode::AgentTimeout);
        assert!(err.is_retryable());

        let err2 = OrchestrationError::invalid_arguments("bad args");
        assert_eq!(err2.code, ErrorCode::InvalidArguments);
        assert!(!err2.is_retryable());
    }
}
