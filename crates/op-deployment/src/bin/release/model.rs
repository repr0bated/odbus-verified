use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs::File, os::fd::AsFd, path::{Path, PathBuf}};
use super::fs;

pub const STATE: &str = "/var/lib/opdbus-release/slots.json";
pub const LOCK: &str = "/var/lib/opdbus-release/lock";
pub const READY: &str = "/run/opdbus-release/ready";
pub const APPROVAL: &str = "/var/lib/opdbus-release/first-activation.json";
pub const GOLDEN: &str = "/opt/op-dbus/golden";
pub const SEND: &str = "/opt/op-dbus/release-send";
pub const FSID: &str = "d53eeac0-949b-499c-b051-9fecf8678f59";
pub const OPT_ID: u64 = 271;
pub const GRAPH: &str = "usr/share/op-dbus/graphs/release.json";
pub const SHIPPED: &str = "/usr/share/op-dbus/plugin-blobs";
pub const PERSISTED: &str = "/var/lib/opdbus/plugin-blobs";
pub const SHM: &str = "/dev/shm/opdbus/plugin-blobs";
pub const SERVICES: &str = "/etc/sv";
pub const ENABLED: &str = "/etc/runit/runsvdir/default";
pub const SERVICE_TARGET: &str = "/usr/local/libexec/op-dbus/sv";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Slot { A, B }
impl Slot {
    pub fn name(self) -> &'static str { match self { Self::A => "a", Self::B => "b" } }
    pub fn root(self) -> &'static str { match self { Self::A => "@release-a", Self::B => "@release-b" } }
    pub fn other(self) -> Self { match self { Self::A => Self::B, Self::B => Self::A } }
    pub fn parse(s: &str) -> Result<Self> { match s { "a" => Ok(Self::A), "b" => Ok(Self::B), _ => anyhow::bail!("slot must be a or b") } }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Parent { pub fsid: String, pub subvolume: u64, pub device: u64, pub mount_id: u64 }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target { pub path: PathBuf, pub source: PathBuf, pub noexec: bool, pub parent: Option<Parent> }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub name: String, pub id: u64, pub native_uuid: String, pub received_uuid: String, pub source_id: u64,
    pub source_uuid: String, pub graph_sha256: String, pub tree_sha256: String,
    pub catalog_sha256: String, pub targets: Vec<Target>, pub devices: BTreeSet<u64>,
}
impl Release {
    pub fn tag(&self, slot: Slot, index: usize) -> String { format!("opdbus-release:{}:{}:{}", slot.name(), self.name, index) }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Wanted {
    pub pid: u32, pub start: [u8; 12], pub paused: bool, pub want: u8,
    pub state: u8, pub down: bool, pub linked: bool, pub new_service: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan { pub changed: BTreeSet<String>, pub order: Vec<String>, pub wanted: BTreeMap<String, Wanted> }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preserved {
    pub path: PathBuf, pub backup: PathBuf, pub fingerprint: Option<String>,
    pub replacement: String, pub replacement_sha256: Option<String>,
    pub original: Vec<serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RootRemoval {
    pub id: u64, pub native_uuid: String, pub received_uuid: String,
    pub parent: String, pub name: String, pub cleaned: bool, pub rejected: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub name: String, pub inode: u64, pub uid: u32, pub gid: u32,
    pub mode: u32, pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRemoval {
    pub path: PathBuf, pub inode: u64, pub device: u64, pub boot_id: String,
    pub btrfs: Option<(String, u64)>, pub entries: Vec<CatalogEntry>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogPhase { Creating, Publishing, Cleaning }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPublication {
    pub destination: PathBuf, pub stage: PathBuf,
    pub parent: CatalogRemoval, pub source: CatalogRemoval,
    pub previous: Option<CatalogRemoval>, pub created: Option<CatalogRemoval>,
    pub phase: CatalogPhase, pub published: bool, pub cleanup: Option<CatalogRemoval>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookChange { pub service: String, pub member: String, pub before: bool, pub after: bool }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoggerChange {
    pub service: String, pub before: bool, pub after: bool, pub created: bool,
    pub identity: Option<(String, u64, u64)>, pub run_before: bool, pub cleaned: bool, pub touched: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub operation: String, pub step: String, pub from: Option<Slot>, pub to: Option<Slot>,
    pub candidate: Option<Release>, pub source_name: Option<String>,
    pub source_uuid: Option<String>, pub plan: Plan, pub preserved: Vec<Preserved>,
    pub moved: BTreeSet<usize>, pub stacked: BTreeSet<usize>, pub allowed: BTreeSet<String>,
    pub error: Option<String>, pub timings_ms: BTreeMap<String, u128>, pub stream_bytes: u64,
    pub incremental: bool, pub evicted: Option<Release>,
    pub removals: Vec<RootRemoval>, pub aborting: bool,
    pub catalog_removals: Vec<CatalogRemoval>,
    #[serde(default)]
    pub catalog_publication: Option<CatalogPublication>,
    pub hooks: Vec<HookChange>,
    pub loggers: Vec<LoggerChange>,
}
impl Journal {
    pub fn new(operation: &str, from: Option<Slot>, to: Option<Slot>) -> Self {
        Self { operation: operation.into(), step: "planned".into(), from, to, candidate: None,
            source_name: None, source_uuid: None, plan: Plan::default(), preserved: Vec::new(),
            moved: BTreeSet::new(), stacked: BTreeSet::new(), allowed: BTreeSet::new(),
            error: None, timings_ms: BTreeMap::new(), stream_bytes: 0, incremental: false,
            evicted: None, removals: Vec::new(), aborting: false, catalog_removals: Vec::new(),
            catalog_publication: None, hooks: Vec::new(), loggers: Vec::new() }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u32, pub active: Option<Slot>, pub previous: Option<Slot>,
    pub slots: BTreeMap<Slot, Option<Release>>, pub journal: Option<Journal>,
    pub last_timings_ms: BTreeMap<String, u128>, pub last_stream_bytes: u64,
    pub first_activation_originals: Vec<Preserved>,
    pub golden_original: Option<serde_json::Value>,
}
impl Default for State {
    fn default() -> Self { Self { version: 1, active: None, previous: None,
        slots: [(Slot::A, None), (Slot::B, None)].into(), journal: None,
        last_timings_ms: BTreeMap::new(), last_stream_bytes: 0, first_activation_originals: Vec::new(), golden_original: None } }
}
impl State {
    pub fn load() -> Result<Self> {
        let file = fs::secure_absolute(Path::new(STATE), libc::O_RDONLY);
        let mut file = match file {
            Ok(f) => f,
            Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let state: Self = serde_json::from_slice(&fs::read_bounded(&mut file, 64 * 1024 * 1024)?)?;
        ensure!(state.version == 1, "unsupported release state; explicit bootstrap migration required");
        for slot in [state.active, state.previous].into_iter().flatten() { state.release(slot)?; }
        ensure!(state.active.is_none() || state.active != state.previous, "active and previous alias");
        Ok(state)
    }
    pub fn release(&self, slot: Slot) -> Result<&Release> { self.slots.get(&slot).and_then(Option::as_ref).context("slot has no verified release") }
    pub fn save(&self) -> Result<()> { fs::atomic_json(Path::new(STATE), self) }
    pub fn intent(&mut self, step: impl Into<String>) -> Result<()> {
        self.journal.as_mut().context("mutation without transaction journal")?.step = step.into(); self.save()
    }
    pub fn root(&self, slot: Slot) -> Result<File> {
        let release = self.release(slot)?;
        let mount = fs::detached_btrfs(FSID, release.id, true)?;
        let root = fs::directory(mount.as_fd())?;
        super::transfer::verify_root(&root, release)?;
        Ok(root)
    }
}
