//! Push browser sockets to the identity's own lungs connection.
//!
//! The container's lungs client exports `org.opdbus.v1.BrowserV1` and says so
//! once with `RegisterBrowserReceiver`. The host pushes each accepted socket
//! with `ReceiveBrowser(h)` on that connection. Nothing waits or polls: the
//! entry exists while the connection is open and goes when it closes.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, ensure};

const RECEIVER_PATH: &str = "/org/opdbus/v1/browser";
const RECEIVER_INTERFACE: &str = "org.opdbus.v1.BrowserV1";
const MAX_RECEIVERS: usize = 64;
const PUSH_TIMEOUT: Duration = Duration::from_secs(5);

struct Receiver {
    connection_id: u64,
    connection: zbus::Connection,
}

/// The index of identities whose container can receive a browser socket now,
/// keyed by `session_id`. Written only by the identity's own registration and
/// by its connection closing.
#[derive(Default)]
pub(crate) struct BrowserReceivers {
    by_session: Mutex<HashMap<String, Receiver>>,
}

impl BrowserReceivers {
    pub(crate) fn register(&self, session_id: String, connection_id: u64, connection: zbus::Connection) -> Result<()> {
        let mut receivers = self.by_session.lock().map_err(|_| anyhow::anyhow!("browser receivers poisoned"))?;
        ensure!(
            receivers.len() < MAX_RECEIVERS || receivers.contains_key(&session_id),
            "browser receiver bound reached"
        );
        // A newer connection of the same identity replaces the older one.
        receivers.insert(session_id, Receiver { connection_id, connection });
        Ok(())
    }

    pub(crate) fn forget_connection(&self, connection_id: u64) {
        if let Ok(mut receivers) = self.by_session.lock() {
            receivers.retain(|_, receiver| receiver.connection_id != connection_id);
        }
    }

    /// Push one accepted socket to the identity. No receiver, a failed call,
    /// or a refusal closes the socket; it never goes anywhere else.
    pub(crate) async fn push(&self, session_id: &str, fd: OwnedFd) -> Result<()> {
        let connection = self.by_session.lock()
            .map_err(|_| anyhow::anyhow!("browser receivers poisoned"))?
            .get(session_id)
            .map(|receiver| receiver.connection.clone())
            .context("identity has no browser receiver")?;
        let fd = zbus::zvariant::OwnedFd::from(fd);
        let reply = tokio::time::timeout(
            PUSH_TIMEOUT,
            connection.call_method(None::<&str>, RECEIVER_PATH, Some(RECEIVER_INTERFACE), "ReceiveBrowser", &(fd,)),
        )
        .await
        .context("browser push timed out")?
        .context("browser push failed")?;
        let accepted: bool = reply.body().deserialize().context("browser push reply is malformed")?;
        ensure!(accepted, "identity refused the browser socket");
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn receiver_count(&self) -> usize {
        self.by_session.lock().map(|receivers| receivers.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    struct Sink(tokio::sync::mpsc::Sender<OwnedFd>);

    #[zbus::interface(name = "org.opdbus.v1.BrowserV1")]
    impl Sink {
        async fn receive_browser(&self, fd: zbus::zvariant::OwnedFd) -> bool {
            self.0.send(fd.into()).await.is_ok()
        }
    }

    async fn pair() -> (zbus::Connection, zbus::Connection, tokio::sync::mpsc::Receiver<OwnedFd>) {
        let (host, container) = tokio::net::UnixStream::pair().unwrap();
        let (sender, received) = tokio::sync::mpsc::channel(4);
        let host = zbus::connection::Builder::unix_stream(host)
            .server(zbus::Guid::generate()).unwrap().p2p()
            .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
            .build();
        let container = zbus::connection::Builder::unix_stream(container).p2p()
            .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
            .serve_at(RECEIVER_PATH, Sink(sender)).unwrap()
            .build();
        let (host, container) = tokio::try_join!(host, container).unwrap();
        (host, container, received)
    }

    fn socket() -> (std::os::unix::net::UnixStream, OwnedFd) {
        let (kept, pushed) = std::os::unix::net::UnixStream::pair().unwrap();
        (kept, pushed.into())
    }

    #[tokio::test]
    async fn each_push_reaches_only_its_own_identity() {
        let receivers = BrowserReceivers::default();
        let (host_a, _container_a, mut received_a) = pair().await;
        let (host_b, _container_b, mut received_b) = pair().await;
        receivers.register("session-a".into(), 1, host_a).unwrap();
        receivers.register("session-b".into(), 2, host_b).unwrap();

        let (mut kept, pushed) = socket();
        receivers.push("session-a", pushed).await.unwrap();
        let delivered = received_a.recv().await.unwrap();
        assert!(received_b.try_recv().is_err());

        let mut delivered = std::os::unix::net::UnixStream::from(delivered);
        kept.write_all(b"hello").unwrap();
        let mut bytes = [0u8; 5];
        delivered.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"hello");
    }

    #[tokio::test]
    async fn no_receiver_or_a_closed_one_delivers_nothing() {
        let receivers = BrowserReceivers::default();
        let (_, pushed) = socket();
        assert!(receivers.push("unknown", pushed).await.is_err());

        let (host, _container, _received) = pair().await;
        receivers.register("session-a".into(), 7, host).unwrap();
        assert_eq!(receivers.receiver_count(), 1);
        receivers.forget_connection(7);
        assert_eq!(receivers.receiver_count(), 0);
        let (_, pushed) = socket();
        assert!(receivers.push("session-a", pushed).await.is_err());
    }
}
