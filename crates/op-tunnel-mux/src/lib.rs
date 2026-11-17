//! Host tonic mux for NIC-less containers.
//!
//! One mux serves two sockets that share the inbound door queue:
//! - `/run/opdbus/tunnel/egress/tunnel.sock`: `Open` (outbound, first frame
//!   names the target) and `Accept`. Bound into identity containers (ID-N).
//! - `/run/opdbus/tunnel/inbound/tunnel.sock`: `Accept` only; `Open` is
//!   refused. Bound into the control-plane chatbot, which has no internet:
//!   its outbound actions are sealed method calls over `lungs.sock`.
//!
//! Provisioning binds exactly one of the two directories into a container at
//! `/run/opdbus/tunnel`, so the policy is which socket exists, not a field the
//! caller sends. Identity is the Unix peer credential. Cancellation drops the
//! spliced TCP connection.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tonic::transport::server::UdsConnectInfo;
use tonic::{Request, Response, Status, Streaming};

pub mod proto {
    tonic::include_proto!("operation.tunnel.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/tunnel_descriptor.bin"));
}

pub use proto::tunnel_client::TunnelClient;
pub use proto::tunnel_server::{Tunnel, TunnelServer};
pub use proto::{frame, Frame, Target};

const MAX_HOST_LEN: usize = 253;
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Default)]
struct AcceptQueue {
    inner: Arc<Mutex<HashMap<u32, Vec<oneshot::Sender<TcpStream>>>>>,
}

impl AcceptQueue {
    async fn push(&self, uid: u32, tx: oneshot::Sender<TcpStream>) {
        self.inner.lock().await.entry(uid).or_default().push(tx);
    }

    /// Deliver an inbound TCP connection to the one waiting container.
    /// More than one uid is ambiguous for the single published door, so refuse.
    async fn deliver(&self, stream: TcpStream) -> Result<(), &'static str> {
        let mut guard = self.inner.lock().await;
        guard.retain(|_, waiters| {
            waiters.retain(|tx| !tx.is_closed());
            !waiters.is_empty()
        });
        let uids: Vec<u32> = guard.keys().copied().collect();
        if uids.len() != 1 {
            return Err("inbound door owner is not a single connected container");
        }
        let uid = uids[0];
        let waiter = guard.get_mut(&uid).and_then(|w| w.pop());
        drop(guard);
        match waiter {
            Some(tx) => tx.send(stream).map_err(|_| "container accept ended"),
            None => Err("no container accept stream"),
        }
    }
}

#[derive(Clone, Default)]
pub struct Mux {
    accepts: AcceptQueue,
    allow_open: bool,
}

impl Mux {
    /// The egress mux: `Open` and `Accept`.
    pub fn new() -> Self {
        Self { accepts: AcceptQueue::default(), allow_open: true }
    }

    /// The inbound-only view of the same door queue: `Open` is refused.
    pub fn inbound_only(&self) -> Self {
        Self { accepts: self.accepts.clone(), allow_open: false }
    }

    pub async fn deliver_inbound(&self, stream: TcpStream) -> Result<(), &'static str> {
        self.accepts.deliver(stream).await
    }
}

fn peer_uid<T>(request: &Request<T>) -> Result<u32, Status> {
    request
        .extensions()
        .get::<UdsConnectInfo>()
        .and_then(|info| info.peer_cred)
        .map(|cred| cred.uid())
        .ok_or_else(|| Status::unauthenticated("unix peer credentials required"))
}

fn parse_target(target: &Target) -> Result<SocketAddr, Status> {
    if target.host.is_empty()
        || target.host.len() > MAX_HOST_LEN
        || target.host.chars().any(char::is_whitespace)
    {
        return Err(Status::invalid_argument("target host is empty or invalid"));
    }
    let port = u16::try_from(target.port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Status::invalid_argument("target port is out of range"))?;
    format!("{}:{}", target.host, port)
        .parse()
        .map_err(|_| Status::invalid_argument("target host must be an IP address"))
}

fn frame_data(bytes: Vec<u8>) -> Frame {
    Frame { payload: Some(frame::Payload::Data(bytes)) }
}

fn frame_target(addr: SocketAddr) -> Frame {
    Frame {
        payload: Some(frame::Payload::Target(Target {
            host: addr.ip().to_string(),
            port: u32::from(addr.port()),
        })),
    }
}

