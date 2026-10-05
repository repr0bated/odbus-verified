use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use fuser::{
    FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyWrite, Request, TimeOrNow,
};
use libc::{EACCES, EINVAL, EIO, EISDIR, ENOENT, EROFS};
use serde_json::Value;

use crate::dispatch::{self, CallReceipt, LungsError};
use crate::namespace::{Namespace, Node, NodeKind, ProjectionTarget};
use crate::store::ObjectStore;

/// Kernel cache lifetime for a pinned view. Its tree never changes, and file
/// contents stay live because every open uses direct I/O. Workspaces rebuild
/// their tree on write, so they are never cached.
const VIEW_TTL: Duration = Duration::from_secs(1);

/// Identifies one committed version of a state file. MutationEngine replaces
/// the file on every commit, so a new commit always yields a new key.
#[derive(Clone, Copy, PartialEq, Eq)]
struct StateKey {
    dev: u64,
    ino: u64,
    mtime: i64,
    mtime_nsec: i64,
    len: u64,
}

/// Rendered live-file bytes by (plugin, pointer), each tagged with the state
/// version it was rendered from.
type LiveCache = HashMap<(String, String), (StateKey, Vec<u8>)>;

impl StateKey {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            len: metadata.len(),
        }
    }
}

type OperationCompletion = Option<Result<Vec<u8>, (i32, Vec<u8>)>>;

struct OpenFile {
    ino: u64,
    bytes: Vec<u8>,
    dirty: bool,
    /// Terminal receipt/error for this handle, isolated from other callers.
    completed: OperationCompletion,
}

/// Where a lungs mount sends its calls, and whose credential it presents.
/// The credential lives on the identity's own device, outside the container.
/// How a lungs mount reaches the bridge.
pub enum Lungs {
    /// The bridge's shared lungs socket. Each connection binds to this
    /// identity's session before calling, so no credential is stored. An empty
    /// session requests kernel authentication of the current container.
    Socket { path: PathBuf, session: String },
    /// Register a SID1 read from the identity's device on the shared bus.
    Credential {
        bus_address: String,
        credential: PathBuf,
    },
}

#[derive(Default)]
struct Outcome {
    result: Vec<u8>,
    error: Vec<u8>,
}

pub struct TypedFuse {
    store: ObjectStore,
    namespace: Namespace,
    open_files: HashMap<u64, OpenFile>,
    next_fh: u64,
    uid: u32,
    gid: u32,
    lungs: Option<Lungs>,
    outcomes: HashMap<(String, String), Outcome>,
    ttl: Duration,
    /// Rendered live files, keyed by (plugin, pointer), valid for one state version.
    live_cache: parking_lot::Mutex<LiveCache>,
}

