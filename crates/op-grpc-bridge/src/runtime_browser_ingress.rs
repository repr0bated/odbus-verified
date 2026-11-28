//! The shared `:8080` browser entry.
//!
//! Each accepted socket names its identity in the TLS ClientHello
//! `server_name`: `<label>.<domain>`, where the label is the identity's
//! `session_id` under the host key. The host peeks that one field without
//! consuming a byte, decrypts the label, and pushes the untouched socket to
//! that identity's lungs connection. TLS ends in the container.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use op_network::browser_label::{self, LabelKey};
use tokio::sync::Semaphore;

use crate::runtime_lungs::BrowserHandoff;

const MAX_IN_FLIGHT: usize = 128;
const CLIENT_HELLO_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct BrowserIngress {
    key: Arc<LabelKey>,
    domain: Arc<str>,
    lungs: BrowserHandoff,
}

impl BrowserIngress {
    pub(crate) fn from_env(lungs: BrowserHandoff) -> Result<Self> {
        let key_path = std::env::var_os("OP_BROWSER_LABEL_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(browser_label::DEFAULT_LABEL_KEY));
        let domain = std::env::var("OP_BROWSER_UI_DOMAIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| browser_label::DEFAULT_UI_DOMAIN.to_owned());
        Ok(Self {
            key: Arc::new(LabelKey::from_root_file(&key_path)?),
            domain: domain.trim().to_ascii_lowercase().into(),
            lungs,
        })
    }

    /// The shared raw browser entry, when this host owns it.
    pub(crate) fn listen_addr_from_env() -> Result<Option<std::net::SocketAddr>> {
        match std::env::var("OP_BROWSER_INGRESS_LISTEN") {
            Ok(value) if !value.trim().is_empty() => Ok(Some(
                value.trim().parse().context("OP_BROWSER_INGRESS_LISTEN is not host:port")?,
            )),
            _ => Ok(None),
        }
    }

    /// Own the shared `:8080` socket and push every connection to its identity.
    pub(crate) async fn serve(self, listener: tokio::net::TcpListener) -> Result<()> {
        let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
        loop {
            let (stream, _) = listener.accept().await.context("browser ingress accept failed")?;
            let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let ingress = self.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) = ingress.deliver(stream).await {
                    tracing::debug!(%error, "browser connection closed");
                }
            });
        }
    }

    async fn deliver(&self, stream: tokio::net::TcpStream) -> Result<()> {
        let server_name = op_network::tls_sni::peek_server_name(&stream, CLIENT_HELLO_DEADLINE).await?;
        let session_id = self.session_id(&server_name).context("server_name names no identity")?;
        self.lungs.push(&session_id, stream).await
    }

    fn session_id(&self, server_name: &str) -> Option<String> {
        let label = browser_label::label_of(server_name, &self.domain)?;
        self.key.session_id(label)
    }
}
