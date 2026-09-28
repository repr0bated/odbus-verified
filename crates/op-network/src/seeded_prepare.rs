//! Root-only, one-shot operator preparation of a blank identity leaf.
//! This prepares storage; it never changes Incus, Cozo, identity keys or grants.

use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::seeded_root::{SeededProvisionDescriptor, SeededRootDescriptor, provision_seeded_leaf};

const PLAN_DIR: &str = "/var/lib/op-dbus/identity-leaf-plans";
const MAX_PLAN_BYTES: u64 = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeafPlan {
    version: u8,
    session_id: String,
    seed_image_path: String,
    seed_sha256: String,
    seed_fsid: String,
    image_fingerprint: String,
    new_leaf_image_path: String,
    leaf_size_bytes: u64,
    mount_point: String,
}

impl LeafPlan {
    fn validate(&self, session_id: &str) -> Result<()> {
        ensure!(self.version == 1, "unsupported leaf plan version");
        ensure!(
            self.session_id == session_id,
            "leaf plan session differs from request"
        );
        ensure!(
            self.seed_sha256.len() == 64
                && self
                    .seed_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid pinned seed SHA-256"
        );
        ensure!(
            self.image_fingerprint.len() == 64
                && self
                    .image_fingerprint
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid pinned image fingerprint"
        );
        Uuid::parse_str(&self.seed_fsid).context("invalid pinned seed FSID")?;
        for value in [
            &self.seed_image_path,
            &self.new_leaf_image_path,
            &self.mount_point,
        ] {
            let path = Path::new(value);
            ensure!(
                path.is_absolute()
                    && path.components().all(|component| matches!(
                        component,
                        Component::RootDir | Component::Normal(_)
                    )),
                "leaf plan path is not absolute and normalized"
            );
        }
        ensure!(
            self.seed_image_path != self.new_leaf_image_path,
            "leaf would overwrite the pinned seed"
        );
        Ok(())
    }

