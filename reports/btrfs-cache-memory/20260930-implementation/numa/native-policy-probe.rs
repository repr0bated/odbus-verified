#![allow(dead_code)]
#[path = "../../../../crates/op-cache/src/numa.rs"] mod numa;
use anyhow::{Result, ensure};
fn readback() -> Result<(i32, Vec<libc::c_ulong>)> {
 let possible=std::fs::read_to_string("/sys/devices/system/node/possible")?;
 let highest=possible.trim().split(',').flat_map(|part|part.split('-')).filter_map(|id|id.parse::<usize>().ok()).max().unwrap_or(0);
 let bits=libc::c_ulong::BITS as usize;
 let mut mask=vec![0 as libc::c_ulong; highest/bits+1];
 let mut mode=-1 as libc::c_int;
 let result=unsafe{libc::syscall(libc::SYS_get_mempolicy,&mut mode as *mut libc::c_int,mask.as_mut_ptr(),mask.len()*bits,std::ptr::null::<libc::c_void>(),0usize)};
 ensure!(result==0,"get_mempolicy failed: {}",std::io::Error::last_os_error());
 Ok((mode,mask))
}
#[tokio::main(flavor="current_thread")]
async fn main()->Result<()> {
 if std::env::args().nth(1).as_deref()==Some("child") {
  let (mode,mask)=readback()?;
  println!("{}",serde_json::json!({"cpus":numa::NumaOptimizer::current_cpu_affinity(),"mode":mode,"mask":mask}));
  return Ok(());
 }
 let cpus=numa::NumaOptimizer::current_cpu_affinity().unwrap();
 let parent_policy=readback()?;
 let cpu=cpus[0];
 for trial in 1..=3 {
  for (policy,mode,mask) in [(numa::NumaMemoryPolicy::Default,0,0),(numa::NumaMemoryPolicy::Preferred(0),1,1),(numa::NumaMemoryPolicy::Bind(vec![0]),2,1),(numa::NumaMemoryPolicy::Interleave(vec![0]),3,1)] {
   let mut command=tokio::process::Command::new(std::env::current_exe()?);
   command.arg("child");
   numa::NumaOptimizer::configure_command(&mut command,&[cpu],&policy)?;
   let output=command.output().await?;
   ensure!(output.status.success(),"child failed: {}",String::from_utf8_lossy(&output.stderr));
   let data:serde_json::Value=serde_json::from_slice(&output.stdout)?;
   ensure!(data["cpus"]==serde_json::json!([cpu]),"child affinity mismatch");
   ensure!(data["mode"]==mode && data["mask"][0]==mask,"child memory policy mismatch");
   ensure!(numa::NumaOptimizer::current_cpu_affinity()==Some(cpus.clone()),"parent affinity changed");
   ensure!(readback()?==parent_policy,"parent memory policy changed");
   println!("trial={trial} policy={policy:?} child={data} parent_unchanged=true");
  }
 }
 Ok(())
}
