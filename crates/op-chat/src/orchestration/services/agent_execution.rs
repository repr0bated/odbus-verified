//! AgentExecution gRPC service implementation.

use std::pin::Pin;
use std::time::Instant;

use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::orchestration::grpc_pool::StreamChunk;
use crate::orchestration::proto::op_chat_orchestration::{
    agent_execution_server::AgentExecution, BatchExecuteRequest, CancelRequest, CancelResponse,
    ChunkType, ExecuteChunk, ExecuteError, ExecuteRequest, ExecuteResponse,
};

use super::OrchestrationServer;

#[tonic::async_trait]
impl AgentExecution for OrchestrationServer {
    /// Execute a single operation on an agent.
    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<ExecuteResponse>, Status> {
        let req = request.into_inner();
        let correlation_id = if req.correlation_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            req.correlation_id.clone()
        };

        info!(
            session = %req.session_id,
            agent = %req.agent_id,
            op = %req.operation,
            correlation = %correlation_id,
            "Execute"
        );

        // Register a cancellation token for this correlation_id
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
        self.cancellation_tokens
            .write()
            .await
            .insert(correlation_id.clone(), cancel_tx);

        let start = Instant::now();

        // Parse arguments
        let args = if req.arguments_json.is_empty() {
            simd_json::json!({})
        } else {
            let mut json_bytes = req.arguments_json.into_bytes();
            match simd_json::from_slice::<simd_json::OwnedValue>(&mut json_bytes) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(Response::new(ExecuteResponse {
                        correlation_id,
                        agent_id: req.agent_id,
                        operation: req.operation,
                        success: false,
                        result_json: String::new(),
                        error: Some(ExecuteError {
                            code: "INVALID_ARGUMENTS".to_string(),
                            message: format!("Failed to parse arguments: {}", e),
                            details: String::new(),
                            retryable: false,
                            stack_trace: String::new(),
                        }),
                        execution_time_ms: 0,
                        metadata: Default::default(),
                    }));
                }
            }
        };

        match self
            .pool
            .execute(&req.session_id, &req.agent_id, &req.operation, args)
            .await
        {
            Ok(result) => {
                let elapsed = start.elapsed().as_millis() as i64;

                // Clean up cancellation token
                self.cancellation_tokens
                    .write()
                    .await
                    .remove(&correlation_id);

                let result_json =
                    simd_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());

                Ok(Response::new(ExecuteResponse {
                    correlation_id,
                    agent_id: req.agent_id,
                    operation: req.operation,
                    success: true,
                    result_json,
                    error: None,
                    execution_time_ms: elapsed,
                    metadata: Default::default(),
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_millis() as i64;
                self.cancellation_tokens
                    .write()
                    .await
                    .remove(&correlation_id);

                Ok(Response::new(ExecuteResponse {
                    correlation_id,
                    agent_id: req.agent_id,
                    operation: req.operation,
                    success: false,
                    result_json: String::new(),
                    error: Some(ExecuteError {
                        code: format!("{:?}", e.code),
                        message: e.to_string(),
                        details: String::new(),
                        retryable: e.is_retryable(),
                        stack_trace: String::new(),
                    }),
                    execution_time_ms: elapsed,
                    metadata: Default::default(),
                }))
            }
        }
    }

    // -- streaming -----------------------------------------------------------

    type ExecuteStreamStream =
        Pin<Box<dyn Stream<Item = Result<ExecuteChunk, Status>> + Send + 'static>>;

    /// Execute with server-streaming response.
    async fn execute_stream(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStreamStream>, Status> {
        let req = request.into_inner();
        let correlation_id = if req.correlation_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            req.correlation_id.clone()
        };

        info!(
            session = %req.session_id,
            agent = %req.agent_id,
            op = %req.operation,
            "ExecuteStream"
        );

        let args = if req.arguments_json.is_empty() {
            simd_json::json!({})
        } else {
            let mut json_bytes = req.arguments_json.into_bytes();
            simd_json::from_slice::<simd_json::OwnedValue>(&mut json_bytes)
                .map_err(|e| Status::invalid_argument(format!("Bad JSON arguments: {}", e)))?
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ExecuteChunk, Status>>(64);

        let pool = self.pool.clone();
        let session_id = req.session_id.clone();
        let agent_id = req.agent_id.clone();
        let operation = req.operation.clone();
        let corr = correlation_id.clone();

        tokio::spawn(async move {
            let chunk_tx = tx.clone();
            let corr_clone = corr.clone();

            let on_chunk = move |chunk: StreamChunk| {
                let proto_chunk = ExecuteChunk {
                    correlation_id: corr_clone.clone(),
                    chunk_type: match chunk.stream_type {
                        crate::orchestration::grpc_pool::StreamType::Stdout => {
                            ChunkType::Stdout.into()
                        }
                        crate::orchestration::grpc_pool::StreamType::Stderr => {
                            ChunkType::Stderr.into()
                        }
                        crate::orchestration::grpc_pool::StreamType::Progress => {
                            ChunkType::Progress.into()
                        }
                        crate::orchestration::grpc_pool::StreamType::Result => {
                            ChunkType::Result.into()
                        }
                        crate::orchestration::grpc_pool::StreamType::Heartbeat => {
                            ChunkType::Heartbeat.into()
                        }
                    },
                    content: chunk.content,
                    is_final: chunk.is_final,
                    sequence: chunk.sequence as i32,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    error: None,
                };
                let _ = chunk_tx.try_send(Ok(proto_chunk));
            };

            match pool
                .execute_streaming(&session_id, &agent_id, &operation, args, on_chunk)
                .await
            {
                Ok(_result) => {
                    // Final chunk already sent by the on_chunk callback with is_final=true
                }
                Err(e) => {
                    let err_chunk = ExecuteChunk {
                        correlation_id: corr,
                        chunk_type: ChunkType::Result.into(),
                        content: String::new(),
                        is_final: true,
                        sequence: -1,
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        error: Some(ExecuteError {
                            code: format!("{:?}", e.code),
                            message: e.to_string(),
                            details: String::new(),
                            retryable: e.is_retryable(),
                            stack_trace: String::new(),
                        }),
                    };
                    let _ = tx.send(Ok(err_chunk)).await;
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::ExecuteStreamStream))
    }

    // -- batch ---------------------------------------------------------------

    type BatchExecuteStream =
        Pin<Box<dyn Stream<Item = Result<ExecuteResponse, Status>> + Send + 'static>>;

    /// Batch-execute multiple operations, yielding results as a server stream.
    async fn batch_execute(
        &self,
        request: Request<BatchExecuteRequest>,
    ) -> Result<Response<Self::BatchExecuteStream>, Status> {
        let req = request.into_inner();
        let parallel = req.parallel;
        let stop_on_error = req.stop_on_error;
        let requests = req.requests;

        info!(count = requests.len(), parallel, "BatchExecute");

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ExecuteResponse, Status>>(64);
        let pool = self.pool.clone();

        tokio::spawn(async move {
            if parallel {
                // Spawn all tasks in parallel
                let mut handles = Vec::new();
                for exec_req in requests {
                    let pool = pool.clone();
                    let handle = tokio::spawn(async move {
                        let start = Instant::now();
                        let correlation_id = if exec_req.correlation_id.is_empty() {
                            uuid::Uuid::new_v4().to_string()
                        } else {
                            exec_req.correlation_id.clone()
                        };

                        let args = if exec_req.arguments_json.is_empty() {
                            simd_json::json!({})
                        } else {
                            let mut json_bytes = exec_req.arguments_json.into_bytes();
                            simd_json::from_slice::<simd_json::OwnedValue>(&mut json_bytes)
                                .unwrap_or(simd_json::json!({}))
                        };

                        match pool
                            .execute(
                                &exec_req.session_id,
                                &exec_req.agent_id,
                                &exec_req.operation,
                                args,
                            )
                            .await
                        {
                            Ok(result) => ExecuteResponse {
                                correlation_id,
                                agent_id: exec_req.agent_id,
                                operation: exec_req.operation,
                                success: true,
                                result_json: simd_json::to_string(&result)
                                    .unwrap_or_else(|_| "{}".to_string()),
                                error: None,
                                execution_time_ms: start.elapsed().as_millis() as i64,
                                metadata: Default::default(),
                            },
                            Err(e) => ExecuteResponse {
                                correlation_id,
                                agent_id: exec_req.agent_id,
                                operation: exec_req.operation,
                                success: false,
                                result_json: String::new(),
                                error: Some(ExecuteError {
                                    code: format!("{:?}", e.code),
                                    message: e.to_string(),
                                    details: String::new(),
                                    retryable: e.is_retryable(),
                                    stack_trace: String::new(),
                                }),
                                execution_time_ms: start.elapsed().as_millis() as i64,
                                metadata: Default::default(),
                            },
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    if let Ok(resp) = handle.await {
                        let is_err = !resp.success;
                        if tx.send(Ok(resp)).await.is_err() {
                            break;
                        }
                        if stop_on_error && is_err {
                            break;
                        }
                    }
                }
            } else {
                // Sequential execution
                for exec_req in requests {
                    let start = Instant::now();
                    let correlation_id = if exec_req.correlation_id.is_empty() {
                        uuid::Uuid::new_v4().to_string()
                    } else {
                        exec_req.correlation_id.clone()
                    };

                    let args = if exec_req.arguments_json.is_empty() {
                        simd_json::json!({})
                    } else {
                        let mut json_bytes = exec_req.arguments_json.into_bytes();
                        simd_json::from_slice::<simd_json::OwnedValue>(&mut json_bytes)
                            .unwrap_or(simd_json::json!({}))
                    };

                    let resp = match pool
                        .execute(
                            &exec_req.session_id,
                            &exec_req.agent_id,
                            &exec_req.operation,
                            args,
                        )
                        .await
                    {
                        Ok(result) => ExecuteResponse {
                            correlation_id,
                            agent_id: exec_req.agent_id,
                            operation: exec_req.operation,
                            success: true,
                            result_json: simd_json::to_string(&result)
                                .unwrap_or_else(|_| "{}".to_string()),
                            error: None,
                            execution_time_ms: start.elapsed().as_millis() as i64,
                            metadata: Default::default(),
                        },
                        Err(e) => {
                            let resp = ExecuteResponse {
                                correlation_id,
                                agent_id: exec_req.agent_id,
                                operation: exec_req.operation,
                                success: false,
                                result_json: String::new(),
                                error: Some(ExecuteError {
                                    code: format!("{:?}", e.code),
                                    message: e.to_string(),
                                    details: String::new(),
                                    retryable: e.is_retryable(),
                                    stack_trace: String::new(),
                                }),
                                execution_time_ms: start.elapsed().as_millis() as i64,
                                metadata: Default::default(),
                            };
                            resp
                        }
                    };

                    let is_err = !resp.success;
                    if tx.send(Ok(resp)).await.is_err() {
                        break;
                    }
                    if stop_on_error && is_err {
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::BatchExecuteStream))
    }

    /// Cancel a running operation by correlation ID.
    async fn cancel(
        &self,
        request: Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        let req = request.into_inner();
        info!(correlation = %req.correlation_id, "Cancel");

        let mut tokens = self.cancellation_tokens.write().await;
        if let Some(tx) = tokens.remove(&req.correlation_id) {
            // Signal cancellation
            let _ = tx.send(true);
            Ok(Response::new(CancelResponse {
                success: true,
                correlation_id: req.correlation_id,
                was_running: true,
            }))
        } else {
            Ok(Response::new(CancelResponse {
                success: false,
                correlation_id: req.correlation_id,
                was_running: false,
            }))
        }
    }
}
