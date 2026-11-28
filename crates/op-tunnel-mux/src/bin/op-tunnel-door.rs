//! `op-tunnel-door [--socket PATH | --lungs-socket PATH] [--door ADDR] [--waiters N]`
//!
//! `--lungs-socket` is the browser path: the host pushes each browser TCP
//! socket over the identity's lungs connection and this door splices it raw
//! to `127.0.0.1:8443`. `--waiters` applies only to the legacy `--socket`
//! framed path, kept for unrelated control-plane tunnel traffic.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use op_tunnel_mux::{frame, Frame, TunnelClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

const CHUNK: usize = 16 * 1024;

async fn channel(socket: String) -> Result<Channel> {
    Endpoint::try_from("http://[::]:0")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let socket = socket.clone();
            async move { tokio::net::UnixStream::connect(socket).await.map(TokioIo::new) }
        }))
        .await
        .context("connect to the door socket")
}

/// One waiter: hold an Accept; when a connection arrives, splice it and
/// immediately hold the next Accept.
async fn waiter(channel: Channel, door: String) -> Result<()> {
    let mut api = TunnelClient::new(channel);
    loop {
        let (to_mux, from_door) = mpsc::channel::<Frame>(32);
        let mut inbound = api.accept(ReceiverStream::new(from_door)).await?.into_inner();
        match inbound.message().await? {
            Some(Frame { payload: Some(frame::Payload::Target(remote)) }) => {
                tracing::debug!(host = %remote.host, port = remote.port, "door connection");
            }
            _ => anyhow::bail!("accept stream did not start with the remote address"),
        }
        let door = door.clone();
        tokio::spawn(async move {
            let tcp = match TcpStream::connect(&door).await {
                Ok(tcp) => tcp,
                Err(err) => {
                    tracing::warn!(%err, %door, "container door unreachable");
                    return;
                }
            };
            tcp.set_nodelay(true).ok();
            let (mut reader, mut writer) = tcp.into_split();
            let upstream = async {
                let mut buf = vec![0u8; CHUNK];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = Frame { payload: Some(frame::Payload::Data(buf[..n].to_vec())) };
                            if to_mux.send(data).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            };
            let downstream = async {
                while let Ok(Some(frame)) = inbound.message().await {
                    match frame.payload {
                        Some(frame::Payload::Data(bytes)) => {
                            if writer.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                writer.shutdown().await.ok();
            };
            tokio::select! { _ = upstream => {}, _ = downstream => {} }
        });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let mut socket = "/run/opdbus/tunnel/tunnel.sock".to_string();
    let mut lungs_socket = None;
    let mut door = None;
    let mut waiters = 4usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next().context("--socket needs a path")?,
            "--lungs-socket" => lungs_socket = Some(args.next().context("--lungs-socket needs a path")?),
            "--door" => door = Some(args.next().context("--door needs an address")?),
            "--waiters" => waiters = args.next().context("--waiters needs a number")?.parse()?,
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    if let Some(lungs_socket) = lungs_socket {
        let door = door.unwrap_or_else(|| "127.0.0.1:8443".into()).parse()?;
        return op_tunnel_mux::browser_fd::run(Path::new(&lungs_socket), door).await;
    }
    let door = door.unwrap_or_else(|| "10.0.0.2:8090".into());
    loop {
        match channel(socket.clone()).await {
            Ok(channel) => {
                tracing::info!(%socket, %door, waiters, "holding the door");
                let mut tasks = tokio::task::JoinSet::new();
                for _ in 0..waiters.max(1) {
                    tasks.spawn(waiter(channel.clone(), door.clone()));
                }
                if let Some(Ok(Err(err))) = tasks.join_next().await {
                    tracing::warn!(%err, "door waiter ended; reconnecting");
                }
                tasks.abort_all();
            }
            Err(err) => tracing::warn!(%err, "door socket unavailable"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
