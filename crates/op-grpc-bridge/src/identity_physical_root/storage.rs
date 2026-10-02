//! Read-only physical-seed membership and mounted guest-subvolume proof.
//! Kernel ABIs match linux/btrfs.h and linux/loop.h; no mount/device mutation.
use super::*;
use nix::libc;
use std::{
    ffi::CString,
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, FileTypeExt, MetadataExt},
    },
};

pub(super) struct StorageGuard {
    _files: Vec<File>,
}

#[repr(C)]
struct FsInfo {
    max_id: u64,
    num_devices: u64,
    fsid: [u8; 16],
    nodesize: u32,
    sectorsize: u32,
    clone_alignment: u32,
    csum_type: u16,
    csum_size: u16,
    flags: u64,
    generation: u64,
    metadata_uuid: [u8; 16],
    reserved: [u8; 944],
}
#[repr(C)]
struct DevInfo {
    devid: u64,
    uuid: [u8; 16],
    bytes_used: u64,
    total_bytes: u64,
    fsid: [u8; 16],
    unused: [u64; 377],
    path: [u8; 1024],
}
#[repr(C)]
struct LoopInfo {
    device: u64,
    inode: u64,
    rdevice: u64,
    offset: u64,
    sizelimit: u64,
    number: u32,
    encrypt_type: u32,
    encrypt_key_size: u32,
    flags: u32,
    file_name: [u8; 64],
    crypt_name: [u8; 64],
    encrypt_key: [u8; 32],
    init: [u64; 2],
}
const _: () = assert!(std::mem::size_of::<FsInfo>() == 1024);
const _: () = assert!(std::mem::size_of::<DevInfo>() == 4096);
const _: () = assert!(std::mem::size_of::<LoopInfo>() == 232);

fn open(path: &Path, directory: bool) -> Result<File> {
    open_controlled(path, directory, None, None)
}

