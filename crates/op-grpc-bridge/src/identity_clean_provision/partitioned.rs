//! Read-only kernel/GPT proof for the operator-prepared upgrade-over-seed view.
use super::*;

fn crc32(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(value & 1)));
        }
    }
    !value
}

pub(super) fn verify_partition(
    device: &File,
    image: &File,
    backing: &std::fs::Metadata,
    layout: &PartitionedOverlay,
    number: u8,
) -> Result<File> {
    let pin = match number { 1 => &layout.root_partition, 2 => &layout.upgrade_partition,
        _ => anyhow::bail!("unsupported identity partition") };
    let mut header = [0u8; 92];
    image.read_exact_at(&mut header, 512)?;
    let header_crc = u32::from_le_bytes(header[16..20].try_into()?);
    header[16..20].fill(0);
    ensure!(&header[..8] == b"EFI PART" && u32::from_le_bytes(header[8..12].try_into()?) == 0x10000
        && u32::from_le_bytes(header[12..16].try_into()?) == 92 && crc32(&header) == header_crc
        && u64::from_le_bytes(header[24..32].try_into()?) == 1
        && u64::from_le_bytes(header[32..40].try_into()?) == backing.len() / 512 - 1
        && uuid::Uuid::from_bytes_le(header[56..72].try_into()?).to_string() == layout.disk_guid
        && u64::from_le_bytes(header[72..80].try_into()?) == 2
        && u32::from_le_bytes(header[80..84].try_into()?) == 128
        && u32::from_le_bytes(header[84..88].try_into()?) == 128,
        "protected image GPT header differs");
    let mut table = [0u8; 16384];
    image.read_exact_at(&mut table, 1024)?;
    ensure!(crc32(&table) == u32::from_le_bytes(header[88..92].try_into()?)
        && table[256..].iter().all(|byte| *byte == 0), "GPT is corrupt or includes another partition");
    let entry = &table[(usize::from(number) - 1) * 128..usize::from(number) * 128];
    ensure!(uuid::Uuid::from_bytes_le(entry[..16].try_into()?).to_string()
            == "0fc63daf-8483-4772-8e79-3d69d8477de4"
        && uuid::Uuid::from_bytes_le(entry[16..32].try_into()?).to_string() == pin.partuuid
        && u64::from_le_bytes(entry[32..40].try_into()?) == pin.start_sector
        && u64::from_le_bytes(entry[40..48].try_into()?).checked_add(1)
            == pin.start_sector.checked_add(pin.sectors), "GPT partition pin differs");
    let meta = device.metadata()?;
    ensure!(meta.file_type().is_block_device(), "partition is not a block device");
    let sysfs = format!("/sys/dev/block/{}:{}", libc::major(meta.rdev()), libc::minor(meta.rdev()));
    let kernel_name = std::fs::read_link(&sysfs)?;
    let kernel_name = kernel_name.file_name().and_then(|name| name.to_str()).context("partition name unavailable")?;
    let suffix = format!("p{number}");
    let loop_name = kernel_name.strip_suffix(&suffix).context("wrong loop partition number")?;
    let loop_number = loop_name.strip_prefix("loop").context("partition parent is not loop storage")?;
    ensure!(!loop_number.is_empty() && loop_number.bytes().all(|byte| byte.is_ascii_digit()), "invalid loop parent");
    for (field, expected) in [("partition", u64::from(number)), ("start", pin.start_sector), ("size", pin.sectors)] {
        let value = std::fs::read_to_string(format!("{sysfs}/{field}"))?;
        ensure!(value.len() <= 32 && value.trim().parse::<u64>()? == expected, "kernel partition geometry differs");
    }
    let parent = open(Path::new(&format!("/dev/{loop_name}")), false)?;
    let mut status: LoopInfo = unsafe { std::mem::zeroed() };
    // SAFETY: held loop descriptor and ABI-sized LOOP_GET_STATUS64 output.
    ensure!(unsafe { libc::ioctl(parent.as_raw_fd(), 0x4c05 as libc::c_ulong, &mut status) } == 0
        && status.number.to_string() == loop_number && loop_matches(&status, backing, false),
        "partition loop does not own the pinned image");
    Ok(parent)
}

fn subvolume_info(file: &File) -> Result<[u8; 504]> {
    let mut info = [0u8; 504];
    // SAFETY: exact btrfs_ioctl_get_subvol_info_args and a held Btrfs descriptor.
    ensure!(unsafe { libc::ioctl(file.as_raw_fd(), 0x81f8_943c as libc::c_ulong, info.as_mut_ptr()) } == 0,
        "upgrade subvolume identity unavailable");
    Ok(info)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PairReceipt {
    phase: String,
    host_fsid: String,
    replica_fsid: String,
    direction: String,
    generation: String,
}

fn readonly_overlay(path: &Path, mounts: &Path) -> Result<()> {
    let mut found = false;
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")?;
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else { continue; };
        let left = left.split_whitespace().collect::<Vec<_>>();
        let right = right.split_whitespace().collect::<Vec<_>>();
        if left.get(4).copied() != path.to_str() { continue; }
        ensure!(left.get(5).is_some_and(|options| options.split(',').any(|option| option == "ro"))
            && right.first() == Some(&"overlay"), "guest root is not a read-only overlay");
        let expected = format!("lowerdir={}/upgrade-layer:{}/seed-layer", mounts.display(), mounts.display());
        ensure!(right.get(2).is_some_and(|options| options.split(',').any(|option| option == expected)),
            "overlay layers differ from the pinned identity");
        found = true;
    }
    ensure!(found, "overlay mount absent");
    Ok(())
}

