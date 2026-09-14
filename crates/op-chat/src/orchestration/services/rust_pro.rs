//! RustProService gRPC implementation
//!
//! Provides cargo operations (check, fmt, build, test, clippy, run, doc, bench)
//! with real process spawning and streaming output.

use std::pin::Pin;
use std::time::Instant;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info};

use crate::orchestration::proto::op_chat_orchestration::{
    rust_pro_service_server::RustProService, AnalyzeRequest, AnalyzeResponse, CargoMetadata,
    CargoOutputLine, CargoRequest, CargoResponse, OutputStreamType, VersionRequest,
    VersionResponse,
};

use super::OrchestrationServer;

/// Build a tokio Command from a CargoRequest and a cargo subcommand.
fn build_cargo_command(subcommand: &str, req: &CargoRequest) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg(subcommand);

    // Working directory
    let path = if req.path.is_empty() { "." } else { &req.path };
    cmd.current_dir(path);

    // Release flag
    if req.release {
        cmd.arg("--release");
    }

    // Package
    if !req.package.is_empty() {
        cmd.arg("-p").arg(&req.package);
    }

    // Features
    if !req.features.is_empty() {
        cmd.arg("--features").arg(req.features.join(","));
    }

    // All targets
    if req.all_targets {
        cmd.arg("--all-targets");
    }

    // Subcommand-specific flags
    match subcommand {
        "test" if !req.filter.is_empty() => {
            cmd.arg("--").arg(&req.filter);
        }
        "clippy" if req.fix => {
            cmd.arg("--fix").arg("--allow-dirty").arg("--allow-staged");
        }
        "fmt" => {
            if req.fix {
                // fmt is fix by default, but we can add --check to not fix
            } else {
                cmd.arg("--check");
            }
        }
        _ => {}
    }

    // Environment variables
    for (key, value) in &req.env {
        cmd.env(key, value);
    }

    cmd
}

/// Count warnings and errors from cargo output lines
fn count_diagnostics(output: &str) -> (i32, i32) {
    let mut warnings = 0i32;
    let mut errors = 0i32;
    for line in output.lines() {
        if line.contains("warning:") || line.contains("warning[") {
            warnings += 1;
        }
        if line.contains("error:") || line.contains("error[") {
            errors += 1;
        }
    }
    (warnings, errors)
}

