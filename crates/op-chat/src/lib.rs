//! op-chat: Chat Orchestration Layer
//!
//! This crate provides:
//! - ChatActor: Central message processor
//! - TrackedToolExecutor: Tool execution with tracking
//! - ForcedExecutionOrchestrator: Hallucination detection
//! - NLAdminOrchestrator: Natural language server administration
//! - SessionManager: Conversation state management

pub mod actor;
pub mod agent_tools;
pub mod chat_service;
pub mod forced_execution;
pub mod forced_tool_pipeline;
pub mod memory_loop;
pub mod nl_admin;
pub mod orchestration;
pub mod session;
pub mod system_prompt;
pub mod tool_executor;

pub use actor::{ChatActor, ChatActorConfig, ChatActorHandle, RpcRequest, RpcResponse};
pub use agent_tools::{register_all_agent_tools, register_context_agents};
pub use chat_service::ChatServiceImpl;
pub use forced_execution::{
    ForcedExecutionOrchestrator, HallucinationCheck, HallucinationIssue, HallucinationType,
    IssueSeverity, ToolCall, ToolCallResult,
};
pub use forced_tool_pipeline::{ForcedToolPipeline, PipelineResult};
pub use nl_admin::{ExtractedToolCall, NLAdminOrchestrator, NLAdminResult, ToolExecutionResult};
pub use orchestration::{
    builtin_workstacks, GrpcAgentPool, Skill, SkillMetadata, SkillRegistry, Workstack,
    WorkstackExecutor, WorkstackPhase, WorkstackResult,
};
pub use session::SessionManager;
pub use system_prompt::{create_session_with_system_prompt, generate_system_prompt};
pub use tool_executor::TrackedToolExecutor;

/// File descriptor set for tonic-reflection (includes chat + orchestration protos).
pub use orchestration::proto::FILE_DESCRIPTOR_SET;
