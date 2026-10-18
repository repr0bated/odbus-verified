use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
#[cfg(test)]
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, UNIX_EPOCH};

use crate::dispatch::{self, CallReceipt, LungsError};
use crate::fuse_buffers as buffers;
#[cfg(test)]
use crate::fuse_process as process;
use crate::namespace::{MountIdentity, Namespace, Node, NodeKind, ProjectionTarget};
use crate::store::{ObjectStore, StageConflict};
use crate::threetched_surface::ThreetchedSurface;
use buffers::{Budget, Buffer, Charge, DOCUMENT_LIMIT, HANDLE_LIMIT, INPUT_LIMIT, TOTAL_LIMIT};
use fuser::{
    FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyWrite, Request, TimeOrNow,
};
use libc::{EACCES, EAGAIN, EBADF, EBUSY, EINVAL, EIO, EISDIR, ENOENT, EROFS, ESTALE};
use parking_lot::Mutex;
use serde_json::Value;

const TTL: Duration = Duration::ZERO;
const JOB_LIMIT: usize = 4;
const WAITER_LIMIT: usize = 32;
enum PendingReply {
    Fuse(ReplyEmpty),
    #[cfg(test)]
    Test(std::sync::mpsc::Sender<i32>),
}
impl PendingReply {
    fn ok(self) {
        self.finish(Ok(()));
    }
    fn error(self, errno: i32) {
        self.finish(Err(errno));
    }
    fn finish(self, result: Result<(), i32>) {
        match self {
            Self::Fuse(reply) => match result {
                Ok(()) => reply.ok(),
                Err(errno) => reply.error(errno),
            },
            #[cfg(test)]
            Self::Test(sender) => {
                let _ = sender.send(result.err().unwrap_or(0));
            }
        }
    }
}
type Completion = Option<Result<Buffer, (i32, Buffer)>>;

struct TargetGuard(Arc<AtomicBool>);
impl Drop for TargetGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct OpenFile {
    /// Descriptor and access are pinned at open, independently of refresh.
    node: Node,
    uid: u32,
    access: i32,
    bytes: Buffer,
    dirty: bool,
    running: bool,
    completed: Completion,
    fallback: Option<Buffer>,
    admission_fallback: Option<Buffer>,
    /// Pin the captured upstream declaration and executable selection at open.
    surface: Option<Arc<ThreetchedSurface>>,
    waiters: Vec<PendingReply>,
    _target_guard: Option<TargetGuard>,
    _slot: Charge,
}
impl OpenFile {
    fn check(&self, ino: u64, uid: u32, write: bool) -> Result<(), i32> {
        if self.node.ino != ino {
            return Err(EBADF);
        }
        if self.uid != uid {
            return Err(EACCES);
        }
        if write && self.access == libc::O_RDONLY || !write && self.access == libc::O_WRONLY {
            return Err(EBADF);
        }
        if self.running {
            return Err(EAGAIN);
        }
        if write && self.completed.is_some() {
            return Err(EINVAL);
        }
        Ok(())
    }
    fn readable_bytes(&self) -> &[u8] {
        if matches!(self.node.kind, NodeKind::CallFile { .. }) {
            match &self.completed {
                Some(Ok(bytes)) | Some(Err((_, bytes))) => &bytes.bytes,
                None => &[],
            }
        } else {
            &self.bytes.bytes
        }
    }
    fn limit(&self) -> usize {
        if matches!(self.node.kind, NodeKind::CallFile { .. }) {
            INPUT_LIMIT
        } else {
            DOCUMENT_LIMIT
        }
    }
}
struct OpenDirectory {
    ino: u64,
    uid: u32,
    entries: Vec<(u64, FileType, OsString)>,
    _bytes: Charge,
    _slot: Charge,
}

#[derive(Clone)]
pub enum Lungs {
    Socket {
        path: PathBuf,
        session: String,
    },
    Credential {
        bus_address: String,
        credential: PathBuf,
    },
}

