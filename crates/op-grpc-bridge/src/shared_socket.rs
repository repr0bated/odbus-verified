//! One listener owner for the shared tonic UDS. Plugin calls control this
//! owner; only the Incoming stream accepts connections and passes them to
//! tonic's unchanged authenticated route stack.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, ensure, Context, Result};
use futures::task::AtomicWaker;
use nix::fcntl::{Flock, FlockArg};
use op_plugins::state_plugins::unix_socket::{
    AcceptInput, AcceptOutput, AcceptedConnection, BindInput, CloseInput, ListenInput,
    UnixSocketState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use socket2::{Domain, SockAddr, SockRef, Socket, Type};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tokio_stream::Stream;
use tonic::transport::server::{Connected, UdsConnectInfo};

pub const DEFAULT_SOCKET_PATH: &str = "/run/ghostbridge/container.sock";
const EVENT_CAPACITY: usize = 256;

#[cfg(test)]
#[path = "shared_socket_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SocketOwnership {
    version: u32,
    path: PathBuf,
    device: u64,
    inode: u64,
    uid: u32,
    ctime: i64,
    ctime_nsec: i64,
}

impl SocketOwnership {
    fn read(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.file_type().is_socket(),
            "refusing a non-socket or symlink at {}",
            path.display()
        );
        ensure!(
            metadata.uid() == nix::unistd::geteuid().as_raw(),
            "socket is not owned by the bridge service"
        );
        Ok(Self {
            version: 1,
            path: path.into(),
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        })
    }

    fn remove_matching(&self) -> Result<()> {
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        ensure!(
            Self::read(&self.path)? == *self,
            "socket ownership changed; refusing to unlink {}",
            self.path.display()
        );
        fs::remove_file(&self.path)?;
        Ok(())
    }
}

struct ListenerLease {
    file: Flock<File>,
    recorded: Option<SocketOwnership>,
}

impl ListenerLease {
    fn acquire(path: &Path, directory: &Path) -> Result<Self> {
        ensure_private_directory(directory)?;
        let hash = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
        let lease_path = directory.join(format!("{hash}.lease"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&lease_path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.uid() == nix::unistd::geteuid().as_raw()
                && metadata.mode() & 0o077 == 0
                && metadata.len() <= 4096,
            "listener lease must be a private service-owned regular file"
        );
        let mut file =
            Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, error)| {
                anyhow::anyhow!("listener ownership lease is already held or unavailable: {error}")
            })?;
        let mut bytes = Vec::new();
        (&mut *file).take(4097).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 4096,
            "listener ownership lease exceeds limit"
        );
        let recorded: Option<SocketOwnership> = if bytes.is_empty() {
            None
        } else {
            let record: SocketOwnership = serde_json::from_slice(&bytes)
                .context("invalid listener ownership proof; operator cleanup required")?;
            ensure!(
                record.version == 1
                    && record.path == path
                    && record.uid == nix::unistd::geteuid().as_raw(),
                "listener lease does not prove ownership of this path"
            );
            Some(record)
        };
        Ok(Self { file, recorded })
    }

    fn record(&mut self, ownership: SocketOwnership) -> Result<()> {
        let bytes = serde_json::to_vec(&ownership)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.set_len(0)?;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        self.recorded = Some(ownership);
        Ok(())
    }

    fn recover_stale(&self, path: &Path) -> Result<()> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let recorded = self.recorded.as_ref().context("existing socket has no bridge ownership proof; refusing cleanup, operator action required")?;
        ensure!(
            SocketOwnership::read(path)? == *recorded,
            "existing socket does not match the previous bridge lease; refusing cleanup"
        );
        let probe = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        probe.set_nonblocking(true)?;
        match probe.connect(&SockAddr::unix(path)?) {
            Ok(()) => bail!("previously recorded socket has a live listener; refusing cleanup"),
            Err(error) if error.raw_os_error() == Some(nix::libc::ECONNREFUSED) => {
                recorded.remove_matching()
            }
            Err(error) => {
                bail!("cannot prove recorded socket is stale ({error}); refusing cleanup")
            }
        }
    }
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == nix::unistd::geteuid().as_raw()
            && metadata.mode() & 0o077 == 0,
        "socket lease directory must be private and owned by the bridge service"
    );
    Ok(())
}

