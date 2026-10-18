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
        "source" => {
            let parent = fs::secure_absolute(Path::new(SEND), libc::O_RDONLY | libc::O_DIRECTORY)?;
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
    if !journal.aborting {
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
        // ENOENT from a linked-root lookup is not lifetime-zero evidence:
        // orphaned roots can still retain detached aliases. Only a successful
        // checked operation authorizes cleaner/rename progression.
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
