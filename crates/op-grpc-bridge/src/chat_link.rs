//! Authenticated ChatLink on the existing TLS door, with no additional listener.
//!
//! `IDENTITY_CHAT_WORKERS_FILE` is an absolute, root-owned, non-group/world-writable
//! JSON array of {principal_id, session_id, genesis, executable, args, profile_path,
//! provider_policy_path}. All path ancestors and the three referenced files must
//! also be protected; symlinks are refused. Arguments are server-owned Node argv,
//! never a shell command. An absent setting leaves only ChatLink unavailable; an
//! invalid configured file or startup handshake fails production binding closed.
//!
//! Each principal has exactly one child for this server lifetime. Only NODE_ENV,
//! IDENTITY_CHAT_PROFILE and IDENTITY_CHAT_PROVIDER_POLICY enter its environment.
//! stdin/stdout carry bounded NDJSON; stderr is discarded, not logged or disclosed.
//! Ready identity must match configuration, and every request's identity must match
//! both. Requests contain only a server UUID, verified caller triple and parsed
//! payload, never credential metadata. A child exit/protocol failure is unavailable
//! with unknown outcome: there is no automatic restart or request replay.
//!
//! Eight in-flight exchanges per worker and bounded queues limit memory. A full
//! response queue backpressures stdout while the command/cancellation path stays
//! responsive. Dropping an RPC sends cancel; every disclosed chunk rechecks current
//! caller authority and the exact sealed identity_sled.get_own_identity capability.

use std::{
    collections::{BTreeMap, HashMap},
    borrow::Cow,
    fs::File,
    io::Read,
    os::{fd::{AsRawFd, FromRawFd}, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore},
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    proto::{chat_link_server::ChatLink, ChatLinkChunk, ChatLinkRequest},
    runtime::VerifiedCaller,
    service::BridgeService,
};

pub(crate) const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_CHUNK_BYTES: usize = 16 * 1024 * 1024;
// Bytes-field tag and varint length are outside the JSON size contract.
pub(crate) const MAX_REQUEST_PROTO_BYTES: usize = MAX_REQUEST_BYTES + 5;
pub(crate) const MAX_CHUNK_PROTO_BYTES: usize = MAX_CHUNK_BYTES + 5;
const MAX_WORKER_FRAME_BYTES: usize = MAX_CHUNK_BYTES + 4096;
const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_WORKERS: usize = 64;
const MAX_IN_FLIGHT: usize = 8;
const CHUNK_QUEUE: usize = 2;
const WORKER_VERSION: &str = "identity-chat-worker/1";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct ChatLinkService {
    bridge: BridgeService,
    workers: Arc<BTreeMap<String, WorkerHandle>>,
}

impl ChatLinkService {
    pub(crate) async fn from_env(bridge: BridgeService) -> anyhow::Result<Self> {
        let configs = match std::env::var_os("IDENTITY_CHAT_WORKERS_FILE") {
            None => Vec::new(),
            Some(path) => tokio::task::spawn_blocking(move || load_config(Path::new(&path)))
                .await.map_err(|_| anyhow::anyhow!("chat worker configuration task failed"))??,
        };
        let mut workers = BTreeMap::new();
        for config in configs {
            let principal = config.principal_id.clone();
            workers.insert(principal, start_worker(config).await?);
        }
        Ok(Self { bridge, workers: Arc::new(workers) })
    }
}

#[tonic::async_trait]
impl ChatLink for ChatLinkService {
    type ExchangeStream = Pin<Box<dyn Stream<Item = Result<ChatLinkChunk, Status>> + Send>>;

