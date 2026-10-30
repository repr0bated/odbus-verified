use anyhow::{Context, Result, bail, ensure};
use std::{collections::{BTreeMap, BTreeSet}, fs::File, os::{fd::{AsFd, AsRawFd}, unix::{fs::MetadataExt, process::CommandExt}}, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
use super::{fs, inventory::{self, Inventory, Service}, model::*};

pub fn run(command: &mut Command, seconds: u64) -> Result<()> {
    command.stdin(Stdio::null()).process_group(0);
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: pre_exec uses only async-signal-safe process operations.
    unsafe { command.pre_exec(move || {
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 || libc::getppid() != parent {
            return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
        }
        Ok(())
    }); }
    let mut child = command.spawn().context("start bounded release helper")?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(status) = child.try_wait()? { ensure!(status.success(), "release helper exited {status}"); return Ok(()); }
        if Instant::now() >= deadline {
            // SAFETY: the child was started in its own process group; never signal a foreign service.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
            let _ = child.wait(); bail!("release helper deadline exceeded ({seconds}s)");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn service_path(name: &str) -> PathBuf { Path::new(SERVICES).join(name) }
pub fn linked(name: &str) -> Result<bool> {
    let path = Path::new(ENABLED).join(name);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(m) => { ensure!(m.file_type().is_symlink(), "enabled service is not a symlink: {name}"); Ok(true) }
    }
}
pub fn status(name: &str, newly_approved: bool) -> Result<Wanted> {
    inventory::service_name(name)?;
    let link = linked(name)?;
    let path = service_path(name);
    if link {
        ensure!(std::fs::canonicalize(Path::new(ENABLED).join(name))? == std::fs::canonicalize(&path)?,
            "enabled service {name} does not use /etc/sv; bootstrap must migrate its supervision without changing wanted state");
    }
    read_status(&path, link, newly_approved)
}
fn read_status(path: &Path, link: bool, newly_approved: bool) -> Result<Wanted> {
    let down = path.join("down").try_exists()?;
    let status = std::fs::read(path.join("supervise/status"));
    let bytes = match status {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !link => {
            return Ok(Wanted { pid: 0, start: [0; 12], paused: false,
                want: if newly_approved && !down { b'u' } else { b'd' }, state: 0, down, linked: false, new_service: newly_approved });
        }
        Err(e) => return Err(e).with_context(|| format!("read supervisor status at {}", path.display())),
    };
    ensure!(bytes.len() == 20 && matches!(bytes[17], b'u' | b'd') && bytes[19] <= 2, "invalid runit status at {}", path.display());
    let mut start = [0; 12]; start.copy_from_slice(&bytes[..12]);
    Ok(Wanted { pid: u32::from_le_bytes(bytes[12..16].try_into()?), start,
        paused: bytes[16] != 0, want: bytes[17], state: bytes[19], down, linked: link, new_service: false })
}
fn external_status(name: &str) -> Result<Wanted> {
    let enabled = Path::new(ENABLED).join(name);
    let path = if linked(name)? { enabled } else { service_path(name) };
    let bytes = std::fs::read(path.join("supervise/status")).with_context(|| format!("external dependency {name} has no supervisor status"))?;
    ensure!(bytes.len() == 20 && matches!(bytes[17], b'u' | b'd'), "invalid external runit status");
    let mut start = [0; 12]; start.copy_from_slice(&bytes[..12]);
    Ok(Wanted { pid: u32::from_le_bytes(bytes[12..16].try_into()?), start, paused: bytes[16] != 0,
        want: bytes[17], state: bytes[19], down: path.join("down").try_exists()?, linked: true, new_service: false })
}
fn visit(name: &str, services: &BTreeMap<String, Service>, selected: &BTreeSet<String>, visiting: &mut BTreeSet<String>, done: &mut BTreeSet<String>, order: &mut Vec<String>) -> Result<()> {
    if done.contains(name) { return Ok(()); }
    ensure!(visiting.insert(name.into()), "service dependency cycle at {name}");
    let s = services.get(name).context("missing dependency declaration")?;
    for dep in s.deps.iter().chain(&s.starts) { if selected.contains(dep) { visit(dep, services, selected, visiting, done, order)?; } }
    visiting.remove(name); done.insert(name.into()); order.push(name.into()); Ok(())
}
pub fn plan(old: Option<&Inventory>, new: &Inventory, approve_links: &BTreeSet<String>) -> Result<Plan> {
    let mut services = new.services.clone();
    if let Some(old) = old { for (n,s) in &old.services { services.entry(n.clone()).or_insert_with(|| s.clone()); } }
    let mut changed = BTreeSet::new();
    let paths: BTreeSet<_> = new.entries.keys().chain(old.into_iter().flat_map(|i| i.entries.keys())).collect();
    for path in paths {
        let a = old.and_then(|i| i.entries.get(path)); let b = new.entries.get(path);
        if inventory::changed(a,b) {
            for e in [a,b].into_iter().flatten() { changed.extend(e.consumers.iter().cloned()); }
            if let Some(svc) = path.strip_prefix("sv/").and_then(|s| s.split('/').next()) { changed.insert(svc.into()); }
        }
    }
    if old.is_none() { changed.extend(services.iter().filter(|(_,s)| s.release_owned).map(|(n,_)| n.clone())); }
    changed.retain(|n| services.get(n).is_some_and(|s| s.release_owned));
    let mut selected = changed.clone();
    loop {
        let before = selected.len();
        for name in selected.clone() {
            for dep in &services[&name].deps {
                if services[dep].release_owned { selected.insert(dep.clone()); }
            }
        }
        for (name,s) in &services {
            if s.release_owned && (linked(name)? || approve_links.contains(name)) && s.deps.iter().chain(&s.starts).any(|d| selected.contains(d)) { selected.insert(name.clone()); }
        }
        if before == selected.len() { break; }
    }
    let mut wanted = BTreeMap::new();
    for name in &selected {
        let state = status(name, approve_links.contains(name))?;
        ensure!(!state.paused, "paused release consumer {name}; no mutation performed");
        wanted.insert(name.clone(), state);
        let log = format!("{name}/log");
        let log_definition = format!("sv/{name}/log/run");
        let existing_log = service_path(name).join("log").try_exists()?;
        if new.entries.contains_key(&log_definition) || old.is_some_and(|i| i.entries.contains_key(&log_definition)) || existing_log {
            let logger = if !existing_log {
                let down = service_path(name).join(".op-release-disabled-log/down").try_exists()?;
                let newly_supervised = new.entries.contains_key(&log_definition) && (wanted[name].linked || wanted[name].new_service);
                Wanted { pid: 0, start: [0; 12], paused: false, want: if newly_supervised && !down { b'u' } else { b'd' },
                    state: 0, down, linked: false, new_service: newly_supervised }
            } else {
                read_status(&service_path(&log), wanted[name].linked, approve_links.contains(name))?
            };
            ensure!(!logger.paused, "paused logger for release consumer {name}; no mutation performed");
            wanted.insert(log, logger);
        }
    }
    for start in &selected {
        let mut seen = BTreeSet::new(); let mut pending = vec![start.as_str()];
        while let Some(name) = pending.pop() {
            if !seen.insert(name) { continue; }
            for dep in services[name].deps.iter().chain(&services[name].starts) {
                let state = if let Some(w) = wanted.get(dep) { w.clone() } else if services[dep].release_owned { status(dep, approve_links.contains(dep))? } else { external_status(dep)? };
                ensure!(!state.paused && state.want == b'u', "{start} auto-start/dependency chain reaches paused or want-down service {dep}; no mutation performed");
                pending.push(dep);
            }
        }
    }
    let mut order = Vec::new(); let mut visiting = BTreeSet::new(); let mut done = BTreeSet::new();
    for name in &selected { visit(name, &services, &selected, &mut visiting, &mut done, &mut order)?; }
    Ok(Plan { changed, order, wanted })
}
fn sv(action: &str, name: &str) -> Result<()> {
    fs::secure_absolute(Path::new("/usr/bin/sv"), libc::O_RDONLY)?;
    run(Command::new("/usr/bin/sv").args(["-w", "30", action]).arg(service_path(name)), 35)
}
pub fn exit_added(state: &mut State, name: &str) -> Result<()> {
    ensure!(!linked(name)?, "cannot exit a supervisor while its enable link remains");
    if !service_path(name).join("supervise/status").try_exists()? { return Ok(()); }
    state.intent(format!("exit-added-supervisor:{name}"))?;
    sv("exit", name)
}
fn wait_supervisor(name: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if service_path(name).join("supervise/status").try_exists()? { return Ok(()); }
        ensure!(Instant::now() < deadline, "runsvdir did not supervise approved service {name}");
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub fn down(state: &mut State) -> Result<()> {
    let order = state.journal.as_ref().context("missing service journal")?.plan.order.clone();
    for name in order.iter().rev() {
        if !linked(name)? { continue; }
        state.intent(format!("service-down:{name}"))?;
        sv("down", name)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let s = status(name, false)?;
            if s.pid == 0 && s.state == 0 && s.want == b'd' { break; }
            ensure!(Instant::now() < deadline, "service did not stop: {name}");
            std::thread::sleep(Duration::from_millis(50));
        }
        let log = format!("{name}/log");
        if state.journal.as_ref().unwrap().plan.wanted.contains_key(&log) && supervisor_identity(&log)?.is_some() {
            state.intent(format!("logger-down:{name}"))?;
            sv("down", &log)?;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let logger = read_status(&service_path(&log), true, false)?;
                if logger.pid == 0 && logger.state == 0 && logger.want == b'd' { break; }
                ensure!(Instant::now() < deadline, "logger did not stop: {name}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    state.intent("services-down")
}
pub fn restore(state: &mut State, inventory: &Inventory) -> Result<()> {
    let plan = state.journal.as_ref().context("missing service journal")?.plan.clone();
    restore_loggers(state, false)?;
    for name in &plan.order {
        let wanted = &plan.wanted[name];
        ensure!(service_path(name).join("down").try_exists()? == wanted.down, "down marker changed for {name}");
        ensure!(linked(name)? == (wanted.linked || wanted.new_service), "enable link changed for {name}");
        if !wanted.linked && !wanted.new_service { continue; }
        if wanted.new_service { wait_supervisor(name)?; }
        let action = if wanted.want == b'u' { "up" } else if wanted.pid != 0 && wanted.state == 1 { "once" } else { continue; };
        state.intent(format!("service-start:{name}:{action}"))?;
        state.journal.as_mut().unwrap().allowed.insert(name.clone());
        state.save()?; super::boot::transition(state)?;
        sv(action, name)?;
        sv("check", name)?;
        if let Some(probe) = inventory.services.get(name).and_then(|s| s.probe.as_ref()) {
            fs::secure_absolute(Path::new(&probe[0]), libc::O_RDONLY)?;
            run(Command::new(&probe[0]).args(&probe[1..]), 30)?;
        }
    }
    state.intent("services-restored")
}
pub fn restore_original(state: &mut State) -> Result<()> {
    let plan = state.journal.as_ref().context("missing original service journal")?.plan.clone();
    restore_loggers(state, true)?;
    for name in &plan.order {
        let wanted = &plan.wanted[name];
        if !wanted.linked { continue; }
        ensure!(linked(name)? && service_path(name).join("down").try_exists()? == wanted.down,
            "original service link/down state differs: {name}");
        let action = if wanted.want == b'u' { "up" } else if wanted.pid != 0 && wanted.state == 1 { "once" } else { continue; };
        state.intent(format!("restore-original-service:{name}:{action}"))?;
        sv(action, name)?;
        sv("check", name)?;
    }
    state.intent("original-service-state-restored")
}
fn restore_loggers(state: &mut State, originals: bool) -> Result<()> {
    let wanted = state.journal.as_ref().context("missing logger journal")?.plan.wanted.clone();
    for (name, wanted) in wanted.iter().filter(|(name,_)| name.ends_with("/log")) {
        if !wanted.linked && (originals || !wanted.new_service) { continue; }
        if !service_path(name).join("run").try_exists()? { continue; }
        ensure!(service_path(name).join("down").try_exists()? == wanted.down, "logger down marker changed");
        if wanted.new_service { wait_supervisor(name)?; }
        let action = if wanted.want == b'u' { "up" } else if wanted.pid != 0 && wanted.state == 1 { "once" } else { continue; };
        state.intent(format!("restore-logger:{name}:{action}"))?;
        sv(action, name)?; sv("check", name)?;
    }
    Ok(())
}

fn supervisor_identity(name: &str) -> Result<Option<(u32, u64)>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = match std::fs::metadata(service_path(name).join("supervise/lock")) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let key = format!("{:02x}:{:02x}:{}", libc::major(metadata.dev()), libc::minor(metadata.dev()), metadata.ino());
    for line in std::fs::read_to_string("/proc/locks")?.lines() {
        let mut fields = line.split_whitespace();
        fields.next();
        if fields.next() != Some("FLOCK") { continue; }
        let owner = fields.nth(2).context("runit lock owner absent")?;
        if fields.next() != Some(key.as_str()) { continue; }
        let pid: u32 = owner.parse().context("runit lock has no process owner")?;
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let start = stat.rsplit_once(") ").context("invalid supervisor process status")?.1
            .split_whitespace().nth(19).context("supervisor start time absent")?.parse().context("invalid supervisor start time")?;
        ensure!(std::fs::read_to_string(format!("/proc/{pid}/comm"))?.trim() == "runsv",
            "service supervision lock is not owned by runsv");
        return Ok(Some((pid, start)));
    }
    Ok(None)
}
pub fn require_supervisor(name: &str) -> Result<()> {
    ensure!(supervisor_identity(name)?.is_some(), "service has no identifiable runsv supervisor: {name}");
    Ok(())
}
pub fn reload_loggers(state: &mut State) -> Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let journal = state.journal.as_ref().context("logger reload without journal")?;
    let names: Vec<_> = journal.plan.order.iter().filter(|name| journal.plan.wanted[*name].linked
        && journal.loggers.iter().any(|change| change.touched && &change.service == *name)).cloned().collect();
    if names.is_empty() { return Ok(()); }
    for name in names {
        if !linked(&name)? { continue; }
        let old = supervisor_identity(&name)?.context("run supervisor disappeared before logger reload")?;
        // A pidfd prevents a recycled PID from receiving the runsv exit signal.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, old.0, 0) };
        ensure!(raw >= 0, "pidfd_open runsv: {}", std::io::Error::last_os_error());
        let pidfd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        ensure!(supervisor_identity(&name)?.as_ref() == Some(&old), "run supervisor changed before reload");
        state.intent(format!("reload-logger-supervisor:{name}"))?;
        // runsv treats SIGTERM as its documented x control command. No enabled
        // link or down marker is changed. Q admission remains closed.
        let result = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, pidfd.as_raw_fd(), libc::SIGTERM, std::ptr::null::<libc::siginfo_t>(), 0) };
        ensure!(result == 0, "pidfd_send_signal runsv: {}", std::io::Error::last_os_error());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(current) = supervisor_identity(&name)? {
                if current != old {
                    let _ = status(&name, false)?;
                    if !service_path(&name).join("log").try_exists()? || supervisor_identity(&format!("{name}/log"))?.is_some() { break; }
                }
            }
            ensure!(Instant::now() < deadline, "runsvdir did not reload logger configuration for {name}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    // A restarted runsv resets its transient wanted state. Fixed run admission
    // is still closed until publication and wanted-state restoration finish.
    down(state)
}
fn absent(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn catalog_open(path: &Path) -> Result<Option<File>> {
    match fs::secure_absolute(path, libc::O_RDONLY | libc::O_DIRECTORY) {
        Ok(root) => Ok(Some(root)),
        Err(error) if absent(&error) => Ok(None),
        Err(error) => Err(error),
    }
}
fn catalog_identity(path: &Path, root: &File) -> Result<CatalogRemoval> {
    let metadata = root.metadata()?;
    ensure!(metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "catalog directory is not exclusively root-owned: {}", path.display());
    let btrfs = match btrfs_uapi::filesystem::filesystem_info(root.as_fd()) {
        Ok(info) => Some((info.uuid.to_string(), btrfs_uapi::subvolume::subvolume_info(root.as_fd())?.id)),
        Err(error) if error as i32 == libc::ENOTTY => None,
        Err(error) => return Err(error.into()),
    };
    Ok(CatalogRemoval { path: path.into(), inode: metadata.ino(), device: metadata.dev(),
        boot_id: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?, btrfs, entries: Vec::new() })
}
fn same_catalog_directory(actual: &CatalogRemoval, expected: &CatalogRemoval) -> bool {
    actual.inode == expected.inode && actual.btrfs == expected.btrfs
        && (actual.btrfs.is_some() || (actual.boot_id == expected.boot_id && actual.device == expected.device))
}
fn catalog_entries(root: &File) -> Result<Vec<CatalogEntry>> {
    let device = root.metadata()?.dev();
    let mount = fs::mount_id(root.as_fd())?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))? {
        ensure!(entries.len() < 50_000, "catalog inventory exceeds bound");
        let name = entry?.file_name().into_string().map_err(|_| anyhow::anyhow!("non-UTF8 catalog entry"))?;
        let mut file = fs::open_beneath(root.as_fd(), Path::new(&name), libc::O_RDONLY | libc::O_NONBLOCK)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file() && metadata.dev() == device && fs::mount_id(file.as_fd())? == mount
            && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "catalog entry is not a root-owned local regular file: {name}");
        entries.push(CatalogEntry { name, inode: metadata.ino(), uid: metadata.uid(), gid: metadata.gid(),
            mode: metadata.mode(), sha256: fs::hash_file(&mut file)? });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}
fn catalog_snapshot(path: &Path, root: &File, allow_empty: bool) -> Result<CatalogRemoval> {
    let mut record = catalog_identity(path, root)?;
    record.entries = catalog_entries(root)?;
    if allow_empty && record.entries.is_empty() { return Ok(record); }
    let mut manifest = fs::open_beneath(root.as_fd(), Path::new(".manifest.json"), libc::O_RDONLY)?;
    let document: serde_json::Value = serde_json::from_slice(&fs::read_bounded(&mut manifest, 16 * 1024 * 1024)?)?;
    let plugins = document["plugins"].as_object().context("invalid catalog manifest")?;
    let mut names: BTreeSet<String> = [".manifest.json".into()].into();
    for (plugin, hash) in plugins {
        let hash = hash.as_str().context("invalid catalog plugin hash")?;
        ensure!(hash.len() >= 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()), "invalid catalog plugin hash");
        let name = format!("{plugin}.{}.blob", &hash[..16]);
        fs::relative(Path::new(&name))?;
        ensure!(Path::new(&name).components().count() == 1 && names.insert(name), "invalid catalog plugin filename");
    }
    ensure!(record.entries.len() == names.len() && record.entries.iter().all(|e| names.contains(&e.name)),
        "catalog contains missing or undeclared files: {}", path.display());
    Ok(record)
}
fn catalog_manifest(record: &CatalogRemoval) -> Result<&str> {
    record.entries.iter().find(|entry| entry.name == ".manifest.json")
        .map(|entry| entry.sha256.as_str()).context("catalog manifest fingerprint missing")
}
fn verify_catalog_entries(actual: &[CatalogEntry], expected: &[CatalogEntry], complete: bool, inodes: bool) -> Result<()> {
    ensure!(!complete || actual.len() == expected.len(), "catalog inventory is incomplete");
    for entry in actual {
        let index = expected.binary_search_by(|known| known.name.cmp(&entry.name))
            .map_err(|_| anyhow::anyhow!("unrecorded catalog entry: {}", entry.name))?;
        let known = &expected[index];
        ensure!(entry.sha256 == known.sha256 && (!inodes || (entry.inode == known.inode
            && entry.uid == known.uid && entry.gid == known.gid && entry.mode == known.mode)),
            "catalog entry differs from durable ownership: {}", entry.name);
    }
    Ok(())
}
fn verify_catalog_at(path: &Path, expected: Option<&CatalogRemoval>) -> Result<()> {
    match (catalog_open(path)?, expected) {
        (None, None) => Ok(()),
        (Some(root), Some(expected)) => {
            ensure!(same_catalog_directory(&catalog_identity(path, &root)?, expected),
                "catalog directory identity changed: {}", path.display());
            verify_catalog_entries(&catalog_entries(&root)?, &expected.entries, true, true)
        }
        _ => bail!("catalog presence changed: {}", path.display()),
    }
}
fn verify_catalog_child(parent: &File, root: &File) -> Result<()> {
    ensure!(parent.metadata()?.dev() == root.metadata()?.dev()
        && fs::mount_id(parent.as_fd())? == fs::mount_id(root.as_fd())?,
        "catalog publication cannot cross a mount boundary");
    Ok(())
}

/// Reconcile the exact directories, not the publisher's exit status. Named files
/// are complete source bytes (the publisher links O_TMPFILE only after fsync).
/// A cleanup inventory is durable before the first unlink and admits only
/// missing recorded entries after a crash, never additional or changed bytes.
pub fn reconcile_catalog(state: &mut State) -> Result<bool> {
    let Some(transaction) = state.journal.as_ref().and_then(|j| j.catalog_publication.as_ref()) else { return Ok(false); };
    ensure!([Path::new(PERSISTED), Path::new(SHM)].contains(&transaction.destination.as_path())
        && transaction.stage.parent() == transaction.destination.parent()
        && transaction.stage.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(".opdbus-release-catalog-")),
        "invalid journaled catalog publication paths");
    // tmpfs identities never survive reboot. Only complete absence permits
    // discarding a volatile intent; a recreated foreign directory is untouched.
    if transaction.destination == Path::new(SHM) && transaction.parent.btrfs.is_none()
        && transaction.parent.boot_id != std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
    {
        ensure!(catalog_open(&transaction.stage)?.is_none() && catalog_open(&transaction.destination)?.is_none(),
            "volatile catalog paths were recreated after reboot; ownership is unknown");
        state.journal.as_mut().unwrap().catalog_publication = None;
        state.intent("catalog-volatile-publication-lost")?;
        return Ok(false);
    }
    let parent = fs::secure_absolute(&transaction.parent.path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    ensure!(same_catalog_directory(&catalog_identity(&transaction.parent.path, &parent)?, &transaction.parent),
        "catalog publication parent changed");
    if transaction.phase != CatalogPhase::Cleaning {
        let stage = catalog_open(&transaction.stage)?;
        let destination = catalog_open(&transaction.destination)?;
        let mut published = false;
        let cleanup = if transaction.phase == CatalogPhase::Creating {
            // No publisher can run until the new inode has been saved. A crash
            // between mkdir and that save may leave only this planned empty dir.
            verify_catalog_at(&transaction.destination, transaction.previous.as_ref())?;
            if let Some(stage) = stage {
                verify_catalog_child(&parent, &stage)?;
                let record = catalog_snapshot(&transaction.stage, &stage, true)?;
                ensure!(record.entries.is_empty(), "unrecorded catalog stage is not empty");
                Some(record)
            } else { None }
        } else {
            let created = transaction.created.as_ref().context("publisher stage identity missing")?;
            let at_destination = destination.as_ref().map(|root| catalog_identity(&transaction.destination, root)).transpose()?;
            if at_destination.as_ref().is_some_and(|actual| same_catalog_directory(actual, created)) {
                published = true;
                let destination = destination.as_ref().unwrap();
                verify_catalog_child(&parent, destination)?;
                verify_catalog_entries(&catalog_entries(destination)?, &transaction.source.entries, true, false)?;
                destination.sync_all()?;
                match (&transaction.previous, stage) {
                    (Some(previous), Some(stage)) => {
                        verify_catalog_child(&parent, &stage)?;
                        let mut record = catalog_identity(&transaction.stage, &stage)?;
                        ensure!(same_catalog_directory(&record, previous), "exchanged catalog stage is not the recorded old destination");
                        record.entries = catalog_entries(&stage)?;
                        verify_catalog_entries(&record.entries, &previous.entries, true, true)?;
                        Some(record)
                    }
                    (None, None) => None,
                    _ => bail!("catalog exchange left unexpected stage presence"),
                }
            } else {
                verify_catalog_at(&transaction.destination, transaction.previous.as_ref())?;
                let stage = stage.context("journaled catalog stage disappeared before publication")?;
                verify_catalog_child(&parent, &stage)?;
                let mut record = catalog_identity(&transaction.stage, &stage)?;
                ensure!(same_catalog_directory(&record, created), "unpublished catalog stage identity changed");
                record.entries = catalog_entries(&stage)?;
                verify_catalog_entries(&record.entries, &transaction.source.entries, false, false)?;
                Some(record)
            }
        };
        // Also completes an exchange whose subprocess died before parent fsync.
        parent.sync_all()?;
        let transaction = state.journal.as_mut().unwrap().catalog_publication.as_mut().unwrap();
        transaction.published = published;
        transaction.cleanup = cleanup;
        transaction.phase = CatalogPhase::Cleaning;
    }
    // Retry the intent even when a previous save failed in this process.
    state.intent("catalog-cleanup-owned-stage")?;
    let transaction = state.journal.as_ref().unwrap().catalog_publication.as_ref().unwrap();
    if transaction.published {
        let destination = catalog_open(&transaction.destination)?.context("published catalog disappeared")?;
        ensure!(same_catalog_directory(&catalog_identity(&transaction.destination, &destination)?,
            transaction.created.as_ref().context("published catalog identity absent")?), "published catalog identity changed");
        verify_catalog_entries(&catalog_entries(&destination)?, &transaction.source.entries, true, false)?;
    } else {
        verify_catalog_at(&transaction.destination, transaction.previous.as_ref())?;
    }
    if let Some(cleanup) = &transaction.cleanup {
        if let Some(root) = catalog_open(&cleanup.path)? {
            verify_catalog_child(&parent, &root)?;
            ensure!(same_catalog_directory(&catalog_identity(&cleanup.path, &root)?, cleanup), "catalog cleanup directory identity changed");
            verify_catalog_entries(&catalog_entries(&root)?, &cleanup.entries, false, true)?;
            for entry in &cleanup.entries {
                let mut file = match fs::open_beneath(root.as_fd(), Path::new(&entry.name), libc::O_RDONLY | libc::O_NONBLOCK) {
                    Ok(file) => file,
                    Err(error) if absent(&error) => continue,
                    Err(error) => return Err(error),
                };
                let metadata = file.metadata()?;
                ensure!(metadata.is_file() && metadata.dev() == root.metadata()?.dev()
                    && metadata.ino() == entry.inode && metadata.uid() == entry.uid && metadata.gid() == entry.gid
                    && metadata.mode() == entry.mode && fs::hash_file(&mut file)? == entry.sha256,
                    "catalog cleanup file changed: {}", entry.name);
                fs::remove_at(root.as_fd(), Path::new(&entry.name), false)?;
            }
            let current = catalog_open(&cleanup.path)?.context("catalog cleanup directory disappeared")?;
            ensure!(same_catalog_directory(&catalog_identity(&cleanup.path, &current)?, cleanup), "catalog cleanup path replaced");
            fs::remove_at(parent.as_fd(), Path::new(cleanup.path.file_name().context("catalog stage has no name")?), true)?;
        }
    } else {
        ensure!(catalog_open(&transaction.stage)?.is_none(), "unexpected catalog stage remains");
    }
    parent.sync_all()?;
    let published = transaction.published;
    state.journal.as_mut().unwrap().catalog_publication = None;
    state.intent(if published { "catalog-publication-reconciled" } else { "catalog-unpublished-stage-removed" })?;
    Ok(published)
}

fn publish_catalog(state: &mut State, command: &mut Command, source: &Path, destination: &Path, expected: &str) -> Result<()> {
    reconcile_catalog(state)?;
    let source_root = fs::secure_absolute(source, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let source = catalog_snapshot(source, &source_root, false)?;
    ensure!(catalog_manifest(&source)? == expected, "catalog source differs from selected manifest");
    let parent_path = destination.parent().context("catalog destination has no parent")?;
    let parent = fs::secure_absolute(parent_path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let previous = catalog_open(destination)?.map(|root| {
        verify_catalog_child(&parent, &root)?;
        catalog_snapshot(destination, &root, true)
    }).transpose()?;
    let stage = parent_path.join(format!(".opdbus-release-catalog-{}", uuid::Uuid::new_v4()));
    ensure!(catalog_open(&stage)?.is_none(), "catalog stage already exists");
    let transaction = CatalogPublication { destination: destination.into(), stage: stage.clone(),
        parent: catalog_identity(parent_path, &parent)?, source, previous, created: None,
        phase: CatalogPhase::Creating, published: false, cleanup: None };
    state.journal.as_mut().context("catalog publication without journal")?.catalog_publication = Some(transaction);
    state.intent(format!("catalog-create-stage:{}", stage.display()))?;
    fs::mkdir_at(parent.as_fd(), Path::new(stage.file_name().unwrap()), 0o700)?;
    let directory = fs::open_beneath(parent.as_fd(), Path::new(stage.file_name().unwrap()), libc::O_RDONLY | libc::O_DIRECTORY)?;
    verify_catalog_child(&parent, &directory)?;
    let created = catalog_snapshot(&stage, &directory, true)?;
    ensure!(created.entries.is_empty(), "new catalog stage is not empty");
    directory.sync_all()?;
    let transaction = state.journal.as_mut().unwrap().catalog_publication.as_mut().unwrap();
    transaction.created = Some(created);
    transaction.phase = CatalogPhase::Publishing;
    state.intent(format!("catalog-publish:{}", destination.display()))?;
    let transaction = state.journal.as_ref().unwrap().catalog_publication.as_ref().unwrap();
    verify_catalog_at(&transaction.source.path, Some(&transaction.source))?;
    verify_catalog_at(destination, transaction.previous.as_ref())?;
    command.env("OPDBUS_RELEASE_CATALOG_STAGE", &stage);
    // Keep the existing transaction flock alive until the publisher exits even
    // if the controller dies. Recovery must never race a surviving publisher.
    unsafe { command.pre_exec(|| {
        if libc::fcntl(9, libc::F_SETFD, 0) < 0 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    }); }
    let result = run(command, 120);
    let published = reconcile_catalog(state)?;
    if published { return Ok(()); }
    result?;
    bail!("catalog helper returned success without publishing the journaled stage")
}

pub fn catalog(state: &mut State, release: &Release) -> Result<()> {
    reconcile_catalog(state)?;
    let binary = Path::new("/usr/local/libexec/op-dbus/bin/opblob");
    fs::secure_absolute(binary, libc::O_RDONLY)?;
    publish_catalog(state, Command::new(binary).args(["persist", SHIPPED, PERSISTED]),
        Path::new(SHIPPED), Path::new(PERSISTED), &release.catalog_sha256)?;
    publish_catalog(state, Command::new(binary).arg("stage-shm"),
        Path::new(PERSISTED), Path::new(SHM), &release.catalog_sha256)?;
    for root in [SHIPPED, PERSISTED, SHM] {
        let digest = fs::hash_file(&mut fs::secure_absolute(&Path::new(root).join(".manifest.json"), libc::O_RDONLY)?)?;
        ensure!(digest == release.catalog_sha256, "catalog manifest mismatch at {root}");
    }
    state.intent("catalog-verified")
}
pub fn restore_boot_lost_shm(state: &mut State, expected: &str) -> Result<()> {
    reconcile_catalog(state)?;
    use std::os::{fd::{AsFd, AsRawFd}, unix::fs::MetadataExt};
    let persisted = fs::hash_file(&mut fs::secure_absolute(&Path::new(PERSISTED).join(".manifest.json"), libc::O_RDONLY)?)?;
    ensure!(persisted == expected, "preserved persisted catalog cannot recreate the original SHM catalog");
    let slot = state.journal.as_ref().context("missing first-activation recovery journal")?.to.context("missing candidate")?;
    let release = state.release(slot)?.clone();
    let root = state.root(slot)?;
    let target = release.targets.iter().find(|t| t.path == Path::new("/usr/local/libexec/op-dbus/bin")).context("candidate has no catalog-owner target")?;
    let lower = fs::open_beneath(root.as_fd(), &target.source, libc::O_RDONLY | libc::O_DIRECTORY)?;
    state.intent("build-detached-catalog-owner-for-shm-restore")?;
    let base = fs::release_base()?;
    let mount = fs::detached_overlay(lower.as_fd(), base.as_fd(), &format!("opdbus-release:{}:{}:catalog-recovery", slot.name(), release.name), false)?;
    let directory = fs::directory(mount.as_fd())?;
    state.slots.get_mut(&slot).and_then(Option::as_mut).unwrap().devices.insert(directory.metadata()?.dev());
    if !Path::new("/dev/shm/opdbus").try_exists()? {
        let shm_parent = fs::secure_absolute(Path::new("/dev/shm"), libc::O_RDONLY | libc::O_DIRECTORY)?;
        state.intent("restore-volatile-shm-parent")?;
        fs::mkdir_at(shm_parent.as_fd(), Path::new("opdbus"), 0o755)?;
    }
    state.intent("restore-original-shm-through-selected-opblob")?;
    let executable = fs::open_beneath(directory.as_fd(), Path::new("opblob"), libc::O_RDONLY)?;
    publish_catalog(state, Command::new(format!("/proc/self/fd/{}", executable.as_raw_fd())).arg("stage-shm"),
        Path::new(PERSISTED), Path::new(SHM), expected)?;
    let shm = fs::hash_file(&mut fs::secure_absolute(&Path::new(SHM).join(".manifest.json"), libc::O_RDONLY)?)?;
    ensure!(shm == expected, "restaged original SHM catalog differs");
    state.intent("original-shm-restored")
}
