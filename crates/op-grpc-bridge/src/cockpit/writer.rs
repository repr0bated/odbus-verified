//! Private, root-confined evidence storage. The held directory descriptor pins
//! the destination; caller-controlled record IDs never become filesystem paths.
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use super::records::EvidenceRecord;
use super::safety::{RecordError, RecordGuard};

pub struct EvidenceRecorder<'a> { root: File, guard: RecordGuard<'a> }

impl<'a> EvidenceRecorder<'a> {
    /// The directory must already exist. No implicit host directories, mounts,
    /// credentials, or runtime state are created by this evidence layer.
    pub fn open(root: &Path, guard: RecordGuard<'a>) -> Result<Self, RecordError> {
        let root = OpenOptions::new().read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(root)?;
        let metadata = root.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o022 != 0 {
            return Err(RecordError::Invalid("evidence directory must be owned and not group/world writable"));
        }
        Ok(Self { root, guard })
    }

    pub fn write(&self, record: &EvidenceRecord) -> Result<(), RecordError> {
        let mut bytes = self.guard.serialize(record)?;
        bytes.push(b'\n');
        match record {
            EvidenceRecord::Blocker(_) => self.append_blocker(&bytes),
            _ => self.replace(record_name(record), &bytes),
        }
    }

    fn open_relative(&self, name: &str, flags: i32) -> Result<File, RecordError> {
        let name = CString::new(name).map_err(|_| RecordError::Invalid("record filename invalid"))?;
        let fd = unsafe { libc::openat(self.root.as_raw_fd(), name.as_ptr(), flags | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
        if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0 || metadata.nlink() != 1 {
            return Err(RecordError::Invalid("evidence file is not a private owned regular file"));
        }
        Ok(file)
    }

    fn append_blocker(&self, bytes: &[u8]) -> Result<(), RecordError> {
        let mut file = self.open_relative("blockers.jsonl", libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND)?;
        let _lock = FileLock::exclusive(&file)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.root.sync_all()?;
        Ok(())
    }

    fn replace(&self, name: &str, bytes: &[u8]) -> Result<(), RecordError> {
        // One sibling lock serializes writers without truncating an existing
        // document. Readers see the entire old document or entire new document.
        let lock_file = self.open_relative(".writer.lock", libc::O_WRONLY | libc::O_CREAT)?;
        let _lock = FileLock::exclusive(&lock_file)?;
        let temporary = format!(".{}.tmp", uuid::Uuid::new_v4());
        let temporary_c = CString::new(temporary.as_str()).map_err(|_| RecordError::Invalid("temporary filename invalid"))?;
        let destination = CString::new(name).map_err(|_| RecordError::Invalid("record filename invalid"))?;
        let mut created = false;
        let result = (|| {
            let mut file = self.open_relative(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
            created = true;
            file.write_all(bytes)?;
            file.sync_all()?;
            let rc = unsafe { libc::renameat(self.root.as_raw_fd(), temporary_c.as_ptr(), self.root.as_raw_fd(), destination.as_ptr()) };
            if rc != 0 { return Err(RecordError::Io(std::io::Error::last_os_error())); }
            self.root.sync_all()?;
            Ok(())
        })();
        if result.is_err() && created {
            // Only our unpredictable O_EXCL temporary is eligible for cleanup.
            unsafe { libc::unlinkat(self.root.as_raw_fd(), temporary_c.as_ptr(), 0); }
        }
        result
    }
}

fn record_name(record: &EvidenceRecord) -> &'static str {
    match record {
        EvidenceRecord::Baseline(_) => "baseline.json",
        EvidenceRecord::Runtime(_) => "runtime.json",
        EvidenceRecord::LegacyChatInventory(_) => "legacy-chat-inventory.json",
        EvidenceRecord::OwnerAuthorization(_) => "owner-authorization.json",
        EvidenceRecord::Status(_) => "status.json",
        EvidenceRecord::Blocker(_) => "blockers.jsonl",
        EvidenceRecord::StorageLayout(_) => "storage-layout.json",
        EvidenceRecord::ImageProvisioning(_) => "image-provisioning.json",
    }
}

struct FileLock(i32);
impl FileLock {
    fn exclusive(file: &File) -> Result<Self, RecordError> {
        let fd = file.as_raw_fd();
        loop {
            if unsafe { libc::flock(fd, libc::LOCK_EX) } == 0 { return Ok(Self(fd)); }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted { return Err(error.into()); }
        }
    }
}
impl Drop for FileLock {
    fn drop(&mut self) { unsafe { libc::flock(self.0, libc::LOCK_UN); } }
}
