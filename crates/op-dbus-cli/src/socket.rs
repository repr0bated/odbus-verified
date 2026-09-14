//! Pre-call socket check — verifies the gRPC socket is connectable
//! immediately before dispatching a method call. Fixes the socket
//! permissions if needed (chmod 777) and returns an actionable error
//! if the bridge isn't listening.

use anyhow::{bail, Result};
use std::os::unix::net::UnixStream;
use std::path::Path;

pub const GRPC_SOCK: &str = "/run/opdbus/grpc.sock";

/// Ensure the gRPC socket is connectable. Call this before every gRPC dispatch.
/// If permissions are wrong (not 777), attempts to fix them.
pub fn ensure_socket_ready() -> Result<()> {
    let p = Path::new(GRPC_SOCK);

    if !p.exists() {
        bail!(
            "gRPC socket missing: {}\nIs op-grpc-bridge running? (sv status op-grpc-bridge)",
            GRPC_SOCK
        );
    }

    // Check and fix permissions
    let meta = std::fs::metadata(p)?;
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o777 {
        // Try to fix
        match std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o777)) {
            Ok(_) => tracing::info!("Fixed socket permissions: {} -> 777", GRPC_SOCK),
            Err(_) => {
                bail!(
                    "Socket permissions {:o} (need 777): sudo chmod 777 {}",
                    mode,
                    GRPC_SOCK
                );
            }
        }
    }

    // Verify connection
    match UnixStream::connect(p) {
        Ok(_) => Ok(()),
        Err(e) => bail!(
            "Socket exists but connect failed: {}\nRestart: sudo sv restart op-grpc-bridge",
            e
        ),
    }
}
