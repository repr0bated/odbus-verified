//! DBus FTS Search Tool
//!
//! Provides semantic search across all DBus capabilities using the FTS5 indexer

use crate::{Tool, ToolDefinition, ToolRequest, ToolResult};
use async_trait::async_trait;
use op_core::types::BusType;
use op_introspection::IndexerManager;
use simd_json::json;
use std::sync::Arc;

/// Tool for searching DBus methods, properties, and signals using FTS
pub struct DbusSearchTool {
    indexer: Arc<IndexerManager>,
}

impl DbusSearchTool {
    pub fn new(indexer: Arc<IndexerManager>) -> Self {
        Self { indexer }
    }
}

#[async_trait]
impl Tool for DbusSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_dbus".to_string(),
            description:
                "Search for DBus methods, properties, and signals using semantic queries. \
                         Supports natural language queries like 'network wifi', 'bluetooth power', \
                         'systemd service control', etc."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (e.g., 'network', 'bluetooth power', 'systemd restart')"
                    },
                    "item_type": {
                        "type": "string",
                        "enum": ["method", "property", "signal", "all"],
                        "description": "Type of DBus item to search for (default: all)",
                        "default": "all"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)",
                        "default": 20,
                        "minimum": 1,
                        "maximum": 100
                    }
                },
                "required": ["query"]
            }),
            category: Some("dbus".to_string()),
            tags: vec![
                "search".to_string(),
                "discovery".to_string(),
                "fts".to_string(),
            ],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        // Parse arguments
        let query = match request.arguments.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => {
                return ToolResult::error(
                    request.id,
                    "Missing required argument: query",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let item_type = request
            .arguments
            .get("item_type")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        let limit = request
            .arguments
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        // Perform search
        let results = match item_type {
            "method" => self.indexer.search_methods(query.to_string(), limit).await,
            "property" => {
                self.indexer
                    .search_properties(query.to_string(), limit)
                    .await
            }
            "all" => self.indexer.search_all(query.to_string(), limit).await,
            _ => {
                return ToolResult::error(
                    request.id,
                    format!(
                        "Invalid item_type: {}. Must be 'method', 'property', 'signal', or 'all'",
                        item_type
                    ),
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        match results {
            Ok(search_results) => {
                let response = json!({
                    "query": query,
                    "item_type": item_type,
                    "count": search_results.len(),
                    "results": search_results.iter().map(|r| {
                        json!({
                            "service": r.service,
                            "object_path": r.object_path,
                            "interface": r.interface,
                            "type": r.item_type,
                            "name": r.item_name,
                            "description": r.description,
                            "relevance": r.relevance_score,
                            "full_name": format!("{}.{}.{}", r.service, r.interface, r.item_name)
                        })
                    }).collect::<Vec<_>>()
                });

                ToolResult::success(request.id, response, start.elapsed().as_millis() as u64)
            }
            Err(e) => ToolResult::error(
                request.id,
                format!("Search failed: {}", e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }

    fn name(&self) -> &str {
        "search_dbus"
    }
}

/// Tool for rebuilding the DBus index
pub struct DbusRebuildIndexTool {
    indexer: Arc<IndexerManager>,
}

impl DbusRebuildIndexTool {
    pub fn new(indexer: Arc<IndexerManager>) -> Self {
        Self { indexer }
    }
}

#[async_trait]
impl Tool for DbusRebuildIndexTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "rebuild_dbus_index".to_string(),
            description:
                "Rebuild the DBus FTS search index. Use this when DBus services have changed \
                         or to ensure the index is up-to-date."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "bus_type": {
                        "type": "string",
                        "enum": ["system", "session"],
                        "description": "Which DBus bus to index (default: system)",
                        "default": "system"
                    }
                },
                "required": []
            }),
            category: Some("dbus".to_string()),
            tags: vec!["admin".to_string(), "index".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        let bus_type_str = request
            .arguments
            .get("bus_type")
            .and_then(|v| v.as_str())
            .unwrap_or("system");

        let bus_type = match bus_type_str {
            "system" => BusType::System,
            "session" => BusType::Session,
            _ => {
                return ToolResult::error(
                    request.id,
                    "Invalid bus_type. Must be 'system' or 'session'",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        // Clear existing index
        if let Err(e) = self.indexer.clear_index().await {
            return ToolResult::error(
                request.id,
                format!("Failed to clear index: {}", e),
                start.elapsed().as_millis() as u64,
            );
        }

        // Rebuild index
        match self.indexer.build_index(bus_type).await {
            Ok(stats) => {
                let response = json!({
                    "bus_type": bus_type_str,
                    "statistics": {
                        "services": stats.total_services,
                        "objects": stats.total_objects,
                        "interfaces": stats.total_interfaces,
                        "methods": stats.total_methods,
                        "properties": stats.total_properties,
                        "signals": stats.total_signals,
                        "scan_duration_seconds": stats.scan_duration_seconds,
                        "indexed_at": stats.indexed_at
                    },
                    "message": format!(
                        "Index rebuilt: {} methods, {} properties in {:.2}s",
                        stats.total_methods,
                        stats.total_properties,
                        stats.scan_duration_seconds
                    )
                });

                ToolResult::success(request.id, response, start.elapsed().as_millis() as u64)
            }
            Err(e) => ToolResult::error(
                request.id,
                format!("Failed to build index: {}", e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }

    fn name(&self) -> &str {
        "rebuild_dbus_index"
    }
}

/// Tool for getting DBus index statistics
pub struct DbusIndexStatsTool {
    indexer: Arc<IndexerManager>,
}

impl DbusIndexStatsTool {
    pub fn new(indexer: Arc<IndexerManager>) -> Self {
        Self { indexer }
    }
}

#[async_trait]
impl Tool for DbusIndexStatsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dbus_index_stats".to_string(),
            description:
                "Get statistics about the DBus search index including number of services, \
                         methods, properties indexed and when it was last updated."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            category: Some("dbus".to_string()),
            tags: vec!["info".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        match self.indexer.get_statistics().await {
            Ok(Some(stats)) => {
                let response = json!({
                    "services": stats.total_services,
                    "objects": stats.total_objects,
                    "interfaces": stats.total_interfaces,
                    "methods": stats.total_methods,
                    "properties": stats.total_properties,
                    "signals": stats.total_signals,
                    "scan_duration_seconds": stats.scan_duration_seconds,
                    "indexed_at": stats.indexed_at,
                    "age_seconds": chrono::Utc::now().timestamp() - stats.indexed_at
                });

                ToolResult::success(request.id, response, start.elapsed().as_millis() as u64)
            }
            Ok(None) => ToolResult::error(
                request.id,
                "No index statistics available. Index may not be built yet.",
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => ToolResult::error(
                request.id,
                format!("Failed to get statistics: {}", e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }

    fn name(&self) -> &str {
        "dbus_index_stats"
    }
}
