//! Receive browser TCP sockets pushed by the host over the identity's lungs
//! connection and splice each one, untouched, to the local TLS door.
//!
//! The door exports `org.opdbus.v1.BrowserV1` on its lungs connection and
//! registers it once. The host calls `ReceiveBrowser(h)` for every browser
//! connection that names this identity. Nothing here waits for a socket.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use zbus::Proxy;

const LUNGS_OBJECT_PATH: &str = "/org/opdbus/v1/lungs";
const LUNGS_INTERFACE: &str = "org.opdbus.v1.LungsV1";
const LUNGS_BUS_NAME: &str = "org.opdbus.v1";
const RECEIVER_PATH: &str = "/org/opdbus/v1/browser";
const MAX_ACTIVE: usize = 64;
/// Where the identity finds its own UI URI.
pub const UI_URI_FILE: &str = "/run/opdbus/browser/ui-uri";

struct Receiver {
    door: SocketAddr,
    active: Arc<Semaphore>,
}

#[zbus::interface(name = "org.opdbus.v1.BrowserV1")]
impl Receiver {
    /// Take one browser socket. `false` closes it on the host side.
    async fn receive_browser(&self, fd: zbus::zvariant::OwnedFd) -> bool {
        let Ok(permit) = self.active.clone().try_acquire_owned() else { return false; };
        let browser = match adopt(fd.into()) {
            Ok(browser) => browser,
            Err(error) => {
                tracing::warn!(%error, "pushed browser socket refused");
                return false;
            }
        };
        let door = self.door;
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = splice(browser, door).await {
                tracing::debug!(%error, "browser splice closed");
            }
        });
        true
    }
}

fn adopt(fd: OwnedFd) -> Result<TcpStream> {
    let stream = std::net::TcpStream::from(fd);
    stream.set_nonblocking(true)?;
    TcpStream::from_std(stream).context("adopt pushed browser socket")
}

async fn splice(browser: TcpStream, door: SocketAddr) -> Result<()> {
    let local = TcpStream::connect(door).await.context("connect local UI door")?;
    browser.set_nodelay(true)?;
    local.set_nodelay(true)?;
    let (mut browser_read, mut browser_write) = browser.into_split();
    let (mut local_read, mut local_write) = local.into_split();
    let browser_to_local = async {
        let copied = tokio::io::copy(&mut browser_read, &mut local_write).await?;
        local_write.shutdown().await?;
        Ok::<_, std::io::Error>(copied)
    };
    let local_to_browser = async {
        let copied = tokio::io::copy(&mut local_read, &mut browser_write).await?;
        browser_write.shutdown().await?;
        Ok::<_, std::io::Error>(copied)
    };
    let _ = tokio::try_join!(browser_to_local, local_to_browser)?;
    Ok(())
}

fn write_ui_uri(uri: &str) -> Result<()> {
    let path = Path::new(UI_URI_FILE);
    let parent = path.parent().context("UI URI file has no parent")?;
    std::fs::create_dir_all(parent)?;
    let staged = path.with_extension("tmp");
    std::fs::write(&staged, format!("{uri}\n"))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o644))?;
    std::fs::rename(&staged, path)?;
    Ok(())
}

/// Serve pushed browser sockets for this identity until its lungs connection ends.
pub async fn run(socket: &Path, door: SocketAddr) -> Result<()> {
    ensure!(door.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) && door.port() == 8443,
        "browser mode requires the local TLS door at 127.0.0.1:8443");
    let stream = tokio::net::UnixStream::connect(socket).await.context("connect identity lungs")?;
    let connection = zbus::connection::Builder::unix_stream(stream)
        .p2p()
        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
        .serve_at(RECEIVER_PATH, Receiver { door, active: Arc::new(Semaphore::new(MAX_ACTIVE)) })?
        .build()
        .await
        .context("negotiate identity lungs")?;
    let lungs = Proxy::new(&connection, LUNGS_BUS_NAME, LUNGS_OBJECT_PATH, LUNGS_INTERFACE).await?;
    let bound: bool = lungs.call("BindCurrentContainer", &()).await?;
    ensure!(bound, "lungs refused the current container binding");
    let registered: bool = lungs.call("RegisterBrowserReceiver", &()).await?;
    ensure!(registered, "lungs refused the browser receiver");
    match lungs.call::<_, _, String>("BrowserUri", &()).await {
        Ok(uri) => write_ui_uri(&uri)?,
        Err(error) => tracing::warn!(%error, "UI URI unavailable"),
    }
    tracing::info!("browser receiver registered");
    connection.closed().await;
    anyhow::bail!("identity lungs connection closed")
}