pub struct TypedFuse {
    store: ObjectStore,
    namespace: Namespace,
    open_files: HashMap<u64, Arc<Mutex<OpenFile>>>,
    directories: HashMap<u64, OpenDirectory>,
    next_fh: u64,
    uid: u32,
    gid: u32,
    call_owner_uid: u32,
    lungs: Option<Lungs>,
    budget: Arc<Budget>,
    slots: Arc<Budget>,
    jobs: Arc<Budget>,
    target_guards: HashMap<(String, String), std::sync::Weak<AtomicBool>>,
    cancelled: Arc<AtomicBool>,
}
impl TypedFuse {
    pub fn new(store: ObjectStore, target: ProjectionTarget) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, None)?;
        Self::from_parts(store, namespace, None)
    }
    pub fn with_state_dir(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, Some(&state_dir))?;
        Self::from_parts(store, namespace, None)
    }
    pub fn lungs(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
        lungs: Lungs,
    ) -> anyhow::Result<Self> {
        let namespace = Namespace::build_lungs(&store, target, &state_dir)?;
        Self::from_parts(store, namespace, Some(lungs))
    }
    fn from_parts(
        store: ObjectStore,
        mut namespace: Namespace,
        lungs: Option<Lungs>,
    ) -> anyhow::Result<Self> {
        let identity = match &lungs {
            Some(Lungs::Socket { session, .. }) if !session.is_empty() => {
                MountIdentity::HostSession {
                    session: session.clone(),
                }
            }
            Some(Lungs::Socket { .. }) => MountIdentity::GuestKernel,
            Some(Lungs::Credential { .. }) => MountIdentity::Credential,
            None => MountIdentity::Unbound,
        };
        namespace.declare_mount_mode(identity, Some(&uuid::Uuid::new_v4().to_string()))?;
        let uid = unsafe { libc::getuid() };
        namespace.declare_local_owner(uid, None);
        Ok(Self {
            store,
            namespace,
            open_files: HashMap::new(),
            directories: HashMap::new(),
            next_fh: 1,
            uid,
            gid: unsafe { libc::getgid() },
            call_owner_uid: uid,
            lungs,
            budget: Arc::new(Budget::default()),
            slots: Arc::new(Budget::default()),
            jobs: Arc::new(Budget::default()),
            target_guards: HashMap::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }
    #[cfg(test)]
    pub fn graft_threetched(&mut self, surface: ThreetchedSurface) -> anyhow::Result<()> {
        self.namespace.graft_threetched(surface)?;
        Ok(())
    }
    pub fn with_catalog_dir(mut self, catalog: PathBuf) -> anyhow::Result<Self> {
        self.namespace.declare_catalog_dir(&catalog)?;
        // Catalog declaration does not require eager SHM discovery.
        Ok(self)
    }
    pub fn with_call_owner_uid(mut self, uid: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.uid == 0 || uid == self.uid,
            "only a root mount daemon can assign a different local caller UID"
        );
        self.call_owner_uid = uid;
        self.namespace
            .declare_local_owner(uid, (self.uid != uid).then_some(self.uid));
        Ok(self)
    }
    fn caller_allowed(&self, uid: u32) -> bool {
        uid == self.uid || uid == self.call_owner_uid
    }
    #[cfg(test)]
    fn refresh_namespace(&mut self) -> anyhow::Result<()> {
        self.namespace = self.namespace.refresh(&self.store)?;
        Ok(())
    }
    fn next_handle(&mut self) -> u64 {
        let fh = self.next_fh;
        self.next_fh = self.next_fh.wrapping_add(1).max(1);
        fh
    }
    fn node_bytes(&self, node: &Node) -> Result<Buffer, i32> {
        Buffer::load(&self.budget, || {
            self.namespace.file_bytes(&self.store, node)
        })
    }
    fn attr(&self, node: &Node, size: Option<u64>) -> Result<FileAttr, i32> {
        let (kind, size, perm, nlink) = match &node.kind {
            NodeKind::Directory => (FileType::Directory, 0, 0o555, 2),
            NodeKind::File(_) | NodeKind::LiveFile { .. } => (
                FileType::RegularFile,
                match size {
                    Some(size) => size,
                    None => self.node_bytes(node)?.bytes.len() as u64,
                },
                if node.write_target.is_some() {
                    0o600
                } else {
                    0o444
                },
                1,
            ),
            NodeKind::Symlink(target) => (FileType::Symlink, target.len() as u64, 0o777, 1),
            NodeKind::CallFile { .. } => (FileType::RegularFile, size.unwrap_or(0), 0o600, 1),
        };
        Ok(FileAttr {
            ino: node.ino,
            size,
            blocks: size.div_ceil(512),
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind,
            perm,
            nlink,
            uid: if node_writable(node) {
                self.call_owner_uid
            } else {
                self.uid
            },
            gid: self.gid,
            rdev: 0,
            flags: 0,
            blksize: 4096,
        })
    }
    fn open_handle(&mut self, ino: u64, flags: i32, uid: u32) -> Result<u64, i32> {
        self.namespace
            .refresh_for_inode(&self.store, ino)
            .map_err(|_| EIO)?;
        let node = self.namespace.node(ino).ok_or(ENOENT)?.clone();
        let access = flags & libc::O_ACCMODE;
        if ![libc::O_RDONLY, libc::O_WRONLY, libc::O_RDWR].contains(&access) {
            return Err(EINVAL);
        }
        let write = access != libc::O_RDONLY;
        if write && !node_writable(&node) {
            return Err(EROFS);
        }
        if node_writable(&node) && !self.caller_allowed(uid) {
            return Err(EACCES);
        }
        let slot = self.slots.take(1, HANDLE_LIMIT)?;
        let guard = if write {
            if let Some(target) = &node.write_target {
                // One writer per workspace object; sibling fields share the CAS.
                let key = (target.workspace_id.clone(), target.plugin_id.clone());
                self.target_guards.retain(|_, weak| weak.strong_count() > 0);
                let held = self
                    .target_guards
                    .get(&key)
                    .and_then(std::sync::Weak::upgrade)
                    .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                held.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| EBUSY)?;
                self.target_guards.insert(key, Arc::downgrade(&held));
                Some(TargetGuard(held))
            } else {
                None
            }
        } else {
            None
        };
        let mut bytes = match &node.kind {
            NodeKind::File(_) | NodeKind::LiveFile { .. } => self.node_bytes(&node)?,
            NodeKind::CallFile { .. } => Buffer::empty(&self.budget),
            NodeKind::Directory => return Err(EISDIR),
            NodeKind::Symlink(_) => return Err(EINVAL),
        };
        if write && flags & libc::O_TRUNC != 0 {
            bytes.resize(0, DOCUMENT_LIMIT)?;
        }
        let dirty =
            write && flags & libc::O_TRUNC != 0 && !matches!(node.kind, NodeKind::CallFile { .. });
        let fallback = if let NodeKind::CallFile { plugin_id, method } = &node.kind {
            Some(Buffer::load_limited(&self.budget, 512, || {
                crate::store::render_document(
                    &serde_json::json!({"plugin":plugin_id,"method":method,"stage":"outcome_unknown","error":"terminal diagnostic unavailable; do not retry automatically"}),
                )
            })?)
        } else {
            None
        };
        let admission_fallback = if let NodeKind::CallFile { plugin_id, method } = &node.kind {
            Some(Buffer::load_limited(&self.budget, 512, || {
                crate::store::render_document(
                    &serde_json::json!({"plugin":plugin_id,"method":method,"stage":"admission","error":"operation was not dispatched; reopen explicitly to make a new attempt"}),
                )
            })?)
        } else {
            None
        };
        let surface = match &node.kind {
            NodeKind::CallFile { plugin_id, .. } if plugin_id == "threetched" => {
                self.namespace.threetched_surface.clone()
            }
            _ => None,
        };
        let fh = self.next_handle();
        self.open_files.insert(
            fh,
            Arc::new(Mutex::new(OpenFile {
                node,
                uid,
                access,
                bytes,
                dirty,
                running: false,
                completed: None,
                fallback,
                admission_fallback,
                surface,
                waiters: Vec::new(),
                _target_guard: guard,
                _slot: slot,
            })),
        );
        Ok(fh)
    }
    /// Enqueue the reply, launch at most four bounded jobs, then immediately
    /// return to the FUSE loop. Kernel release uses pinned FH/inode, not its unset UID.
    fn submit(&mut self, ino: u64, fh: u64, uid: u32, reply: PendingReply, release: bool) {
        let Some(state) = self.open_files.get(&fh).cloned() else {
            reply.error(EBADF);
            return;
        };
        let mut handle = state.lock();
        if handle.node.ino != ino || (!release && handle.uid != uid) {
            reply.error(EACCES);
            return;
        }
        if release {
            self.open_files.remove(&fh);
        }
        if let Some(completed) = &handle.completed {
            reply_result(
                reply,
                completed.as_ref().map(|_| ()).map_err(|(errno, _)| *errno),
            );
            return;
        }
        if handle.running {
            if handle.waiters.len() >= WAITER_LIMIT {
                reply.error(EBUSY);
            } else {
                handle.waiters.push(reply);
            }
            return;
        }
        if !handle.dirty {
            reply.ok();
            return;
        }
        let permit = match self.jobs.take(1, JOB_LIMIT) {
            Ok(permit) => permit,
            Err(_) => {
                // A rejected fsync is terminal for this descriptor. Closing it
                // after capacity returns must never dispatch rejected input.
                handle.dirty = false;
                handle.bytes = Buffer::empty(&self.budget);
                let diagnostic = handle
                    .admission_fallback
                    .take()
                    .unwrap_or_else(|| Buffer::empty(&self.budget));
                handle.completed = Some(Err((EBUSY, diagnostic)));
                reply.error(EBUSY);
                return;
            }
        };
        let bytes = std::mem::replace(&mut handle.bytes, Buffer::empty(&self.budget));
        let node = handle.node.clone();
        handle.running = true;
        handle.dirty = false;
        handle.waiters.push(reply);
        let threetched = handle.surface.clone();
        drop(handle);
        let store = self.store.clone();
        let target = self.namespace.target.clone();
        let lungs = self.lungs.clone();
        let cancelled = self.cancelled.clone();
        let budget = self.budget.clone();
        let failure_state = state.clone();
        let failure_budget = budget.clone();
        let failure_node = node.clone();
        let spawned = std::thread::Builder::new()
            .name("3tchedfs-operation".into())
            .spawn(move || {
                let _permit = permit;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute(
                        &store,
                        &target,
                        lungs.as_ref(),
                        threetched.as_deref(),
                        &node,
                        bytes,
                        &budget,
                        &cancelled,
                    )
                }))
                .unwrap_or_else(|_| {
                    Execution::Call(Err((
                        EIO,
                        diagnostic(
                            &budget,
                            &node,
                            "outcome_unknown",
                            "worker panicked; do not retry automatically",
                            None,
                        ),
                    )))
                });
                complete(&state, result);
            });
        if spawned.is_err() {
            complete(
                &failure_state,
                Execution::Rejected(
                    EBUSY,
                    diagnostic(
                        &failure_budget,
                        &failure_node,
                        "admission",
                        "worker could not start",
                        None,
                    ),
                ),
            );
        }
    }
}
impl Drop for TypedFuse {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
fn node_writable(node: &Node) -> bool {
    node.write_target.is_some() || matches!(node.kind, NodeKind::CallFile { .. })
}
fn reply_result(reply: PendingReply, result: Result<(), i32>) {
    match result {
        Ok(()) => reply.ok(),
        Err(error) => reply.error(error),
    }
}

enum Execution {
    Call(Result<Buffer, (i32, Buffer)>),
    Rejected(i32, Buffer),
    Workspace(Result<(Buffer, String), i32>),
}
#[allow(clippy::too_many_arguments)]
fn execute(
    store: &ObjectStore,
    target: &ProjectionTarget,
    lungs: Option<&Lungs>,
    _threetched: Option<&ThreetchedSurface>,
    node: &Node,
    bytes: Buffer,
    budget: &Arc<Budget>,
    _cancelled: &AtomicBool,
) -> Execution {
    if let NodeKind::CallFile { plugin_id, method } = &node.kind {
        // Binary execution here exists only for isolated lifecycle fixtures.
        // Production always uses the identity-bound sealed backend route below.
        #[cfg(test)]
        if plugin_id == "threetched" && _threetched.is_some() {
            // Keep accepted input charged until execution and serialization
            // finish, including after RELEASE removes the table entry.
            return Execution::Call(
                match run_threetched_call(method, &bytes.bytes, _threetched, budget, _cancelled) {
                    Ok((true, envelope)) => Ok(envelope),
                    Ok((false, envelope)) => Err((EIO, envelope)),
                    Err(error) => {
                        let bytes =
                            diagnostic(budget, node, error.stage(), &error.to_string(), None);
                        if error.stage() == "admission" {
                            return Execution::Rejected(error.errno(), bytes);
                        }
                        Err((error.errno(), bytes))
                    }
                },
            );
        }
        let outcome = run_lungs_call(store, target, lungs, plugin_id, method, &bytes.bytes);
        drop(bytes);
        return Execution::Call(match outcome {
            Ok(receipt) => Buffer::load(budget, || crate::store::render_document(&receipt))
                .map_err(|errno| {
                    (
                        errno,
                        diagnostic(
                            budget,
                            node,
                            "outcome_unknown",
                            "receipt could not be retained; use declared receipt recovery",
                            receipt.result["operation_id"].as_str(),
                        ),
                    )
                }),
            Err(error) => Err((
                error.errno(),
                diagnostic(
                    budget,
                    node,
                    error.stage(),
                    &error.to_string(),
                    error.operation_id(),
                ),
            )),
        });
    }
    let Some(write) = &node.write_target else {
        return Execution::Workspace(Err(EROFS));
    };
    let result = serde_json::from_slice::<Value>(&bytes.bytes)
        .map_err(|_| EINVAL)
        .and_then(|value| {
            store
                .stage_pointer_checked(
                    &write.workspace_id,
                    &write.plugin_id,
                    &write.pointer,
                    &write.expected_object_hash,
                    value,
                )
                .map_err(|error| {
                    if error.downcast_ref::<StageConflict>().is_some() {
                        ESTALE
                    } else {
                        EINVAL
                    }
                })
        });
    Execution::Workspace(result.map(|hash| (bytes, hash)))
}
fn complete(state: &Arc<Mutex<OpenFile>>, result: Execution) {
    let mut handle = state.lock();
    handle.running = false;
    let status = match result {
        Execution::Rejected(errno, mut bytes) => {
            if bytes.bytes.is_empty() {
                bytes = handle
                    .admission_fallback
                    .take()
                    .unwrap_or_else(|| Buffer::empty(&handle.bytes.charge_budget()));
            }
            handle.completed = Some(Err((errno, bytes)));
            Err(errno)
        }
        Execution::Call(mut result) => {
            if let Err((_, bytes)) = &mut result
                && bytes.bytes.is_empty()
                && let Some(fallback) = handle.fallback.take()
            {
                *bytes = fallback;
            }
            let status = result.as_ref().map(|_| ()).map_err(|(errno, _)| *errno);
            handle.completed = Some(result);
            status
        }
        Execution::Workspace(Ok((bytes, hash))) => {
            handle.bytes = bytes;
            if let Some(target) = &mut handle.node.write_target {
                target.expected_object_hash = hash;
            }
            Ok(())
        }
        Execution::Workspace(Err(errno)) => {
            // A failed staging attempt is terminal until reopen, so release
            // cannot silently retry a conflicting or malformed replacement.
            let empty = Buffer::empty(&handle.bytes.charge_budget());
            handle.completed = Some(Err((errno, empty)));
            Err(errno)
        }
    };
    handle.admission_fallback = None;
    let waiters = std::mem::take(&mut handle.waiters);
    drop(handle);
    for reply in waiters {
        reply_result(reply, status);
    }
}
fn diagnostic(
    budget: &Arc<Budget>,
    node: &Node,
    stage: &str,
    error: &str,
    operation_id: Option<&str>,
) -> Buffer {
    let (plugin, method) = match &node.kind {
        NodeKind::CallFile { plugin_id, method } => (plugin_id.as_str(), method.as_str()),
        _ => ("workspace", "stage"),
    };
    // Diagnostics remain small even when a remote peer supplies a huge error.
    let end = error
        .char_indices()
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset <= 4096)
        .last()
        .unwrap_or(0);
    let text = if error.len() <= 4096 {
        error
    } else {
        &error[..end]
    };
    Buffer::load_limited(budget, 8192, || crate::store::render_document(&serde_json::json!({
        "plugin": plugin, "method": method, "stage": stage, "error": text, "operation_id": operation_id
    }))).unwrap_or_else(|_| Buffer::empty(budget))
}

