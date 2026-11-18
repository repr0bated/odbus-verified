//! Root-owned host storage binding, outside every application release overlay.
//! There is deliberately no historical-host fallback.
use super::fs;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{os::unix::fs::MetadataExt, path::Path, sync::OnceLock};

const PATH: &str = "/var/lib/opdbus-release/storage.json";
static STORAGE: OnceLock<Storage> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub fsid: String,
    pub parent_subvolume: u64,
    pub slot_a: String,
    pub slot_b: String,
}

pub fn initialize() -> Result<()> {
    ensure!(
        STORAGE.get().is_none(),
        "release storage binding already initialized"
    );
    let mut file = fs::secure_absolute(Path::new(PATH), libc::O_RDONLY)
        .context("install the root-owned release storage binding before release administration")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.gid() == 0
            && metadata.mode() & 0o7777 == 0o600
            && metadata.nlink() == 1,
        "release storage binding must be a root:root 0600 single-link regular file"
    );
    let storage: Storage = serde_json::from_slice(&fs::read_bounded(&mut file, 16 * 1024)?)?;
    let uuid: uuid::Uuid = storage
        .fsid
        .parse()
        .context("invalid release filesystem UUID")?;
    ensure!(
        !uuid.is_nil() && uuid.to_string() == storage.fsid,
        "release filesystem UUID must be canonical and nonzero"
    );
    ensure!(
        storage.parent_subvolume >= 256,
        "release preparation parent must be a native Btrfs subvolume"
    );
    for name in [&storage.slot_a, &storage.slot_b] {
        ensure!(
            name.starts_with("@opdbus-system-")
                && name.len() > "@opdbus-system-".len()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"@-_.".contains(&c)),
            "release slot must be a single dedicated @opdbus-system- name"
        );
    }
    ensure!(
        storage.slot_a != storage.slot_b,
        "release slots must be distinct"
    );
    STORAGE
        .set(storage)
        .map_err(|_| anyhow::anyhow!("release storage binding already initialized"))
}

pub fn config() -> Result<&'static Storage> {
    STORAGE
        .get()
        .context("release storage binding has not been initialized")
}
pub fn fsid() -> Result<&'static str> {
    Ok(config()?.fsid.as_str())
}
pub fn parent_subvolume() -> Result<u64> {
    Ok(config()?.parent_subvolume)
}
