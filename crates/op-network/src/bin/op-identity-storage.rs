//! Root-only identity storage preparation and Incus boot prerequisite.
use anyhow::{Result, ensure};

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
        _ => {
            anyhow::bail!("usage: op-identity-storage mount-all | prepare-leaf <SID> <plan_sha256>")
        }
    }
    Ok(())
}
