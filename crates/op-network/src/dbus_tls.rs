//! Private native D-Bus transport between the admitted bridge and a decoy.
//! Both peers authenticate with a dedicated certificate chain before D-Bus
//! starts. D-Bus ANONYMOUS here performs framing negotiation only; TLS is the
//! node authentication layer. This module never accepts plaintext, uses system
//! CA roots, mints a user identity, or authorizes a plugin method.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
};
use std::{
    io,
    net::SocketAddr,
    os::fd::{BorrowedFd, OwnedFd},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};
use zbus::connection::{
    AuthMechanism,
    socket::{ReadHalf, Socket, Split, WriteHalf},
};

fn roots(certificates: Vec<CertificateDer<'static>>) -> Result<RootCertStore> {
    ensure!(
        !certificates.is_empty(),
        "decoy transport requires explicit trust roots"
    );
    let mut roots = RootCertStore::empty();
    for cert in certificates {
        roots.add(cert).context("invalid decoy trust certificate")?;
    }
    Ok(roots)
}

pub fn client_config(
    trust: Vec<CertificateDer<'static>>,
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ClientConfig>> {
    ensure!(
        !chain.is_empty(),
        "decoy transport requires a bridge client certificate"
    );
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots(trust)?)
        .with_client_auth_cert(chain, key)?;
    config.alpn_protocols = vec![b"opdbus-native-v1".to_vec()];
    Ok(Arc::new(config))
}

pub fn server_config(
    trust: Vec<CertificateDer<'static>>,
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ServerConfig>> {
    let verifier =
        rustls::server::WebPkiClientVerifier::builder(Arc::new(roots(trust)?)).build()?;
    let mut config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)?;
    config.alpn_protocols = vec![b"opdbus-native-v1".to_vec()];
    Ok(Arc::new(config))
}

pub struct TlsDbusSocket(TlsStream<TcpStream>);
pub struct TlsRead(tokio::io::ReadHalf<TlsStream<TcpStream>>);
pub struct TlsWrite(tokio::io::WriteHalf<TlsStream<TcpStream>>);
impl std::fmt::Debug for TlsRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TlsDbusRead")
    }
}
impl std::fmt::Debug for TlsWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TlsDbusWrite")
    }
}

impl TlsDbusSocket {
    pub async fn connect(
        address: SocketAddr,
        server_name: String,
        config: Arc<ClientConfig>,
    ) -> Result<Self> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let stream = TcpStream::connect(address).await?;
            stream.set_nodelay(true)?;
            let stream = TlsConnector::from(config)
                .connect(ServerName::try_from(server_name)?, stream)
                .await?;
            ensure!(
                stream.get_ref().1.alpn_protocol() == Some(b"opdbus-native-v1"),
                "decoy TLS protocol mismatch"
            );
            Ok(Self(stream.into()))
        })
        .await
        .context("decoy mTLS connection timeout")?
    }

    /// `config` must come from server_config: anonymous TLS clients are never
    /// accepted. Recheck peer-chain presence even with an injected config.
    pub async fn accept(stream: TcpStream, config: Arc<ServerConfig>) -> Result<Self> {
        tokio::time::timeout(Duration::from_secs(10), async {
            stream.set_nodelay(true)?;
            let stream = TlsAcceptor::from(config).accept(stream).await?;
            ensure!(
                stream
                    .get_ref()
                    .1
                    .peer_certificates()
                    .is_some_and(|chain| !chain.is_empty()),
                "decoy TLS client identity is required"
            );
            ensure!(
                stream.get_ref().1.alpn_protocol() == Some(b"opdbus-native-v1"),
                "decoy TLS protocol mismatch"
            );
            Ok(Self(stream.into()))
        })
        .await
        .context("decoy mTLS accept timeout")?
    }

    /// Authenticated peer leaf certificate DER. Callers may apply a stricter
    /// application pin before registering any D-Bus object on this socket.
    pub fn peer_leaf_certificate(&self) -> Result<&[u8]> {
        self.0
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first())
            .map(|certificate| certificate.as_ref())
            .context("decoy TLS peer leaf certificate is unavailable")
    }
}

impl Socket for TlsDbusSocket {
    type ReadHalf = TlsRead;
    type WriteHalf = TlsWrite;
    fn split(self) -> Split<Self::ReadHalf, Self::WriteHalf> {
        let (read, write) = tokio::io::split(self.0);
        Split::new(TlsRead(read), TlsWrite(write))
    }
}