#[cfg(test)]
fn run_threetched_call(
    command: &str,
    bytes: &[u8],
    surface: Option<&ThreetchedSurface>,
    budget: &Arc<Budget>,
    cancelled: &AtomicBool,
) -> Result<(bool, Buffer), ThreetchedCallError> {
    let surface = surface.ok_or_else(|| {
        ThreetchedCallError::Contract("threetched surface is not attached".into())
    })?;
    let arguments: Value = serde_json::from_slice(bytes)
        .map_err(|error| ThreetchedCallError::Json(error.to_string()))?;
    if !arguments.is_object() {
        return Err(ThreetchedCallError::Arguments(
            "arguments must be a JSON object".into(),
        ));
    }
    let schema = surface
        .input_schema(command)
        .map_err(|error| ThreetchedCallError::Arguments(error.to_string()))?;
    jsonschema::Validator::new(&schema)
        .map_err(|error| ThreetchedCallError::Arguments(format!("argument schema: {error}")))?
        .validate(&arguments)
        .map_err(|error| ThreetchedCallError::Arguments(error.to_string()))?;
    let argv = surface
        .arguments(command, &arguments)
        .map_err(|error| ThreetchedCallError::Arguments(error.to_string()))?;
    let mut process = Command::new(surface.binary_path());
    process.args(argv);
    let output = process::run(&mut process, budget, cancelled, process::Limits::default())
        .map_err(ThreetchedCallError::Process)?;
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        stdout: &'a str,
        stderr: &'a str,
        exit: i32,
    }
    // Borrow the charged output buffers during serialization. Invalid UTF-8
    // is terminal; lossy conversion could allocate another uncharged document.
    let envelope = Envelope {
        stdout: std::str::from_utf8(&output.stdout.bytes)
            .map_err(|_| ThreetchedCallError::Outcome("stdout is not UTF-8"))?,
        stderr: std::str::from_utf8(&output.stderr.bytes)
            .map_err(|_| ThreetchedCallError::Outcome("stderr is not UTF-8"))?,
        exit: output.status.code().unwrap_or(-1),
    };
    let bytes = Buffer::serialize(budget, &envelope)
        .map_err(|_| ThreetchedCallError::Outcome("receipt could not be retained"))?;
    Ok((output.status.success(), bytes))
}

#[cfg(test)]
#[derive(Debug, thiserror::Error)]
enum ThreetchedCallError {
    #[error("arguments are not a JSON value: {0}")]
    Json(String),
    #[error("arguments failed validation: {0}")]
    Arguments(String),
    #[error("threetched surface unavailable: {0}")]
    Contract(String),
    #[error("{0}")]
    Process(process::Error),
    #[error("{0}; do not retry automatically")]
    Outcome(&'static str),
}

#[cfg(test)]
impl ThreetchedCallError {
    fn stage(&self) -> &'static str {
        match self {
            Self::Json(_) => "parse",
            Self::Arguments(_) => "validate",
            Self::Contract(_) => "contract",
            Self::Process(error) => error.stage(),
            Self::Outcome(_) => "outcome_unknown",
        }
    }

    fn errno(&self) -> i32 {
        match self {
            Self::Json(_) | Self::Arguments(_) => EINVAL,
            Self::Contract(_) => libc::EPROTO,
            Self::Process(error) => error.errno(),
            Self::Outcome(_) => EIO,
        }
    }
}

fn run_lungs_call(
    store: &ObjectStore,
    target: &ProjectionTarget,
    lungs: Option<&Lungs>,
    plugin_id: &str,
    method_name: &str,
    bytes: &[u8],
) -> Result<CallReceipt, LungsError> {
    let lungs = lungs.ok_or_else(|| LungsError::Transport("not a lungs mount".into()))?;
    let arguments: Value =
        serde_json::from_slice(bytes).map_err(|error| LungsError::Json(error.to_string()))?;
    let method = dispatch::method_decl(store, target, plugin_id, method_name)
        .map_err(|error| LungsError::Arguments(error.to_string()))?;
    dispatch::validate_arguments(&method, &arguments)?;
    let credential = match lungs {
        Lungs::Credential { credential, .. } => Some(dispatch::read_credential(credential)?),
        _ => None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| LungsError::Transport(format!("runtime: {error}")))?;
    runtime.block_on(async {
        match (lungs, credential) {
            (Lungs::Credential { bus_address, .. }, Some(credential)) => {
                dispatch::call_as_identity(
                    bus_address,
                    plugin_id,
                    method_name,
                    &method,
                    arguments,
                    credential,
                )
                .await
            }
            (Lungs::Socket { path, session }, _) => {
                dispatch::call_over_socket(
                    path,
                    session,
                    plugin_id,
                    method_name,
                    &method,
                    arguments,
                )
                .await
            }
            _ => Err(LungsError::Credential("credential was not read".into())),
        }
    })
}

