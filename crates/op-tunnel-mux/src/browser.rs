//! Browser sockets use authenticated connection metadata, never request bytes, for registration.
use super::*;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{UnixListener, UnixStream};
use tokio_stream::{wrappers::UnixListenerStream, StreamExt};
use tonic::transport::server::Connected;

pub struct WaiterProof<K> {
    pub key: K,
    pub revalidate: Box<dyn Fn() -> bool + Send + Sync>,
}

#[tonic::async_trait]
pub trait WaiterIdentity<K>: Send + Sync + 'static {
    async fn identify(&self, stream: &UnixStream) -> anyhow::Result<WaiterProof<K>>;
}

pub struct ListenerMeta {
    pub port: u16,
}

#[tonic::async_trait]
pub trait Selector<K>: Send + Sync + 'static {
    async fn select(&self, listener: &ListenerMeta, stream: &TcpStream) -> Option<K>;
}

#[derive(Clone)]
pub struct BrowserMux<K> {
    accepts: AcceptQueue<K>,
    splices: Arc<Splices>,
}

pub struct BrowserStatus {
    pub open: bool,
    pub waiting: usize,
    pub active: usize,
}

impl<K: Eq + Hash> Default for BrowserMux<K> {
    fn default() -> Self { Self::new() }
}

impl<K: Eq + Hash> BrowserMux<K> {
    pub fn new() -> Self {
        Self { accepts: Default::default(), splices: Default::default() }
    }

    pub fn set_open(&self, open: bool) -> usize { self.splices.set_open(open) }
    pub fn close_active(&self) -> usize { self.splices.close_active() }

    pub async fn status(&self) -> BrowserStatus {
        let mut queue = self.accepts.inner.lock().await;
        let mut waiting = 0;
        queue.retain(|_, waiters| {
            waiters.retain(|w| !w.tx.is_closed());
            waiting += waiters.len();
            !waiters.is_empty()
        });
        BrowserStatus {
            open: self.splices.open.load(Ordering::SeqCst),
            waiting,
            active: self.splices.active.lock().expect("active map").len(),
        }
    }

    pub async fn deliver_to(&self, key: &K, stream: TcpStream) -> Result<(), &'static str> {
        if !self.splices.open.load(Ordering::SeqCst) { return Err("door is closed"); }
        self.accepts.deliver_to(key, stream).await
    }
}

#[tonic::async_trait]
impl<K: Eq + Hash + Clone + Send + Sync + 'static> Tunnel for BrowserMux<K> {
    type AcceptStream = AbortOnDrop;

    async fn accept(&self, request: Request<Streaming<Frame>>) -> Result<Response<Self::AcceptStream>, Status> {
        let proof = request.extensions().get::<Arc<WaiterProof<K>>>().cloned()
            .ok_or_else(|| Status::unauthenticated("browser peer proof required"))?;
        let (tx, rx) = oneshot::channel();
        self.accepts.push(proof.key.clone(), Waiter { proof: Some(proof), tx }).await;
        let tcp = rx.await.map_err(|_| Status::cancelled("browser waiter ended"))?;
        self.splices.start(None, request.into_inner(), tcp)
    }
}

struct Identified<K>(UnixStream, Arc<WaiterProof<K>>);

impl<K: Send + Sync + 'static> Connected for Identified<K> {
    type ConnectInfo = Arc<WaiterProof<K>>;
    fn connect_info(&self) -> Self::ConnectInfo { self.1.clone() }
}

impl<K> AsyncRead for Identified<K> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<K> AsyncWrite for Identified<K> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// Serve authenticated container Accept RPCs independently of the dynamic listener set.
pub async fn serve_browser<K: Eq + Hash + Clone + Send + Sync + 'static>(
    unix: UnixListener, mux: BrowserMux<K>, identity: Arc<dyn WaiterIdentity<K>>,
) -> anyhow::Result<()> {
    let incoming = UnixListenerStream::new(unix).then(|accepted| {
        let identity = &identity;
        async move {
            let stream = match accepted { Ok(stream) => stream, Err(err) => return Some(Err(err)) };
            match identity.identify(&stream).await {
                Ok(proof) => Some(Ok(Identified(stream, Arc::new(proof)))),
                Err(_) => { tracing::warn!("refused unidentified browser waiter"); None }
            }
        }
    }).filter_map(|item| item);
    tonic::transport::Server::builder().add_service(TunnelServer::new(mux)).serve_with_incoming(incoming).await?;
    Ok(())
}

/// The owner can stop listening on a removed port by cancelling this future.
pub async fn serve_browser_listener<K: Eq + Hash + Send + Sync + 'static>(
    listener: TcpListener, mux: BrowserMux<K>, selector: Arc<dyn Selector<K>>,
) -> anyhow::Result<()> {
    let meta = ListenerMeta { port: listener.local_addr()?.port() };
    loop {
        let (stream, _) = listener.accept().await?;
        if let Some(key) = selector.select(&meta, &stream).await {
            if let Err(reason) = mux.deliver_to(&key, stream).await { tracing::warn!(reason, "refused browser connection"); }
        } else {
            tracing::warn!("refused browser connection without a selected door");
        }
    }
}
