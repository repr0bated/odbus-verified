#![allow(dead_code)]
#[path = "native-source/numa.rs"] mod numa;
#[path = "native-source/snapshot_manager.rs"] mod snapshot_manager;
#[path = "native-source/btrfs_cache.rs"] mod btrfs_cache;
use anyhow::{Result, ensure};
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
async fn cancel_probe(start:Instant)->Result<()> {
    for trial in 1..=3 {
        let sender=tokio::process::Command::new("/usr/bin/sleep").arg("10").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let receiver=tokio::process::Command::new("/usr/bin/sleep").arg("10").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let pids=[sender.id().unwrap(),receiver.id().unwrap()];
        let result=tokio::time::timeout(Duration::from_millis(100),btrfs_cache::investigation_pipe(sender,receiver)).await;
        ensure!(result.is_err(),"expected stalled transport timeout");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let survivors:Vec<_>=pids.iter().filter(|pid|Path::new(&format!("/proc/{pid}")).exists()).copied().collect();
        println!("cancellation_trial={trial} survivors_after_timeout={survivors:?}");
        for pid in pids {unsafe{libc::kill(pid as i32,libc::SIGTERM);}}
        tokio::time::sleep(Duration::from_millis(200)).await;
        let remaining=pids.iter().filter(|pid|Path::new(&format!("/proc/{pid}")).exists()).count();
        println!("cancellation_trial={trial} remaining_after_explicit_fixture_cleanup={remaining}");
        ensure!(remaining==0,"fixture children not reaped");
        metric("cancellation_drained",trial,start)?;
    } Ok(())
}
fn numa_probe()->Result<()> {
    let topology=numa::NumaTopology::detect()?;
    println!("topology_nodes={} current_node={:?} node0_memory_kb={:?}",topology.node_count(),topology.current_node(),topology.get_node(0).map(|n|(n.memory_total_kb,n.memory_free_kb)));
    for policy in [numa::NumaMemoryPolicy::Default,numa::NumaMemoryPolicy::Preferred(0),numa::NumaMemoryPolicy::Bind(vec![0]),numa::NumaMemoryPolicy::Interleave(vec![0])] {
        std::thread::spawn(move || {
            let result=numa::NumaOptimizer::apply_memory_policy(&policy);
            let mut mode:libc::c_int=-1; let mut mask:libc::c_ulong=0;
            let status=unsafe{libc::syscall(libc::SYS_get_mempolicy,&mut mode as *mut libc::c_int,&mut mask as *mut libc::c_ulong,64usize,std::ptr::null::<libc::c_void>(),0usize)};
            let error=if status==0{String::new()}else{std::io::Error::last_os_error().to_string()};
            println!("policy={policy:?} apply={result:?} query_status={status} query_mode={mode} query_mask={mask:x} query_error={error}");
        }).join().map_err(|_|anyhow::anyhow!("policy thread panicked"))?;
    }
    std::thread::spawn(|| -> Result<()> {
        let allowed=numa::NumaOptimizer::current_cpu_affinity().unwrap();
        numa::NumaOptimizer::apply_cpu_affinity_to(&[allowed[0]],"probe-only thread")?;
        let readback=numa::NumaOptimizer::current_cpu_affinity().unwrap();
        ensure!(readback==vec![allowed[0]],"affinity readback failed");
        println!("affinity_before={allowed:?} affinity_after={readback:?}");
        Ok(())
    }).join().map_err(|_|anyhow::anyhow!("affinity thread panicked"))??;
    Ok(())
}
#[tokio::main(flavor="current_thread")]
async fn main()->Result<()> {
    let start=Instant::now();
    let mode=std::env::args().nth(1).unwrap_or_else(||"native".into());
    println!("mode={mode} pid={} transport=local-no-ssh",std::process::id());
    metric("startup",0,start)?;
    if mode=="cancel" {return cancel_probe(start).await;}
    if mode=="numa" {return numa_probe();}
    ensure!(mode=="native","unknown mode");
    let parent=tempfile::Builder::new().prefix("op-cache-native-followup-").tempdir_in("/var/tmp")?.keep();
    println!("owned_fixture={}",parent.display());
    let fixture=Fixture(parent);
    let cache=BtrfsCache::new(fixture.0.join("cache")).await?;
    for key in 0..16 {cache.put_embedding(&format!("key-{key}"),&vec![key as f32;1024])?;}
    for member in ["blocks","queries","diffs","kv"] {std::fs::write(cache.cache_dir().join(member).join("probe.bin"),vec![42u8;4096])?;}
    for member in CACHE_MEMBERS {
        let source=if member=="root" {cache.cache_dir().clone()} else {cache.cache_dir().join(member)};
        use std::os::unix::fs::MetadataExt;
        ensure!(std::fs::metadata(&source)?.ino()==256,"not subvolume");
        let out=std::process::Command::new("/usr/bin/btrfs").args(["property","get"]).arg(&source).arg("compression").output()?;
        ensure!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("zstd"),"missing compression policy");
        println!("member={member} source_inode=256 compression_policy=zstd");
    }
    metric("warm",0,start)?;
    for cycle in 1..=3 {
        let snapshot=cache.create_snapshot().await?;
        let dest=fixture.0.join(format!("received-{cycle}")); std::fs::create_dir(&dest)?;
        metric("snapshot",cycle,start)?;
        for member in CACHE_MEMBERS {
            let sender=tokio::process::Command::new("/usr/bin/btrfs").arg("send").arg(snapshot.join(member)).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
            let receiver=tokio::process::Command::new("/usr/bin/btrfs").arg("receive").arg(&dest).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
            btrfs_cache::investigation_pipe(sender,receiver).await?;
        }
        snapshot_manager::SnapshotManager::verify_members(&dest).await?;
        for member in ["blocks","queries","diffs","kv"] {ensure!(std::fs::read(dest.join(member).join("probe.bin"))?==vec![42u8;4096],"member data mismatch");}
        let source_vectors=cache.cache_dir().join("embeddings/vectors");
        let mut checked=0;
        for entry in std::fs::read_dir(source_vectors)? {let entry=entry?;ensure!(std::fs::read(entry.path())?==std::fs::read(dest.join("embeddings/vectors").join(entry.file_name()))?,"vector mismatch");checked+=1;}
        ensure!(checked==16,"missing vectors");
        let nested=dest.join("root/embeddings");
        ensure!(!nested.exists() || std::fs::read_dir(nested)?.count()==0,"parent snapshot unexpectedly included nested payload");
        println!("cycle={cycle} verified_members=6 verified_vectors={checked} root_nested_stub_empty=true");
        metric("transfer_drained",cycle,start)?;
        ensure!(cache.delete_all_snapshots().await?==1,"unexpected snapshot count");
        for member in CACHE_MEMBERS.iter().rev(){remove_subvolume(&dest.join(member))?;}
        std::fs::remove_dir(&dest)?;
        ensure!(cache.list_snapshots().await?.is_empty(),"snapshots remain");
        tokio::time::sleep(Duration::from_millis(200)).await;
        metric("explicit_cleanup_drained",cycle,start)?;
    }
    drop(cache); metric("cache_dropped",3,start)?;
    let path=fixture.0.clone(); drop(fixture);
    ensure!(!path.exists(),"fixture cleanup failed");
    println!("fixture_removed=true");
    Ok(())
}
