# op-chat - Comprehensive Specification

**Crate**: `op-chat`  
**Location**: `crates/op-chat`  
**Version**: Workspace  
**Edition**: Rust 2021  
**Description**: Chat orchestration layer with LLM integration, anti-hallucination architecture, and gRPC agent coordination

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Core Components](#core-components)
4. [Orchestration System](#orchestration-system)
5. [Protocol Definitions](#protocol-definitions)
6. [Anti-Hallucination System](#anti-hallucination-system)
7. [Tool Execution](#tool-execution)
8. [Session Management](#session-management)
9. [MCP Server](#mcp-server)
10. [Dependencies](#dependencies)
11. [Build Configuration](#build-configuration)
12. [Usage Examples](#usage-examples)

---

## 1. Overview

`op-chat` is the **brain** of the op-dbus-v2 system. It provides:

- **Natural Language Administration**: Convert user intent → tool execution
- **Anti-Hallucination Architecture**: Force all LLM output through verified tools
- **gRPC Agent Orchestration**: Coordinate run-on-connection agents (rust_pro, memory, sequential_thinking, etc.)
- **Workstack Execution**: Multi-phase workflow orchestration with rollback support
- **Skills System**: Domain-specific knowledge augmentation
- **Session Management**: Authenticated chat sessions with WireGuard integration
- **MCP Server**: Model Context Protocol server over gRPC
- **Execution Tracking**: Full audit trail with rate limiting

### Key Design Principles

1. **GRPC-First**: All internal agent communication uses gRPC for performance
2. **SIMD JSON**: Uses `simd-json` instead of `serde_json` for serialization
3. **D-Bus Integration**: Native D-Bus protocol support (no CLI wrappers)
4. **Zero Trust**: All LLM claims verified against execution log
5. **Streaming Support**: Long-running operations stream results
6. **Circuit Breaker**: Fault tolerance for agent failures

---

## 2. Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         FRONTENDS                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│  │   Web    │  │   MCP    │  │   CLI    │  │  gRPC    │       │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘       │
└───────┼─────────────┼─────────────┼─────────────┼──────────────┘
        │             │             │             │
        └─────────────┴─────────────┴─────────────┘
                      │
        ┌─────────────▼─────────────────────────────────────┐
        │          ChatActor (Central Brain)                │
        │  - Message routing                                │
        │  - Session management                             │
        │  - Request/response coordination                  │
        └─────────────┬─────────────────────────────────────┘
                      │
        ┌─────────────┴─────────────────────────────────────┐
        │                                                    │
        ▼                                                    ▼
┌───────────────────┐                          ┌────────────────────┐
│ NLAdminOrchestrator│                          │ WorkstackExecutor  │
│ - Intent parsing  │                          │ - Multi-phase exec │
│ - Tool extraction │                          │ - Rollback support │
│ - LLM interaction │                          │ - Agent coordination│
└────────┬──────────┘                          └─────────┬──────────┘
         │                                                │
         ▼                                                ▼
┌────────────────────┐                          ┌────────────────────┐
│ TrackedToolExecutor│                          │  GrpcAgentPool     │
│ - Rate limiting    │                          │ - Connection pool  │
│ - Audit logging    │                          │ - Health checks    │
│ - Metrics tracking │                          │ - Circuit breaker  │
└────────┬───────────┘                          └─────────┬──────────┘
         │                                                │
         ▼                                                ▼
┌────────────────────┐                          ┌────────────────────┐
│ ForcedExecution    │                          │  Run-on-Connection │
│ Orchestrator       │                          │  Agents (gRPC)     │
│ - Hallucination    │                          │  ┌──────────────┐  │
│   detection        │                          │  │ rust_pro     │  │
│ - Claim verification│                         │  │ memory       │  │
│ - Response tools   │                          │  │ seq_thinking │  │
└────────────────────┘                          │  │ backend_arch │  │
                                                │  │ context_mgr  │  │
                                                │  └──────────────┘  │
                                                └────────────────────┘
```

### 2.2 Data Flow

#### User Request Flow
```
User: "Create OVS bridge br0"
    │
    ▼
ChatActor receives RpcRequest::Chat
    │
    ▼
NLAdminOrchestrator.process()
    │
    ├─► LLM Provider (with system prompt + tools)
    │
    ▼
LLM Response: tool_calls: [
    {name: "ovs_create_bridge", args: {name: "br0"}},
    {name: "respond_to_user", args: {message: "Created br0"}}
]
    │
    ▼
TrackedToolExecutor.execute_sequence()
    │
    ├─► Execute: ovs_create_bridge → Success
    ├─► Execute: respond_to_user → Success
    │
    ▼
ForcedExecutionOrchestrator.verify()
    │
    ├─► Check: ovs_create_bridge was called ✓
    ├─► Check: respond_to_user references it ✓
    ├─► Check: No raw text output ✓
    │
    ▼
Return verified response to user
```

### 2.3 Module Organization

```
op-chat/
├── src/
│   ├── lib.rs                      # Public API exports
│   ├── main.rs                     # Standalone MCP server binary
│   │
│   ├── actor.rs                    # ChatActor - central message processor
│   ├── session.rs                  # Session management
│   ├── system_prompt.rs            # System prompt generation
│   │
│   ├── nl_admin.rs                 # Natural language admin orchestrator
│   ├── forced_execution.rs         # Anti-hallucination verification
│   ├── tool_executor.rs            # Tracked tool execution with rate limiting
│   │
│   ├── mcp_server.rs               # MCP server implementation
│   │
│   └── orchestration/              # Advanced orchestration
│       ├── mod.rs                  # Workstacks and exports
│       ├── workstacks.rs           # Workstack definitions
│       ├── workstack_executor.rs   # Workstack execution engine
│       ├── skills.rs               # Skills system
│       ├── grpc_pool.rs            # gRPC agent connection pool
│       ├── error.rs                # Orchestration errors
│       │
│       ├── proto/                  # Generated protobuf code
│       │   ├── mod.rs
│       │   └── op_chat.orchestration.rs
│       │
│       └── skills_builtin/         # Built-in skill definitions
│           ├── architecture_patterns.md
│           ├── auth_implementation_patterns.md
│           ├── btrfs_deployment.md
│           ├── code_review_excellence.md
│           ├── debugging_strategies.md
│           ├── distributed_tracing.md
│           ├── e2e_testing_patterns.md
│           ├── error_handling_patterns.md
│           ├── gitops_workflow.md
│           ├── k8s_manifest_generator.md
│           ├── microservices_patterns.md
│           ├── prometheus_configuration.md
│           ├── python_testing_patterns.md
│           ├── secrets_management.md
│           └── sql_optimization_patterns.md
│
├── proto/
│   ├── orchestration.proto         # Full gRPC service definitions
│   └── agents.proto                # Internal agent protocol
│
├── build.rs                        # Protobuf compilation
├── Cargo.toml
└── SPEC.md                         # This file
```

---

## 3. Core Components

### 3.1 ChatActor

**File**: `src/actor.rs`

The central message processor and coordinator. All requests flow through ChatActor.

#### Key Types

```rust
pub struct ChatActor {
    config: ChatActorConfig,
    tool_registry: Arc<ToolRegistry>,
    session_manager: Arc<SessionManager>,
    executor: Arc<TrackedToolExecutor>,
    tracker: Arc<ExecutionTracker>,
}

pub struct ChatActorConfig {
    pub max_concurrent: usize,           // Default: 10
    pub request_timeout_secs: u64,       // Default: 300
    pub enable_tracking: bool,           // Default: true
    pub max_history: usize,              // Default: 1000
}

pub enum RpcRequest {
    ListTools { offset: Option<usize>, limit: Option<usize> },
    ExecuteTool { name: String, arguments: Value, session_id: Option<String> },
    GetTool { name: String },
    Chat { message: String, session_id: String, model: Option<String> },
    GetHistory { limit: Option<usize> },
    GetStats,
    Health,
    Introspect { service: String, bus_type: Option<String> },
    DbusCall { service: String, path: String, interface: String, method: String, args: Vec<Value> },
}

pub struct RpcResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
    pub metadata: HashMap<String, Value>,
}
```

#### Key Methods

- `new(config: ChatActorConfig) -> Result<(Self, ChatActorHandle)>` - Create actor and handle
- `run(self) -> Result<()>` - Start actor message loop
- `handle_request(&mut self, req: RpcRequest) -> Result<RpcResponse>` - Process request

#### Actor Pattern

ChatActor uses Tokio's actor pattern with message passing:
- Requests sent via `mpsc::channel`
- Responses returned via `oneshot::channel`
- Single-threaded message processing (no locks needed)

### 3.2 SessionManager

**File**: `src/session.rs`

Manages chat sessions with message history and authentication.

#### Key Types

```rust
pub struct ChatSession {
    pub id: String,
    pub name: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, Value>,
    pub auth_session_id: Option<String>,      // From WireGuard gateway
    pub is_controller: bool,                   // Controller vs regular user
    pub peer_pubkey: Option<String>,          // WireGuard peer key
}

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ChatSession>>>,
}
```

#### Key Methods

- `new() -> Self`
- `create_session() -> String` - Create new session, return ID
- `get_session(&self, id: &str) -> Option<ChatSession>`
- `add_message(&self, session_id: &str, message: ChatMessage)`
- `authenticated(auth_session_id: String, is_controller: bool, peer_pubkey: Option<String>) -> ChatSession`

### 3.3 System Prompt Generator

**File**: `src/system_prompt.rs`

Generates comprehensive system prompts with anti-hallucination rules.

#### Structure

System prompt has two parts:

1. **FIXED PART** (immutable):
   - Anti-hallucination rules
   - Forced tool execution architecture
   - Available tools and capabilities
   - Forbidden CLI commands
   - OVS/D-Bus protocol usage

2. **CUSTOM PART** (mutable):
   - Loaded from `/etc/op-dbus/custom-prompt.txt`
   - Or `./custom-prompt.txt` (development)
   - Or `CUSTOM_SYSTEM_PROMPT` environment variable

#### Key Functions

```rust
pub fn generate_system_prompt(
    tools: &[ToolDefinition],
    custom_additions: Option<&str>,
    repo_info: Option<&SelfRepositoryInfo>,
) -> String

pub fn create_session_with_system_prompt(
    session_id: String,
    tools: &[ToolDefinition],
) -> (ChatSession, String)
```

#### Critical Rules in System Prompt

```
⚠️ CRITICAL: FORCED TOOL EXECUTION ARCHITECTURE

YOU MUST USE TOOLS FOR EVERYTHING - INCLUDING RESPONDING TO THE USER.

Workflow:
1. User asks you to do something
2. Call the appropriate ACTION TOOL (e.g., ovs_create_bridge)
3. Then call respond_to_user to explain the result

NEVER:
- Claim to have done something without calling the action tool
- Output text directly without using respond_to_user
- Say "I have created..." when you haven't called the tool
```

---

## 4. Orchestration System

### 4.1 Workstacks

**File**: `src/orchestration/workstacks.rs`

Workstacks define complex, multi-phase workflows that coordinate multiple tools and agents.

#### Key Types

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

pub struct WorkstackPhase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<PhaseToolCall>,
    pub agents: Vec<String>,
    pub depends_on: Vec<String>,           // Phase dependencies
    pub condition: Option<String>,         // Execution condition
    pub rollback: Vec<PhaseToolCall>,      // Rollback actions
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

pub struct PhaseToolCall {
    pub tool: String,
    pub arguments: Value,                  // Can use ${var} references
    pub store_as: Option<String>,          // Store result in variable
    pub retries: u32,
}
```

#### Built-in Workstacks

The `builtin_workstacks()` function provides pre-defined workflows:

1. **rust_project_setup** - Initialize Rust project with best practices
2. **microservice_deployment** - Deploy microservice with monitoring
3. **database_migration** - Safe database schema migration
4. **security_audit** - Comprehensive security analysis
5. **performance_optimization** - Profile and optimize performance
6. **disaster_recovery** - Backup and recovery procedures
7. **ci_cd_pipeline** - Set up CI/CD with testing
8. **monitoring_setup** - Deploy Prometheus + Grafana
9. **load_testing** - Performance load testing
10. **code_review** - Automated code review workflow

### 4.2 WorkstackExecutor

**File**: `src/orchestration/workstack_executor.rs`

Executes workstacks with dependency resolution, rollback support, and progress tracking.

#### Key Types

```rust
pub struct WorkstackExecutor {
    tool_executor: Arc<TrackedToolExecutor>,
    agent_pool: Arc<GrpcAgentPool>,
}

pub struct WorkstackResult {
    pub success: bool,
    pub workstack_id: String,
    pub execution_id: String,
    pub phases_completed: Vec<String>,
    pub phases_failed: Vec<String>,
    pub variables: HashMap<String, Value>,
    pub execution_time_ms: u64,
    pub error: Option<String>,
}
```

#### Execution Flow

```
1. Validate workstack definition
2. Resolve phase dependencies (topological sort)
3. For each phase (in dependency order):
   a. Check condition (if specified)
   b. Execute tools in sequence
   c. Store results in variables
   d. Update phase status
   e. If failure and rollback_on_failure:
      - Execute rollback tools
      - Stop execution
4. Return WorkstackResult
```

#### Variable Substitution

Tool arguments can reference variables:
```json
{
  "tool": "ovs_add_port",
  "arguments": {
    "bridge": "${bridge_name}",
    "port": "${interface}"
  }
}
```

### 4.3 Skills System

**File**: `src/orchestration/skills.rs`

Skills provide domain-specific knowledge and capabilities that augment tool execution.

#### Key Types

```rust
pub struct Skill {
    pub name: String,
    pub metadata: SkillMetadata,
    pub content: String,                   // Markdown content
    pub context: SkillContext,
}

pub struct SkillMetadata {
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub required_tools: Vec<String>,
    pub version: String,
}

pub struct SkillContext {
    pub system_prompt_additions: Vec<String>,
    pub input_transformations: HashMap<String, Value>,
    pub output_transformations: HashMap<String, Value>,
    pub variables: HashMap<String, Value>,
    pub constraints: Vec<SkillConstraint>,
}

pub struct SkillConstraint {
    pub constraint_type: ConstraintType,
    pub target: String,
    pub value: Value,
}

pub enum ConstraintType {
    RequireArgument,
    ForbidArgument,
    RequireBefore,
    RequireAfter,
    MaxExecutions,
    RequireConfirmation,
}
```

#### Built-in Skills

Located in `src/orchestration/skills_builtin/`:

- **architecture_patterns.md** - System design patterns
- **auth_implementation_patterns.md** - Authentication best practices
- **btrfs_deployment.md** - Btrfs filesystem deployment
- **code_review_excellence.md** - Code review guidelines
- **debugging_strategies.md** - Debugging methodologies
- **distributed_tracing.md** - Distributed tracing setup
- **e2e_testing_patterns.md** - End-to-end testing
- **error_handling_patterns.md** - Error handling strategies
- **gitops_workflow.md** - GitOps deployment patterns
- **k8s_manifest_generator.md** - Kubernetes manifest generation
- **microservices_patterns.md** - Microservices architecture
- **prometheus_configuration.md** - Prometheus monitoring
- **python_testing_patterns.md** - Python testing best practices
- **secrets_management.md** - Secrets management
- **sql_optimization_patterns.md** - SQL query optimization

#### Skill Registry

```rust
pub struct SkillRegistry {
    skills: Arc<RwLock<HashMap<String, Skill>>>,
}

impl SkillRegistry {
    pub fn new() -> Self
    pub async fn register(&self, skill: Skill)
    pub async fn get(&self, name: &str) -> Option<Skill>
    pub async fn list(&self) -> Vec<SkillMetadata>
    pub async fn activate(&self, name: &str, context: &mut SkillContext)
}
```

### 4.4 GrpcAgentPool

**File**: `src/orchestration/grpc_pool.rs`

Manages persistent gRPC connections to run-on-connection agents with health checks, reconnection, and circuit breaker.

#### Key Types

```rust
pub struct GrpcAgentPool {
    config: AgentPoolConfig,
    connections: Arc<RwLock<HashMap<String, AgentConnection>>>,
    metrics: Arc<PoolMetrics>,
    health_checker: Arc<HealthChecker>,
}

pub struct AgentPoolConfig {
    pub base_address: String,              // Default: "http://127.0.0.1"
    pub connect_timeout: Duration,         // Default: 5s
    pub request_timeout: Duration,         // Default: 30s
    pub health_check_interval: Duration,   // Default: 30s
    pub max_retries: u32,                  // Default: 3
    pub retry_base_delay: Duration,        // Default: 100ms
    pub max_concurrent_per_agent: usize,   // Default: 10
    pub pool_connections: bool,            // Default: true
    pub run_on_connection: Vec<String>,    // Default agents to start
    pub circuit_breaker_threshold: u32,    // Default: 5
    pub circuit_breaker_reset: Duration,   // Default: 60s
}

pub struct AgentConnection {
    agent_id: String,
    client: AgentServiceClient<Channel>,
    status: AgentStatus,
    last_health_check: Instant,
    failure_count: AtomicU32,
    circuit_state: CircuitState,
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
    Open,        // Failing, reject requests
    HalfOpen,    // Testing recovery
}
```

#### Default Run-on-Connection Agents

```rust
vec![
    "rust_pro",           // Cargo operations
    "backend_architect",  // Architecture analysis
    "sequential_thinking",// Reasoning chains
    "memory",            // Key-value storage
    "context_manager",   // Persistent context
]
```

#### Key Methods

```rust
impl GrpcAgentPool {
    pub async fn new(config: AgentPoolConfig) -> Result<Self>
    
    pub async fn start_session(&self, session_id: &str) -> Result<Vec<String>>
    
    pub async fn end_session(&self, session_id: &str) -> Result<()>
    
    pub async fn execute(
        &self,
        agent_id: &str,
        operation: &str,
        arguments: Value,
        timeout: Option<Duration>,
    ) -> Result<Value>
    
    pub async fn execute_stream(
        &self,
        agent_id: &str,
        operation: &str,
        arguments: Value,
    ) -> Result<impl Stream<Item = Result<ExecuteChunk>>>
    
    pub async fn batch_execute(
        &self,
        requests: Vec<ExecuteRequest>,
        parallel: bool,
    ) -> Result<Vec<ExecuteResponse>>
    
    pub async fn health_check(&self, agent_id: &str) -> Result<HealthStatus>
}
```

#### Circuit Breaker Pattern

```
Closed (Normal) ──[failures >= threshold]──> Open (Failing)
      ▲                                          │
      │                                          │
      │                                          │ [timeout]
      │                                          ▼
      └──[success]──── HalfOpen (Testing) <─────┘
```

When circuit is Open:
- Requests fail immediately without attempting connection
- Reduces load on failing agents
- Allows time for recovery

---

## 5. Protocol Definitions

### 5.1 Orchestration Protocol

**File**: `proto/orchestration.proto`

Full gRPC service definitions for agent communication.

#### Services

##### AgentLifecycle Service
```protobuf
service AgentLifecycle {
    rpc StartSession(StartSessionRequest) returns (StartSessionResponse);
    rpc EndSession(EndSessionRequest) returns (EndSessionResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc WatchAgents(WatchAgentsRequest) returns (stream AgentStatusEvent);
    rpc Shutdown(ShutdownRequest) returns (ShutdownResponse);
}
```

##### AgentExecution Service
```protobuf
service AgentExecution {
    rpc Execute(ExecuteRequest) returns (ExecuteResponse);
    rpc ExecuteStream(ExecuteRequest) returns (stream ExecuteChunk);
    rpc BatchExecute(BatchExecuteRequest) returns (stream ExecuteResponse);
    rpc Cancel(CancelRequest) returns (CancelResponse);
}
```

##### MemoryService
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

##### SequentialThinkingService
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

##### ContextManagerService
```protobuf
service ContextManagerService {
    rpc Save(SaveContextRequest) returns (SaveContextResponse);
    rpc Load(LoadContextRequest) returns (LoadContextResponse);
    rpc List(ListContextsRequest) returns (ListContextsResponse);
    rpc Delete(DeleteContextRequest) returns (DeleteContextResponse);
    rpc Export(ExportContextRequest) returns (stream ExportChunk);
    rpc Import(stream ImportChunk) returns (ImportContextResponse);
    rpc Merge(MergeContextsRequest) returns (MergeContextsResponse);
}
```

##### RustProService
```protobuf
service RustProService {
    rpc Check(CargoRequest) returns (CargoResponse);
    rpc Fmt(CargoRequest) returns (CargoResponse);
    rpc Version(VersionRequest) returns (VersionResponse);
    rpc Build(CargoRequest) returns (stream CargoOutputLine);
    rpc Test(CargoRequest) returns (stream CargoOutputLine);
    rpc Clippy(CargoRequest) returns (stream CargoOutputLine);
    rpc Run(CargoRequest) returns (stream CargoOutputLine);
    rpc Doc(CargoRequest) returns (stream CargoOutputLine);
    rpc Bench(CargoRequest) returns (stream CargoOutputLine);
    rpc Analyze(AnalyzeRequest) returns (AnalyzeResponse);
}
```

##### BackendArchitectService
```protobuf
service BackendArchitectService {
    rpc Analyze(ArchitectAnalyzeRequest) returns (ArchitectAnalyzeResponse);
    rpc Design(ArchitectDesignRequest) returns (ArchitectDesignResponse);
    rpc Review(ArchitectReviewRequest) returns (ArchitectReviewResponse);
    rpc Suggest(ArchitectSuggestRequest) returns (ArchitectSuggestResponse);
    rpc Document(ArchitectDocumentRequest) returns (stream DocumentChunk);
}
```

##### WorkstackService
```protobuf
service WorkstackService {
    rpc Execute(WorkstackExecuteRequest) returns (stream WorkstackEvent);
    rpc GetStatus(WorkstackStatusRequest) returns (WorkstackStatusResponse);
    rpc Cancel(WorkstackCancelRequest) returns (WorkstackCancelResponse);
    rpc Rollback(WorkstackRollbackRequest) returns (WorkstackRollbackResponse);
    rpc List(ListWorkstacksRequest) returns (ListWorkstacksResponse);
}
```

### 5.2 Internal Agent Protocol

**File**: `proto/agents.proto`

Simplified protocol for chatbot-to-agent communication.

#### Services

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

Individual agent services:
- `MemoryAgent`
- `SequentialThinkingAgent`
- `ContextManagerAgent`
- `RustProAgent`
- `BackendArchitectAgent`

---

## 6. Anti-Hallucination System

### 6.1 ForcedExecutionOrchestrator

**File**: `src/forced_execution.rs`

Ensures the LLM cannot hallucinate by requiring all output to go through verified tools.

#### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ User: "Create a bridge called br0"                         │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ LLM Response (MUST contain tool_calls)                      │
│                                                             │
│ tool_calls: [                                               │
│   { name: "ovs_create_bridge", args: {name: "br0"} },      │
│   { name: "respond_to_user", args: {                        │
│       message: "Created bridge br0",                        │
│       message_type: "success",                              │
│       related_actions: ["ovs_create_bridge"]                │
│   }}                                                        │
│ ]                                                           │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│ Execution Verifier                                          │
│                                                             │
│ ✓ ovs_create_bridge was called                              │
│ ✓ respond_to_user references ovs_create_bridge              │
│ ✓ No raw text output (all via respond_to_user)              │
│                                                             │
│ Result: VERIFIED - No hallucination                         │
└─────────────────────────────────────────────────────────────┘
```

#### Key Types

```rust
pub struct ForcedExecutionOrchestrator {
    executor: Arc<TrackedToolExecutor>,
    current_turn_tools: Arc<RwLock<Vec<String>>>,
}

pub struct HallucinationCheck {
    pub verified: bool,
    pub issues: Vec<HallucinationIssue>,
    pub executed_tools: Vec<String>,
    pub unverified_claims: Vec<String>,
}

pub struct HallucinationIssue {
    pub issue_type: HallucinationType,
    pub description: String,
    pub severity: IssueSeverity,
}

pub enum HallucinationType {
    RawTextOutput,              // LLM output text without respond_to_user
    UnverifiedActionClaim,      // Claimed action without calling tool
    ResponseWithoutAction,      // respond_to_user without any action
    FailedToolClaimedSuccess,   // Tool failed but claimed success
    NoResponseTool,             // No respond_to_user called
}

pub enum IssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

pub struct ToolCallResult {
    pub tool_call: ToolCall,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}
```

#### Key Methods

```rust
impl ForcedExecutionOrchestrator {
    pub fn new(executor: Arc<TrackedToolExecutor>) -> Self
    
    pub async fn start_turn(&self)
    
    pub async fn execute_and_verify(
        &self,
        llm_response: &Value,
        session_id: &str,
    ) -> Result<(Vec<ToolCallResult>, HallucinationCheck)>
    
    pub async fn verify_response(
        &self,
        llm_response: &Value,
        executed_tools: &[String],
    ) -> HallucinationCheck
}
```

#### Verification Rules

1. **No Raw Text Output**: All LLM output must go through `respond_to_user` tool
2. **Action Verification**: Any claimed action must have corresponding tool execution
3. **Response Tool Required**: Every turn must call `respond_to_user`
4. **Success Verification**: Cannot claim success if tool execution failed
5. **Reference Integrity**: `respond_to_user` must reference actual executed tools

#### Helper Functions

```rust
pub fn parse_tool_calls(llm_response: &Value) -> Vec<ToolCall>

pub fn detect_raw_text_output(llm_response: &Value) -> Option<String>
```

### 6.2 Response Tools

Response tools are special tools that the LLM MUST use to communicate with users.

#### respond_to_user

```json
{
  "name": "respond_to_user",
  "description": "Send a message to the user. REQUIRED for all responses.",
  "parameters": {
    "message": "string - The message to send",
    "message_type": "enum - success|error|info|warning",
    "related_actions": "array - Tool names that were executed",
    "data": "object - Optional structured data"
  }
}
```

#### cannot_perform

```json
{
  "name": "cannot_perform",
  "description": "Explain why a requested action cannot be performed.",
  "parameters": {
    "reason": "string - Why the action cannot be performed",
    "suggestion": "string - Alternative approach if available"
  }
}
```

### 6.3 Hallucination Detection Examples

#### Example 1: Raw Text Output (CRITICAL)

```json
// LLM Response
{
  "content": "I have created the bridge br0 for you.",
  "tool_calls": []
}

// Detection Result
{
  "verified": false,
  "issues": [
    {
      "issue_type": "RawTextOutput",
      "description": "LLM output raw text without using respond_to_user",
      "severity": "Critical"
    },
    {
      "issue_type": "NoResponseTool",
      "description": "No respond_to_user tool was called",
      "severity": "Critical"
    }
  ]
}
```

#### Example 2: Unverified Action Claim (ERROR)

```json
// LLM Response
{
  "tool_calls": [
    {
      "name": "respond_to_user",
      "arguments": {
        "message": "I have created bridge br0",
        "message_type": "success",
        "related_actions": ["ovs_create_bridge"]
      }
    }
  ]
}

// But ovs_create_bridge was never called!

// Detection Result
{
  "verified": false,
  "issues": [
    {
      "issue_type": "UnverifiedActionClaim",
      "description": "Claimed to execute ovs_create_bridge but tool was not called",
      "severity": "Error"
    }
  ]
}
```

#### Example 3: Verified Response (SUCCESS)

```json
// LLM Response
{
  "tool_calls": [
    {
      "name": "ovs_create_bridge",
      "arguments": {"name": "br0"}
    },
    {
      "name": "respond_to_user",
      "arguments": {
        "message": "Created OVS bridge br0",
        "message_type": "success",
        "related_actions": ["ovs_create_bridge"]
      }
    }
  ]
}

// Execution: ovs_create_bridge succeeded

// Detection Result
{
  "verified": true,
  "issues": [],
  "executed_tools": ["ovs_create_bridge", "respond_to_user"],
  "unverified_claims": []
}
```

---

## 7. Tool Execution

### 7.1 TrackedToolExecutor

**File**: `src/tool_executor.rs`

Wraps tool execution with tracking, rate limiting, and metrics.

#### Key Types

```rust
pub struct TrackedToolExecutor {
    tool_registry: Arc<ToolRegistry>,
    tracker: Arc<ExecutionTracker>,
    rate_limiter: Arc<RwLock<HashMap<String, SessionRateState>>>,
    config: RateLimitConfig,
    concurrent_semaphore: Arc<Semaphore>,
    metrics: Arc<ExecutorMetrics>,
}

pub struct RateLimitConfig {
    pub max_per_minute: u32,    // Default: 60
    pub max_per_hour: u32,      // Default: 500
    pub max_concurrent: u32,    // Default: 10
}

struct SessionRateState {
    minute_count: u32,
    minute_window_start: Instant,
    hour_count: u32,
    hour_window_start: Instant,
}

struct ExecutorMetrics {
    total_executions: AtomicU64,
    successful_executions: AtomicU64,
    failed_executions: AtomicU64,
    rate_limited: AtomicU64,
    total_execution_time_ms: AtomicU64,
}
```

#### Key Methods

```rust
impl TrackedToolExecutor {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        tracker: Arc<ExecutionTracker>,
        config: RateLimitConfig,
    ) -> Self
    
    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: &str,
    ) -> Result<Value>
    
    pub async fn execute_sequence(
        &self,
        tool_calls: Vec<ToolCall>,
        session_id: &str,
    ) -> Vec<ToolCallResult>
    
    pub async fn get_metrics(&self) -> ExecutorMetrics
    
    async fn check_rate_limit(&self, session_id: &str) -> Result<()>
}
```

#### Execution Flow

```
1. Check rate limit for session
   ├─ Per-minute limit (default: 60)
   └─ Per-hour limit (default: 500)

2. Acquire concurrent execution permit
   └─ Max concurrent (default: 10)

3. Create execution context
   ├─ Session ID
   ├─ Tool name
   ├─ Arguments
   └─ Timestamp

4. Execute tool via ToolRegistry

5. Record execution in tracker
   ├─ Success/failure
   ├─ Execution time
   ├─ Result/error
   └─ Metadata

6. Update metrics
   ├─ Total executions
   ├─ Success/failure counts
   └─ Execution time

7. Release permit and return result
```

#### Rate Limiting

Rate limits are per-session with sliding windows:

- **Per-minute window**: Resets every 60 seconds
- **Per-hour window**: Resets every 3600 seconds

When limit exceeded:
```rust
Err("Rate limit exceeded: 60 executions per minute")
```

#### Metrics

```rust
pub struct ExecutorMetrics {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub rate_limited: u64,
    pub total_execution_time_ms: u64,
    pub average_execution_time_ms: u64,
}
```

### 7.2 Integration with ExecutionTracker

TrackedToolExecutor integrates with `op-execution-tracker` for full audit trail:

```rust
let context = ExecutionContext {
    session_id: session_id.to_string(),
    tool_name: tool_name.to_string(),
    arguments: arguments.clone(),
    timestamp: Utc::now(),
    metadata: HashMap::new(),
};

let result = ExecutionResult {
    success: true,
    result: Some(result_value),
    error: None,
    execution_time_ms: elapsed.as_millis() as u64,
};

tracker.record(context, result).await?;
```

---

## 8. Session Management

### 8.1 ChatSession

**File**: `src/session.rs`

#### Session Lifecycle

```
1. Create Session
   ├─ Generate UUID
   ├─ Initialize message history
   └─ Set metadata

2. Add Messages
   ├─ User messages
   ├─ Assistant messages
   └─ System messages

3. Update Metadata
   ├─ Last activity timestamp
   ├─ Message count
   └─ Custom fields

4. Authentication (Optional)
   ├─ WireGuard session ID
   ├─ Controller flag
   └─ Peer public key

5. Session End
   ├─ Save to persistent storage
   └─ Cleanup resources
```

#### Key Methods

```rust
impl ChatSession {
    pub fn new() -> Self
    
    pub fn with_id(id: impl Into<String>) -> Self
    
    pub fn authenticated(
        auth_session_id: String,
        is_controller: bool,
        peer_pubkey: Option<String>,
    ) -> Self
    
    pub fn add_message(&mut self, message: ChatMessage)
    
    pub fn get_messages(&self) -> &[ChatMessage]
    
    pub fn set_metadata(&mut self, key: String, value: Value)
    
    pub fn get_metadata(&self, key: &str) -> Option<&Value>
}
```

### 8.2 SessionManager

#### Key Methods

```rust
impl SessionManager {
    pub fn new() -> Self
    
    pub async fn create_session(&self) -> String
    
    pub async fn get_session(&self, id: &str) -> Option<ChatSession>
    
    pub async fn add_message(
        &self,
        session_id: &str,
        message: ChatMessage,
    ) -> Result<()>
    
    pub async fn list_sessions(&self) -> Vec<String>
    
    pub async fn delete_session(&self, id: &str) -> Result<()>
    
    pub async fn cleanup_old_sessions(&self, max_age: Duration) -> usize
}
```

### 8.3 WireGuard Integration

Sessions can be authenticated via WireGuard gateway:

```rust
let session = ChatSession::authenticated(
    "wg-session-abc123".to_string(),  // Auth session ID
    true,                              // Is controller
    Some("peer-pubkey-xyz".to_string()), // Peer public key
);
```

Controller sessions have elevated privileges:
- Can manage other sessions
- Can access system-level operations
- Can view audit logs

---

## 9. Natural Language Administration

### 9.1 NLAdminOrchestrator

**File**: `src/nl_admin.rs`

The core module that enables natural language server administration.

#### Architecture

```
User Input: "Create an OVS bridge called br0"
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│ NLAdminOrchestrator                                     │
│                                                         │
│ 1. Build system prompt with tools                      │
│ 2. Send to LLM provider                                │
│ 3. Parse LLM response for tool calls                   │
│ 4. Execute tools via TrackedToolExecutor               │
│ 5. Format results for user                             │
└─────────────────────────────────────────────────────────┘
```

#### Key Types

```rust
pub struct NLAdminOrchestrator {
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor: Arc<TrackedToolExecutor>,
    tool_registry: Arc<ToolRegistry>,
}

pub struct NLAdminResult {
    pub message: String,
    pub success: bool,
    pub tools_executed: Vec<String>,
    pub tool_results: Vec<ToolExecutionResult>,
    pub llm_response: Option<String>,
}

pub struct ToolExecutionResult {
    pub tool_name: String,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

pub struct ExtractedToolCall {
    pub name: String,
    pub arguments: Value,
    pub source: ToolCallSource,
}

pub enum ToolCallSource {
    Native,        // Native tool_calls from LLM API
    XmlTags,       // Parsed from <tool_call>...</tool_call>
    CodeBlock,     // Parsed from ```tool ... ```
    JsonInText,    // Parsed from JSON in text
}
```

#### Key Methods

```rust
impl NLAdminOrchestrator {
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        tool_executor: Arc<TrackedToolExecutor>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self
    
    pub async fn process(
        &self,
        user_message: &str,
        session_id: &str,
        conversation_history: &[ChatMessage],
    ) -> Result<NLAdminResult>
    
    async fn extract_tool_calls(
        &self,
        llm_response: &ChatResponse,
    ) -> Vec<ExtractedToolCall>
    
    async fn execute_tools(
        &self,
        tool_calls: Vec<ExtractedToolCall>,
        session_id: &str,
    ) -> Vec<ToolExecutionResult>
}
```

#### Processing Flow

```
1. Build System Prompt
   ├─ Load fixed anti-hallucination rules
   ├─ Load custom prompt additions
   ├─ Add tool definitions
   └─ Add repository context

2. Prepare LLM Request
   ├─ System message
   ├─ Conversation history
   ├─ User message
   └─ Tool definitions

3. Call LLM Provider
   └─ Get response with tool_calls

4. Extract Tool Calls
   ├─ Native tool_calls (preferred)
   ├─ XML tags: <tool_call>name(args)</tool_call>
   ├─ Code blocks: ```tool\nname(args)\n```
   └─ JSON in text

5. Execute Tools
   ├─ For each tool call:
   │   ├─ Validate arguments
   │   ├─ Execute via TrackedToolExecutor
   │   └─ Collect result
   └─ Return results

6. Format Response
   ├─ Extract user message from respond_to_user
   ├─ Collect tool execution details
   └─ Return NLAdminResult
```

### 9.2 ToolCallParser

Extracts tool calls from various LLM response formats.

#### Supported Formats

##### 1. Native Tool Calls (Preferred)
```json
{
  "tool_calls": [
    {
      "id": "call_123",
      "type": "function",
      "function": {
        "name": "ovs_create_bridge",
        "arguments": "{\"name\": \"br0\"}"
      }
    }
  ]
}
```

##### 2. XML Tags
```xml
<tool_call>ovs_create_bridge({"name": "br0"})</tool_call>
<tool_call>respond_to_user({"message": "Created bridge br0"})</tool_call>
```

##### 3. Code Blocks
````markdown
```tool
ovs_create_bridge({"name": "br0"})
```

```tool
respond_to_user({"message": "Created bridge br0"})
```
````

##### 4. JSON in Text
```
I will call ovs_create_bridge({"name": "br0"}) to create the bridge.
```

#### Parser Implementation

```rust
pub struct ToolCallParser {
    xml_tag_regex: Regex,
    code_block_regex: Regex,
    function_call_regex: Regex,
}

impl ToolCallParser {
    pub fn new() -> Self
    
    pub fn parse(&self, text: &str) -> Vec<ExtractedToolCall>
    
    fn parse_xml_tags(&self, text: &str) -> Vec<ExtractedToolCall>
    
    fn parse_code_blocks(&self, text: &str) -> Vec<ExtractedToolCall>
    
    fn parse_function_calls(&self, text: &str) -> Vec<ExtractedToolCall>
}
```

### 9.3 Response Formatting

The `respond_to_user` tool accumulates responses:

```rust
// In op-tools/src/builtin/response_tools.rs
static RESPONSE_ACCUMULATOR: Lazy<Arc<RwLock<Vec<String>>>> = ...;

pub fn get_response_accumulator() -> Arc<RwLock<Vec<String>>>

pub async fn clear_response_accumulator()
```

After tool execution:
```rust
let responses = get_response_accumulator().read().await;
let message = responses.join("\n");
clear_response_accumulator().await;
```

---

## 10. MCP Server

### 10.1 ChatMcpServer

**File**: `src/mcp_server.rs`

Exposes chat capabilities as a Model Context Protocol (MCP) server over gRPC.

#### Architecture

```
┌─────────────────────────────────────────────────────────┐
│ MCP Client (e.g., Claude Desktop)                       │
└─────────────────┬───────────────────────────────────────┘
                  │ gRPC
                  ▼
┌─────────────────────────────────────────────────────────┐
│ ChatMcpServer                                           │
│ ├─ Prompts (workstacks)                                │
│ ├─ Resources (skills, docs)                            │
│ └─ Tools (all registered tools)                        │
└─────────────────┬───────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────┐
│ ChatActor                                               │
└─────────────────────────────────────────────────────────┘
```

#### MCP Protocol Support

The server implements MCP via the generic `call` tunnel for JSON-RPC:

##### Prompts
- `prompts/list` - List available workstacks
- `prompts/get` - Get workstack details

##### Resources
- `resources/list` - List available skills and documentation
- `resources/read` - Read skill content

##### Tools
- `tools/list` - List all registered tools
- `tools/call` - Execute a tool

#### Key Types

```rust
pub struct ChatMcpServer {
    chat_actor: Arc<ChatActor>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Prompt {
    name: String,
    description: Option<String>,
    arguments: Option<Vec<PromptArgument>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Resource {
    uri: String,
    name: String,
    description: Option<String>,
    mime_type: Option<String>,
}
```

#### Implementation

```rust
#[tonic::async_trait]
impl McpService for ChatMcpServer {
    async fn initialize(
        &self,
        request: Request<InitializeRequest>,
    ) -> Result<Response<InitializeResponse>, Status>
    
    async fn health(
        &self,
        request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status>
    
    async fn list_tools(
        &self,
        request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status>
    
    async fn call_tool(
        &self,
        request: Request<CallToolRequest>,
    ) -> Result<Response<CallToolResponse>, Status>
    
    async fn call(
        &self,
        request: Request<ProtoMcpRequest>,
    ) -> Result<Response<ProtoMcpResponse>, Status>
    
    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<ReceiverStream<ProtoMcpEvent>>, Status>
}
```

#### Running the Server

```rust
pub async fn run_chat_mcp_server(
    addr: SocketAddr,
    chat_actor: Arc<ChatActor>,
) -> Result<()> {
    let server = ChatMcpServer::new(chat_actor);
    
    Server::builder()
        .add_service(McpServiceServer::new(server))
        .serve(addr)
        .await?;
    
    Ok(())
}
```

#### Standalone Binary

**File**: `src/main.rs`

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let addr: SocketAddr = std::env::var("OP_CHAT_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:50052".to_string())
        .parse()?;
        
    let config = ChatActorConfig::default();
    let (actor_obj, _handle) = ChatActor::new(config).await?;
    let actor = Arc::new(actor_obj);
    
    println!("Starting op-chat MCP server on {}", addr);
    run_chat_mcp_server(addr, actor).await?;
    
    Ok(())
}
```

Usage:
```bash
# Default: 0.0.0.0:50052
cargo run --bin op-chat

# Custom address
OP_CHAT_LISTEN=127.0.0.1:8080 cargo run --bin op-chat
```

---

## 11. Dependencies

### 11.1 Workspace Dependencies

```toml
tokio = { workspace = true, features = ["full"] }
serde = { workspace = true, features = ["derive"] }
simd-json = { workspace = true }
chrono = { workspace = true, features = ["serde"] }
uuid = { workspace = true, features = ["v4", "serde"] }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
anyhow = { workspace = true }
futures = { workspace = true }
regex = { workspace = true }
libc = { workspace = true }
```

### 11.2 D-Bus Support

```toml
zbus = { workspace = true }
```

### 11.3 gRPC Support

```toml
tonic = { workspace = true }
tokio-stream = { workspace = true }
prost = { workspace = true }
prost-types = { workspace = true }
```

### 11.4 Internal Dependencies

```toml
op-core = { path = "../op-core" }
op-tools = { path = "../op-tools" }
op-introspection = { path = "../op-introspection" }
op-llm = { path = "../op-llm" }
op-execution-tracker = { path = "../op-execution-tracker" }
op-agents = { path = "../op-agents" }
op-mcp = { path = "../op-mcp" }
```

### 11.5 Dev Dependencies

```toml
[dev-dependencies]
tokio-test = "0.4"
```

### 11.6 Build Dependencies

```toml
[build-dependencies]
tonic-build = "0.11"
prost-build = "0.12"
```

---

## 12. Build Configuration

### 12.1 build.rs

**File**: `build.rs`

Compiles protobuf definitions using `tonic-build`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir("src/orchestration/proto")
        .compile(
            &[
                "proto/orchestration.proto",
                "proto/agents.proto",
            ],
            &["proto"],
        )?;
    
    Ok(())
}
```

Generated files:
- `src/orchestration/proto/op_chat.orchestration.rs`
- `src/orchestration/proto/op_chat.agents.rs`

### 12.2 Cargo Features

Currently no features defined. All functionality is enabled by default.

### 12.3 Build Commands

```bash
# Development build
cargo build

# Release build
cargo build --release

# Build with verbose output
cargo build -vv

# Check without building
cargo check

# Run tests
cargo test

# Run clippy
cargo clippy

# Format code
cargo fmt
```

---

## 13. Usage Examples

### 13.1 Creating a ChatActor

```rust
use op_chat::{ChatActor, ChatActorConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ChatActorConfig {
        max_concurrent: 10,
        request_timeout_secs: 300,
        enable_tracking: true,
        max_history: 1000,
    };
    
    let (actor, handle) = ChatActor::new(config).await?;
    
    // Spawn actor task
    tokio::spawn(async move {
        actor.run().await
    });
    
    // Use handle to send requests
    // ...
    
    Ok(())
}
```

### 13.2 Sending a Chat Request

```rust
use op_chat::RpcRequest;

let request = RpcRequest::Chat {
    message: "Create an OVS bridge called br0".to_string(),
    session_id: "session-123".to_string(),
    model: None,
};

let response = handle.send(request).await?;

if response.success {
    println!("Response: {:?}", response.data);
} else {
    eprintln!("Error: {:?}", response.error);
}
```

### 13.3 Executing a Tool

```rust
use simd_json::json;

let request = RpcRequest::ExecuteTool {
    name: "ovs_create_bridge".to_string(),
    arguments: json!({
        "name": "br0"
    }),
    session_id: Some("session-123".to_string()),
};

let response = handle.send(request).await?;
```

### 13.4 Using NLAdminOrchestrator

```rust
use op_chat::NLAdminOrchestrator;
use op_llm::provider::ChatMessage;

let orchestrator = NLAdminOrchestrator::new(
    llm_provider,
    tool_executor,
    tool_registry,
);

let result = orchestrator.process(
    "Create an OVS bridge called br0",
    "session-123",
    &conversation_history,
).await?;

println!("Success: {}", result.success);
println!("Message: {}", result.message);
println!("Tools executed: {:?}", result.tools_executed);
```

### 13.5 Executing a Workstack

```rust
use op_chat::orchestration::{WorkstackExecutor, builtin_workstacks};

let workstacks = builtin_workstacks();
let rust_setup = workstacks.iter()
    .find(|w| w.id == "rust_project_setup")
    .unwrap();

let executor = WorkstackExecutor::new(tool_executor, agent_pool);

let result = executor.execute(
    rust_setup,
    HashMap::from([
        ("project_name".to_string(), json!("my-project")),
        ("project_path".to_string(), json!("/tmp/my-project")),
    ]),
    "session-123",
).await?;

println!("Success: {}", result.success);
println!("Phases completed: {:?}", result.phases_completed);
```

### 13.6 Using the GrpcAgentPool

```rust
use op_chat::orchestration::{GrpcAgentPool, AgentPoolConfig};

let config = AgentPoolConfig::default();
let pool = GrpcAgentPool::new(config).await?;

// Start session (starts run-on-connection agents)
let started = pool.start_session("session-123").await?;
println!("Started agents: {:?}", started);

// Execute on an agent
let result = pool.execute(
    "rust_pro",
    "build",
    json!({"path": ".", "release": true}),
    None,
).await?;

// End session (cleanup)
pool.end_session("session-123").await?;
```

### 13.7 Verifying Against Hallucination

```rust
use op_chat::ForcedExecutionOrchestrator;

let orchestrator = ForcedExecutionOrchestrator::new(executor);

orchestrator.start_turn().await;

let (results, check) = orchestrator.execute_and_verify(
    &llm_response,
    "session-123",
).await?;

if !check.verified {
    eprintln!("Hallucination detected!");
    for issue in check.issues {
        eprintln!("  {:?}: {}", issue.severity, issue.description);
    }
}
```

### 13.8 Running the MCP Server

```bash
# Start the server
cargo run --bin op-chat

# Or with custom address
OP_CHAT_LISTEN=127.0.0.1:8080 cargo run --bin op-chat
```

Connect from MCP client:
```json
{
  "mcpServers": {
    "op-chat": {
      "command": "cargo",
      "args": ["run", "--bin", "op-chat"],
      "env": {
        "OP_CHAT_LISTEN": "127.0.0.1:50052"
      }
    }
  }
}
```

---

## 14. Testing

### 14.1 Unit Tests

Each module includes unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_tool_execution() {
        // Test implementation
    }
}
```

Run tests:
```bash
cargo test
cargo test -- --nocapture  # Show output
cargo test --lib            # Library tests only
```

### 14.2 Integration Tests

Integration tests would go in `tests/` directory (not currently present).

### 14.3 Test Coverage

Key areas to test:
- Tool execution and tracking
- Rate limiting
- Hallucination detection
- Session management
- Workstack execution
- Agent pool connection management
- MCP protocol compliance

---

## 15. Performance Considerations

### 15.1 SIMD JSON

Uses `simd-json` for faster JSON parsing:
- 2-3x faster than `serde_json`
- SIMD instructions for parsing
- Important for high-frequency tool execution

### 15.2 gRPC for Agent Communication

- Binary protocol (faster than JSON-RPC)
- HTTP/2 multiplexing
- Streaming support for long operations
- Connection pooling

### 15.3 Connection Pooling

GrpcAgentPool maintains persistent connections:
- Avoid connection overhead
- Health checks to detect failures
- Automatic reconnection

### 15.4 Rate Limiting

Prevents resource exhaustion:
- Per-session limits
- Global concurrent execution limit
- Sliding window algorithm

### 15.5 Async/Await

Fully async architecture:
- Non-blocking I/O
- Efficient resource usage
- Tokio runtime

---

## 16. Security Considerations

### 16.1 Tool Execution Tracking

All tool executions are logged:
- Who executed (session ID)
- What was executed (tool name + args)
- When (timestamp)
- Result (success/failure)

### 16.2 Rate Limiting

Prevents abuse:
- Per-session limits
- Cannot exhaust system resources
- Configurable thresholds

### 16.3 Anti-Hallucination

Prevents LLM from claiming false actions:
- All claims verified against execution log
- Cannot bypass tool execution
- Forced response tools

### 16.4 WireGuard Authentication

Sessions can be authenticated:
- Cryptographic peer identity
- Controller vs regular user roles
- Audit trail with peer public key

### 16.5 Input Validation

Tool arguments validated before execution:
- Type checking
- Required fields
- Value constraints

---

## 17. Future Enhancements

### 17.1 Planned Features

- [ ] Persistent session storage (database)
- [ ] Workstack templates and customization
- [ ] Skill marketplace
- [ ] Agent hot-reload
- [ ] Distributed agent pool (multi-host)
- [ ] Advanced metrics and monitoring
- [ ] Workstack visualization
- [ ] Interactive debugging mode

### 17.2 Performance Improvements

- [ ] Tool execution caching
- [ ] Parallel phase execution in workstacks
- [ ] Lazy agent initialization
- [ ] Response streaming to client

### 17.3 Protocol Enhancements

- [ ] WebSocket support for MCP
- [ ] GraphQL API
- [ ] REST API fallback
- [ ] Batch tool execution API

---

## 18. Troubleshooting

### 18.1 Common Issues

#### Agent Connection Failures

```
Error: Failed to connect to agent rust_pro
```

**Solution**: Check that agent is running and address is correct:
```bash
# Check agent status
netstat -tlnp | grep 50051

# Check logs
journalctl -u op-agent-rust-pro
```

#### Rate Limit Exceeded

```
Error: Rate limit exceeded: 60 executions per minute
```

**Solution**: Increase rate limits in config:
```rust
let config = ChatActorConfig {
    rate_limit: RateLimitConfig {
        max_per_minute: 120,
        max_per_hour: 1000,
        ..Default::default()
    },
    ..Default::default()
};
```

#### Hallucination Detected

```
Warning: Hallucination detected - UnverifiedActionClaim
```

**Solution**: This is working as intended. The LLM tried to claim an action without executing it. The system prevented the hallucination.

### 18.2 Debugging

Enable debug logging:
```bash
RUST_LOG=op_chat=debug cargo run
```

Enable trace logging:
```bash
RUST_LOG=op_chat=trace cargo run
```

### 18.3 Performance Profiling

```bash
# CPU profiling
cargo flamegraph --bin op-chat

# Memory profiling
cargo valgrind --bin op-chat
```

---

## 19. Contributing

### 19.1 Code Style

Follow repository guidelines in `/AGENTS.md`:
- `rustfmt` defaults
- `cargo clippy` clean
- Comprehensive error handling
- Tracing for observability

### 19.2 Adding New Workstacks

1. Define workstack in `src/orchestration/mod.rs`
2. Add to `builtin_workstacks()` function
3. Document phases and dependencies
4. Add tests

### 19.3 Adding New Skills

1. Create markdown file in `src/orchestration/skills_builtin/`
2. Define skill metadata
3. Register in skill registry
4. Document usage

### 19.4 Adding New Agents

1. Define protobuf service in `proto/orchestration.proto`
2. Implement agent service
3. Add to `run_on_connection` list
4. Document operations

---

## 20. References

### 20.1 Related Crates

- `op-core` - Core types and utilities
- `op-tools` - Tool registry and execution
- `op-llm` - LLM provider abstraction
- `op-execution-tracker` - Execution audit trail
- `op-agents` - Agent implementations
- `op-mcp` - MCP protocol implementation

### 20.2 External Documentation

- [Model Context Protocol](https://modelcontextprotocol.io/)
- [gRPC](https://grpc.io/)
- [Tokio](https://tokio.rs/)
- [Tonic](https://github.com/hyperium/tonic)

### 20.3 Design Documents

- `/AGENTS.md` - Repository guidelines
- `/spec.md` - Overall system specification
- `/HIERARCHICAL_DBUS_DESIGN.md` - D-Bus architecture

---

**Document Version**: 1.0  
**Last Updated**: 2026-02-16  
**Maintainer**: op-dbus-v2 team