struct ListenerState {
    listener: Option<UnixListener>,
    listening: bool,
    ownership: Option<SocketOwnership>,
    lease: ListenerLease,
    backlog: Option<u32>,
    binding_generation: u64,
    sequence: u64,
    events: VecDeque<AcceptedConnection>,
    // Duplicated descriptors permit real SHUT_RDWR without taking the stream
    // away from tonic. Dropping a ManagedUnixStream releases its entry.
    connections: HashMap<String, Socket>,
}

pub struct SharedListenerController {
    path: PathBuf,
    generation: String,
    state: Mutex<ListenerState>,
    incoming_taken: AtomicBool,
    incoming_waker: AtomicWaker,
    accepted: Notify,
}

impl SharedListenerController {
    pub fn open(path: PathBuf) -> Result<Arc<Self>> {
        let directory = std::env::var_os("OPDBUS_SOCKET_LEASE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/run/opdbus/socket-leases".into());
        Self::open_with_lease_dir(path, &directory)
    }

    pub fn open_with_lease_dir(path: PathBuf, lease_directory: &Path) -> Result<Arc<Self>> {
        ensure!(
            path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
                && path.file_name().is_some(),
            "configured socket path must be absolute and normalized"
        );
        let lease = ListenerLease::acquire(&path, lease_directory)?;
        Ok(Arc::new(Self {
            path,
            generation: uuid::Uuid::new_v4().to_string(),
            state: Mutex::new(ListenerState {
                listener: None,
                listening: false,
                ownership: None,
                lease,
                backlog: None,
                binding_generation: 0,
                sequence: 0,
                events: VecDeque::new(),
                connections: HashMap::new(),
            }),
            incoming_taken: AtomicBool::new(false),
            incoming_waker: AtomicWaker::new(),
            accepted: Notify::new(),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<MutexGuard<'_, ListenerState>> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("listener controller lock poisoned"))
    }

    fn exact_path(&self, path: &str) -> Result<()> {
        ensure!(
            path == self
                .path
                .to_str()
                .context("configured socket path is not UTF-8")?,
            "method path must exactly equal the configured shared listener path"
        );
        Ok(())
    }

    fn snapshot_locked(&self, state: &ListenerState) -> UnixSocketState {
        UnixSocketState {
            path: self.path.to_string_lossy().into_owned(),
            protocol: "grpc".into(),
            phase: if state.listening {
                "listening"
            } else if state.listener.is_some() {
                "bound"
            } else {
                "closed"
            }
            .into(),
            generation: self.generation.clone(),
            binding_generation: state.binding_generation,
            requested_backlog: state.backlog,
            active_connections: state.connections.len() as u64,
            accepted_connections: state.sequence,
        }
    }

    pub fn snapshot(&self) -> Result<UnixSocketState> {
        let state = self.lock()?;
        Ok(self.snapshot_locked(&state))
    }

    fn bind(&self) -> Result<UnixSocketState> {
        let mut state = self.lock()?;
        if state.listener.is_some() {
            return Ok(self.snapshot_locked(&state));
        }
        let next_generation = state
            .binding_generation
            .checked_add(1)
            .context("listener binding generation exhausted")?;
        let parent = self.path.parent().context("socket has no parent")?;
        fs::create_dir_all(parent)?;
        let metadata = fs::symlink_metadata(parent)?;
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == nix::unistd::geteuid().as_raw()
                && metadata.mode() & 0o022 == 0,
            "socket parent must be service-owned and not writable by other users"
        );
        state.lease.recover_stale(&self.path)?;
        let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        socket.set_nonblocking(true)?;
        socket.bind(&SockAddr::unix(&self.path)?)?;
        let mut ownership = SocketOwnership::read(&self.path).context(
            "socket bound but ownership could not be established; refusing unproven cleanup",
        )?;
        let listener_result = (|| -> Result<UnixListener> {
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o666))?;
            ownership = SocketOwnership::read(&self.path)?;
            state.lease.record(ownership.clone())?;
            Ok(UnixListener::from_std(
                std::os::unix::net::UnixListener::from(socket),
            )?)
        })();
        let listener = match listener_result {
            Ok(listener) => listener,
            Err(error) => {
                if let Err(cleanup) = ownership.remove_matching() {
                    return Err(error).context(format!(
                        "socket setup failed and owned cleanup was refused: {cleanup}"
                    ));
                }
                return Err(error)
                    .context("socket setup failed; newly bound owned pathname was removed");
            }
        };
        state.binding_generation = next_generation;
        state.listener = Some(listener);
        state.ownership = Some(ownership);
        state.listening = false;
        state.backlog = None;
        let result = self.snapshot_locked(&state);
        drop(state);
        self.incoming_waker.wake();
        Ok(result)
    }

