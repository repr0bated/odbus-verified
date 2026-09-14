//! Btrfs filesystem **in** the blob artifact.
//!
//! The outermost deployment artifact is a btrfs filesystem image whose
//! contents are the sealed plugin object blobs plus an index — the "blob is
//! complete" deployment unit from BLOB_ARCHITECTURE_SYNTHESIS.md: mountable
//! (loop/ro), snapshottable, `btrfs send/receive`-able, and self-describing.
//!
//! Layout inside the image:
//!
//! ```text
//! /blobs/<plugin_id>/<plugin_id>.<hash16>.blob   (one subvolume per plugin
//! /blob-index.json                                when btrfs-progs supports
//!                                                 --subvol; plain dirs otherwise)
//! ```
//!
//! Sealing uses `mkfs.btrfs --rootdir --shrink`, which populates the image
//! from a staging directory **without mounting** — no root privileges needed.
//! Runtime consumption of blobs stays on tmpfs (REQ-8.3); the btrfs image is
//! the at-rest/transport form, activated by loop-mounting read-only and
//! loading the catalog from the mountpoint.

use crate::blob::BlobRef;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Btrfs superblock magic "_BHRfS_M" at superblock offset 0x10000 + 0x40.
const BTRFS_MAGIC_OFFSET: u64 = 0x10040;
const BTRFS_MAGIC: &[u8; 8] = b"_BHRfS_M";

#[derive(Debug)]
pub struct BtrfsImageReport {
    pub image_path: PathBuf,
    pub image_bytes: u64,
    pub blob_count: usize,
    pub subvolumes: bool,
    pub mount_hint: String,
}

/// Seal a set of sealed `.blob` files into a btrfs filesystem image.
pub fn seal_blobs_into_btrfs_image(
    blob_paths: &[PathBuf],
    image_path: &Path,
) -> io::Result<BtrfsImageReport> {
    if blob_paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no blobs to seal",
        ));
    }
    if which_mkfs().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "mkfs.btrfs not found (install btrfs-progs)",
        ));
    }

    // Stage: /blobs/<plugin_id>/<file>, /blob-index.json
    let staging = staging_dir(image_path)?;
    let blobs_root = staging.join("blobs");
    std::fs::create_dir_all(&blobs_root)?;

    let mut index = Vec::new();
    let mut total: u64 = 0;
    let mut subvol_paths = Vec::new();
    for p in blob_paths {
        let bytes = std::fs::read(p)?;
        let blob =
            BlobRef::new(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let manifest = blob
            .manifest()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let dir = blobs_root.join(&manifest.plugin_id);
        std::fs::create_dir_all(&dir)?;
        let file_name = p
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "blob path has no name"))?;
        std::fs::copy(p, dir.join(file_name))?;
        total += bytes.len() as u64;
        subvol_paths.push(format!("blobs/{}", manifest.plugin_id));
        index.push(serde_json::json!({
            "plugin_id": manifest.plugin_id,
            "schema_version": manifest.schema_version,
            "schema_hash": manifest.schema_hash,
            "dbus_path": manifest.dbus.object_path,
            "services": manifest.grpc.services,
            "file": format!("blobs/{}/{}", manifest.plugin_id, file_name.to_string_lossy()),
        }));
    }
    std::fs::write(
        staging.join("blob-index.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "opdbus-blob-image/1",
            "blobs": index,
        }))?,
    )?;

    // Pre-size the image; --shrink trims it to minimal afterwards.
    let size = (total * 2 + 32 * 1024 * 1024).max(64 * 1024 * 1024);
    if image_path.exists() {
        std::fs::remove_file(image_path)?;
    }
    let f = std::fs::File::create(image_path)?;
    f.set_len(size)?;
    drop(f);

    // Try one-subvolume-per-plugin first (btrfs-progs >= 6.x --subvol),
    // fall back to plain directories. `-f`: we own the just-created image, and
    // without it mkfs aborts when any loop device's backing file is
    // unreadable (e.g. root-owned incus disks) during its mount-status scan.
    let mut subvolumes = true;
    let mut args: Vec<String> = vec![
        "-q".into(),
        "-f".into(),
        "--rootdir".into(),
        staging.to_string_lossy().into_owned(),
        "--shrink".into(),
    ];
    for sv in &subvol_paths {
        args.push("--subvol".into());
        args.push(sv.clone());
    }
    args.push(image_path.to_string_lossy().into_owned());
    let ok = run_mkfs(&args)?;
    if !ok {
        subvolumes = false;
        let args: Vec<String> = vec![
            "-q".into(),
            "-f".into(),
            "--rootdir".into(),
            staging.to_string_lossy().into_owned(),
            "--shrink".into(),
            image_path.to_string_lossy().into_owned(),
        ];
        if !run_mkfs(&args)? {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(io::Error::other("mkfs.btrfs failed"));
        }
    }
    let _ = std::fs::remove_dir_all(&staging);

    if !verify_btrfs_image(image_path)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "image missing btrfs superblock magic",
        ));
    }

    let image_bytes = std::fs::metadata(image_path)?.len();
    Ok(BtrfsImageReport {
        mount_hint: format!(
            "mount -o loop,ro {} /mnt/opdbus-blob   # then ActiveReflectionCatalog::open(\"/mnt/opdbus-blob/blobs/<plugin>\")",
            image_path.display()
        ),
        image_path: image_path.to_path_buf(),
        image_bytes,
        blob_count: blob_paths.len(),
        subvolumes,
    })
}

/// Check the btrfs superblock magic without mounting or external tools.
pub fn verify_btrfs_image(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    if f.metadata()?.len() < BTRFS_MAGIC_OFFSET + 8 {
        return Ok(false);
    }
    f.seek(SeekFrom::Start(BTRFS_MAGIC_OFFSET))?;
    let mut buf = [0u8; 8];
    f.read_exact(&mut buf)?;
    Ok(&buf == BTRFS_MAGIC)
}

pub fn which_mkfs() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("mkfs.btrfs"))
            .find(|p| p.is_file())
    })
}

fn run_mkfs(args: &[String]) -> io::Result<bool> {
    let out = Command::new("mkfs.btrfs").args(args).output()?;
    if !out.status.success() {
        // Caller decides whether to retry without --subvol.
        return Ok(false);
    }
    Ok(true)
}

fn staging_dir(image_path: &Path) -> io::Result<PathBuf> {
    let parent = image_path.parent().unwrap_or(Path::new("."));
    let dir = parent.join(format!(
        ".opblob-staging-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
