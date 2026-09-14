//! Log Streaming Handlers

use axum::{
    extract::Extension,
    response::{
        sse::{Event, KeepAlive, Sse},
        Json,
    },
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::error;

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub component: String,
    pub message: String,
}

/// GET /api/logs - Get recent logs
pub async fn logs_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<Vec<LogEntry>> {
    let mut logs = Vec::new();

    // Log sources to read
    let log_files = vec![
        ("/var/log/op-web.log", "op-web"),
        ("/var/log/op-dbus.log", "op-dbus"),
        ("/tmp/op-web.log", "op-web-tmp"),
    ];

    for (log_path, component) in log_files {
        if let Ok(data) = tokio::fs::read_to_string(log_path).await {
            let lines: Vec<&str> = data.lines().collect();
            let start = lines.len().saturating_sub(50);
            let tail = lines[start..].join("\n");
            logs.extend(parse_logs(&tail, component));
        }
    }

    // Sort by timestamp (most recent first)
    logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    logs.truncate(100);

    Json(logs)
}

fn parse_logs(data: &str, component: &str) -> Vec<LogEntry> {
    data.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if line.is_empty() {
                return None;
            }

            // Simple parsing - adjust based on actual log format
            let level = if line.contains("ERROR") {
                "error"
            } else if line.contains("WARN") {
                "warn"
            } else if line.contains("INFO") {
                "info"
            } else {
                "debug"
            };

            Some(LogEntry {
                id: format!("{}-{}", component, i),
                timestamp: chrono::Utc::now().to_rfc3339(),
                level: level.to_string(),
                component: component.to_string(),
                message: line.to_string(),
            })
        })
        .collect()
}

#[allow(dead_code)]
fn parse_journalctl_logs(data: &str, component: &str) -> Vec<LogEntry> {
    data.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if line.is_empty() {
                return None;
            }

            // Parse journalctl format: "Mar 02 08:00:00 hostname service[pid]: message"
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            let message = if parts.len() >= 4 {
                parts[3..].join(" ")
            } else {
                line.to_string()
            };

            let level = if message.contains("error") || message.contains("ERROR") {
                "error"
            } else if message.contains("warn") || message.contains("WARN") {
                "warn"
            } else {
                "info"
            };

            Some(LogEntry {
                id: format!("{}-{}", component, i),
                timestamp: chrono::Utc::now().to_rfc3339(),
                level: level.to_string(),
                component: component.to_string(),
                message,
            })
        })
        .collect()
}

/// GET /api/logs/stream - SSE live log stream using linemux for efficient file watching
pub async fn logs_stream_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_broadcaster.subscribe();

    // Convert the broadcast channel into a stream of SSE events
    let broadcaster_stream = BroadcastStream::new(rx).filter_map(|result| {
        result
            .ok()
            .map(|ev| Ok(Event::default().event(ev.event_type).data(ev.data)))
    });

    // Background log watcher: use linemux to efficiently tail log files with inotify
    let broadcaster = state.sse_broadcaster.clone();
    tokio::spawn(async move {
        use linemux::MuxedLines;

        let mut lines = MuxedLines::new().expect("Failed to create MuxedLines");

        // Add log files to watch
        let log_files = vec![
            ("/var/log/op-web.log", "op-web"),
            ("/var/log/op-dbus.log", "op-dbus"),
            ("/tmp/op-web.log", "op-web-tmp"),
        ];

        for (path, _) in &log_files {
            if std::path::Path::new(path).exists() {
                if let Err(e) = lines.add_file(path).await {
                    error!("Failed to watch {}: {}", path, e);
                }
            }
        }

        // Stream new lines as they appear
        while let Ok(Some(line)) = lines.next_line().await {
            // Determine which service based on the source file
            let service = line
                .source()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|name| {
                    if name.contains("op-web") {
                        if name.contains("tmp") {
                            "op-web-tmp"
                        } else {
                            "op-web"
                        }
                    } else if name.contains("op-dbus") {
                        "op-dbus"
                    } else {
                        "unknown"
                    }
                })
                .unwrap_or("unknown");

            let text = line.line();

            // Parse log level from line content
            let level = if text.contains("ERROR") || text.contains("error") {
                "ERROR"
            } else if text.contains("WARN") || text.contains("warn") {
                "WARN"
            } else if text.contains("DEBUG") || text.contains("debug") {
                "DEBUG"
            } else {
                "INFO"
            };

            let payload = match simd_json::to_string(&simd_json::json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "level": level,
                "service": service,
                "message": text,
            })) {
                Ok(s) => s,
                Err(_) => continue,
            };

            broadcaster.broadcast("log", &payload);
        }
    });

    Sse::new(broadcaster_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
