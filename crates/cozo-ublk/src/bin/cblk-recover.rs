//! Offline recovery for CBLK-wrapped ublk chunks.
//!
//! Scans a Cozo/RocksDB data directory (WAL `.log`, SST `.sst`, and any other
//! files) as raw bytes for the `CBLK` magic. Does not open Cozo or RocksDB and
//! does not interpret guest filesystem contents — every valid chunk is opaque
//! device data.
//!
//! usage:
//!   cblk-recover <store-dir> <device-id> <output-image> [size-bytes]
//!
//! If `size-bytes` is omitted, the image size is inferred from the highest
//! recovered chunk index for that device. For each chunk the highest
//! generation wins; when that is a discard tombstone the chunk stays a hole,
//! so discarded data is never resurrected.

use op_cozo_store::cblk::{self, CBLK_CHUNK_SIZE, CblkRecoveredChunk, CblkScanSummary};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("cblk-recover: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() < 3 || args.len() > 4 {
        return Err(
            "usage: cblk-recover <store-dir> <device-id> <output-image> [size-bytes]".into(),
        );
    }
    let store_dir = PathBuf::from(&args[0]);
    let device_id = &args[1];
    let output = PathBuf::from(&args[2]);
    let size_override = match args.get(3) {
        Some(text) => Some(
            text.parse::<u64>()
                .map_err(|e| format!("size-bytes: {e}"))?,
        ),
        None => None,
    };

    let (chunks, summary) = cblk::scan_directory(&store_dir).map_err(|e| e.to_string())?;
    print_summary(&summary);

    let device_chunks: Vec<&CblkRecoveredChunk> = chunks
        .iter()
        .filter(|c| c.device_id == *device_id)
        .collect();
    if device_chunks.is_empty() {
        return Err(format!(
            "no valid CBLK chunks found for device {device_id:?}"
        ));
    }

    let size_bytes = match size_override {
        Some(size) => size,
        None => {
            let max_idx = device_chunks.iter().map(|c| c.chunk_idx).max().unwrap_or(0);
            (max_idx + 1) * u64::from(CBLK_CHUNK_SIZE)
        }
    };

    let owned: Vec<CblkRecoveredChunk> = device_chunks.into_iter().cloned().collect();
    let written = cblk::write_sparse_image(&owned, device_id, size_bytes, &output)
        .map_err(|e| e.to_string())?;
    println!(
        "wrote {} chunks for device {device_id} into {} ({} bytes)",
        written,
        output.display(),
        size_bytes
    );
    Ok(())
}

fn print_summary(summary: &CblkScanSummary) {
    println!(
        "scan: files={} candidates={} valid={} tombstones={} corrupt={}",
        summary.files_scanned,
        summary.candidates,
        summary.valid,
        summary.tombstones,
        summary.corrupt
    );
    if summary.device_ids.is_empty() {
        println!("devices: (none)");
    } else {
        print!("devices:");
        for (id, count) in &summary.device_ids {
            print!(" {id}={count}");
        }
        println!();
    }
}