impl TypedFuse {
    pub fn new(store: ObjectStore, target: ProjectionTarget) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, None)?;
        Ok(Self::from_parts(store, namespace, None))
    }

    pub fn with_state_dir(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, Some(&state_dir))?;
        Ok(Self::from_parts(store, namespace, None))
    }

    /// A live view owned by one identity: every method gets a `call` file
    /// whose writes are dispatched as that identity.
    pub fn lungs(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
        lungs: Lungs,
    ) -> anyhow::Result<Self> {
        let mut namespace = Namespace::build_lungs(&store, target, &state_dir)?;
        if let Lungs::Socket { session, .. } = &lungs
            && !session.is_empty()
        {
            namespace.declare_socket_session(session)?;
        }
        Ok(Self::from_parts(store, namespace, Some(lungs)))
    }

    fn from_parts(store: ObjectStore, namespace: Namespace, lungs: Option<Lungs>) -> Self {
        let ttl = match namespace.target {
            ProjectionTarget::View(_) => VIEW_TTL,
            ProjectionTarget::Workspace(_) => Duration::ZERO,
        };
        Self {
            store,
            namespace,
            open_files: HashMap::new(),
            next_fh: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            lungs,
            outcomes: HashMap::new(),
            ttl,
            live_cache: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    fn outcome_bytes(&self, plugin_id: &str, method: &str, error: bool) -> Vec<u8> {
        self.outcomes
            .get(&(plugin_id.to_string(), method.to_string()))
            .map(|outcome| {
                if error {
                    outcome.error.clone()
                } else {
                    outcome.result.clone()
                }
            })
            .unwrap_or_default()
    }

    /// Run one lungs call and record its receipt or diagnostic. The errno
    /// tells the writer which stage failed; the `error` file says why.
    fn lungs_call(
        &mut self,
        plugin_id: &str,
        method: &str,
        bytes: &[u8],
    ) -> Result<Vec<u8>, (i32, Vec<u8>)> {
        let outcome = self.run_lungs_call(plugin_id, method, bytes);
        let entry = self
            .outcomes
            .entry((plugin_id.to_string(), method.to_string()))
            .or_default();
        match outcome {
            Ok(receipt) => {
                entry.result = pretty(&receipt);
                entry.error.clear();
                Ok(entry.result.clone())
            }
            Err(error) => {
                entry.result.clear();
                entry.error = pretty(&serde_json::json!({
                    "plugin": plugin_id,
                    "method": method,
                    "stage": error.stage(),
                    "error": error.to_string(),
                }));
                Err((error.errno(), entry.error.clone()))
            }
        }
    }

    fn run_lungs_call(
        &self,
        plugin_id: &str,
        method_name: &str,
        bytes: &[u8],
    ) -> Result<CallReceipt, LungsError> {
        let lungs = self
            .lungs
            .as_ref()
            .ok_or_else(|| LungsError::Transport("not a lungs mount".into()))?;
        let arguments: Value =
            serde_json::from_slice(bytes).map_err(|error| LungsError::Json(error.to_string()))?;
        let method =
            dispatch::method_decl(&self.store, &self.namespace.target, plugin_id, method_name)
                .map_err(|error| LungsError::Arguments(error.to_string()))?;
        dispatch::validate_arguments(&method, &arguments)?;
        let credential = match lungs {
            Lungs::Socket { .. } => None,
            Lungs::Credential { credential, .. } => Some(dispatch::read_credential(credential)?),
        };
        // FUSE callbacks run inside the mount's tokio context, which cannot
        // block on another future. A scoped thread with its own runtime keeps
        // the D-Bus round trip off that context.
        std::thread::scope(|scope| {
            scope
                .spawn(move || {
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
                            (Lungs::Credential { .. }, None) => {
                                Err(LungsError::Credential("credential was not read".into()))
                            }
                        }
                    })
                })
                .join()
                .unwrap_or_else(|_| Err(LungsError::Transport("dispatch thread panicked".into())))
        })
    }

    fn attr(&self, node: &Node, size_override: Option<u64>) -> FileAttr {
        let (kind, size, perm, nlink) = match &node.kind {
            NodeKind::Directory => (FileType::Directory, 0, 0o555, 2),
            NodeKind::File(bytes) => (
                FileType::RegularFile,
                size_override.unwrap_or_else(|| bytes.read().len() as u64),
                if node.write_target.is_some() {
                    0o644
                } else {
                    0o444
                },
                1,
            ),
            NodeKind::LiveFile { .. } => {
                let size = size_override.unwrap_or_else(|| {
                    self.live_state_bytes(node)
                        .map(|b| b.len() as u64)
                        .unwrap_or(0)
                });
                (FileType::RegularFile, size, 0o444, 1)
            }
            NodeKind::Symlink(target) => (FileType::Symlink, target.len() as u64, 0o777, 1),
            // Receipts are readable only through the handle that ran the call.
            NodeKind::CallFile { .. } => {
                (FileType::RegularFile, size_override.unwrap_or(0), 0o666, 1)
            }
            NodeKind::Outcome {
                plugin_id,
                method,
                error,
            } => {
                let size = size_override
                    .unwrap_or_else(|| self.outcome_bytes(plugin_id, method, *error).len() as u64);
                (FileType::RegularFile, size, 0o444, 1)
            }
        };
        FileAttr {
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
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            flags: 0,
            blksize: 4096,
        }
    }

    /// Read a plugin's live state from SHM and extract the value at the given
    /// JSON pointer, as pretty-printed JSON bytes. Rendering happens once per
    /// committed state version; later opens and size checks reuse it.
    fn live_state_bytes(&self, node: &Node) -> Option<Vec<u8>> {
        let (plugin_id, pointer) = match &node.kind {
            NodeKind::LiveFile { plugin_id, pointer } => (plugin_id.as_str(), pointer.as_str()),
            _ => return None,
        };
        let state_dir = self.namespace.state_dir.as_ref()?;
        let path = state_dir.join(format!("{plugin_id}.json"));
        let key = StateKey::of(&std::fs::metadata(&path).ok()?);
        let cache_key = (plugin_id.to_string(), pointer.to_string());
        if let Some((cached, bytes)) = self.live_cache.lock().get(&cache_key)
            && *cached == key
        {
            return Some(bytes.clone());
        }
        let raw = std::fs::read(&path).ok()?;
        let value: Value = serde_json::from_slice(&raw).ok()?;
        let extracted = if pointer.is_empty() {
            value
        } else {
            value.pointer(pointer).cloned().unwrap_or(Value::Null)
        };
        let mut bytes = serde_json::to_vec_pretty(&extracted).ok()?;
        bytes.push(b'\n');
        // A commit that lands between the stat and the read is stored under
        // the older key, so the next open simply renders it again.
        self.live_cache
            .lock()
            .insert(cache_key, (key, bytes.clone()));
        Some(bytes)
    }

    fn refresh_namespace(&mut self) -> anyhow::Result<()> {
        let state_dir = self.namespace.state_dir.clone();
        self.namespace = Namespace::build_with(
            &self.store,
            self.namespace.target.clone(),
            state_dir.as_deref(),
            self.namespace.lungs,
        )?;
        if let Some(Lungs::Socket { session, .. }) = &self.lungs
            && !session.is_empty()
        {
            self.namespace.declare_socket_session(session)?;
        }
        Ok(())
    }

    fn flush_handle(&mut self, fh: u64) -> Result<(), i32> {
        let Some(handle) = self.open_files.get_mut(&fh) else {
            return Err(ENOENT);
        };
        if let Some(completed) = &handle.completed {
            return completed.as_ref().map(|_| ()).map_err(|(errno, _)| *errno);
        }
        if !handle.dirty {
            return Ok(());
        }
        let node = self.namespace.node(handle.ino).ok_or(ENOENT)?;
        if let NodeKind::CallFile { plugin_id, method } = &node.kind {
            // Clear dirty first: release flushes again, and a call must run
            // once per complete write even when it fails.
            handle.dirty = false;
            if handle.bytes.is_empty() {
                return Ok(());
            }
            let bytes = std::mem::take(&mut handle.bytes);
            let (plugin_id, method) = (plugin_id.clone(), method.clone());
            let completed = self.lungs_call(&plugin_id, &method, &bytes);
            let result = completed.as_ref().map(|_| ()).map_err(|(errno, _)| *errno);
            self.open_files.get_mut(&fh).ok_or(ENOENT)?.completed = Some(completed);
            return result;
        }
        let target = node.write_target.clone().ok_or(EROFS)?;
        let value: Value = serde_json::from_slice(&handle.bytes).map_err(|_| EINVAL)?;
        self.store
            .stage_pointer(
                &target.workspace_id,
                &target.plugin_id,
                &target.pointer,
                value,
            )
            .map_err(|_| EINVAL)?;
        handle.dirty = false;
        self.refresh_namespace().map_err(|_| EIO)?;
        Ok(())
    }

    fn read_bytes(&self, ino: u64, fh: u64) -> Result<Vec<u8>, i32> {
        if let Some(handle) = self.open_files.get(&fh)
            && handle.ino == ino
        {
            if matches!(
                self.namespace.node(ino).ok_or(ENOENT)?.kind,
                NodeKind::CallFile { .. }
            ) {
                return Ok(match &handle.completed {
                    Some(Ok(receipt)) => receipt.clone(),
                    Some(Err((_, diagnostic))) => diagnostic.clone(),
                    None => Vec::new(),
                });
            }
            return Ok(handle.bytes.clone());
        }
        match &self.namespace.node(ino).ok_or(ENOENT)?.kind {
            NodeKind::File(bytes) => Ok(bytes.read().clone()),
            NodeKind::LiveFile { .. } => {
                let node = self.namespace.node(ino).ok_or(ENOENT)?;
                self.live_state_bytes(node).ok_or(EIO)
            }
            NodeKind::Directory => Err(EISDIR),
            NodeKind::Symlink(_) => Err(EINVAL),
            NodeKind::CallFile { .. } => Ok(Vec::new()),
            NodeKind::Outcome {
                plugin_id,
                method,
                error,
            } => Ok(self.outcome_bytes(plugin_id, method, *error)),
        }
    }
}