/// Response stream that aborts the splice task when the RPC is dropped.
pub struct AbortOnDrop {
    rx: mpsc::Receiver<Result<Frame, Status>>,
    abort: tokio::task::AbortHandle,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

impl tokio_stream::Stream for AbortOnDrop {
    type Item = Result<Frame, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Splice a gRPC frame stream with a TCP connection. `first` is sent to the
/// container before any bytes (Accept's remote address). After that, both
/// Open and Accept carry only bytes.
fn spawn_splice(mut inbound: Streaming<Frame>, tcp: TcpStream, first: Option<Frame>) -> AbortOnDrop {
    let (tx, rx) = mpsc::channel(32);
    let task = tokio::spawn(async move {
        if let Some(frame) = first {
            if tx.send(Ok(frame)).await.is_err() {
                return;
            }
        }
        let (mut reader, mut writer) = tokio::io::split(tcp);
        let mut buf = vec![0u8; CHUNK];
        loop {
            tokio::select! {
                incoming = inbound.message() => {
                    match incoming {
                        Ok(None) => break,
                        Ok(Some(frame)) => match frame.payload {
                            Some(frame::Payload::Data(bytes)) => {
                                if writer.write_all(&bytes).await.is_err() {
                                    break;
                                }
                            }
                            Some(frame::Payload::Target(_)) => {
                                let _ = tx.send(Err(Status::invalid_argument("target is not accepted on this stream"))).await;
                                break;
                            }
                            None => {
                                let _ = tx.send(Err(Status::invalid_argument("empty frame"))).await;
                                break;
                            }
                        },
                        Err(status) => {
                            let _ = tx.send(Err(status)).await;
                            break;
                        }
                    }
                }
                read = reader.read(&mut buf) => {
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(Ok(frame_data(buf[..n].to_vec()))).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        writer.shutdown().await.ok();
    });
    AbortOnDrop { rx, abort: task.abort_handle() }
}

#[tonic::async_trait]
impl Tunnel for Mux {
    type OpenStream = AbortOnDrop;
    type AcceptStream = AbortOnDrop;

    async fn open(&self, request: Request<Streaming<Frame>>) -> Result<Response<Self::OpenStream>, Status> {
        if !self.allow_open {
            return Err(Status::permission_denied("this container has no outbound tunnel"));
        }
        peer_uid(&request)?;
        let mut inbound = request.into_inner();
        let target = match inbound.message().await?.and_then(|frame| frame.payload) {
            Some(frame::Payload::Target(target)) => target,
            _ => return Err(Status::invalid_argument("first frame must name the target")),
        };
        let addr = parse_target(&target)?;
        let tcp = TcpStream::connect(addr)
            .await
            .map_err(|err| Status::unavailable(format!("connect {addr}: {err}")))?;
        tcp.set_nodelay(true).ok();
        Ok(Response::new(spawn_splice(inbound, tcp, None)))
    }

    async fn accept(&self, request: Request<Streaming<Frame>>) -> Result<Response<Self::AcceptStream>, Status> {
        let uid = peer_uid(&request)?;
        let inbound = request.into_inner();
        let (tx, rx) = oneshot::channel();
        self.accepts.push(uid, tx).await;
        let tcp = rx.await.map_err(|_| Status::cancelled("inbound splice cancelled"))?;
        let remote = tcp.peer_addr().unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0)));
        Ok(Response::new(spawn_splice(inbound, tcp, Some(frame_target(remote)))))
    }
}

/// Serve `mux` on an already-bound Unix listener. `inbound` receives the
/// bridge-side door connections and splices each one to the single waiting
/// container. Dropping either side, or a deadline that cancels the RPC, closes
/// the TCP connection.
pub async fn serve(
    unix: tokio::net::UnixListener,
    inbound: Option<TcpListener>,
    mux: Mux,
) -> anyhow::Result<()> {
    serve_both(unix, None, inbound, mux).await
}

/// Serve the egress socket and, if given, the inbound-only socket, sharing
/// one door queue.
pub async fn serve_both(
    egress: tokio::net::UnixListener,
    inbound_only: Option<tokio::net::UnixListener>,
    inbound: Option<TcpListener>,
    mux: Mux,
) -> anyhow::Result<()> {
    if let Some(unix) = inbound_only {
        let view = mux.inbound_only();
        tokio::spawn(async move {
            if let Err(err) = serve_unix(unix, view).await {
                tracing::error!(%err, "inbound-only tunnel socket stopped");
            }
        });
    }
    if let Some(listener) = inbound {
        let deliver = mux.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        if deliver.deliver_inbound(stream).await.is_err() {
                            tracing::warn!("refused inbound door splice");
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "inbound accept failed");
                        break;
                    }
                }
            }
        });
    }
    serve_unix(egress, mux).await
}

