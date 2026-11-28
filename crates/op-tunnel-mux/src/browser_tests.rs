use super::*;
use tokio::net::{UnixListener, UnixStream};
use tokio::time::{timeout, Duration};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};

const LIMIT: Duration = Duration::from_secs(5);

async fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
    (listener.accept().await.unwrap().0, remote)
}

async fn push(queue: &AcceptQueue<u32>, key: u32, live: bool) -> oneshot::Receiver<TcpStream> {
    let (tx, rx) = oneshot::channel();
    let proof = WaiterProof { key, revalidate: Box::new(move || live) };
    queue.push(key, Waiter { proof: Some(Arc::new(proof)), tx }).await;
    rx
}

#[tokio::test]
async fn delivers_only_to_selected_live_waiter() {
    let queue = AcceptQueue::default();
    let selected = push(&queue, 1, true).await;
    let expired = push(&queue, 1, false).await;
    drop(push(&queue, 1, true).await);
    let mut other = push(&queue, 2, true).await;
    let (stream, peer) = tcp_pair().await;
    queue.deliver_to(&1, stream).await.unwrap();
    assert_eq!(selected.await.unwrap().peer_addr().unwrap(), peer.local_addr().unwrap());
    assert!(expired.await.is_err());
    let (stream, mut peer) = tcp_pair().await;
    assert!(queue.deliver_to(&1, stream).await.is_err());
    assert_eq!(timeout(LIMIT, peer.read(&mut [0])).await.unwrap().unwrap(), 0);
    assert!(matches!(other.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
}

struct FakeIdentity(Option<u32>);
#[tonic::async_trait]
impl WaiterIdentity<u32> for FakeIdentity {
    async fn identify(&self, _: &UnixStream) -> anyhow::Result<WaiterProof<u32>> {
        Ok(WaiterProof { key: self.0.ok_or_else(|| anyhow::anyhow!("not a container"))?, revalidate: Box::new(|| true) })
    }
}

fn socket(tag: &str) -> (String, UnixListener) {
    let path = std::env::temp_dir().join(format!("browser-mux-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    (path.display().to_string(), listener)
}

#[tokio::test]
async fn unidentified_peer_is_closed_before_grpc() {
    let (path, unix) = socket("unidentified");
    let mux = BrowserMux::new();
    let server = tokio::spawn(serve_browser(unix, mux.clone(), Arc::new(FakeIdentity(None))));
    let mut peer = UnixStream::connect(&path).await.unwrap();
    assert_eq!(timeout(LIMIT, peer.read(&mut [0])).await.unwrap().unwrap(), 0);
    assert_eq!(mux.status().await.waiting, 0);
    server.abort();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn accept_without_proof_is_unauthenticated() {
    let (path, unix) = socket("no-proof");
    let mux = BrowserMux::<u32>::new();
    let server = tokio::spawn(tonic::transport::Server::builder().add_service(TunnelServer::new(mux.clone()))
        .serve_with_incoming(UnixListenerStream::new(unix)));
    let mut api = tests::client(&path).await;
    let (_tx, rx) = mpsc::channel(1);
    let err = timeout(LIMIT, api.accept(ReceiverStream::new(rx))).await.unwrap().unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
    assert_eq!(mux.status().await.waiting, 0);
    server.abort();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn proof_keys_the_browser_waiter_and_doors_are_isolated() {
    let (path, unix) = socket("isolation");
    let browser = BrowserMux::new();
    let control = Mux::new();
    let server = tokio::spawn(serve_browser(unix, browser.clone(), Arc::new(FakeIdentity(Some(7)))));
    let mut api = tests::client(&path).await;
    let (_tx, rx) = mpsc::channel(1);
    let accept = tokio::spawn(async move { api.accept(ReceiverStream::new(rx)).await.unwrap().into_inner() });
    timeout(LIMIT, async { while browser.status().await.waiting != 1 { tokio::task::yield_now().await; } }).await.unwrap();
    assert!(control.deliver_inbound(tcp_pair().await.0).await.is_err());
    let (stream, mut peer) = tcp_pair().await;
    browser.deliver_to(&7, stream).await.unwrap();
    let mut rpc = timeout(LIMIT, accept).await.unwrap().unwrap();
    assert!(matches!(rpc.message().await.unwrap().unwrap().payload, Some(frame::Payload::Target(_))));
    let (tx, mut control_waiter) = oneshot::channel();
    control.accepts.push(7, Waiter { proof: None, tx }).await;
    assert!(browser.deliver_to(&7, tcp_pair().await.0).await.is_err());
    assert!(matches!(control_waiter.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    assert_eq!(browser.set_open(false), 1);
    assert_eq!(timeout(LIMIT, peer.read(&mut [0])).await.unwrap().unwrap(), 0);
    assert_eq!(browser.deliver_to(&7, tcp_pair().await.0).await, Err("door is closed"));
    assert_eq!(browser.status().await.active, 0);
    assert!(control.is_open());
    server.abort();
    std::fs::remove_file(path).unwrap();
}
