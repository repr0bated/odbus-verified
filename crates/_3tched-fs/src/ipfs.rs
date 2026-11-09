//! Private IPFS exchange of content-addressed 3tchedFS views (feature `ipfs`).
//!
//! Every 3tchedFS object and view id is a BLAKE3-256 digest of its canonical
//! bytes, so each maps to a CIDv1 with the `raw` codec and a `blake3`
//! multihash without rehashing. rust-ipfs verifies each block against its CID,
//! and the store verifies it again (content hash and typed validation) before
//! anything is written.
//!
//! Off unless switched on: every node start refuses while SWITCH is absent, and
//! a node exists only for the duration of one command, run by the host inside a
//! throwaway mesh-only network namespace. When on it is private only: a
//! pre-shared swarm key (libp2p pnet) gates every handshake, it dials only the
//! peers it is given, and it runs bitswap/identify/ping without a DHT, autonat,
//! pubsub, mDNS, relay or upnp, so it never joins or announces to the public
//! IPFS network.

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use connexa::prelude::transport::pnet::PreSharedKey;
use ipld_core::cid::{Cid, multihash::Multihash};
use rust_ipfs::builder::DefaultIpfsBuilder;
use rust_ipfs::{Block, Ipfs, Multiaddr};

use crate::store::{ObjectStore, ViewManifest};

const RAW_CODEC: u64 = 0x55;
const BLAKE3_256: u64 = 0x1e;

/// Root-owned marker that switches IPFS exchange on. Remove it to switch off.
pub const SWITCH: &str = "/etc/op-dbus/3tchedfs-ipfs.enabled";

/// Refuse unless the switch is present, root-owned and not writable by others.
pub fn ensure_enabled() -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(SWITCH) else {
        bail!("IPFS exchange is switched off (create {SWITCH} as root to switch it on)");
    };
    ensure!(
        meta.file_type().is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0,
        "{SWITCH} must be a root-owned regular file not writable by group or others"
    );
    Ok(())
}

/// Read a go-ipfs style swarm.key; it must not be accessible by group or others.
pub fn read_swarm_key(path: &Path) -> Result<PreSharedKey> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("swarm key {}", path.display()))?;
    ensure!(
        meta.mode() & 0o077 == 0,
        "swarm key {} must not be accessible by group or others",
        path.display()
    );
    let text = std::fs::read_to_string(path)?;
    PreSharedKey::from_str(text.trim_end())
        .map_err(|error| anyhow::anyhow!("invalid swarm key {}: {error}", path.display()))
}

/// CID of a 3tchedFS object or view hash.
pub fn cid_for(hash: &str) -> Result<Cid> {
    ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid 3tchedFS hash '{hash}'"
    );
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[2 * index..2 * index + 2], 16)?;
    }
    Ok(Cid::new_v1(RAW_CODEC, Multihash::<64>::wrap(BLAKE3_256, &digest)?))
}