    fn descriptor(&self) -> Result<SeededProvisionDescriptor<'_>> {
        Ok(SeededProvisionDescriptor {
            seed_image: Path::new(&self.seed_image_path),
            seed_sha256: &self.seed_sha256,
            seed_fsid: Uuid::parse_str(&self.seed_fsid)?,
            image_fingerprint: &self.image_fingerprint,
            new_leaf_image: Path::new(&self.new_leaf_image_path),
            leaf_size_bytes: self.leaf_size_bytes,
            mount_point: Path::new(&self.mount_point),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct PreparedLeafReceipt {
    pub session_id: String,
    pub plan_sha256: String,
    pub seed_fsid: Uuid,
    pub sprout_fsid: Uuid,
    pub leaf_image: String,
    pub persist_source: String,
    pub journal_path: String,
}

fn open_at(parent: &File, name: &OsStr, flags: i32) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes())?;
    // SAFETY: parent is a pinned directory and name is NUL terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned an owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn trusted_plan_directory() -> Result<File> {
    let mut parent = File::open("/")?;
    for component in Path::new(PLAN_DIR).components().skip(1) {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        let next = open_at(&parent, name, libc::O_RDONLY | libc::O_DIRECTORY)
            .context("open trusted leaf plan directory")?;
        let metadata = next.metadata()?;
        ensure!(
            metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "leaf plan ancestor is not root-controlled"
        );
        parent = next;
    }
    ensure!(
        parent.metadata()?.mode() & 0o777 == 0o700,
        "leaf plan directory must be root-only mode 0700"
    );
    Ok(parent)
}

fn trusted_plan_file(metadata: &Metadata, owner_uid: u32) -> Result<()> {
    ensure!(
        metadata.is_file() && metadata.uid() == owner_uid && metadata.mode() & 0o777 == 0o600,
        "leaf plan must be a root-owned mode 0600 regular file"
    );
    ensure!(metadata.len() <= MAX_PLAN_BYTES, "leaf plan is too large");
    Ok(())
}

fn read_plan(
    directory: &File,
    session_id: &str,
    expected_sha256: &str,
    owner_uid: u32,
) -> Result<LeafPlan> {
    ensure!(
        expected_sha256.len() == 64
            && expected_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid leaf plan digest"
    );
    let filename = format!("{session_id}.json");
    let file = open_at(
        directory,
        OsStr::new(&filename),
        libc::O_RDONLY | libc::O_NONBLOCK,
    )
    .context("open exact leaf plan")?;
    let before = file.metadata()?;
    trusted_plan_file(&before, owner_uid)?;
    let mut bytes = Vec::new();
    (&file).take(MAX_PLAN_BYTES + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        bytes.len() as u64 <= MAX_PLAN_BYTES
            && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "leaf plan changed while being read"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == expected_sha256,
        "leaf plan SHA-256 differs from operator pin"
    );
    let plan: LeafPlan = serde_json::from_slice(&bytes).context("decode leaf plan")?;
    plan.validate(session_id)?;
    Ok(plan)
}

/// Prepare one absent, blank leaf and its empty sibling persist subvolume.
/// An existing leaf or journal is a reconciliation case and is never retried.
pub fn prepare_leaf(session_id: &str, plan_sha256: &str) -> Result<PreparedLeafReceipt> {
    // SAFETY: geteuid has no pointer arguments or side effects.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "leaf preparation requires root"
    );
    let session = Uuid::parse_str(session_id).context("invalid session UUID")?;
    ensure!(
        session.to_string() == session_id,
        "noncanonical session UUID"
    );
    let directory = trusted_plan_directory()?;
    let plan = read_plan(&directory, session_id, plan_sha256, 0)?;
    let descriptor = plan.descriptor()?;
    let mut guard = provision_seeded_leaf(&descriptor)?;
    let persist = guard.prepare_persist_subvolume("pool")?;
    let native = guard.receipt.clone();
    let root = SeededRootDescriptor {
        seed_image: &native.seed_image,
        seed_sha256: &native.seed_sha256,
        seed_fsid: native.seed_fsid,
        leaf_image: &native.leaf_image,
        sprout_fsid: native.sprout_fsid,
        mount_point: &native.mount_point,
        pool_subvolume: "pool",
    };
    let mounted = guard.prepared_root_guard("pool")?;
    mounted.verify_clean_core(&root, &plan.image_fingerprint)?;
    ensure!(
        mounted.verify_persist_subvolume(&root, "persist", true)? == persist,
        "new persist path differs from native readback"
    );
    guard.journal_owner_stage(
        "OPERATOR_READY",
        serde_json::json!({
            "session_id": session_id,
            "plan_sha256": plan_sha256,
            "sprout_fsid": native.sprout_fsid,
        }),
    )?;
    Ok(PreparedLeafReceipt {
        session_id: session_id.to_owned(),
        plan_sha256: plan_sha256.to_owned(),
        seed_fsid: native.seed_fsid,
        sprout_fsid: native.sprout_fsid,
        leaf_image: native.leaf_image.display().to_string(),
        persist_source: persist.display().to_string(),
        journal_path: native.journal_path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn rejects_weak_mode_and_wrong_owner() {
        let path = std::env::temp_dir().join(format!("leaf-plan-test-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        let meta = file.metadata().unwrap();
        assert!(trusted_plan_file(&meta, meta.uid()).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let meta = file.metadata().unwrap();
        assert!(trusted_plan_file(&meta, meta.uid() + 1).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_symlink_plan_before_read() {
        let path = std::env::temp_dir().join(format!("leaf-plan-dir-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let target = path.join("target");
        fs::write(&target, b"{}").unwrap();
        symlink(&target, path.join("plan.json")).unwrap();
        let directory = File::open(&path).unwrap();
        assert!(open_at(&directory, OsStr::new("plan.json"), libc::O_RDONLY).is_err());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn rejects_wrong_digest_before_schema() {
        let path = std::env::temp_dir().join(format!("leaf-plan-dir-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let session = Uuid::new_v4().to_string();
        let name = path.join(format!("{session}.json"));
        fs::write(&name, b"{}").unwrap();
        fs::set_permissions(&name, fs::Permissions::from_mode(0o600)).unwrap();
        let directory = File::open(&path).unwrap();
        assert!(
            read_plan(&directory, &session, &"0".repeat(64), file_uid(&name))
                .unwrap_err()
                .to_string()
                .contains("SHA-256 differs")
        );
        fs::remove_dir_all(path).unwrap();
    }

    fn file_uid(path: &Path) -> u32 {
        fs::metadata(path).unwrap().uid()
    }
}
