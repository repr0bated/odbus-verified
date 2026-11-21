//! TLS-only listener: TLS works, plaintext gets no HTTP response, and the
//! server refuses to start without a usable, private key (Requirement 1).

use axum::routing::get;
use op_mcp_door::server::{load_tls, serve};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn cert_files(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = dir.join("door.crt");
    let key = dir.join("door.key");
    let cert_pem = generated.cert.pem();
    std::fs::write(&cert, &cert_pem).unwrap();
    std::fs::write(&key, generated.key_pair.serialize_pem()).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    (cert, key, cert_pem)
}

async fn start(dir: &std::path::Path) -> (std::net::SocketAddr, String) {
    let (cert, key, pem) = cert_files(dir);
    let tls = load_tls(&cert, &key).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().route(
        "/mcp/full",
        get(|| async { (axum::http::StatusCode::METHOD_NOT_ALLOWED, "") }),
    );
    tokio::spawn(serve(listener, tls, app, std::future::pending()));
    (addr, pem)
}

#[tokio::test]
async fn plaintext_http_gets_no_http_response() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, _) = start(dir.path()).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"POST /mcp/full HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")
        .await
        .unwrap();
    let mut response = Vec::new();
    let read =
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response)).await;
    assert!(read.is_ok(), "the server closes the connection");
    assert!(
        !response.starts_with(b"HTTP/"),
        "no plaintext HTTP response: {:?}",
        String::from_utf8_lossy(&response)
    );
    assert!(
        !String::from_utf8_lossy(&response).contains("mcp"),
        "no application data leaks"
    );
}

#[tokio::test]
async fn tls_clients_are_served() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, pem) = start(dir.path()).await;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut tls = connector.connect(name, tcp).await.unwrap();
    tls.write_all(b"GET /mcp/full HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tls.read_to_end(&mut response)).await;
    assert!(
        response.starts_with(b"HTTP/1.1 405"),
        "{}",
        String::from_utf8_lossy(&response)
    );
}

#[test]
fn startup_refuses_missing_unreadable_public_or_garbage_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, key, _) = cert_files(dir.path());
    assert!(load_tls(&cert, &key).is_ok());
    assert!(load_tls(&cert, &dir.path().join("absent.key")).is_err());
    assert!(load_tls(&dir.path().join("absent.crt"), &key).is_err());
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
    let error = load_tls(&cert, &key).unwrap_err().to_string();
    assert!(error.contains("group or other"), "{error}");
    assert!(
        !error.contains("PRIVATE KEY"),
        "errors never carry key material"
    );
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&cert, "garbage").unwrap();
    assert!(load_tls(&cert, &key).is_err());
    let (cert, key, _) = cert_files(dir.path());
    std::fs::write(&key, "").unwrap();
    assert!(load_tls(&cert, &key).is_err());
}
