//! Intent-Based Tool Executor
//!
//! This module provides a DETERMINISTIC execution layer that:
//! 1. Parses user intent from natural language
//! 2. Maps intents to registered tools
//! 3. Executes tools DIRECTLY (not via LLM)
//! 4. Returns verified results
//!
//! This bypasses LLM tool-calling limitations (like HuggingFace)
//! and ensures tools are ACTUALLY executed, not hallucinated.

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use op_core::{ToolRequest, ToolResult};
use op_tools::ToolRegistry;

/// Detected intent from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedIntent {
    /// The action (create, delete, list, get, etc.)
    pub action: IntentAction,
    /// The resource type (ovs_bridge, systemd_service, etc.)
    pub resource: ResourceType,
    /// Extracted parameters
    pub params: HashMap<String, Value>,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Original user input
    pub original_input: String,
    /// Matched tool name (if found)
    pub matched_tool: Option<String>,
}

/// Action types we can detect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentAction {
    Create,
    Delete,
    List,
    Get,
    Update,
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    Add,
    Remove,
    Check,
    Query,
    Unknown,
}

impl IntentAction {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "create" | "make" | "add" | "new" | "setup" | "configure" => Self::Create,
            "delete" | "remove" | "destroy" | "drop" | "rm" => Self::Delete,
            "list" | "show" | "ls" | "get all" | "display" => Self::List,
            "get" | "fetch" | "retrieve" | "info" | "describe" => Self::Get,
            "update" | "modify" | "change" | "set" | "edit" => Self::Update,
            "start" | "begin" | "launch" | "run" => Self::Start,
            "stop" | "halt" | "kill" | "terminate" => Self::Stop,
            "restart" | "reboot" | "reload" => Self::Restart,
            "enable" | "activate" | "turn on" => Self::Enable,
            "disable" | "deactivate" | "turn off" => Self::Disable,
            "check" | "verify" | "test" | "validate" => Self::Check,
            "query" | "search" | "find" => Self::Query,
            _ => Self::Unknown,
        }
    }
}

/// Resource types we can operate on
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    OvsBridge,
    OvsPort,
    OvsFlow,
    SystemdService,
    SystemdUnit,
    NetworkInterface,
    File,
    Process,
    Container,
    Package,
    User,
    Unknown,
}

impl ResourceType {
    fn from_str(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("bridge") || lower.contains("ovs") && lower.contains("br") {
            Self::OvsBridge
        } else if lower.contains("port") && (lower.contains("ovs") || lower.contains("bridge")) {
            Self::OvsPort
        } else if lower.contains("flow") {
            Self::OvsFlow
        } else if lower.contains("service") || lower.contains("systemd") {
            Self::SystemdService
        } else if lower.contains("unit") {
            Self::SystemdUnit
        } else if lower.contains("interface") || lower.contains("network") || lower.contains("nic") {
            Self::NetworkInterface
        } else if lower.contains("file") {
            Self::File
        } else if lower.contains("process") || lower.contains("proc") {
            Self::Process
        } else if lower.contains("container") || lower.contains("docker") || lower.contains("lxc") {
            Self::Container
        } else if lower.contains("package") || lower.contains("pkg") {
            Self::Package
        } else if lower.contains("user") {
            Self::User
        } else {
            Self::Unknown
        }
    }
}

/// Result of intent-based execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentExecutionResult {
    /// Whether execution was successful
    pub success: bool,
    /// Human-readable response
    pub response: String,
    /// The detected intent
    pub intent: DetectedIntent,
    /// Tool that was executed (if any)
    pub executed_tool: Option<String>,
    /// Raw tool result (if executed)
    pub tool_result: Option<Value>,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Intent-based tool executor
///
/// This is the CORRECT way to ensure tools are executed:
/// - Parse user intent deterministically
/// - Map to registered tools
/// - Execute directly (no LLM in the loop)
/// - Return verified results
pub struct IntentExecutor {
    tool_registry: Arc<ToolRegistry>,
    intent_patterns: Vec<IntentPattern>,
    tool_mappings: HashMap<(IntentAction, ResourceType), String>,
}

/// Pattern for detecting intents
struct IntentPattern {
    regex: Regex,
    action: IntentAction,
    resource: ResourceType,
    param_extractors: Vec<ParamExtractor>,
}

/// Extracts parameters from matched text
struct ParamExtractor {
    name: String,
    regex: Regex,
    group: usize,
}

impl IntentExecutor {
    /// Create new intent executor
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        let mut executor = Self {
            tool_registry,
            intent_patterns: Vec::new(),
            tool_mappings: HashMap::new(),
        };
        
        executor.register_default_patterns();
        executor.register_default_mappings();
        
