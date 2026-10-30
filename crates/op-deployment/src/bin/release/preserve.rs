use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::{self as stdfs, File, OpenOptions}, os::{fd::{AsFd, AsRawFd}, unix::{fs::{MetadataExt, OpenOptionsExt, PermissionsExt}, ffi::OsStrExt}}, path::{Path, PathBuf}};
use super::{fs, inventory::Inventory, model::*};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval { pub targets: BTreeSet<PathBuf>, pub services: BTreeSet<String>, pub link_services: BTreeSet<String>, pub preserve_current: bool }
pub fn approval(inventory: &Inventory) -> Result<Approval> {
    let mut file = fs::secure_absolute(Path::new(APPROVAL), libc::O_RDONLY)?;
    ensure!(file.metadata()?.mode() & 0o777 == 0o600, "first activation approval must be root-owned mode 0600");
    let approval: Approval = serde_json::from_slice(&fs::read_bounded(&mut file, 1024 * 1024)?)?;
    ensure!(approval.targets == inventory.targets.iter().map(|t| t.path.clone()).collect(), "approval does not exactly cover graph targets");
    let services: BTreeSet<_> = inventory.services.iter().filter(|(_,s)| s.release_owned).map(|(n,_)| n.clone()).collect();
    ensure!(approval.services == services && approval.link_services.is_subset(&services), "approval service set differs from release graph");
    for name in &approval.link_services { super::inventory::service_name(name)?; }
    Ok(approval)
}
#[derive(Serialize)]
struct Original { path: PathBuf, kind: &'static str, mode: u32, uid: u32, gid: u32, device: u64, inode: u64, size: u64, sha256: Option<String>, target: Option<PathBuf> }
fn describe(path: &Path, relative: &Path, out: &mut Vec<Original>, device: u64, depth: usize) -> Result<()> {
    ensure!(depth <= 64 && out.len() < 50_000, "preservation inventory exceeds bound");
    ensure!(relative.as_os_str().as_bytes().len() <= 450, "preserved relative path exceeds ledger bound");
    let m = stdfs::symlink_metadata(path)?;
    ensure!(m.dev() == device, "preservation would cross a filesystem");
    let kind = if m.is_file() { "regular" } else if m.is_dir() { "directory" } else if m.file_type().is_symlink() { "symlink" } else { bail!("cannot preserve special file {}", path.display()) };
    let sha256 = if m.is_file() && !super::inventory::protected(path.to_str().context("non-UTF8 preserved path")?) {
        Some(fs::hash_file(&mut OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path)?)?)
    } else { None };
    let target = if m.file_type().is_symlink() { Some(stdfs::read_link(path)?) } else { None };
    out.push(Original { path: relative.to_owned(), kind, mode: m.mode(), uid: m.uid(), gid: m.gid(), device: m.dev(), inode: m.ino(), size: if m.is_dir() { 0 } else { m.len() }, sha256, target });
    if m.is_dir() {
        let mut entries = stdfs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?; entries.sort_by_key(|e| e.file_name());
        for entry in entries { describe(&entry.path(), &relative.join(entry.file_name()), out, device, depth + 1)?; }
    }
    Ok(())
}
pub fn fingerprint(path: &Path) -> Result<Option<String>> {
    let m = match stdfs::symlink_metadata(path) { Ok(m) => m, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(e) => return Err(e.into()) };
    let mut entries = Vec::new(); describe(path, Path::new("."), &mut entries, m.dev(), 0)?;
    // st_dev is an anonymous Btrfs device number and can change over reboot.
    // Same-subvolume placement is checked separately; inode identity survives rename.
    for entry in &mut entries { entry.device = 0; }
    Ok(Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&entries)?))))
}
fn backup(path: &Path, transaction: &str) -> Result<PathBuf> {
    Ok(path.parent().context("preservation path has no parent")?.join(format!(".op-release-original-{transaction}-{}", path.file_name().context("preservation path has no basename")?.to_string_lossy())))
}
fn record(state: &mut State, path: &Path, replacement: &str, replacement_sha256: Option<String>, transaction: &str) -> Result<()> {
    let parent = path.parent().context("preservation path has no parent")?;
    let parent_fd = fs::secure_absolute(parent, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let destination = backup(path, transaction)?;
    ensure!(!destination.try_exists()?, "preservation destination exists");
    let fp = fingerprint(path)?;
    let mut originals = Vec::new();
    if let Ok(metadata) = stdfs::symlink_metadata(path) {
        describe(path, Path::new("."), &mut originals, metadata.dev(), 0)?;
    }
    let original = originals.iter().map(serde_json::to_value).collect::<std::result::Result<Vec<_>, _>>()?;
    if let Ok(m) = stdfs::symlink_metadata(path) {
        ensure!(m.dev() == parent_fd.metadata()?.dev(), "preservation source is another mount");
        if m.is_dir() {
            let item = fs::open_beneath(parent_fd.as_fd(), Path::new(path.file_name().unwrap()), libc::O_RDONLY | libc::O_DIRECTORY)?;
            ensure!(fs::mount_id(item.as_fd())? == fs::mount_id(parent_fd.as_fd())?, "cannot preserve a mountpoint");
            ensure!(m.ino() != 256, "cannot preserve a subvolume root");
        }
    }
    state.journal.as_mut().context("preservation without transaction")?.preserved.push(Preserved { path: path.into(), backup: destination, fingerprint: fp, replacement: replacement.into(), replacement_sha256, original });
    ensure!(state.journal.as_ref().unwrap().preserved.iter().map(|p| p.original.len()).sum::<usize>() <= 50_000,
        "combined preservation ledger exceeds its bound");
    state.save()
}
pub(super) fn stub(service: &str, member: &str) -> String {
    let gate = if matches!(member, "run" | "check") {
        format!("r=/run/opdbus-release/ready\nIFS= read -r state < \"$r\" || exit 111\ncase \"$state\" in\n  complete\\ *) ;;\n  transition\\ *)\n    if grep -Fqx 'q {service}' \"$r\" && ! grep -Fqx 'allow {service}' \"$r\"; then exit 111; fi ;;\n  *) exit 111 ;;\nesac\n")
    } else { String::new() };
    format!("#!/bin/sh\n{gate}exec {SERVICE_TARGET}/{service}/{member} \"$@\"\n")
}
pub fn plan_hooks(state: &mut State, inventory: &Inventory) -> Result<()> {
    let names = state.journal.as_ref().context("hook plan without journal")?.plan.order.clone();
    for name in names {
        for member in ["check", "finish"] {
            let path = Path::new(SERVICES).join(&name).join(member);
            let before = match stdfs::symlink_metadata(&path) {
                Ok(m) => { ensure!(m.is_file(), "fixed hook is not a regular file: {}", path.display()); true },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(e) => return Err(e.into()),
            };
            if before {
                let expected = format!("{:x}", Sha256::digest(stub(&name, member).as_bytes()));
                ensure!(fs::hash_file(&mut fs::secure_absolute(&path, libc::O_RDONLY)?)? == expected,
                    "fixed hook was modified outside the controller: {}", path.display());
            }
            let after = inventory.entries.contains_key(&format!("sv/{name}/{member}"));
            if before != after {
                state.journal.as_mut().unwrap().hooks.push(HookChange { service: name.clone(), member: member.into(), before, after });
            }
        }
    }
    state.intent("optional-hooks-planned")
}
pub fn apply_hooks(state: &mut State, forward: bool) -> Result<()> {
    let mut changes = state.journal.as_ref().context("hook mutation without journal")?.hooks.clone();
    if !forward { changes.reverse(); }
    for change in changes {
        let desired = if forward { change.after } else { change.before };
        let path = Path::new(SERVICES).join(&change.service).join(&change.member);
        let content = stub(&change.service, &change.member);
        let expected = format!("{:x}", Sha256::digest(content.as_bytes()));
        let parent = fs::secure_absolute(path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
        let exists = match stdfs::symlink_metadata(&path) {
            Ok(_) => {
                ensure!(fs::hash_file(&mut fs::secure_absolute(&path, libc::O_RDONLY)?)? == expected,
                    "fixed hook changed during transition: {}", path.display());
                true
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if desired {
            if !exists {
                state.intent(format!("hook-create:{}", path.display()))?;
                fs::atomic_text(&path, &content)?;
            }
            state.intent(format!("hook-executable:{}", path.display()))?;
            stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o755))?;
            File::open(&path)?.sync_all()?; parent.sync_all()?;
        } else if exists {
            state.intent(format!("hook-remove:{}", path.display()))?;
            fs::remove_at(parent.as_fd(), Path::new(path.file_name().unwrap()), false)?;
        }
    }
    Ok(())
}
pub fn plan(state: &mut State, inventory: &Inventory, approval: &Approval) -> Result<()> {
    let transaction = state.journal.as_ref().context("missing first activation journal")?.candidate.as_ref().context("missing candidate")?.name.clone();
    if Path::new(SHM).try_exists()? {
        let live = fs::hash_file(&mut fs::secure_absolute(&Path::new(SHM).join(".manifest.json"), libc::O_RDONLY)?)?;
        let persisted = fs::hash_file(&mut fs::secure_absolute(&Path::new(PERSISTED).join(".manifest.json"), libc::O_RDONLY)?)?;
        ensure!(live == persisted, "first activation requires a persisted copy of the preexisting SHM catalog for reboot-safe restoration");
    }
    for target in &inventory.targets { record(state, &target.path, "directory", None, &transaction)?; }
    if stdfs::symlink_metadata("/opt/op-dbus/current").is_ok() {
        ensure!(approval.preserve_current, "approval must authorize preservation of obsolete current path");
        record(state, Path::new("/opt/op-dbus/current"), "absent", None, &transaction)?;
    }
    for path in [PERSISTED, SHM] { record(state, Path::new(path), "catalog", Some(inventory.catalog_sha256.clone()), &transaction)?; }
    for name in &approval.services {
        let base = Path::new(SERVICES).join(name);
        if !base.try_exists()? { record(state, &base, "service-directory", None, &transaction)?; }
        for member in ["run", "check", "finish", "log/run"] {
            if member == "log/run" && state.journal.as_ref().unwrap().loggers.iter().any(|change| change.service == *name && change.created) { continue; }
            let source = format!("sv/{name}/{member}");
            let path = base.join(member);
            if !inventory.entries.contains_key(&source) {
                if stdfs::symlink_metadata(&path).is_ok() { record(state, &path, "absent", None, &transaction)?; }
                continue;
            }
            // Missing service/log directories are described separately and created before member intents.
            if let Some(parent) = path.parent() { if !parent.try_exists()? && parent != base {
                if !state.journal.as_ref().unwrap().preserved.iter().any(|p| p.path == parent) {
                    state.journal.as_mut().unwrap().preserved.push(Preserved { path: parent.into(), backup: backup(parent, &transaction)?, fingerprint: None, replacement: "service-directory".into(), replacement_sha256: None, original: Vec::new() }); state.save()?;
                }
            } }
            let content = stub(name, member);
            if path.parent().is_some_and(|p| p.try_exists().unwrap_or(false)) {
                record(state, &path, "stub", Some(format!("{:x}", Sha256::digest(content.as_bytes()))), &transaction)?;
            } else {
                state.journal.as_mut().unwrap().preserved.push(Preserved { path: path.clone(), backup: backup(&path, &transaction)?, fingerprint: None, replacement: "stub".into(), replacement_sha256: Some(format!("{:x}", Sha256::digest(content.as_bytes()))), original: Vec::new() }); state.save()?;
            }
        }
        if approval.link_services.contains(name) && !super::services::linked(name)? {
            record(state, &Path::new(ENABLED).join(name), "link", None, &transaction)?;
        }
    }
    state.intent("preservation-planned")
}
pub fn apply(state: &mut State) -> Result<()> {
    let items = state.journal.as_ref().context("missing preservation journal")?.preserved.clone();
    for item in items {
        if matches!(item.replacement.as_str(), "link") { continue; }
        state.intent(format!("preserve:{}", item.path.display()))?;
        let parent = fs::secure_absolute(item.path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
        if item.fingerprint.is_some() && fingerprint(&item.backup)?.is_none() {
            ensure!(fingerprint(&item.path)? == item.fingerprint, "original changed before preservation: {}", item.path.display());
            fs::rename_at(parent.as_fd(), Path::new(item.path.file_name().unwrap()), parent.as_fd(), Path::new(item.backup.file_name().unwrap()))?;
        }
        if item.fingerprint.is_some() { ensure!(fingerprint(&item.backup)? == item.fingerprint, "preserved original fingerprint mismatch"); }
        match item.replacement.as_str() {
            "directory" | "service-directory" => {
                if !item.path.try_exists()? { fs::mkdir_at(parent.as_fd(), Path::new(item.path.file_name().unwrap()), 0o755)?; }
            }
            "stub" => {
                let relative = item.path.strip_prefix(SERVICES)?;
                let mut components = relative.components();
                let name = components.next().context("stub service absent")?.as_os_str().to_str().context("stub service UTF8")?;
                let member = components.as_path().to_str().context("stub member UTF8")?;
                let content = stub(name, member);
                if !item.path.try_exists()? {
                    fs::atomic_text(&item.path, &content)?;
                }
                ensure!(fs::hash_file(&mut fs::secure_absolute(&item.path, libc::O_RDONLY)?)? == item.replacement_sha256.as_deref().unwrap(), "fixed stub content differs");
                state.intent(format!("stub-executable:{}", item.path.display()))?;
                stdfs::set_permissions(&item.path, stdfs::Permissions::from_mode(0o755))?;
                File::open(&item.path)?.sync_all()?; parent.sync_all()?;
            }
            "catalog" | "absent" => {}
            _ => bail!("unknown preservation replacement"),
        }
    }
    state.intent("preservation-applied")
}
pub fn links(state: &mut State) -> Result<()> {
    let items = state.journal.as_ref().context("missing preservation journal")?.preserved.clone();
    for item in items.iter().filter(|i| i.replacement == "link") {
        state.intent(format!("enable-approved:{}", item.path.display()))?;
        let target = Path::new(SERVICES).join(item.path.file_name().unwrap());
        if stdfs::symlink_metadata(&item.path).is_err() { std::os::unix::fs::symlink(&target, &item.path)?; File::open(item.path.parent().unwrap())?.sync_all()?; }
        ensure!(stdfs::read_link(&item.path)? == target, "new enable link differs from approval");
    }
    Ok(())
}
fn remove_catalog(state: &mut State, item: &Preserved) -> Result<()> {
    if !item.path.try_exists()? { return Ok(()); }
    let root = fs::secure_absolute(&item.path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let metadata = root.metadata()?;
    let boot_id = stdfs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let btrfs = match btrfs_uapi::filesystem::filesystem_info(root.as_fd()) {
        Ok(info) => Some((info.uuid.to_string(), btrfs_uapi::subvolume::subvolume_info(root.as_fd())?.id)),
        Err(error) if error as i32 == libc::ENOTTY => None,
        Err(error) => return Err(error.into()),
    };
    let existing = state.journal.as_ref().context("catalog deletion without journal")?.catalog_removals
        .iter().find(|record| record.path == item.path).cloned();
    let record = if let Some(record) = existing { record } else {
        let expected = item.replacement_sha256.as_ref().context("catalog has no expected manifest")?;
        let mut manifest = fs::open_beneath(root.as_fd(), Path::new(".manifest.json"), libc::O_RDONLY)?;
        ensure!(fs::hash_file(&mut manifest)? == *expected, "refusing to remove a catalog not written by this transaction");
        use std::io::{Seek, SeekFrom};
        manifest.seek(SeekFrom::Start(0))?;
        let document: serde_json::Value = serde_json::from_slice(&fs::read_bounded(&mut manifest, 16 * 1024 * 1024)?)?;
        let plugins = document["plugins"].as_object().context("invalid catalog manifest")?;
        let mut expected_names: BTreeSet<String> = [".manifest.json".into()].into();
        for (name, hash) in plugins {
            let hash = hash.as_str().context("invalid plugin hash")?;
            ensure!(hash.len() >= 16 && hash.is_ascii(), "invalid plugin hash");
            expected_names.insert(format!("{name}.{}.blob", &hash[..16]));
        }
        let mut entries = Vec::new();
        for entry in stdfs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))? {
            ensure!(entries.len() < 50_000, "catalog cleanup inventory exceeds bound");
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_| anyhow::anyhow!("non-UTF8 catalog entry"))?;
            ensure!(expected_names.contains(&name), "unexpected catalog content; preserving it for recovery");
            let mut file = fs::open_beneath(root.as_fd(), Path::new(&name), libc::O_RDONLY)?;
            let m = file.metadata()?;
            ensure!(m.is_file() && m.dev() == metadata.dev(), "catalog contains a nonlocal regular entry");
            entries.push(CatalogEntry { name, inode: m.ino(), uid: m.uid(), gid: m.gid(), mode: m.mode(), sha256: fs::hash_file(&mut file)? });
        }
        entries.sort_by(|a,b| a.name.cmp(&b.name));
        let record = CatalogRemoval { path: item.path.clone(), inode: metadata.ino(), device: metadata.dev(),
            boot_id: boot_id.clone(), btrfs: btrfs.clone(), entries };
        state.journal.as_mut().unwrap().catalog_removals.push(record.clone());
        state.intent(format!("catalog-removal-inventory:{}", item.path.display()))?;
        record
    };
    ensure!(metadata.ino() == record.inode && btrfs == record.btrfs
        && (record.btrfs.is_some() || (record.boot_id == boot_id && record.device == metadata.dev())),
        "catalog replacement directory identity changed");
    for entry in stdfs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("non-UTF8 catalog entry")?;
        ensure!(record.entries.binary_search_by(|known| known.name.as_str().cmp(name)).is_ok(),
            "unrecorded catalog entry appeared during cleanup");
    }
    for entry in &record.entries {
        let mut file = match fs::open_beneath(root.as_fd(), Path::new(&entry.name), libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => continue,
            Err(error) => return Err(error),
        };
        let m = file.metadata()?;
        ensure!(m.is_file() && m.ino() == entry.inode && m.uid() == entry.uid && m.gid() == entry.gid
            && m.mode() == entry.mode && fs::hash_file(&mut file)? == entry.sha256,
            "catalog cleanup entry changed: {}", entry.name);
        drop(file);
        state.intent(format!("catalog-remove-entry:{}:{}", item.path.display(), entry.name))?;
        fs::remove_at(root.as_fd(), Path::new(&entry.name), false)?;
    }
    let parent = fs::secure_absolute(item.path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
    state.intent(format!("catalog-remove-directory:{}", item.path.display()))?;
    fs::remove_at(parent.as_fd(), Path::new(item.path.file_name().unwrap()), true)
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RestorePhase { Content, Hooks, Links, Directories }
pub fn restore(state: &mut State, phase: RestorePhase) -> Result<()> {
    let items = state.journal.as_ref().context("missing preservation journal")?.preserved.clone();
    let mut order: Vec<_> = items.iter().enumerate().collect();
    order.sort_by_key(|(index,item)| {
        let priority = if item.path.starts_with(SERVICES) || item.path.starts_with(ENABLED) { 3 }
            else if item.path == Path::new(SHM) { 2 }
            else if item.path == Path::new(PERSISTED) { 1 } else { 0 };
        (priority, std::cmp::Reverse(*index))
    });
    for (_,item) in order {
        let item_phase = if !item.path.starts_with(SERVICES) && !item.path.starts_with(ENABLED) { RestorePhase::Content }
            else if item.replacement == "link" { RestorePhase::Links }
            else if item.replacement == "service-directory" { RestorePhase::Directories }
            else { RestorePhase::Hooks };
        if phase != item_phase { continue; }
        state.intent(format!("restore-original:{}", item.path.display()))?;
        let backup = fingerprint(&item.backup)?;
        if item.fingerprint.is_some() && backup.is_none() {
            if item.path == Path::new(SHM) && fingerprint(&item.path)? != item.fingerprint {
                let expected = item.original.iter().find(|entry| entry["path"].as_str().is_some_and(|p| Path::new(p).file_name().is_some_and(|n| n == ".manifest.json")))
                    .and_then(|entry| entry["sha256"].as_str()).context("original SHM manifest digest absent")?;
                super::services::restore_boot_lost_shm(state, expected)?;
                continue;
            }
            ensure!(fingerprint(&item.path)? == item.fingerprint, "original missing at both source and preservation destination: {}", item.path.display());
            continue;
        }
        if backup.is_some() { ensure!(backup == item.fingerprint, "preserved original changed: {}", item.backup.display()); }
        if item.replacement != "link" && item.fingerprint.is_none() && backup.is_none() && stdfs::symlink_metadata(&item.path).is_err() { continue; }
        match item.replacement.as_str() {
            "directory" => { if item.path.try_exists()? { stdfs::remove_dir(&item.path).with_context(|| format!("replacement directory is not empty: {}", item.path.display()))?; } }
            "service-directory" => remove_service_directory(state, &item.path)?,
            "stub" => {
                if item.path.try_exists()? {
                    ensure!(fs::hash_file(&mut fs::secure_absolute(&item.path, libc::O_RDONLY)?)? == item.replacement_sha256.as_deref().unwrap(), "stub modified outside transaction");
                    stdfs::remove_file(&item.path)?;
                }
            }
            "link" => {
                if stdfs::symlink_metadata(&item.path).is_ok() {
                    ensure!(stdfs::read_link(&item.path)? == Path::new(SERVICES).join(item.path.file_name().unwrap()), "enable link modified outside transaction");
                    stdfs::remove_file(&item.path)?;
                }
                super::services::exit_added(state, item.path.file_name().unwrap().to_str().context("invalid service name")?)?;
            }
            "catalog" => remove_catalog(state, item)?,
            "absent" => ensure!(stdfs::symlink_metadata(&item.path).is_err(), "obsolete path unexpectedly recreated"),
            _ => bail!("unknown preservation item"),
        }
        let parent = fs::secure_absolute(item.path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
        if backup.is_some() { fs::rename_at(parent.as_fd(), Path::new(item.backup.file_name().unwrap()), parent.as_fd(), Path::new(item.path.file_name().unwrap()))?; }
        parent.sync_all()?;
    }
    state.intent("originals-restored")
}
pub(super) fn remove_service_directory(state: &mut State, path: &Path) -> Result<()> {
    if !path.try_exists()? { return Ok(()); }
    let root = fs::secure_absolute(path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let supervise = path.join("supervise");
    if let Ok(metadata) = stdfs::symlink_metadata(&supervise) {
        if metadata.file_type().is_symlink() {
            state.intent(format!("remove-created-supervise-link:{}", path.display()))?;
            fs::remove_at(root.as_fd(), Path::new("supervise"), false)?;
        } else {
            ensure!(metadata.is_dir(), "unexpected new supervise object");
            let entries = stdfs::read_dir(&supervise)?.collect::<std::io::Result<Vec<_>>>()?;
            let allowed = ["control", "ok", "status", "status.new", "pid", "pid.new", "stat", "stat.new", "lock"];
            for entry in &entries {
                ensure!(allowed.iter().any(|name| entry.file_name() == *name) && !entry.file_type()?.is_dir(), "unexpected new supervision state");
            }
            let lock = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(supervise.join("lock")) {
                Ok(lock) => Some(lock),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ensure!(entries.is_empty(), "supervisor lock disappeared before its other entries");
                    None
                },
                Err(error) => return Err(error.into()),
            };
            if let Some(lock) = &lock {
                // SAFETY: locks only this transaction-created supervisor's lock.
                ensure!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                    "new supervisor is still running; preserve its state for recovery");
            }
            let directory = fs::open_beneath(root.as_fd(), Path::new("supervise"), libc::O_RDONLY | libc::O_DIRECTORY)?;
            // Delete the lock last: after a crash, absent lock + empty directory
            // is the only accepted lockless intermediate state.
            for name in allowed.into_iter().filter(|name| *name != "lock").chain(["lock"]) {
                if entries.iter().any(|entry| entry.file_name() == name) {
                    state.intent(format!("remove-created-supervise-entry:{}:{name}", path.display()))?;
                    fs::remove_at(directory.as_fd(), Path::new(name), false)?;
                }
            }
            state.intent(format!("remove-created-supervise-directory:{}", path.display()))?;
            fs::remove_at(root.as_fd(), Path::new("supervise"), true)?;
        }
    }
    let parent = fs::secure_absolute(path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
    state.intent(format!("remove-created-service-directory:{}", path.display()))?;
    fs::remove_at(parent.as_fd(), Path::new(path.file_name().unwrap()), true)
        .with_context(|| format!("new service directory has unrecorded contents: {}", path.display()))
}
