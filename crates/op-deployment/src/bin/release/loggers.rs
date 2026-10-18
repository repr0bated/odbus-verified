use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs::File, os::{fd::AsFd, unix::fs::{MetadataExt, PermissionsExt}}, path::{Path, PathBuf}};
use super::{fs, inventory::Inventory, model::*, preserve, services};

const DISABLED: &str = ".op-release-disabled-log";
fn base(name: &str) -> PathBuf { Path::new(SERVICES).join(name) }
fn present(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => { ensure!(metadata.is_dir(), "logger location is not a real directory: {}", path.display()); Ok(true) },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn identity(path: &Path) -> Result<(String, u64, u64)> {
    let parent = fs::secure_absolute(path.parent().unwrap(), libc::O_RDONLY | libc::O_DIRECTORY)?;
    let root = fs::secure_absolute(path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let subvolume = btrfs_uapi::subvolume::subvolume_info(root.as_fd())?;
    ensure!(root.metadata()?.ino() != 256 && fs::mount_id(root.as_fd())? == fs::mount_id(parent.as_fd())?
        && subvolume.id == btrfs_uapi::subvolume::subvolume_info(parent.as_fd())?.id,
        "logger preservation must remain in the same parent subvolume");
    Ok((btrfs_uapi::filesystem::filesystem_info(root.as_fd())?.uuid.to_string(), subvolume.id, root.metadata()?.ino()))
}
fn stub_hash(name: &str) -> String { format!("{:x}", Sha256::digest(preserve::stub(name, "log/run").as_bytes())) }
fn verify_stub(name: &str, path: &Path) -> Result<()> {
    ensure!(fs::hash_file(&mut fs::secure_absolute(path, libc::O_RDONLY)?)? == stub_hash(name),
        "logger stub modified outside the controller: {}", path.display());
    Ok(())
}

pub fn plan(plan: &Plan, inventory: &Inventory, first: bool) -> Result<Vec<LoggerChange>> {
    let mut changes = Vec::new();
    for name in &plan.order {
        let directory = base(name);
        let before = present(&directory.join("log"))?;
        let after = inventory.entries.contains_key(&format!("sv/{name}/log/run"));
        if before == after { continue; }
        let disabled = present(&directory.join(DISABLED))?;
        ensure!(!(before && disabled), "both active and inactive logger directories exist for {name}");
        ensure!(!first || !disabled, "unregistered inactive logger directory during first activation");
        let parent_wanted = &plan.wanted[name];
        if parent_wanted.linked {
            ensure!(parent_wanted.want == b'u' || parent_wanted.down,
                "logger transition would auto-start want-down main service {name}; no mutation performed");
            services::require_supervisor(name)?;
        }
        if let Some(logger) = plan.wanted.get(&format!("{name}/log")) {
            ensure!(!logger.paused && (logger.want == b'u' || logger.down || !logger.linked),
                "logger transition/rollback would auto-start want-down logger for {name}; no mutation performed");
        }
        let source = directory.join(if before { "log" } else { DISABLED });
        let root_identity = if before || disabled { Some(identity(&source)?) } else { None };
        let run_before = source.join("run").try_exists()?;
        if run_before && !first { verify_stub(name, &source.join("run"))?; }
        changes.push(LoggerChange {
            service: name.clone(), before, after, created: !before && !disabled,
            identity: root_identity, run_before, cleaned: false, touched: false,
        });
    }
    Ok(changes)
}

fn remap_backups(items: &mut [Preserved], from: &Path, to: &Path) {
    for item in items {
        if let Ok(relative) = item.backup.strip_prefix(from) { item.backup = to.join(relative); }
    }
}
pub fn commit_original_receipts(state: &mut State, journal: &Journal) {
    if journal.from.is_some() { return; }
    for change in &journal.loggers {
        if !change.after {
            remap_backups(&mut state.first_activation_originals, &base(&change.service).join("log"), &base(&change.service).join(DISABLED));
        }
    }
}

pub fn apply(state: &mut State, forward: bool) -> Result<()> {
    let mut indices: Vec<_> = (0..state.journal.as_ref().context("logger mutation without journal")?.loggers.len()).collect();
    if !forward { indices.reverse(); }
    for index in indices {
        let mut change = state.journal.as_ref().unwrap().loggers[index].clone();
        if change.cleaned || (!forward && !change.touched) { continue; }
        let desired = if forward { change.after } else { change.before };
        let directory = base(&change.service);
        let active = directory.join("log"); let inactive = directory.join(DISABLED);
        let active_exists = present(&active)?; let inactive_exists = present(&inactive)?;
        ensure!(!(active_exists && inactive_exists), "ambiguous logger directory transition");
        if !active_exists && !inactive_exists {
            if !forward && change.created { continue; }
            ensure!(forward && desired && change.created, "preserved logger directory disappeared");
            let parent = fs::secure_absolute(&directory, libc::O_RDONLY | libc::O_DIRECTORY)?;
            state.journal.as_mut().unwrap().loggers[index].touched = true;
            state.intent(format!("logger-directory-create:{}", change.service))?;
            fs::mkdir_at(parent.as_fd(), Path::new("log"), 0o755)?;
        }
        let actual = if present(&active)? { &active } else { &inactive };
        let actual_identity = identity(actual)?;
        if let Some(expected) = &change.identity {
            ensure!(&actual_identity == expected, "logger directory identity changed");
        } else {
            ensure!(change.created && std::fs::read_dir(actual)?.next().is_none(),
                "unrecorded created logger directory is not empty");
            change.identity = Some(actual_identity);
            state.journal.as_mut().unwrap().loggers[index].identity = change.identity.clone();
            state.intent(format!("logger-directory-identity:{}", change.service))?;
        }
        let destination = if desired { &active } else { &inactive };
        if actual != destination {
            let parent = fs::secure_absolute(&directory, libc::O_RDONLY | libc::O_DIRECTORY)?;
            state.journal.as_mut().unwrap().loggers[index].touched = true;
            state.intent(format!("logger-directory-move:{}:{}", change.service, desired))?;
            fs::rename_at(parent.as_fd(), Path::new(actual.file_name().unwrap()), parent.as_fd(), Path::new(destination.file_name().unwrap()))?;
        }
        // Reconcile physical locations of the one-time originals receipt even
        // after a crash between rename and its following state save.
        let other = if desired { &inactive } else { &active };
        remap_backups(&mut state.first_activation_originals, other, destination);
        state.intent(format!("logger-directory-placed:{}", change.service))?;
        let run = destination.join("run");
        if desired && forward {
            if run.try_exists()? { verify_stub(&change.service, &run)?; }
            else {
                state.intent(format!("logger-stub-create:{}", change.service))?;
                fs::atomic_text(&run, &preserve::stub(&change.service, "log/run"))?;
            }
            state.intent(format!("logger-stub-executable:{}", change.service))?;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755))?;
            File::open(&run)?.sync_all()?; File::open(destination)?.sync_all()?;
        } else if !forward && !change.before && !change.run_before && run.try_exists()? {
            verify_stub(&change.service, &run)?;
            state.intent(format!("logger-added-stub-remove:{}", change.service))?;
            let parent = fs::secure_absolute(destination, libc::O_RDONLY | libc::O_DIRECTORY)?;
            fs::remove_at(parent.as_fd(), Path::new("run"), false)?;
        }
    }
    Ok(())
}

pub fn cleanup_created(state: &mut State) -> Result<()> {
    let changes = state.journal.as_ref().context("logger cleanup without journal")?.loggers.clone();
    for (index, change) in changes.iter().enumerate() {
        if !change.created || change.cleaned { continue; }
        let path = base(&change.service).join(DISABLED);
        if present(&path)? {
            ensure!(change.identity.as_ref() == Some(&identity(&path)?), "created logger directory identity changed");
            preserve::remove_service_directory(state, &path)?;
        }
        state.journal.as_mut().unwrap().loggers[index].cleaned = true;
        state.intent(format!("created-logger-cleaned:{}", change.service))?;
    }
    Ok(())
}
