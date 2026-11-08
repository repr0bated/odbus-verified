use anyhow::{Context, Result, bail, ensure};
use btrfs_uapi::{filesystem::filesystem_info, subvolume::{self, SubvolumeFlags}};
use serde::Deserialize;
use std::{ffi::CString, fs::File, io::{self, Read, Seek, SeekFrom}, os::{fd::{AsFd, AsRawFd, FromRawFd}, unix::{fs::MetadataExt, process::CommandExt}}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::mpsc, time::{Duration, Instant}};
use super::{fs, inventory, lifetime, model::*};

pub fn verify_root(root: &File, release: &Release) -> Result<()> {
    let info = subvolume::subvolume_info(root.as_fd())?;
    ensure!(filesystem_info(root.as_fd())?.uuid.to_string() == FSID && root.metadata()?.ino() == 256,
        "release root is not the expected Btrfs subvolume");
    ensure!(info.id == release.id && info.uuid.to_string() == release.native_uuid
        && info.received_uuid.to_string() == release.received_uuid
        && subvolume::subvolume_flags_get(root.as_fd())?.contains(SubvolumeFlags::RDONLY),
        "release root ID/native UUID/received UUID/read-only identity differs");
    Ok(())
}
pub fn top() -> Result<File> { fs::directory(fs::detached_btrfs(FSID, 5, false)?.as_fd()) }
fn open_child(root: &File, path: &Path) -> Result<Option<File>> {
    match fs::open_beneath(root.as_fd(), path, libc::O_RDONLY | libc::O_DIRECTORY) {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.downcast_ref::<io::Error>().is_some_and(|e| e.kind() == io::ErrorKind::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}
fn check_private(dir: &File) -> Result<()> {
    let m = dir.metadata()?;
    ensure!(m.uid() == 0 && m.gid() == 0 && m.mode() & 0o777 == 0o700, "release administration directory must be root:root 0700"); Ok(())
}
pub fn provision(state: &mut State) -> Result<()> {
    ensure!(state.journal.is_none(), "recover existing transaction before provisioning");
    let parent = fs::secure_absolute(Path::new("/opt/op-dbus"), libc::O_RDONLY | libc::O_DIRECTORY)?;
    ensure!(filesystem_info(parent.as_fd())?.uuid.to_string() == FSID && subvolume::subvolume_info(parent.as_fd())?.id == OPT_ID,
        "/opt/op-dbus must be on the approved @opt subvolume 271");
    let golden = open_child(&parent, Path::new("golden"))?;
    if let Some(golden) = &golden {
        if golden.metadata()?.ino() != 256 {
            ensure!(std::fs::read_dir(format!("/proc/self/fd/{}", golden.as_raw_fd()))?.next().is_none(), "golden is a nonempty ordinary directory; will not replace it");
            let metadata = golden.metadata()?;
            state.golden_original = Some(serde_json::json!({
                "path": GOLDEN, "fsid": FSID, "parent_subvolume": OPT_ID,
                "inode": metadata.ino(), "uid": metadata.uid(), "gid": metadata.gid(),
                "mode": metadata.mode(), "entries": 0,
            }));
        } else {
            let info = subvolume::subvolume_info(golden.as_fd())?;
            ensure!(info.parent_id == OPT_ID && !subvolume::subvolume_flags_get(golden.as_fd())?.contains(SubvolumeFlags::RDONLY), "golden has wrong parent or read-only state");
        }
    }
    state.journal = Some(Journal::new("provision", state.active, None)); state.save()?;
    provision_resume(state)
}
pub fn provision_resume(state: &mut State) -> Result<()> {
    let parent = fs::secure_absolute(Path::new("/opt/op-dbus"), libc::O_RDONLY | libc::O_DIRECTORY)?;
    ensure!(filesystem_info(parent.as_fd())?.uuid.to_string() == FSID && subvolume::subvolume_info(parent.as_fd())?.id == OPT_ID, "golden parent changed");
    if let Some(golden) = open_child(&parent, Path::new("golden"))? {
        if golden.metadata()?.ino() != 256 {
            ensure!(std::fs::read_dir(format!("/proc/self/fd/{}", golden.as_raw_fd()))?.next().is_none(), "ordinary golden directory is not empty");
            state.intent("golden-remove-empty-directory")?;
            fs::remove_at(parent.as_fd(), Path::new("golden"), true)?;
        }
    }
    if open_child(&parent, Path::new("golden"))?.is_none() {
        state.intent("golden-create-subvolume")?;
        subvolume::subvolume_create(parent.as_fd(), c"golden", &[])?; parent.sync_all()?;
    }
    let golden = open_child(&parent, Path::new("golden"))?.context("golden missing after creation")?;
    let info = subvolume::subvolume_info(golden.as_fd())?;
    ensure!(golden.metadata()?.ino() == 256 && info.parent_id == OPT_ID && !subvolume::subvolume_flags_get(golden.as_fd())?.contains(SubvolumeFlags::RDONLY), "golden verification failed");
    if open_child(&parent, Path::new("release-send"))?.is_none() {
        state.intent("create-release-send")?; fs::mkdir_at(parent.as_fd(), Path::new("release-send"), 0o700)?;
    }
    let send = open_child(&parent, Path::new("release-send"))?.unwrap(); check_private(&send)?;
    let root = top()?;
    for slot in [Slot::A, Slot::B] {
        if let Some(actual) = open_child(&root, Path::new(slot.root()))? {
            verify_root(&actual, state.release(slot).context("preexisting unregistered slot requires explicit bootstrap reconciliation")?)?;
        } else { ensure!(state.slots.get(&slot).and_then(Option::as_ref).is_none(), "recorded slot is missing"); }
    }
    state.journal = None; state.save()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiverPin { path: PathBuf, sha256: String, device: u64, inode: u64, uid: u32, gid: u32, mode: u32 }
fn receiver() -> Result<File> {
    let mut file = fs::secure_absolute(Path::new("/var/lib/opdbus-release/receiver.json"), libc::O_RDONLY)?;
    ensure!(file.metadata()?.mode() & 0o777 == 0o600, "receiver pin must be mode 0600");
    let pin: ReceiverPin = serde_json::from_slice(&fs::read_bounded(&mut file, 16 * 1024)?)?;
    inventory::normalized(&pin.path, true)?;
    let mut binary = fs::secure_absolute(&pin.path, libc::O_RDONLY)?;
    let m = binary.metadata()?;
    ensure!(m.is_file() && m.dev() == pin.device && m.ino() == pin.inode && m.uid() == pin.uid && m.gid() == pin.gid && m.mode() == pin.mode
        && m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0, "receiver inode/ownership/mode pin differs");
    ensure!(fs::hash_file(&mut binary)? == pin.sha256, "receiver digest pin differs");
    verify_static_receiver(&mut binary)?;
    Ok(binary)
}
fn verify_static_receiver(binary: &mut File) -> Result<()> {
    binary.seek(SeekFrom::Start(0))?;
    let mut elf = [0u8; 64]; binary.read_exact(&mut elf)?;
    ensure!(&elf[..6] == b"\x7fELF\x02\x01" && u16::from_le_bytes(elf[18..20].try_into()?) == 62,
        "receiver must be a native x86_64 little-endian ELF executable");
    let offset = u64::from_le_bytes(elf[32..40].try_into()?);
    let size = u16::from_le_bytes(elf[54..56].try_into()?) as u64;
    let count = u16::from_le_bytes(elf[56..58].try_into()?) as u64;
    ensure!(size >= 56 && count > 0 && count <= 1024 && offset.checked_add(size * count).is_some_and(|end| end <= binary.metadata().map(|m| m.len()).unwrap_or(0)),
        "receiver ELF program-header bounds are invalid");
    for index in 0..count {
        binary.seek(SeekFrom::Start(offset + index * size))?;
        let mut kind = [0; 4]; binary.read_exact(&mut kind)?;
        ensure!(u32::from_le_bytes(kind) != 3,
            "pinned receiver is dynamically linked. Upstream btrfs receive canonicalizes paths, so detached-root receive requires an approved STATIC btrfs receiver for fd-authenticated exec inside the detached top-level chroot");
    }
    Ok(())
}
fn pipe() -> Result<(File, File)> {
    let mut fds = [-1; 2];
    // SAFETY: fds is a writable two-integer array; successful pipe2 transfers ownership.
    ensure!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0, "pipe2: {}", io::Error::last_os_error());
    Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
}
fn receive(source: &File, parent_id: u64, top: &File, receiver: &File) -> Result<u64> {
    let executable = receiver.as_raw_fd(); let top_fd = top.as_raw_fd();
    let parent_pid = std::process::id() as libc::pid_t;
    let mut command = Command::new(format!("/proc/self/fd/{executable}"));
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::inherit()).process_group(0).env_clear();
    // Upstream receive realpath() cannot preserve detached /proc/fd operands.
    // A static receiver instead executes by authenticated fd after entering the
    // detached top-level root. No mount is attached in any namespace.
    // SAFETY: only stack data and async-signal-safe syscalls run between fork and exec.
    unsafe { command.pre_exec(move || {
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 || libc::getppid() != parent_pid {
            return Err(io::Error::from_raw_os_error(libc::ESRCH));
        }
        let lock_flags = libc::fcntl(9, libc::F_GETFD);
        if lock_flags < 0 || libc::fcntl(9, libc::F_SETFD, lock_flags & !libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fchdir(top_fd) < 0 || libc::chroot(c".".as_ptr()) < 0 || libc::chdir(c"/".as_ptr()) < 0 {
            return Err(io::Error::last_os_error());
        }
        let argv = [c"btrfs".as_ptr(), c"receive".as_ptr(), c"-e".as_ptr(),
            c"-m".as_ptr(), c"/".as_ptr(), c"/".as_ptr(), std::ptr::null()];
        let envp = [c"PATH=/usr/sbin:/usr/bin:/sbin:/bin".as_ptr(), c"LC_ALL=C".as_ptr(), std::ptr::null()];
        libc::fexecve(executable, argv.as_ptr(), envp.as_ptr());
        Err(io::Error::last_os_error())
    }); }
    let (reader, writer) = pipe()?;
    let mut child = command.spawn().context("execute authenticated static Btrfs receiver in detached-root chroot")?;
    let stdin = child.stdin.take().context("receiver stdin absent")?;
    let result = std::thread::scope(|scope| -> Result<u64> {
        let send = scope.spawn(move || btrfs_uapi::send_receive::send(source.as_fd(), writer.as_raw_fd(), parent_id, &mut [], btrfs_uapi::send_receive::SendFlags::empty(), 1));
        let (done_tx, done_rx) = mpsc::channel();
        let copy = scope.spawn(move || {
            let mut stdin = stdin;
            let result = io::copy(&mut reader.take((1u64 << 40) + 1), &mut stdin);
            drop(stdin); let _ = done_tx.send(()); result
        });
        let deadline = Instant::now() + Duration::from_secs(1800);
        loop {
            if done_rx.try_recv().is_ok() { break; }
            if Instant::now() >= deadline || child.try_wait()?.is_some_and(|status| !status.success()) {
                // Child::kill respects an already-reaped child; never signal a recycled PID/group.
                let _ = child.kill();
                let _ = child.wait();
                // Receiver closure breaks the copying thread, which closes the read end and unblocks send.
                let _ = copy.join(); let _ = send.join();
                bail!("send/receive failed or exceeded 1800-second bound");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let count = copy.join().map_err(|_| anyhow::anyhow!("stream copy panicked"))??;
        ensure!(count <= 1u64 << 40, "receive stream exceeded the one-TiB transaction bound");
        send.join().map_err(|_| anyhow::anyhow!("native sender panicked"))??;
        loop {
            if let Some(status) = child.try_wait()? { ensure!(status.success(), "Btrfs receiver failed: {status}"); break; }
            if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); bail!("receiver finalization timeout"); }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(count)
    });
    if result.is_err() {
        // Reap the receiver on every error; it must not outlive the transaction lock.
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
pub fn deploy(state: &mut State) -> Result<Slot> {
    ensure!(state.journal.is_none(), "transaction requires recovery");
    let slot = state.active.map_or(Slot::A, Slot::other);
    let evicted = state.slots.get(&slot).and_then(Option::as_ref).cloned();
    let top = top()?;
    match (open_child(&top, Path::new(slot.root()))?, evicted.as_ref()) {
        (Some(root), Some(record)) => verify_root(&root, record)?,
        (None, None) => {},
        _ => bail!("slot contents and durable record disagree; refusing replacement"),
    }
    // Received roots and final slots share one parent: moving a readonly
    // subvolume across parents requires MAY_WRITE and is rejected by the VFS.
    // Only our reserved staging names matter; unrelated top-level roots remain untouched.
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", top.as_raw_fd()))? {
        ensure!(!entry?.file_name().as_encoded_bytes().starts_with(b"release-"),
            "unrecorded incoming release exists; recover the previous transaction");
    }
    let receiver = receiver()?;
    let golden = fs::secure_absolute(Path::new(GOLDEN), libc::O_RDONLY | libc::O_DIRECTORY)?;
    let g = subvolume::subvolume_info(golden.as_fd())?;
    ensure!(golden.metadata()?.ino() == 256 && filesystem_info(golden.as_fd())?.uuid.to_string() == FSID && g.parent_id == OPT_ID && !subvolume::subvolume_flags_get(golden.as_fd())?.contains(SubvolumeFlags::RDONLY), "golden is not the approved writable @opt child subvolume");
    let send = fs::secure_absolute(Path::new(SEND), libc::O_RDONLY | libc::O_DIRECTORY)?;
    check_private(&send)?;
    ensure!(filesystem_info(send.as_fd())?.uuid.to_string() == FSID && subvolume::subvolume_info(send.as_fd())?.id == OPT_ID, "source snapshot parent differs");
    let parent_id = if let Some(active) = state.active {
        let parent = state.release(active)?;
        match open_child(&send, Path::new(&parent.name))? {
            Some(root) => {
                let info = subvolume::subvolume_info(root.as_fd())?;
                ensure!(info.id == parent.source_id && info.uuid.to_string() == parent.source_uuid
                    && subvolume::subvolume_flags_get(root.as_fd())?.contains(SubvolumeFlags::RDONLY),
                    "incremental source parent changed");
                info.id
            },
            None => 0,
        }
    } else { 0 };
    let name = format!("release-{}", uuid::Uuid::new_v4());
    let mut journal = Journal::new("deploy", state.active, Some(slot));
    journal.source_name = Some(name.clone()); journal.incremental = parent_id != 0; journal.evicted = evicted;
    state.journal = Some(journal); state.intent("snapshot-create")?;
    let started = Instant::now();
    subvolume::snapshot_create(send.as_fd(), golden.as_fd(), &CString::new(name.as_str())?, true, &[])?;
    send.sync_all()?;
    let source = open_child(&send, Path::new(&name))?.context("source snapshot absent")?;
    let source_info = subvolume::subvolume_info(source.as_fd())?;
    { let j = state.journal.as_mut().unwrap(); j.source_uuid = Some(source_info.uuid.to_string()); j.timings_ms.insert("snapshot".into(), started.elapsed().as_millis()); }
    state.intent("send-receive")?;
    let started = Instant::now(); let bytes = receive(&source, parent_id, &top, &receiver)?;
    { let j = state.journal.as_mut().unwrap(); j.stream_bytes = bytes; j.timings_ms.insert("send_receive".into(), started.elapsed().as_millis()); }
    state.intent("verify-receive")?;
    let started = Instant::now();
    let received = open_child(&top, Path::new(&name))?.context("received subvolume absent")?;
    let info = subvolume::subvolume_info(received.as_fd())?;
    ensure!(info.received_uuid == source_info.uuid && subvolume::subvolume_flags_get(received.as_fd())?.contains(SubvolumeFlags::RDONLY) && received.metadata()?.ino() == 256, "received UUID/read-only root mismatch");
    let source_inventory = inventory::load(&source)?; let received_inventory = inventory::load(&received)?;
    ensure!(source_inventory.tree_sha256 == received_inventory.tree_sha256 && source_inventory.graph_sha256 == received_inventory.graph_sha256, "received tree differs from immutable source snapshot");
    let release = Release { name, id: info.id, native_uuid: info.uuid.to_string(), received_uuid: info.received_uuid.to_string(), source_id: source_info.id, source_uuid: source_info.uuid.to_string(),
        graph_sha256: received_inventory.graph_sha256, tree_sha256: received_inventory.tree_sha256, catalog_sha256: received_inventory.catalog_sha256,
        targets: received_inventory.targets, devices: [received.metadata()?.dev()].into() };
    { let j = state.journal.as_mut().unwrap(); j.candidate = Some(release); j.timings_ms.insert("fingerprint".into(), started.elapsed().as_millis()); }
    state.intent("receive-verified")?;
    drop(received); drop(source);
    install_received(state)?;
    Ok(slot)
}
pub fn install_received(state: &mut State) -> Result<()> {
    let j = state.journal.as_ref().context("missing transfer journal")?;
    ensure!(!j.aborting, "transfer cleanup is already committed");
    let slot = j.to.context("missing destination slot")?;
    ensure!(Some(slot) != state.active, "active slot cannot be replaced");
    let release = j.candidate.clone().context("incoming has not been verified")?;
    let evicted = j.evicted.clone();
    let top = top()?;
    // Reverify the complete incoming tree before authorizing any occupied deletion.
    // A crash after rename is reconciled against the exact installed native UUID.
    let installed = {
        let root = open_child(&top, Path::new(slot.root()))?;
        if let Some(root) = root {
            let info = subvolume::subvolume_info(root.as_fd())?;
            if info.id == release.id && info.uuid.to_string() == release.native_uuid {
                verify_root(&root, &release)?;
                ensure!(inventory::load(&root)?.tree_sha256 == release.tree_sha256, "installed receive fingerprint mismatch");
                true
            } else {
                verify_root(&root, evicted.as_ref().context("unregistered occupied slot")?)?;
                false
            }
        } else { false }
    };
    if !installed {
        let root = open_child(&top, Path::new(&release.name))?.context("verified incoming missing; occupied slot will not be touched")?;
        verify_root(&root, &release)?;
        let observed = inventory::load(&root)?;
        ensure!(observed.tree_sha256 == release.tree_sha256 && observed.graph_sha256 == release.graph_sha256,
            "verified incoming fingerprint mismatch");
        drop(root);
    }
    if let Some(old) = &evicted {
        let started = Instant::now();
        // The actual kernel operation closes this exact generation's admission and
        // rejects surviving aliases/references atomically with unlink. No proc scan,
        // umount result, guard toggle, or userspace token substitutes for that check.
        remove_root(state, &top, RootRemoval {
            id: old.id, native_uuid: old.native_uuid.clone(), received_uuid: old.received_uuid.clone(),
            parent: "slot".into(), name: slot.root().into(), cleaned: false, rejected: false,
        })?;
        state.slots.insert(slot, None);
        if state.previous == Some(slot) { state.previous = None; }
        state.intent("occupied-slot-cleaned")?;
        remove_root(state, &top, RootRemoval {
            id: old.source_id, native_uuid: old.source_uuid.clone(), received_uuid: uuid::Uuid::nil().to_string(),
            parent: "source".into(), name: old.name.clone(), cleaned: false, rejected: false,
        })?;
        state.journal.as_mut().unwrap().timings_ms.insert("reuse_cleanup".into(), started.elapsed().as_millis());
    }
    if !installed {
        ensure!(open_child(&top, Path::new(slot.root()))?.is_none(), "slot reappeared before no-replace installation");
        state.intent("install-received-slot")?;
        fs::rename_at(top.as_fd(), Path::new(&release.name), top.as_fd(), Path::new(slot.root()))
            .context("install verified receive via same-parent no-replace rename")?;
    }
    state.slots.insert(slot, Some(release));
    if state.previous == Some(slot) { state.previous = None; }
    let j = state.journal.take().unwrap();
    state.last_timings_ms = j.timings_ms.clone(); state.last_stream_bytes = j.stream_bytes;
    if let Err(error) = state.save() { state.journal = Some(j); return Err(error).context("slot commit failed; receive remains recoverable"); }
    Ok(())
}
pub fn recover_transfer(state: &mut State) -> Result<()> {
    if state.journal.as_ref().is_some_and(|j| j.aborting) { return abort_transfer(state); }
    if state.journal.as_ref().and_then(|j| j.candidate.as_ref()).is_some() { return install_received(state); }
    let journal = state.journal.as_ref().context("missing receive journal")?.clone();
    let name = journal.source_name.context("receive journal has no source snapshot name")?;
    let top = top()?;
    if let Some(received) = open_child(&top, Path::new(&name))? {
        let info = subvolume::subvolume_info(received.as_fd())?;
        if subvolume::subvolume_flags_get(received.as_fd())?.contains(SubvolumeFlags::RDONLY) && !info.received_uuid.is_nil() {
            let send = fs::secure_absolute(Path::new(SEND), libc::O_RDONLY | libc::O_DIRECTORY)?;
            let source = open_child(&send, Path::new(&name))?.context("received snapshot has no source")?;
            let source_info = subvolume::subvolume_info(source.as_fd())?;
            ensure!(subvolume::subvolume_flags_get(source.as_fd())?.contains(SubvolumeFlags::RDONLY) && info.received_uuid == source_info.uuid,
                "interrupted receive source identity differs");
            if let Some(expected) = journal.source_uuid {
                ensure!(expected == source_info.uuid.to_string(), "journal source UUID mismatch");
            }
            let original = inventory::load(&source)?;
            let observed = inventory::load(&received)?;
            ensure!(original.tree_sha256 == observed.tree_sha256 && original.graph_sha256 == observed.graph_sha256,
                "interrupted receive fingerprint mismatch");
            state.journal.as_mut().unwrap().candidate = Some(Release {
                name, id: info.id, native_uuid: info.uuid.to_string(), received_uuid: info.received_uuid.to_string(),
                source_id: source_info.id, source_uuid: source_info.uuid.to_string(),
                graph_sha256: observed.graph_sha256, tree_sha256: observed.tree_sha256,
                catalog_sha256: observed.catalog_sha256, targets: observed.targets,
                devices: [received.metadata()?.dev()].into(),
            });
            state.intent("recovered-verified-receive")?;
            drop(received); drop(source);
            return install_received(state);
        }
    }
    abort_transfer(state)
}

fn removal_parent(top: &File, kind: &str) -> Result<File> {
    match kind {
        "slot" | "incoming" => fs::directory(top.as_fd()),
        "source" | "source-orphaned" => {
            let parent = fs::secure_absolute(reset_source_path(kind)?, libc::O_RDONLY | libc::O_DIRECTORY)?;
            check_private(&parent)?;
            ensure!(filesystem_info(parent.as_fd())?.uuid.to_string() == FSID
                && subvolume::subvolume_info(parent.as_fd())?.id == OPT_ID, "source parent identity changed");
            Ok(parent)
        },
        _ => bail!("unknown removal parent"),
    }
}

fn remove_root(state: &mut State, top: &File, removal: RootRemoval) -> Result<()> {
    inventory::normalized(Path::new(&removal.name), false)?;
    ensure!(Path::new(&removal.name).components().count() == 1 && removal.id > OPT_ID,
        "invalid cleanup root identity/name");
    if let Some(active) = state.active {
        let active = state.release(active)?;
        ensure!(removal.id != active.id && removal.id != active.source_id, "cannot remove active backing/source root");
    }
    let journal = state.journal.as_mut().context("cleanup without journal")?;
    ensure!(removal.parent != "source-orphaned" || journal.operation == "reset",
        "historical source parent requires explicit native reset admission");
    if journal.operation == "reset" {
        ensure!(journal.reset_phase == Some(ResetPhase::Retiring)
            && journal.to.is_none() && journal.candidate.is_none() && !journal.aborting,
            "reset deletion outside its forward-only retirement phase");
        ensure!(journal.removals.iter().any(|r| r.id == removal.id
            && r.native_uuid == removal.native_uuid && r.received_uuid == removal.received_uuid
            && r.parent == removal.parent && r.name == removal.name),
            "reset deletion was not admitted in the complete identity plan");
    } else if !journal.aborting {
        let candidate = journal.candidate.as_ref().context("cleanup before incoming verification")?;
        ensure!(removal.id != candidate.id && removal.id != candidate.source_id, "cannot remove incoming candidate during installation");
    }
    let index = if let Some(index) = journal.removals.iter().position(|r| r.id == removal.id) {
        let prior = &journal.removals[index];
        ensure!(prior.native_uuid == removal.native_uuid && prior.received_uuid == removal.received_uuid
            && prior.parent == removal.parent && prior.name == removal.name, "cleanup identity changed");
        if prior.cleaned { return Ok(()); }
        index
    } else {
        journal.removals.push(removal.clone()); journal.removals.len() - 1
    };
    state.intent(format!("cleanup-root-recorded:{}", removal.id))?;
    let parent = removal_parent(top, &removal.parent)?;
    if let Some(root) = open_child(&parent, Path::new(&removal.name))? {
        let actual = subvolume::subvolume_info(root.as_fd())?;
        ensure!(root.metadata()?.ino() == 256 && filesystem_info(root.as_fd())?.uuid.to_string() == FSID
            && actual.id == removal.id && actual.uuid.to_string() == removal.native_uuid
            && actual.received_uuid.to_string() == removal.received_uuid,
            "cleanup name no longer denotes the exact recorded root");
        if !subvolume::subvolume_flags_get(root.as_fd())?.contains(SubvolumeFlags::RDONLY) {
            ensure!(removal.parent == "incoming" && state.journal.as_ref().unwrap().aborting,
                "previously immutable cleanup root is writable");
            state.intent(format!("freeze-partial-incoming:{}", removal.id))?;
            subvolume::subvolume_flags_set(root.as_fd(), SubvolumeFlags::RDONLY)?;
            root.sync_all()?;
        }
        // No target-root fd may survive into the zero-reference check.
        drop(root);
    }
    drop(parent);
    state.journal.as_mut().unwrap().removals[index].rejected = false;
    state.intent(format!("quiescent-delete:{}", removal.id))?;
    match lifetime::destroy(top.as_fd(), FSID.parse()?, removal.id,
        removal.native_uuid.parse()?, removal.received_uuid.parse()?) {
        Ok(()) => {},
        // Only ENOENT returned by this exact checked ABI proves absence across
        // the native lifetime registry, including orphaned objects. Replaying a
        // successful unlink after a crash therefore remains resumable; a missing
        // pathname or ordinary subvolume lookup is never accepted as proof.
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {},
        Err(error) => {
            if matches!(error.raw_os_error(), Some(libc::EBUSY | libc::ESTALE | libc::EINVAL | libc::EROFS | libc::ENOTTY)) {
                state.journal.as_mut().unwrap().removals[index].rejected = true;
                state.save()?;
            }
            return Err(error).with_context(|| format!(
                "exact-generation checked-delete failed for root {}; generic errors may follow unlink, so recovery must reconcile actual roots; no ordinary-delete fallback; approved running kernel support and zero live references are required",
                removal.id));
        },
    }
    state.intent(format!("wait-root-cleaner:{}", removal.id))?;
    btrfs_uapi::filesystem::sync(top.as_fd())?;
    subvolume::subvol_sync_wait_one(top.as_fd(), removal.id)?;
    let parent = removal_parent(top, &removal.parent)?;
    ensure!(open_child(&parent, Path::new(&removal.name))?.is_none(),
        "cleanup name remains or was reused after cleaner synchronization");
    state.journal.as_mut().unwrap().removals[index].cleaned = true;
    state.intent(format!("root-cleaned:{}", removal.id))
}

/// Explicit operator recovery for a completed receive whose journal was lost.
/// Admission records exact identities; all deletion stays on the normal
/// journaled, kernel-quiescent abort path.
pub fn abort_orphan(
    state: &mut State, name: &str, received_id: u64, received_uuid: uuid::Uuid,
    source_id: u64, source_uuid: uuid::Uuid,
) -> Result<()> {
    ensure!(state.journal.is_none(), "orphan admission requires no pending transaction");
    let suffix = name.strip_prefix("release-").context("not a reserved incoming release name")?;
    uuid::Uuid::parse_str(suffix).context("invalid incoming release name")?;
    ensure!(received_id > OPT_ID && source_id > OPT_ID && received_id != source_id,
        "invalid orphan root IDs");
    let active = state.active.context("orphan cleanup requires a retained active release")?;
    let slot = active.other();
    ensure!(state.slots.get(&slot).and_then(Option::as_ref).is_none(),
        "orphan cleanup requires an empty inactive slot");
    for retained in state.slots.values().flatten() {
        ensure!(retained.name != name
            && ![retained.id, retained.source_id].contains(&received_id)
            && ![retained.id, retained.source_id].contains(&source_id),
            "orphan request aliases a retained release");
    }
    let candidate = {
        let top = top()?;
        let retained = open_child(&top, Path::new(active.root()))?.context("active backing is missing")?;
        verify_root(&retained, state.release(active)?)?;
        ensure!(open_child(&top, Path::new(slot.root()))?.is_none(),
            "unrecorded inactive slot exists; refusing orphan cleanup");
        let send = removal_parent(&top, "source")?;
        let source = open_child(&send, Path::new(name))?.context("orphan source is missing")?;
        let source_info = subvolume::subvolume_info(source.as_fd())?;
        ensure!(source.metadata()?.ino() == 256
            && filesystem_info(source.as_fd())?.uuid.to_string() == FSID
            && source_info.parent_id == OPT_ID && source_info.id == source_id
            && source_info.uuid == source_uuid && source_info.received_uuid.is_nil()
            && subvolume::subvolume_flags_get(source.as_fd())?.contains(SubvolumeFlags::RDONLY),
            "orphan source does not match the explicit read-only identity");
        let received = open_child(&top, Path::new(name))?.context("orphan receive is missing")?;
        let info = subvolume::subvolume_info(received.as_fd())?;
        ensure!(received.metadata()?.ino() == 256
            && filesystem_info(received.as_fd())?.uuid.to_string() == FSID
            && info.parent_id == 5 && info.id == received_id && info.uuid == received_uuid
            && info.received_uuid == source_uuid
            && subvolume::subvolume_flags_get(received.as_fd())?.contains(SubvolumeFlags::RDONLY),
            "orphan receive does not match the explicit read-only identity");
        let original = inventory::load(&source)?;
        let observed = inventory::load(&received)?;
        ensure!(original.tree_sha256 == observed.tree_sha256
            && original.graph_sha256 == observed.graph_sha256
            && original.catalog_sha256 == observed.catalog_sha256,
            "orphan source and receive inventories differ");
        Release {
            name: name.into(), id: info.id, native_uuid: info.uuid.to_string(),
            received_uuid: info.received_uuid.to_string(), source_id: source_info.id,
            source_uuid: source_info.uuid.to_string(), graph_sha256: observed.graph_sha256,
            tree_sha256: observed.tree_sha256, catalog_sha256: observed.catalog_sha256,
            targets: observed.targets, devices: [received.metadata()?.dev()].into(),
        }
    }; // Drop every target-root descriptor before the kernel reference census.
    let mut journal = Journal::new("deploy", state.active, Some(slot));
    journal.source_name = Some(candidate.name.clone());
    journal.source_uuid = Some(candidate.source_uuid.clone());
    journal.candidate = Some(candidate);
    journal.aborting = true;
    state.journal = Some(journal);
    state.intent("admit-verified-orphan-abort")?;
    abort_transfer(state)
}

pub fn abort_transfer(state: &mut State) -> Result<()> {
    let journal = state.journal.as_ref().context("no transfer journal")?.clone();
    ensure!(journal.operation == "deploy", "not a transfer transaction");
    let top = top()?;
    let slot = journal.to.context("transfer slot absent")?;
    ensure!(Some(slot) != state.active, "cannot abort an active slot");
    if let Some(old) = &journal.evicted {
        ensure!(journal.removals.iter().all(|r| r.id != old.id || (!r.cleaned && r.rejected)),
            "occupied deletion may already have begun; abort cannot restore that generation. Reconcile the verified incoming with a non-abort release invocation");
        let old_root = open_child(&top, Path::new(slot.root()))?.context("previous backing is already absent; complete verified-receive recovery instead")?;
        verify_root(&old_root, old)?;
    }
    state.journal.as_mut().unwrap().aborting = true;
    state.intent("abort-transfer")?;
    let name = journal.source_name.as_deref().context("transfer source name absent")?;
    // The receiver inherits fd9 without CLOEXEC and holds its flock through exit,
    // preventing cleanup while a crashed parent's receiver still has access.
    // The live receive path kills/reaps its child on every error.
    let source_record = {
        let send = removal_parent(&top, "source")?;
        if let Some(root) = open_child(&send, Path::new(name))? {
            let info = subvolume::subvolume_info(root.as_fd())?;
            ensure!(root.metadata()?.ino() == 256 && info.parent_id == OPT_ID && info.received_uuid.is_nil(),
                "unexpected source snapshot in abort");
            if let Some(expected) = &journal.source_uuid { ensure!(&info.uuid.to_string() == expected, "abort source UUID changed"); }
            if let Some(candidate) = &journal.candidate {
                ensure!(info.id == candidate.source_id && info.uuid.to_string() == candidate.source_uuid,
                    "abort source root differs from the verified candidate");
            }
            Some(RootRemoval { id: info.id, native_uuid: info.uuid.to_string(), received_uuid: info.received_uuid.to_string(),
                parent: "source".into(), name: name.into(), cleaned: false, rejected: false })
        } else { None }
    };
    let received_record = {
        let parent = removal_parent(&top, "incoming")?;
        for entry in std::fs::read_dir(format!("/proc/self/fd/{}", parent.as_raw_fd()))? {
            let entry = entry?;
            if entry.file_name().as_encoded_bytes().starts_with(b"release-") {
                ensure!(entry.file_name() == std::ffi::OsStr::new(name), "unexpected incoming entry; refusing broad cleanup");
            }
        }
        let located = if let Some(root) = open_child(&parent, Path::new(name))? {
            Some((root, "incoming", name))
        } else if journal.evicted.is_none() {
            open_child(&top, Path::new(slot.root()))?.map(|root| (root, "slot", slot.root()))
        } else { None };
        if let Some((root, parent, name)) = located {
            let info = subvolume::subvolume_info(root.as_fd())?;
            ensure!(root.metadata()?.ino() == 256 && info.parent_id == 5, "abort target is not a receive root");
            if let Some(candidate) = &journal.candidate { verify_root(&root, candidate)?; }
            else {
                ensure!(parent == "incoming", "unverified receive escaped incoming");
                if !info.received_uuid.is_nil() {
                    ensure!(source_record.as_ref().is_some_and(|r| r.native_uuid == info.received_uuid.to_string()), "abort receive UUID differs from source");
                }
            }
            Some(RootRemoval { id: info.id, native_uuid: info.uuid.to_string(), received_uuid: info.received_uuid.to_string(),
                parent: parent.into(), name: name.into(), cleaned: false, rejected: false })
        } else { None }
    };
    // Persist both identities before cleanup, including the snapshot-create
    // crash window before source_uuid was recorded.
    for record in [received_record, source_record].into_iter().flatten() {
        if !state.journal.as_ref().unwrap().removals.iter().any(|r| r.id == record.id) {
            state.journal.as_mut().unwrap().removals.push(record);
        }
    }
    state.intent("abort-root-identities-recorded")?;
    let removals = state.journal.as_ref().unwrap().removals.clone();
    for record in removals {
        if journal.evicted.as_ref().is_some_and(|old| record.id == old.id || record.id == old.source_id) { continue; }
        remove_root(state, &top, record)?;
    }
    state.slots.insert(slot, journal.evicted);
    let pending = state.journal.take().unwrap();
    if let Err(error) = state.save() { state.journal = Some(pending); return Err(error); }
    super::boot::complete(state)
}

fn reset_source_path(kind: &str) -> Result<&'static Path> {
    match kind {
        "source" => Ok(Path::new(SEND)),
        "source-orphaned" => Ok(Path::new(SEND_ORPHANED)),
        _ => bail!("reset source parent is outside the two approved directories"),
    }
}

fn reset_source_parent(kind: &str) -> Result<Option<File>> {
    let parent = match fs::secure_absolute(reset_source_path(kind)?, libc::O_RDONLY | libc::O_DIRECTORY) {
        Ok(parent) => parent,
        Err(error) if error.downcast_ref::<io::Error>().is_some_and(|e| e.kind() == io::ErrorKind::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    check_private(&parent)?;
    ensure!(filesystem_info(parent.as_fd())?.uuid.to_string() == FSID
        && subvolume::subvolume_info(parent.as_fd())?.id == OPT_ID, "reset source parent identity changed");
    Ok(Some(parent))
}

fn reset_release_name(name: &str) -> Result<()> {
    inventory::normalized(Path::new(name), false)?;
    ensure!(Path::new(name).components().count() == 1, "reset release name is not a basename");
    uuid::Uuid::parse_str(name.strip_prefix("release-").context("not a reserved release artifact name")?)?;
    Ok(())
}

fn reset_source_record(source: &ResetSource) -> Result<RootRemoval> {
    reset_source_path(&source.parent)?;
    reset_release_name(&source.name)?;
    Ok(RootRemoval { id: source.id, native_uuid: source.native_uuid.parse::<uuid::Uuid>()?.to_string(),
        received_uuid: uuid::Uuid::nil().to_string(), parent: source.parent.clone(), name: source.name.clone(),
        cleaned: false, rejected: false })
}

fn reset_records(plan: &ResetPlan) -> Result<Vec<RootRemoval>> {
    ensure!(plan.version == 1 && plan.received.len() + plan.sources.len() <= 128,
        "unsupported or oversized native reset plan");
    let mut records = Vec::with_capacity(plan.received.len() * 2 + plan.sources.len());
    let mut slots = std::collections::BTreeSet::new();
    for received in &plan.received {
        let parent = if let Some(slot) = received.slot {
            ensure!(slots.insert(slot) && received.name == slot.root(), "reset slot name/uniqueness differs");
            "slot"
        } else {
            reset_release_name(&received.name)?;
            ensure!(received.name == received.source.name, "incoming/source basenames differ");
            "incoming"
        };
        let native: uuid::Uuid = received.native_uuid.parse()?;
        let source: uuid::Uuid = received.source.native_uuid.parse()?;
        ensure!(received.received_uuid.parse::<uuid::Uuid>()? == source,
            "reset received UUID is not its explicitly paired source UUID");
        records.push(RootRemoval { id: received.id, native_uuid: native.to_string(), received_uuid: source.to_string(),
            parent: parent.into(), name: received.name.clone(), cleaned: false, rejected: false });
        records.push(reset_source_record(&received.source)?);
    }
    for source in &plan.sources { records.push(reset_source_record(source)?); }
    ensure!(plan.mounted.is_none_or(|slot| slots.contains(&slot)), "mounted reset owner is not an admitted slot");
    for (name, hash) in &plan.catalogs {
        ensure!(matches!(name.as_str(), "persisted" | "shm")
            && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid exact reset catalog name/digest");
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut uuids = std::collections::BTreeSet::new();
    let mut locations = std::collections::BTreeSet::new();
    for record in &records {
        let native: uuid::Uuid = record.native_uuid.parse()?;
        ensure!(record.id > OPT_ID && ids.insert(record.id) && !native.is_nil() && uuids.insert(native)
            && locations.insert((record.parent.as_str(), record.name.as_str())),
            "invalid or aliased native reset identity/location");
    }
    Ok(records)
}

fn verify_reset_root(root: &File, record: &RootRemoval) -> Result<()> {
    let parent_id = match record.parent.as_str() {
        "slot" | "incoming" => 5,
        "source" | "source-orphaned" => OPT_ID,
        _ => bail!("invalid native reset root parent"),
    };
    let info = subvolume::subvolume_info(root.as_fd())?;
    ensure!(root.metadata()?.ino() == 256 && filesystem_info(root.as_fd())?.uuid.to_string() == FSID
        && info.parent_id == parent_id && info.id == record.id
        && info.uuid.to_string() == record.native_uuid && info.received_uuid.to_string() == record.received_uuid
        && subvolume::subvolume_flags_get(root.as_fd())?.contains(SubvolumeFlags::RDONLY),
        "reset name does not denote its exact approved immutable native root: {}", record.name);
    Ok(())
}

fn check_reset_namespace(top: &File, records: &[RootRemoval], require_present: bool) -> Result<()> {
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", top.as_raw_fd()))? {
        let name = entry?.file_name();
        if name.as_encoded_bytes().starts_with(b"release-") || name.as_encoded_bytes().starts_with(b"@release-") {
            ensure!(records.iter().any(|r| matches!(r.parent.as_str(), "slot" | "incoming")
                && name == std::ffi::OsStr::new(&r.name)), "reserved release root is absent from the explicit reset plan");
        }
    }
    let parents = [("source", reset_source_parent("source")?), ("source-orphaned", reset_source_parent("source-orphaned")?)];
    for (kind, parent) in &parents {
        if let Some(parent) = parent {
            for entry in std::fs::read_dir(format!("/proc/self/fd/{}", parent.as_raw_fd()))? {
                let name = entry?.file_name();
                // Historical source parents can also contain identity/guest
                // inputs. Non-release names are outside reset authority and are
                // not traversed, adopted, renamed or deleted.
                if name.as_encoded_bytes().starts_with(b"release-") {
                    ensure!(records.iter().any(|r| r.parent == *kind && name == std::ffi::OsStr::new(&r.name)),
                        "reserved release source is absent from the explicit reset plan");
                }
            }
        }
    }
    for record in records {
        let parent = if matches!(record.parent.as_str(), "slot" | "incoming") { top } else {
            parents.iter().find(|(kind,_)| *kind == record.parent)
                .and_then(|(_,parent)| parent.as_ref()).context("approved reset source parent is missing")?
        };
        if let Some(root) = open_child(parent, Path::new(&record.name))? {
            ensure!(!record.cleaned, "a cleaned reset name has reappeared: {}", record.name);
            verify_reset_root(&root, record)?;
        } else { ensure!(!require_present, "approved reset root is absent before admission: {}", record.name); }
    }
    Ok(())
}

pub(super) struct ResetAdmission {
    pub plan: ResetPlan, pub removals: Vec<RootRemoval>,
    pub releases: std::collections::BTreeMap<Slot, Release>, pub inventories: Vec<inventory::Inventory>,
}

/// Read explicit native authority; slots.json is deliberately not an input.
pub(super) fn plan_reset() -> Result<ResetAdmission> {
    let mut approval = fs::secure_absolute(Path::new(RESET_PLAN), libc::O_RDONLY)?;
    let metadata = approval.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.gid() == 0
        && metadata.mode() & 0o7777 == 0o600 && metadata.nlink() == 1,
        "native reset plan must be a root:root 0600 single-link regular file");
    let plan: ResetPlan = serde_json::from_slice(&fs::read_bounded(&mut approval, 1024 * 1024)?)?;
    let records = reset_records(&plan)?;
    let top = top()?;
    check_reset_namespace(&top, &records, true)?;
    let mut inventories = Vec::with_capacity(plan.received.len() + plan.sources.len());
    let mut releases = std::collections::BTreeMap::new();
    for received in &plan.received {
        let record = records.iter().find(|r| r.id == received.id).context("reset receive record missing")?;
        let root = open_child(&top, Path::new(&record.name))?.context("approved received root disappeared")?;
        verify_reset_root(&root, record)?;
        let observed = inventory::load(&root)?;
        let source_record = records.iter().find(|r| r.id == received.source.id).context("reset source record missing")?;
        let parent = removal_parent(&top, &source_record.parent)?;
        let source = open_child(&parent, Path::new(&source_record.name))?.context("approved source root disappeared")?;
        verify_reset_root(&source, source_record)?;
        let original = inventory::load(&source)?;
        ensure!(observed.graph_sha256 == original.graph_sha256 && observed.tree_sha256 == original.tree_sha256
            && observed.catalog_sha256 == original.catalog_sha256, "native reset source/received inventories differ");
        if let Some(slot) = received.slot {
            releases.insert(slot, Release { name: source_record.name.clone(), id: record.id,
                native_uuid: record.native_uuid.clone(), received_uuid: record.received_uuid.clone(),
                source_id: source_record.id, source_uuid: source_record.native_uuid.clone(),
                graph_sha256: observed.graph_sha256.clone(), tree_sha256: observed.tree_sha256.clone(),
                catalog_sha256: observed.catalog_sha256.clone(), targets: observed.targets.clone(),
                devices: [root.metadata()?.dev()].into() });
        }
        inventories.push(observed);
    }
    for source in &plan.sources {
        let record = records.iter().find(|r| r.id == source.id).context("standalone source record missing")?;
        let parent = removal_parent(&top, &record.parent)?;
        let root = open_child(&parent, Path::new(&record.name))?.context("approved standalone source disappeared")?;
        verify_reset_root(&root, record)?;
        inventories.push(inventory::load(&root)?);
    }
    Ok(ResetAdmission { plan, removals: records, releases, inventories })
}

pub(super) fn check_reset_roots(state: &State) -> Result<()> {
    let journal = state.journal.as_ref().context("reset root check without journal")?;
    ensure!(journal.operation == "reset", "not a reset root plan");
    let plan = journal.reset_plan.as_ref().context("reset has no explicit native admission plan")?;
    let expected = reset_records(plan)?;
    ensure!(expected.len() == journal.removals.len() && expected.iter().all(|r| journal.removals.iter().any(|known|
        known.id == r.id && known.native_uuid == r.native_uuid && known.received_uuid == r.received_uuid
            && known.parent == r.parent && known.name == r.name)), "reset removals differ from the admitted native plan");
    ensure!(journal.from == plan.mounted
        && journal.reset_releases.len() == plan.received.iter().filter(|r| r.slot.is_some()).count(),
        "reset native mount owner set differs from admission");
    for received in &plan.received {
        if let Some(slot) = received.slot {
            let release = journal.reset_releases.get(&slot).context("admitted native slot receipt missing")?;
            ensure!(release.id == received.id
                && release.native_uuid.parse::<uuid::Uuid>()? == received.native_uuid.parse::<uuid::Uuid>()?
                && release.received_uuid.parse::<uuid::Uuid>()? == received.received_uuid.parse::<uuid::Uuid>()?
                && release.source_id == received.source.id
                && release.source_uuid.parse::<uuid::Uuid>()? == received.source.native_uuid.parse::<uuid::Uuid>()?
                && release.name == received.source.name, "native reset slot receipt differs from admission");
        }
    }
    check_reset_namespace(&top()?, &journal.removals, false)
}

pub(super) fn retire_reset_roots(state: &mut State) -> Result<()> {
    check_reset_roots(state)?;
    ensure!(state.active.is_none() && state.previous.is_none(), "reset still selects a release");
    let records = state.journal.as_ref().unwrap().removals.clone();
    let top = top()?;
    for record in records { remove_root(state, &top, record)?; }
    check_reset_roots(state)
}