    fn listen(&self, backlog: u32) -> Result<UnixSocketState> {
        ensure!((1..=65535).contains(&backlog), "backlog must be 1..=65535");
        let mut state = self.lock()?;
        let listener = state
            .listener
            .as_ref()
            .context("listener is closed; call bind before listen")?;
        SockRef::from(listener).listen(backlog as i32)?;
        state.listening = true;
        state.backlog = Some(backlog);
        let result = self.snapshot_locked(&state);
        drop(state);
        self.incoming_waker.wake();
        Ok(result)
    }

    pub fn start(&self, backlog: u32) -> Result<UnixSocketState> {
        self.bind()?;
        self.listen(backlog)
    }

    fn close(&self, force: bool) -> Result<UnixSocketState> {
        let mut state = self.lock()?;
        ensure!(
            force || state.connections.is_empty(),
            "listener has active connections; force is required to shut them down"
        );
        let mut shutdown_error = None;
        if force {
            state
                .connections
                .retain(|_, socket| match socket.shutdown(Shutdown::Both) {
                    Ok(()) => false,
                    Err(error) if error.kind() == io::ErrorKind::NotConnected => false,
                    Err(error) => {
                        shutdown_error.get_or_insert(error);
                        true
                    }
                });
        }
        drop(state.listener.take());
        state.listening = false;
        state.backlog = None;
        if let Some(ownership) = &state.ownership {
            ownership.remove_matching()?;
        }
        state.ownership = None;
        let result = self.snapshot_locked(&state);
        drop(state);
        self.incoming_waker.wake();
        if let Some(error) = shutdown_error {
            return Err(error).context("listener closed but an active connection shutdown failed");
        }
        Ok(result)
    }

    pub fn take_incoming(self: &Arc<Self>) -> Result<ControlledIncoming> {
        ensure!(
            !self.incoming_taken.swap(true, Ordering::AcqRel),
            "listener already has its sole tonic acceptor"
        );
        Ok(ControlledIncoming {
            owner: self.clone(),
        })
    }

    fn observation(&self, cursor: u64, timed_out: bool) -> Result<AcceptOutput> {
        let state = self.lock()?;
        ensure!(
            cursor <= state.sequence,
            "accept cursor is ahead of this listener"
        );
        if let Some(oldest) = state.events.front() {
            ensure!(
                cursor >= oldest.sequence - 1,
                "accept cursor expired from the bounded connection event history"
            );
        }
        let event = state
            .events
            .iter()
            .find(|event| event.sequence > cursor)
            .cloned();
        Ok(AcceptOutput {
            cursor: event.as_ref().map_or(cursor, |event| event.sequence),
            observed_generation: self.generation.clone(),
            timed_out: timed_out && event.is_none(),
            event,
        })
    }

    async fn observe(&self, input: AcceptInput) -> Result<AcceptOutput> {
        self.exact_path(&input.path)?;
        if let Some(generation) = input.generation {
            ensure!(
                generation == self.generation,
                "accept generation changed; obtain a new listener cursor"
            );
        }
        let seconds = input.timeout_secs.unwrap_or(0);
        ensure!(seconds <= 60, "accept timeout must be 0..=60 seconds");
        let cursor = input.after_sequence.unwrap_or(self.lock()?.sequence);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        loop {
            let notified = self.accepted.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let output = self.observation(cursor, false)?;
            if output.event.is_some() || seconds == 0 {
                return Ok(output);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.observation(cursor, true);
            }
        }
    }