impl Filesystem for TypedFuse {
    fn destroy(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn init(&mut self, _request: &Request<'_>, config: &mut KernelConfig) -> Result<(), i32> {
        config
            .add_capabilities(fuser::consts::FUSE_ATOMIC_O_TRUNC)
            .map_err(|_| EIO)?;
        config
            .set_max_write(INPUT_LIMIT as u32)
            .map(|_| ())
            .map_err(|_| EIO)
    }
    fn lookup(&mut self, _request: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if self
            .namespace
            .refresh_for_inode(&self.store, parent)
            .is_err()
        {
            reply.error(EIO);
            return;
        }
        match self.namespace.lookup(parent, name) {
            Some(node) => match self.attr(node, None) {
                Ok(attr) => reply.entry(&TTL, &attr, 0),
                Err(error) => reply.error(error),
            },
            None => reply.error(ENOENT),
        }
    }
    fn getattr(&mut self, request: &Request<'_>, ino: u64, fh: Option<u64>, reply: ReplyAttr) {
        if let Some(fh) = fh {
            let Some(state) = self.open_files.get(&fh) else {
                reply.error(EBADF);
                return;
            };
            let handle = state.lock();
            if handle.node.ino != ino || handle.uid != request.uid() {
                reply.error(EACCES);
                return;
            }
            let size = if handle.running {
                0
            } else {
                handle.readable_bytes().len() as u64
            };
            match self.attr(&handle.node, Some(size)) {
                Ok(attr) => reply.attr(&TTL, &attr),
                Err(error) => reply.error(error),
            }
            return;
        }
        if self.namespace.refresh_for_inode(&self.store, ino).is_err() {
            reply.error(EIO);
            return;
        }
        match self.namespace.node(ino) {
            Some(node) => match self.attr(node, None) {
                Ok(attr) => reply.attr(&TTL, &attr),
                Err(error) => reply.error(error),
            },
            None => reply.error(ENOENT),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn setattr(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        if mode.is_some() || uid.is_some() || gid.is_some() {
            reply.error(EROFS);
            return;
        }
        let Some(fh) = fh else {
            reply.error(EACCES);
            return;
        };
        let Some(state) = self.open_files.get(&fh) else {
            reply.error(EBADF);
            return;
        };
        let mut handle = state.lock();
        if let Err(error) = handle.check(ino, request.uid(), true) {
            reply.error(error);
            return;
        }
        if !node_writable(&handle.node) {
            reply.error(EROFS);
            return;
        }
        if let Some(size) = size {
            let size = match usize::try_from(size) {
                Ok(size) => size,
                Err(_) => {
                    reply.error(libc::EFBIG);
                    return;
                }
            };
            let limit = handle.limit();
            if let Err(error) = handle.bytes.resize(size, limit) {
                reply.error(error);
                return;
            }
            handle.dirty = !matches!(handle.node.kind, NodeKind::CallFile { .. }) || size > 0;
        }
        match self.attr(&handle.node, Some(handle.bytes.bytes.len() as u64)) {
            Ok(attr) => reply.attr(&TTL, &attr),
            Err(error) => reply.error(error),
        }
    }
    fn readlink(&mut self, _request: &Request<'_>, ino: u64, reply: ReplyData) {
        if self.namespace.refresh_for_inode(&self.store, ino).is_err() {
            reply.error(EIO);
            return;
        }
        match self.namespace.node(ino).map(|node| &node.kind) {
            Some(NodeKind::Symlink(target)) => reply.data(target.as_bytes()),
            Some(_) => reply.error(EINVAL),
            None => reply.error(ENOENT),
        }
    }
    fn open(&mut self, request: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        match self.open_handle(ino, flags, request.uid()) {
            Ok(fh) => reply.opened(fh, fuser::consts::FOPEN_DIRECT_IO),
            Err(error) => reply.error(error),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn read(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Ok(start) = usize::try_from(offset) else {
            reply.error(EINVAL);
            return;
        };
        let Some(state) = self.open_files.get(&fh) else {
            reply.error(EBADF);
            return;
        };
        let handle = state.lock();
        if let Err(error) = handle.check(ino, request.uid(), false) {
            reply.error(error);
            return;
        }
        let bytes = handle.readable_bytes();
        let start = start.min(bytes.len());
        let end = start.saturating_add(size as usize).min(bytes.len());
        reply.data(&bytes[start..end]);
    }
    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let Ok(start) = usize::try_from(offset) else {
            reply.error(EINVAL);
            return;
        };
        let Some(state) = self.open_files.get(&fh) else {
            reply.error(EBADF);
            return;
        };
        let mut handle = state.lock();
        if let Err(error) = handle.check(ino, request.uid(), true) {
            reply.error(error);
            return;
        }
        if !node_writable(&handle.node) {
            reply.error(EROFS);
            return;
        }
        let Some(end) = start.checked_add(data.len()) else {
            reply.error(libc::EFBIG);
            return;
        };
        let limit = handle.limit();
        if end > limit || data.len() > INPUT_LIMIT {
            reply.error(libc::EFBIG);
            return;
        }
        if data.is_empty() {
            reply.written(0);
            return;
        }
        let new_len = handle.bytes.bytes.len().max(end);
        if let Err(error) = handle.bytes.resize(new_len, limit) {
            reply.error(error);
            return;
        }
        handle.bytes.bytes[start..end].copy_from_slice(data);
        handle.dirty = true;
        reply.written(data.len() as u32);
    }
    fn flush(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        self.submit(ino, fh, request.uid(), PendingReply::Fuse(reply), false);
    }
    fn release(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.submit(ino, fh, request.uid(), PendingReply::Fuse(reply), true);
    }
    fn fsync(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        self.submit(ino, fh, request.uid(), PendingReply::Fuse(reply), false);
    }
    fn opendir(&mut self, request: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & libc::O_ACCMODE != libc::O_RDONLY {
            reply.error(EROFS);
            return;
        }
        if self.namespace.refresh_for_inode(&self.store, ino).is_err() {
            reply.error(EIO);
            return;
        }
        let Some(node) = self.namespace.node(ino) else {
            reply.error(ENOENT);
            return;
        };
        if !matches!(node.kind, NodeKind::Directory) {
            reply.error(EINVAL);
            return;
        }
        let Some(children) = self.namespace.children(ino) else {
            reply.error(EIO);
            return;
        };
        let count = match children.len().checked_add(2) {
            Some(count) => count,
            None => {
                reply.error(libc::EFBIG);
                return;
            }
        };
        let charge_size = children.keys().fold(
            count
                .checked_mul(std::mem::size_of::<(u64, FileType, OsString)>())
                .and_then(|bytes| bytes.checked_add(3)),
            |sum, name| sum.and_then(|sum| sum.checked_add(name.len())),
        );
        let Some(charge_size) = charge_size.filter(|size| *size <= DOCUMENT_LIMIT) else {
            reply.error(libc::EFBIG);
            return;
        };
        let slot = match self.slots.take(1, HANDLE_LIMIT) {
            Ok(slot) => slot,
            Err(error) => {
                reply.error(error);
                return;
            }
        };
        let charge = match self.budget.take(charge_size, TOTAL_LIMIT) {
            Ok(charge) => charge,
            Err(error) => {
                reply.error(error);
                return;
            }
        };
        let mut entries = Vec::new();
        if entries.try_reserve_exact(count).is_err() {
            reply.error(libc::ENOMEM);
            return;
        }
        entries.push((ino, FileType::Directory, OsString::from(".")));
        entries.push((node.parent, FileType::Directory, OsString::from("..")));
        for (name, child_ino) in children {
            let Some(child) = self.namespace.node(*child_ino) else {
                reply.error(EIO);
                return;
            };
            let kind = match &child.kind {
                NodeKind::Directory => FileType::Directory,
                NodeKind::Symlink(_) => FileType::Symlink,
                _ => FileType::RegularFile,
            };
            entries.push((*child_ino, kind, name.clone()));
        }
        let fh = self.next_handle();
        self.directories.insert(
            fh,
            OpenDirectory {
                ino,
                uid: request.uid(),
                entries,
                _bytes: charge,
                _slot: slot,
            },
        );
        reply.opened(fh, 0);
    }
    fn readdir(
        &mut self,
        request: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Ok(offset) = usize::try_from(offset) else {
            reply.error(EINVAL);
            return;
        };
        let Some(directory) = self.directories.get(&fh) else {
            reply.error(EBADF);
            return;
        };
        if directory.ino != ino || directory.uid != request.uid() {
            reply.error(EACCES);
            return;
        }
        for (index, (child_ino, kind, name)) in directory.entries.iter().enumerate().skip(offset) {
            if reply.add(*child_ino, (index + 1) as i64, *kind, name) {
                break;
            }
        }
        reply.ok();
    }
    fn releasedir(
        &mut self,
        _request: &Request<'_>,
        ino: u64,
        fh: u64,
        _flags: i32,
        reply: ReplyEmpty,
    ) {
        match self.directories.get(&fh) {
            Some(directory) if directory.ino == ino => {
                self.directories.remove(&fh);
                reply.ok();
            }
            Some(_) => reply.error(EACCES),
            None => reply.error(EBADF),
        }
    }
    fn access(&mut self, request: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        if self.namespace.refresh_for_inode(&self.store, ino).is_err() {
            reply.error(EIO);
            return;
        }
        match self.namespace.node(ino) {
            None => reply.error(ENOENT),
            Some(node) if mask & libc::W_OK != 0 && !node_writable(node) => reply.error(EROFS),
            Some(node) if node_writable(node) && !self.caller_allowed(request.uid()) => {
                reply.error(EACCES)
            }
            Some(_) => reply.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PluginGuarantees, PluginSchema, TypedObjectBlob};
    use crate::namespace::ROOT_INO;
    use crate::source::SourceSnapshot;
    use std::collections::BTreeMap;
    use tempfile::tempdir;
    fn lungs_fixture(credential: PathBuf) -> (tempfile::TempDir, TypedFuse) {
        lungs_fixture_with(Lungs::Credential {
            bus_address: "unix:path=/nonexistent/lungs-test.sock".into(),
            credential,
        })
    }

    fn lungs_fixture_with(lungs: Lungs) -> (tempfile::TempDir, TypedFuse) {
        lungs_fixture_plugin(lungs, "demo")
    }

    fn lungs_fixture_plugin(lungs: Lungs, plugin: &str) -> (tempfile::TempDir, TypedFuse) {
        let directory = tempdir().unwrap();
        let state = directory.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let store = ObjectStore::open(directory.path().join("store")).unwrap();
        let method = crate::model::MethodDecl {
            name: "record_event".into(),
            args: serde_json::json!({
                "type": "object",
                "properties": {"event": {"type": "string"}},
                "required": ["event"],
                "additionalProperties": false
            }),
            returns: Some(
                serde_json::json!({"type":"object", "properties":{"method":{"type":"string"},"args":{"type":"string"}}, "required":["method","args"],"additionalProperties":false}),
            ),
            side_effect: crate::model::SideEffect::Mutation,
            idempotent: false,
            required_capability: Some("cap.demo.record@v1".into()),
            subid: "mut.demo.record@v1".into(),
        };
        let object = TypedObjectBlob::new(
            plugin.into(),
            "a".repeat(64),
            PluginSchema {
                name: plugin.into(),
                category: String::new(),
                version: "1".into(),
                description: String::new(),
                display_name: None,
                fields: BTreeMap::new(),
                dependencies: Vec::new(),
                immutable_paths: Vec::new(),
                tags: Vec::new(),
                dialect: String::new(),
                mutation_index: None,
                subids: BTreeMap::new(),
                org: None,
                methods: BTreeMap::from([("record_event".into(), method)]),
                signals: Vec::new(),
                guarantees: PluginGuarantees::default(),
                ..Default::default()
            },
            serde_json::json!({}),
        )
        .unwrap();
        store
            .capture_snapshot(
                SourceSnapshot {
                    catalog_hash: "catalog".into(),
                    catalog_generation: 1,
                    objects: BTreeMap::from([(plugin.into(), object)]),
                },
                true,
            )
            .unwrap();
        let target = ProjectionTarget::View(store.current_view_id().unwrap().unwrap());
        let fs = TypedFuse::lungs(store, target, state, lungs).unwrap();
        (directory, fs)
    }

    fn method_node<'a>(fs: &'a TypedFuse, name: &str) -> &'a Node {
        let plugins = fs
            .namespace
            .lookup(ROOT_INO, OsStr::new("plugins"))
            .unwrap();
        let demo = fs
            .namespace
            .lookup(plugins.ino, OsStr::new("demo"))
            .unwrap();
        let operations = fs
            .namespace
            .lookup(demo.ino, OsStr::new("operations"))
            .unwrap();
        let method = fs
            .namespace
            .lookup(operations.ino, OsStr::new("record_event"))
            .unwrap();
        fs.namespace.lookup(method.ino, OsStr::new(name)).unwrap()
    }

    fn stage_call(fs: &mut TypedFuse, bytes: &[u8]) -> u64 {
        let ino = method_node(fs, "call").ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        let mut handle = fs.open_files[&fh].lock();
        handle.bytes.resize(bytes.len(), INPUT_LIMIT).unwrap();
        handle.bytes.bytes.copy_from_slice(bytes);
        handle.dirty = !bytes.is_empty();
        fh
    }
    fn run_staged(fs: &mut TypedFuse, fh: u64) -> Result<(), i32> {
        let state = fs.open_files[&fh].clone();
        let mut handle = state.lock();
        if let Some(result) = &handle.completed {
            return result.as_ref().map(|_| ()).map_err(|(errno, _)| *errno);
        }
        if !handle.dirty {
            return Ok(());
        }
        let bytes = std::mem::replace(&mut handle.bytes, Buffer::empty(&fs.budget));
        let node = handle.node.clone();
        let surface = handle.surface.clone();
        handle.dirty = false;
        handle.running = true;
        drop(handle);
        complete(
            &state,
            execute(
                &fs.store,
                &fs.namespace.target,
                fs.lungs.as_ref(),
                surface.as_deref(),
                &node,
                bytes,
                &fs.budget,
                &fs.cancelled,
            ),
        );
        let handle = state.lock();
        handle
            .completed
            .as_ref()
            .map(|result| result.as_ref().map(|_| ()).map_err(|(errno, _)| *errno))
            .unwrap_or(Ok(()))
    }
    fn handle_json(fs: &TypedFuse, fh: u64) -> Value {
        serde_json::from_slice(fs.open_files[&fh].lock().readable_bytes()).unwrap()
    }
    #[test]
    fn zero_ttl_private_calls_and_public_schema() {
        let (_directory, fs) = lungs_fixture("/nonexistent/sid1".into());
        let call = method_node(&fs, "call");
        assert_eq!(fs.attr(call, None).unwrap().perm, 0o600);
        assert_eq!(
            fs.attr(method_node(&fs, "input.schema.json"), None)
                .unwrap()
                .perm,
            0o444
        );
        assert_eq!(TTL, Duration::ZERO);
        assert!(
            fs.namespace
                .lookup(call.parent, OsStr::new("result"))
                .is_none()
        );
        assert!(
            fs.namespace
                .lookup(call.parent, OsStr::new("error"))
                .is_none()
        );
    }
    #[test]
    fn terminal_errors_are_same_fd_only_and_empty_handles_never_execute() {
        let (_directory, mut fs) = lungs_fixture("/nonexistent/sid1".into());
        let first = stage_call(&mut fs, b"not json");
        let second = stage_call(&mut fs, br#"{"event":7}"#);
        let empty = stage_call(&mut fs, b"");
        assert!(fs.open_files[&first].lock().readable_bytes().is_empty());
        assert_eq!(run_staged(&mut fs, first), Err(EINVAL));
        assert_eq!(run_staged(&mut fs, second), Err(EINVAL));
        assert_eq!(handle_json(&fs, first)["stage"], "parse");
        assert_eq!(handle_json(&fs, second)["stage"], "validate");
        assert_eq!(run_staged(&mut fs, first), Err(EINVAL));
        assert_eq!(run_staged(&mut fs, empty), Ok(()));
        assert!(fs.open_files[&empty].lock().completed.is_none());
        assert!(
            fs.open_files[&first]
                .lock()
                .check(method_node(&fs, "call").ino, fs.uid, true)
                .is_err()
        );
    }
    #[test]
    fn delivered_sid1_and_socket_modes_preserve_bootstrap_stages() {
        let (_directory, mut fs) = lungs_fixture("/nonexistent/sid1".into());
        let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        assert_eq!(handle_json(&fs, fh)["stage"], "credential");
        let directory = tempdir().unwrap();
        let credential = directory.path().join("sid1");
        std::fs::write(&credential, b"OIA1-not-a-session").unwrap();
        let (_fixture, mut fs) = lungs_fixture(credential.clone());
        let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        std::fs::write(&credential, b"SID1 fixture envelope").unwrap();
        let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
        assert_eq!(run_staged(&mut fs, fh), Err(EIO));
        assert_eq!(handle_json(&fs, fh)["stage"], "transport");
        let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: "/nonexistent/lungs.sock".into(),
            session: "session".into(),
        });
        let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
        assert_eq!(run_staged(&mut fs, fh), Err(EIO));
        assert_eq!(handle_json(&fs, fh)["stage"], "transport");
    }
    #[test]
    fn mount_modes_uuid_and_local_owner_survive_refresh() {
        for (lungs, identity) in [
            (
                Lungs::Socket {
                    path: "/nonexistent/lungs".into(),
                    session: "declared-session".into(),
                },
                "host_session",
            ),
            (
                Lungs::Socket {
                    path: "/nonexistent/lungs".into(),
                    session: String::new(),
                },
                "guest_kernel",
            ),
            (
                Lungs::Credential {
                    bus_address: "unix:path=/nonexistent".into(),
                    credential: "/nonexistent/sid1".into(),
                },
                "credential",
            ),
        ] {
            let (_directory, mut fs) = lungs_fixture_with(lungs);
            let instance = fs.namespace.instance_uuid.clone().unwrap();
            uuid::Uuid::parse_str(&instance).unwrap();
            for _ in 0..2 {
                let node = fs
                    .namespace
                    .lookup(ROOT_INO, OsStr::new(".3tchedfs.json"))
                    .unwrap();
                let value: Value =
                    serde_json::from_slice(&fs.node_bytes(node).unwrap().bytes).unwrap();
                assert_eq!(value["instance_uuid"], instance);
                assert_eq!(value["mount_identity"]["kind"], identity);
                assert_eq!(value["local_call_owner_uid"], fs.uid);
                assert!(value.get("credential").is_none());
                fs.refresh_namespace().unwrap();
            }
        }
    }
    #[test]
    fn handles_enforce_access_uid_and_keep_descriptor_after_shape_change() {
        let (_directory, mut fs) = lungs_fixture("/nonexistent/sid1".into());
        let node = method_node(&fs, "input.schema.json").clone();
        let fh = fs.open_handle(node.ino, libc::O_RDONLY, fs.uid).unwrap();
        let state = fs.open_files[&fh].clone();
        assert_eq!(state.lock().check(node.ino, fs.uid, true), Err(EBADF));
        assert_eq!(
            state.lock().check(node.ino, fs.uid.wrapping_add(1), false),
            Err(EACCES)
        );
        fs.namespace.nodes.remove(&node.ino);
        assert_eq!(state.lock().node.path, node.path);
        assert!(!state.lock().readable_bytes().is_empty());
        let ino = method_node(&fs, "call").ino;
        let fh = fs.open_handle(ino, libc::O_WRONLY, fs.uid).unwrap();
        assert_eq!(
            fs.open_files[&fh].lock().check(ino, fs.uid, false),
            Err(EBADF)
        );
        let stranger = fs.uid.wrapping_add(7);
        assert_eq!(fs.open_handle(ino, libc::O_RDWR, stranger), Err(EACCES));
        assert!(!fs.open_files.contains_key(&u64::MAX));
    }
    #[test]
    fn buffer_limits_and_aggregate_charges_follow_moves() {
        let budget = Arc::new(Budget::default());
        let mut buffer = Buffer::empty(&budget);
        assert_eq!(
            buffer.resize(INPUT_LIMIT + 1, INPUT_LIMIT),
            Err(libc::EFBIG)
        );
        buffer.resize(INPUT_LIMIT, INPUT_LIMIT).unwrap();
        let mut held = Vec::new();
        for _ in 0..3 {
            held.push(budget.take(DOCUMENT_LIMIT, TOTAL_LIMIT).unwrap());
        }
        assert!(budget.take(DOCUMENT_LIMIT, TOTAL_LIMIT).is_err());
        drop(buffer);
        let full = budget.take(DOCUMENT_LIMIT, TOTAL_LIMIT).unwrap();
        assert!(budget.take(1, TOTAL_LIMIT).is_err());
        drop(full);
        assert!(budget.take(1, TOTAL_LIMIT).is_ok());
        let jobs = Arc::new(Budget::default());
        let permits: Vec<_> = (0..JOB_LIMIT)
            .map(|_| jobs.take(1, JOB_LIMIT).unwrap())
            .collect();
        assert!(jobs.take(1, JOB_LIMIT).is_err());
        drop(permits);
        assert!(jobs.take(1, JOB_LIMIT).is_ok());
    }
    fn workspace_fixture() -> (tempfile::TempDir, TypedFuse, u64) {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let schema = PluginSchema {
            name: "demo".into(),
            version: "1".into(),
            public_state: serde_json::json!({"paths":["/x"]}),
            fields: BTreeMap::from([(
                "x".into(),
                crate::model::FieldSchema {
                    field_type: crate::model::FieldType::Integer,
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: false,
                    read_only_when: None,
                },
            )]),
            ..Default::default()
        };
        let object = TypedObjectBlob::new(
            "demo".into(),
            "a".repeat(64),
            schema,
            serde_json::json!({"x":1}),
        )
        .unwrap();
        store
            .capture_snapshot(
                SourceSnapshot {
                    catalog_hash: "catalog".into(),
                    catalog_generation: 1,
                    objects: BTreeMap::from([("demo".into(), object)]),
                },
                true,
            )
            .unwrap();
        store.create_workspace(Some("edit"), None).unwrap();
        let fs = TypedFuse::new(store, ProjectionTarget::Workspace("edit".into())).unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| {
                node.write_target
                    .as_ref()
                    .is_some_and(|target| target.pointer == "/x")
            })
            .unwrap()
            .ino;
        (directory, fs, ino)
    }
    #[test]
    fn workspace_guard_and_atomic_conflict_do_not_overwrite_newer_state() {
        let (_directory, mut fs, ino) = workspace_fixture();
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        assert_eq!(fs.open_handle(ino, libc::O_RDWR, fs.uid), Err(EBUSY));
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(1, DOCUMENT_LIMIT).unwrap();
            handle.bytes.bytes[0] = b'2';
            handle.dirty = true;
        }
        fs.store
            .stage_pointer("edit", "demo", "/x", Value::from(3))
            .unwrap();
        assert_eq!(run_staged(&mut fs, fh), Err(ESTALE));
        let workspace = fs.store.read_workspace("edit").unwrap();
        assert_eq!(
            fs.store.workspace_object(&workspace, "demo").unwrap().value["x"],
            3
        );
        fs.open_files.remove(&fh);
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        for replacement in [b'4', b'5'] {
            {
                let mut handle = fs.open_files[&fh].lock();
                handle.bytes.resize(1, DOCUMENT_LIMIT).unwrap();
                handle.bytes.bytes[0] = replacement;
                handle.dirty = true;
            }
            assert_eq!(run_staged(&mut fs, fh), Ok(()));
        }
        assert_eq!(
            fs.store
                .workspace_object(&fs.store.read_workspace("edit").unwrap(), "demo")
                .unwrap()
                .value["x"],
            5
        );
        fs.refresh_namespace().unwrap();
        assert_eq!(
            fs.attr(fs.namespace.node(ino).unwrap(), None).unwrap().perm,
            0o600
        );
    }
    struct FixtureBridge {
        entered: std::sync::mpsc::Sender<()>,
        resume: Arc<tokio::sync::Notify>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[zbus::interface(name = "org.opdbus.v1.PluginV1")]
    impl FixtureBridge {
        async fn call_with_operation_id(
            &self,
            method: String,
            json_args: String,
            operation_id: String,
        ) -> String {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.send(()).unwrap();
            self.resume.notified().await;
            serde_json::json!({"success":true,"operation_id":operation_id,"outcome":"completed",
                "event_id":7,"event_hash":"a".repeat(32),"terminal_event_id":8,"terminal_event_hash":"b".repeat(32),
                "plugin_id":"demo","method":method,"result":{"method":method,"args":json_args}}).to_string()
        }
    }
    struct FixtureLungs {
        bound: Arc<Mutex<Option<String>>>,
        refuse: bool,
    }
    #[zbus::interface(name = "org.opdbus.v1.LungsV1")]
    impl FixtureLungs {
        async fn bind(&self, session_id: String) -> zbus::fdo::Result<bool> {
            if self.refuse {
                return Err(zbus::fdo::Error::AccessDenied("fixture refusal".into()));
            }
            *self.bound.lock() = Some(session_id);
            Ok(true)
        }
        async fn bind_current_container(&self) -> bool {
            *self.bound.lock() = Some("kernel-container".into());
            true
        }
    }
    fn reply() -> (PendingReply, std::sync::mpsc::Receiver<i32>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (PendingReply::Test(sender), receiver)
    }
    #[test]
    fn async_dispatch_duplicate_flush_release_and_guest_bind_are_one_operation() {
        for session in ["", "named-session"] {
            let directory = tempdir().unwrap();
            let socket = directory.path().join("lungs.sock");
            let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let (entered, received) = std::sync::mpsc::channel();
            let resume = Arc::new(tokio::sync::Notify::new());
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let bound = Arc::new(Mutex::new(None));
            let (server_resume, server_calls, server_bound) =
                (resume.clone(), calls.clone(), bound.clone());
            let server = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async move {
                    let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                    let (stream, _) = listener.accept().await.unwrap();
                    let connection = zbus::connection::Builder::unix_stream(stream)
                        .server(zbus::Guid::generate())
                        .unwrap()
                        .p2p()
                        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
                        .serve_at(
                            "/org/opdbus/v1/plugins/demo",
                            FixtureBridge {
                                entered,
                                resume: server_resume,
                                calls: server_calls,
                            },
                        )
                        .unwrap()
                        .serve_at(
                            "/org/opdbus/v1/lungs",
                            FixtureLungs {
                                bound: server_bound,
                                refuse: false,
                            },
                        )
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    connection.closed().await;
                });
            });
            let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
                path: socket,
                session: session.into(),
            });
            let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
            let ino = method_node(&fs, "call").ino;
            let state = fs.open_files[&fh].clone();
            let (first, first_rx) = reply();
            fs.submit(ino, fh, fs.uid, first, false);
            // If submit blocks on the reply this test cannot reach the resume.
            received.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(state.lock().check(ino, fs.uid, false), Err(EAGAIN));
            let schema = method_node(&fs, "input.schema.json").ino;
            assert!(fs.open_handle(schema, libc::O_RDONLY, fs.uid).is_ok());
            let (second, second_rx) = reply();
            fs.submit(ino, fh, fs.uid, second, false);
            let (released, released_rx) = reply();
            fs.submit(ino, fh, fs.uid, released, true);
            assert!(!fs.open_files.contains_key(&fh));
            assert!(first_rx.try_recv().is_err());
            resume.notify_one();
            for rx in [first_rx, second_rx, released_rx] {
                assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
            }
            let receipt: Value = serde_json::from_slice(state.lock().readable_bytes()).unwrap();
            assert_eq!(receipt["result"]["result"]["method"], "record_event");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                bound.lock().as_deref(),
                Some(if session.is_empty() {
                    "kernel-container"
                } else {
                    session
                })
            );
            server.join().unwrap();
        }
    }
    #[test]
    fn local_owner_assignment_requires_operator_and_handles_stay_private() {
        let (_directory, fs) = lungs_fixture("/nonexistent/sid1".into());
        let daemon_uid = fs.uid;
        let owner = daemon_uid.wrapping_add(7);
        if daemon_uid == 0 {
            let mut fs = fs.with_call_owner_uid(owner).unwrap();
            let ino = method_node(&fs, "call").ino;
            assert_eq!(fs.attr(method_node(&fs, "call"), None).unwrap().uid, owner);
            let fh = fs.open_handle(ino, libc::O_RDWR, owner).unwrap();
            assert_eq!(
                fs.open_files[&fh].lock().check(ino, daemon_uid, false),
                Err(EACCES)
            );
            assert!(fs.open_handle(ino, libc::O_RDWR, daemon_uid).is_ok());
            fs.refresh_namespace().unwrap();
            assert_eq!(fs.namespace.local_call_owner_uid, Some(owner));
        } else {
            assert!(fs.with_call_owner_uid(owner).is_err());
        }
    }
    #[test]
    fn socket_refusal_is_terminal_before_plugin_execution() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("lungs.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let connection = zbus::connection::Builder::unix_stream(stream)
                    .server(zbus::Guid::generate())
                    .unwrap()
                    .p2p()
                    .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
                    .serve_at(
                        "/org/opdbus/v1/lungs",
                        FixtureLungs {
                            bound: Arc::new(Mutex::new(None)),
                            refuse: true,
                        },
                    )
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                connection.closed().await;
            });
        });
        let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: socket,
            session: "named-session".into(),
        });
        let fh = stage_call(&mut fs, br#"{"event":"arrival"}"#);
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        assert_eq!(handle_json(&fs, fh)["stage"], "register");
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        server.join().unwrap();
    }

    #[test]
    fn sealed_threetched_call_without_fixture_surface_requires_lungs_admission() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("sealed-lungs.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let connection = zbus::connection::Builder::unix_stream(stream)
                    .server(zbus::Guid::generate()).unwrap().p2p()
                    .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
                    .serve_at("/org/opdbus/v1/lungs", FixtureLungs {
                        bound: Arc::new(Mutex::new(None)), refuse: true,
                    }).unwrap().build().await.unwrap();
                connection.closed().await;
            });
        });
        let (_fixture, mut fs) = lungs_fixture_plugin(Lungs::Socket {
            path: socket, session: "named-session".into(),
        }, "threetched");
        assert!(fs.namespace.threetched_surface.is_none());
        let ino = fs.namespace.nodes.values()
            .find(|node| node.path == "/plugins/threetched/operations/record_event/call")
            .unwrap().ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        {
            let mut handle = fs.open_files[&fh].lock();
            let bytes = br#"{"event":"arrival"}"#;
            handle.bytes.resize(bytes.len(), INPUT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(bytes);
            handle.dirty = true;
        }
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        assert_eq!(handle_json(&fs, fh)["stage"], "register");
        assert_eq!(run_staged(&mut fs, fh), Err(EACCES));
        server.join().unwrap();
    }
    #[test]
    fn fresh_opens_follow_commits_without_invalidating_old_handle() {
        let (directory, workspace, _) = workspace_fixture();
        let state = directory.path().join("state");
        std::fs::create_dir(&state).unwrap();
        let publish = |x: i32| {
            op_core::projection_shm::update_projection_in(state.to_str().unwrap(), "demo", |_| {
                Ok(Some(
                    serde_json::to_vec(&serde_json::json!({"x":x})).unwrap(),
                ))
            })
            .unwrap();
        };
        publish(1);
        let base = workspace
            .store
            .read_view(&workspace.store.current_view_id().unwrap().unwrap())
            .unwrap();
        let schema = workspace
            .store
            .object_for_view(&base, "demo")
            .unwrap()
            .schema;
        let catalog = crate::source::tests::fixture_catalog(directory.path(), &schema);
        let view = workspace
            .store
            .capture_snapshot(catalog.capture_schema_only().unwrap(), false)
            .unwrap();
        let target = ProjectionTarget::View(view.view_id);
        let mut fs = TypedFuse::with_state_dir(workspace.store.clone(), target, state.clone())
            .unwrap()
            .with_catalog_dir(directory.path().join("catalog"))
            .unwrap();
        let data = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/demo/data")
            .unwrap()
            .ino;
        fs.namespace.refresh_for_inode(&fs.store, data).unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| matches!(&node.kind,NodeKind::LiveFile {pointer,..} if pointer=="/x"))
            .unwrap()
            .ino;
        let first = fs.open_handle(ino, libc::O_RDONLY, fs.uid).unwrap();
        assert_eq!(fs.open_files[&first].lock().readable_bytes(), b"1\n");
        publish(2);
        let second = fs.open_handle(ino, libc::O_RDONLY, fs.uid).unwrap();
        assert_eq!(fs.open_files[&second].lock().readable_bytes(), b"2\n");
        assert_eq!(fs.open_files[&first].lock().readable_bytes(), b"1\n");
        std::fs::remove_file(state.join("demo.json")).unwrap();
        assert!(fs.open_handle(ino, libc::O_RDONLY, fs.uid).is_err());
        assert_eq!(fs.open_files[&first].lock().readable_bytes(), b"1\n");
    }

    #[test]
    #[ignore = "explicit disposable kernel FUSE check; requires root and /dev/fuse"]
    fn upstream_kernel_nonroot_handle_lifecycle() {
        use std::os::unix::{fs::PermissionsExt, process::CommandExt};
        assert_eq!(unsafe { libc::geteuid() }, 0, "run explicitly as root");
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = crate::threetched_surface::test_fixture_binary(directory.path());
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        let slots = fs.slots.clone();
        let budget = fs.budget.clone();
        let jobs = fs.jobs.clone();
        let mountpoint = directory.path().join("mount");
        std::fs::create_dir(&mountpoint).unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let session = fuser::spawn_mount2(
            fs,
            &mountpoint,
            &[
                fuser::MountOption::FSName("threetched-inode-fixture".into()),
                fuser::MountOption::AllowOther,
                fuser::MountOption::DefaultPermissions,
                fuser::MountOption::NoDev,
                fuser::MountOption::NoSuid,
                fuser::MountOption::NoExec,
            ],
        )
        .unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fuse_fs::tests::upstream_kernel_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("THREETCHED_INODE_TEST_MOUNT", &mountpoint)
            .uid(1001)
            .gid(1001)
            .status()
            .unwrap();
        drop(session);
        assert!(status.success());
        // Session teardown drops all remaining namespace handles and charges.
        drop(slots.take(HANDLE_LIMIT, HANDLE_LIMIT).unwrap());
        drop(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap());
        drop(jobs.take(JOB_LIMIT, JOB_LIMIT).unwrap());
    }

    #[test]
    #[ignore = "child probe of upstream_kernel_nonroot_handle_lifecycle"]
    fn upstream_kernel_probe() {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(unsafe { libc::geteuid() }, 1001);
        let mount = PathBuf::from(std::env::var_os("THREETCHED_INODE_TEST_MOUNT").unwrap());
        let directory = mount.join("plugins/threetched/operations/self-test");
        let schema = directory.join("input.schema.json");
        let ino = std::fs::metadata(&schema).unwrap().ino();
        for _ in 0..512 {
            let names: Vec<_> = std::fs::read_dir(&directory)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert!(names.contains(&OsString::from("call")));
            assert_eq!(std::fs::metadata(&schema).unwrap().ino(), ino);
            serde_json::from_slice::<Value>(&std::fs::read(&schema).unwrap()).unwrap();
        }
        for _ in 0..4 {
            let handles: Vec<_> = (0..HANDLE_LIMIT)
                .map(|_| std::fs::File::open(&schema).unwrap())
                .collect();
            assert_eq!(
                std::fs::File::open(&schema).unwrap_err().raw_os_error(),
                Some(libc::ENOMEM)
            );
            drop(handles);
            // RELEASE may follow close asynchronously in the kernel.
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                match std::fs::read(&schema) {
                    Ok(_) => break,
                    Err(error)
                        if error.raw_os_error() == Some(libc::ENOMEM)
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("handle slots did not drain: {error}"),
                }
            }
        }
    }

    #[test]
    fn upstream_open_pins_surface_and_kernel_release_drains_exact_limit() {
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = crate::threetched_surface::test_fixture_binary(directory.path());
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        fs = fs.with_call_owner_uid(unsafe { libc::getuid() }).unwrap();
        let path = "/plugins/threetched/operations/self-test/call";
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == path)
            .unwrap()
            .ino;
        let surface = fs.namespace.threetched_surface.clone().unwrap();
        for _ in 0..3 {
            let mut handles = Vec::new();
            for _ in 0..HANDLE_LIMIT {
                handles.push(fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap());
            }
            assert_eq!(fs.open_handle(ino, libc::O_RDWR, fs.uid), Err(libc::ENOMEM));
            fs.refresh_namespace().unwrap();
            assert_eq!(fs.namespace.nodes[&ino].path, path);
            for fh in handles {
                assert!(Arc::ptr_eq(
                    fs.open_files[&fh].lock().surface.as_ref().unwrap(),
                    &surface
                ));
                let (sender, receiver) = std::sync::mpsc::channel();
                fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
                assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
            }
            drop(fs.slots.take(HANDLE_LIMIT, HANDLE_LIMIT).unwrap());
            drop(fs.budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap());
        }
    }

    #[test]
    fn upstream_rejected_fsync_stays_terminal_under_job_and_byte_exhaustion() {
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = crate::threetched_surface::test_fixture_binary(directory.path());
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/operations/self-test/call")
            .unwrap()
            .ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(2, INPUT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(b"{}");
            handle.dirty = true;
        }
        let jobs = fs.jobs.take(JOB_LIMIT, JOB_LIMIT).unwrap();
        // Leave no space for a freshly allocated diagnostic.
        let mut held = Vec::new();
        for amount in [DOCUMENT_LIMIT, 8192, 1] {
            while let Ok(charge) = fs.budget.take(amount, TOTAL_LIMIT) {
                held.push(charge);
            }
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, fs.uid, PendingReply::Test(sender), false);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            EBUSY
        );
        assert_eq!(handle_json(&fs, fh)["stage"], "admission");
        assert!(handle_json(&fs, fh)["operation_id"].is_null());
        drop((jobs, held));
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            EBUSY
        );
        assert!(fs.open_files.is_empty());
        drop(fs.slots.take(HANDLE_LIMIT, HANDLE_LIMIT).unwrap());
        drop(fs.budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap());
        drop(fs.jobs.take(JOB_LIMIT, JOB_LIMIT).unwrap());
    }

    #[test]
    fn upstream_cancelled_before_spawn_keeps_admission_when_diagnostics_cannot_allocate() {
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = crate::threetched_surface::test_fixture_binary(directory.path());
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/operations/self-test/call")
            .unwrap()
            .ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(2, INPUT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(b"{}");
            handle.dirty = true;
        }
        let mut held = Vec::new();
        for amount in [DOCUMENT_LIMIT, 8192, 1] {
            while let Ok(charge) = fs.budget.take(amount, TOTAL_LIMIT) {
                held.push(charge);
            }
        }
        fs.cancelled.store(true, Ordering::Release);
        assert_eq!(run_staged(&mut fs, fh), Err(EBUSY));
        assert_eq!(handle_json(&fs, fh)["stage"], "admission");
        drop(held);
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            EBUSY
        );
        drop(fs.budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap());
    }

    #[test]
    fn upstream_discovered_method_executes_exact_flags_and_positionals() {
        use std::os::unix::fs::PermissionsExt;
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = directory.path().join("discovered-cli");
        std::fs::write(
            &binary,
            r#"#!/bin/sh
case "$*" in
  '--version') printf 'fixture 1\n' ;;
  'config schema') printf '{"type":"object"}\n' ;;
  '--help')
    cat <<'EOF'
