//! Private fixture probe. Never point this at a live identity.
use std::path::Path;

use anyhow::{Context, Result};
use op_network::seeded_root::{
    ensure_seeded_root_mounted, verify_seeded_root_mounted, SeededRootDescriptor,
};
use uuid::Uuid;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 9,
        "usage: ensure_seeded_root MODE SEED SHA SEED_FSID LEAF SPROUT_FSID MOUNT POOL"
    );
    let desc = SeededRootDescriptor {
        seed_image: Path::new(&args[2]),
        seed_sha256: &args[3],
        seed_fsid: Uuid::parse_str(&args[4]).context("seed FSID")?,
        leaf_image: Path::new(&args[5]),
        sprout_fsid: Uuid::parse_str(&args[6]).context("sprout FSID")?,
        mount_point: Path::new(&args[7]),
        pool_subvolume: &args[8],
    };
    let guard = match args[1].as_str() {
        "ensure" => ensure_seeded_root_mounted(&desc)?,
        "verify" => verify_seeded_root_mounted(&desc)?,
        _ => anyhow::bail!("mode must be ensure or verify"),
    };
    println!("mounted_now={}", guard.mounted_now);
    println!("seed_loop={}", guard.seed_loop.display());
    println!("leaf_loop={}", guard.leaf_loop.display());
    println!("pool_path={}", guard.pool_path.display());
    Ok(())
}
