//! Service-private namespaces using the canonical projection transaction.
//!
//! This handle supplies filesystem confinement only. Callers must derive the
//! namespace from verified identity and pin the owning plugin contract before
//! every use. It never supplies catalog membership, authorization, recovery of
//! a durable backend, or a public broadcast/cache path.

use super::*;
use std::sync::Arc;

#[path = "private_document_publication.rs"]
mod publication;
pub use publication::{PrivateDocumentCommit, PrivateDocumentPublication};

#[derive(Clone)]
pub struct PrivateProjectionDirectory {
    directory: Arc<File>,
}

fn require_private(directory: &File, writer: bool) -> Result<()> {
    let metadata = directory.metadata()?;
    ensure!(
        metadata.is_dir()
            && metadata.mode() & 0o7777 == 0o700
            && trusted_directory_owner(metadata.uid(), nix::unistd::geteuid().as_raw(), writer),
        "private projection namespace must be a trusted owner-only 0700 directory"
    );
    Ok(())
}

impl PrivateProjectionDirectory {
    /// Prepare a private namespace and its lock without publishing state. New
    /// path components are created 0700. Existing permissive directories are
    /// rejected, never silently tightened after private data could be exposed.
    pub fn prepare(path: &Path) -> Result<Self> {
        let directory = directory(path, true, 0o700)?.context("private namespace missing")?;
        Self::prepare_directory(directory)
    }

    fn prepare_directory(directory: File) -> Result<Self> {
        require_private(&directory, true)?;
        let _lock = LockedDirectory::from_directory(directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        Ok(Self {
            directory: Arc::new(directory),
        })
    }

    /// Open existing state without creating directories, locks or files.
    pub fn open(path: &Path) -> Result<Option<Self>> {
        let Some(directory) = directory(path, false, 0)? else {
            return Ok(None);
        };
        require_private(&directory, false)?;
        Ok(Some(Self {
            directory: Arc::new(directory),
        }))
    }

    /// Establish the one-way private-document -> canonical identity-read lock
    /// order before entering a private transaction. Both paths are opened
    /// without symlink traversal and compared with this held directory inode.
    /// This does not authorize any identity or document read.
    pub fn ensure_distinct_from(&self, other: &Path) -> Result<()> {
        require_private(&self.directory, false)?;
        let other = directory(other, false, 0)?.context("identity projection directory missing")?;
        let held = self.directory.metadata()?;
        let other = other.metadata()?;
        ensure!(
            held.dev() != other.dev() || held.ino() != other.ino(),
            "private document namespace aliases the canonical identity directory"
        );
        Ok(())
    }

    /// Derive a child from the held namespace descriptor. The name must be an
    /// opaque flat identifier supplied by the domain owner, never a user path.
    pub fn prepare_child(&self, name: &str) -> Result<Self> {
        filename(name)?;
        require_private(&self.directory, true)?;
        match nix::sys::stat::mkdirat(
            Some(self.directory.as_raw_fd()),
            name,
            Mode::from_bits_truncate(0o700),
        ) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(error) => return Err(error.into()),
        }
        let child = open_at(
            &self.directory,
            name.as_ref(),
            OFlag::O_RDONLY | OFlag::O_DIRECTORY,
            0,
        )?;
        Self::prepare_directory(child)
    }

    pub fn open_child(&self, name: &str) -> Result<Option<Self>> {
        filename(name)?;
        require_private(&self.directory, false)?;
        let child = match open_at(
            &self.directory,
            name.as_ref(),
            OFlag::O_RDONLY | OFlag::O_DIRECTORY,
            0,
        ) {
            Ok(file) => file,
            Err(Errno::ENOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        require_private(&child, false)?;
        Ok(Some(Self {
            directory: Arc::new(child),
        }))
    }

    /// Exact committed bytes for caller-selected sealed IDs. The original
    /// manifest/lock rules apply; no path is reopened after handle creation.
    pub fn read_snapshot(&self, plugin_ids: &[String]) -> Result<ProjectionSnapshot> {
        for plugin in plugin_ids {
            filename(plugin)?;
            ensure!(
                plugin != "identity_sled",
                "identity uses its dedicated paired projection"
            );
        }
        require_private(&self.directory, false)?;
        let Some(locked) = LockedDirectory::from_directory(self.directory.try_clone()?, false)?
        else {
            return Ok(ProjectionSnapshot {
                generation: 0,
                states: BTreeMap::new(),
            });
        };
        read_locked_snapshot(&locked, plugin_ids)
    }

    /// Fold and commit one complete private document with the existing
    /// transaction writer. The synchronous closure has no external effects and
    /// must not call another projection helper. Revision checks belong inside
    /// this closure; domain durability/recovery belongs to its upstream owner.
    pub fn update(
        &self,
        plugin_id: &str,
        fold: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    ) -> Result<ProjectionCommit> {
        let name = filename(plugin_id)?;
        ensure!(
            plugin_id != "identity_sled",
            "identity uses its dedicated paired projection"
        );
        require_private(&self.directory, true)?;
        let locked = LockedDirectory::from_directory(self.directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        update_locked_projection(&locked, plugin_id, &name, fold)
    }
}

#[cfg(test)]
#[path = "private_projection_directory_tests.rs"]
mod tests;