fn pretty(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

/// A workspace field or a lungs call file accepts writes; nothing else does.
fn node_writable(node: &Node) -> bool {
    node.write_target.is_some() || matches!(node.kind, NodeKind::CallFile { .. })
}

impl Filesystem for TypedFuse {
    fn init(&mut self, _request: &Request<'_>, config: &mut KernelConfig) -> Result<(), i32> {
        // Typed files must be parsed and committed only after a complete
        // write. Ask the kernel to deliver O_TRUNC with open(), where a file
        // handle can buffer the replacement until flush/release.
        config
            .add_capabilities(fuser::consts::FUSE_ATOMIC_O_TRUNC)
            .map_err(|_| EIO)?;
        config
            .set_max_write(1024 * 1024)
            .map(|_| ())
            .map_err(|_| EIO)
    }

    fn lookup(&mut self, _request: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        match self.namespace.lookup(parent, name) {
            Some(node) => reply.entry(&self.ttl, &self.attr(node, None), 0),
            None => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _request: &Request<'_>, ino: u64, fh: Option<u64>, reply: ReplyAttr) {
        match self.namespace.node(ino) {
            Some(node) => {
                let size = fh
                    .and_then(|fh| self.open_files.get(&fh))
                    .filter(|handle| handle.ino == ino)
                    .map(|handle| match &handle.completed {
                        Some(Ok(bytes)) | Some(Err((_, bytes))) => bytes.len() as u64,
                        None => handle.bytes.len() as u64,
                    });
                reply.attr(&self.ttl, &self.attr(node, size));
            }
            None => reply.error(ENOENT),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setattr(
        &mut self,
        _request: &Request<'_>,
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
        let Some(node) = self.namespace.node(ino) else {
            reply.error(ENOENT);
            return;
        };
        if !node_writable(node) {
            reply.error(EROFS);
            return;
        }
        let Some(size) = size else {
            reply.attr(&self.ttl, &self.attr(node, None));
            return;
        };
        let Some(fh) = fh else {
            reply.error(EACCES);
            return;
        };
        let Some(handle) = self.open_files.get_mut(&fh) else {
            reply.error(ENOENT);
            return;
        };
        if handle.ino != ino || handle.completed.is_some() {
            reply.error(EINVAL);
            return;
        }
        handle.bytes.resize(size as usize, 0);
        handle.dirty = true;
        reply.attr(&self.ttl, &self.attr(node, Some(size)));
    }

    fn readlink(&mut self, _request: &Request<'_>, ino: u64, reply: ReplyData) {
        match self.namespace.node(ino).map(|node| &node.kind) {
            Some(NodeKind::Symlink(target)) => reply.data(target.as_bytes()),
            Some(_) => reply.error(EINVAL),
            None => reply.error(ENOENT),
        }
    }

    fn open(&mut self, _request: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        let Some(node) = self.namespace.node(ino) else {
            reply.error(ENOENT);
            return;
        };
        let access = flags & libc::O_ACCMODE;
        let write_requested = access == libc::O_WRONLY || access == libc::O_RDWR;

        let content = match &node.kind {
            NodeKind::File(bytes) => {
                if write_requested && node.write_target.is_none() {
                    reply.error(EROFS);
                    return;
                }
                let mut content = bytes.read().clone();
                if write_requested && flags & libc::O_TRUNC != 0 {
                    content.clear();
                }
                content
            }
            NodeKind::LiveFile { .. } => {
                // LiveFile is read-only; reject write opens.
                if write_requested {
                    reply.error(EROFS);
                    return;
                }
                // Read SHM state once at open and snapshot into the handle.
                // Reopening gets fresh state; the handle is stable until close.
                let Some(content) = self.live_state_bytes(node) else {
                    reply.error(EIO);
                    return;
                };
                content
            }
            NodeKind::Directory => {
                reply.error(EISDIR);
                return;
            }
            NodeKind::Symlink(_) => {
                reply.error(EINVAL);
                return;
            }
            NodeKind::CallFile { .. } => Vec::new(),
            NodeKind::Outcome {
                plugin_id,
                method,
                error,
            } => {
                if write_requested {
                    reply.error(EROFS);
                    return;
                }
                self.outcome_bytes(plugin_id, method, *error)
            }
        };

        // A call file is dispatched only after real bytes arrive, so an open
        // that merely truncates never runs a call with empty arguments.
        let dirty = write_requested
            && flags & libc::O_TRUNC != 0
            && !matches!(node.kind, NodeKind::CallFile { .. });
        let fh = self.next_fh;
        self.next_fh = self.next_fh.wrapping_add(1).max(1);
        self.open_files.insert(
            fh,
            OpenFile {
                ino,
                bytes: content,
                dirty,
                completed: None,
            },
        );
        reply.opened(fh, fuser::consts::FOPEN_DIRECT_IO);
    }

    fn read(
        &mut self,
        _request: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if offset < 0 {
            reply.error(EINVAL);
            return;
        }
        match self.read_bytes(ino, fh) {
            Ok(bytes) => {
                let start = offset as usize;
                if start >= bytes.len() {
                    reply.data(&[]);
                    return;
                }
                let end = start.saturating_add(size as usize).min(bytes.len());
                reply.data(&bytes[start..end]);
            }
            Err(error) => reply.error(error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        _request: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        if offset < 0 {
            reply.error(EINVAL);
            return;
        }
        let Some(node) = self.namespace.node(ino) else {
            reply.error(ENOENT);
            return;
        };
        if !node_writable(node) {
            reply.error(EROFS);
            return;
        }
        let Some(handle) = self.open_files.get_mut(&fh) else {
            reply.error(ENOENT);
            return;
        };
        if handle.ino != ino || handle.completed.is_some() {
            reply.error(EINVAL);
            return;
        }
        let start = offset as usize;
        if data.is_empty() {
            reply.written(0);
            return;
        }
        let end = match start.checked_add(data.len()) {
            Some(end) => end,
            None => {
                reply.error(EINVAL);
                return;
            }
        };
        if handle.bytes.len() < end {
            handle.bytes.resize(end, 0);
        }
        handle.bytes[start..end].copy_from_slice(data);
        handle.dirty = !handle.bytes.is_empty();
        reply.written(data.len() as u32);
    }

    fn flush(
        &mut self,
        _request: &Request<'_>,
        _ino: u64,
        fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        match self.flush_handle(fh) {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(error),
        }
    }

    fn release(
        &mut self,
        _request: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let result = self.flush_handle(fh);
        self.open_files.remove(&fh);
        match result {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(error),
        }
    }

    fn fsync(
        &mut self,
        _request: &Request<'_>,
        _ino: u64,
        fh: u64,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        match self.flush_handle(fh) {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(error),
        }
    }

    fn readdir(
        &mut self,
        _request: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
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
        let mut entries = Vec::with_capacity(children.len() + 2);
        entries.push((ino, FileType::Directory, OsStr::new(".")));
        entries.push((node.parent, FileType::Directory, OsStr::new("..")));
        for (name, child_ino) in children {
            let child = &self.namespace.nodes[child_ino];
            let kind = match child.kind {
                NodeKind::Directory => FileType::Directory,
                NodeKind::File(_)
                | NodeKind::LiveFile { .. }
                | NodeKind::CallFile { .. }
                | NodeKind::Outcome { .. } => FileType::RegularFile,
                NodeKind::Symlink(_) => FileType::Symlink,
            };
            entries.push((*child_ino, kind, name.as_os_str()));
        }

        for (index, (child_ino, kind, name)) in
            entries.into_iter().enumerate().skip(offset.max(0) as usize)
        {
            if reply.add(child_ino, (index + 1) as i64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn access(&mut self, _request: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        match self.namespace.node(ino) {
            None => reply.error(ENOENT),
            Some(node) if mask & libc::W_OK != 0 && !node_writable(node) => reply.error(EROFS),
            Some(_) => reply.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;
    use crate::model::{PluginGuarantees, PluginSchema, TypedObjectBlob};
    use crate::namespace::ROOT_INO;
    use crate::source::SourceSnapshot;

    #[test]
    fn attributes_reflect_workspace_writability() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let object = TypedObjectBlob::new(
            "demo".into(),
            "a".repeat(64),
            PluginSchema {
                name: "demo".into(),
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
                methods: BTreeMap::new(),
                signals: Vec::new(),
                guarantees: PluginGuarantees::default(),
                ..Default::default()
            },
            serde_json::json!({"enabled": true}),
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
        store.create_workspace(Some("test"), None).unwrap();
        let fs = TypedFuse::new(store, ProjectionTarget::Workspace("test".into())).unwrap();
        let plugin = fs
            .namespace
            .lookup(
                fs.namespace
                    .lookup(ROOT_INO, OsStr::new("plugins"))
                    .unwrap()
                    .ino,
                OsStr::new("demo"),
            )
            .unwrap();
        let data = fs.namespace.lookup(plugin.ino, OsStr::new("data")).unwrap();
        let enabled = fs
            .namespace
            .lookup(data.ino, OsStr::new("enabled"))
            .unwrap();
        assert_eq!(fs.attr(enabled, None).perm, 0o644);
        // A workspace rebuilds its tree on write, so the kernel never caches it.
        assert_eq!(fs.ttl, Duration::ZERO);
    }

    fn lungs_fixture(credential: PathBuf) -> (tempfile::TempDir, TypedFuse) {
        lungs_fixture_with(Lungs::Credential {
            bus_address: "unix:path=/nonexistent/lungs-test.sock".into(),
            credential,
        })
    }

    fn lungs_fixture_with(lungs: Lungs) -> (tempfile::TempDir, TypedFuse) {
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
            returns: None,
            side_effect: crate::model::SideEffect::Mutation,
            idempotent: false,
            required_capability: Some("cap.demo.record@v1".into()),
            subid: "mut.demo.record@v1".into(),
        };
        let object = TypedObjectBlob::new(
            "demo".into(),
            "a".repeat(64),
            PluginSchema {
                name: "demo".into(),
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
                    objects: BTreeMap::from([("demo".into(), object)]),
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

    /// Stage a complete write on the method's call file and flush it.
    fn write_call(fs: &mut TypedFuse, bytes: &[u8]) -> Result<(), i32> {
        let ino = method_node(fs, "call").ino;
        fs.open_files.insert(
            7,
            OpenFile {
                ino,
                bytes: bytes.to_vec(),
                dirty: true,
                completed: None,
            },
        );
        let result = fs.flush_handle(7);
        fs.open_files.remove(&7);
        result
    }

    fn error_stage(fs: &TypedFuse) -> String {
        let node = method_node(fs, "error");
        let bytes = fs.read_bytes(node.ino, 0).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        value["stage"].as_str().unwrap().to_string()
    }

    #[test]
    fn socket_mount_session_survives_refresh_without_credentials() {
        let (_directory, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: "/nonexistent/lungs.sock".into(),
            session: "declared-session".into(),
        });
        for _ in 0..2 {
            let node = fs
                .namespace
                .lookup(ROOT_INO, OsStr::new(".3tchedfs.json"))
                .unwrap();
            let metadata: Value =
                serde_json::from_slice(&fs.read_bytes(node.ino, 0).unwrap()).unwrap();
            assert_eq!(metadata["session_id"], "declared-session");
            assert!(metadata.get("credential").is_none());
            fs.refresh_namespace().unwrap();
        }
        let (_directory, fs) = lungs_fixture("/nonexistent/sid1".into());
        let node = fs
            .namespace
            .lookup(ROOT_INO, OsStr::new(".3tchedfs.json"))
            .unwrap();
        let metadata: Value = serde_json::from_slice(&fs.read_bytes(node.ino, 0).unwrap()).unwrap();
        assert!(metadata.get("session_id").is_none());
    }

    #[test]
    fn call_handles_keep_terminal_errors_isolated_and_empty_handles_do_not_dispatch() {
        let (_directory, mut fs) = lungs_fixture("/nonexistent/sid1".into());
        let ino = method_node(&fs, "call").ino;
        for (fh, bytes) in [
            (1, b"not json".as_slice()),
            (2, br#"{"event":7}"#.as_slice()),
            (3, b"".as_slice()),
        ] {
            fs.open_files.insert(
                fh,
                OpenFile {
                    ino,
                    bytes: bytes.to_vec(),
                    dirty: !bytes.is_empty(),
                    completed: None,
                },
            );
        }
        assert!(fs.read_bytes(ino, 1).unwrap().is_empty());
        assert_eq!(fs.flush_handle(1), Err(EINVAL));
        let first = fs.read_bytes(ino, 1).unwrap();
        assert_eq!(fs.flush_handle(2), Err(EINVAL));
        assert_eq!(fs.read_bytes(ino, 1).unwrap(), first);
        assert_eq!(
            serde_json::from_slice::<Value>(&first).unwrap()["stage"],
            "parse"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs.read_bytes(ino, 2).unwrap()).unwrap()["stage"],
            "validate"
        );
        assert_eq!(fs.flush_handle(1), Err(EINVAL));
        assert_eq!(fs.flush_handle(3), Ok(()));
        assert!(fs.open_files[&3].completed.is_none());
        assert!(fs.read_bytes(ino, 0).unwrap().is_empty());
    }

    #[test]
    fn lungs_exposes_call_result_and_error_per_method() {
        let (_directory, fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        let call = method_node(&fs, "call");
        assert!(matches!(call.kind, NodeKind::CallFile { .. }));
        assert_eq!(fs.attr(call, None).perm, 0o666);
        assert_eq!(fs.attr(method_node(&fs, "result"), None).perm, 0o444);
        assert_eq!(fs.attr(method_node(&fs, "error"), None).perm, 0o444);
        assert!(node_writable(call));
        assert!(!node_writable(method_node(&fs, "input.schema.json")));
    }

    #[test]
    fn lungs_rejects_non_json_before_anything_else() {
        let (_directory, mut fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        assert_eq!(write_call(&mut fs, b"not json"), Err(EINVAL));
        assert_eq!(error_stage(&fs), "parse");
    }

    #[test]
    fn lungs_validates_against_the_sealed_schema() {
        let (_directory, mut fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        assert_eq!(write_call(&mut fs, br#"{"event": 7}"#), Err(EINVAL));
        assert_eq!(error_stage(&fs), "validate");
    }

    #[test]
    fn lungs_refuses_without_a_delivered_credential() {
        let (_directory, mut fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        assert_eq!(write_call(&mut fs, br#"{"event": "arrival"}"#), Err(EACCES));
        assert_eq!(error_stage(&fs), "credential");
        let result = method_node(&fs, "result");
        assert!(fs.read_bytes(result.ino, 0).unwrap().is_empty());
    }

    #[test]
    fn lungs_refuses_a_credential_that_is_not_sid1() {
        let directory = tempdir().unwrap();
        let credential = directory.path().join("sid1");
        std::fs::write(&credential, b"OIA1-not-a-session").unwrap();
        let (_fixture, mut fs) = lungs_fixture(credential);
        assert_eq!(write_call(&mut fs, br#"{"event": "arrival"}"#), Err(EACCES));
        assert_eq!(error_stage(&fs), "credential");
    }

    #[test]
    fn lungs_reaches_the_bus_only_with_a_sid1_credential() {
        let directory = tempdir().unwrap();
        let credential = directory.path().join("sid1");
        std::fs::write(&credential, b"SID1 fixture envelope").unwrap();
        let (_fixture, mut fs) = lungs_fixture(credential);
        // The fixture bus does not exist, so a SID1 credential gets as far
        // as the transport and no further.
        assert_eq!(write_call(&mut fs, br#"{"event": "arrival"}"#), Err(EIO));
        assert_eq!(error_stage(&fs), "transport");
    }

    #[test]
    fn socket_lungs_never_ask_for_a_credential() {
        let (_directory, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: PathBuf::from("/nonexistent/lungs.sock"),
            session: "session".into(),
        });
        // No socket exists, so the call fails at the transport, never at
        // the credential stage: a socket mount has nothing to deliver.
        assert_eq!(write_call(&mut fs, br#"{"event": "arrival"}"#), Err(EIO));
        assert_eq!(error_stage(&fs), "transport");
    }

    /// Stands in for the bridge's per-identity socket: the same PluginV1
    /// `Call`, served peer to peer with no bus and no registration.
    struct FixtureBridge;

    #[zbus::interface(name = "org.opdbus.v1.PluginV1")]
    impl FixtureBridge {
        async fn call(&self, method: String, json_args: String) -> String {
            serde_json::json!({ "method": method, "args": json_args }).to_string()
        }
    }

    /// Stands in for the bridge's `LungsV1.Bind`, recording the session.
    struct FixtureLungs(std::sync::Arc<std::sync::Mutex<Option<String>>>);

    #[zbus::interface(name = "org.opdbus.v1.LungsV1")]
    impl FixtureLungs {
        async fn bind(&self, session_id: String) -> bool {
            *self.0.lock().unwrap() = Some(session_id);
            true
        }

        async fn bind_current_container(&self) -> bool {
            *self.0.lock().unwrap() = Some("kernel-container".into());
            true
        }
    }

    #[test]
    fn guest_socket_bootstrap_sends_no_session_and_returns_operation_receipt() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("lungs.sock");
        let bound = std::sync::Arc::new(std::sync::Mutex::new(None));
        let bound_by_server = bound.clone();
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
                    .serve_at("/org/opdbus/v1/plugins/demo", FixtureBridge)
                    .unwrap()
                    .serve_at("/org/opdbus/v1/lungs", FixtureLungs(bound_by_server))
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                connection.closed().await;
            });
        });
        let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: socket,
            session: String::new(),
        });
        let ino = method_node(&fs, "call").ino;
        fs.open_files.insert(
            7,
            OpenFile {
                ino,
                bytes: br#"{"event":"arrival"}"#.to_vec(),
                dirty: true,
                completed: None,
            },
        );
        assert_eq!(fs.flush_handle(7), Ok(()));
        let result = method_node(&fs, "result");
        let receipt: Value =
            serde_json::from_slice(&fs.read_bytes(result.ino, 0).unwrap()).unwrap();
        assert_eq!(receipt["result"]["method"], "record_event");
        assert_eq!(bound.lock().unwrap().as_deref(), Some("kernel-container"));
        fs.refresh_namespace().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn socket_lungs_dispatch_over_the_identity_socket() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("lungs.sock");
        let bound = std::sync::Arc::new(std::sync::Mutex::new(None));
        let bound_by_server = bound.clone();
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
                    .serve_at("/org/opdbus/v1/plugins/demo", FixtureBridge)
                    .unwrap()
                    .serve_at("/org/opdbus/v1/lungs", FixtureLungs(bound_by_server))
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                connection.closed().await;
            });
        });

        let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: socket,
            session: "b0c777e3".into(),
        });
        let ino = method_node(&fs, "call").ino;
        fs.open_files.insert(
            7,
            OpenFile {
                ino,
                bytes: br#"{"event": "arrival"}"#.to_vec(),
                dirty: true,
                completed: None,
            },
        );
        assert_eq!(fs.flush_handle(7), Ok(()));
        let handle_receipt = fs.read_bytes(ino, 7).unwrap();
        // fsync, flush and release all use the same terminal state.
        assert_eq!(fs.flush_handle(7), Ok(()));
        assert_eq!(fs.flush_handle(7), Ok(()));
        assert_eq!(fs.read_bytes(ino, 7).unwrap(), handle_receipt);
        let result = method_node(&fs, "result");
        let receipt: Value =
            serde_json::from_slice(&fs.read_bytes(result.ino, 0).unwrap()).unwrap();
        assert_eq!(receipt["result"]["method"], "record_event");
        assert_eq!(receipt["result"]["args"], r#"{"event":"arrival"}"#);
        assert!(
            fs.read_bytes(method_node(&fs, "error").ino, 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(bound.lock().unwrap().as_deref(), Some("b0c777e3"));
        // A later failed handle must never return this successful receipt.
        let result_ino = result.ino;
        fs.open_files.insert(
            8,
            OpenFile {
                ino,
                bytes: b"not json".to_vec(),
                dirty: true,
                completed: None,
            },
        );
        assert_eq!(fs.flush_handle(8), Err(EINVAL));
        assert_eq!(fs.read_bytes(ino, 7).unwrap(), handle_receipt);
        assert!(fs.read_bytes(result_ino, 0).unwrap().is_empty());
        server.join().unwrap();
    }

    /// Stands in for the bridge refusing `Bind`, as it does for a session
    /// that is parked, expired, revoked or not anchored.
    struct RefusingLungs;

    #[zbus::interface(name = "org.opdbus.v1.LungsV1")]
    impl RefusingLungs {
        async fn bind(&self, session_id: String) -> zbus::fdo::Result<bool> {
            Err(zbus::fdo::Error::AccessDenied(format!(
                "lungs identity unavailable: {session_id}"
            )))
        }
    }

    #[test]
    fn socket_lungs_stop_at_a_refused_bind() {
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
                    .serve_at("/org/opdbus/v1/plugins/demo", FixtureBridge)
                    .unwrap()
                    .serve_at("/org/opdbus/v1/lungs", RefusingLungs)
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                connection.closed().await;
            });
        });

        let (_fixture, mut fs) = lungs_fixture_with(Lungs::Socket {
            path: socket,
            session: "b0c777e3".into(),
        });
        // A refused bind stops the call before any plugin sees it, at the
        // same stage and errno as a refused SID1 registration.
        assert_eq!(write_call(&mut fs, br#"{"event": "arrival"}"#), Err(EACCES));
        assert_eq!(error_stage(&fs), "register");
        assert!(
            fs.read_bytes(method_node(&fs, "result").ino, 0)
                .unwrap()
                .is_empty()
        );
        server.join().unwrap();
    }

    #[test]
    fn live_reads_follow_each_committed_state_version() {
        let (directory, fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        let state = directory.path().join("state");
        let commit = |value: &str| {
            // MutationEngine commits by writing a sibling and renaming it.
            let staged = state.join(".demo.json.tmp");
            std::fs::write(&staged, value).unwrap();
            std::fs::rename(&staged, state.join("demo.json")).unwrap();
        };
        let plugins = fs
            .namespace
            .lookup(ROOT_INO, OsStr::new("plugins"))
            .unwrap();
        let demo = fs
            .namespace
            .lookup(plugins.ino, OsStr::new("demo"))
            .unwrap();
        let value = fs
            .namespace
            .lookup(demo.ino, OsStr::new("value.json"))
            .unwrap();

        commit(r#"{"generation": 1}"#);
        let first = fs.live_state_bytes(value).unwrap();
        assert!(String::from_utf8_lossy(&first).contains("\"generation\": 1"));
        // A second read of the same version is served from the cache.
        assert_eq!(fs.live_state_bytes(value).unwrap(), first);

        commit(r#"{"generation": 2}"#);
        let second = fs.live_state_bytes(value).unwrap();
        assert!(String::from_utf8_lossy(&second).contains("\"generation\": 2"));
    }

    #[test]
    fn lungs_views_use_the_kernel_cache() {
        let (_directory, fs) = lungs_fixture(PathBuf::from("/nonexistent/sid1"));
        assert_eq!(fs.ttl, VIEW_TTL);
    }
}
