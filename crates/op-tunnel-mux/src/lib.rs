//! Host tonic mux delivering the door into the container that owns it.
//!
//! The socket is `/run/opdbus/tunnel/tunnel.sock`, bound only into the
//! door-owning container (the control-plane chatbot). That container holds
//! `Accept` open; each door connection is spliced to it, first frame the
//! remote address, later frames bytes. More than one waiting container is
//! refused. Inbound only: NIC-less egress for identity containers is
//! ct-egress (loopback socat to host Unix sockets); the chatbot has none.
//! Identity is the Unix peer credential. Cancellation drops the TCP connection.
//!
//! The host bridge owns the mux as the sealed `tunnel` plugin: its status and
//! open/close controls are methods visible through 3tchedFS; the bytes stay on
//! this socket.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

const CHUNK: usize = 16 * 1024;

mod browser;
pub use browser::{BrowserMux, BrowserStatus, ListenerMeta, Selector, WaiterIdentity, WaiterProof, serve_browser, serve_browser_listener};

#[cfg(test)]
mod browser_tests;

struct Waiter<K> {
    proof: Option<Arc<WaiterProof<K>>>,
    tx: oneshot::Sender<TcpStream>,
}

#[derive(Clone)]
struct AcceptQueue<K> {
    inner: Arc<Mutex<HashMap<K, Vec<Waiter<K>>>>>,
}

impl<K> Default for AcceptQueue<K> {
    fn default() -> Self { Self { inner: Arc::default() } }
}

impl<K: Hash + Eq> AcceptQueue<K> {
    async fn push(&self, key: K, waiter: Waiter<K>) {
        self.inner.lock().await.entry(key).or_default().push(waiter);
    }

    async fn deliver_to(&self, key: &K, mut stream: TcpStream) -> Result<(), &'static str> {
        let mut queue = self.inner.lock().await;
        if let Some(waiters) = queue.get_mut(key) {
            waiters.retain(|w| !w.tx.is_closed());
            while let Some(waiter) = waiters.pop() {
                if !waiter.proof.is_some_and(|p| (p.revalidate)()) {
                    continue;
                }
                match waiter.tx.send(stream) {
                    Ok(()) => {
                        if waiters.is_empty() { queue.remove(key); }
                        return Ok(());
                    }
                    Err(returned) => stream = returned,
                }
            }
            queue.remove(key);
        }
        Err("no live waiter for selected door")
    }

    /// Deliver an inbound TCP connection to the one waiting container.
    /// More than one uid is ambiguous for the single published door, so refuse.
    async fn deliver_sole(&self, stream: TcpStream) -> Result<(), &'static str> {
        let mut guard = self.inner.lock().await;
        guard.retain(|_, waiters| {
            waiters.retain(|w| !w.tx.is_closed());
            !waiters.is_empty()
        });
        if guard.len() != 1 {
            return Err("inbound door owner is not a single connected container");
        }
        let waiter = guard.values_mut().next().and_then(|w| w.pop());
        drop(guard);
        match waiter {
            Some(w) => w.tx.send(stream).map_err(|_| "container accept ended"),
            None => Err("no container accept stream"),
        }
    }
}

struct Active {
    uid: Option<u32>,
    since: i64,
    abort: tokio::task::AbortHandle,
}

#[derive(Clone)]
pub struct Mux {
    accepts: AcceptQueue<u32>,
    splices: Arc<Splices>,
}

struct Splices {
    open: AtomicBool,
    next_id: AtomicU64,
    active: std::sync::Mutex<HashMap<u64, Active>>,
}

impl Default for Splices {
    fn default() -> Self {
        Self { open: AtomicBool::new(true), next_id: AtomicU64::new(1), active: Default::default() }
    }
}

impl Default for Mux {
    fn default() -> Self {
        Self { accepts: Default::default(), splices: Default::default() }
    }
}

/// A point-in-time view of the door, for the `tunnel` plugin state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxStatus {
    pub open: bool,
    /// Peer uids holding an Accept; one is the door owner.
    pub waiting_uids: Vec<u32>,
    pub waiting: usize,
    pub active: usize,
    /// Unix seconds of the oldest active splice.
    pub oldest_active_since: Option<i64>,
}

