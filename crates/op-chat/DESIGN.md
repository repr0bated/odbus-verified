# op-chat Design Document

**Project**: op-chat - Chat Orchestration Layer for op-dbus-v2  
**Status**: Design Phase  
**Version**: 1.0  
**Date**: 2026-02-16  
**Authors**: op-dbus-v2 team

---

## Executive Summary

`op-chat` is the **intelligent orchestration layer** that transforms natural language into verified system operations. It serves as the "brain" of op-dbus-v2, coordinating between LLMs, tools, agents, and system protocols to enable safe, auditable, natural language server administration.

### Core Innovation

**Zero-Trust LLM Architecture**: Unlike traditional chatbots that trust LLM output, op-chat enforces a "forced tool execution" model where:
- LLMs cannot output text directly to users
- All actions must go through verified tool calls
- Claims are validated against execution logs
- Hallucinations are detected and blocked

This architecture makes it **impossible** for the LLM to hallucinate system changes.

### Key Capabilities

1. **Natural Language → System Operations**: "Create OVS bridge br0" → actual bridge creation
2. **Multi-Agent Orchestration**: Coordinate specialized agents (rust_pro, memory, sequential_thinking)
3. **Complex Workflows**: Multi-phase workstacks with rollback support
4. **Full Auditability**: Every action tracked with who/what/when/result
5. **MCP Server**: Expose capabilities via Model Context Protocol

---

## Table of Contents

