//! Optional container-owned browser assets on the existing TLS door. No host
//! HTTP listener, desktop, identity endpoint, credential injection or RPC proxy.
use std::path::PathBuf;
use anyhow::{Context, Result, ensure};
use tower_http::services::{ServeDir, ServeFile};

pub(crate) fn setting(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => match std::fs::read_to_string(format!("/etc/op-dbus/env/{name}")) {
            Ok(value) => Ok(Some(value.trim().to_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("container UI setting unavailable"),
        },
        Err(error) => Err(error).context("container UI setting invalid"),
    }
}

pub(crate) fn mount(app: axum::Router) -> Result<axum::Router> {
    let Some(directory) = setting("CHAT_UI_DIR")? else { return Ok(app); };
    let root = PathBuf::from(directory);
    ensure!(root.is_absolute() && root.join("index.html").is_file() && root.join("assets").is_dir(),
        "configured container browser payload is absent");
    let mount = PathBuf::from(setting("CHAT_UI_MOUNT")?
        .unwrap_or_else(|| "/run/mounts/3tchedFS".to_owned()));
    // Public application bytes do not establish browser identity. The existing
    // generic door still validates every human credential and declared grant.
    // A host-session/credential/unbound mount is never a container substitute.
    let namespace = _3tched_fs::mounted::MountedNamespace::open_for_identity_mode(
        &mount, _3tched_fs::mounted::IdentityMode::GuestKernel, None)?;
    namespace.verify()?;
    Ok(app
        .route_service("/", ServeFile::new(root.join("index.html")))
        .route_service("/chat", ServeFile::new(root.join("index.html")))
        .nest_service("/assets", ServeDir::new(root.join("assets"))))
}