async fn serve_unix(unix: tokio::net::UnixListener, mux: Mux) -> anyhow::Result<()> {
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    tonic::transport::Server::builder()
        .add_service(TunnelServer::new(mux))
        .add_service(reflection)
        .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(unix))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;
    use tonic::transport::{Endpoint, Uri};
    use tower::service_fn;

    /// Returns (egress socket, inbound-only socket, door address).
    async fn start_mux(tag: &str) -> (String, String, SocketAddr) {
        let dir = std::env::temp_dir().join(format!("tunnel-mux-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let egress = dir.join("egress.sock");
        let only = dir.join("inbound.sock");
        let egress_unix = tokio::net::UnixListener::bind(&egress).unwrap();
        let only_unix = tokio::net::UnixListener::bind(&only).unwrap();
        let door = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let door_addr = door.local_addr().unwrap();
        tokio::spawn(async move {
            serve_both(egress_unix, Some(only_unix), Some(door), Mux::new()).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (egress.display().to_string(), only.display().to_string(), door_addr)
    }

    async fn echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).await.unwrap();
            stream.write_all(&buf[..n]).await.unwrap();
        });
        addr
    }

    fn target(addr: SocketAddr) -> Frame {
        frame_target(addr)
    }

    #[tokio::test]
    async fn egress_socket_opens_to_the_named_target() {
        let (egress, _, _) = start_mux("open").await;
        let echo = echo_server().await;
        let mut api = client(&egress).await;
        let outbound = tokio_stream::iter(vec![target(echo), frame_data(b"ping".to_vec())]);
        let mut inbound = api.open(outbound).await.unwrap().into_inner();
        match inbound.next().await.unwrap().unwrap().payload {
            Some(frame::Payload::Data(bytes)) => assert_eq!(bytes, b"ping"),
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn inbound_only_socket_refuses_open() {
        let (_, only, _) = start_mux("refuse").await;
        let echo = echo_server().await;
        let mut api = client(&only).await;
        let outbound = tokio_stream::iter(vec![target(echo)]);
        let err = api.open(outbound).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    async fn client(path: &str) -> TunnelClient<tonic::transport::Channel> {
        let path = path.to_string();
        let channel = Endpoint::try_from("http://[::]:0").unwrap()
            .connect_with_connector(service_fn(move |_: Uri| {
                let path = path.clone();
                async move { tokio::net::UnixStream::connect(path).await.map(TokioIo::new) }
            }))
            .await
            .unwrap();
        TunnelClient::new(channel)
    }

    #[tokio::test]
    async fn accept_splices_a_door_connection_into_the_container() {
        let (_, only, door) = start_mux("accept").await;
        let mut api = client(&only).await;
        let (to_mux, from_container) = mpsc::channel(4);
        let accept = tokio::spawn(async move {
            api.accept(ReceiverStream::new(from_container)).await.unwrap().into_inner()
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut remote = TcpStream::connect(door).await.unwrap();
        let mut inbound = accept.await.unwrap();
        match inbound.next().await.unwrap().unwrap().payload {
            Some(frame::Payload::Target(_)) => {}
            other => panic!("expected the remote address first, got {other:?}"),
        }
        remote.write_all(b"hello").await.unwrap();
        match inbound.next().await.unwrap().unwrap().payload {
            Some(frame::Payload::Data(bytes)) => assert_eq!(bytes, b"hello"),
            other => panic!("expected data, got {other:?}"),
        }
        to_mux.send(frame_data(b"back".to_vec())).await.unwrap();
        let mut buf = [0u8; 4];
        remote.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"back");
    }

    #[tokio::test]
    async fn a_door_connection_without_a_waiting_container_is_refused() {
        let mux = Mux::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = TcpStream::connect(addr).await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        assert!(mux.deliver_inbound(stream).await.is_err());
    }
}
