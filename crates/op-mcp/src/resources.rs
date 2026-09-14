//! Resource Registry for MCP
//!
//! Provides documentation resources served via MCP resources protocol.

use serde::{Deserialize, Serialize};

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Resource template information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplateInfo {
    pub uri_template: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Resource registry
pub struct ResourceRegistry {
    resources: Vec<ResourceInfo>,
    templates: Vec<ResourceTemplateInfo>,
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceRegistry {
    pub fn new() -> Self {
        let resources = vec![
            ResourceInfo {
                uri: "docs://system-prompt".to_string(),
                name: "System Prompt".to_string(),
                description: Some("System prompt for op-mcp".to_string()),
                mime_type: Some("text/plain".to_string()),
            },
            ResourceInfo {
                uri: "docs://architecture".to_string(),
                name: "Architecture".to_string(),
                description: Some("System architecture documentation".to_string()),
                mime_type: Some("text/markdown".to_string()),
            },
        ];
        let templates = vec![ResourceTemplateInfo {
            uri_template: "docs://{name}".to_string(),
            name: "Documentation".to_string(),
            description: Some("Read bundled op-mcp documentation resources".to_string()),
            mime_type: Some("text/plain".to_string()),
        }];
        Self {
            resources,
            templates,
        }
    }

    /// Advertise every sealed plugin as `blob://<plugin_id>`.
    pub fn with_blob_catalog() -> Self {
        let mut registry = Self::new();
        let dir = std::env::var("OP_BLOB_CATALOG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
        if let Some(ids) = op_blob::catalog::read_manifest_plugin_ids(&dir) {
            for id in ids {
                registry.add_resource(ResourceInfo {
                    uri: format!("blob://{id}"),
                    name: format!("{id} schema"),
                    description: Some(format!("Sealed PluginSchema JSON for {id}")),
                    mime_type: Some("application/json".to_string()),
                });
            }
        }
        registry.templates.push(ResourceTemplateInfo {
            uri_template: "blob://{plugin_id}".to_string(),
            name: "Sealed plugin schema".to_string(),
            description: Some("Read PluginSchema JSON from the SHM blob catalog".to_string()),
            mime_type: Some("application/json".to_string()),
        });
        registry
    }

    pub fn add_resource(&mut self, resource: ResourceInfo) {
        self.resources.push(resource);
    }

    pub fn list_resources(&self) -> &[ResourceInfo] {
        &self.resources
    }

    pub fn list_templates(&self) -> &[ResourceTemplateInfo] {
        &self.templates
    }

    pub fn get_resource(&self, uri: &str) -> Option<&ResourceInfo> {
        self.resources.iter().find(|r| r.uri == uri)
    }

    pub async fn read_resource(&self, uri: &str) -> Option<String> {
        match uri {
            "docs://system-prompt" => Some(self.generate_system_prompt().await),
            "docs://architecture" => Some(ARCHITECTURE_DOC.to_string()),
            blob if blob.starts_with("blob://") => {
                let plugin_id = blob.trim_start_matches("blob://").trim();
                let dir = std::env::var("OP_BLOB_CATALOG_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        std::path::PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR)
                    });
                crate::blob_schema::read_schema_resource(&dir, plugin_id)
            }
            _ => None,
        }
    }

    async fn generate_system_prompt(&self) -> String {
        "You are a helpful assistant with access to system tools.".to_string()
    }
}

const ARCHITECTURE_DOC: &str = r#"# op-mcp Architecture

## Overview

op-mcp is a unified MCP (Model Context Protocol) server supporting multiple transports:

- **Stdio**: Standard input/output for CLI integration
- **HTTP**: REST endpoints with SSE support
- **WebSocket**: Full-duplex communication
- **gRPC**: High-performance RPC (optional)

## Components

### McpServer
Core server handling all MCP protocol logic. Transport-agnostic.

### Transport Layer
Abstract `Transport` trait with implementations for each protocol.

### Tool System
`ToolExecutor` trait allows pluggable tool backends.

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `compact_mode` | false | Use 5 lazy meta-tools instead of all |
| `max_tools` | 500 | Maximum tools to expose |
| `blocked_patterns` | [...] | Tool patterns to block |
"#;
