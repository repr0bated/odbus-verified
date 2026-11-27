//! Receive browser TCP descriptors from the identity's lungs and splice them locally.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::OwnedFd;
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
const MAX_WAITERS: usize = 16;
const MAX_ACTIVE: usize = 64;

async fn connection(socket: &Path) -> Result<zbus::Connection> {
    let stream = tokio::net::UnixStream::connect(socket).await.context("connect identity lungs")?;
    zbus::connection::Builder::unix_stream(stream)
        .p2p()
        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
        .build()
        .await
        .context("negotiate identity lungs")
}

async fn bind_current(connection: &zbus::Connection) -> Result<()> {
    let proxy = Proxy::new(connection, LUNGS_BUS_NAME, LUNGS_OBJECT_PATH, LUNGS_INTERFACE).await?;
    let bound: bool = proxy.call("BindCurrentContainer", &()).await?;
    ensure!(bound, "lungs refused the current container binding");
    Ok(())
}

async fn accept_browser(connection: &zbus::Connection) -> Result<TcpStream> {
    let proxy = Proxy::new(connection, LUNGS_BUS_NAME, LUNGS_OBJECT_PATH, LUNGS_INTERFACE).await?;
    let fd: zbus::zvariant::OwnedFd = proxy.call("AcceptBrowser", &()).await?;
    let fd: OwnedFd = fd.into();
    let stream = std::net::TcpStream::from(fd);
    stream.set_nonblocking(true)?;
    TcpStream::from_std(stream).context("adopt handed browser socket")
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

async fn waiter(
    connection: zbus::Connection,
    door: SocketAddr,
    active: Arc<Semaphore>,
) -> Result<()> {
    loop {
        let permit = active.clone().acquire_owned().await.context("browser splice owner stopped")?;
        let browser = accept_browser(&connection).await?;
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = splice(browser, door).await {
                tracing::warn!(%error, "browser splice closed");
            }
        });
    }
}

pub async fn run(socket: &Path, door: SocketAddr, waiters: usize) -> Result<()> {
    ensure!(door.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) && door.port() == 8443,
        "browser fd mode requires the local TLS door at 127.0.0.1:8443");
    ensure!((1..=MAX_WAITERS).contains(&waiters), "browser waiter count is out of bounds");
    let connection = connection(socket).await?;
    bind_current(&connection).await?;
    let active = Arc::new(Semaphore::new(MAX_ACTIVE));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..waiters {
        tasks.spawn(waiter(connection.clone(), door, active.clone()));
    }
    match tasks.join_next().await {
        Some(Ok(Err(error))) => Err(error).context("browser waiter ended"),
        Some(Err(error)) => Err(error).context("browser waiter task failed"),
        Some(Ok(Ok(()))) | None => anyhow::bail!("browser waiters stopped"),
    }
}
