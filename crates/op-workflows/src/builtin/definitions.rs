//! Built-in Workflow Definitions
//!
//! Pre-defined workflows for common operations.

use crate::flow::{WorkflowDefinition, WorkflowNodeDef};
use crate::node::NodeConnection;
use simd_json::json;

/// Get all built-in workflow definitions
pub fn builtin_workflows() -> Vec<WorkflowDefinition> {
    vec![
        cargo_check_workflow(),
        service_status_workflow(),
        deploy_workflow(),
        code_review_workflow(),
    ]
}

/// Cargo check workflow
fn cargo_check_workflow() -> WorkflowDefinition {
    WorkflowDefinition::new(
        "cargo_check",
        "Cargo Check",
        "Run cargo check, clippy, and format",
    )
    .with_node(WorkflowNodeDef {
        id: "check".into(),
        node_type: "tool:cargo_check".into(),
        name: "Cargo Check".into(),
        config: json!({"path": "."}),
        position: Some((100.0, 100.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "clippy".into(),
        node_type: "tool:cargo_clippy".into(),
        name: "Cargo Clippy".into(),
        config: json!({"path": ".", "fix": false}),
        position: Some((300.0, 100.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "format".into(),
        node_type: "tool:cargo_fmt".into(),
        name: "Cargo Format".into(),
        config: json!({"path": ".", "check": true}),
        position: Some((500.0, 100.0)),
    })
    .with_connection(NodeConnection::new("check", "result", "clippy", "source"))
    .with_connection(NodeConnection::new("clippy", "result", "format", "source"))
}

/// Service status workflow
fn service_status_workflow() -> WorkflowDefinition {
    WorkflowDefinition::new(
        "service_status",
        "Service Status",
        "Check status of system services",
    )
    .with_node(WorkflowNodeDef {
        id: "list_units".into(),
        node_type: "tool:systemd_list_units".into(),
        name: "List Units".into(),
        config: json!({"pattern": "*.service"}),
        position: Some((100.0, 100.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "filter_failed".into(),
        node_type: "tool:filter".into(),
        name: "Filter Failed".into(),
        config: json!({"field": "active_state", "value": "failed"}),
        position: Some((300.0, 100.0)),
    })
    .with_connection(NodeConnection::new(
        "list_units",
        "units",
        "filter_failed",
        "input",
    ))
}

/// Deployment workflow
fn deploy_workflow() -> WorkflowDefinition {
    WorkflowDefinition::new(
        "deploy",
        "Deploy Application",
        "Build, test, and deploy application",
    )
    .with_node(WorkflowNodeDef {
        id: "build".into(),
        node_type: "tool:cargo_build".into(),
        name: "Build".into(),
        config: json!({"release": true}),
        position: Some((100.0, 100.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "test".into(),
        node_type: "tool:cargo_test".into(),
        name: "Test".into(),
        config: json!({}),
        position: Some((300.0, 100.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "deploy".into(),
        node_type: "tool:deploy".into(),
        name: "Deploy".into(),
        config: json!({"target": "production"}),
        position: Some((500.0, 100.0)),
    })
    .with_connection(NodeConnection::new("build", "binary", "test", "source"))
    .with_connection(NodeConnection::new("test", "result", "deploy", "artifact"))
}

/// Code review workflow
fn code_review_workflow() -> WorkflowDefinition {
    WorkflowDefinition::new(
        "code_review",
        "Code Review",
        "Multi-perspective code review",
    )
    .with_node(WorkflowNodeDef {
        id: "security".into(),
        node_type: "agent:security_reviewer".into(),
        name: "Security Review".into(),
        config: json!({"focus": "security"}),
        position: Some((100.0, 50.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "architecture".into(),
        node_type: "agent:architect".into(),
        name: "Architecture Review".into(),
        config: json!({"focus": "design"}),
        position: Some((100.0, 150.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "performance".into(),
        node_type: "agent:performance_analyst".into(),
        name: "Performance Review".into(),
        config: json!({"focus": "performance"}),
        position: Some((100.0, 250.0)),
    })
    .with_node(WorkflowNodeDef {
        id: "consolidate".into(),
        node_type: "merge".into(),
        name: "Consolidate".into(),
        config: json!({}),
        position: Some((300.0, 150.0)),
    })
    .with_connection(NodeConnection::new(
        "security",
        "findings",
        "consolidate",
        "security",
    ))
    .with_connection(NodeConnection::new(
        "architecture",
        "findings",
        "consolidate",
        "architecture",
    ))
    .with_connection(NodeConnection::new(
        "performance",
        "findings",
        "consolidate",
        "performance",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_workflows_valid() {
        let workflows = builtin_workflows();
        assert!(!workflows.is_empty());

        for wf in workflows {
            assert!(wf.validate().is_ok(), "Workflow '{}' is invalid", wf.id);
        }
    }
}