Usage: fixture <COMMAND>

Commands:
  widgets  Widget operations
EOF
    ;;
  'widgets --help')
    cat <<'EOF'
Usage: fixture widgets <COMMAND>

Commands:
  publish  Publish one widget
EOF
    ;;
  'widgets publish --help')
    cat <<'EOF'
Usage: fixture widgets publish [OPTIONS] <NAME>

Arguments:
  <NAME>  Widget name

Options:
      --format <FORMAT>  Wire format [possible values: json, yaml]
  -v, --verbose          Verbose output
  -h, --help             Print help
EOF
    ;;
  *) printf '%s\n' "$@" ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/operations/widgets.publish/call")
            .expect("discovered leaf becomes a method")
            .ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        let input = br#"{"--format":["json"],"--verbose":true,"arguments":["sample"]}"#;
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(input.len(), INPUT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(input);
            handle.dirty = true;
        }
        assert_eq!(run_staged(&mut fs, fh), Ok(()));
        assert_eq!(
            handle_json(&fs, fh)["stdout"],
            "widgets\npublish\n--format\njson\n--verbose\n--\nsample\n"
        );
        let surface = fs.namespace.threetched_surface.as_ref().unwrap();
        let invalid = run_threetched_call(
            "widgets.publish",
            br#"{"--invented":true,"arguments":["sample"]}"#,
            Some(surface),
            &fs.budget,
            &fs.cancelled,
        );
        assert!(matches!(invalid, Err(ThreetchedCallError::Arguments(_))));
    }

    #[test]
    fn upstream_exec_uses_captured_binary_and_retains_same_descriptor_receipt() {
        let (directory, mut fs) = lungs_fixture("/nonexistent".into());
        let binary = crate::threetched_surface::test_fixture_binary(directory.path());
        fs.graft_threetched(ThreetchedSurface::capture(&binary).unwrap())
            .unwrap();
        let ino = fs
            .namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/operations/self-test/call")
            .unwrap()
            .ino;
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(2, INPUT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(b"{}");
            handle.dirty = true;
        }
        // Discovery can change while this descriptor retains its original surface.
        fs.namespace.threetched_surface = None;
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, fs.uid, PendingReply::Test(sender), false);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(3)).unwrap(), 0);
        assert_eq!(handle_json(&fs, fh)["stdout"], "ok\n");
        assert_eq!(handle_json(&fs, fh)["exit"], 0);
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
    }

    #[test]
    fn workspace_rejected_fsync_cannot_commit_on_release() {
        let (_directory, mut fs, ino) = workspace_fixture();
        let fh = fs.open_handle(ino, libc::O_RDWR, fs.uid).unwrap();
        let before = fs
            .node_bytes(&fs.open_files[&fh].lock().node)
            .unwrap()
            .bytes;
        {
            let mut handle = fs.open_files[&fh].lock();
            handle.bytes.resize(2, DOCUMENT_LIMIT).unwrap();
            handle.bytes.bytes.copy_from_slice(b"99");
            handle.dirty = true;
        }
        let jobs = fs.jobs.take(JOB_LIMIT, JOB_LIMIT).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, fs.uid, PendingReply::Test(sender), false);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            EBUSY
        );
        drop(jobs);
        let (sender, receiver) = std::sync::mpsc::channel();
        fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            EBUSY
        );
        fs.refresh_namespace().unwrap();
        assert_eq!(
            fs.node_bytes(fs.namespace.node(ino).unwrap())
                .unwrap()
                .bytes,
            before
        );
        drop(fs.budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap());
    }

    #[test]
    fn kernel_release_reclaims_nonroot_handles_without_weakening_io_checks() {
        let (_directory, mut fs) = lungs_fixture(PathBuf::from("/nonexistent"));
        let ino = method_node(&fs, "input.schema.json").ino;
        let owner = 1001;
        for _ in 0..HANDLE_LIMIT * 2 {
            let fh = fs.open_handle(ino, libc::O_RDONLY, owner).unwrap();
            assert_eq!(fs.open_files[&fh].lock().check(ino, 0, false), Err(EACCES));
            let (sender, receiver) = std::sync::mpsc::channel();
            fs.submit(ino, fh, 0, PendingReply::Test(sender), false);
            assert_eq!(receiver.recv().unwrap(), EACCES);
            assert!(fs.open_files.contains_key(&fh));
            let (sender, receiver) = std::sync::mpsc::channel();
            fs.submit(ino.wrapping_add(1), fh, 0, PendingReply::Test(sender), true);
            assert_eq!(receiver.recv().unwrap(), EACCES);
            assert!(fs.open_files.contains_key(&fh));
            let (sender, receiver) = std::sync::mpsc::channel();
            fs.submit(ino, fh, 0, PendingReply::Test(sender), true);
            assert_eq!(receiver.recv().unwrap(), 0);
            assert!(fs.open_files.is_empty());
        }
        let slots = fs.slots.take(HANDLE_LIMIT, HANDLE_LIMIT).unwrap();
        let bytes = fs.budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap();
        drop((slots, bytes));
    }
}
