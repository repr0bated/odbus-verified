//! Isolated ownership probes for BtrfsCache's userspace memory layers.
//!
//! Uses a temporary directory on /dev/shm (ordinary-filesystem fallback), so
//! this does not exercise native Btrfs snapshots, kernel cache, or live data.
//! Run each case in a fresh process; see the investigation report for commands.

use anyhow::{Context, Result, ensure};
use op_cache::BtrfsCache;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct MeasuredAllocator;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// Count requested Rust allocation bytes, not libc arena capacity or process
// RSS. Every operation delegates to System with the original layout/pointer.
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            if size >= layout.size() {
                allocated(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

fn checkpoint(label: &str, baseline: usize) {
    let live = LIVE.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed);
    println!(
        "phase={label} rust_live_bytes={live} delta_bytes={} peak_bytes={peak}",
        live as i128 - baseline as i128
    );
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn require_limit(name: &str, expected: usize) -> Result<()> {
    ensure!(
        std::env::var(name).ok().as_deref() == Some(expected.to_string().as_str()),
        "set {name}={expected} for this case"
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let case = std::env::args()
        .nth(1)
        .context("case: index | keys | cleanup")?;
    match case.as_str() {
        "index" => {
            require_limit("OPDBUS_CACHE_MEMORY_MAX_ENTRIES", 0)?;
            require_limit("OPDBUS_CACHE_MEMORY_MAX_BYTES", 0)?;
        }
        "keys" => {
            require_limit("OPDBUS_CACHE_MEMORY_MAX_ENTRIES", 32)?;
            require_limit("OPDBUS_CACHE_MEMORY_MAX_BYTES", 32)?;
        }
        "cleanup" => {
            require_limit("OPDBUS_CACHE_MEMORY_MAX_ENTRIES", 8)?;
            require_limit("OPDBUS_CACHE_MEMORY_MAX_BYTES", 4_194_304)?;
        }
        _ => anyhow::bail!("unknown case: {case}"),
    }
    let directory = tempfile::Builder::new()
        .prefix("op-cache-memory-probe-")
        .tempdir_in("/dev/shm")?;
    let root = directory.path().join("cache");
    let cache = BtrfsCache::new(root.clone()).await?;
    println!(
        "case={case} fixture=temporary ordinary-filesystem memory_accounting=Rust_requested_bytes"
    );
    let baseline = LIVE.load(Ordering::Relaxed);
    checkpoint("empty", baseline);

    match case.as_str() {
        "index" => {
            for entry in 0..256 {
                let text = format!("{entry:08}:{}", "x".repeat(65_536));
                cache.put_embedding(&text, &[1.0])?;
                if entry == 127 || entry == 255 {
                    drop(text);
                    checkpoint(if entry == 127 { "128_rows" } else { "256_rows" }, baseline);
                }
            }
            println!(
                "embedding_files={}",
                std::fs::read_dir(root.join("embeddings/vectors"))?.count()
            );
            cache.clear_embeddings()?;
            checkpoint("cleared_index", baseline);
        }
        "keys" => {
            for entry in 0..32 {
                let key = format!("{entry:08}:{}", "k".repeat(262_144));
                cache.put_raw(&key, &[1]).await?;
            }
            checkpoint("32_keys_32_payload_bytes", baseline);
            // This public operation clears the entire fast path, but leaves
            // the KV index intact. The allocation delta isolates key storage.
            cache.clear_embeddings()?;
            checkpoint("fast_path_cleared_kv_index_kept", baseline);
            let key = format!("{:08}:{}", 0, "k".repeat(262_144));
            ensure!(cache.get_raw(&key).await? == Some(vec![1]));
            drop(key);
            cache.clear()?;
            checkpoint("all_cleared", baseline);
        }
        "cleanup" => {
            {
                let vector = vec![1.0; 65_536];
                for entry in 0..8 {
                    cache.put_embedding(&format!("embedding-{entry}"), &vector)?;
                }
            }
            checkpoint("8_embeddings", baseline);
            // A future cutoff deterministically selects all fresh fixture
            // rows through the same cleanup path without waiting a day.
            let removed = cache.cleanup_old(-1)?;
            let files = std::fs::read_dir(root.join("embeddings/vectors"))?.count();
            ensure!(removed == 8 && files == 0, "fixture cleanup failed");
            checkpoint("after_cleanup_old", baseline);
            let still_readable = cache.get_embedding("embedding-0")?.is_some();
            println!(
                "removed_rows={removed} remaining_files={files} hit_after_cleanup={still_readable}"
            );
            ensure!(
                !still_readable,
                "expired embedding remains reachable in the fast path"
            );
            cache.clear_embeddings()?;
            checkpoint("after_explicit_fast_path_clear", baseline);
            ensure!(cache.get_embedding("embedding-0")?.is_none());
        }
        _ => unreachable!(),
    }
    drop(cache);
    checkpoint("cache_dropped", baseline);
    Ok(())
}
