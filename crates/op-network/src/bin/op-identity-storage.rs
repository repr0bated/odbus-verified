//! Root-only identity storage preparation and Incus boot prerequisite.
use anyhow::{ensure, Result};

#[path = "op-identity-storage/lungs_attach.rs"]
mod lungs_attach;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    // SAFETY: geteuid has no pointer arguments or side effects.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "identity storage boot preparation requires root"
    );
    match args.as_slice() {
        [_, command] if command == "mount-all" => {
            let count = op_network::seeded_boot::mount_all()?;
            println!("verified {count} seeded identity mounts; no identity activation performed");
        }
        [_, command, session, digest] if command == "prepare-leaf" => {
            let session = session
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("session is not UTF-8"))?;
            let digest = digest
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("digest is not UTF-8"))?;
            let receipt = op_network::seeded_prepare::prepare_leaf(session, digest)?;
            println!("{}", serde_json::to_string(&receipt)?);
        }
        [_, command, name, pid, start, source] if command == "attach-existing-lungs" => {
            lungs_attach::attach(
                name.to_str()
                    .ok_or_else(|| anyhow::anyhow!("name is not UTF-8"))?,
                pid.to_str()
                    .ok_or_else(|| anyhow::anyhow!("PID is not UTF-8"))?
                    .parse()?,
                start
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("start time is not UTF-8"))?
                    .parse()?,
                std::path::Path::new(source),
            )?;
            println!("attached scoped host lungs to verified identity mount namespace; no identity activation performed");
        }
        _ => {
            anyhow::bail!("usage: op-identity-storage mount-all | prepare-leaf <SID> <plan_sha256> | attach-existing-lungs <NAME> <PID> <STARTTIME> <HOST_FUSE_SOURCE>")
        }
    }
    Ok(())
}