/// Run a cargo command unary (non-streaming) and return CargoResponse
async fn run_cargo_unary(subcommand: &str, req: &CargoRequest) -> Result<CargoResponse, Status> {
    let start = Instant::now();

    let mut cmd = build_cargo_command(subcommand, req);

    let timeout_duration = if req.timeout_secs > 0 {
        std::time::Duration::from_secs(req.timeout_secs as u64)
    } else {
        std::time::Duration::from_secs(300)
    };

    let output = match tokio::time::timeout(timeout_duration, cmd.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Ok(CargoResponse {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to execute cargo {}: {}", subcommand, e),
                exit_code: -1,
                duration_ms: start.elapsed().as_millis() as i64,
                metadata: None,
            });
        }
        Err(_) => {
            return Ok(CargoResponse {
                success: false,
                stdout: String::new(),
                stderr: format!(
                    "cargo {} timed out after {} seconds",
                    subcommand,
                    timeout_duration.as_secs()
                ),
                exit_code: -1,
                duration_ms: start.elapsed().as_millis() as i64,
                metadata: None,
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    let duration_ms = start.elapsed().as_millis() as i64;

    let (warnings, errors) = count_diagnostics(&stderr);

    Ok(CargoResponse {
        success: exit_code == 0,
        stdout,
        stderr,
        exit_code,
        duration_ms,
        metadata: Some(CargoMetadata {
            warnings,
            errors,
            failed_targets: vec![],
        }),
    })
}

/// Run a cargo command with streaming output, sending lines to a channel
async fn run_cargo_streaming(
    subcommand: &str,
    req: CargoRequest,
    tx: tokio::sync::mpsc::Sender<Result<CargoOutputLine, Status>>,
) {
    let start = Instant::now();

    let mut cmd = build_cargo_command(subcommand, &req);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tx
                .send(Ok(CargoOutputLine {
                    line: format!("Failed to spawn cargo {}: {}", subcommand, e),
                    stream: OutputStreamType::OutputStreamStderr as i32,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    is_final: true,
                    exit_code: -1,
                    duration_ms: start.elapsed().as_millis() as i64,
                    metadata: None,
                }))
                .await;
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tx_stdout = tx.clone();
    let tx_stderr = tx.clone();

    // Spawn stdout reader
    let stdout_handle = tokio::spawn(async move {
        let mut warnings = 0i32;
        let mut errors = 0i32;
        if let Some(stdout) = stdout {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Cargo JSON messages can appear on stdout too
                if line.contains("warning:") || line.contains("warning[") {
                    warnings += 1;
                }
                if line.contains("error:") || line.contains("error[") {
                    errors += 1;
                }
                let msg = CargoOutputLine {
                    line: line.clone(),
                    stream: OutputStreamType::OutputStreamStdout as i32,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    is_final: false,
                    exit_code: 0,
                    duration_ms: 0,
                    metadata: None,
                };
                if tx_stdout.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        }
        (warnings, errors)
    });

    // Spawn stderr reader
    let stderr_handle = tokio::spawn(async move {
        let mut warnings = 0i32;
        let mut errors = 0i32;
        if let Some(stderr) = stderr {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("warning:") || line.contains("warning[") {
                    warnings += 1;
                }
                if line.contains("error:") || line.contains("error[") {
                    errors += 1;
                }
                let msg = CargoOutputLine {
                    line: line.clone(),
                    stream: OutputStreamType::OutputStreamStderr as i32,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    is_final: false,
                    exit_code: 0,
                    duration_ms: 0,
                    metadata: None,
                };
                if tx_stderr.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        }
        (warnings, errors)
    });

    // Apply timeout if specified
    let timeout_duration = if req.timeout_secs > 0 {
        std::time::Duration::from_secs(req.timeout_secs as u64)
    } else {
        std::time::Duration::from_secs(600)
    };

    let wait_result = tokio::time::timeout(timeout_duration, child.wait()).await;

    // Wait for readers to finish
    let (stdout_diag, stderr_diag) = tokio::join!(stdout_handle, stderr_handle);
    let (sw, se) = stdout_diag.unwrap_or((0, 0));
    let (ew, ee) = stderr_diag.unwrap_or((0, 0));
    let total_warnings = sw + ew;
    let total_errors = se + ee;

    let (exit_code, timed_out) = match wait_result {
        Ok(Ok(status)) => (status.code().unwrap_or(-1), false),
        Ok(Err(e)) => {
            error!(error = %e, "Error waiting for cargo process");
            (-1, false)
        }
        Err(_) => {
            // Timeout: attempt to kill the child
            let _ = child.kill().await;
            (-1, true)
        }
    };

    let duration_ms = start.elapsed().as_millis() as i64;

    // Send final message
    let final_line = if timed_out {
        format!(
            "cargo {} timed out after {} seconds",
            subcommand,
            timeout_duration.as_secs()
        )
    } else {
        format!("cargo {} exited with code {}", subcommand, exit_code)
    };

    let _ = tx
        .send(Ok(CargoOutputLine {
            line: final_line,
            stream: OutputStreamType::OutputStreamStdout as i32,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            is_final: true,
            exit_code,
            duration_ms,
            metadata: Some(CargoMetadata {
                warnings: total_warnings,
                errors: total_errors,
                failed_targets: vec![],
            }),
        }))
        .await;
}

type CargoStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<CargoOutputLine, Status>> + Send + 'static>>;

fn spawn_streaming_cargo(subcommand: &'static str, req: CargoRequest) -> CargoStream {
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(async move {
        run_cargo_streaming(subcommand, req, tx).await;
    });
    Box::pin(ReceiverStream::new(rx))
}

#[tonic::async_trait]
impl RustProService for OrchestrationServer {
    type BuildStream = CargoStream;
    type TestStream = CargoStream;
    type ClippyStream = CargoStream;
    type RunStream = CargoStream;
    type DocStream = CargoStream;
    type BenchStream = CargoStream;

    async fn check(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<CargoResponse>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, "Running cargo check");
        let resp = run_cargo_unary("check", &req).await?;
        Ok(Response::new(resp))
    }

    async fn fmt(&self, request: Request<CargoRequest>) -> Result<Response<CargoResponse>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, "Running cargo fmt");
        let resp = run_cargo_unary("fmt", &req).await?;
        Ok(Response::new(resp))
    }

    async fn version(
        &self,
        _request: Request<VersionRequest>,
    ) -> Result<Response<VersionResponse>, Status> {
        debug!("Getting Rust toolchain versions");

        let rustc = Command::new("rustc")
            .arg("--version")
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("unavailable: {}", e));

        let cargo = Command::new("cargo")
            .arg("--version")
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("unavailable: {}", e));

        let clippy = Command::new("cargo")
            .args(["clippy", "--version"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("unavailable: {}", e));

        let rustfmt = Command::new("rustfmt")
            .arg("--version")
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| format!("unavailable: {}", e));

        Ok(Response::new(VersionResponse {
            rustc_version: rustc,
            cargo_version: cargo,
            clippy_version: clippy,
            rustfmt_version: rustfmt,
        }))
    }

    async fn build(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::BuildStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, release = req.release, "Streaming cargo build");
        Ok(Response::new(spawn_streaming_cargo("build", req)))
    }

    async fn test(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::TestStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, filter = %req.filter, "Streaming cargo test");
        Ok(Response::new(spawn_streaming_cargo("test", req)))
    }

    async fn clippy(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::ClippyStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, fix = req.fix, "Streaming cargo clippy");
        Ok(Response::new(spawn_streaming_cargo("clippy", req)))
    }

    async fn run(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::RunStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, "Streaming cargo run");
        Ok(Response::new(spawn_streaming_cargo("run", req)))
    }

    async fn doc(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::DocStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, "Streaming cargo doc");
        Ok(Response::new(spawn_streaming_cargo("doc", req)))
    }

    async fn bench(
        &self,
        request: Request<CargoRequest>,
    ) -> Result<Response<Self::BenchStream>, Status> {
        let req = request.into_inner();
        info!(path = %req.path, "Streaming cargo bench");
        Ok(Response::new(spawn_streaming_cargo("bench", req)))
    }

    async fn analyze(
        &self,
        request: Request<AnalyzeRequest>,
    ) -> Result<Response<AnalyzeResponse>, Status> {
        let req = request.into_inner();
        let path = if req.path.is_empty() { "." } else { &req.path };
        info!(path = %path, "Running cargo metadata analysis");

        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1"])
            .current_dir(path)
            .output()
            .await
            .map_err(|e| Status::internal(format!("Failed to run cargo metadata: {}", e)))?;

        if output.status.success() {
            let result_json = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(Response::new(AnalyzeResponse {
                success: true,
                result_json,
            }))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Ok(Response::new(AnalyzeResponse {
                success: false,
                result_json: format!("{{\"error\": \"{}\"}}", stderr.replace('"', "\\\"")),
            }))
        }
    }
}