#[async_trait]
impl ReadHalf for TlsRead {
    async fn recvmsg(&mut self, buffer: &mut [u8]) -> io::Result<(usize, Vec<OwnedFd>)> {
        Ok((self.0.read(buffer).await?, vec![]))
    }
    fn auth_mechanism(&self) -> AuthMechanism {
        AuthMechanism::Anonymous
    }
}

#[async_trait]
impl WriteHalf for TlsWrite {
    async fn sendmsg(&mut self, buffer: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<usize> {
        if !fds.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native TLS transport cannot pass file descriptors",
            ));
        }
        let written = self.0.write(buffer).await?;
        self.0.flush().await?;
        Ok(written)
    }
    async fn close(&mut self) -> io::Result<()> {
        self.0.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use tokio::net::TcpListener;

    fn configs() -> (Arc<ClientConfig>, Arc<ServerConfig>) {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![]).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = params.self_signed(&key).unwrap();
        let server_key = KeyPair::generate().unwrap();
        let server_cert = CertificateParams::new(vec!["decoy.fixture".into()])
            .unwrap()
            .signed_by(&server_key, &ca, &key)
            .unwrap();
        let client_key = KeyPair::generate().unwrap();
        let client_cert = CertificateParams::new(vec!["bridge.fixture".into()])
            .unwrap()
            .signed_by(&client_key, &ca, &key)
            .unwrap();
        let client = client_config(
            vec![ca.der().clone()],
            vec![client_cert.der().clone()],
            PrivateKeyDer::Pkcs8(client_key.serialize_der().into()),
        )
        .unwrap();
        let server = server_config(
            vec![ca.der().clone()],
            vec![server_cert.der().clone()],
            PrivateKeyDer::Pkcs8(server_key.serialize_der().into()),
        )
        .unwrap();
        (client, server)
    }

    struct NativeFixture;
    #[zbus::interface(name = "org.opdbus.NativeFixture")]
    impl NativeFixture {
        fn verify_metadata(&self, credential: Vec<u8>) -> Vec<u8> {
            credential
        }
    }

    #[tokio::test]
    async fn actual_dbus_over_mutual_tls_preserves_opaque_metadata() {
        let (client, server) = configs();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let socket = TlsDbusSocket::accept(tcp, server).await.unwrap();
            zbus::connection::Builder::socket(socket)
                .server(zbus::Guid::generate())
                .unwrap()
                .p2p()
                .auth_mechanism(AuthMechanism::Anonymous)
                .serve_at("/org/opdbus/fixture", NativeFixture)
                .unwrap()
                .build()
                .await
                .unwrap()
        });
        let socket = TlsDbusSocket::connect(address, "decoy.fixture".into(), client)
            .await
            .unwrap();
        let connection = zbus::connection::Builder::socket(socket)
            .p2p()
            .auth_mechanism(AuthMechanism::Anonymous)
            .build()
            .await
            .unwrap();
        let _server = server_task.await.unwrap();
        let proxy = zbus::Proxy::new(
            &connection,
            "org.opdbus.NativeFixture",
            "/org/opdbus/fixture",
            "org.opdbus.NativeFixture",
        )
        .await
        .unwrap();
        let credential = b"exact-authored-credential-fixture".to_vec();
        let received: Vec<u8> = proxy
            .call("VerifyMetadata", &(credential.clone(),))
            .await
            .unwrap();
        assert_eq!(received, credential);
    }

    #[tokio::test]
    async fn wrong_server_name_and_untrusted_client_are_rejected() {
        let (client, server) = configs();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            TlsDbusSocket::accept(stream, server).await
        });
        assert!(
            TlsDbusSocket::connect(address, "wrong.fixture".into(), client)
                .await
                .is_err()
        );
        assert!(task.await.unwrap().is_err());
        let (foreign_client, _) = configs();
        let (trusted_server_client, server) = configs();
        let mut bad_client = (*trusted_server_client).clone();
        bad_client.client_auth_cert_resolver = foreign_client.client_auth_cert_resolver.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            TlsDbusSocket::accept(stream, server).await
        });
        let _ = TlsDbusSocket::connect(address, "decoy.fixture".into(), Arc::new(bad_client)).await;
        assert!(task.await.unwrap().is_err());
    }
}