fn open_controlled(
    path: &Path,
    directory: bool,
    guest_boundary: Option<&Path>,
    mapped_parent: Option<(&Path, u64)>,
) -> Result<File> {
    ensure!(
        canonical_path(path.to_str().context("physical path encoding")?),
        "unsafe physical path"
    );
    let root = CString::new("/")?;
    // SAFETY: NUL-terminated paths, owned descriptors; no filesystem mutation.
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    ensure!(fd >= 0, "open physical path root");
    // SAFETY: successful open returned a fresh descriptor.
    let mut current = unsafe { File::from_raw_fd(fd) };
    let components: Vec<_> = path
        .components()
        .filter_map(|p| {
            if let Component::Normal(p) = p {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    let mut prefix = PathBuf::from("/");
    for (index, component) in components.iter().enumerate() {
        prefix.push(component);
        let name = CString::new(component.as_encoded_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if index + 1 < components.len() || directory {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: valid held parent FD and NUL-terminated component.
        let next = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        ensure!(next >= 0, "physical path absent or contains a symlink");
        // SAFETY: successful openat returned a fresh descriptor.
        current = unsafe { File::from_raw_fd(next) };
        if index + 1 < components.len() || directory {
            let meta = current.metadata()?;
            if guest_boundary.is_some_and(|boundary| prefix.starts_with(boundary)) {
                // The guest-owned tree is separately verified as the pinned
                // Btrfs subvolume, with RO parent and protected children.
            } else if let Some((parent, uid)) =
                mapped_parent.filter(|(parent, _)| *parent == prefix.as_path())
            {
                let _ = parent;
                ensure!(
                    meta.uid() as u64 == uid && meta.gid() == 0 && meta.mode() & 0o777 == 0o100,
                    "native parent no longer matches the protected guest-root idmap"
                );
            } else {
                ensure!(
                    meta.uid() == 0 && meta.mode() & 0o022 == 0,
                    "physical path ancestor is not root-controlled"
                );
            }
        }
    }
    Ok(current)
}

fn fs_info(file: &File, uuid: &str) -> Result<FsInfo> {
    // SAFETY: integer-only kernel output with exact asserted ABI size.
    let mut info: FsInfo = unsafe { std::mem::zeroed() };
    // SAFETY: valid descriptor and correctly sized writable output.
    ensure!(
        unsafe { libc::ioctl(file.as_raw_fd(), 0x8400_941f as libc::c_ulong, &mut info) } == 0,
        "physical Btrfs filesystem query failed"
    );
    ensure!(
        info.fsid == *uuid::Uuid::parse_str(uuid)?.as_bytes()
            && info.num_devices == 1
            && info.max_id > 0
            && info.max_id <= 16,
        "physical root is not the pinned single-writable-member filesystem"
    );
    Ok(info)
}

fn subvolume(file: &File) -> Result<u64> {
    let mut bytes = [0u8; 504];
    // SAFETY: buffer matches btrfs_ioctl_get_subvol_info_args.
    ensure!(
        unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                0x81f8_943c as libc::c_ulong,
                bytes.as_mut_ptr(),
            )
        } == 0,
        "physical Btrfs subvolume query failed"
    );
    Ok(u64::from_ne_bytes(bytes[..8].try_into()?))
}

fn seed_header(file: &File, fsid: &str, generation: u64) -> Result<()> {
    let mut header = [0u8; 80];
    file.read_exact_at(&mut header, 65536)?;
    ensure!(
        header[32..48] == *uuid::Uuid::parse_str(fsid)?.as_bytes()
            && u64::from_le_bytes(header[48..56].try_into()?) == 65536
            && u64::from_le_bytes(header[64..72].try_into()?) == 0x4d5f_5366_5248_425f
            && u64::from_le_bytes(header[56..64].try_into()?) & (1 << 32) != 0
            && u64::from_le_bytes(header[72..80].try_into()?) == generation,
        "shared physical seed superblock changed"
    );
    Ok(())
}

fn loop_matches(info: &LoopInfo, backing: &std::fs::Metadata, readonly: bool) -> bool {
    info.device == backing.dev()
        && info.inode == backing.ino()
        && info.rdevice == backing.rdev()
        && info.offset == 0
        && info.sizelimit == 0
        && info.encrypt_type == 0
        && info.encrypt_key_size == 0
        && (info.flags & 1 != 0) == readonly
}

pub(super) fn verify(plan: &PhysicalRootPlan) -> Result<StorageGuard> {
    let seed = open(Path::new(&plan.seed_device), false)?;
    let leaf = open(Path::new(&plan.binding.device_path), false)?;
    let seed_meta = seed.metadata()?;
    let leaf_meta = leaf.metadata()?;
    ensure!(
        seed_meta.file_type().is_block_device()
            && seed_meta.uid() == 0
            && seed_meta.mode() & 0o002 == 0,
        "shared physical seed is not protected root-owned block storage"
    );
    ensure!(
        leaf_meta.is_file()
            && leaf_meta.uid() == 0
            && leaf_meta.mode() & 0o077 == 0
            && leaf_meta.ino() == plan.leaf_inode
            && leaf_meta.len() == plan.leaf_size,
        "private writable leaf changed"
    );
    seed_header(&seed, &plan.seed_fsid, plan.seed_generation)?;
    let volume = open(Path::new(&plan.binding.mount_point), true)?;
    let guest_path = Path::new(&plan.binding.mount_point).join("guest");
    let guest = open_controlled(&guest_path, true, Some(&guest_path), None)?;
    let native_path = Path::new(&plan.native_root);
    let native_parent = native_path
        .parent()
        .context("physical native parent absent")?;
    let mapped_uid = if std::fs::symlink_metadata(native_parent)?.uid() == 0 {
        None
    } else {
        let idmap: JsonValue = serde_json::from_str(
            plan.instance["config"]["volatile.idmap.current"]
                .as_str()
                .context("physical native current idmap absent")?,
        )?;
        let roots: Vec<_> = idmap
            .as_array()
            .context("physical idmap is not an array")?
            .iter()
            .filter(|entry| {
                entry["Isuid"] == true
                    && entry["Nsid"] == 0
                    && entry["Maprange"].as_u64().is_some_and(|size| size > 0)
            })
            .collect();
        ensure!(
            roots.len() == 1,
            "physical idmap has no unique root mapping"
        );
        Some(
            roots[0]["Hostid"]
                .as_u64()
                .filter(|id| *id > 0)
                .context("invalid physical mapped root UID")?,
        )
    };
    let native = open_controlled(
        native_path,
        true,
        Some(native_path),
        mapped_uid.map(|uid| (native_parent, uid)),
    )?;
    let fs = fs_info(&volume, &plan.binding.btrfs_uuid)?;
    fs_info(&guest, &plan.binding.btrfs_uuid)?;
    fs_info(&native, &plan.binding.btrfs_uuid)?;
    ensure!(
        subvolume(&volume)? == 5
            && subvolume(&guest)? == plan.guest_subvolume_id
            && subvolume(&native)? == plan.guest_subvolume_id
            && guest.metadata()?.dev() == native.metadata()?.dev()
            && guest.metadata()?.ino() == native.metadata()?.ino(),
        "native root is not the pinned guest subvolume"
    );
    let mut files = vec![seed, leaf, volume, guest, native];
    for (path, readonly) in [
        ("agent", true),
        ("agent/soul", true),
        ("agent/rules", true),
        ("agent/shared", true),
        ("agent/user", false),
        ("agent/memory", false),
        ("agent/skills", false),
        ("opt/run-mounts/persist", false),
    ] {
        let member = open_controlled(&guest_path.join(path), true, Some(&guest_path), None)?;
        ensure!(
            member.metadata()?.ino() == 256,
            "physical guest member is not a subvolume root"
        );
        fs_info(&member, &plan.binding.btrfs_uuid)?;
        ensure!(
            subvolume(&member)? > 256,
            "physical guest member is not a subvolume"
        );
        let mut flags = 0u64;
        // SAFETY: held Btrfs subvolume descriptor and u64 output ABI.
        ensure!(
            unsafe { libc::ioctl(member.as_raw_fd(), 0x8008_9419 as libc::c_ulong, &mut flags) }
                == 0
                && (flags & 2 != 0) == readonly,
            "physical guest subvolume protection differs"
        );
        files.push(member);
    }
    let mut saw_seed = false;
    let mut saw_leaf = false;
    let mut count = 0;
    for id in 1..=fs.max_id {
        // SAFETY: integer-only kernel output with exact asserted ABI size.
        let mut dev: DevInfo = unsafe { std::mem::zeroed() };
        dev.devid = id;
        // SAFETY: held mount descriptor and correctly sized writable output.
        if unsafe { libc::ioctl(files[2].as_raw_fd(), 0xd000_941e as libc::c_ulong, &mut dev) } != 0
        {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(libc::ENODEV) | Some(libc::ENOENT) | Some(libc::EINVAL)
            ) {
                continue;
            }
            return Err(error).context("read physical Btrfs member");
        }
        count += 1;
        let end = dev
            .path
            .iter()
            .position(|x| *x == 0)
            .context("unterminated physical member path")?;
        let path = Path::new(std::str::from_utf8(&dev.path[..end])?);
        let member = open(path, false)?;
        ensure!(
            member.metadata()?.file_type().is_block_device(),
            "physical member is not a block device"
        );
        // SAFETY: integer-only kernel output with exact asserted ABI size.
        let mut status: LoopInfo = unsafe { std::mem::zeroed() };
        // SAFETY: held block descriptor and correctly sized writable output.
        ensure!(
            unsafe { libc::ioctl(member.as_raw_fd(), 0x4c05 as libc::c_ulong, &mut status) } == 0,
            "physical member is not a pinned loop mapping"
        );
        if dev.fsid == *uuid::Uuid::parse_str(&plan.seed_fsid)?.as_bytes()
            && loop_matches(&status, &seed_meta, true)
        {
            ensure!(!saw_seed, "duplicate physical seed member");
            saw_seed = true;
        } else if dev.fsid == *uuid::Uuid::parse_str(&plan.binding.btrfs_uuid)?.as_bytes()
            && loop_matches(&status, &leaf_meta, false)
        {
            ensure!(!saw_leaf, "duplicate physical writable member");
            saw_leaf = true;
        } else {
            anyhow::bail!("unplanned physical Btrfs member");
        }
        files.push(member);
    }
    ensure!(
        count == 2 && saw_seed && saw_leaf,
        "physical seed/sprout ancestry incomplete"
    );
    Ok(StorageGuard { _files: files })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_loop_provenance_requires_exact_backing_and_access_mode() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let meta = file.as_file().metadata().unwrap();
        // SAFETY: test ABI fixture contains only integer fields.
        let mut status: LoopInfo = unsafe { std::mem::zeroed() };
        status.device = meta.dev();
        status.inode = meta.ino();
        status.rdevice = meta.rdev();
        status.flags = 1;
        assert!(loop_matches(&status, &meta, true));
        assert!(!loop_matches(&status, &meta, false));
        status.offset = 4096;
        assert!(!loop_matches(&status, &meta, true));
        status.offset = 0;
        status.inode += 1;
        assert!(!loop_matches(&status, &meta, true));
    }
}