impl Mux {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn deliver_inbound(&self, stream: TcpStream) -> Result<(), &'static str> {
        if !self.is_open() {
            return Err("door is closed");
        }
        self.accepts.deliver_sole(stream).await
    }

    pub fn is_open(&self) -> bool {
        self.splices.open.load(Ordering::SeqCst)
    }

    /// Open or close the door. Closing refuses new door connections and
    /// drops every active splice; returns how many were dropped.
    pub fn set_open(&self, open: bool) -> usize {
        self.splices.set_open(open)
    }

    /// Drop every active splice without changing open/closed.
    pub fn close_active(&self) -> usize {
        self.splices.close_active()
    }

    pub async fn status(&self) -> MuxStatus {
        let mut waiting_uids = Vec::new();
        let mut waiting = 0;
        {
            let mut guard = self.accepts.inner.lock().await;
            guard.retain(|_, waiters| {
                waiters.retain(|w| !w.tx.is_closed());
                !waiters.is_empty()
            });
            for (uid, waiters) in guard.iter() {
                waiting_uids.push(*uid);
                waiting += waiters.len();
            }
        }
        waiting_uids.sort_unstable();
        let active = self.splices.active.lock().expect("active map");
        MuxStatus {
            open: self.is_open(),
            waiting_uids,
            waiting,
            active: active.len(),
            oldest_active_since: active.values().map(|a| a.since).min(),
        }
    }

    /// Active splices per owning uid (diagnostics).
    pub fn active_uids(&self) -> Vec<u32> {
        let mut uids: Vec<u32> = self.splices.active.lock().expect("active map").values().filter_map(|a| a.uid).collect();
        uids.sort_unstable();
        uids.dedup();
        uids
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

/// Splice a gRPC frame stream with a TCP connection. `first` (the remote
/// address) is sent to the container before any bytes.
fn spawn_splice(
    mut inbound: Streaming<Frame>,
    tcp: TcpStream,
    first: Option<Frame>,
    done: impl FnOnce() + Send + 'static,
) -> (AbortOnDrop, tokio::task::AbortHandle) {
    let (tx, rx) = mpsc::channel(32);
    let done = OnDrop(Some(done));
    let task = tokio::spawn(async move {
        let _done = done;
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
    let abort = task.abort_handle();
    (AbortOnDrop { rx, abort: abort.clone() }, abort)
}

/// Runs its closure when dropped, including when the splice task is aborted.
struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(done) = self.0.take() {
            done();
        }
    }
}

#[tonic::async_trait]
impl Tunnel for Mux {
    type AcceptStream = AbortOnDrop;

    async fn accept(&self, request: Request<Streaming<Frame>>) -> Result<Response<Self::AcceptStream>, Status> {
        let uid = peer_uid(&request)?;
        let inbound = request.into_inner();
        let (tx, rx) = oneshot::channel();
        self.accepts.push(uid, Waiter { proof: None, tx }).await;
        let tcp = rx.await.map_err(|_| Status::cancelled("inbound splice cancelled"))?;
        self.splices.start(Some(uid), inbound, tcp)
    }
}

impl Splices {
    fn set_open(&self, open: bool) -> usize {
        self.open.store(open, Ordering::SeqCst);
        if open { 0 } else { self.close_active() }
    }

    fn close_active(&self) -> usize {
        let mut active = self.active.lock().expect("active map");
        let count = active.len();
        for (_, conn) in active.drain() { conn.abort.abort(); }
        count
    }

    fn start(self: &Arc<Self>, uid: Option<u32>, inbound: Streaming<Frame>, tcp: TcpStream) -> Result<Response<AbortOnDrop>, Status> {
        let mut active = self.active.lock().expect("active map");
        if !self.open.load(Ordering::SeqCst) { return Err(Status::unavailable("door is closed")); }
        let remote = tcp.peer_addr().unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0)));
        // Register before the splice can finish, so its untrack always follows.
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let splices = self.clone();
        let (stream, abort) = spawn_splice(inbound, tcp, Some(frame_target(remote)), move || {
            splices.active.lock().expect("active map").remove(&id);
        });
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        active.insert(id, Active { uid, since, abort });
        Ok(Response::new(stream))
    }
}

/// Serve the mux on an already-bound Unix listener. `inbound` receives the
/// door connections and splices each one to the single waiting container.
/// Dropping either side, or a deadline that cancels the RPC, closes the TCP
/// connection.
pub async fn serve(
    unix: tokio::net::UnixListener,
    inbound: Option<TcpListener>,
    mux: Mux,
) -> anyhow::Result<()> {
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
    serve_unix(unix, mux).await
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

    /// Returns (socket, door address).
    async fn start_mux(tag: &str) -> (String, SocketAddr) {
        let dir = std::env::temp_dir().join(format!("tunnel-mux-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tunnel.sock");
        let unix = tokio::net::UnixListener::bind(&path).unwrap();
        let door = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let door_addr = door.local_addr().unwrap();
        tokio::spawn(async move {
            serve(unix, Some(door), Mux::new()).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (path.display().to_string(), door_addr)
    }

    pub(super) async fn client(path: &str) -> TunnelClient<tonic::transport::Channel> {
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
        let (path, door) = start_mux("accept").await;
        let mut api = client(&path).await;
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
    async fn a_closed_door_refuses_and_drops_active_splices() {
        let mux = Mux::new();
        assert!(mux.is_open());
        assert_eq!(mux.set_open(false), 0);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = TcpStream::connect(addr).await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        assert_eq!(mux.deliver_inbound(stream).await, Err("door is closed"));
        let status = mux.status().await;
        assert!(!status.open);
        assert_eq!(status.active, 0);
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
