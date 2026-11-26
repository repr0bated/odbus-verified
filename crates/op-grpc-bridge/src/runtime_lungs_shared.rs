//! Import the chatbot's socket-activated listener without forwarding connections.
//! Accepting the original listener on the host preserves SO_PEERCRED/SO_PEERPIDFD.
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::incus::{IncusPlugin, IncusRootRepairBackend};
use tokio::io::unix::AsyncFd;

use crate::runtime::Runtime;
use super::identity::ProcessIdentity;

pub(super) struct ImportedListener {
    pub listener: tokio::net::UnixListener,
    pub creator: AsyncFd<OwnedFd>,
}

fn process_fd(pid: u32) -> Result<OwnedFd> {
    // SAFETY: scalar PID and flags. A successful pidfd_open returns an owned fd.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("pin chatbot init lifetime");
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

fn duplicate_fd(process: &OwnedFd, fd: i32) -> Result<Option<OwnedFd>> {
    // SAFETY: held pidfd, scalar descriptor and zero flags. The returned fd is new.
    let imported = unsafe { libc::syscall(libc::SYS_pidfd_getfd, process.as_raw_fd(), fd, 0) };
    if imported < 0 {
        let error = std::io::Error::last_os_error();
        // PID 1 can close an unrelated fd between directory enumeration and import.
        if error.raw_os_error() == Some(libc::EBADF) {
            return Ok(None);
        }
        return Err(error).context("import chatbot listener descriptor");
    }
    Ok(Some(unsafe { OwnedFd::from_raw_fd(imported as i32) }))
}

fn socket_option(fd: &OwnedFd, option: i32) -> Result<i32> {
    let mut value: i32 = 0;
    let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
    // SAFETY: held socket and correctly sized integer output.
    let result = unsafe {
        libc::getsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, option,
            (&mut value as *mut i32).cast(), &mut length)
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("inspect chatbot socket descriptor");
    }
    ensure!(length as usize == std::mem::size_of_val(&value), "invalid socket option size");
    Ok(value)
}

fn listener_for_path(fd: OwnedFd, path: &Path) -> Result<Option<StdUnixListener>> {
    if socket_option(&fd, libc::SO_DOMAIN)? != libc::AF_UNIX
        || socket_option(&fd, libc::SO_TYPE)? != libc::SOCK_STREAM
        || socket_option(&fd, libc::SO_ACCEPTCONN)? != 1
    {
        return Ok(None);
    }
    let listener = StdUnixListener::from(fd);
    if listener.local_addr()?.as_pathname() != Some(path) {
        return Ok(None);
    }
    Ok(Some(listener))
}

fn import(pid: u32, path: &Path) -> Result<ImportedListener> {
    let process = process_fd(pid)?;
    let init = ProcessIdentity::read(pid)?;
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd"))?;
    let mut selected = None;
    for entry in entries {
        let entry = entry?;
        let Some(fd) = entry.file_name().to_str().and_then(|value| value.parse::<i32>().ok()) else {
            continue;
        };
        let target = match std::fs::read_link(entry.path()) {
            Ok(target) => target,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !target.as_os_str().as_encoded_bytes().starts_with(b"socket:[") {
            continue;
        }
        let Some(fd) = duplicate_fd(&process, fd)? else { continue; };
        if let Some(listener) = listener_for_path(fd, path)? {
            ensure!(selected.is_none(), "multiple chatbot listeners claim the shared path");
            selected = Some(listener);
        }
    }
    init.revalidate()?;
    let listener = selected.context("chatbot has not activated the shared lungs socket")?;
    listener.set_nonblocking(true)?;
    Ok(ImportedListener {
        listener: tokio::net::UnixListener::from_std(listener)?,
        creator: AsyncFd::new(process)?,
    })
}

pub(super) async fn acquire(runtime: &Arc<Runtime>, path: &Path) -> Result<ImportedListener> {
    ensure!(path == Path::new(super::DEFAULT_LUNGS_SOCKET), "shared lungs requires the declared public path");
    let session = crate::identity_binding::control_plane_chatbot_session_id()
        .context("configured chatbot session absent")?;
    let store = runtime.identity_store().clone();
    let revision = tokio::task::spawn_blocking(move || store.read_identity_revision(&session)).await??;
    let sled = crate::runtime_identity::record_to_sled(
        revision.record.as_ref().context("chatbot identity absent")?, revision.genesis.as_ref())?;
    ensure!(sled.active && sled.is_anchored()
        && crate::identity_binding::configured_chatbot_key().as_deref() == Some(sled.wireguard_pubkey.as_str()),
        "shared listener requires the active configured chatbot identity");
    let bound = sled.instance.as_ref().context("chatbot native binding absent")?;
    let native = IncusRootRepairBackend::local();
    let (metadata, _) = native.instance(&bound.name).await?;
    let current = IncusPlugin::instance_from_native_metadata(metadata)?;
    ensure!(current.status == "Running" && crate::identity_binding::identity_instance_matches(&sled, &current),
        "chatbot native identity changed");
    let uuid = |instance: &op_plugins::state_plugins::incus::IncusInstance| {
        instance.config.as_ref().and_then(|config| config.get("volatile.uuid")).cloned()
    };
    ensure!(uuid(bound).is_some() && uuid(bound) == uuid(&current), "chatbot native UUID changed");
    let pid = native.instance_init_pid(&bound.name).await?;
    let path = path.to_owned();
    let imported = tokio::task::spawn_blocking(move || import(pid, &path)).await??;
    ensure!(native.instance_init_pid(&bound.name).await? == pid, "chatbot restarted during listener import");
    // Readability is the pidfd's exit notification, including an exited zombie.
    ensure!(tokio::time::timeout(std::time::Duration::ZERO, imported.creator.readable()).await.is_err(),
        "chatbot listener owner exited");
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_listening_unix_socket_can_be_imported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lungs.sock");
        let listener = StdUnixListener::bind(&path).unwrap();
        let imported = listener_for_path(listener.into(), &path).unwrap().unwrap();
        assert_eq!(imported.local_addr().unwrap().as_pathname(), Some(path.as_path()));
        let other = directory.path().join("other.sock");
        assert!(listener_for_path(imported.into(), &other).unwrap().is_none());
    }

    #[test]
    fn a_connected_socket_cannot_replace_the_listener() {
        let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(listener_for_path(stream.into(), Path::new(super::super::DEFAULT_LUNGS_SOCKET))
            .unwrap().is_none());
    }
}
