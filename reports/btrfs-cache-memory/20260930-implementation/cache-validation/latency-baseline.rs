#![allow(dead_code)]
#[path = "/srv/git/odbus-verified/crates/op-cache/src/managed_process.rs"] mod managed_process;
#[path = "/srv/git/odbus-verified/reports/btrfs-cache-memory/20260930-followup/source/numa.rs"] mod numa;
#[path = "/srv/git/odbus-verified/reports/btrfs-cache-memory/20260930-followup/source/snapshot_manager.rs"] mod snapshot_manager;
#[path = "/srv/git/odbus-verified/reports/btrfs-cache-memory/20260930-followup/source/btrfs_cache.rs"] mod btrfs_cache;

#[tokio::main(flavor="current_thread")]
async fn main() -> anyhow::Result<()> {
    let fixture = tempfile::tempdir_in("/dev/shm")?;
    let cache = btrfs_cache::BtrfsCache::new(fixture.path().join("cache")).await?;
    for bytes in [256, 65536] {
        let payload = vec![7u8; bytes];
        cache.put_raw("latency", &payload).await?;
        for _ in 0..10 { assert_eq!(cache.get_raw("latency").await?.as_deref(), Some(payload.as_slice())); }
        let started = std::time::Instant::now();
        for _ in 0..1000 { assert_eq!(cache.get_raw("latency").await?.as_deref(), Some(payload.as_slice())); }
        println!("payload_bytes={bytes} reads=1000 elapsed_us={} us_per_read={:.3}", started.elapsed().as_micros(), started.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(())
}