/// 3tchedFS hash carried by a raw BLAKE3-256 CID.
pub fn hash_for(cid: &Cid) -> Result<String> {
    let multihash = cid.hash();
    ensure!(
        cid.codec() == RAW_CODEC && multihash.code() == BLAKE3_256 && multihash.size() == 32,
        "CID {cid} is not a raw blake3-256 3tchedFS block"
    );
    Ok(multihash.digest().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Start a private node. Without `repo` the blockstore is in memory and
/// published blocks live only as long as the process.
pub async fn start(
    swarm_key: &Path,
    repo: Option<&Path>,
    listen: &[Multiaddr],
    peers: &[Multiaddr],
) -> Result<Ipfs> {
    ensure_enabled()?;
    let psk = read_swarm_key(swarm_key)?;
    let mut builder = DefaultIpfsBuilder::new()
        .with_identify(Default::default())
        .with_ping(Default::default())
        .with_bitswap()
        .enable_tcp()
        .enable_pnet(psk);
    if let Some(repo) = repo {
        builder = builder.set_path(repo);
    }
    for address in listen {
        builder = builder.add_listening_addr(address.clone());
    }
    let ipfs = builder.start().await.context("start IPFS node")?;
    for peer in peers {
        ipfs.connect(peer.clone())
            .await
            .with_context(|| format!("connect to {peer}"))?;
    }
    Ok(ipfs)
}

/// Add a view and every object it names as pinned raw blocks, served by
/// bitswap to connected private peers. Returns (label, CID) pairs, view last.
pub async fn publish_view(
    ipfs: &Ipfs,
    store: &ObjectStore,
    view_id: &str,
) -> Result<Vec<(String, Cid)>> {
    let view = store.read_view(view_id)?;
    let mut published = Vec::with_capacity(view.objects.len() + 1);
    for (plugin, hash) in &view.objects {
        let cid = put(ipfs, hash, store.object_bytes(hash)?).await?;
        published.push((format!("object {plugin}"), cid));
    }
    let cid = put(ipfs, view_id, store.view_body_bytes(view_id)?).await?;
    published.push(("view".to_owned(), cid));
    Ok(published)
}

/// Fetch a view and its missing objects into the store. Objects are imported
/// first, so a view is only written once everything it names is verified.
pub async fn fetch_view(
    ipfs: &Ipfs,
    store: &ObjectStore,
    view_id: &str,
    timeout: Duration,
) -> Result<ViewManifest> {
    let body = get(ipfs, view_id, timeout).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).context("decode fetched view body")?;
    let objects = value
        .get("objects")
        .and_then(serde_json::Value::as_object)
        .context("fetched view has no object map")?;
    for (plugin, hash) in objects {
        let hash = hash
            .as_str()
            .with_context(|| format!("object hash for {plugin} is not a string"))?;
        if store.read_object(hash).is_ok() {
            continue;
        }
        let bytes = get(ipfs, hash, timeout).await?;
        store
            .import_object_bytes(hash, &bytes)
            .with_context(|| format!("import object {plugin}"))?;
    }
    store.import_view_body(view_id, &body)
}

async fn put(ipfs: &Ipfs, hash: &str, bytes: Vec<u8>) -> Result<Cid> {
    let cid = cid_for(hash)?;
    let block = Block::new(cid, bytes).with_context(|| format!("bytes do not match {hash}"))?;
    ipfs.put_block(&block).await.with_context(|| format!("store block {cid}"))?;
    ipfs.insert_pin(cid).await.with_context(|| format!("pin {cid}"))?;
    Ok(cid)
}

async fn get(ipfs: &Ipfs, hash: &str, timeout: Duration) -> Result<Vec<u8>> {
    let cid = cid_for(hash)?;
    let block = ipfs
        .get_block(cid)
        .timeout(timeout)
        .await
        .with_context(|| format!("fetch {cid}"))?;
    block.verify().with_context(|| format!("block {cid} does not match its CID"))?;
    Ok(block.data().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_cid_round_trip() {
        let hash = blake3::hash(b"3tchedFS").to_hex().to_string();
        let cid = cid_for(&hash).unwrap();
        assert_eq!(cid.codec(), RAW_CODEC);
        assert_eq!(hash_for(&cid).unwrap(), hash);
        assert!(cid.to_string().starts_with("bafkr4"));
    }

    #[test]
    fn rejects_non_hashes_and_foreign_cids() {
        assert!(cid_for("not-a-hash").is_err());
        assert!(cid_for(&"g".repeat(64)).is_err());
        let foreign = Cid::new_v1(0x71, Multihash::<64>::wrap(BLAKE3_256, &[0; 32]).unwrap());
        assert!(hash_for(&foreign).is_err());
    }

    #[test]
    fn block_verifies_against_blake3_cid() {
        let bytes = b"typed object".to_vec();
        let cid = cid_for(&blake3::hash(&bytes).to_hex().to_string()).unwrap();
        assert!(Block::new(cid, bytes).is_ok());
        assert!(Block::new(cid, b"tampered".to_vec()).is_err());
    }
}