    pub async fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "bind" => {
                let input: BindInput = serde_json::from_value(args.clone())?;
                self.exact_path(&input.path)?;
                ensure!(input.protocol == "grpc", "shared listener protocol is grpc");
                Ok(serde_json::to_value(self.bind()?)?)
            }
            "listen" => {
                let input: ListenInput = serde_json::from_value(args.clone())?;
                self.exact_path(&input.path)?;
                Ok(serde_json::to_value(self.listen(input.backlog)?)?)
            }
            "accept" => Ok(serde_json::to_value(
                self.observe(serde_json::from_value(args.clone())?).await?,
            )?),
            "close" => {
                let input: CloseInput = serde_json::from_value(args.clone())?;
                self.exact_path(&input.path)?;
                Ok(serde_json::to_value(self.close(input.force)?)?)
            }
            _ => bail!("unknown unix_socket method: {method}"),
        }
    }
}

impl Drop for SharedListenerController {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for socket in state.connections.values() {
                let _ = socket.shutdown(Shutdown::Both);
            }
            drop(state.listener.take());
            if let Some(ownership) = &state.ownership {
                if let Err(error) = ownership.remove_matching() {
                    tracing::warn!(%error, "owned listener cleanup refused");
                }
            }
        }
    }
}

pub struct ControlledIncoming {
    owner: Arc<SharedListenerController>,
}

impl Stream for ControlledIncoming {
    type Item = io::Result<ManagedUnixStream>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.owner.incoming_waker.register(cx.waker());
        let mut state = match self.owner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return Poll::Ready(Some(Err(io::Error::other(
                    "listener controller lock poisoned",
                ))))
            }
        };
        if !state.listening {
            return Poll::Pending;
        }
        let Some(listener) = &state.listener else {
            return Poll::Pending;
        };
        let stream = match listener.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok((stream, _))) => stream,
        };
        let shutdown = match SockRef::from(&stream).try_clone() {
            Ok(socket) => socket,
            Err(error) => return Poll::Ready(Some(Err(error))),
        };
        let Some(sequence) = state.sequence.checked_add(1) else {
            return Poll::Ready(Some(Err(io::Error::other(
                "listener event sequence exhausted",
            ))));
        };
        state.sequence = sequence;
        let connection_id = uuid::Uuid::new_v4().to_string();
        let credentials = stream.peer_cred().ok();
        let event = AcceptedConnection {
            sequence,
            connection_id: connection_id.clone(),
            binding_generation: state.binding_generation,
            accepted_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            peer_uid: credentials.as_ref().map(|cred| cred.uid()),
            peer_gid: credentials.as_ref().map(|cred| cred.gid()),
            peer_pid: credentials.and_then(|cred| cred.pid()),
        };
        state.connections.insert(connection_id.clone(), shutdown);
        state.events.push_back(event);
        if state.events.len() > EVENT_CAPACITY {
            state.events.pop_front();
        }
        drop(state);
        self.owner.accepted.notify_waiters();
        Poll::Ready(Some(Ok(ManagedUnixStream {
            stream,
            connection_id,
            owner: Arc::downgrade(&self.owner),
        })))
    }
}

pub struct ManagedUnixStream {
    stream: UnixStream,
    connection_id: String,
    owner: Weak<SharedListenerController>,
}
impl Connected for ManagedUnixStream {
    type ConnectInfo = UdsConnectInfo;
    fn connect_info(&self) -> Self::ConnectInfo {
        self.stream.connect_info()
    }
}
impl AsyncRead for ManagedUnixStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buffer)
    }
}
impl AsyncWrite for ManagedUnixStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buffer)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
impl Drop for ManagedUnixStream {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            if let Ok(mut state) = owner.state.lock() {
                state.connections.remove(&self.connection_id);
            }
        }
    }
}

/// Tonic preserves these kernel credentials through ManagedUnixStream's
/// Connected implementation. They are not substituted for SID1/OIA1 admission.
pub fn extract_peer_cred<T>(request: &tonic::Request<T>) -> Option<tokio::net::unix::UCred> {
    request
        .extensions()
        .get::<UdsConnectInfo>()
        .and_then(|info| info.peer_cred)
}
