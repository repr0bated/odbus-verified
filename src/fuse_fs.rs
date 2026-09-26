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

use crate::namespace::{Namespace, Node, NodeKind, ProjectionTarget};
use crate::store::ObjectStore;

const TTL: Duration = Duration::ZERO;

struct OpenFile {
    ino: u64,
    bytes: Vec<u8>,
    dirty: bool,
}

pub struct TypedFuse {
    store: ObjectStore,
    namespace: Namespace,
    open_files: HashMap<u64, OpenFile>,
    next_fh: u64,
    uid: u32,
    gid: u32,
}

impl TypedFuse {
    pub fn new(store: ObjectStore, target: ProjectionTarget) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, None)?;
        Ok(Self {
            store,
            namespace,
            open_files: HashMap::new(),
            next_fh: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        })
    }

    pub fn with_state_dir(
        store: ObjectStore,
        target: ProjectionTarget,
        state_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let namespace = Namespace::build(&store, target, Some(&state_dir))?;
        Ok(Self {
            store,
            namespace,
            open_files: HashMap::new(),
            next_fh: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
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

    /// Read a plugin's live state from SHM, parse it, and extract the value
    /// at the given JSON pointer. Returns pretty-printed JSON bytes.
    fn live_state_bytes(&self, node: &Node) -> Option<Vec<u8>> {
        let (plugin_id, pointer) = match &node.kind {
            NodeKind::LiveFile { plugin_id, pointer } => (plugin_id.as_str(), pointer.as_str()),
            _ => return None,
        };
        let state_dir = self.namespace.state_dir.as_ref()?;
        let path = state_dir.join(format!("{plugin_id}.json"));
        let raw = std::fs::read(&path).ok()?;
        let value: Value = serde_json::from_slice(&raw).ok()?;
        let extracted = if pointer.is_empty() {
            value
        } else {
            value.pointer(pointer).cloned().unwrap_or(Value::Null)
        };
        let mut bytes = serde_json::to_vec_pretty(&extracted).ok()?;
        bytes.push(b'\n');
        Some(bytes)
    }

    fn refresh_namespace(&mut self) -> anyhow::Result<()> {
        let state_dir = self.namespace.state_dir.clone();
        self.namespace = Namespace::build(
            &self.store,
            self.namespace.target.clone(),
            state_dir.as_deref(),
        )?;
        Ok(())
    }

    fn flush_handle(&mut self, fh: u64) -> Result<(), i32> {
        let Some(handle) = self.open_files.get_mut(&fh) else {
            return Err(ENOENT);
        };
        if !handle.dirty {
            return Ok(());
        }
        let node = self.namespace.node(handle.ino).ok_or(ENOENT)?;
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
        }
    }
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
            Some(node) => reply.entry(&TTL, &self.attr(node, None), 0),
            None => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _request: &Request<'_>, ino: u64, fh: Option<u64>, reply: ReplyAttr) {
        match self.namespace.node(ino) {
            Some(node) => {
                let size = fh
                    .and_then(|fh| self.open_files.get(&fh))
                    .filter(|handle| handle.ino == ino)
                    .map(|handle| handle.bytes.len() as u64);
                reply.attr(&TTL, &self.attr(node, size));
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
        if node.write_target.is_none() {
            reply.error(EROFS);
            return;
        }
        let Some(size) = size else {
            reply.attr(&TTL, &self.attr(node, None));
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
        handle.bytes.resize(size as usize, 0);
        handle.dirty = true;
        reply.attr(&TTL, &self.attr(node, Some(size)));
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
        };

        let dirty = write_requested && flags & libc::O_TRUNC != 0;
        let fh = self.next_fh;
        self.next_fh = self.next_fh.wrapping_add(1).max(1);
        self.open_files.insert(
            fh,
            OpenFile {
                ino,
                bytes: content,
                dirty,
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
        if node.write_target.is_none() {
            reply.error(EROFS);
            return;
        }
        let Some(handle) = self.open_files.get_mut(&fh) else {
            reply.error(ENOENT);
            return;
        };
        if handle.ino != ino {
            reply.error(EINVAL);
            return;
        }
        let start = offset as usize;
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
        handle.dirty = true;
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
                NodeKind::File(_) | NodeKind::LiveFile { .. } => FileType::RegularFile,
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
            Some(node) if mask & libc::W_OK != 0 && node.write_target.is_none() => {
                reply.error(EROFS)
            }
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
    }
}
