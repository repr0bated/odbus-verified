//! Stdio Transport
//!
//! Standard MCP transport over stdin/stdout.

use super::{McpHandler, Transport};
use crate::{JsonRpcError, McpRequest, McpResponse};
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

/// Stdio transport - reads JSON-RPC from stdin, writes to stdout
pub struct StdioTransport;

impl StdioTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Transport for StdioTransport {
    async fn serve<H: McpHandler + 'static>(self, handler: Arc<H>) -> Result<()> {
        info!("Starting MCP stdio transport");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        while let Some(line) = reader.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            debug!(request = %line, "Received request");

            let mut line_mut = line.to_string();
            let response = match unsafe { simd_json::from_str::<McpRequest>(&mut line_mut) } {
                Ok(request) => handler.handle_request(request).await,
                Err(e) => {
                    error!(error = %e, "Parse error");
                    McpResponse::error(None, JsonRpcError::parse_error(e.to_string()))
                }
            };

            let response_json = simd_json::to_string(&response)?;
            debug!(response = %response_json, "Sending response");

            stdout.write_all(response_json.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }

        info!("Stdio transport shutting down");
        Ok(())
    }
}
