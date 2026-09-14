use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_SUBVOLUMES: usize = 5;

macro_rules! path_input {
    ($name:ident) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub path: String,
        }
    };
}

path_input!(CreateSubvolumeInput);
path_input!(DeleteSubvolumeInput);
path_input!(ListSubvolumesInput);
path_input!(ScrubStartInput);
path_input!(ScrubStatusInput);
path_input!(BalanceStartInput);
path_input!(BalanceStatusInput);
path_input!(FilesystemDfInput);
path_input!(FilesystemUsageInput);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotInput {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceAddInput {
    pub device: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceRemoveInput {
    pub device: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubvolumeRecord {
    pub id: u64,
    pub name: String,
    pub parent_id: u64,
    pub generation: u64,
    pub uuid: String,
    pub parent_uuid: Option<String>,
    pub received_uuid: Option<String>,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubvolumeMutationOutput {
    pub operation: String,
    pub path: String,
    pub subvolume: SubvolumeRecord,
    pub total_subvolumes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteSubvolumeOutput {
    pub path: String,
    pub removed_subvolume_id: u64,
    pub remaining_subvolumes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSubvolumesOutput {
    pub filesystem_uuid: String,
    pub subvolumes: Vec<SubvolumeRecord>,
    pub total: u64,
    pub maximum_supported: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrubProgressRecord {
    pub device_id: u64,
    pub data_bytes_scrubbed: u64,
    pub tree_bytes_scrubbed: u64,
    pub read_errors: u64,
    pub checksum_errors: u64,
    pub verify_errors: u64,
    pub uncorrectable_errors: u64,
    pub corrected_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrubState {
    Idle,
    Running,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrubOutput {
    pub filesystem_uuid: String,
    pub state: ScrubState,
    pub devices: Vec<ScrubProgressRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BalanceState {
    Idle,
    Running,
    Paused,
    PauseRequested,
    CancelRequested,
    Completed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BalanceProgressRecord {
    pub expected_chunks: u64,
    pub considered_chunks: u64,
    pub completed_chunks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BalanceOutput {
    pub filesystem_uuid: String,
    pub state: BalanceState,
    pub progress: Option<BalanceProgressRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpaceRecord {
    pub flags: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesystemDfOutput {
    pub filesystem_uuid: String,
    pub spaces: Vec<SpaceRecord>,
    pub allocated_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceRecord {
    pub id: u64,
    pub uuid: String,
    pub path: String,
    pub total_bytes: u64,
    pub bytes_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesystemUsageOutput {
    pub filesystem_uuid: String,
    pub generation: u64,
    pub node_size: u32,
    pub sector_size: u32,
    pub devices: Vec<DeviceRecord>,
    pub spaces: Vec<SpaceRecord>,
    pub device_total_bytes: u64,
    pub allocated_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceMutationOutput {
    pub operation: String,
    pub filesystem_uuid: String,
    pub device: String,
    pub devices: Vec<DeviceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.btrfs.schema@v2"))]
#[schemars(extend("x-oscal-category" = "storage"))]
pub struct BtrfsState {
    pub maximum_subvolumes: u64,
    pub deployment_transport: String,
}

impl Default for BtrfsState {
    fn default() -> Self {
        Self {
            maximum_subvolumes: MAX_SUBVOLUMES as u64,
            deployment_transport: "btrfs_send_receive".into(),
        }
    }
}

#[derive(Debug, Clone)]
struct FsIdentity {
    uuid: String,
    generation: u64,
    node_size: u32,
    sector_size: u32,
}

trait BtrfsBackend: Send + Sync {
    fn filesystem(&self, path: &Path) -> Result<FsIdentity>;
    fn list_subvolumes(&self, path: &Path) -> Result<Vec<SubvolumeRecord>>;
    fn subvolume_info(&self, path: &Path) -> Result<SubvolumeRecord>;
    fn subvolume_lookup(&self, path: &Path) -> Result<Option<SubvolumeRecord>>;
    fn create_subvolume(&self, path: &Path) -> Result<()>;
    fn delete_subvolume(&self, path: &Path) -> Result<()>;
    fn snapshot(&self, source: &Path, destination: &Path, readonly: bool) -> Result<()>;
    fn scrub_start(&self, path: &Path) -> Result<Vec<ScrubProgressRecord>>;
    fn scrub_status(&self, path: &Path) -> Result<Vec<ScrubProgressRecord>>;
    fn balance_start(&self, path: &Path) -> Result<BalanceProgressRecord>;
    fn balance_status(&self, path: &Path) -> Result<Option<(BalanceState, BalanceProgressRecord)>>;
    fn spaces(&self, path: &Path) -> Result<Vec<SpaceRecord>>;
    fn devices(&self, path: &Path) -> Result<Vec<DeviceRecord>>;
    fn is_block_device(&self, path: &Path) -> Result<bool>;
    fn matching_device_id(&self, device: &Path, devices: &[DeviceRecord]) -> Result<Option<u64>>;
    fn device_add(&self, path: &Path, device: &Path) -> Result<()>;
    fn device_remove(&self, path: &Path, device_id: u64) -> Result<()>;
}

struct NativeBtrfsBackend;

fn open(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("open {}", path.display()))
}
fn c_leaf(path: &Path) -> Result<(File, CString)> {
    let parent = path.parent().context("path has no parent")?;
    let leaf = path.file_name().context("path has no leaf name")?;
    let leaf = CString::new(leaf.as_encoded_bytes()).context("path contains NUL")?;
    Ok((open(parent)?, leaf))
}
fn optional_uuid(uuid: uuid::Uuid) -> Option<String> {
    (!uuid.is_nil()).then(|| uuid.to_string())
}

fn subvolume_record(info: btrfs_uapi::subvolume::SubvolumeInfo) -> SubvolumeRecord {
    SubvolumeRecord {
        id: info.id,
        name: info.name,
        parent_id: info.parent_id,
        generation: info.generation,
        uuid: info.uuid.to_string(),
        parent_uuid: optional_uuid(info.parent_uuid),
        received_uuid: optional_uuid(info.received_uuid),
        readonly: info
            .flags
            .contains(btrfs_uapi::subvolume::SubvolumeFlags::RDONLY),
    }
}

impl NativeBtrfsBackend {
    fn fs_file(path: &Path) -> Result<(File, btrfs_uapi::filesystem::FilesystemInfo)> {
        let file = open(path)?;
        let info = btrfs_uapi::filesystem::filesystem_info(file.as_fd())
            .context("query btrfs filesystem")?;
        Ok((file, info))
    }
}

impl BtrfsBackend for NativeBtrfsBackend {
    fn filesystem(&self, path: &Path) -> Result<FsIdentity> {
        let (_, i) = Self::fs_file(path)?;
        Ok(FsIdentity {
            uuid: i.uuid.to_string(),
            generation: i.generation,
            node_size: i.nodesize,
            sector_size: i.sectorsize,
        })
    }
    fn list_subvolumes(&self, path: &Path) -> Result<Vec<SubvolumeRecord>> {
        let file = open(path)?;
        btrfs_uapi::subvolume::subvolume_list(file.as_fd())
            .context("list btrfs subvolumes")?
            .into_iter()
            .map(|i| {
                Ok(SubvolumeRecord {
                    id: i.root_id,
                    name: i.name,
                    parent_id: i.parent_id,
                    generation: i.generation,
                    uuid: i.uuid.to_string(),
                    parent_uuid: optional_uuid(i.parent_uuid),
                    received_uuid: optional_uuid(i.received_uuid),
                    readonly: i
                        .flags
                        .contains(btrfs_uapi::subvolume::SubvolumeFlags::RDONLY),
                })
            })
            .collect()
    }
    fn subvolume_info(&self, path: &Path) -> Result<SubvolumeRecord> {
        Ok(subvolume_record(
            btrfs_uapi::subvolume::subvolume_info(open(path)?.as_fd())
                .context("query btrfs subvolume")?,
        ))
    }
    fn subvolume_lookup(&self, path: &Path) -> Result<Option<SubvolumeRecord>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("open {}", path.display()));
            }
        };
        match btrfs_uapi::subvolume::subvolume_info(file.as_fd()) {
            Ok(info) => Ok(Some(subvolume_record(info))),
            Err(error) if error as i32 == nix::libc::ENOENT => Ok(None),
            Err(error) => Err(anyhow::Error::new(error).context("query btrfs subvolume")),
        }
    }
    fn create_subvolume(&self, path: &Path) -> Result<()> {
        let (parent, leaf) = c_leaf(path)?;
        btrfs_uapi::subvolume::subvolume_create(parent.as_fd(), &leaf, &[])
            .context("create btrfs subvolume")
    }
    fn delete_subvolume(&self, path: &Path) -> Result<()> {
        let info = self.subvolume_info(path)?;
        let (parent, leaf) = c_leaf(path)?;
        btrfs_uapi::subvolume::subvolume_delete(parent.as_fd(), &leaf)
            .context("delete btrfs subvolume")?;
        btrfs_uapi::filesystem::sync(parent.as_fd()).context(
            "subvolume delete ioctl succeeded but filesystem sync failed; deletion effect is indeterminate",
        )?;
        btrfs_uapi::subvolume::subvol_sync_wait_one(parent.as_fd(), info.id)
            .context("subvolume delete and sync succeeded but cleanup wait failed; target may already be absent")
    }
    fn snapshot(&self, source: &Path, destination: &Path, readonly: bool) -> Result<()> {
        let source = open(source)?;
        let (parent, leaf) = c_leaf(destination)?;
        btrfs_uapi::subvolume::snapshot_create(parent.as_fd(), source.as_fd(), &leaf, readonly, &[])
            .context("create btrfs snapshot")
    }
    fn scrub_start(&self, path: &Path) -> Result<Vec<ScrubProgressRecord>> {
        let (file, fs) = Self::fs_file(path)?;
        let devices =
            btrfs_uapi::device::device_info_all(file.as_fd(), &fs).context("list scrub devices")?;
        let mut completed = Vec::with_capacity(devices.len());
        for d in devices {
            let progress =
                btrfs_uapi::scrub::scrub_start(file.as_fd(), d.devid, false).map_err(|error| {
                    let scope = if completed.is_empty() {
                        "scrub start failed; kernel effect is indeterminate"
                    } else {
                        "scrub partially completed on earlier devices"
                    };
                    anyhow::Error::new(error).context(scope)
                })?;
            completed.push(scrub_record(d.devid, progress));
        }
        Ok(completed)
    }
    fn scrub_status(&self, path: &Path) -> Result<Vec<ScrubProgressRecord>> {
        let (file, fs) = Self::fs_file(path)?;
        let devices =
            btrfs_uapi::device::device_info_all(file.as_fd(), &fs).context("list scrub devices")?;
        devices
            .into_iter()
            .filter_map(
                |d| match btrfs_uapi::scrub::scrub_progress(file.as_fd(), d.devid) {
                    Ok(Some(p)) => Some(Ok(scrub_record(d.devid, p))),
                    Ok(None) => None,
                    Err(e) => Some(Err(
                        anyhow::Error::new(e).context("query btrfs scrub progress")
                    )),
                },
            )
            .collect()
    }
    fn balance_start(&self, path: &Path) -> Result<BalanceProgressRecord> {
        use btrfs_uapi::balance::BalanceFlags;
        let file = open(path)?;
        let p = btrfs_uapi::balance::balance(
            file.as_fd(),
            BalanceFlags::DATA | BalanceFlags::METADATA | BalanceFlags::SYSTEM,
            None,
            None,
            None,
        )
        .context("run btrfs balance")?;
        Ok(balance_record(p))
    }
    fn balance_status(&self, path: &Path) -> Result<Option<(BalanceState, BalanceProgressRecord)>> {
        use btrfs_uapi::balance::BalanceState as S;
        let file = open(path)?;
        match btrfs_uapi::balance::balance_progress(file.as_fd()) {
            Ok((s, p)) => {
                let state = if s.contains(S::CANCEL_REQ) {
                    BalanceState::CancelRequested
                } else if s.contains(S::PAUSE_REQ) {
                    BalanceState::PauseRequested
                } else if s.contains(S::RUNNING) {
                    BalanceState::Running
                } else {
                    BalanceState::Paused
                };
                Ok(Some((state, balance_record(p))))
            }
            Err(e) if e as i32 == nix::libc::ENOTCONN || e as i32 == nix::libc::ENOENT => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context("query btrfs balance progress")),
        }
    }
    fn spaces(&self, path: &Path) -> Result<Vec<SpaceRecord>> {
        let file = open(path)?;
        Ok(btrfs_uapi::space::space_info(file.as_fd())
            .context("query btrfs space info")?
            .into_iter()
            .map(|s| SpaceRecord {
                flags: s.flags.bits(),
                total_bytes: s.total_bytes,
                used_bytes: s.used_bytes,
            })
            .collect())
    }
    fn devices(&self, path: &Path) -> Result<Vec<DeviceRecord>> {
        let (file, fs) = Self::fs_file(path)?;
        Ok(btrfs_uapi::device::device_info_all(file.as_fd(), &fs)
            .context("query btrfs devices")?
            .into_iter()
            .map(|d| DeviceRecord {
                id: d.devid,
                uuid: d.uuid.to_string(),
                path: d.path,
                total_bytes: d.total_bytes,
                bytes_used: d.bytes_used,
            })
            .collect())
    }
    fn is_block_device(&self, path: &Path) -> Result<bool> {
        Ok(std::fs::metadata(path)?.file_type().is_block_device())
    }
    fn matching_device_id(&self, device: &Path, devices: &[DeviceRecord]) -> Result<Option<u64>> {
        let requested = std::fs::metadata(device)
            .with_context(|| format!("inspect block device {}", device.display()))?;
        if !requested.file_type().is_block_device() {
            return Ok(None);
        }
        let requested_rdev = requested.rdev();
        for member in devices {
            let metadata = std::fs::metadata(&member.path)
                .with_context(|| format!("inspect provider device {}", member.path))?;
            if metadata.file_type().is_block_device() && metadata.rdev() == requested_rdev {
                return Ok(Some(member.id));
            }
        }
        Ok(None)
    }
    fn device_add(&self, path: &Path, device: &Path) -> Result<()> {
        let file = open(path)?;
        let device = CString::new(device.as_os_str().as_encoded_bytes())?;
        btrfs_uapi::device::device_add(file.as_fd(), &device).context("add btrfs device")
    }
    fn device_remove(&self, path: &Path, device_id: u64) -> Result<()> {
        let file = open(path)?;
        btrfs_uapi::device::device_remove(
            file.as_fd(),
            &btrfs_uapi::device::DeviceSpec::Id(device_id),
        )
        .context("remove btrfs device")
    }
}

fn scrub_record(id: u64, p: btrfs_uapi::scrub::ScrubProgress) -> ScrubProgressRecord {
    ScrubProgressRecord {
        device_id: id,
        data_bytes_scrubbed: p.data_bytes_scrubbed,
        tree_bytes_scrubbed: p.tree_bytes_scrubbed,
        read_errors: p.read_errors,
        checksum_errors: p.csum_errors,
        verify_errors: p.verify_errors,
        uncorrectable_errors: p.uncorrectable_errors,
        corrected_errors: p.corrected_errors,
    }
}
fn balance_record(p: btrfs_uapi::balance::BalanceProgress) -> BalanceProgressRecord {
    BalanceProgressRecord {
        expected_chunks: p.expected,
        considered_chunks: p.considered,
        completed_chunks: p.completed,
    }
}

fn checked_path(raw: &str, allow_root: bool) -> Result<PathBuf> {
    if raw.is_empty() {
        bail!("path must not be empty");
    }
    if raw.as_bytes().contains(&0) {
        bail!("path contains NUL");
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        bail!("path must be absolute");
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        bail!("path must be lexically normalized");
    }
    if !allow_root && path == Path::new("/") {
        bail!("filesystem root is not a valid mutation target");
    }
    if !allow_root && path.file_name().is_none() {
        bail!("path must name a child entry");
    }
    Ok(path)
}

fn mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn add_output(
    operation: &str,
    path: &Path,
    info: SubvolumeRecord,
    total: usize,
) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(SubvolumeMutationOutput {
        operation: operation.into(),
        path: path.display().to_string(),
        subvolume: info,
        total_subvolumes: total as u64,
    })?)
}

fn dispatch_with_backend(
    backend: &dyn BtrfsBackend,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "create_subvolume" => {
            let i: CreateSubvolumeInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, false)?;
            let parent = path.parent().unwrap();
            let _guard = mutation_lock()
                .lock()
                .map_err(|_| anyhow::anyhow!("btrfs mutation lock poisoned"))?;
            backend.filesystem(parent)?;
            let before = backend.list_subvolumes(parent)?;
            if before.len() >= MAX_SUBVOLUMES {
                bail!("subvolume limit {MAX_SUBVOLUMES} reached");
            }
            backend.create_subvolume(&path)?;
            let info = backend.subvolume_info(&path)?;
            let after = backend.list_subvolumes(parent)?;
            if after.len() > MAX_SUBVOLUMES {
                bail!("subvolume was created but an external race exceeded limit {MAX_SUBVOLUMES}");
            }
            if !after.iter().any(|s| s.id == info.id) {
                bail!(
                    "subvolume create completed but readback did not contain id {}",
                    info.id
                );
            }
            add_output("created", &path, info, after.len())
        }
        "delete_subvolume" => {
            let i: DeleteSubvolumeInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, false)?;
            let _guard = mutation_lock()
                .lock()
                .map_err(|_| anyhow::anyhow!("btrfs mutation lock poisoned"))?;
            let info = backend.subvolume_info(&path)?;
            backend.delete_subvolume(&path)?;
            if backend.subvolume_lookup(&path).with_context(|| {
                "subvolume deletion returned successfully but absence could not be verified; deletion effect is indeterminate"
            })?.is_some() {
                bail!("subvolume delete returned but target remains present");
            }
            let remaining = backend
                .list_subvolumes(path.parent().unwrap())
                .context("subvolume was deleted but remaining-subvolume readback failed")?
                .len();
            Ok(serde_json::to_value(DeleteSubvolumeOutput {
                path: i.path,
                removed_subvolume_id: info.id,
                remaining_subvolumes: remaining as u64,
            })?)
        }
        "snapshot" => {
            let i: SnapshotInput = serde_json::from_value(args.clone())?;
            let src = checked_path(&i.source, true)?;
            let dst = checked_path(&i.destination, false)?;
            let parent = dst.parent().unwrap();
            if src == dst {
                bail!("source and destination must differ");
            }
            let _guard = mutation_lock()
                .lock()
                .map_err(|_| anyhow::anyhow!("btrfs mutation lock poisoned"))?;
            let src_fs = backend.filesystem(&src)?;
            let dst_fs = backend.filesystem(parent)?;
            if src_fs.uuid != dst_fs.uuid {
                bail!("snapshot source and destination must be on one filesystem");
            }
            let source = backend.subvolume_info(&src)?;
            let before = backend.list_subvolumes(parent)?;
            if before.len() >= MAX_SUBVOLUMES {
                bail!("subvolume limit {MAX_SUBVOLUMES} reached");
            }
            backend.snapshot(&src, &dst, i.readonly)?;
            let info = backend.subvolume_info(&dst).context(
                "snapshot creation returned successfully but postcondition readback failed; snapshot state is indeterminate",
            )?;
            if info.parent_uuid.as_deref() != Some(source.uuid.as_str())
                || info.readonly != i.readonly
            {
                bail!("snapshot completed but readback does not match requested source/readonly state");
            }
            let after = backend
                .list_subvolumes(parent)
                .context("snapshot was created but filesystem subvolume-count readback failed")?;
            if after.len() > MAX_SUBVOLUMES {
                bail!("snapshot was created but an external race exceeded limit {MAX_SUBVOLUMES}");
            }
            add_output("snapshotted", &dst, info, after.len())
        }
        "list_subvolumes" => {
            let i: ListSubvolumesInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let subvolumes = backend.list_subvolumes(&path)?;
            let total = subvolumes.len() as u64;
            Ok(serde_json::to_value(ListSubvolumesOutput {
                filesystem_uuid: fs.uuid,
                subvolumes,
                total,
                maximum_supported: MAX_SUBVOLUMES as u64,
            })?)
        }
        "scrub_start" => {
            let i: ScrubStartInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let devices = backend.scrub_start(&path)?;
            Ok(serde_json::to_value(ScrubOutput {
                filesystem_uuid: fs.uuid,
                state: ScrubState::Completed,
                devices,
            })?)
        }
        "scrub_status" => {
            let i: ScrubStatusInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let devices = backend.scrub_status(&path)?;
            let state = if devices.is_empty() {
                ScrubState::Idle
            } else {
                ScrubState::Running
            };
            Ok(serde_json::to_value(ScrubOutput {
                filesystem_uuid: fs.uuid,
                state,
                devices,
            })?)
        }
        "balance_start" => {
            let i: BalanceStartInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let progress = backend.balance_start(&path)?;
            Ok(serde_json::to_value(BalanceOutput {
                filesystem_uuid: fs.uuid,
                state: BalanceState::Completed,
                progress: Some(progress),
            })?)
        }
        "balance_status" => {
            let i: BalanceStatusInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let status = backend.balance_status(&path)?;
            let (state, progress) = match status {
                Some((state, progress)) => (state, Some(progress)),
                None => (BalanceState::Idle, None),
            };
            Ok(serde_json::to_value(BalanceOutput {
                filesystem_uuid: fs.uuid,
                state,
                progress,
            })?)
        }
        "filesystem_df" => {
            let i: FilesystemDfInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let spaces = backend.spaces(&path)?;
            let allocated_bytes =
                checked_sum("allocated bytes", spaces.iter().map(|s| s.total_bytes))?;
            let used_bytes = checked_sum("used bytes", spaces.iter().map(|s| s.used_bytes))?;
            Ok(serde_json::to_value(FilesystemDfOutput {
                filesystem_uuid: fs.uuid,
                spaces,
                allocated_bytes,
                used_bytes,
            })?)
        }
        "filesystem_usage" => {
            let i: FilesystemUsageInput = serde_json::from_value(args.clone())?;
            let path = checked_path(&i.path, true)?;
            let fs = backend.filesystem(&path)?;
            let devices = backend.devices(&path)?;
            let spaces = backend.spaces(&path)?;
            let device_total_bytes =
                checked_sum("device total bytes", devices.iter().map(|d| d.total_bytes))?;
            let allocated_bytes =
                checked_sum("allocated bytes", spaces.iter().map(|s| s.total_bytes))?;
            let used_bytes = checked_sum("used bytes", spaces.iter().map(|s| s.used_bytes))?;
            Ok(serde_json::to_value(FilesystemUsageOutput {
                filesystem_uuid: fs.uuid,
                generation: fs.generation,
                node_size: fs.node_size,
                sector_size: fs.sector_size,
                devices,
                spaces,
                device_total_bytes,
                allocated_bytes,
                used_bytes,
            })?)
        }
        "device_add" => device_mutation(backend, args, true),
        "device_remove" => device_mutation(backend, args, false),
        other => bail!("unknown btrfs method: {other}"),
    }
}

fn checked_sum(label: &str, mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .with_context(|| format!("{label} overflowed u64"))
    })
}

fn device_mutation(
    backend: &dyn BtrfsBackend,
    args: &serde_json::Value,
    add: bool,
) -> Result<serde_json::Value> {
    let (device_raw, path_raw) = if add {
        let i: DeviceAddInput = serde_json::from_value(args.clone())?;
        (i.device, i.path)
    } else {
        let i: DeviceRemoveInput = serde_json::from_value(args.clone())?;
        (i.device, i.path)
    };
    let device = checked_path(&device_raw, true)?;
    let path = checked_path(&path_raw, true)?;
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("btrfs mutation lock poisoned"))?;
    let fs = backend.filesystem(&path)?;
    let before = backend.devices(&path)?;
    let matched = backend.matching_device_id(&device, &before)?;
    if add {
        if !backend.is_block_device(&device)? {
            bail!("device must be an existing block device");
        }
        if matched.is_some() {
            bail!("device is already a member of the filesystem");
        }
        backend.device_add(&path, &device)?;
    } else {
        let id = matched.context("device is not a member of the filesystem")?;
        if before.len() <= 1 {
            bail!("cannot remove the last filesystem device");
        }
        backend.device_remove(&path, id)?;
    }
    let devices = backend.devices(&path)?;
    let present = backend.matching_device_id(&device, &devices)?.is_some();
    if present != add {
        bail!("device mutation completed but membership readback does not match request");
    }
    Ok(serde_json::to_value(DeviceMutationOutput {
        operation: if add { "added" } else { "removed" }.into(),
        filesystem_uuid: fs.uuid,
        device: device_raw,
        devices,
    })?)
}

pub async fn dispatch_btrfs_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let method = method.to_string();
    let args = args.clone();
    tokio::task::spawn_blocking(move || dispatch_with_backend(&NativeBtrfsBackend, &method, &args))
        .await
        .context("btrfs native operation task failed")?
}

pub struct BtrfsPlugin;
impl Default for BtrfsPlugin {
    fn default() -> Self {
        Self
    }
}
impl BtrfsPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for BtrfsPlugin {
    fn name(&self) -> &str {
        "btrfs"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(btrfs_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(BtrfsState::default())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn btrfs_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(BtrfsState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "btrfs",
        "2.0.0",
        "Native Btrfs filesystem operations and send/receive deployment topology",
        &root,
    );
    schema.category = "infrastructure".into();
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:expr, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                    $input,
                    $output,
                >($name, $effect, false, $cap, $subid),
            );
            schema.capabilities.insert(
                $cap.into(),
                op_state_store::CapabilityDecl {
                    id: $cap.into(),
                    description: concat!("Grants: ", $name, ".").into(),
                },
            );
        };
    }
    method!(
        "create_subvolume",
        CreateSubvolumeInput,
        SubvolumeMutationOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.subvolume.create@v1",
        "mut.storage.btrfs.subvolume.create@v2"
    );
    method!(
        "delete_subvolume",
        DeleteSubvolumeInput,
        DeleteSubvolumeOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.subvolume.delete@v1",
        "mut.storage.btrfs.subvolume.delete@v2"
    );
    method!(
        "snapshot",
        SnapshotInput,
        SubvolumeMutationOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.snapshot.create@v1",
        "mut.storage.btrfs.snapshot.create@v2"
    );
    method!(
        "list_subvolumes",
        ListSubvolumesInput,
        ListSubvolumesOutput,
        SideEffect::Read,
        "cap.storage.btrfs.subvolume.list@v1",
        "obs.storage.btrfs.subvolume.list@v2"
    );
    method!(
        "scrub_start",
        ScrubStartInput,
        ScrubOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.scrub.start@v1",
        "mut.storage.btrfs.scrub.start@v2"
    );
    method!(
        "scrub_status",
        ScrubStatusInput,
        ScrubOutput,
        SideEffect::Read,
        "cap.storage.btrfs.scrub.status@v1",
        "obs.storage.btrfs.scrub.status@v2"
    );
    method!(
        "balance_start",
        BalanceStartInput,
        BalanceOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.balance.start@v1",
        "mut.storage.btrfs.balance.start@v2"
    );
    method!(
        "balance_status",
        BalanceStatusInput,
        BalanceOutput,
        SideEffect::Read,
        "cap.storage.btrfs.balance.status@v1",
        "obs.storage.btrfs.balance.status@v2"
    );
    method!(
        "filesystem_df",
        FilesystemDfInput,
        FilesystemDfOutput,
        SideEffect::Read,
        "cap.storage.btrfs.filesystem.df@v1",
        "obs.storage.btrfs.filesystem.df@v2"
    );
    method!(
        "filesystem_usage",
        FilesystemUsageInput,
        FilesystemUsageOutput,
        SideEffect::Read,
        "cap.storage.btrfs.filesystem.usage@v1",
        "obs.storage.btrfs.filesystem.usage@v2"
    );
    method!(
        "device_add",
        DeviceAddInput,
        DeviceMutationOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.device.add@v1",
        "mut.storage.btrfs.device.add@v2"
    );
    method!(
        "device_remove",
        DeviceRemoveInput,
        DeviceMutationOutput,
        SideEffect::Mutation,
        "cap.storage.btrfs.device.remove@v1",
        "mut.storage.btrfs.device.remove@v2"
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestState {
        subvolumes: Vec<SubvolumeRecord>,
        devices: Vec<DeviceRecord>,
    }

    struct TestBackend {
        state: Mutex<TestState>,
        effects: AtomicUsize,
        omit_created_readback: bool,
        fail_lookup: bool,
        idle_balance: bool,
        backend_error: bool,
    }

    fn subvol(id: u64, name: &str, parent_uuid: Option<&str>, readonly: bool) -> SubvolumeRecord {
        SubvolumeRecord {
            id,
            name: name.into(),
            parent_id: 5,
            generation: 10,
            uuid: format!("00000000-0000-0000-0000-{id:012}"),
            parent_uuid: parent_uuid.map(str::to_string),
            received_uuid: None,
            readonly,
        }
    }

    impl TestBackend {
        fn new() -> Self {
            Self {
                state: Mutex::new(TestState {
                    subvolumes: vec![subvol(256, "source", None, false)],
                    devices: vec![
                        DeviceRecord {
                            id: 1,
                            uuid: "dev-1".into(),
                            path: "/dev/test1".into(),
                            total_bytes: 1000,
                            bytes_used: 400,
                        },
                        DeviceRecord {
                            id: 2,
                            uuid: "dev-2".into(),
                            path: "/dev/test2".into(),
                            total_bytes: 1000,
                            bytes_used: 300,
                        },
                    ],
                }),
                effects: AtomicUsize::new(0),
                omit_created_readback: false,
                fail_lookup: false,
                idle_balance: false,
                backend_error: false,
            }
        }

        fn record(&self) -> Result<()> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            if self.backend_error {
                bail!("fixture backend failure");
            }
            Ok(())
        }
    }

    impl BtrfsBackend for TestBackend {
        fn filesystem(&self, _path: &Path) -> Result<FsIdentity> {
            if self.backend_error {
                bail!("fixture backend failure");
            }
            Ok(FsIdentity {
                uuid: "fs-uuid".into(),
                generation: 42,
                node_size: 16_384,
                sector_size: 4096,
            })
        }
        fn list_subvolumes(&self, _path: &Path) -> Result<Vec<SubvolumeRecord>> {
            Ok(self.state.lock().unwrap().subvolumes.clone())
        }
        fn subvolume_info(&self, path: &Path) -> Result<SubvolumeRecord> {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("source");
            self.state
                .lock()
                .unwrap()
                .subvolumes
                .iter()
                .find(|s| s.name == name)
                .cloned()
                .with_context(|| format!("subvolume {name} not found"))
        }
        fn subvolume_lookup(&self, path: &Path) -> Result<Option<SubvolumeRecord>> {
            if self.fail_lookup {
                bail!("fixture lookup permission failure");
            }
            Ok(self.subvolume_info(path).ok())
        }
        fn create_subvolume(&self, path: &Path) -> Result<()> {
            self.record()?;
            if !self.omit_created_readback {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                self.state
                    .lock()
                    .unwrap()
                    .subvolumes
                    .push(subvol(300, &name, None, false));
            }
            Ok(())
        }
        fn delete_subvolume(&self, path: &Path) -> Result<()> {
            self.record()?;
            let name = path.file_name().unwrap().to_string_lossy();
            self.state
                .lock()
                .unwrap()
                .subvolumes
                .retain(|s| s.name != name);
            Ok(())
        }
        fn snapshot(&self, source: &Path, destination: &Path, readonly: bool) -> Result<()> {
            self.record()?;
            let source_uuid = self.subvolume_info(source)?.uuid;
            let name = destination
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            self.state.lock().unwrap().subvolumes.push(subvol(
                301,
                &name,
                Some(&source_uuid),
                readonly,
            ));
            Ok(())
        }
        fn scrub_start(&self, _path: &Path) -> Result<Vec<ScrubProgressRecord>> {
            self.record()?;
            Ok(vec![ScrubProgressRecord {
                device_id: 1,
                data_bytes_scrubbed: 100,
                ..Default::default()
            }])
        }
        fn scrub_status(&self, _path: &Path) -> Result<Vec<ScrubProgressRecord>> {
            Ok(vec![ScrubProgressRecord {
                device_id: 1,
                data_bytes_scrubbed: 50,
                ..Default::default()
            }])
        }
        fn balance_start(&self, _path: &Path) -> Result<BalanceProgressRecord> {
            self.record()?;
            Ok(BalanceProgressRecord {
                expected_chunks: 3,
                considered_chunks: 3,
                completed_chunks: 2,
            })
        }
        fn balance_status(
            &self,
            _path: &Path,
        ) -> Result<Option<(BalanceState, BalanceProgressRecord)>> {
            if self.idle_balance {
                return Ok(None);
            }
            Ok(Some((
                BalanceState::Running,
                BalanceProgressRecord {
                    expected_chunks: 3,
                    considered_chunks: 2,
                    completed_chunks: 1,
                },
            )))
        }
        fn spaces(&self, _path: &Path) -> Result<Vec<SpaceRecord>> {
            Ok(vec![SpaceRecord {
                flags: 1,
                total_bytes: 800,
                used_bytes: 500,
            }])
        }
        fn devices(&self, _path: &Path) -> Result<Vec<DeviceRecord>> {
            Ok(self.state.lock().unwrap().devices.clone())
        }
        fn is_block_device(&self, _path: &Path) -> Result<bool> {
            Ok(true)
        }
        fn matching_device_id(
            &self,
            device: &Path,
            devices: &[DeviceRecord],
        ) -> Result<Option<u64>> {
            let device = if device == Path::new("/dev/by-id/test2") {
                Path::new("/dev/test2")
            } else {
                device
            };
            Ok(devices
                .iter()
                .find(|record| Path::new(&record.path) == device)
                .map(|record| record.id))
        }
        fn device_add(&self, _path: &Path, device: &Path) -> Result<()> {
            self.record()?;
            self.state.lock().unwrap().devices.push(DeviceRecord {
                id: 3,
                uuid: "dev-3".into(),
                path: device.display().to_string(),
                total_bytes: 1000,
                bytes_used: 0,
            });
            Ok(())
        }
        fn device_remove(&self, _path: &Path, device_id: u64) -> Result<()> {
            self.record()?;
            self.state
                .lock()
                .unwrap()
                .devices
                .retain(|d| d.id != device_id);
            Ok(())
        }
    }

    #[test]
    fn all_twelve_declared_methods_execute_against_injected_backend() {
        let backend = TestBackend::new();
        let cases = [
            (
                "create_subvolume",
                serde_json::json!({"path":"/mnt/new"}),
                "created",
            ),
            (
                "snapshot",
                serde_json::json!({"source":"/mnt/source","destination":"/mnt/snap","readonly":true}),
                "snapshotted",
            ),
            (
                "list_subvolumes",
                serde_json::json!({"path":"/mnt"}),
                "subvolumes",
            ),
            (
                "scrub_start",
                serde_json::json!({"path":"/mnt"}),
                "completed",
            ),
            (
                "scrub_status",
                serde_json::json!({"path":"/mnt"}),
                "running",
            ),
            (
                "balance_start",
                serde_json::json!({"path":"/mnt"}),
                "completed",
            ),
            (
                "balance_status",
                serde_json::json!({"path":"/mnt"}),
                "running",
            ),
            (
                "filesystem_df",
                serde_json::json!({"path":"/mnt"}),
                "spaces",
            ),
            (
                "filesystem_usage",
                serde_json::json!({"path":"/mnt"}),
                "devices",
            ),
            (
                "device_add",
                serde_json::json!({"device":"/dev/test3","path":"/mnt"}),
                "added",
            ),
            (
                "device_remove",
                serde_json::json!({"device":"/dev/test3","path":"/mnt"}),
                "removed",
            ),
            (
                "delete_subvolume",
                serde_json::json!({"path":"/mnt/new"}),
                "removed_subvolume_id",
            ),
        ];
        for (method, args, proof) in cases {
            let output = dispatch_with_backend(&backend, method, &args).unwrap();
            let object = output.as_object().expect("typed method object output");
            assert!(
                object.get(proof).is_some()
                    || object.values().any(|value| value.as_str() == Some(proof)),
                "{method}: {output}"
            );
        }
        assert_eq!(backend.effects.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn invalid_arguments_are_rejected_before_any_effect() {
        let backend = TestBackend::new();
        for (method, args) in [
            ("create_subvolume", serde_json::json!({"path":"relative"})),
            (
                "snapshot",
                serde_json::json!({"source":"/mnt/source","destination":"/mnt/../snap"}),
            ),
            (
                "device_add",
                serde_json::json!({"device":"dev/test3","path":"/mnt"}),
            ),
            ("delete_subvolume", serde_json::json!({"path":"/"})),
            (
                "balance_start",
                serde_json::json!({"path":"/mnt","unknown":true}),
            ),
        ] {
            assert!(dispatch_with_backend(&backend, method, &args).is_err());
        }
        assert_eq!(backend.effects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn subvolume_limit_is_based_on_observed_whole_filesystem_count() {
        let backend = TestBackend::new();
        backend.state.lock().unwrap().subvolumes = (0..MAX_SUBVOLUMES)
            .map(|i| subvol(256 + i as u64, &format!("s{i}"), None, false))
            .collect();
        let error = dispatch_with_backend(
            &backend,
            "create_subvolume",
            &serde_json::json!({"path":"/mnt/one-too-many"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("limit 5"));
        assert_eq!(backend.effects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_postcondition_and_backend_failures_are_errors() {
        let mut backend = TestBackend::new();
        backend.omit_created_readback = true;
        let error = dispatch_with_backend(
            &backend,
            "create_subvolume",
            &serde_json::json!({"path":"/mnt/missing"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not found"));

        let mut failing = TestBackend::new();
        failing.backend_error = true;
        let error =
            dispatch_with_backend(&failing, "scrub_start", &serde_json::json!({"path":"/mnt"}))
                .unwrap_err();
        assert!(error.to_string().contains("fixture backend failure"));
    }

    #[test]
    fn delete_provider_error_is_not_misreported_as_absence() {
        let mut backend = TestBackend::new();
        backend
            .state
            .lock()
            .unwrap()
            .subvolumes
            .push(subvol(300, "delete-me", None, false));
        backend.fail_lookup = true;
        let error = dispatch_with_backend(
            &backend,
            "delete_subvolume",
            &serde_json::json!({"path":"/mnt/delete-me"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("absence could not be verified"));
        assert_eq!(backend.effects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn idle_balance_has_no_fabricated_progress() {
        let mut backend = TestBackend::new();
        backend.idle_balance = true;
        let output = dispatch_with_backend(
            &backend,
            "balance_status",
            &serde_json::json!({"path":"/mnt"}),
        )
        .unwrap();
        assert_eq!(output["state"], "idle");
        assert!(output["progress"].is_null());
    }

    #[test]
    fn device_membership_uses_provider_identity_across_path_aliases() {
        let backend = TestBackend::new();
        let output = dispatch_with_backend(
            &backend,
            "device_remove",
            &serde_json::json!({"device":"/dev/by-id/test2","path":"/mnt"}),
        )
        .unwrap();
        assert_eq!(output["operation"], "removed");
        assert!(output["devices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|device| device["id"] != 2));
    }

    #[test]
    fn aggregate_byte_counts_reject_overflow() {
        let error = checked_sum("allocated bytes", [u64::MAX, 1].into_iter()).unwrap_err();
        assert!(error.to_string().contains("overflowed u64"));
    }

    #[test]
    fn schema_and_dispatch_cover_the_same_twelve_methods() {
        let expected = BTreeSet::from([
            "create_subvolume",
            "delete_subvolume",
            "snapshot",
            "list_subvolumes",
            "scrub_start",
            "scrub_status",
            "balance_start",
            "balance_status",
            "filesystem_df",
            "filesystem_usage",
            "device_add",
            "device_remove",
        ]);
        let schema = btrfs_schema();
        let actual = schema
            .methods
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
        assert!(schema
            .methods
            .values()
            .all(|m| m.returns.is_some() && m.args["type"] == "object"));
    }
}

inventory::submit! { crate::default_registry::PluginReg::new("btrfs", |_ctx| std::sync::Arc::new(BtrfsPlugin::new())) }
