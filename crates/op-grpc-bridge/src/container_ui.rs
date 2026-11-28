//! Optional container-owned browser assets. They are served on the container's
//! own UI door (`container_ui_door.rs`) when `OP_UI_LISTEN` enables it, else on
//! the identity door. No host HTTP listener, desktop, identity endpoint or
//! credential injection.
use std::path::{Path, PathBuf};
use _3tched_fs::mounted::MountedNamespace;
use _3tched_fs::namespace::MountIdentity;
use anyhow::{Context, Result, bail, ensure};
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

pub(crate) fn mount_path() -> Result<PathBuf> {
    Ok(PathBuf::from(setting("CHAT_UI_MOUNT")?
        .unwrap_or_else(|| "/run/mounts/3tchedFS".to_owned())))
}

/// This container's session, from its own identity volume.
const OWN_SESSION: &str = "/agent/user/session_id";

/// Open this container's own view. The container is the identity, so the view
/// is used as given, in either form a container receives: an Incus guest mounts
/// it in-kernel (`guest_kernel`); the host projects an LXD subsidiary's view
/// bound to that container's own session (`host_session`). A view bound to any
/// other session, a credential or nothing is not this container's.
pub(crate) fn open_own_mount(mount: &Path) -> Result<MountedNamespace> {
    let declaration: serde_json::Value = serde_json::from_slice(
        &std::fs::read(mount.join(".3tchedfs.json")).context("read the container view declaration")?,
    )?;
    let expected = match declaration["mount_identity"]["kind"].as_str() {
        Some("guest_kernel") => MountIdentity::GuestKernel,
        Some("host_session") => MountIdentity::HostSession {
            session: std::fs::read_to_string(OWN_SESSION)
                .context("this container's session is unavailable")?
                .trim()
                .to_owned(),
        },
        _ => bail!("the mount is not this container's own view"),
    };
    MountedNamespace::open_for_mode(mount, &expected)
}

/// Assets on the identity door, unless the container's UI door owns them.
pub(crate) fn mount(app: axum::Router) -> Result<axum::Router> {
    if crate::container_ui_door::enabled()? { return Ok(app); }
    assets(app)
}

pub(crate) fn assets(app: axum::Router) -> Result<axum::Router> {
    let Some(directory) = setting("CHAT_UI_DIR")? else { return Ok(app); };
    let root = PathBuf::from(directory);
    ensure!(root.is_absolute() && root.join("index.html").is_file() && root.join("assets").is_dir(),
        "configured container browser payload is absent");
    let mount = mount_path()?;
    // The container is the identity (owner, 2026-11-25): calls from this UI run
    // through the container's own mount under its grants, on the UI door only.
    // The identity door still validates every credential and declared grant.
    let namespace = open_own_mount(&mount)?;
    namespace.verify()?;
    Ok(app
        .route_service("/", ServeFile::new(root.join("index.html")))
        .route_service("/chat", ServeFile::new(root.join("index.html")))
        .nest_service("/assets", ServeDir::new(root.join("assets"))))
}
