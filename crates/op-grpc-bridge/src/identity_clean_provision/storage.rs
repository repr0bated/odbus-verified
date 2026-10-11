//! Read-only physical-seed membership and mounted guest-subvolume proof.
//! Kernel ABIs match linux/btrfs.h and linux/loop.h; no mount/device mutation.
use super::*;
#[path = "partitioned.rs"]
mod partitioned;
use std::path::PathBuf;
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
        canonical(path.to_str().context("physical path encoding")?),
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


fn member_identity(file: &File, pin: &MemberPin, readonly: bool) -> Result<()> {
    let mut bytes = [0u8; 504];
    // SAFETY: held Btrfs FD and exact btrfs_ioctl_get_subvol_info_args output.
    ensure!(unsafe { libc::ioctl(file.as_raw_fd(), 0x81f8_943c as libc::c_ulong, bytes.as_mut_ptr()) } == 0,
        "read inherited subvolume identity");
    ensure!(u64::from_ne_bytes(bytes[..8].try_into()?) == pin.id
        && bytes[296..312] == *uuid::Uuid::parse_str(&pin.uuid)?.as_bytes()
        && (u64::from_ne_bytes(bytes[288..296].try_into()?) & 2 != 0) == readonly,
        "inherited subvolume ID/UUID/protection changed");
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SearchKey {
    tree_id: u64, min_objectid: u64, max_objectid: u64, min_offset: u64, max_offset: u64,
    min_transid: u64, max_transid: u64, min_type: u32, max_type: u32, nr_items: u32,
    unused: u32, unused1: u64, unused2: u64, unused3: u64, unused4: u64,
}
#[repr(C)]
struct SearchArgs { key: SearchKey, buffer: [u8; 3992] }
const _: () = assert!(std::mem::size_of::<SearchArgs>() == 4096);

fn exact_members(volume: &File, plan: &CleanPlan) -> Result<()> {
    let expected = plan.members.values().map(|pin| pin.id).collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut min_objectid = 256u64;
    for _ in 0..16 {
        let mut search = SearchArgs { key: SearchKey { tree_id:1, min_objectid, max_objectid:u64::MAX-255, min_offset:0, max_offset:u64::MAX,
            min_transid:0, max_transid:u64::MAX, min_type:132, max_type:132, nr_items:32,
            unused:0, unused1:0, unused2:0, unused3:0, unused4:0 }, buffer:[0;3992] };
        // SAFETY: ABI-sized integer-only search args, held Btrfs descriptor; read-only tree search.
        ensure!(unsafe { libc::ioctl(volume.as_raw_fd(), 0xd000_9411 as libc::c_ulong, &mut search) } == 0,
            "enumerate inherited subvolumes");
        if search.key.nr_items == 0 { ensure!(seen == expected, "filesystem does not contain exactly seven inherited subvolumes"); return Ok(()); }
        let mut offset = 0usize; let mut last = None;
        for _ in 0..search.key.nr_items {
            ensure!(offset + 32 <= search.buffer.len(), "invalid subvolume search output");
            let header = &search.buffer[offset..offset+32];
            let objectid = u64::from_ne_bytes(header[8..16].try_into()?);
            let kind = u32::from_ne_bytes(header[24..28].try_into()?);
            let length = u32::from_ne_bytes(header[28..32].try_into()?) as usize;
            ensure!(objectid >= min_objectid, "subvolume search moved backwards");
            if kind == 132 {
                ensure!(expected.contains(&objectid) && seen.insert(objectid), "unexpected or duplicate inherited subvolume");
            }
            offset = offset.checked_add(32 + length).context("subvolume search overflow")?;
            ensure!(offset <= search.buffer.len(), "invalid subvolume search item length");
            last = Some(objectid);
        }
        min_objectid = last.context("empty subvolume page")?.checked_add(1).context("subvolume enumeration overflow")?;
    }
    anyhow::bail!("subvolume enumeration exceeded bounded budget")
}

pub(super) fn verify(plan: &CleanPlan) -> Result<StorageGuard> {
    let seed = open(Path::new(&plan.seed_device), false)?;
    let leaf = open(Path::new(&plan.binding.device_path), false)?;
    let seed_meta = seed.metadata()?; let leaf_meta = leaf.metadata()?;
    ensure!(seed_meta.file_type().is_block_device() && seed_meta.uid() == 0 && seed_meta.mode() & 2 == 0,
        "shared seed is not protected physical block storage");
    let mut readonly: libc::c_int = 0;
    // SAFETY: BLKROGET reads one integer from held block FD.
    ensure!(unsafe { libc::ioctl(seed.as_raw_fd(), 0x125e as libc::c_ulong, &mut readonly) } == 0 && readonly == 1,
        "shared seed lost block read-only enforcement");
    ensure!(leaf_meta.is_file() && leaf_meta.uid() == 0 && leaf_meta.mode() & 0o077 == 0
        && leaf_meta.ino() == plan.leaf_inode && leaf_meta.len() == plan.leaf_size, "private fresh leaf changed");
    seed_header(&seed, &plan.seed_fsid, plan.seed_generation)?;
    let volume_path = Path::new(&plan.binding.mount_point);
    let volume = open(volume_path, true)?;
    let fs = fs_info(&volume, &plan.binding.btrfs_uuid)?;
    ensure!(subvolume(&volume)? == 5, "external volume is not top-level Btrfs mount");
    exact_members(&volume, plan)?;
    let guest_path = volume_path.join("@");
    let guest = open_controlled(&guest_path, true, Some(&guest_path), None)?;
    let native_path = Path::new(&plan.native_root);
    let native_parent = native_path.parent().context("native root parent missing")?;
    let parent_meta = std::fs::symlink_metadata(native_parent)?;
    let mapped_uid = if parent_meta.uid() == 0 { None } else {
        let idmap: JsonValue = serde_json::from_str(plan.instance["config"]["volatile.idmap.next"].as_str()
            .or_else(|| plan.instance["config"]["volatile.idmap.current"].as_str()).context("native root mapping absent")?)?;
        let roots = idmap.as_array().context("idmap is not an array")?.iter().filter(|entry| entry["Isuid"] == true && entry["Nsid"] == 0).collect::<Vec<_>>();
        ensure!(roots.len() == 1, "native idmap root is not unique");
        Some(roots[0]["Hostid"].as_u64().filter(|id| *id > 0).context("invalid root idmap")?)
    };
    let native = open_controlled(native_path, true, Some(native_path), mapped_uid.map(|uid| (native_parent, uid)))?;
    fs_info(&guest, &plan.binding.btrfs_uuid)?;
    if plan.version != 4 {
        fs_info(&native, &plan.binding.btrfs_uuid)?;
        ensure!(guest.metadata()?.dev() == native.metadata()?.dev() && guest.metadata()?.ino() == native.metadata()?.ino()
            && subvolume(&native)? == plan.members["@"].id, "native root differs from inherited @");
    }
    let mut files = vec![seed, leaf, volume, guest, native];
    if plan.version == 4 {
        files.extend(partitioned::verify_overlay(plan, &files[1], &leaf_meta, &files[3], &files[4])?);
    }
    for member in MEMBERS {
        let member_path = volume_path.join(member);
        let source = open_controlled(&member_path, true, Some(&member_path), None)?;
        let protected = matches!(member, "@agent_shared" | "@agent_soul")
            || (plan.version != 4 && member == "@agent_rules");
        member_identity(&source, &plan.members[member], protected)?;
        fs_info(&source, &plan.binding.btrfs_uuid)?;
        if member != "@" {
            let target_path = native_path.join("agent").join(member.trim_start_matches("@agent_"));
            let target = open_controlled(&target_path, true, Some(native_path), mapped_uid.map(|uid| (native_parent, uid)))?;
            ensure!(source.metadata()?.dev() == target.metadata()?.dev() && source.metadata()?.ino() == target.metadata()?.ino(), "native sibling mount changed");
            member_identity(&target, &plan.members[member], protected)?;
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            // SAFETY: held FD, writable native statvfs output.
            ensure!(unsafe { libc::fstatvfs(target.as_raw_fd(), &mut stat) } == 0
                && (stat.f_flag & libc::ST_RDONLY != 0) == protected, "native sibling mount protection changed");
            files.push(target);
        }
        files.push(source);
    }
    let persist_path = volume_path.join("@agent_user/persist");
    let persist = open_controlled(&persist_path, true, Some(&volume_path.join("@agent_user")), None)?;
    let persist_meta = persist.metadata()?;
    ensure!(persist_meta.ino() == plan.persist_inode && persist_meta.ino() != 256
        && persist_meta.mode() & 0o777 == 0o700 && subvolume(&persist)? == plan.members["@agent_user"].id, "ordinary persistence directory changed");
    let target = open_controlled(&native_path.join("opt/run-mounts/persist"), true, Some(native_path), mapped_uid.map(|uid| (native_parent, uid)))?;
    ensure!(persist_meta.ino() == target.metadata()?.ino() && persist_meta.dev() == target.metadata()?.dev(), "native persistence mount changed");
    files.push(persist); files.push(target);
    let mut saw_seed = false; let mut saw_leaf = false; let mut count = 0;
    for id in 1..=fs.max_id {
        let mut device: DevInfo = unsafe { std::mem::zeroed() }; device.devid = id;
        // SAFETY: exact asserted DEV_INFO ABI and held volume FD.
        if unsafe { libc::ioctl(files[2].as_raw_fd(), 0xd000_941e as libc::c_ulong, &mut device) } != 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::ENODEV) | Some(libc::ENOENT) | Some(libc::EINVAL)) { continue; }
            return Err(error.into());
        }
        count += 1;
        let end = device.path.iter().position(|b| *b == 0).context("invalid native member path")?;
        let file = open(Path::new(std::str::from_utf8(&device.path[..end])?), false)?;
        if device.fsid == *uuid::Uuid::parse_str(&plan.binding.btrfs_uuid)?.as_bytes() && plan.version == 4 {
            let layout = plan.partitioned_overlay.as_ref().context("partition pins absent")?;
            files.push(partitioned::verify_partition(&file, &files[1], &leaf_meta, layout, 1)?);
            ensure!(!saw_leaf, "duplicate writable leaf"); saw_leaf = true;
        } else {
            let mut status: LoopInfo = unsafe { std::mem::zeroed() };
            // SAFETY: exact LOOP_GET_STATUS64 ABI and held member descriptor.
            ensure!(unsafe { libc::ioctl(file.as_raw_fd(), 0x4c05 as libc::c_ulong, &mut status) } == 0,
                "member is not a pinned loop mapping");
            if device.fsid == *uuid::Uuid::parse_str(&plan.seed_fsid)?.as_bytes()
                && loop_matches(&status, &seed_meta, true) {
                ensure!(!saw_seed, "duplicate shared seed member"); saw_seed = true;
            } else if plan.version != 4
                && device.fsid == *uuid::Uuid::parse_str(&plan.binding.btrfs_uuid)?.as_bytes()
                && loop_matches(&status, &leaf_meta, false) {
                ensure!(!saw_leaf, "duplicate writable leaf"); saw_leaf = true;
            } else { anyhow::bail!("unplanned physical Btrfs member"); }
        }
        files.push(file);
    }
    ensure!(count == 2 && saw_seed && saw_leaf, "shared seed/leaf ancestry incomplete");
    Ok(StorageGuard { _files: files })
}