pub(super) fn verify_overlay(
    plan: &CleanPlan, image: &File, backing: &std::fs::Metadata, seed_root: &File, guest: &File,
) -> Result<Vec<File>> {
    let layout = plan.partitioned_overlay.as_ref().context("overlay plan absent")?;
    let name = plan.native_name()?;
    let mounts = Path::new(PARTITIONED_POOL).join(".mounts").join(name);
    let view_path = mounts.join("root-view");
    let view = open_controlled(&view_path, true, Some(&view_path), None)?;
    readonly_overlay(&view_path, &mounts)?;
    readonly_overlay(Path::new(&plan.native_root), &mounts)?;
    ensure!(view.metadata()?.dev() == guest.metadata()?.dev()
        && view.metadata()?.ino() == guest.metadata()?.ino() && guest.metadata()?.uid() == 1000000,
        "Incus root is not the UID-mapped overlay view");
    let seed_layer_path = mounts.join("seed-layer");
    let seed_layer = open_controlled(&seed_layer_path, true, Some(&seed_layer_path), None)?;
    fs_info(&seed_layer, &plan.binding.btrfs_uuid)?;
    member_identity(&seed_layer, &plan.members["@"], false)?;
    ensure!(seed_layer.metadata()?.dev() == seed_root.metadata()?.dev()
        && seed_layer.metadata()?.ino() == seed_root.metadata()?.ino()
        && seed_layer.metadata()?.uid() == 1000000, "seed layer is not the mapped inherited @");
    let upgrade_path = mounts.join("upgrade");
    let upgrade = open(&upgrade_path, true)?;
    let upgrade_fs = fs_info(&upgrade, &layout.upgrade_fsid)?;
    ensure!(subvolume(&upgrade)? == 5 && upgrade_fs.max_id == 1, "upgrade is not standalone partition storage");
    let mut device: DevInfo = unsafe { std::mem::zeroed() }; device.devid = 1;
    // SAFETY: exact DEV_INFO ABI and the held upgrade descriptor.
    ensure!(unsafe { libc::ioctl(upgrade.as_raw_fd(), 0xd000_941e as libc::c_ulong, &mut device) } == 0,
        "upgrade device identity unavailable");
    let end = device.path.iter().position(|byte| *byte == 0).context("upgrade device path invalid")?;
    let partition = open(Path::new(std::str::from_utf8(&device.path[..end])?), false)?;
    let parent = verify_partition(&partition, image, backing, layout, 2)?;
    let published_path = upgrade_path.join("@");
    let published = open_controlled(&published_path, true, Some(&published_path), None)?;
    let upgrade_layer_path = mounts.join("upgrade-layer");
    let upgrade_layer = open_controlled(&upgrade_layer_path, true, Some(&upgrade_layer_path), None)?;
    fs_info(&published, &layout.upgrade_fsid)?;
    fs_info(&upgrade_layer, &layout.upgrade_fsid)?;
    ensure!(published.metadata()?.dev() == upgrade_layer.metadata()?.dev()
        && published.metadata()?.ino() == upgrade_layer.metadata()?.ino()
        && upgrade_layer.metadata()?.uid() == 1000000, "upgrade layer is not the mapped published @");
    for layer in [&seed_layer, &upgrade_layer] {
        let mut flags: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: held layer descriptor and native statvfs output.
        ensure!(unsafe { libc::fstatvfs(layer.as_raw_fd(), &mut flags) } == 0
            && flags.f_flag & libc::ST_RDONLY != 0, "overlay layer lost mount read-only protection");
    }
    let pair_path = Path::new(PARTITIONED_POOL).join(".provisioning").join(format!("{name}.upgrade.json"));
    let pair = open(&pair_path, false)?;
    let pair_meta = pair.metadata()?;
    ensure!(pair_meta.is_file() && pair_meta.uid() == 0 && pair_meta.mode() & 0o077 == 0
        && pair_meta.len() <= 4096, "upgrade pair receipt is not protected and bounded");
    let mut bytes = vec![0; pair_meta.len() as usize];
    pair.read_exact_at(&mut bytes, 0)?;
    let receipt: PairReceipt = serde_json::from_slice(&bytes)?;
    ensure!(receipt.phase == "paired" && receipt.direction == "host-to-container"
        && receipt.host_fsid == layout.host_upgrade_fsid && receipt.replica_fsid == layout.upgrade_fsid
        && !receipt.generation.is_empty() && receipt.generation.len() <= 128
        && receipt.generation.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "upgrade pair receipt differs from the adoption plan");
    let received_path = upgrade_path.join(".upgrade-sync").join(name).join(&receipt.generation);
    let received = open(&received_path, true)?;
    let host_path = Path::new(PARTITIONED_POOL).join(".mounts/host-upgrade/.upgrade-sync")
        .join(name).join(&receipt.generation);
    let host = open(&host_path, true)?;
    fs_info(&received, &layout.upgrade_fsid)?;
    fs_info(&host, &layout.host_upgrade_fsid)?;
    let published_info = subvolume_info(&published)?;
    let received_info = subvolume_info(&received)?;
    let host_info = subvolume_info(&host)?;
    ensure!(u64::from_ne_bytes(received_info[288..296].try_into()?) & 1 != 0
        && u64::from_ne_bytes(host_info[288..296].try_into()?) & 1 != 0
        && published_info[312..328] == received_info[296..312]
        && received_info[328..344] == host_info[296..312], "incremental upgrade baseline ancestry differs");
    Ok(vec![view, seed_layer, upgrade, partition, parent, published, upgrade_layer, pair, received, host])
}