        executor
    }

    /// Register default intent patterns
    fn register_default_patterns(&mut self) {
        // OVS Bridge patterns
        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:create|make|add|setup)\s+(?:an?\s+)?(?:ovs\s+)?bridge\s+(?:called\s+|named\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Create,
            resource: ResourceType::OvsBridge,
            param_extractors: vec![ParamExtractor {
                name: "name".to_string(),
                regex: Regex::new(r"(?i)bridge\s+(?:called\s+|named\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:delete|remove|destroy|drop)\s+(?:the\s+)?(?:ovs\s+)?bridge\s+([\w-]+)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Delete,
            resource: ResourceType::OvsBridge,
            param_extractors: vec![ParamExtractor {
                name: "name".to_string(),
                regex: Regex::new(r"(?i)bridge\s+([\w-]+)").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:list|show|get|display)\s+(?:all\s+)?(?:ovs\s+)?bridges").expect("hardcoded regex pattern is valid"),
            action: IntentAction::List,
            resource: ResourceType::OvsBridge,
            param_extractors: vec![],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:what|which)\s+(?:ovs\s+)?bridges\s+(?:exist|are there)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::List,
            resource: ResourceType::OvsBridge,
            param_extractors: vec![],
        });

        // OVS Port patterns
        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:add|create)\s+port\s+([\w-]+)\s+to\s+(?:bridge\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Add,
            resource: ResourceType::OvsPort,
            param_extractors: vec![
                ParamExtractor {
                    name: "port".to_string(),
                    regex: Regex::new(r"(?i)port\s+([\w-]+)").expect("hardcoded regex pattern is valid"),
                    group: 1,
                },
                ParamExtractor {
                    name: "bridge".to_string(),
                    regex: Regex::new(r"(?i)to\s+(?:bridge\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
                    group: 1,
                },
            ],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:list|show|get)\s+ports\s+(?:on|for|of)\s+(?:bridge\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::List,
            resource: ResourceType::OvsPort,
            param_extractors: vec![ParamExtractor {
                name: "bridge".to_string(),
                regex: Regex::new(r"(?i)(?:on|for|of)\s+(?:bridge\s+)?([\w-]+)").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        // Systemd patterns
        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:start|begin|launch)\s+(?:the\s+)?(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Start,
            resource: ResourceType::SystemdService,
            param_extractors: vec![ParamExtractor {
                name: "unit".to_string(),
                regex: Regex::new(r"(?i)(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:stop|halt|kill)\s+(?:the\s+)?(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Stop,
            resource: ResourceType::SystemdService,
            param_extractors: vec![ParamExtractor {
                name: "unit".to_string(),
                regex: Regex::new(r"(?i)(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:restart|reboot|reload)\s+(?:the\s+)?(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Restart,
            resource: ResourceType::SystemdService,
            param_extractors: vec![ParamExtractor {
                name: "unit".to_string(),
                regex: Regex::new(r"(?i)(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:status|state)\s+(?:of\s+)?(?:the\s+)?(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Get,
            resource: ResourceType::SystemdService,
            param_extractors: vec![ParamExtractor {
                name: "unit".to_string(),
                regex: Regex::new(r"(?i)(?:service\s+)?([\w-]+)(?:\.service)?").expect("hardcoded regex pattern is valid"),
                group: 1,
            }],
        });

        // Check OVS availability
        self.intent_patterns.push(IntentPattern {
            regex: Regex::new(r"(?i)(?:check|is|verify)\s+(?:if\s+)?ovs\s+(?:is\s+)?(?:running|available|installed)").expect("hardcoded regex pattern is valid"),
            action: IntentAction::Check,
            resource: ResourceType::OvsBridge,
            param_extractors: vec![],
        });
    }

    /// Register default tool mappings
    fn register_default_mappings(&mut self) {
        // OVS mappings
        self.tool_mappings.insert(
            (IntentAction::Create, ResourceType::OvsBridge),
            "ovs_create_bridge".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Delete, ResourceType::OvsBridge),
            "ovs_delete_bridge".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::List, ResourceType::OvsBridge),
            "ovs_list_bridges".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Get, ResourceType::OvsBridge),
            "ovs_get_bridge".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Check, ResourceType::OvsBridge),
            "ovs_check_available".to_string(),
        );

        // OVS Port mappings
        self.tool_mappings.insert(
            (IntentAction::Add, ResourceType::OvsPort),
            "ovs_add_port".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Remove, ResourceType::OvsPort),
            "ovs_delete_port".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::List, ResourceType::OvsPort),
            "ovs_list_ports".to_string(),
        );

        // Systemd mappings (using D-Bus tools)
        self.tool_mappings.insert(
            (IntentAction::Start, ResourceType::SystemdService),
            "dbus_systemd_start_unit".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Stop, ResourceType::SystemdService),
            "dbus_systemd_stop_unit".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Restart, ResourceType::SystemdService),
            "dbus_systemd_restart_unit".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Get, ResourceType::SystemdService),
            "dbus_systemd_get_unit".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Enable, ResourceType::SystemdService),
            "dbus_systemd_enable_unit".to_string(),
        );
        self.tool_mappings.insert(
            (IntentAction::Disable, ResourceType::SystemdService),
            "dbus_systemd_disable_unit".to_string(),
        );
    }

    /// Parse user input to detect intent
    pub fn parse_intent(&self, input: &str) -> DetectedIntent {
        let input_lower = input.to_lowercase();
        
        // Try each pattern
        for pattern in &self.intent_patterns {
            if pattern.regex.is_match(&input_lower) {
                // Extract parameters
                let mut params = HashMap::new();
                for extractor in &pattern.param_extractors {
                    if let Some(captures) = extractor.regex.captures(input) {
                        if let Some(value) = captures.get(extractor.group) {
                            params.insert(
                                extractor.name.clone(),
                                Value::String(value.as_str().to_string()),
                            );
                        }
                    }
                }

                // Find matched tool
                let matched_tool = self
                    .tool_mappings
                    .get(&(pattern.action, pattern.resource))
                    .cloned();

                return DetectedIntent {
                    action: pattern.action,
                    resource: pattern.resource,
                    params,
                    confidence: 0.9, // High confidence for regex match
                    original_input: input.to_string(),
                    matched_tool,
                };
            }
        }

        // Fallback: try to detect action and resource from keywords
        let action = self.detect_action_from_keywords(&input_lower);
        let resource = self.detect_resource_from_keywords(&input_lower);
        let matched_tool = self.tool_mappings.get(&(action, resource)).cloned();

        DetectedIntent {
            action,
            resource,
            params: HashMap::new(),
            confidence: if matched_tool.is_some() { 0.5 } else { 0.2 },
            original_input: input.to_string(),
            matched_tool,
        }
    }

    /// Detect action from keywords
    fn detect_action_from_keywords(&self, input: &str) -> IntentAction {
        let action_keywords = [
            ("create", IntentAction::Create),
            ("make", IntentAction::Create),
            ("add", IntentAction::Create),
            ("delete", IntentAction::Delete),
            ("remove", IntentAction::Delete),
            ("list", IntentAction::List),
            ("show", IntentAction::List),
            ("get", IntentAction::Get),
            ("start", IntentAction::Start),
            ("stop", IntentAction::Stop),
            ("restart", IntentAction::Restart),
            ("enable", IntentAction::Enable),
            ("disable", IntentAction::Disable),
            ("check", IntentAction::Check),
        ];

        for (keyword, action) in action_keywords {
            if input.contains(keyword) {
                return action;
            }
        }

        IntentAction::Unknown
    }

    /// Detect resource from keywords
    fn detect_resource_from_keywords(&self, input: &str) -> ResourceType {
        ResourceType::from_str(input)
    }

    /// Execute based on detected intent
    pub async fn execute(&self, input: &str) -> Result<IntentExecutionResult> {
        let start = std::time::Instant::now();
        
        // Parse intent
        let intent = self.parse_intent(input);
        
        info!(
            "Detected intent: action={:?}, resource={:?}, confidence={}, tool={:?}",
            intent.action, intent.resource, intent.confidence, intent.matched_tool
        );

        // Check if we have a tool mapping
        let tool_name = match &intent.matched_tool {
            Some(name) => name.clone(),
            None => {
                return Ok(IntentExecutionResult {
                    success: false,
                    response: format!(
                        "I couldn't determine which tool to use for: '{}'. \n\
                         Detected action: {:?}, resource: {:?}\n\
                         Please be more specific or use a direct tool name.",
                        input, intent.action, intent.resource
                    ),
                    intent,
                    executed_tool: None,
                    tool_result: None,
                    execution_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        };

        // Check if tool exists
        let tool = match self.tool_registry.get(&tool_name).await {
            Some(t) => t,
            None => {
                return Ok(IntentExecutionResult {
                    success: false,
                    response: format!(
                        "Tool '{}' is not registered. \n\
                         This may be a configuration issue.",
                        tool_name
                    ),
                    intent,
                    executed_tool: Some(tool_name),
                    tool_result: None,
                    execution_time_ms: start.elapsed().as_millis() as u64,
                });
            }
        };

        // Build tool request
        let arguments = self.build_arguments(&intent);
        let request = ToolRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool_name: tool_name.clone(),
            arguments,
            timeout_ms: Some(30000),
        };

        info!("Executing tool '{}' with args: {:?}", tool_name, request.arguments);

        // Execute tool
        let result = tool.execute(request).await;
        let execution_time_ms = start.elapsed().as_millis() as u64;

        // Generate response
        let (success, response) = self.generate_response(&intent, &tool_name, &result);

        Ok(IntentExecutionResult {
            success,
            response,
            intent,
            executed_tool: Some(tool_name),
            tool_result: Some(result.content.clone()),
            execution_time_ms,
        })
    }

    /// Build arguments from intent parameters
    fn build_arguments(&self, intent: &DetectedIntent) -> Value {
        if intent.params.is_empty() {
            json!({})
        } else {
            Value::Object(intent.params.clone().into_iter().collect())
        }
    }

    /// Generate human-readable response from tool result
    fn generate_response(
        &self,
        intent: &DetectedIntent,
        tool_name: &str,
        result: &ToolResult,
    ) -> (bool, String) {
        if result.success {
            let action_past = match intent.action {
                IntentAction::Create => "Created",
                IntentAction::Delete => "Deleted",
                IntentAction::List => "Listed",
                IntentAction::Get => "Retrieved",
                IntentAction::Start => "Started",
                IntentAction::Stop => "Stopped",
                IntentAction::Restart => "Restarted",
                IntentAction::Enable => "Enabled",
                IntentAction::Disable => "Disabled",
                IntentAction::Add => "Added",
                IntentAction::Remove => "Removed",
                IntentAction::Check => "Checked",
                IntentAction::Update => "Updated",
                IntentAction::Query => "Queried",
                IntentAction::Unknown => "Executed",
            };

            let resource_name = match intent.resource {
                ResourceType::OvsBridge => "OVS bridge",
                ResourceType::OvsPort => "OVS port",
                ResourceType::OvsFlow => "OVS flow",
                ResourceType::SystemdService => "systemd service",
                ResourceType::SystemdUnit => "systemd unit",
                ResourceType::NetworkInterface => "network interface",
                ResourceType::File => "file",
                ResourceType::Process => "process",
                ResourceType::Container => "container",
                ResourceType::Package => "package",
                ResourceType::User => "user",
                ResourceType::Unknown => "resource",
            };

            // Format result data
            let data_str = if let Some(data) = result.content.get("data") {
                format!("\n\nResult:\n{}", simd_json::to_string_pretty(data).unwrap_or_default())
            } else {
                format!("\n\nResult:\n{}", simd_json::to_string_pretty(&result.content).unwrap_or_default())
            };

            (
                true,
                format!(
                    "✅ {} {} successfully via {} (native protocol, not CLI).{}",
                    action_past, resource_name, tool_name, data_str
                ),
            )
        } else {
            let error_msg = result
                .content
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");

            (
                false,
                format!(
                    "❌ Failed to execute '{}': {}\n\nTool: {}",
                    intent.original_input, error_msg, tool_name
                ),
            )
        }
    }

    /// Check if input looks like a system operation (vs general chat)
    pub fn is_system_operation(&self, input: &str) -> bool {
        let intent = self.parse_intent(input);
        intent.matched_tool.is_some() || intent.confidence > 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_bridge() {
        let registry = Arc::new(ToolRegistry::new());
        let executor = IntentExecutor::new(registry);

        let intent = executor.parse_intent("create ovs bridge ovsbr0");
        assert_eq!(intent.action, IntentAction::Create);
        assert_eq!(intent.resource, ResourceType::OvsBridge);
        assert_eq!(intent.params.get("name").and_then(|v| v.as_str()), Some("ovsbr0"));
        assert_eq!(intent.matched_tool, Some("ovs_create_bridge".to_string()));
    }

    #[test]
    fn test_parse_list_bridges() {
        let registry = Arc::new(ToolRegistry::new());
        let executor = IntentExecutor::new(registry);

        let intent = executor.parse_intent("show all ovs bridges");
        assert_eq!(intent.action, IntentAction::List);
        assert_eq!(intent.resource, ResourceType::OvsBridge);
        assert_eq!(intent.matched_tool, Some("ovs_list_bridges".to_string()));
    }

    #[test]
    fn test_parse_delete_bridge() {
        let registry = Arc::new(ToolRegistry::new());
        let executor = IntentExecutor::new(registry);

        let intent = executor.parse_intent("delete the bridge br0");
        assert_eq!(intent.action, IntentAction::Delete);
        assert_eq!(intent.resource, ResourceType::OvsBridge);
        assert_eq!(intent.matched_tool, Some("ovs_delete_bridge".to_string()));
    }
}
