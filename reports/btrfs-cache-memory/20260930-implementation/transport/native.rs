#![allow(dead_code)]
#[path = "/srv/git/odbus-verified/crates/op-cache/src/numa.rs"] mod numa;
#[path = "/srv/git/odbus-verified/crates/op-cache/src/snapshot_manager.rs"] mod snapshot_manager;
#[path = "/srv/git/odbus-verified/crates/op-cache/src/btrfs_cache.rs"] mod btrfs_cache;
use anyhow::{Result, ensure, Context};
use std::{alloc::{GlobalAlloc, Layout, System}, path::{Path, PathBuf}, process::Stdio, sync::atomic::{AtomicUsize, Ordering}, time::{Duration, Instant}};
use btrfs_cache::BtrfsCache;
use snapshot_manager::CACHE_MEMBERS;
struct Tracker;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn add(n: usize) { let value=LIVE.fetch_add(n,Ordering::Relaxed)+n; PEAK.fetch_max(value,Ordering::Relaxed); }
unsafe impl GlobalAlloc for Tracker {
    unsafe fn alloc(&self,l:Layout)->*mut u8 {let p=unsafe{System.alloc(l)}; if !p.is_null(){add(l.size());} p}
    unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8 {let p=unsafe{System.alloc_zeroed(l)}; if !p.is_null(){add(l.size());} p}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout) {unsafe{System.dealloc(p,l)}; LIVE.fetch_sub(l.size(),Ordering::Relaxed);}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {let p=unsafe{System.realloc(p,l,n)}; if !p.is_null(){if n>=l.size(){add(n-l.size());}else{LIVE.fetch_sub(l.size()-n,Ordering::Relaxed);}} p}
}
#[global_allocator] static ALLOC:Tracker=Tracker;
fn metric(phase:&str,cycle:usize,start:Instant) -> Result<()> {
    let live=LIVE.load(Ordering::Relaxed); let peak=PEAK.swap(live,Ordering::Relaxed);
    let maps=std::fs::read_to_string("/proc/self/smaps_rollup")?;
    let field=|name:&str| maps.lines().find_map(|l| l.strip_prefix(name)).and_then(|v|v.split_whitespace().next()).unwrap_or("0");
    let fds=std::fs::read_dir("/proc/self/fd")?.count();
    let tasks=std::fs::read_dir("/proc/self/task")?.collect::<std::io::Result<Vec<_>>>()?;
    let children:usize=tasks.iter().map(|t|std::fs::read_to_string(t.path().join("children")).unwrap_or_default().split_whitespace().count()).sum();
    println!("phase={phase} cycle={cycle} ms={} rust_live={live} rust_peak={peak} rss_kib={} pss_kib={} anonymous_kib={} swap_kib={} fds={fds} threads={} children={children}",start.elapsed().as_millis(),field("Rss:"),field("Pss:"),field("Anonymous:"),field("Swap:"),tasks.len());
    ensure!(field("Rss:").parse::<usize>()? < 512*1024,"RSS limit exceeded");
    Ok(())
}
fn remove_subvolume(path:&Path)->Result<()> {
    if !path.exists(){return Ok(());}
    let out=std::process::Command::new("/usr/bin/btrfs").args(["subvolume","delete"]).arg(path).output()?;
    ensure!(out.status.success(),"delete {}: {}",path.display(),String::from_utf8_lossy(&out.stderr)); Ok(())
}
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        fn walk(p:&Path) {
            let Ok(meta)=std::fs::symlink_metadata(p) else{return};
            if !meta.is_dir() || meta.file_type().is_symlink(){return;}
            use std::os::unix::fs::MetadataExt;
            if let Ok(entries)=std::fs::read_dir(p){for entry in entries.flatten(){walk(&entry.path());}}
            if meta.ino()==256 {if let Err(e)=remove_subvolume(p){eprintln!("CLEANUP_FAILED {e:#}");}}
        }
        walk(&self.0);
        if let Err(e)=std::fs::remove_dir_all(&self.0){eprintln!("CLEANUP_FAILED {} {e}",self.0.display());}
    }
}
#[path="/srv/git/odbus-verified/crates/op-cache/src/managed_process.rs"] mod managed_process;
#[tokio::main(flavor="current_thread")]
async fn main()->Result<()> {
    let start=Instant::now();
    let fixture=Fixture(tempfile::Builder::new().prefix("op-cache-fixed-transport-").tempdir_in("/var/tmp")?.keep());
    println!("owned_fixture={}",fixture.0.display());
    let cache=BtrfsCache::new(fixture.0.join("cache")).await?;
    let snapshots=fixture.0.join("snapshots");
    let manager=snapshot_manager::SnapshotManager::new(cache.cache_dir().clone(),snapshot_manager::SnapshotConfig { snapshot_dir:snapshots.clone(),..Default::default() });
    for key in 0..16 {cache.put_embedding(&format!("key-{key}"),&vec![key as f32;1024])?;}
    for member in ["blocks","queries","diffs","kv"] {std::fs::write(cache.cache_dir().join(member).join("probe.bin"),vec![42u8;4096])?;}
    manager.validate_layout().await?;
    metric("warm",0,start)?;
    for cycle in 1..=3 {
        let temporary=manager.create_transport_snapshot().await?;
        let snapshot=temporary.path().to_owned();
        ensure!(manager.list_snapshots().await?.is_empty(),"ephemeral entered manual rotation");
        let destination=fixture.0.join(format!("received-{cycle}")); std::fs::create_dir(&destination)?;
        metric("snapshot",cycle,start)?;
        for member in CACHE_MEMBERS {
            let mut sender=tokio::process::Command::new("btrfs"); sender.arg("send").arg(snapshot.join(member));
            let mut receiver=tokio::process::Command::new("btrfs"); receiver.arg("receive").arg(&destination);
            let bytes=managed_process::pipeline(&mut sender,&mut receiver,Duration::from_secs(30)).await?;
            ensure!(bytes>0,"empty send stream");
        }
        snapshot_manager::SnapshotManager::verify_members(&destination).await?;
        for member in ["blocks","queries","diffs","kv"] {ensure!(std::fs::read(destination.join(member).join("probe.bin"))?==vec![42u8;4096],"member mismatch");}
        for entry in std::fs::read_dir(cache.cache_dir().join("embeddings/vectors"))? {
            let entry=entry?; ensure!(std::fs::read(entry.path())?==std::fs::read(destination.join("embeddings/vectors").join(entry.file_name()))?,"vector mismatch");
        }
        temporary.cleanup().await?;
        ensure!(!snapshot.exists(),"source temporary retained");
        ensure!(destination.exists(),"destination data lost");
        for member in CACHE_MEMBERS.iter().rev() {remove_subvolume(&destination.join(member))?;}
        std::fs::remove_dir(destination)?;
        ensure!(std::fs::read_dir(&snapshots)?.count()==0,"temporary snapshot retained");
        metric("drained",cycle,start)?;
    }
    // Cancellation while the owned creation task is taking its six snapshots.
    {
        let creation=manager.create_transport_snapshot(); tokio::pin!(creation);
        tokio::select! {
            result=&mut creation => { drop(result?); },
            observed=tokio::time::timeout(Duration::from_secs(5),async {
                loop { if std::fs::read_dir(&snapshots).unwrap().count()>0 {break;} tokio::time::sleep(Duration::from_millis(1)).await; }
            }) => {observed?;}
        }
    }
    tokio::time::timeout(Duration::from_secs(15),async {
        loop {if std::fs::read_dir(&snapshots).unwrap().count()==0 {break;} tokio::time::sleep(Duration::from_millis(20)).await;}
    }).await.context("cancelled snapshot creation did not clean")?;
    // An error/cancelled receive owns its partial set; only successful receives
    // call preserve. Construct one real received member, then cancel ownership.
    let partial=fixture.0.join("partial-receive");
    let guard=snapshot_manager::TemporarySet::create(partial.clone(),snapshot_manager::transfer_permit().await?)?;
    let temporary=manager.create_transport_snapshot().await?;
    let mut sender=tokio::process::Command::new("btrfs");sender.arg("send").arg(temporary.path().join("root"));
    let mut receiver=tokio::process::Command::new("btrfs");receiver.arg("receive").arg(&partial);
    managed_process::pipeline(&mut sender,&mut receiver,Duration::from_secs(30)).await?;
    drop(guard);temporary.cleanup().await?;
    tokio::time::timeout(Duration::from_secs(15),async {while partial.exists(){tokio::time::sleep(Duration::from_millis(20)).await;}}).await?;
    metric("cancelled_drained",3,start)?;
    println!("PASS: three complete six-member cycles; source cleanup; destination integrity; cancelled creation; partial received cleanup");
    Ok(())
}
