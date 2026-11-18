//! Set up and check the ublk Cozo block store. Touches only the database:
//! no ublk device, no btrfs, no mounts, no Incus.
//!
//! usage: ublk-block-store <init|verify|status>
//!   init    create/open the store, register `pool-default-persist` (500 GiB)
//!   verify  write, read back and delete one scratch chunk; print row counts
//!   status  print the device row and row counts
//!
//! Path: $OP_UBLK_COZO_DB_PATH, else /var/lib/op-dbus/ublk-cozo. Refuses a
//! path on btrfs: the store must never live in the pool it backs.

use op_cozo_store::{
    BlockDevice, CozoBlockStore, POOL_DEFAULT_PERSIST_BYTES, POOL_DEFAULT_PERSIST_DEVICE,
    POOL_PERSIST_ROLE, UBLK_CHUNK_BYTES, ublk_cozo_db_path,
};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "init" => init(),
        "verify" => verify(),
        "status" => status(),
        _ => Err("usage: ublk-block-store <init|verify|status>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ublk-block-store: {error}");
            ExitCode::FAILURE
        }
    }
}

fn open() -> Result<CozoBlockStore, String> {
    let path = ublk_cozo_db_path();
    refuse_btrfs(&path)?;
    let store = CozoBlockStore::open(&path).map_err(|e| e.to_string())?;
    // Re-check the created directory itself, not just its parent.
    refuse_btrfs(store.path())?;
    println!("store: {}", store.path().display());
    Ok(store)
}

/// Check the nearest existing ancestor (the store may not exist yet).
fn refuse_btrfs(path: &Path) -> Result<(), String> {
    let existing = path
        .ancestors()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| format!("no existing ancestor of {}", path.display()))?;
    let fs = nix::sys::statfs::statfs(existing)
        .map_err(|e| format!("statfs {}: {e}", existing.display()))?;
    if fs.filesystem_type() == nix::sys::statfs::BTRFS_SUPER_MAGIC {
        return Err(format!(
            "{} is on btrfs; the ublk block store must not live on the pool it backs",
            existing.display()
        ));
    }
    Ok(())
}

fn persist_device(store: &CozoBlockStore) -> Result<BlockDevice, String> {
    store
        .device(POOL_DEFAULT_PERSIST_DEVICE)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{POOL_DEFAULT_PERSIST_DEVICE} is not registered; run `init` first"))
}

fn print_device(device: &BlockDevice) {
    println!(
        "device: id={} size_bytes={} ({} GiB) block_size={} chunk_size={} chunks={} created_at={} role={} write_generation={}",
        device.id,
        device.size_bytes,
        device.size_bytes >> 30,
        device.block_size,
        device.chunk_size,
        device.chunk_count(),
        device.created_at,
        device.role,
        device.write_generation
    );
}

fn print_counts(label: &str, store: &CozoBlockStore) -> Result<(), String> {
    let counts = store.counts().map_err(|e| e.to_string())?;
    println!(
        "rows ({label}): devices={} blocks={} tombstones={}",
        counts.devices, counts.blocks, counts.tombstones
    );
    Ok(())
}

fn init() -> Result<(), String> {
    let store = open()?;
    let device = store
        .register_device(
            POOL_DEFAULT_PERSIST_DEVICE,
            POOL_DEFAULT_PERSIST_BYTES,
            POOL_PERSIST_ROLE,
        )
        .map_err(|e| e.to_string())?;
    store.flush().map_err(|e| e.to_string())?;
    print_device(&device);
    print_counts("after init", &store)
}

fn status() -> Result<(), String> {
    let store = open()?;
    print_device(&persist_device(&store)?);
    print_counts("now", &store)
}

fn verify() -> Result<(), String> {
    let store = open()?;
    let device = persist_device(&store)?;
    print_device(&device);
    let chunk = UBLK_CHUNK_BYTES as usize;
    // Last chunk: far from every btrfs superblock copy (64 KiB, 64 MiB, 256 GiB).
    let idx = device.chunk_count() - 1;
    let mut out = vec![0u8; chunk];
    if store
        .read_block(&device, idx, &mut out)
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "chunk {idx} already holds data; refusing to use it as scratch"
        ));
    }
    print_counts("before", &store)?;

    let scratch: Vec<u8> = (0..chunk)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
        .collect();
    store
        .write_block(&device, idx, &scratch)
        .map_err(|e| e.to_string())?;
    store.flush().map_err(|e| e.to_string())?;
    println!(
        "write: chunk {idx} ({} bytes at offset {}) cblk-wrapped",
        chunk,
        idx * u64::from(UBLK_CHUNK_BYTES)
    );
    print_counts("after write", &store)?;

    let stored = store
        .read_block(&device, idx, &mut out)
        .map_err(|e| e.to_string())?;
    if !stored || out != scratch {
        return Err(format!(
            "read-back mismatch on chunk {idx} (stored={stored})"
        ));
    }
    println!("read: chunk {idx} matches, cblk crc32c verified");

    store
        .discard_block(&device, idx)
        .map_err(|e| e.to_string())?;
    store.flush().map_err(|e| e.to_string())?;
    let stored = store
        .read_block(&device, idx, &mut out)
        .map_err(|e| e.to_string())?;
    if stored || out.iter().any(|b| *b != 0) {
        return Err(format!("chunk {idx} still readable after delete"));
    }
    println!("delete: chunk {idx} removed, reads as zeros");
    print_counts("after delete", &store)
}
