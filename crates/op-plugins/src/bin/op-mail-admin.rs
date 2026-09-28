//! Private Postfix/Dovecot administration D-Bus peer for `mail-3tched`.

use anyhow::{Context, Result, ensure};
use nix::fcntl::{Flock, FlockArg};
use op_plugins::state_plugins::mail_server::{methods, native_owner::MailAdminService};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use tokio::net::UnixListener;

const MAX_CLIENTS: usize = 32;

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == ["--check-runtime"] {
        println!(
            "{{\"service\":\"op-mail-admin\",\"runtime\":\"{}\",\"status\":\"ok\"}}",
            if cfg!(target_env = "gnu") {
                "gnu"
            } else if cfg!(target_env = "musl") {
                "musl"
            } else {
                "unknown"
            }
        );
        return Ok(());
    }
    ensure!(
        arguments.is_empty(),
        "usage: op-mail-admin [--check-runtime]"
    );

    let socket = std::env::var_os("OP_MAIL_ADMIN_LISTEN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| methods::DEFAULT_CONTAINER_SOCKET.into());
    let parent = socket
        .parent()
        .context("mail administration socket has no parent")?;
    std::fs::create_dir_all(parent)?;
    let lock_path = parent.join(".mail-admin.lock");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)?;
    let _lease = Flock::lock(lock_file, FlockArg::LockExclusiveNonblock)
        .map_err(|(_, error)| error)
        .context("another mail administration owner holds the socket lease")?;
    if let Ok(metadata) = std::fs::symlink_metadata(&socket) {
        ensure!(
            metadata.file_type().is_socket(),
            "refusing to replace non-socket {}",
            socket.display()
        );
        ensure!(
            metadata.uid() == nix::unistd::geteuid().as_raw(),
            "refusing to replace a mail administration socket owned by another uid"
        );
        match std::os::unix::net::UnixStream::connect(&socket) {
            Ok(_) => anyhow::bail!("mail administration socket is already active"),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) => {}
            Err(error) => return Err(error).context("inspect existing mail administration socket"),
        }
        std::fs::remove_file(&socket)
            .with_context(|| format!("remove stale socket {}", socket.display()))?;
    }
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("bind mail administration socket {}", socket.display()))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;

    let service = MailAdminService::production()?;
    service
        .adopt_or_reconcile()
        .await
        .context("adopt or reconcile Postfix/Dovecot administration state")?;

    let clients = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CLIENTS));
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = clients.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let service = service.clone();
        let _task = tokio::spawn(async move {
            let _permit = permit;
            let connection = zbus::connection::Builder::unix_stream(stream)
                .server(zbus::Guid::generate())?
                .p2p()
                .serve_at(methods::OBJECT_PATH, service)?
                .build()
                .await?;
            connection.closed().await;
            Ok::<(), zbus::Error>(())
        });
    }
}