1. [Vision & Goals](#1-vision--goals)
2. [Requirements](#2-requirements)
3. [Architecture Overview](#3-architecture-overview)
4. [Core Components](#4-core-components)
5. [Anti-Hallucination System](#5-anti-hallucination-system)
6. [Agent Orchestration](#6-agent-orchestration)
7. [Workstack System](#7-workstack-system)
8. [Protocol Design](#8-protocol-design)
9. [Data Models](#9-data-models)
10. [Implementation Plan](#10-implementation-plan)
11. [Testing Strategy](#11-testing-strategy)
12. [Performance Targets](#12-performance-targets)
13. [Security Model](#13-security-model)
14. [Deployment Architecture](#14-deployment-architecture)
15. [Future Roadmap](#15-future-roadmap)

---

## 1. Vision & Goals

### 1.1 Vision Statement

Enable **safe, natural language server administration** where users can express intent in plain English and have the system execute verified operations with full accountability.

### 1.2 Primary Goals

1. **Safety First**: Prevent LLM hallucinations from causing system changes
2. **Natural Interface**: Users shouldn't need to know CLI syntax or API details
3. **Full Auditability**: Every action logged with complete context
4. **Extensibility**: Easy to add new tools, agents, and workflows
5. **Performance**: Sub-second response for simple operations, streaming for complex ones

### 1.3 Non-Goals

- **Not a general chatbot**: Focused on system administration, not conversation
- **Not autonomous**: Requires user intent, doesn't make decisions independently
- **Not a replacement for experts**: Augments expertise, doesn't replace it

### 1.4 Success Metrics

- **Hallucination Rate**: < 0.1% (1 in 1000 operations)
- **Intent Recognition**: > 95% accuracy for common operations
- **Response Time**: < 1s for simple operations, < 5s for complex
- **Uptime**: 99.9% availability
- **Audit Coverage**: 100% of operations tracked

---

## 2. Requirements

### 2.1 Functional Requirements

#### FR1: Natural Language Processing
- **FR1.1**: Accept natural language input in English
- **FR1.2**: Parse user intent and map to tool operations
- **FR1.3**: Handle ambiguous requests with clarification
- **FR1.4**: Support multi-step operations in single request

#### FR2: Tool Execution
- **FR2.1**: Execute tools from registry with argument validation
- **FR2.2**: Track all executions with full context
- **FR2.3**: Rate limit per session (60/min, 500/hour)
- **FR2.4**: Support synchronous and streaming execution

#### FR3: Anti-Hallucination
- **FR3.1**: Force all LLM output through response tools
- **FR3.2**: Verify claimed actions against execution log
- **FR3.3**: Detect and block 5 hallucination types
- **FR3.4**: Provide detailed hallucination reports

#### FR4: Agent Orchestration
- **FR4.1**: Manage persistent gRPC connections to agents
- **FR4.2**: Start/stop run-on-connection agents per session
- **FR4.3**: Health check agents every 30 seconds
- **FR4.4**: Automatic reconnection with exponential backoff

#### FR5: Workstack Execution
- **FR5.1**: Execute multi-phase workflows with dependencies
- **FR5.2**: Support rollback on failure
- **FR5.3**: Variable substitution in tool arguments
- **FR5.4**: Progress tracking and streaming updates

#### FR6: Session Management
- **FR6.1**: Create and manage chat sessions
- **FR6.2**: Store conversation history
- **FR6.3**: Support WireGuard authentication
- **FR6.4**: Distinguish controller vs regular users

#### FR7: MCP Server
- **FR7.1**: Expose tools via MCP protocol
- **FR7.2**: Provide prompts (workstacks) and resources (skills)
- **FR7.3**: Support gRPC transport
- **FR7.4**: Handle streaming responses

### 2.2 Non-Functional Requirements

#### NFR1: Performance
- **NFR1.1**: Handle 100 concurrent sessions
- **NFR1.2**: < 1s latency for simple operations
- **NFR1.3**: < 100ms overhead for tool execution tracking
- **NFR1.4**: Support 1000+ tools in registry

#### NFR2: Reliability
- **NFR2.1**: 99.9% uptime
- **NFR2.2**: Graceful degradation when agents unavailable
- **NFR2.3**: Circuit breaker for failing agents
- **NFR2.4**: No data loss on crash (persistent sessions)

#### NFR3: Security
- **NFR3.1**: All operations authenticated
- **NFR3.2**: Role-based access control (controller vs user)
- **NFR3.3**: Complete audit trail
- **NFR3.4**: Input validation on all tool arguments

#### NFR4: Maintainability
- **NFR4.1**: Modular architecture with clear boundaries
- **NFR4.2**: Comprehensive logging and tracing
- **NFR4.3**: Self-documenting code with examples
- **NFR4.4**: < 10% code duplication

#### NFR5: Scalability
- **NFR5.1**: Horizontal scaling via multiple instances
- **NFR5.2**: Stateless design (sessions in external store)
- **NFR5.3**: Connection pooling for agents
- **NFR5.4**: Efficient resource usage (< 100MB per instance)

### 2.3 Constraints

#### Technical Constraints
- **TC1**: Must use Rust 2021 edition
- **TC2**: Must use gRPC for agent communication
- **TC3**: Must use SIMD JSON for performance
- **TC4**: Must integrate with existing op-tools registry
- **TC5**: Must support D-Bus native protocols

#### Business Constraints
- **BC1**: Open source (workspace license)
- **BC2**: No external API dependencies (self-contained)
- **BC3**: Must work offline (no cloud services)

#### Operational Constraints
- **OC1**: Must run on Linux (systemd/dinit)
- **OC2**: Must support x86_64 and aarch64
- **OC3**: Must integrate with WireGuard gateway

---

## 3. Architecture Overview

### 3.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    PRESENTATION LAYER                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │   Web    │  │   MCP    │  │   CLI    │  │  gRPC    │   │
│  │  (HTTP)  │  │ (gRPC)   │  │ (stdin)  │  │ (direct) │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
└───────┼─────────────┼─────────────┼─────────────┼──────────┘
        │             │             │             │
        └─────────────┴─────────────┴─────────────┘
                      │
        ┌─────────────▼──────────────────────────────────────┐
        │         ORCHESTRATION LAYER (op-chat)              │
        │                                                    │
        │  ┌──────────────────────────────────────────┐    │
        │  │         ChatActor (Message Bus)          │    │
        │  │  - Request routing                       │    │
        │  │  - Session coordination                  │    │
        │  │  - Response aggregation                  │    │
        │  └──────────────┬───────────────────────────┘    │
        │                 │                                 │
        │  ┌──────────────┴───────────────┐                │
        │  │                               │                │
        │  ▼                               ▼                │
        │  ┌─────────────────┐  ┌──────────────────┐       │
        │  │  NLAdmin        │  │  Workstack       │       │
        │  │  Orchestrator   │  │  Executor        │       │
        │  │  - Intent parse │  │  - Multi-phase   │       │
        │  │  - Tool extract │  │  - Rollback      │       │
        │  │  - LLM interact │  │  - Dependencies  │       │
        │  └────────┬────────┘  └────────┬─────────┘       │
        │           │                    │                  │
        │           ▼                    ▼                  │
        │  ┌────────────────────────────────────────┐      │
        │  │    TrackedToolExecutor                 │      │
        │  │    - Rate limiting                     │      │
        │  │    - Audit logging                     │      │
        │  │    - Metrics tracking                  │      │
        │  └────────┬───────────────────────────────┘      │
        │           │                                       │
        │           ▼                                       │
        │  ┌────────────────────────────────────────┐      │
        │  │  ForcedExecution Orchestrator          │      │
        │  │  - Hallucination detection             │      │
        │  │  - Claim verification                  │      │
        │  │  - Response tool enforcement           │      │
        │  └────────────────────────────────────────┘      │
        └────────────────────────────────────────────────────┘
                      │                    │
        ┌─────────────┴────────┐  ┌────────┴──────────────┐
        │                      │  │                       │
        ▼                      ▼  ▼                       ▼
┌──────────────┐      ┌────────────────┐      ┌──────────────┐
│ TOOL LAYER   │      │  AGENT LAYER   │      │ SYSTEM LAYER │
│              │      │                │      │              │
│ ┌──────────┐ │      │ ┌────────────┐ │      │ ┌──────────┐ │
│ │ OVS      │ │      │ │ rust_pro   │ │      │ │ D-Bus    │ │
│ │ Systemd  │ │      │ │ memory     │ │      │ │ Netlink  │ │
│ │ Network  │ │      │ │ seq_think  │ │      │ │ OVSDB    │ │
│ │ Container│ │      │ │ backend    │ │      │ │ Kernel   │ │
│ └──────────┘ │      │ │ context    │ │      │ └──────────┘ │
└──────────────┘      │ └────────────┘ │      └──────────────┘
                      │                │
                      │ GrpcAgentPool  │
                      │ - Pooling      │
                      │ - Health check │
                      │ - Circuit break│
                      └────────────────┘
```

### 3.2 Design Principles

#### Principle 1: Actor Model for Concurrency
- Single-threaded message processing (no locks)
- Message passing via channels
- Clear ownership and lifecycle

#### Principle 2: Zero-Trust LLM Output
- Never trust LLM claims
- Always verify against execution log
- Force structured output (tools only)

#### Principle 3: gRPC-First Communication
- Binary protocol for performance
- Streaming for long operations
- Type-safe with protobuf

#### Principle 4: Fail-Safe Defaults
- Rate limiting enabled by default
- Tracking enabled by default
- Rollback on failure by default

#### Principle 5: Observable by Design
- Structured logging (tracing)
- Metrics at every layer
- Complete audit trail

### 3.3 Technology Stack

#### Core Runtime
- **Language**: Rust 2021
- **Async Runtime**: Tokio (full features)
- **Serialization**: simd-json (performance)
- **Error Handling**: anyhow + thiserror

#### Communication
- **RPC Framework**: Tonic (gRPC)
- **Protocol**: Protocol Buffers 3
- **Streaming**: tokio-stream
- **D-Bus**: zbus

#### Data Management
- **Sessions**: In-memory (RwLock<HashMap>)
- **Audit Log**: op-execution-tracker
- **Metrics**: Atomic counters + aggregation

#### Observability
- **Logging**: tracing + tracing-subscriber
- **Metrics**: Custom (atomic counters)
- **Health**: gRPC health checks

---

## 4. Core Components

### 4.1 ChatActor - Central Message Bus

#### Responsibility
Single point of coordination for all requests. Routes messages to appropriate handlers and aggregates responses.

#### Design Pattern
Actor model with message passing:
```rust
pub struct ChatActor {
    config: ChatActorConfig,
    tool_registry: Arc<ToolRegistry>,
    session_manager: Arc<SessionManager>,
    executor: Arc<TrackedToolExecutor>,
    tracker: Arc<ExecutionTracker>,
    receiver: mpsc::Receiver<ActorMessage>,
}

struct ActorMessage {
    request: RpcRequest,
    response_tx: oneshot::Sender<RpcResponse>,
}
```

#### Message Flow
```
Client → Handle.send(request) 
    → mpsc::channel 
    → ChatActor.run() loop
    → handle_request()
    → oneshot::channel
    → Client receives response
```

#### Key Decisions
- **Why Actor Model?**: Eliminates lock contention, clear ownership
- **Why mpsc?**: Multiple producers (clients), single consumer (actor)
- **Why oneshot?**: Each request gets exactly one response

#### Interface
```rust
pub enum RpcRequest {
    ListTools { offset: Option<usize>, limit: Option<usize> },
    ExecuteTool { name: String, arguments: Value, session_id: Option<String> },
    GetTool { name: String },
    Chat { message: String, session_id: String, model: Option<String> },
    GetHistory { limit: Option<usize> },
    GetStats,
    Health,
    Introspect { service: String, bus_type: Option<String> },
    DbusCall { /* ... */ },
}

pub struct RpcResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
    pub metadata: HashMap<String, Value>,
}
```

### 4.2 SessionManager - Conversation State

#### Responsibility
Manage chat sessions with message history and authentication context.

#### Design
```rust
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ChatSession>>>,
}

pub struct ChatSession {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, Value>,
    
    // Authentication
    pub auth_session_id: Option<String>,      // From WireGuard
    pub is_controller: bool,                   // Elevated privileges
    pub peer_pubkey: Option<String>,          // Crypto identity
}
```

#### Key Decisions
- **Why in-memory?**: Fast access, simple implementation (Phase 1)
- **Why RwLock?**: Many reads (get session), few writes (add message)
- **Future**: Persistent storage (Redis/PostgreSQL) for Phase 2

#### Operations
- `create_session()` - Generate UUID, initialize
- `get_session(id)` - Retrieve by ID
- `add_message(id, message)` - Append to history
- `authenticated(...)` - Create with WireGuard context

### 4.3 SystemPromptGenerator - LLM Context

#### Responsibility
Generate comprehensive system prompts with anti-hallucination rules and tool definitions.

#### Structure
```
SYSTEM PROMPT = FIXED_PART + CUSTOM_PART + TOOL_DEFINITIONS

FIXED_PART (immutable):
  - Anti-hallucination rules
  - Forced tool execution architecture
  - Forbidden CLI commands
  - Protocol usage guidelines

CUSTOM_PART (mutable):
  - Loaded from /etc/op-dbus/custom-prompt.txt
  - Or ./custom-prompt.txt (dev)
  - Or CUSTOM_SYSTEM_PROMPT env var

TOOL_DEFINITIONS:
  - Generated from ToolRegistry
  - JSON schema for each tool
  - Examples and constraints
```

#### Key Decisions
- **Why split fixed/custom?**: Safety (fixed) + flexibility (custom)
- **Why file-based custom?**: Easy admin editing without code changes
- **Why include tools?**: LLM needs to know what's available

---

## 5. Anti-Hallucination System

### 5.1 Problem Statement

**LLMs hallucinate**. They will confidently claim to have performed actions they never executed:
- "I have created the bridge br0" (but didn't call the tool)
- "The service is now running" (but didn't start it)
- "I've updated the configuration" (but didn't write the file)

This is **unacceptable** for system administration. A hallucinated claim could lead operators to believe a system is configured when it's not, causing outages or security issues.

### 5.2 Solution: Forced Tool Execution Architecture

#### Core Concept
**The LLM cannot output text directly to the user**. All communication must go through verified tools.

#### Architecture
```
┌─────────────────────────────────────────────────────────┐
│ User: "Create OVS bridge br0"                           │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│ LLM MUST respond with tool_calls:                       │
│                                                         │
│ [                                                       │
│   {                                                     │
│     "name": "ovs_create_bridge",                        │
│     "arguments": {"name": "br0"}                        │
│   },                                                    │
│   {                                                     │
│     "name": "respond_to_user",                          │
│     "arguments": {                                      │
│       "message": "Created OVS bridge br0",              │
│       "message_type": "success",                        │
│       "related_actions": ["ovs_create_bridge"]          │
│     }                                                   │
│   }                                                     │
│ ]                                                       │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│ ForcedExecutionOrchestrator                             │
│                                                         │
│ 1. Execute: ovs_create_bridge → SUCCESS                │
│ 2. Execute: respond_to_user → SUCCESS                  │
│                                                         │
│ 3. Verify:                                              │
│    ✓ ovs_create_bridge was actually called             │
│    ✓ respond_to_user references it                     │
│    ✓ No raw text output                                │
│                                                         │
│ Result: VERIFIED ✓                                      │
└─────────────────────────────────────────────────────────┘
```

### 5.3 Component Design

#### ForcedExecutionOrchestrator
```rust
pub struct ForcedExecutionOrchestrator {
    executor: Arc<TrackedToolExecutor>,
    current_turn_tools: Arc<RwLock<Vec<String>>>,
}

impl ForcedExecutionOrchestrator {
    // Start new turn - clear tracking
    pub async fn start_turn(&self);
    
    // Execute tools and verify against hallucination
    pub async fn execute_and_verify(
        &self,
        llm_response: &Value,
        session_id: &str,
    ) -> Result<(Vec<ToolCallResult>, HallucinationCheck)>;
    
    // Verify response after execution
    pub async fn verify_response(
        &self,
        llm_response: &Value,
        executed_tools: &[String],
    ) -> HallucinationCheck;
}
```

#### HallucinationCheck
```rust
pub struct HallucinationCheck {
    pub verified: bool,
    pub issues: Vec<HallucinationIssue>,
    pub executed_tools: Vec<String>,
    pub unverified_claims: Vec<String>,
}

pub enum HallucinationType {
    RawTextOutput,              // LLM output text without respond_to_user
    UnverifiedActionClaim,      // Claimed action without calling tool
    ResponseWithoutAction,      // respond_to_user without any action
    FailedToolClaimedSuccess,   // Tool failed but claimed success
    NoResponseTool,             // No respond_to_user called
}

pub enum IssueSeverity {
    Info,      // Informational
    Warning,   // Suspicious but might be valid
    Error,     // Definite hallucination
    Critical,  // Severe, reject response
}
```

### 5.4 Response Tools

Special tools that LLM MUST use to communicate:

#### respond_to_user
```json
{
  "name": "respond_to_user",
  "description": "Send a message to the user. REQUIRED for all responses.",
  "parameters": {
    "type": "object",
    "properties": {
      "message": {
        "type": "string",
        "description": "The message to send to the user"
      },
      "message_type": {
        "type": "string",
        "enum": ["success", "error", "info", "warning"],
        "description": "Type of message"
      },
      "related_actions": {
        "type": "array",
        "items": {"type": "string"},
        "description": "Names of action tools that were executed"
      },
      "data": {
        "type": "object",
        "description": "Optional structured data to return"
      }
    },
    "required": ["message", "message_type"]
  }
}
```

#### cannot_perform
```json
{
  "name": "cannot_perform",
  "description": "Explain why a requested action cannot be performed.",
  "parameters": {
    "type": "object",
    "properties": {
      "reason": {
        "type": "string",
        "description": "Why the action cannot be performed"
      },
      "suggestion": {
        "type": "string",
        "description": "Alternative approach if available"
      }
    },
    "required": ["reason"]
  }
}
```

### 5.5 Verification Rules

#### Rule 1: No Raw Text Output
```rust
// Check if LLM output contains "content" field with text
if let Some(content) = llm_response.get("content") {
    if content.as_str().is_some() {
        return HallucinationIssue {
            issue_type: RawTextOutput,
            severity: Critical,
            description: "LLM output raw text without using respond_to_user",
        };
    }
}
```

#### Rule 2: Action Verification
```rust
// Check if respond_to_user claims actions that weren't executed
if let Some(related_actions) = respond_args.get("related_actions") {
    for action in related_actions.as_array() {
        if !executed_tools.contains(action.as_str()) {
            return HallucinationIssue {
                issue_type: UnverifiedActionClaim,
                severity: Error,
                description: format!("Claimed to execute {} but tool was not called", action),
            };
        }
    }
}
```

#### Rule 3: Response Tool Required
```rust
// Every turn must call respond_to_user or cannot_perform
let has_response_tool = executed_tools.iter()
    .any(|t| t == "respond_to_user" || t == "cannot_perform");

if !has_response_tool {
    return HallucinationIssue {
        issue_type: NoResponseTool,
        severity: Critical,
        description: "No respond_to_user or cannot_perform tool was called",
    };
}
```

#### Rule 4: Success Verification
```rust
// If tool failed, cannot claim success
for result in tool_results {
    if !result.success && respond_message_type == "success" {
        return HallucinationIssue {
            issue_type: FailedToolClaimedSuccess,
            severity: Error,
            description: format!("{} failed but success was claimed", result.tool_name),
        };
    }
}
```

### 5.6 System Prompt Rules

The system prompt enforces this architecture:

```markdown
## ⚠️ CRITICAL: FORCED TOOL EXECUTION ARCHITECTURE

**YOU MUST USE TOOLS FOR EVERYTHING - INCLUDING RESPONDING TO THE USER.**

WORKFLOW:
1. User asks you to do something
2. Call the appropriate ACTION TOOL (e.g., ovs_create_bridge)
3. Then call respond_to_user to explain the result

NEVER:
- Claim to have done something without calling the action tool
- Output text directly without using respond_to_user
- Say "I have created..." when you haven't called the tool

EXAMPLES:

User: "Create an OVS bridge called br0"
You should call:
1. ovs_create_bridge {"name": "br0"}
2. respond_to_user {"message": "Created OVS bridge br0", "message_type": "success"}

User: "What bridges exist?"
You should call:
1. ovs_list_bridges {}
2. respond_to_user {"message": "Found bridges: br0, br1", "message_type": "info"}
```

### 5.7 Implementation Phases

#### Phase 1: Basic Detection
- Detect raw text output
- Detect missing response tool
- Block critical hallucinations

#### Phase 2: Advanced Verification
- Verify related_actions claims
- Check success/failure consistency
- Detailed issue reporting

#### Phase 3: Learning System
- Track hallucination patterns
- Adjust system prompt based on patterns
- Provide feedback to LLM provider

---

## 6. Agent Orchestration

### 6.1 Problem Statement

Complex operations require specialized capabilities:
- **Rust operations**: cargo build, test, clippy
- **Memory**: Key-value storage across turns
- **Sequential thinking**: Multi-step reasoning chains
- **Architecture**: Design analysis and suggestions
- **Context**: Persistent context across sessions

We need a way to coordinate these specialized agents efficiently.

### 6.2 Solution: gRPC Agent Pool

#### Design Goals
1. **Persistent Connections**: Avoid connection overhead
2. **Health Monitoring**: Detect and handle failures
3. **Circuit Breaker**: Prevent cascading failures
4. **Streaming Support**: Long-running operations
5. **Parallel Execution**: Batch operations

#### Architecture
```
┌─────────────────────────────────────────────────────────┐
│ GrpcAgentPool                                           │
│                                                         │
│ ┌─────────────────────────────────────────────────┐   │
│ │ Connection Map                                  │   │
│ │ ┌──────────────┐  ┌──────────────┐             │   │
│ │ │ rust_pro     │  │ memory       │             │   │
│ │ │ - client     │  │ - client     │    ...      │   │
│ │ │ - status     │  │ - status     │             │   │
│ │ │ - circuit    │  │ - circuit    │             │   │
│ │ └──────────────┘  └──────────────┘             │   │
│ └─────────────────────────────────────────────────┘   │
│                                                         │
│ ┌─────────────────────────────────────────────────┐   │
│ │ Health Checker (background task)                │   │
│ │ - Periodic health checks (30s)                  │   │
│ │ - Update agent status                           │   │
│ │ - Trigger reconnection if needed                │   │
│ └─────────────────────────────────────────────────┘   │
│                                                         │
│ ┌─────────────────────────────────────────────────┐   │
│ │ Circuit Breaker                                 │   │
│ │ - Track failure count                           │   │
│ │ - Open circuit after threshold (5 failures)     │   │
│ │ - Half-open after timeout (60s)                 │   │
│ │ - Close on success                              │   │
│ └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

### 6.3 Component Design

#### GrpcAgentPool
```rust
pub struct GrpcAgentPool {
    config: AgentPoolConfig,
    connections: Arc<RwLock<HashMap<String, AgentConnection>>>,
    metrics: Arc<PoolMetrics>,
    health_checker: Arc<HealthChecker>,
}

pub struct AgentPoolConfig {
    pub base_address: String,              // "http://127.0.0.1"
    pub connect_timeout: Duration,         // 5s
    pub request_timeout: Duration,         // 30s
    pub health_check_interval: Duration,   // 30s
    pub max_retries: u32,                  // 3
    pub retry_base_delay: Duration,        // 100ms
    pub max_concurrent_per_agent: usize,   // 10
    pub circuit_breaker_threshold: u32,    // 5 failures
    pub circuit_breaker_reset: Duration,   // 60s
    pub run_on_connection: Vec<String>,    // Default agents
}
```

#### AgentConnection
```rust
struct AgentConnection {
    agent_id: String,
    client: AgentServiceClient<Channel>,
    status: AgentStatus,
    last_health_check: Instant,
    failure_count: AtomicU32,
    circuit_state: CircuitState,
    semaphore: Arc<Semaphore>,  // Concurrent request limit
}

pub enum AgentStatus {
    Unknown,
    Starting,
    Running,
    Busy,
    Stopping,
    Stopped,
    Error,
    Unresponsive,
}

pub enum CircuitState {
    Closed,      // Normal operation
    Open,        // Failing, reject requests immediately
    HalfOpen,    // Testing recovery
}
```

### 6.4 Circuit Breaker Pattern

#### State Machine
```
         failures >= threshold
Closed ─────────────────────────> Open
  ▲                                  │
  │                                  │ timeout
  │                                  ▼
  └──── success ────────────── HalfOpen
```

#### Implementation
```rust
async fn execute_with_circuit_breaker(
    &self,
    agent_id: &str,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    let conn = self.get_connection(agent_id).await?;
    
    match conn.circuit_state {
        CircuitState::Open => {
            // Check if timeout elapsed
            if conn.last_failure_time.elapsed() > self.config.circuit_breaker_reset {
                conn.circuit_state = CircuitState::HalfOpen;
            } else {
                return Err(anyhow!("Circuit breaker open for {}", agent_id));
            }
        }
        CircuitState::HalfOpen => {
            // Allow one request to test
        }
        CircuitState::Closed => {
            // Normal operation
        }
    }
    
    match operation.await {
        Ok(result) => {
            conn.failure_count.store(0, Ordering::Relaxed);
            conn.circuit_state = CircuitState::Closed;
            Ok(result)
        }
        Err(e) => {
            let failures = conn.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
            if failures >= self.config.circuit_breaker_threshold {
                conn.circuit_state = CircuitState::Open;
                conn.last_failure_time = Instant::now();
            }
            Err(e)
        }
    }
}
```

### 6.5 Run-on-Connection Agents

Agents that start automatically when a session begins:

```rust
pub const DEFAULT_RUN_ON_CONNECTION: &[&str] = &[
    "rust_pro",           // Cargo operations
    "backend_architect",  // Architecture analysis
    "sequential_thinking",// Reasoning chains
    "memory",            // Key-value storage
    "context_manager",   // Persistent context
];
```

#### Session Lifecycle
```
1. User connects → ChatActor.start_session()
2. GrpcAgentPool.start_session(session_id)
   ├─ For each run-on-connection agent:
   │  ├─ Connect to agent gRPC service
   │  ├─ Call agent.StartSession(session_id)
   │  └─ Add to connection pool
3. User interacts → agents available
4. User disconnects → ChatActor.end_session()
5. GrpcAgentPool.end_session(session_id)
   ├─ For each agent:
   │  ├─ Call agent.EndSession(session_id)
   │  └─ Close connection (or keep in pool)
```

### 6.6 Operations

#### Execute (Single)
```rust
pub async fn execute(
    &self,
    agent_id: &str,
    operation: &str,
    arguments: Value,
    timeout: Option<Duration>,
) -> Result<Value>
```

#### Execute Stream (Long-running)
```rust
pub async fn execute_stream(
    &self,
    agent_id: &str,
    operation: &str,
    arguments: Value,
) -> Result<impl Stream<Item = Result<ExecuteChunk>>>
```

#### Batch Execute (Parallel)
```rust
pub async fn batch_execute(
    &self,
    requests: Vec<ExecuteRequest>,
    parallel: bool,
) -> Result<Vec<ExecuteResponse>>
```

### 6.7 Health Checking

Background task that runs every 30 seconds:

```rust
async fn health_check_loop(pool: Arc<GrpcAgentPool>) {
    let mut interval = tokio::time::interval(
        pool.config.health_check_interval
    );
    
    loop {
        interval.tick().await;
        
        let connections = pool.connections.read().await;
        for (agent_id, conn) in connections.iter() {
            match conn.client.health_check().await {
                Ok(response) if response.healthy => {
                    conn.status = AgentStatus::Running;
                }
                Ok(_) => {
                    conn.status = AgentStatus::Error;
                }
                Err(_) => {
                    conn.status = AgentStatus::Unresponsive;
                    // Trigger reconnection
                    pool.reconnect(agent_id).await;
                }
            }
        }
    }
}
```

---

## 7. Workstack System

### 7.1 Problem Statement

Complex operations require multiple steps with dependencies:
- **Rust project setup**: Create project, add dependencies, configure CI, write tests
- **Microservice deployment**: Build, test, containerize, deploy, configure monitoring
- **Database migration**: Backup, validate, migrate, verify, rollback if needed

We need a way to define and execute these multi-phase workflows reliably.

### 7.2 Solution: Workstacks

#### Concept
A **workstack** is a multi-phase workflow with:
- **Phases**: Ordered steps with dependencies
- **Tools**: Operations to execute in each phase
- **Variables**: Shared state across phases
- **Rollback**: Undo operations on failure
- **Conditions**: Skip phases based on state

#### Example: Rust Project Setup
```yaml
id: rust_project_setup
name: Rust Project Setup
description: Initialize a new Rust project with best practices

phases:
  - id: create_project
    name: Create Project
    tools:
      - tool: execute_command
        arguments:
          command: cargo new ${project_name}
          working_dir: ${project_path}
        store_as: project_created
    
  - id: add_dependencies
    name: Add Dependencies
    depends_on: [create_project]
    tools:
      - tool: execute_command
        arguments:
          command: cargo add tokio serde anyhow
          working_dir: ${project_path}/${project_name}
    
  - id: configure_ci
    name: Configure CI
    depends_on: [create_project]
    tools:
      - tool: write_file
        arguments:
          path: ${project_path}/${project_name}/.github/workflows/ci.yml
          content: ${ci_template}
    
  - id: write_tests
    name: Write Initial Tests
    depends_on: [add_dependencies]
    tools:
      - tool: write_file
        arguments:
          path: ${project_path}/${project_name}/tests/integration_test.rs
          content: ${test_template}
    
  - id: verify
    name: Verify Build
    depends_on: [add_dependencies, write_tests]
    tools:
      - tool: execute_command
        arguments:
          command: cargo check
          working_dir: ${project_path}/${project_name}
    rollback:
      - tool: execute_command
        arguments:
          command: rm -rf ${project_path}/${project_name}
```

### 7.3 Component Design

#### Workstack
```rust
pub struct Workstack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub phases: Vec<WorkstackPhase>,
    pub required_agents: Vec<String>,
    pub variables: HashMap<String, Value>,
    pub timeout_secs: u64,
    pub rollback_on_failure: bool,
}
```

#### WorkstackPhase
```rust
pub struct WorkstackPhase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<PhaseToolCall>,
    pub agents: Vec<String>,
    pub depends_on: Vec<String>,
    pub condition: Option<String>,
    pub rollback: Vec<PhaseToolCall>,
    pub continue_on_failure: bool,
    pub timeout_secs: u64,
    pub status: PhaseStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
}

pub enum PhaseStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    RolledBack,
}
```

#### PhaseToolCall
```rust
pub struct PhaseToolCall {
    pub tool: String,
    pub arguments: Value,
    pub store_as: Option<String>,  // Store result in variable
    pub retries: u32,
}
```

### 7.4 WorkstackExecutor

```rust
pub struct WorkstackExecutor {
    tool_executor: Arc<TrackedToolExecutor>,
    agent_pool: Arc<GrpcAgentPool>,
}

impl WorkstackExecutor {
    pub async fn execute(
        &self,
        workstack: &Workstack,
        variables: HashMap<String, Value>,
        session_id: &str,
    ) -> Result<WorkstackResult>;
    
    async fn execute_phase(
        &self,
        phase: &WorkstackPhase,
        variables: &mut HashMap<String, Value>,
        session_id: &str,
    ) -> Result<PhaseResult>;
    
    async fn rollback_phase(
        &self,
        phase: &WorkstackPhase,
        variables: &HashMap<String, Value>,
        session_id: &str,
    ) -> Result<()>;
    
    fn resolve_dependencies(
        &self,
        phases: &[WorkstackPhase],
    ) -> Result<Vec<Vec<usize>>>;  // Topological sort
}
```

### 7.5 Execution Flow

```
1. Validate Workstack
   ├─ Check all required agents available
   ├─ Validate phase dependencies (no cycles)
   └─ Validate variable references

2. Resolve Dependencies
   ├─ Topological sort of phases
   └─ Group independent phases (can run parallel)

3. Initialize Variables
   ├─ Merge workstack defaults
   └─ Merge user-provided variables

4. Execute Phases (in dependency order)
   For each phase:
     ├─ Check condition (skip if false)
     ├─ Substitute variables in arguments
     ├─ Execute tools in sequence
     ├─ Store results in variables
     ├─ Update phase status
     └─ If failure and rollback_on_failure:
         ├─ Execute rollback tools
         └─ Stop execution

5. Return WorkstackResult
   ├─ Success/failure
   ├─ Phases completed/failed
   ├─ Final variable state
   └─ Execution time
```

### 7.6 Variable Substitution

Variables can be referenced in tool arguments:

```json
{
  "tool": "ovs_add_port",
  "arguments": {
    "bridge": "${bridge_name}",
    "port": "eth${port_number}"
  }
}
```

Implementation:
```rust
fn substitute_variables(
    value: &Value,
    variables: &HashMap<String, Value>,
) -> Result<Value> {
    match value {
        Value::String(s) => {
            let mut result = s.clone();
            for (key, val) in variables {
                let pattern = format!("${{{}}}", key);
                if result.contains(&pattern) {
                    result = result.replace(&pattern, &val.to_string());
                }
            }
            Ok(Value::String(result))
        }
        Value::Object(obj) => {
            let mut new_obj = Object::new();
            for (k, v) in obj {
                new_obj.insert(k.clone(), substitute_variables(v, variables)?);
            }
            Ok(Value::Object(new_obj))
        }
        Value::Array(arr) => {
            let new_arr: Result<Vec<_>> = arr.iter()
                .map(|v| substitute_variables(v, variables))
                .collect();
            Ok(Value::Array(new_arr?))
        }
        _ => Ok(value.clone()),
    }
}
```

### 7.7 Built-in Workstacks

#### Phase 1 (MVP)
1. **rust_project_setup** - Initialize Rust project
2. **microservice_deployment** - Deploy microservice
3. **database_migration** - Safe DB migration

#### Phase 2
4. **security_audit** - Security analysis
5. **performance_optimization** - Profile and optimize
6. **disaster_recovery** - Backup and recovery
7. **ci_cd_pipeline** - CI/CD setup
8. **monitoring_setup** - Prometheus + Grafana
9. **load_testing** - Performance testing
10. **code_review** - Automated review

---

## 8. Protocol Design

### 8.1 gRPC Service Definitions

We need two protocol layers:
1. **Orchestration Protocol**: Full-featured for external clients
2. **Internal Agent Protocol**: Simplified for chatbot-to-agent communication

### 8.2 Orchestration Protocol (orchestration.proto)

#### AgentLifecycle Service
```protobuf
service AgentLifecycle {
    rpc StartSession(StartSessionRequest) returns (StartSessionResponse);
    rpc EndSession(EndSessionRequest) returns (EndSessionResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc WatchAgents(WatchAgentsRequest) returns (stream AgentStatusEvent);
    rpc Shutdown(ShutdownRequest) returns (ShutdownResponse);
}
```

**Design Decisions**:
- `StartSession`: Idempotent, returns already-started agents
- `WatchAgents`: Server streaming for real-time status updates
- `Shutdown`: Graceful shutdown with timeout

#### AgentExecution Service
```protobuf
service AgentExecution {
    rpc Execute(ExecuteRequest) returns (ExecuteResponse);
    rpc ExecuteStream(ExecuteRequest) returns (stream ExecuteChunk);
    rpc BatchExecute(BatchExecuteRequest) returns (stream ExecuteResponse);
    rpc Cancel(CancelRequest) returns (CancelResponse);
}
```

**Design Decisions**:
- `Execute`: Synchronous, waits for completion
- `ExecuteStream`: Server streaming for long operations (cargo build, tests)
- `BatchExecute`: Parallel or sequential execution of multiple operations
- `Cancel`: Best-effort cancellation with correlation ID

#### MemoryService
```protobuf
service MemoryService {
    rpc Remember(RememberRequest) returns (RememberResponse);
    rpc Recall(RecallRequest) returns (RecallResponse);
    rpc Forget(ForgetRequest) returns (ForgetResponse);
    rpc List(ListKeysRequest) returns (ListKeysResponse);
    rpc Search(SearchMemoryRequest) returns (SearchMemoryResponse);
    rpc BulkRemember(stream RememberRequest) returns (BulkOperationResponse);
    rpc BulkRecall(BulkRecallRequest) returns (stream RecallResponse);
    rpc BulkForget(BulkForgetRequest) returns (BulkOperationResponse);
}
```

**Design Decisions**:
- Simple key-value operations for high frequency
- Bulk operations use streaming for efficiency
- TTL support for automatic expiry
- Tags for categorization and filtering

#### SequentialThinkingService
```protobuf
service SequentialThinkingService {
    rpc StartChain(StartChainRequest) returns (StartChainResponse);
    rpc AddThought(AddThoughtRequest) returns (AddThoughtResponse);
    rpc ThinkStream(StartChainRequest) returns (stream ThoughtEvent);
    rpc Conclude(ConcludeRequest) returns (ConcludeResponse);
    rpc GetChain(GetChainRequest) returns (GetChainResponse);
    rpc ForkChain(ForkChainRequest) returns (ForkChainResponse);
}
```

**Design Decisions**:
- Chain-based reasoning with max steps
- Streaming for real-time thinking process
- Fork support for exploring alternatives
- Confidence scoring per thought

#### RustProService
```protobuf
service RustProService {
    rpc Check(CargoRequest) returns (CargoResponse);
    rpc Fmt(CargoRequest) returns (CargoResponse);
    rpc Build(CargoRequest) returns (stream CargoOutputLine);
    rpc Test(CargoRequest) returns (stream CargoOutputLine);
    rpc Clippy(CargoRequest) returns (stream CargoOutputLine);
    rpc Run(CargoRequest) returns (stream CargoOutputLine);
    rpc Analyze(AnalyzeRequest) returns (AnalyzeResponse);
}
```

**Design Decisions**:
- Quick operations (check, fmt) are synchronous
- Long operations (build, test) stream output
- Separate stdout/stderr/compiler streams
- Analysis for dependencies, structure, complexity

#### WorkstackService
```protobuf
service WorkstackService {
    rpc Execute(WorkstackExecuteRequest) returns (stream WorkstackEvent);
    rpc GetStatus(WorkstackStatusRequest) returns (WorkstackStatusResponse);
    rpc Cancel(WorkstackCancelRequest) returns (WorkstackCancelResponse);
    rpc Rollback(WorkstackRollbackRequest) returns (WorkstackRollbackResponse);
    rpc List(ListWorkstacksRequest) returns (ListWorkstacksResponse);
}
```

**Design Decisions**:
- Execute streams events for progress tracking
- Status query for monitoring
- Cancel with optional rollback
- Partial rollback (to specific phase)

### 8.3 Internal Agent Protocol (agents.proto)

Simplified protocol for chatbot-to-agent communication:

```protobuf
service AgentService {
    rpc StartSession(StartSessionRequest) returns (StartSessionResponse);
    rpc EndSession(EndSessionRequest) returns (EndSessionResponse);
    rpc Execute(ExecuteRequest) returns (ExecuteResponse);
    rpc ExecuteStream(ExecuteRequest) returns (stream ExecuteChunk);
    rpc BatchExecute(BatchExecuteRequest) returns (stream ExecuteResponse);
    rpc Session(stream SessionMessage) returns (stream SessionMessage);
}
```

**Design Decisions**:
- Unified `Execute` operation (agent determines what to do)
- Bidirectional `Session` stream for entire session lifecycle
- Simpler than orchestration protocol (fewer services)

### 8.4 Message Design Patterns

#### Request/Response Pattern
```protobuf
message ExecuteRequest {
    string session_id = 1;
    string agent_id = 2;
    string operation = 3;
    string arguments_json = 4;  // Flexible JSON payload
    int64 timeout_ms = 5;
    string correlation_id = 6;  // For tracking
    ExecutionOptions options = 7;
}

message ExecuteResponse {
    string correlation_id = 1;
    string agent_id = 2;
    string operation = 3;
    bool success = 4;
    string result_json = 5;     // Flexible JSON payload
    ExecuteError error = 6;
    int64 execution_time_ms = 7;
    map<string, string> metadata = 8;
}
```

**Design Decisions**:
- JSON payloads for flexibility (avoid proto changes)
- Correlation ID for request tracking
- Execution time for metrics
- Metadata for extensibility

#### Streaming Pattern
```protobuf
message ExecuteChunk {
    string correlation_id = 1;
    ChunkType chunk_type = 2;
    string content = 3;
    bool is_final = 4;
    int32 sequence = 5;
    int64 timestamp_ms = 6;
    ExecuteError error = 7;  // Only if is_final and error
}

enum ChunkType {
    CHUNK_TYPE_STDOUT = 1;
    CHUNK_TYPE_STDERR = 2;
    CHUNK_TYPE_PROGRESS = 3;
    CHUNK_TYPE_RESULT = 4;
    CHUNK_TYPE_HEARTBEAT = 5;
}
```

**Design Decisions**:
- Sequence numbers for ordering
- Chunk types for different streams
- Heartbeats to detect connection issues
- Final chunk includes error if failed

#### Error Handling
```protobuf
message ExecuteError {
    string code = 1;           // Machine-readable
    string message = 2;        // Human-readable
    string details = 3;        // Additional context
    bool retryable = 4;        // Can retry?
    string stack_trace = 5;    // For debugging
}
```

**Design Decisions**:
- Structured errors (not just strings)
- Retryable flag for automatic retry logic
- Stack trace for debugging (not exposed to users)

---

## 9. Data Models

### 9.1 Core Types

#### ChatSession
```rust
pub struct ChatSession {
    pub id: String,                        // UUID
    pub name: Option<String>,              // User-provided name
    pub messages: Vec<ChatMessage>,        // Conversation history
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, Value>,  // Extensible
    
    // Authentication
    pub auth_session_id: Option<String>,   // From WireGuard
    pub is_controller: bool,               // Elevated privileges
    pub peer_pubkey: Option<String>,       // Crypto identity
}
```

**Storage Strategy**:
- **Phase 1**: In-memory (RwLock<HashMap>)
- **Phase 2**: Redis (fast, persistent)
- **Phase 3**: PostgreSQL (full history, queries)

#### ChatMessage
```rust
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, Value>,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}
```

#### ToolCall
```rust
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

pub struct ToolCallResult {
    pub tool_call: ToolCall,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}
```

### 9.2 Execution Tracking

#### ExecutionContext
```rust
pub struct ExecutionContext {
    pub session_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, Value>,
}
```

#### ExecutionResult
```rust
pub struct ExecutionResult {
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}
```

**Storage**: Append-only log in `op-execution-tracker`

### 9.3 Metrics

#### ExecutorMetrics
```rust
pub struct ExecutorMetrics {
    pub total_executions: AtomicU64,
    pub successful_executions: AtomicU64,
    pub failed_executions: AtomicU64,
    pub rate_limited: AtomicU64,
    pub total_execution_time_ms: AtomicU64,
}
```

**Aggregation**:
```rust
pub struct AggregatedMetrics {
    pub total_executions: u64,
    pub success_rate: f64,
    pub average_execution_time_ms: u64,
    pub p50_execution_time_ms: u64,
    pub p95_execution_time_ms: u64,
    pub p99_execution_time_ms: u64,
}
```

### 9.4 Configuration

#### ChatActorConfig
```rust
pub struct ChatActorConfig {
    pub max_concurrent: usize,           // Default: 10
    pub request_timeout_secs: u64,       // Default: 300
    pub enable_tracking: bool,           // Default: true
    pub max_history: usize,              // Default: 1000
    pub rate_limit: RateLimitConfig,
}

pub struct RateLimitConfig {
    pub max_per_minute: u32,    // Default: 60
    pub max_per_hour: u32,      // Default: 500
    pub max_concurrent: u32,    // Default: 10
}
```

#### AgentPoolConfig
```rust
pub struct AgentPoolConfig {
    pub base_address: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub health_check_interval: Duration,
    pub max_retries: u32,
    pub retry_base_delay: Duration,
    pub max_concurrent_per_agent: usize,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_reset: Duration,
    pub run_on_connection: Vec<String>,
}
```

**Configuration Sources** (priority order):
1. Environment variables
2. Configuration file (`/etc/op-dbus/chat.toml`)
3. Defaults

---

## 10. Implementation Plan

### 10.1 Phase 1: MVP (Weeks 1-4)

#### Week 1: Core Infrastructure
- [ ] ChatActor with actor pattern
- [ ] SessionManager (in-memory)
- [ ] SystemPromptGenerator
- [ ] Basic RpcRequest/RpcResponse types
- [ ] Unit tests for core components

#### Week 2: Tool Execution
- [ ] TrackedToolExecutor
- [ ] Rate limiting implementation
- [ ] Integration with op-execution-tracker
- [ ] Metrics collection
- [ ] Tool execution tests

#### Week 3: Anti-Hallucination
- [ ] ForcedExecutionOrchestrator
- [ ] Response tools (respond_to_user, cannot_perform)
- [ ] Hallucination detection (5 types)
- [ ] Verification rules
- [ ] Anti-hallucination tests

#### Week 4: NL Admin
- [ ] NLAdminOrchestrator
- [ ] ToolCallParser (4 formats)
- [ ] LLM provider integration
- [ ] End-to-end tests
- [ ] Documentation

**Deliverable**: Working natural language admin with anti-hallucination

### 10.2 Phase 2: Agent Orchestration (Weeks 5-8)

#### Week 5: gRPC Infrastructure
- [ ] Protocol definitions (orchestration.proto, agents.proto)
- [ ] Code generation with tonic-build
- [ ] Basic gRPC client/server
- [ ] Connection management
- [ ] gRPC tests

#### Week 6: Agent Pool
- [ ] GrpcAgentPool implementation
- [ ] Connection pooling
- [ ] Health checking
- [ ] Circuit breaker
- [ ] Pool tests

#### Week 7: Agent Integration
- [ ] rust_pro agent
- [ ] memory agent
- [ ] sequential_thinking agent
- [ ] Agent tests
- [ ] Integration tests

#### Week 8: Session Lifecycle
- [ ] Start/end session with agents
- [ ] Run-on-connection logic
- [ ] Agent coordination
- [ ] Lifecycle tests
- [ ] Documentation

**Deliverable**: Working agent orchestration with 3 agents

### 10.3 Phase 3: Workstacks (Weeks 9-12)

#### Week 9: Workstack Core
- [ ] Workstack data structures
- [ ] WorkstackExecutor
- [ ] Dependency resolution
- [ ] Variable substitution
- [ ] Core tests

#### Week 10: Rollback & Streaming
- [ ] Rollback implementation
- [ ] Progress streaming
- [ ] Condition evaluation
- [ ] Rollback tests
- [ ] Streaming tests

#### Week 11: Built-in Workstacks
- [ ] rust_project_setup
- [ ] microservice_deployment
- [ ] database_migration
- [ ] Workstack tests
- [ ] Documentation

#### Week 12: Workstack Service
- [ ] WorkstackService gRPC implementation
- [ ] Status queries
- [ ] Cancel/rollback operations
- [ ] Service tests
- [ ] End-to-end tests

**Deliverable**: Working workstack system with 3 built-in workflows

### 10.4 Phase 4: MCP Server (Weeks 13-14)

#### Week 13: MCP Implementation
- [ ] ChatMcpServer
- [ ] Prompts (workstacks)
- [ ] Resources (skills)
- [ ] Tools (registry)
- [ ] MCP tests

#### Week 14: Standalone Binary
- [ ] main.rs for standalone server
- [ ] Configuration loading
- [ ] Logging setup
- [ ] Integration tests
- [ ] Documentation

**Deliverable**: Working MCP server

### 10.5 Phase 5: Polish & Production (Weeks 15-16)

#### Week 15: Performance
- [ ] Benchmark suite
- [ ] Performance optimization
- [ ] Memory profiling
- [ ] Load testing
- [ ] Performance documentation

#### Week 16: Production Readiness
- [ ] Comprehensive error handling
- [ ] Logging and tracing
- [ ] Metrics dashboard
- [ ] Deployment guide
- [ ] Operations runbook

**Deliverable**: Production-ready op-chat

---

## 11. Testing Strategy

### 11.1 Unit Tests

**Coverage Target**: 80%

#### Component Tests
- ChatActor message handling
- SessionManager CRUD operations
- TrackedToolExecutor rate limiting
- ForcedExecutionOrchestrator verification
- GrpcAgentPool connection management
- WorkstackExecutor dependency resolution

#### Test Structure
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_rate_limiting() {
        let executor = create_test_executor();
        
        // Execute 60 times (should succeed)
        for _ in 0..60 {
            executor.execute("test_tool", json!({}), "session-1").await.unwrap();
        }
        
        // 61st should fail
        let result = executor.execute("test_tool", json!({}), "session-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Rate limit"));
    }
}
```

### 11.2 Integration Tests

#### End-to-End Scenarios
1. **Natural Language → Tool Execution**
   - User: "Create OVS bridge br0"
   - Verify: Bridge created, response sent, audit logged

2. **Multi-Step Operation**
   - User: "Set up a new Rust project called my-app"
   - Verify: Project created, dependencies added, tests written

3. **Hallucination Detection**
   - LLM claims action without calling tool
   - Verify: Hallucination detected, response blocked

4. **Agent Coordination**
   - Execute workstack requiring multiple agents
   - Verify: All agents called, results aggregated

5. **Rollback on Failure**
   - Workstack fails in phase 3
   - Verify: Phases 1-2 rolled back

### 11.3 Performance Tests

#### Load Testing
```rust
#[tokio::test]
async fn test_concurrent_sessions() {
    let actor = create_test_actor();
    
    let mut handles = vec![];
    for i in 0..100 {
        let actor = actor.clone();
        handles.push(tokio::spawn(async move {
            let session_id = format!("session-{}", i);
            actor.chat("Hello", &session_id).await
        }));
    }
    
    let results = futures::future::join_all(handles).await;
    assert!(results.iter().all(|r| r.is_ok()));
}
```

#### Benchmark Suite
- Tool execution overhead: < 100ms
- Session creation: < 10ms
- Agent connection: < 100ms
- Workstack execution: < 5s (3-phase)

### 11.4 Security Tests

#### Authentication Tests
- Unauthenticated requests rejected
- Controller vs user permissions enforced
- Session isolation verified

#### Input Validation Tests
- Malformed JSON rejected
- SQL injection attempts blocked
- Path traversal attempts blocked
- Command injection attempts blocked

### 11.5 Chaos Testing

#### Failure Scenarios
- Agent crashes mid-operation
- Network partition
- Database unavailable
- LLM provider timeout
- Disk full

#### Recovery Verification
- Circuit breaker opens
- Automatic reconnection
- Graceful degradation
- No data loss

---

## 12. Performance Targets

### 12.1 Latency

| Operation | Target | Measurement |
|-----------|--------|-------------|
| Simple tool execution | < 1s | p95 |
| Complex tool execution | < 5s | p95 |
| Session creation | < 10ms | p95 |
| Agent connection | < 100ms | p95 |
| Workstack (3-phase) | < 5s | p95 |
| Health check | < 50ms | p95 |

### 12.2 Throughput

| Metric | Target |
|--------|--------|
| Concurrent sessions | 100 |
| Requests per second | 1000 |
| Tool executions per second | 500 |
| Agent operations per second | 200 |

### 12.3 Resource Usage

| Resource | Target |
|----------|--------|
| Memory per instance | < 100MB |
| CPU per instance | < 50% (1 core) |
| Network bandwidth | < 10MB/s |
| Disk I/O | < 1MB/s |

### 12.4 Scalability

| Dimension | Target |
|-----------|--------|
| Tools in registry | 1000+ |
| Active sessions | 1000+ |
| Message history per session | 10,000+ |
| Concurrent agent connections | 50+ |

### 12.5 Optimization Strategies

#### SIMD JSON
- Use `simd-json` instead of `serde_json`
- 2-3x faster parsing
- Critical for high-frequency operations

#### Connection Pooling
- Reuse gRPC connections
- Avoid connection overhead
- Health checks to detect failures

#### Async/Await
- Non-blocking I/O
- Efficient resource usage
- Tokio runtime

#### Caching
- Cache tool definitions
- Cache system prompts
- Cache agent capabilities

---

## 13. Security Model

### 13.1 Authentication

#### WireGuard Integration
```rust
pub struct ChatSession {
    pub auth_session_id: Option<String>,   // From WireGuard gateway
    pub is_controller: bool,               // Elevated privileges
    pub peer_pubkey: Option<String>,       // Crypto identity
}
```

**Flow**:
1. User connects via WireGuard
2. Gateway authenticates and assigns session ID
3. Gateway forwards to op-chat with auth context
4. op-chat creates authenticated session

#### Unauthenticated Access
- Read-only operations allowed
- Tool execution requires authentication
- System operations require controller role

### 13.2 Authorization

#### Role-Based Access Control

| Role | Permissions |
|------|-------------|
| Unauthenticated | List tools, health check |
| User | Execute tools, manage own sessions |
| Controller | All user permissions + manage all sessions, view audit logs |

#### Implementation
```rust
fn check_permission(session: &ChatSession, operation: &str) -> Result<()> {
    match operation {
        "list_tools" | "health" => Ok(()),
        "execute_tool" | "chat" => {
            if session.auth_session_id.is_some() {
                Ok(())
            } else {
                Err(anyhow!("Authentication required"))
            }
        }
        "get_all_sessions" | "get_audit_log" => {
            if session.is_controller {
                Ok(())
            } else {
                Err(anyhow!("Controller role required"))
            }
        }
        _ => Err(anyhow!("Unknown operation")),
    }
}
```

### 13.3 Audit Trail

#### What to Log
- All tool executions (who, what, when, result)
- Session creation/deletion
- Authentication events
- Authorization failures
- System errors

#### Log Format
```rust
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub peer_pubkey: Option<String>,
    pub operation: String,
    pub arguments: Value,
    pub result: AuditResult,
    pub execution_time_ms: u64,
}

pub enum AuditResult {
    Success { result: Value },
    Failure { error: String },
    Denied { reason: String },
}
```

#### Storage
- Append-only log
- Immutable entries
- Indexed by session_id, timestamp
- Retention: 90 days (configurable)

### 13.4 Input Validation

#### Tool Arguments
```rust
fn validate_tool_arguments(
    tool: &ToolDefinition,
    arguments: &Value,
) -> Result<()> {
    // Check required fields
    for param in &tool.parameters.required {
        if !arguments.get(param).is_some() {
            return Err(anyhow!("Missing required parameter: {}", param));
        }
    }
    
    // Type checking
    for (key, value) in arguments.as_object() {
        let param = tool.parameters.properties.get(key)
            .ok_or_else(|| anyhow!("Unknown parameter: {}", key))?;
        
        validate_type(value, &param.param_type)?;
    }
    
    // Value constraints
    for (key, value) in arguments.as_object() {
        if let Some(param) = tool.parameters.properties.get(key) {
            validate_constraints(value, &param.constraints)?;
        }
    }
    
    Ok(())
}
```

#### Path Traversal Prevention
```rust
fn validate_path(path: &str) -> Result<()> {
    if path.contains("..") {
        return Err(anyhow!("Path traversal not allowed"));
    }
    
    let canonical = std::fs::canonicalize(path)?;
    if !canonical.starts_with("/allowed/base/path") {
        return Err(anyhow!("Path outside allowed directory"));
    }
    
    Ok(())
}
```

#### Command Injection Prevention
```rust
fn validate_command(command: &str) -> Result<()> {
    // Whitelist allowed commands
    const ALLOWED_COMMANDS: &[&str] = &[
        "cargo", "rustc", "git", "docker",
    ];
    
    let cmd = command.split_whitespace().next()
        .ok_or_else(|| anyhow!("Empty command"))?;
    
    if !ALLOWED_COMMANDS.contains(&cmd) {
        return Err(anyhow!("Command not allowed: {}", cmd));
    }
    
    // Check for shell metacharacters
    if command.contains(&['|', '&', ';', '>', '<', '`', '$'][..]) {
        return Err(anyhow!("Shell metacharacters not allowed"));
    }
    
    Ok(())
}
```

### 13.5 Rate Limiting

#### Per-Session Limits
- 60 executions per minute
- 500 executions per hour
- 10 concurrent executions

#### Global Limits
- 1000 requests per second
- 100 concurrent sessions
- 50 concurrent agent connections

#### Implementation
```rust
struct RateLimiter {
    session_limits: RwLock<HashMap<String, SessionRateState>>,
    global_semaphore: Arc<Semaphore>,
}

impl RateLimiter {
    async fn check_rate_limit(&self, session_id: &str) -> Result<()> {
        // Check session limits
        let mut limits = self.session_limits.write().await;
        let state = limits.entry(session_id.to_string())
            .or_insert_with(SessionRateState::new);
        
        state.check_and_increment(&self.config)?;
        
        // Check global limit
        self.global_semaphore.acquire().await?;
        
        Ok(())
    }
}
```

---

## 14. Deployment Architecture

### 14.1 Deployment Modes

#### Mode 1: Standalone (Development)
```
┌─────────────────────────────────────┐
│ Single Host                         │
│                                     │
│  ┌──────────────┐                  │
│  │  op-chat     │                  │
│  │  (binary)    │                  │
│  └──────┬───────┘                  │
│         │                           │
│         ├─► op-tools (embedded)    │
│         ├─► op-llm (embedded)      │
│         └─► Agents (local gRPC)    │
│                                     │
│  ┌──────────────┐                  │
│  │  Agents      │                  │
│  │  - rust_pro  │                  │
│  │  - memory    │                  │
│  │  - seq_think │                  │
│  └──────────────┘                  │
└─────────────────────────────────────┘
```

**Use Case**: Development, testing, single-user

#### Mode 2: Distributed (Production)
```
┌─────────────────────────────────────────────────────────┐
│ Load Balancer (HAProxy/Nginx)                          │
└────────────┬────────────────────────────────────────────┘
             │
    ┌────────┴────────┐
    │                 │
    ▼                 ▼
┌─────────┐       ┌─────────┐
│ op-chat │       │ op-chat │
│ Instance│       │ Instance│
│    1    │       │    2    │
└────┬────┘       └────┬────┘
     │                 │
     └────────┬────────┘
              │
    ┌─────────┴─────────┐
    │                   │
    ▼                   ▼
┌─────────┐       ┌─────────┐
│ Redis   │       │ Agent   │
│ (sessions)      │ Cluster │
└─────────┘       │         │
                  │ ┌─────┐ │
                  │ │rust │ │
                  │ │mem  │ │
                  │ │seq  │ │
                  │ └─────┘ │
                  └─────────┘
```

**Use Case**: Production, multi-user, high availability

#### Mode 3: Embedded (Library)
```
┌─────────────────────────────────────┐
│ Your Application                    │
│                                     │
│  use op_chat::{                     │
│      ChatActor,                     │
│      ChatActorConfig,               │
│  };                                 │
│                                     │
│  let (actor, handle) =              │
│      ChatActor::new(config).await?; │
│                                     │
│  // Use handle to send requests     │
└─────────────────────────────────────┘
```

**Use Case**: Integration into existing applications

### 14.2 Service Configuration

#### systemd Unit (op-chat.service)
```ini
[Unit]
Description=op-chat - Chat Orchestration Layer
After=network.target
Requires=op-agent-rust-pro.service
Requires=op-agent-memory.service
Requires=op-agent-sequential-thinking.service

[Service]
Type=simple
User=op-chat
Group=op-chat
ExecStart=/usr/local/bin/op-chat
Restart=always
RestartSec=5
Environment="RUST_LOG=op_chat=info"
Environment="OP_CHAT_LISTEN=0.0.0.0:50052"
Environment="OP_AGENT_POOL_ADDRESS=http://127.0.0.1"

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/op-chat

# Resource limits
LimitNOFILE=65536
MemoryMax=512M
CPUQuota=200%

[Install]
WantedBy=multi-user.target
```

#### dinit Service (op-chat)
```
type = process
command = /usr/local/bin/op-chat
logfile = /var/log/op-chat/op-chat.log
restart = true
depends-on = op-agent-rust-pro
depends-on = op-agent-memory
depends-on = op-agent-sequential-thinking
```

### 14.3 Configuration Files

#### /etc/op-dbus/chat.toml
```toml
[server]
listen = "0.0.0.0:50052"
max_concurrent = 100
request_timeout_secs = 300

[rate_limit]
max_per_minute = 60
max_per_hour = 500
max_concurrent = 10

[agent_pool]
base_address = "http://127.0.0.1"
connect_timeout_ms = 5000
request_timeout_ms = 30000
health_check_interval_ms = 30000
max_retries = 3
circuit_breaker_threshold = 5
circuit_breaker_reset_ms = 60000

run_on_connection = [
    "rust_pro",
    "backend_architect",
    "sequential_thinking",
    "memory",
    "context_manager",
]

[tracking]
enabled = true
max_history = 10000

[logging]
level = "info"
format = "json"
output = "/var/log/op-chat/op-chat.log"

[llm]
provider = "anthropic"
model = "claude-3-5-sonnet-20241022"
api_key_file = "/etc/op-dbus/llm-api-key"
```

#### /etc/op-dbus/custom-prompt.txt
```
# Custom System Prompt Additions

## Organization-Specific Rules

- Always use staging environment for testing
- Require approval for production changes
- Follow naming convention: {env}-{service}-{component}

## Compliance

- Log all PII access
- Encrypt sensitive data at rest
- Use approved cryptographic algorithms
```

### 14.4 Monitoring

#### Metrics Endpoint
```rust
// Expose metrics on /metrics
async fn metrics_handler() -> impl IntoResponse {
    let metrics = EXECUTOR_METRICS.get_aggregated().await;
    
    format!(
        "# HELP op_chat_executions_total Total tool executions\n\
         # TYPE op_chat_executions_total counter\n\
         op_chat_executions_total {}\n\
         \n\
         # HELP op_chat_execution_duration_seconds Tool execution duration\n\
         # TYPE op_chat_execution_duration_seconds histogram\n\
         op_chat_execution_duration_seconds_sum {}\n\
         op_chat_execution_duration_seconds_count {}\n",
        metrics.total_executions,
        metrics.total_execution_time_ms as f64 / 1000.0,
        metrics.total_executions,
    )
}
```

#### Health Check Endpoint
```rust
async fn health_handler() -> impl IntoResponse {
    let health = HealthCheck {
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: UPTIME.elapsed().as_secs(),
        active_sessions: SESSION_MANAGER.count().await,
        agent_status: AGENT_POOL.get_status().await,
    };
    
    Json(health)
}
```

#### Prometheus Configuration
```yaml
scrape_configs:
  - job_name: 'op-chat'
    static_configs:
      - targets: ['localhost:50052']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

#### Grafana Dashboard
- Active sessions over time
- Tool execution rate
- Success/failure ratio
- Average execution time
- Agent health status
- Rate limit hits
- Hallucination detection rate

### 14.5 Backup & Recovery

#### Session Backup
```bash
#!/bin/bash
# Backup sessions to S3
redis-cli --rdb /tmp/sessions.rdb
aws s3 cp /tmp/sessions.rdb s3://op-chat-backups/sessions-$(date +%Y%m%d).rdb
```

#### Audit Log Backup
```bash
#!/bin/bash
# Backup audit logs
tar -czf /tmp/audit-$(date +%Y%m%d).tar.gz /var/lib/op-chat/audit/
aws s3 cp /tmp/audit-$(date +%Y%m%d).tar.gz s3://op-chat-backups/
```

#### Recovery Procedure
1. Stop op-chat service
2. Restore Redis dump
3. Restore audit logs
4. Verify data integrity
5. Start op-chat service
6. Verify health checks

---

## 15. Future Roadmap

### 15.1 Phase 6: Advanced Features (Q2 2026)

#### Persistent Sessions
- Redis backend for sessions
- Session migration between instances
- Session replay for debugging

#### Advanced Workstacks
- Conditional branching
- Parallel phase execution
- Dynamic phase generation
- Workstack templates

#### Skills Marketplace
- Community-contributed skills
- Skill versioning
- Skill dependencies
- Skill discovery

### 15.2 Phase 7: Intelligence (Q3 2026)

#### Learning System
- Track hallucination patterns
- Adjust system prompt automatically
- Learn from corrections
- Improve intent recognition

#### Context Management
- Long-term memory across sessions
- Project-specific context
- Team-shared context
- Context search and retrieval

#### Proactive Assistance
- Suggest optimizations
- Detect potential issues
- Recommend best practices
- Automated maintenance tasks

### 15.3 Phase 8: Scale (Q4 2026)

#### Distributed Architecture
- Multi-region deployment
- Agent federation
- Load balancing
- Failover

#### Performance Optimization
- Tool execution caching
- Predictive agent startup
- Query optimization
- Resource pooling

#### Enterprise Features
- Multi-tenancy
- SSO integration
- Advanced RBAC
- Compliance reporting

### 15.4 Research Areas

#### LLM Improvements
- Fine-tuning for system administration
- Smaller, faster models
- On-device inference
- Multi-modal support (diagrams, logs)

#### Agent Intelligence
- Autonomous agents
- Agent collaboration
- Agent learning
- Agent specialization

#### Verification
- Formal verification of critical operations
- Property-based testing
- Chaos engineering
- Fault injection

---

## 16. Risk Analysis

### 16.1 Technical Risks

#### Risk: LLM Hallucination
- **Probability**: High
- **Impact**: Critical
- **Mitigation**: Forced tool execution architecture, verification
- **Contingency**: Manual review mode, rollback capability

#### Risk: Agent Failure
- **Probability**: Medium
- **Impact**: High
- **Mitigation**: Circuit breaker, health checks, graceful degradation
- **Contingency**: Fallback to direct tool execution

#### Risk: Performance Degradation
- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Rate limiting, connection pooling, caching
- **Contingency**: Horizontal scaling, resource limits

#### Risk: Security Breach
- **Probability**: Low
- **Impact**: Critical
- **Mitigation**: Authentication, authorization, input validation, audit trail
- **Contingency**: Incident response plan, forensics

### 16.2 Operational Risks

#### Risk: Configuration Error
- **Probability**: Medium
- **Impact**: High
- **Mitigation**: Configuration validation, defaults, documentation
- **Contingency**: Rollback to previous config, safe mode

#### Risk: Dependency Failure
- **Probability**: Low
- **Impact**: High
- **Mitigation**: Vendor diversity, fallbacks, monitoring
- **Contingency**: Manual operation mode

#### Risk: Data Loss
- **Probability**: Low
- **Impact**: High
- **Mitigation**: Backups, replication, append-only logs
- **Contingency**: Recovery procedures, data reconstruction

### 16.3 Business Risks

#### Risk: Adoption Resistance
- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Documentation, training, gradual rollout
- **Contingency**: Traditional CLI fallback

#### Risk: Compliance Issues
- **Probability**: Low
- **Impact**: High
- **Mitigation**: Audit trail, data retention, access controls
- **Contingency**: Compliance review, remediation plan

---

## 17. Success Criteria

### 17.1 Technical Success

- [ ] Hallucination rate < 0.1%
- [ ] Intent recognition > 95%
- [ ] Response time < 1s (p95)
- [ ] Uptime > 99.9%
- [ ] Zero data loss
- [ ] 100% audit coverage

### 17.2 User Success

- [ ] Users prefer NL interface over CLI
- [ ] Reduced time to complete tasks
- [ ] Fewer errors in operations
- [ ] Positive user feedback
- [ ] Active daily usage

### 17.3 Business Success

- [ ] Reduced operational costs
- [ ] Faster incident response
- [ ] Improved system reliability
- [ ] Compliance maintained
- [ ] Positive ROI

---

## 18. Open Questions

### 18.1 Technical Questions

1. **LLM Provider**: Which provider(s) to support initially?
   - Anthropic (Claude)
   - OpenAI (GPT-4)
   - Local (Ollama)
   - Multiple?

2. **Session Storage**: When to move from in-memory to persistent?
   - Phase 1: In-memory
   - Phase 2: Redis
   - Phase 3: PostgreSQL
   - Configurable?

3. **Agent Discovery**: How do agents register themselves?
   - Static configuration
   - Service discovery (Consul, etcd)
   - DNS-based
   - Hybrid?

4. **Workstack Distribution**: How to share workstacks?
   - Built-in only
   - File-based
   - Registry service
   - Git-based?

### 18.2 Design Questions

1. **Error Recovery**: How aggressive should automatic retry be?
   - Conservative (fail fast)
   - Aggressive (retry everything)
   - Configurable per tool
   - Learn from history?

2. **Rate Limiting**: Should limits be per-user or per-session?
   - Per-session (current design)
   - Per-user (requires user tracking)
   - Per-tool (different limits per tool)
   - Dynamic (based on load)?

3. **Streaming**: When to use streaming vs synchronous?
   - Always stream (consistent interface)
   - Stream only for long operations (complexity)
   - Client choice (flexibility)
   - Automatic (based on timeout)?

### 18.3 Operational Questions

1. **Deployment**: How to handle rolling updates?
   - Blue-green deployment
   - Canary deployment
   - Rolling restart
   - Session migration?

2. **Monitoring**: What metrics are most important?
   - Current: Execution count, time, success rate
   - Add: User satisfaction, intent accuracy
   - Add: Resource usage, cost
   - Add: Business metrics?

3. **Scaling**: When to scale horizontally vs vertically?
   - Horizontal: More instances
   - Vertical: Bigger instances
   - Hybrid: Both
   - Auto-scaling rules?

---

## 19. Appendices

### 19.1 Glossary

- **Actor**: Concurrency pattern with message passing
- **Agent**: Specialized service for specific capabilities
- **Circuit Breaker**: Fault tolerance pattern
- **Hallucination**: LLM claiming false information
- **MCP**: Model Context Protocol
- **Workstack**: Multi-phase workflow
- **Tool**: Executable operation
- **Session**: Conversation context

### 19.2 References

#### External
- [Model Context Protocol](https://modelcontextprotocol.io/)
- [gRPC](https://grpc.io/)
- [Tokio](https://tokio.rs/)
- [Tonic](https://github.com/hyperium/tonic)
- [Actor Model](https://en.wikipedia.org/wiki/Actor_model)
- [Circuit Breaker Pattern](https://martinfowler.com/bliki/CircuitBreaker.html)

#### Internal
- `/AGENTS.md` - Repository guidelines
- `/spec.md` - Overall system specification
- `/HIERARCHICAL_DBUS_DESIGN.md` - D-Bus architecture
- `op-tools/SPEC.md` - Tool registry specification
- `op-llm/SPEC.md` - LLM provider specification

### 19.3 Change Log

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-02-16 | Initial design document |

---

## 20. Approval & Sign-off

### 20.1 Design Review

- [ ] Architecture reviewed
- [ ] Security reviewed
- [ ] Performance reviewed
- [ ] Operations reviewed

### 20.2 Stakeholder Approval

- [ ] Engineering lead
- [ ] Product owner
- [ ] Security team
- [ ] Operations team

### 20.3 Implementation Authorization

- [ ] Budget approved
- [ ] Resources allocated
- [ ] Timeline agreed
- [ ] Success criteria defined

---

**Document Status**: Draft  
**Next Review**: 2026-02-23  
**Owner**: op-dbus-v2 team  
**Contact**: [team email]

---

*This design document is a living document and will be updated as the project evolves.*
