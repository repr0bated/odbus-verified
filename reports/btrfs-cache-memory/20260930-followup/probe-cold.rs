use anyhow::{Result, ensure};
use crate::btrfs_cache::BtrfsCache;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Instant;
struct Measured;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn added(n: usize) { let v=LIVE.fetch_add(n,Ordering::Relaxed)+n; PEAK.fetch_max(v,Ordering::Relaxed); }
unsafe impl GlobalAlloc for Measured {
 unsafe fn alloc(&self,l:Layout)->*mut u8 { let p=unsafe{System.alloc(l)}; if !p.is_null(){added(l.size());} p }
 unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8 {let p=unsafe{System.alloc_zeroed(l)};if !p.is_null(){added(l.size());}p}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){unsafe{System.dealloc(p,l)};LIVE.fetch_sub(l.size(),Ordering::Relaxed);}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {let q=unsafe{System.realloc(p,l,n)};if !q.is_null(){if n>=l.size(){added(n-l.size());}else{LIVE.fetch_sub(l.size()-n,Ordering::Relaxed);}}q}
}
#[global_allocator] static ALLOCATOR: Measured=Measured;
fn cpu_ns()->u128 {let mut ts=libc::timespec{tv_sec:0,tv_nsec:0};assert_eq!(unsafe{libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID,&mut ts)},0);ts.tv_sec as u128*1_000_000_000+ts.tv_nsec as u128}
fn checkpoint(case:&str,phase:&str,cycle:usize,baseline:usize,start:Instant,cpu:u128,lat:&mut [u128])->Result<()> {
 let live=LIVE.load(Ordering::Relaxed);let peak=PEAK.load(Ordering::Relaxed);
 let smaps=std::fs::read_to_string("/proc/self/smaps_rollup")?;
 let mut rss=0;let mut anon=0;let mut pss=0;let mut dirty=0;
 for line in smaps.lines(){let mut p=line.split_whitespace();match p.next(){Some("Rss:")=>rss=p.next().unwrap().parse::<u64>()?,Some("Anonymous:")=>anon=p.next().unwrap().parse::<u64>()?,Some("Pss:")=>pss=p.next().unwrap().parse::<u64>()?,Some("Private_Dirty:")=>dirty=p.next().unwrap().parse::<u64>()?,_=>{}}}
 ensure!(rss<=512*1024,"RSS guard exceeded");
 let fd=std::fs::read_dir("/proc/self/fd")?.count();let threads=std::fs::read_dir("/proc/self/task")?.count();
 let mut children=0;for task in std::fs::read_dir("/proc/self/task")? {children+=std::fs::read_to_string(task?.path().join("children"))?.split_whitespace().count();}
 lat.sort_unstable();let quantile=|q:usize| if lat.is_empty(){0}else{lat[(lat.len()-1)*q/100]};
 println!("{}",serde_json::json!({"case":case,"phase":phase,"cycle":cycle,"pid":std::process::id(),"rust_live_bytes":live,"rust_delta_bytes":live as i64-baseline as i64,"rust_peak_bytes":peak,"rss_kib":rss,"pss_kib":pss,"anonymous_kib":anon,"private_dirty_kib":dirty,"fds":fd,"threads":threads,"children":children,"wall_ns":start.elapsed().as_nanos(),"cpu_ns":cpu_ns()-cpu,"latency_samples":lat.len(),"latency_p50_ns":quantile(50),"latency_p95_ns":quantile(95),"latency_max_ns":lat.last().copied().unwrap_or(0)}));
 PEAK.store(LIVE.load(Ordering::Relaxed),Ordering::Relaxed);Ok(())
}
fn keys(n:usize,len:usize)->Vec<String>{(0..n).map(|i|{let mut s=format!("{i:08}:");s.extend(std::iter::repeat_n('x',len-s.len()));s}).collect()}
fn populate(c:&BtrfsCache,k:&[String],size:usize,value:f32)->Result<()> {let v=vec![value;size/4];for key in k{c.put_embedding(key,&v)?;}Ok(())}
fn read(c:&BtrfsCache,k:&str,size:usize,expected:f32)->Result<()> {let v=c.get_embedding(k)?.unwrap();ensure!(v.len()==size/4&&v.iter().all(|x| *x==expected),"incorrect cached value");drop(v);Ok(())}
#[tokio::main(flavor="current_thread")]
async fn main()->Result<()> {
 let args:Vec<String>=std::env::args().collect();let case=args.get(1).expect("case").as_str();let size:usize=args.get(2).expect("size").parse()?;ensure!(size==4096||size==65536,"bounded size");
 let cycles=10;let directory=tempfile::Builder::new().prefix("op-cache-followup-").tempdir_in("/dev/shm")?;
 let fsname=std::process::Command::new("stat").args(["-f","-c","%T"]).arg(directory.path()).output()?;ensure!(String::from_utf8_lossy(&fsname.stdout).trim()=="tmpfs","requires explicit tmpfs fallback");
 println!("{}",serde_json::json!({"metadata":true,"case":case,"cycles":cycles,"keys":16,"payload_bytes":size,"fixture":"/dev/shm tmpfs ordinary-filesystem fallback","requested_bytes":"Rust GlobalAlloc only; native direct malloc and allocator capacity excluded","pid":std::process::id()}));
 let start=Instant::now();let cpu=cpu_ns();let baseline=LIVE.load(Ordering::Relaxed);checkpoint(case,"process_ready",0,baseline,start,cpu,&mut [])?;
 let fixed=Arc::new(keys(16,32));let mut retained:Vec<Vec<u8>>=Vec::with_capacity(cycles);let mut samples=[0u128;128];
 if case=="negative"||case=="retention" {
  for cycle in 1..=cycles {let t=Instant::now();if case=="retention" {retained.push(vec![1u8;1024*1024]);std::hint::black_box(&retained);}else{for _ in 0..128{let v=vec![1u8;size];std::hint::black_box(&v);drop(v);}}samples[0]=t.elapsed().as_nanos();checkpoint(case,"cycle_end",cycle,baseline,start,cpu,&mut samples[..1])?;}
  drop(retained);drop(fixed);checkpoint(case,"released",cycles,baseline,start,cpu,&mut [])?;return Ok(());
 }
 if case=="construction" {
  for cycle in 1..=cycles {let c=BtrfsCache::new(directory.path().join(format!("cache-{cycle}"))).await?;populate(&c,&fixed,size,1.0)?;read(&c,&fixed[0],size,1.0)?;checkpoint(case,"populated",cycle,baseline,start,cpu,&mut [])?;drop(c);std::fs::remove_dir_all(directory.path().join(format!("cache-{cycle}")))?;checkpoint(case,"cycle_end",cycle,baseline,start,cpu,&mut [])?;}
  drop(fixed);checkpoint(case,"released",cycles,baseline,start,cpu,&mut [])?;return Ok(());
 }
 let c=Arc::new(BtrfsCache::new(directory.path().join("cache")).await?);populate(&c,&fixed,size,1.0)?;
 if case=="kv-warm" {for k in fixed.iter(){c.put_raw(k,&vec![1u8;size]).await?;}}
 checkpoint(case,"warmed",0,baseline,start,cpu,&mut [])?;
 for cycle in 1..=cycles {
  let mut n=0;
  match case {
   "cold-corruption"=>{use sha2::{Digest,Sha256};let mut h=Sha256::new();h.update(fixed[0].as_bytes());let path=directory.path().join("cache/embeddings/vectors").join(format!("{:x}.vec",h.finalize()));std::fs::write(&path,[0u8])?;ensure!(c.get_embedding(&fixed[0]).is_err(),"truncated file must error on cold API");let t=Instant::now();c.put_embedding(&fixed[0],&vec![1.0;size/4])?;read(&c,&fixed[0],size,1.0)?;samples[0]=t.elapsed().as_nanos();n=1;},
   "warm"=>{for i in 0..128{let t=Instant::now();read(&c,&fixed[i%16],size,1.0)?;samples[n]=t.elapsed().as_nanos();n+=1;}},
   "kv-warm"=>{for i in 0..128 {let t=Instant::now();let v=c.get_raw(&fixed[i%16]).await?.unwrap();ensure!(v.len()==size&&v[0]==1,"incorrect KV");drop(v);samples[n]=t.elapsed().as_nanos();n+=1;}},
   "overwrite"=>{for k in fixed.iter(){let t=Instant::now();c.put_embedding(k,&vec![cycle as f32;size/4])?;read(&c,k,size,cycle as f32)?;samples[n]=t.elapsed().as_nanos();n+=1;}},
   "clear"=>{c.clear()?;checkpoint(case,"cleared",cycle,baseline,start,cpu,&mut [])?;let t=Instant::now();populate(&c,&fixed,size,1.0)?;for k in fixed.iter(){read(&c,k,size,1.0)?;}samples[0]=t.elapsed().as_nanos();n=1;},
   "burst"=>{let barrier=Arc::new(std::sync::Barrier::new(4));let mut workers=Vec::new();for _ in 0..4{let c=c.clone();let k=fixed.clone();let b=barrier.clone();workers.push(std::thread::spawn(move||->Result<[u128;32]>{let mut s=[0;32];b.wait();for i in 0..32{let t=Instant::now();read(&c,&k[i%16],size,1.0)?;s[i]=t.elapsed().as_nanos();}Ok(s)}));}for w in workers{let s=w.join().unwrap()?;samples[n..n+32].copy_from_slice(&s);n+=32;}},
   "growth"=>{let target=(cycle*24+16).min(256);let previous=((cycle-1)*24+16).min(256);let text=keys(target,65536);let t=Instant::now();for k in &text[previous..target]{c.put_embedding(k,&[1.0])?;}drop(text);samples[0]=t.elapsed().as_nanos();n=1;},
   "expiry"|"expiry-mitigated"=>{let removed=c.cleanup_old(-1)?;ensure!(removed==16,"expected removed 16, got {removed}");let files=std::fs::read_dir(directory.path().join("cache/embeddings/vectors"))?.count();let hit=c.get_embedding(&fixed[0])?.is_some();ensure!(files==0,"backing files remain");ensure!(hit==(case=="expiry"),"unexpected expiry hit: {hit}");println!("{}",serde_json::json!({"case":case,"cycle":cycle,"removed":removed,"files":files,"hit_after_expiry":hit}));checkpoint(case,"expired",cycle,baseline,start,cpu,&mut [])?;populate(&c,&fixed,size,1.0)?;},
   _=>anyhow::bail!("unknown case"),
  }
  checkpoint(case,"cycle_end",cycle,baseline,start,cpu,&mut samples[..n])?;
 }
 c.clear()?;checkpoint(case,"final_clear",cycles,baseline,start,cpu,&mut [])?;drop(c);drop(fixed);drop(retained);checkpoint(case,"cache_dropped",cycles,baseline,start,cpu,&mut [])?;Ok(())
}