    async fn exchange(&self, request: Request<ChatLinkRequest>) -> Result<Response<Self::ExchangeStream>, Status> {
        // This service never takes the fixture services' unauthenticated path.
        let caller = self.bridge.authenticate(&request).await?;
        self.bridge.authorize_chat(&caller).await?;
        let payload = parse_request(&request.get_ref().json)?;
        let identity = WorkerIdentity::from_caller(&caller);
        let worker = self.workers.get(&identity.principal_id)
            .ok_or_else(|| Status::unavailable("chat worker is not configured"))?;
        let mut response = worker.request(&identity, &payload)?;
        let bridge = self.bridge.clone();
        let stream = async_stream::try_stream! {
            while let Some(json) = response.next().await? {
                // Revalidate after queueing, immediately at the disclosure boundary.
                bridge.authorize_chat(&caller).await?;
                yield ChatLinkChunk { json };
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}

fn parse_request(bytes: &[u8]) -> Result<Box<RawValue>, Status> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Status::resource_exhausted("chat request exceeds 1 MiB"));
    }
    serde_json::from_slice(bytes).map_err(|_| Status::invalid_argument("chat request must be valid JSON"))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerIdentity {
    principal_id: String,
    session_id: String,
    genesis: String,
}

impl WorkerIdentity {
    fn from_caller(caller: &VerifiedCaller) -> Self {
        Self {
            principal_id: caller.principal_id.clone(),
            session_id: caller.session_id.clone(),
            genesis: caller.session_genesis.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerConfig {
    principal_id: String,
    session_id: String,
    genesis: String,
    executable: PathBuf,
    args: Vec<String>,
    profile_path: PathBuf,
    provider_policy_path: PathBuf,
}

impl WorkerConfig {
    fn identity(&self) -> WorkerIdentity {
        WorkerIdentity {
            principal_id: self.principal_id.clone(),
            session_id: self.session_id.clone(),
            genesis: self.genesis.clone(),
        }
    }
}

fn load_config(path: &Path) -> anyhow::Result<Vec<WorkerConfig>> {
    let mut file = protected_file(path)?;
    anyhow::ensure!(file.metadata()?.len() <= MAX_CONFIG_BYTES as u64, "chat worker configuration is oversized");
    let mut bytes = Vec::new();
    file.by_ref().take(MAX_CONFIG_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= MAX_CONFIG_BYTES, "chat worker configuration is oversized");
    let configs: Vec<WorkerConfig> = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid chat worker configuration"))?;
    validate_configs(&configs)?;
    for config in &configs {
        let executable = protected_file(&config.executable)?;
        anyhow::ensure!(executable.metadata()?.mode() & 0o111 != 0, "chat worker executable is not executable");
        protected_file(&config.profile_path)?;
        protected_file(&config.provider_policy_path)?;
    }
    Ok(configs)
}

fn validate_configs(configs: &[WorkerConfig]) -> anyhow::Result<()> {
    anyhow::ensure!(configs.len() <= MAX_WORKERS, "too many configured chat workers");
    let mut principals = std::collections::HashSet::new();
    for config in configs {
        anyhow::ensure!(principals.insert(&config.principal_id), "duplicate chat worker principal");
        for value in [&config.principal_id, &config.session_id, &config.genesis] {
            anyhow::ensure!(!value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control),
                "invalid chat worker identity binding");
        }
        anyhow::ensure!(config.executable.is_absolute() && config.profile_path.is_absolute()
            && config.provider_policy_path.is_absolute(), "chat worker paths must be absolute");
    }
    Ok(())
}

/// Walk held descriptors, not check-then-reopen pathnames. With every ancestor
/// root-controlled, only the server's trusted administrator can replace a path.
fn protected_file(path: &Path) -> anyhow::Result<File> {
    anyhow::ensure!(path.is_absolute(), "chat worker path must be absolute");
    let mut directory = File::open("/")?;
    let root = directory.metadata()?;
    anyhow::ensure!(root.uid() == 0 && root.mode() & 0o022 == 0,
        "chat worker root directory is not protected");
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => anyhow::bail!("chat worker path is not canonical"),
        };
        let final_component = components.peek().is_none();
        let name = std::ffi::CString::new(name.as_encoded_bytes())?;
        let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK
            | if final_component { 0 } else { libc::O_DIRECTORY };
        // SAFETY: both the held directory descriptor and NUL-terminated name are
        // valid for this call. A successful fd is immediately owned by File.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        anyhow::ensure!(fd >= 0, "chat worker path is absent or unsafe");
        // SAFETY: openat returned a fresh, nonnegative descriptor.
        let opened = unsafe { File::from_raw_fd(fd) };
        let metadata = opened.metadata()?;
        anyhow::ensure!(metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "chat worker path is not protected and root-owned");
        if final_component {
            anyhow::ensure!(metadata.is_file(), "chat worker path is not a regular file");
            return Ok(opened);
        }
        anyhow::ensure!(metadata.is_dir(), "chat worker ancestor is not a directory");
        directory = opened;
    }
    anyhow::bail!("chat worker path is not a file")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReadyFrame {
    r#type: String,
    version: String,
    principal_id: String,
    session_id: String,
    genesis: String,
}

async fn accept_ready<R: AsyncBufRead + Unpin>(reader: &mut FrameReader<R>, expected: &WorkerIdentity)
    -> Result<(), WorkerFailure> {
    let bytes = reader.next(4096).await?.ok_or(WorkerFailure::Unavailable)?;
    let ready: ReadyFrame = serde_json::from_slice(&bytes).map_err(|_| WorkerFailure::Unavailable)?;
    if ready.r#type != "ready" || ready.version != WORKER_VERSION
        || ready.principal_id != expected.principal_id || ready.session_id != expected.session_id
        || ready.genesis != expected.genesis {
        return Err(WorkerFailure::Unavailable);
    }
    Ok(())
}

async fn start_worker(config: WorkerConfig) -> anyhow::Result<WorkerHandle> {
    let identity = config.identity();
    let mut child = worker_command(&config).spawn()
        .map_err(|_| anyhow::anyhow!("chat worker could not start"))?;
    let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("chat worker input is unavailable"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("chat worker output is unavailable"))?;
    let mut reader = FrameReader::new(BufReader::new(stdout));
    if !matches!(tokio::time::timeout(HANDSHAKE_TIMEOUT, accept_ready(&mut reader, &identity)).await, Ok(Ok(()))) {
        let _ = child.kill().await;
        let _ = child.wait().await;
        anyhow::bail!("chat worker handshake failed");
    }
    Ok(spawn_transport(identity, reader, stdin, Some(child)))
}

fn worker_command(config: &WorkerConfig) -> Command {
    let mut command = Command::new(&config.executable);
    command.args(&config.args)
        .current_dir("/")
        .env_clear()
        .env("NODE_ENV", "production")
        .env("IDENTITY_CHAT_PROFILE", &config.profile_path)
        .env("IDENTITY_CHAT_PROVIDER_POLICY", &config.provider_policy_path)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

/// The buffer belongs to the reader, not to the pending future: selecting a
/// command while a partial line is arriving must not discard the partial frame.
struct FrameReader<R> {
    reader: R,
    partial: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> FrameReader<R> {
    fn new(reader: R) -> Self { Self { reader, partial: Vec::new() } }

    async fn next(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, WorkerFailure> {
        loop {
            let available = self.reader.fill_buf().await.map_err(|_| WorkerFailure::Unavailable)?;
            if available.is_empty() {
                return if self.partial.is_empty() { Ok(None) } else { Err(WorkerFailure::Unavailable) };
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let length = newline.unwrap_or(available.len());
            // Check before extending: neither an unterminated nor a terminated
            // oversized line may cause an unbounded read_line allocation.
            if length > maximum.saturating_sub(self.partial.len()) {
                return Err(WorkerFailure::Unavailable);
            }
            self.partial.extend_from_slice(&available[..length]);
            self.reader.consume(length + usize::from(newline.is_some()));
            if newline.is_some() { return Ok(Some(std::mem::take(&mut self.partial))); }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkerFailure {
    InvalidRequest,
    PermissionDenied,
    ResourceExhausted,
    Unavailable,
    Cancelled,
    Internal,
}

impl WorkerFailure {
    fn status(self) -> Status {
        match self {
            Self::InvalidRequest => Status::invalid_argument("chat request was rejected"),
            Self::PermissionDenied => Status::permission_denied("chat request is not permitted"),
            Self::ResourceExhausted => Status::resource_exhausted("chat worker capacity exceeded"),
            Self::Unavailable => Status::unavailable("chat worker unavailable; request outcome unknown"),
            Self::Cancelled => Status::cancelled("chat request cancelled"),
            Self::Internal => Status::internal("chat worker failed"),
        }
    }
}

// A struct, rather than an internally tagged enum, keeps RawValue borrowing from
// serde_json's original input instead of an intermediate serde content buffer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerFrame<'a> {
    #[serde(borrow)]
    r#type: Cow<'a, str>,
    id: Uuid,
    #[serde(borrow)]
    chunk: Option<&'a RawValue>,
    code: Option<WorkerFailure>,
    #[serde(borrow)]
    message: Option<Cow<'a, str>>,
}

#[derive(Serialize)]
struct RequestFrame<'a> {
    r#type: &'static str,
    id: Uuid,
    caller: &'a WorkerIdentity,
    payload: &'a RawValue,
}

#[derive(Serialize)]
struct CancelFrame {
    r#type: &'static str,
    id: Uuid,
}

#[derive(Clone)]
struct WorkerHandle {
    identity: WorkerIdentity,
    commands: mpsc::Sender<WorkerCommand>,
    capacity: Arc<Semaphore>,
}

struct RequestSlot {
    chunks: mpsc::Sender<Vec<u8>>,
    terminal: oneshot::Sender<Result<(), WorkerFailure>>,
    _permit: Arc<OwnedSemaphorePermit>,
}

enum WorkerCommand {
    Start { id: Uuid, frame: Vec<u8>, slot: RequestSlot },
    Cancel(Uuid),
}

impl WorkerHandle {
    fn request(&self, caller: &WorkerIdentity, payload: &RawValue) -> Result<WorkerResponse, Status> {
        if caller != &self.identity {
            return Err(Status::permission_denied("chat worker identity does not match caller"));
        }
        if self.commands.is_closed() { return Err(WorkerFailure::Unavailable.status()); }
        let permit = Arc::new(self.capacity.clone().try_acquire_owned()
            .map_err(|_| WorkerFailure::ResourceExhausted.status())?);
        let id = Uuid::new_v4();
        let frame = serde_json::to_vec(&RequestFrame { r#type: "request", id, caller, payload })
            .map_err(|_| WorkerFailure::Internal.status())?;
        let (chunks, receiver) = mpsc::channel(CHUNK_QUEUE);
        let (terminal, outcome) = oneshot::channel();
        let slot = RequestSlot { chunks, terminal, _permit: permit.clone() };
        self.commands.try_send(WorkerCommand::Start { id, frame, slot }).map_err(|error| match error {
            mpsc::error::TrySendError::Closed(_) => WorkerFailure::Unavailable.status(),
            mpsc::error::TrySendError::Full(_) => WorkerFailure::ResourceExhausted.status(),
        })?;
        Ok(WorkerResponse {
            id, chunks: receiver, terminal: Some(outcome), commands: self.commands.clone(),
            permit, finished: false,
        })
    }
}

struct WorkerResponse {
    id: Uuid,
    chunks: mpsc::Receiver<Vec<u8>>,
    terminal: Option<oneshot::Receiver<Result<(), WorkerFailure>>>,
    commands: mpsc::Sender<WorkerCommand>,
    permit: Arc<OwnedSemaphorePermit>,
    finished: bool,
}

impl WorkerResponse {
    async fn next(&mut self) -> Result<Option<Vec<u8>>, Status> {
        loop {
            if self.finished { return Ok(None); }
            let Some(terminal) = self.terminal.as_mut() else {
                let chunk = self.chunks.recv().await;
                if chunk.is_none() { self.finished = true; }
                return Ok(chunk);
            };
            tokio::select! {
                // A failure discards any still-buffered data; a clean end drains it.
                biased;
                result = terminal => {
                    self.terminal = None;
                    match result {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => { self.finished = true; return Err(error.status()); },
                        Err(_) => { self.finished = true; return Err(WorkerFailure::Unavailable.status()); },
                    }
                }
                chunk = self.chunks.recv() => {
                    if chunk.is_some() { return Ok(chunk); }
                }
            }
        }
    }
}

impl Drop for WorkerResponse {
    fn drop(&mut self) {
        if self.finished { return; }
        let commands = self.commands.clone();
        let id = self.id;
        // Keeping admission until cancellation is queued bounds Drop tasks too.
        let permit = self.permit.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = commands.send(WorkerCommand::Cancel(id)).await;
        });
    }
}

struct WorkerDriver<R> {
    reader: FrameReader<R>,
    commands: mpsc::Receiver<WorkerCommand>,
    writes: mpsc::Sender<Vec<u8>>,
    requests: HashMap<Uuid, RequestSlot>,
    pending: Option<(Uuid, Vec<u8>)>,
}

impl<R: AsyncBufRead + Unpin> WorkerDriver<R> {
    async fn run(&mut self) -> Result<(), WorkerFailure> {
        loop {
            if let Some((id, _)) = self.pending.as_ref() {
                let id = *id;
                let Some(slot) = self.requests.get(&id) else {
                    self.pending = None;
                    continue;
                };
                let chunks = slot.chunks.clone();
                tokio::select! {
                    command = self.commands.recv() => {
                        if !self.command(command)? { return Ok(()); }
                    }
                    capacity = chunks.reserve() => {
                        match capacity {
                            Ok(capacity) => {
                                if let Some((_, chunk)) = self.pending.take() { capacity.send(chunk); }
                            }
                            Err(_) => {
                                self.pending = None;
                                self.cancel(id, WorkerFailure::Cancelled)?;
                            }
                        }
                    }
                }
                continue;
            }
            tokio::select! {
                command = self.commands.recv() => {
                    if !self.command(command)? { return Ok(()); }
                }
                frame = self.reader.next(MAX_WORKER_FRAME_BYTES) => {
                    let frame = frame?.ok_or(WorkerFailure::Unavailable)?;
                    self.deliver(&frame)?;
                }
            }
        }
    }

    fn command(&mut self, command: Option<WorkerCommand>) -> Result<bool, WorkerFailure> {
        match command {
            None => return Ok(false),
            Some(WorkerCommand::Start { id, frame, slot }) => {
                self.requests.insert(id, slot);
                self.writes.try_send(frame).map_err(|_| WorkerFailure::Unavailable)?;
            }
            Some(WorkerCommand::Cancel(id)) => self.cancel(id, WorkerFailure::Cancelled)?,
        }
        Ok(true)
    }

    fn cancel(&mut self, id: Uuid, reason: WorkerFailure) -> Result<(), WorkerFailure> {
        if let Some(slot) = self.requests.remove(&id) {
            let _ = slot.terminal.send(Err(reason));
            let frame = serde_json::to_vec(&CancelFrame { r#type: "cancel", id })
                .map_err(|_| WorkerFailure::Internal)?;
            self.writes.try_send(frame).map_err(|_| WorkerFailure::Unavailable)?;
        }
        Ok(())
    }

    fn deliver(&mut self, bytes: &[u8]) -> Result<(), WorkerFailure> {
        let frame: WorkerFrame<'_> = serde_json::from_slice(bytes).map_err(|_| WorkerFailure::Unavailable)?;
        let id = frame.id;
        match (frame.r#type.as_ref(), frame.chunk, frame.code, frame.message) {
            ("chunk", Some(chunk), None, None) => {
                if !chunk.get().starts_with('{') || chunk.get().len() > MAX_CHUNK_BYTES {
                    return Err(WorkerFailure::Unavailable);
                }
                #[derive(Deserialize)]
                struct ChunkShape<'a> { #[serde(borrow)] r#type: Cow<'a, str> }
                let shape: ChunkShape<'_> = serde_json::from_str(chunk.get())
                    .map_err(|_| WorkerFailure::Unavailable)?;
                if shape.r#type.is_empty() {
                    return Err(WorkerFailure::Unavailable);
                }
                // Frames for finished/cancelled/unknown IDs are never disclosed.
                let Some(slot) = self.requests.get(&id) else { return Ok(()); };
                match slot.chunks.try_send(chunk.get().as_bytes().to_vec()) {
                    Ok(()) => {},
                    Err(mpsc::error::TrySendError::Closed(_)) => self.cancel(id, WorkerFailure::Cancelled)?,
                    Err(mpsc::error::TrySendError::Full(chunk)) => self.pending = Some((id, chunk)),
                }
            }
            ("end", None, None, None) => {
                if let Some(slot) = self.requests.remove(&id) { let _ = slot.terminal.send(Ok(())); }
            }
            ("error", None, Some(code), Some(_)) => {
                // The worker's arbitrary message is deliberately never used.
                if let Some(slot) = self.requests.remove(&id) { let _ = slot.terminal.send(Err(code)); }
            }
            _ => return Err(WorkerFailure::Unavailable),
        }
        Ok(())
    }
}

fn spawn_transport<R, W>(identity: WorkerIdentity, reader: FrameReader<R>, mut stdin: W, mut child: Option<Child>)
    -> WorkerHandle
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (commands, receiver) = mpsc::channel(MAX_IN_FLIGHT * 2);
    let (writes, mut writer_queue) = mpsc::channel::<Vec<u8>>(MAX_IN_FLIGHT * 2);
    tokio::spawn(async move {
        // Reading stdout and writing stdin must remain independent: each pipe can
        // be full while the other peer needs to drain it before making progress.
        let mut writer = tokio::spawn(async move {
            while let Some(frame) = writer_queue.recv().await {
                stdin.write_all(&frame).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            }
            Ok::<(), std::io::Error>(())
        });
        let mut driver = WorkerDriver {
            reader, commands: receiver, writes, requests: HashMap::new(), pending: None,
        };
        let writer_completed = if let Some(process) = child.as_mut() {
            tokio::select! {
                _ = process.wait() => false,
                _ = &mut writer => true,
                _ = driver.run() => false,
            }
        } else {
            tokio::select! { _ = &mut writer => true, _ = driver.run() => false }
        };
        // Dropping queued/active slots closes their outcome channels. Pending
        // consumers receive a fixed unavailable/unknown status, never a replay.
        drop(driver);
        if !writer_completed {
            writer.abort();
            let _ = writer.await;
        }
        if let Some(process) = child.as_mut() {
            let _ = process.kill().await;
            let _ = process.wait().await;
        }
    });
    WorkerHandle { identity, commands, capacity: Arc::new(Semaphore::new(MAX_IN_FLIGHT)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, DuplexStream};

    #[tokio::test]
    async fn chat_link_has_no_unauthenticated_fixture_bypass() {
        let core = crate::BridgeCore::new(crate::Catalog::from_entries(Vec::new()).unwrap());
        let service = ChatLinkService {
            bridge: BridgeService::new(core), workers: Arc::new(BTreeMap::new()),
        };
        let result = service.exchange(Request::new(ChatLinkRequest { json: b"{}".to_vec() })).await;
        assert_eq!(result.err().unwrap().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn command_passes_only_explicit_server_environment_and_literal_arguments() {
        let config = WorkerConfig {
            principal_id: "alice".into(), session_id: "session-a".into(), genesis: "genesis-a".into(),
            executable: "/protected/node".into(),
            args: vec!["/protected/worker.mjs".into(), "$(not-a-shell)".into()],
            profile_path: "/protected/profile.json".into(),
            provider_policy_path: "/protected/policy.json".into(),
        };
        let command = worker_command(&config);
        let command = command.as_std();
        let environment: BTreeMap<_, _> = command.get_envs().map(|(key, value)| (key.to_str().unwrap(), value.unwrap().to_str().unwrap())).collect();
        assert_eq!(environment, BTreeMap::from([
            ("IDENTITY_CHAT_PROFILE", "/protected/profile.json"),
            ("IDENTITY_CHAT_PROVIDER_POLICY", "/protected/policy.json"),
            ("NODE_ENV", "production"),
        ]));
        assert_eq!(command.get_current_dir(), Some(Path::new("/")));
        assert_eq!(command.get_args().last().unwrap(), "$(not-a-shell)");
    }

    #[test]
    fn chunk_limit_is_exact_and_only_json_objects_are_accepted() {
        let (_commands, receiver) = mpsc::channel(1);
        let (writes, _writer_queue) = mpsc::channel(1);
        let mut driver = WorkerDriver {
            reader: FrameReader::new(BufReader::new(b"".as_slice())),
            commands: receiver, writes, requests: HashMap::new(), pending: None,
        };
        let id = Uuid::new_v4();
        let empty_chunk = r#"{"type":"text-delta","delta":""}"#;
        let value = "x".repeat(MAX_CHUNK_BYTES - empty_chunk.len());
        let mut chunk = json!({"type":"text-delta", "delta":value});
        assert_eq!(serde_json::to_vec(&chunk).unwrap().len(), MAX_CHUNK_BYTES);
        let bytes = serde_json::to_vec(&json!({"type":"chunk", "id":id, "chunk":chunk})).unwrap();
        assert!(driver.deliver(&bytes).is_ok());
        chunk["delta"] = json!("x".repeat(MAX_CHUNK_BYTES - empty_chunk.len() + 1));
        let bytes = serde_json::to_vec(&json!({"type":"chunk", "id":id, "chunk":chunk})).unwrap();
        assert!(driver.deliver(&bytes).is_err());
        let bytes = serde_json::to_vec(&json!({"type":"chunk", "id":id, "chunk":["text-delta"]})).unwrap();
        assert!(driver.deliver(&bytes).is_err());
    }

    fn identity() -> WorkerIdentity {
        WorkerIdentity { principal_id: "alice".into(), session_id: "session-a".into(), genesis: "genesis-a".into() }
    }

    fn synthetic_worker() -> (WorkerHandle, DuplexStream) {
        let (bridge, worker) = tokio::io::duplex(64 * 1024);
        let (reader, writer) = tokio::io::split(bridge);
        (spawn_transport(identity(), FrameReader::new(BufReader::new(reader)), writer, None), worker)
    }

    async fn read_json<R: AsyncBufRead + Unpin>(reader: &mut FrameReader<R>) -> Value {
        let frame = tokio::time::timeout(Duration::from_secs(2), reader.next(MAX_WORKER_FRAME_BYTES))
            .await.unwrap().unwrap().unwrap();
        serde_json::from_slice(&frame).unwrap()
    }

    async fn send_json<W: AsyncWrite + Unpin>(writer: &mut W, value: Value) {
        writer.write_all(&serde_json::to_vec(&value).unwrap()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();
    }

    async fn next(response: &mut WorkerResponse) -> Result<Option<Vec<u8>>, Status> {
        tokio::time::timeout(Duration::from_secs(2), response.next()).await.unwrap()
    }

    #[test]
    fn request_json_is_validated_and_bounded() {
        for bytes in [b"".as_slice(), b"{", b"null trailing", &[0xff]] {
            assert_eq!(parse_request(bytes).unwrap_err().code(), tonic::Code::InvalidArgument);
        }
        assert_eq!(parse_request(&vec![b' '; MAX_REQUEST_BYTES + 1]).unwrap_err().code(), tonic::Code::ResourceExhausted);
        assert_eq!(parse_request(br#"{"messages":[]}"#).unwrap().get(), r#"{"messages":[]}"#);
        let exact = format!("\"{}\"", "x".repeat(MAX_REQUEST_BYTES - 2));
        assert_eq!(parse_request(exact.as_bytes()).unwrap().get().len(), MAX_REQUEST_BYTES);
    }

    #[tokio::test]
    async fn ready_rejects_each_identity_mismatch_and_protocol_version() {
        for field in ["principalId", "sessionId", "genesis", "version", "type"] {
            let mut frame = json!({"type":"ready", "version":WORKER_VERSION,
                "principalId":"alice", "sessionId":"session-a", "genesis":"genesis-a"});
            frame[field] = json!("wrong");
            let mut bytes = serde_json::to_vec(&frame).unwrap(); bytes.push(b'\n');
            let mut reader = FrameReader::new(BufReader::new(bytes.as_slice()));
            assert!(accept_ready(&mut reader, &identity()).await.is_err(), "{field}");
        }
        let bytes = format!("{{\"type\":\"ready\",\"version\":\"{WORKER_VERSION}\",\"principalId\":\"alice\",\"sessionId\":\"session-a\",\"genesis\":\"genesis-a\"}}\n");
        assert!(accept_ready(&mut FrameReader::new(BufReader::new(bytes.as_bytes())), &identity()).await.is_ok());
    }

    #[tokio::test]
    async fn line_reader_rejects_oversize_and_truncated_frames_before_growth() {
        for bytes in [b"123456789\n".as_slice(), b"123456789", b"abc"] {
            let mut reader = FrameReader::new(BufReader::new(bytes));
            assert!(reader.next(8).await.is_err());
            assert!(reader.partial.len() <= 8);
        }
        let mut reader = FrameReader::new(BufReader::new(b"12345678\n".as_slice()));
        assert_eq!(reader.next(8).await.unwrap().unwrap(), b"12345678");
    }

    #[tokio::test]
    async fn partial_line_survives_future_cancellation() {
        let (mut writer, reader) = tokio::io::duplex(32);
        let mut reader = FrameReader::new(BufReader::new(reader));
        writer.write_all(b"{\"type\":").await.unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(10), reader.next(128)).await.is_err());
        writer.write_all(b"\"end\"}\n").await.unwrap();
        assert_eq!(reader.next(128).await.unwrap().unwrap(), br#"{"type":"end"}"#);
    }

    #[tokio::test]
    async fn caller_triple_mismatch_never_reaches_worker() {
        let (handle, mut worker) = synthetic_worker();
        let payload = parse_request(b"{}").unwrap();
        for field in 0..3 {
            let mut caller = identity();
            match field { 0 => caller.principal_id.push('x'), 1 => caller.session_id.push('x'), _ => caller.genesis.push('x') }
            assert_eq!(handle.request(&caller, &payload).err().unwrap().code(), tonic::Code::PermissionDenied);
        }
        let mut byte = [0];
        assert!(tokio::time::timeout(Duration::from_millis(10), worker.read(&mut byte)).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_ids_are_isolated_and_drop_sends_cancel() {
        let (handle, worker) = synthetic_worker();
        let (reader, mut writer) = tokio::io::split(worker);
        let mut reader = FrameReader::new(BufReader::new(reader));
        let payload = parse_request(br#"{"messages":[],"caller":{"principalId":"mallory"}}"#).unwrap();
        let first = handle.request(&identity(), &payload).unwrap();
        let mut second = handle.request(&identity(), &payload).unwrap();
        let first_request = read_json(&mut reader).await;
        let second_request = read_json(&mut reader).await;
        assert_ne!(first_request["id"], second_request["id"]);
        assert_eq!(first_request["caller"], serde_json::to_value(identity()).unwrap());
        assert_eq!(first_request["payload"]["caller"]["principalId"], "mallory");
        assert_eq!(first_request.as_object().unwrap().len(), 4);
        assert!(Uuid::parse_str(first_request["id"].as_str().unwrap()).is_ok());
        drop(first);
        let cancellation = read_json(&mut reader).await;
        assert_eq!(cancellation, json!({"type":"cancel", "id":first_request["id"]}));
        send_json(&mut writer, json!({"type":"chunk", "id":first_request["id"], "chunk":{"type":"text-delta", "delta":"private-first"}})).await;
        send_json(&mut writer, json!({"type":"chunk", "id":second_request["id"], "chunk":{"type":"text-delta", "delta":"second"}})).await;
        send_json(&mut writer, json!({"type":"end", "id":second_request["id"]})).await;
        let chunk: Value = serde_json::from_slice(&next(&mut second).await.unwrap().unwrap()).unwrap();
        assert_eq!(chunk["delta"], "second");
        assert!(next(&mut second).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn worker_crash_is_unavailable_without_restart_or_replay() {
        let (handle, worker) = synthetic_worker();
        let mut reader = FrameReader::new(BufReader::new(worker));
        let payload = parse_request(b"{}").unwrap();
        let mut response = handle.request(&identity(), &payload).unwrap();
        assert_eq!(read_json(&mut reader).await["type"], "request");
        drop(reader);
        assert_eq!(next(&mut response).await.unwrap_err().code(), tonic::Code::Unavailable);
        assert_eq!(handle.request(&identity(), &payload).err().unwrap().code(), tonic::Code::Unavailable);
    }

    #[tokio::test]
    async fn malformed_worker_json_and_chunk_shape_close_transport() {
        for frame in [b"not-json\n".as_slice(), b"{\"type\":\"ready\"}\n", b"{\"type\":\"chunk\",\"id\":\"bad-id\",\"chunk\":{}}\n"] {
            let (handle, worker) = synthetic_worker();
            let (reader, mut writer) = tokio::io::split(worker);
            let mut reader = FrameReader::new(BufReader::new(reader));
            let mut response = handle.request(&identity(), &parse_request(b"{}").unwrap()).unwrap();
            read_json(&mut reader).await;
            writer.write_all(frame).await.unwrap();
            assert_eq!(next(&mut response).await.unwrap_err().code(), tonic::Code::Unavailable);
        }
        for chunk in [json!("not an object"), json!({"delta":"no type"}), json!({"type":""})] {
            let (handle, worker) = synthetic_worker();
            let (reader, mut writer) = tokio::io::split(worker);
            let mut reader = FrameReader::new(BufReader::new(reader));
            let mut response = handle.request(&identity(), &parse_request(b"{}").unwrap()).unwrap();
            let request = read_json(&mut reader).await;
            send_json(&mut writer, json!({"type":"chunk", "id":request["id"], "chunk":chunk})).await;
            assert_eq!(next(&mut response).await.unwrap_err().code(), tonic::Code::Unavailable);
        }
    }

    #[tokio::test]
    async fn arbitrary_worker_error_text_is_not_disclosed() {
        let (handle, worker) = synthetic_worker();
        let (reader, mut writer) = tokio::io::split(worker);
        let mut reader = FrameReader::new(BufReader::new(reader));
        let mut response = handle.request(&identity(), &parse_request(b"{}").unwrap()).unwrap();
        let request = read_json(&mut reader).await;
        send_json(&mut writer, json!({"type":"error", "id":request["id"], "code":"internal", "message":"secret-token-and-provider-body"})).await;
        let error = next(&mut response).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert_eq!(error.message(), "chat worker failed");
    }

    #[tokio::test]
    async fn admission_is_bounded_and_backpressure_remains_cancellable() {
        let (handle, worker) = synthetic_worker();
        let (reader, mut writer) = tokio::io::split(worker);
        let mut reader = FrameReader::new(BufReader::new(reader));
        let payload = parse_request(b"{}").unwrap();
        let mut responses = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..MAX_IN_FLIGHT {
            responses.push(handle.request(&identity(), &payload).unwrap());
            ids.push(read_json(&mut reader).await["id"].clone());
        }
        assert_eq!(handle.request(&identity(), &payload).err().unwrap().code(), tonic::Code::ResourceExhausted);
        for _ in 0..=CHUNK_QUEUE {
            send_json(&mut writer, json!({"type":"chunk", "id":ids[0], "chunk":{"type":"text-delta", "delta":"slow"}})).await;
        }
        // The full response remains live instead of losing chunks. Its dropped
        // receiver must still cancel promptly while stdout is backpressured.
        drop(responses.remove(0));
        assert_eq!(read_json(&mut reader).await, json!({"type":"cancel", "id":ids[0]}));
        send_json(&mut writer, json!({"type":"chunk", "id":ids[1], "chunk":{"type":"text-delta", "delta":"independent"}})).await;
        assert!(String::from_utf8(next(&mut responses[0]).await.unwrap().unwrap()).unwrap().contains("independent"));
    }

    #[test]
    fn configuration_rejects_duplicate_principals_and_relative_paths() {
        let make = || WorkerConfig {
            principal_id: "alice".into(), session_id: "session-a".into(), genesis: "genesis-a".into(),
            executable: "/protected/node".into(), args: vec!["/protected/worker.mjs".into()],
            profile_path: "/protected/profile.json".into(), provider_policy_path: "/protected/policy.json".into(),
        };
        assert!(validate_configs(&[make(), make()]).is_err());
        let mut invalid = make(); invalid.profile_path = "relative.json".into();
        assert!(validate_configs(&[invalid]).is_err());
        assert!(validate_configs(&[make()]).is_ok());
    }

    #[test]
    fn protected_configuration_rejects_symlinks_and_nonprotected_ancestors() {
        let temp = tempfile::tempdir_in("/tmp").unwrap();
        let file = temp.path().join("workers.json");
        std::fs::write(&file, b"[]").unwrap();
        let link = temp.path().join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(protected_file(&link).is_err());
        // /tmp's writable ancestor is refused even if the leaf is root-owned.
        assert!(protected_file(&file).is_err());
    }
}
