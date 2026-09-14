// Agent-to-agent gRPC speed test.
//
// Measures round-trip latency and throughput of live gRPC endpoints on the
// internal svc0 (10.200.0.2) and WireGuard (100.69.0.254) networks.
//
// Run: cargo test --test grpc_speed_test -p op-grpc-bridge -- --nocapture

use std::time::{Duration, Instant};

use tonic::transport::Channel;

/// Install rustls CryptoProvider (required by rustls 0.23+).
fn install_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Connect to an endpoint, returning None if unreachable.
async fn try_connect(uri: &str) -> Option<Channel> {
    tokio::time::timeout(
        Duration::from_secs(3),
        Channel::from_shared(uri.to_string())
            .unwrap()
            .connect_timeout(Duration::from_secs(2))
            .connect(),
    )
    .await
    .ok()?
    .ok()
}

/// Measure N sequential unary RPCs and report p50/p99/throughput.
async fn bench_get_state(channel: Channel, label: &str, n: usize) {
    use op_grpc_bridge::proto::{state_sync_client::StateSyncClient, GetStateRequest};

    let mut client = StateSyncClient::new(channel);
    let mut latencies = Vec::with_capacity(n);

    // Warmup
    for _ in 0..3 {
        let _ = client
            .get_state(tonic::Request::new(GetStateRequest {
                plugin_id: "tched_router".to_string(),
                object_path: String::new(),
            }))
            .await;
    }

    let wall_start = Instant::now();
    for _ in 0..n {
        let start = Instant::now();
        let _ = client
            .get_state(tonic::Request::new(GetStateRequest {
                plugin_id: "tched_router".to_string(),
                object_path: String::new(),
            }))
            .await;
        latencies.push(start.elapsed());
    }
    let wall = wall_start.elapsed();

    latencies.sort();
    let p50 = latencies[n / 2];
    let p99 = latencies[(n * 99) / 100];
    let avg = latencies.iter().sum::<Duration>() / n as u32;
    let rps = n as f64 / wall.as_secs_f64();

    println!("  [{label}] GetState x{n}:");
    println!("    avg={avg:?}  p50={p50:?}  p99={p99:?}");
    println!("    wall={wall:?}  throughput={rps:.0} rpc/s");
}

/// Measure health check latency.
async fn bench_health(channel: Channel, label: &str, n: usize) {
    let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
    let mut latencies = Vec::with_capacity(n);

    // Warmup
    for _ in 0..3 {
        let _ = client
            .check(tonic_health::pb::HealthCheckRequest {
                service: String::new(),
            })
            .await;
    }

    let wall_start = Instant::now();
    for _ in 0..n {
        let start = Instant::now();
        let _ = client
            .check(tonic_health::pb::HealthCheckRequest {
                service: String::new(),
            })
            .await;
        latencies.push(start.elapsed());
    }
    let wall = wall_start.elapsed();

    latencies.sort();
    let p50 = latencies[n / 2];
    let p99 = latencies[(n * 99) / 100];
    let avg = latencies.iter().sum::<Duration>() / n as u32;
    let rps = n as f64 / wall.as_secs_f64();

    println!("  [{label}] Health x{n}:");
    println!("    avg={avg:?}  p50={p50:?}  p99={p99:?}");
    println!("    wall={wall:?}  throughput={rps:.0} rpc/s");
}

/// Measure reflection ListServices latency.
async fn bench_reflection(channel: Channel, label: &str, n: usize) {
    use tonic_reflection::pb::v1::{
        server_reflection_client::ServerReflectionClient,
        server_reflection_request::MessageRequest, ServerReflectionRequest,
    };

    let mut latencies = Vec::with_capacity(n);

    let wall_start = Instant::now();
    for _ in 0..n {
        let start = Instant::now();

        let mut client = ServerReflectionClient::new(channel.clone());
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.send(ServerReflectionRequest {
            host: String::new(),
            message_request: Some(MessageRequest::ListServices(String::new())),
        })
        .await
        .unwrap();
        drop(tx);

        let resp = client
            .server_reflection_info(tokio_stream::wrappers::ReceiverStream::new(rx))
            .await;
        if let Ok(resp) = resp {
            let mut stream = resp.into_inner();
            let _ = stream.message().await;
        }
        latencies.push(start.elapsed());
    }
    let wall = wall_start.elapsed();

    latencies.sort();
    let p50 = latencies[n / 2];
    let p99 = latencies[(n * 99) / 100];
    let avg = latencies.iter().sum::<Duration>() / n as u32;
    let rps = n as f64 / wall.as_secs_f64();

    println!("  [{label}] Reflection ListServices x{n}:");
    println!("    avg={avg:?}  p50={p50:?}  p99={p99:?}");
    println!("    wall={wall:?}  throughput={rps:.0} rpc/s");
}

#[tokio::test]
async fn grpc_agent_to_agent_speed() {
    install_crypto_provider();

    let endpoints = [
        ("loopback:50051", "http://127.0.0.1:50051"),
        ("loopback:8090", "http://127.0.0.1:8090"),
        ("svc0:50051", "http://10.200.0.2:50051"),
        ("svc0:8090", "http://10.200.0.2:8090"),
        ("wg:8090", "http://100.69.0.254:8090"),
        ("wg:50051", "http://100.69.0.254:50051"),
    ];

    println!("\n══════════════════════════════════════════════════");
    println!("  Agent-to-Agent gRPC Speed Test");
    println!("══════════════════════════════════════════════════\n");

    for (label, uri) in &endpoints {
        print!("Connecting {label} ({uri})... ");
        match try_connect(uri).await {
            Some(channel) => {
                println!("✓");
                bench_health(channel.clone(), label, 100).await;
                bench_get_state(channel.clone(), label, 100).await;
                bench_reflection(channel, label, 20).await;
                println!();
            }
            None => {
                println!("✗ (unreachable)");
            }
        }
    }

    println!("══════════════════════════════════════════════════");
}
