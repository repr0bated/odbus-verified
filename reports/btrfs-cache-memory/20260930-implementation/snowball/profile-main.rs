use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
static LIVE:AtomicUsize=AtomicUsize::new(0);static PEAK:AtomicUsize=AtomicUsize::new(0);static BASELINE:AtomicUsize=AtomicUsize::new(0);
struct Tracking;
fn added(n:usize){let v=LIVE.fetch_add(n,Ordering::Relaxed)+n;PEAK.fetch_max(v,Ordering::Relaxed);}
unsafe impl GlobalAlloc for Tracking {
 unsafe fn alloc(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc(l)};if !p.is_null(){added(l.size());}p}
 unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc_zeroed(l)};if !p.is_null(){added(l.size());}p}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){unsafe{System.dealloc(p,l)};LIVE.fetch_sub(l.size(),Ordering::Relaxed);}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{let q=unsafe{System.realloc(p,l,n)};if !q.is_null(){if n>=l.size(){added(n-l.size());}else{LIVE.fetch_sub(l.size()-n,Ordering::Relaxed);}}q}
}
#[global_allocator]static ALLOC:Tracking=Tracking;
fn profile_checkpoint(phase:&str){let live=LIVE.load(Ordering::Relaxed);let peak=PEAK.load(Ordering::Relaxed);let baseline=BASELINE.load(Ordering::Relaxed);let smaps=std::fs::read_to_string("/proc/self/smaps_rollup").unwrap();let rss=smaps.lines().find(|s|s.starts_with("Rss:")).unwrap().split_whitespace().nth(1).unwrap().parse::<usize>().unwrap();assert!(rss<524288);println!("phase={phase} rust_live_delta={} rust_peak_delta={} rss_kib={rss}",live as i64-baseline as i64,peak as i64-baseline as i64);}
#[tokio::main(flavor="current_thread")]
async fn main()->anyhow::Result<()> {
 let n:usize=std::env::args().nth(1).unwrap().parse()?;assert!(n<=10000);
 let fixture=tempfile::Builder::new().prefix("snowball-bounded-profile-").tempdir_in("/dev/shm")?;let timing=fixture.path().join("timing");let vectors=fixture.path().join("vectors");std::fs::create_dir(&timing)?;std::fs::create_dir(&vectors)?;
 let record=serde_json::to_vec(&serde_json::json!({"timestamp":1,"category":"fixture","action":"mutation","hash":"h","vector":[],"data":{"plugin_id":"fixture","operation":"mutation","payload":{"event_id":1,"event_hash":"abc","prev_hash":"zero","input_patch_hash":"digest","input_payload":{"message":"x".repeat(512),"items":["one","two","three"]}},"metadata":{"actor_id":"principal-1","session_genesis":"fixture-private","decision":"Allow"}}}))?;
 for i in 1..=n{std::fs::write(timing.join(format!("block-{i:012}.json")),&record)?;std::fs::write(vectors.join(format!("vec-{i:012}.bin")),1.0f32.to_le_bytes())?;}
 drop(record);drop(tokio::fs::read(timing.join("block-000000000001.json")).await?);
 let base=LIVE.load(Ordering::Relaxed);BASELINE.store(base,Ordering::Relaxed);PEAK.store(base,Ordering::Relaxed);let start=std::time::Instant::now();let (pending,present)=snowball::profile_collect(timing,vectors).await?;profile_checkpoint("pending_selected");assert_eq!(present,n);assert!(pending.is_empty());drop(pending);profile_checkpoint("pending_dropped");println!("records={n} already_present={present} elapsed_ns={}",start.elapsed().as_nanos());Ok(())
}
