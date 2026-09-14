//! Workflow Context - Execution context for workflows
//!
//! Provides access to:
//! - Variables and state
//! - Plugin instances
//! - Tool execution
//! - Logging and metrics

use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Workflow execution context
pub struct WorkflowContext {
    /// Workflow ID
    pub workflow_id: String,
    /// Execution ID (unique per run)
    pub execution_id: String,
    /// Variables available during execution
    pub variables: Arc<RwLock<HashMap<String, Value>>>,
    /// Execution log
    log: Arc<RwLock<Vec<LogEntry>>>,
}

/// Log entry for workflow execution
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: LogLevel,
    pub node_id: Option<String>,
    pub message: String,
}

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl WorkflowContext {
    /// Create a new workflow context
    pub fn new(workflow_id: &str) -> Self {
        Self {
            workflow_id: workflow_id.to_string(),
            execution_id: uuid::Uuid::new_v4().to_string(),
            variables: Arc::new(RwLock::new(HashMap::new())),
            log: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get a variable
    pub async fn get_variable(&self, name: &str) -> Option<Value> {
        let vars = self.variables.read().await;
        vars.get(name).cloned()
    }

    /// Set a variable
    pub async fn set_variable(&self, name: &str, value: Value) {
        let mut vars = self.variables.write().await;
        vars.insert(name.to_string(), value);
    }

    /// Get all variables
    pub async fn get_all_variables(&self) -> HashMap<String, Value> {
        let vars = self.variables.read().await;
        vars.clone()
    }

    /// Log a message
    pub async fn log(&self, level: LogLevel, node_id: Option<&str>, message: &str) {
        let entry = LogEntry {
            timestamp: chrono::Utc::now(),
            level,
            node_id: node_id.map(String::from),
            message: message.to_string(),
        };
        let mut log = self.log.write().await;
        log.push(entry);
    }

    /// Log debug message
    pub async fn debug(&self, node_id: Option<&str>, message: &str) {
        self.log(LogLevel::Debug, node_id, message).await;
    }

    /// Log info message
    pub async fn info(&self, node_id: Option<&str>, message: &str) {
        self.log(LogLevel::Info, node_id, message).await;
    }

    /// Log warning message
    pub async fn warn(&self, node_id: Option<&str>, message: &str) {
        self.log(LogLevel::Warn, node_id, message).await;
    }

    /// Log error message
    pub async fn error(&self, node_id: Option<&str>, message: &str) {
        self.log(LogLevel::Error, node_id, message).await;
    }

    /// Get execution log
    pub async fn get_log(&self) -> Vec<LogEntry> {
        let log = self.log.read().await;
        log.clone()
    }

    /// Interpolate variables in a string
    pub async fn interpolate(&self, template: &str) -> String {
        let vars = self.variables.read().await;
        let mut result = template.to_string();

        for (name, value) in vars.iter() {
            let pattern = format!("${{{}}}", name);
            let replacement = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&pattern, &replacement);
        }

        result
    }

    /// Interpolate variables in a JSON value
    pub async fn interpolate_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.interpolate(s).await),
            Value::Object(obj) => {
                let mut new_obj = simd_json::value::owned::Object::new();
                for (k, v) in obj.iter() {
                    new_obj.insert(k.clone(), Box::pin(self.interpolate_value(v)).await);
                }
                Value::Object(Box::new(new_obj))
            }
            Value::Array(arr) => {
                let mut new_arr = Vec::new();
                for v in arr {
                    new_arr.push(Box::pin(self.interpolate_value(v)).await);
                }
                Value::Array(new_arr)
            }
            other => other.clone(),
        }
    }
}

impl Default for WorkflowContext {
    fn default() -> Self {
        Self::new("default")
    }
}
