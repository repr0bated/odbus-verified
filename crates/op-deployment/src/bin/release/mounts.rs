use anyhow::{Context, Result, ensure};
use std::{fs::File, os::{fd::{AsFd, OwnedFd}, unix::fs::MetadataExt}, path::{Path, PathBuf}, time::Instant};
use super::{fs, inventory, model::*};

#[derive(Debug, serde::Serialize)]
pub struct Mount { pub id: u64, pub parent: u64, pub path: PathBuf, pub kind: String, pub source: String, pub options: String, pub propagation: Vec<String> }
fn unescape(s: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(s.len()); let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b != b'\\' { bytes.push(b); continue; }
        let a = iter.next().context("truncated mount escape")?; let b = iter.next().context("truncated mount escape")?; let c = iter.next().context("truncated mount escape")?;
        ensure!([a,b,c].iter().all(|c| (b'0'..=b'7').contains(c)), "invalid mount escape");
        bytes.push((a-b'0')*64+(b-b'0')*8+c-b'0');
    }
    Ok(String::from_utf8(bytes)?)
}
pub fn table() -> Result<Vec<Mount>> {
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mut result = Vec::new();
    for line in text.lines() {
        let (left,right) = line.split_once(" - ").context("invalid mountinfo")?;
        let a: Vec<_> = left.split_whitespace().collect(); let b: Vec<_> = right.split_whitespace().collect();
        ensure!(a.len() >= 6 && b.len() >= 3, "short mountinfo record");
        result.push(Mount { id: a[0].parse()?, parent: a[1].parse()?, path: PathBuf::from(unescape(a[4])?), kind: b[0].into(), source: unescape(b[1])?, options: format!("{},{}", a[5], b[2]), propagation: a[6..].iter().map(|s| (*s).into()).collect() });
    }
    Ok(result)
}
pub fn parent(target: &Path) -> Result<Parent> {
    let file = fs::secure_absolute(target.parent().context("target has no parent")?, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let id = fs::mount_id(file.as_fd())?;
    let mounts = table()?; let m = mounts.iter().find(|m| m.id == id).context("target parent mount absent from host namespace")?;
    ensure!(m.propagation.iter().all(|f| !f.starts_with("shared:") && !f.starts_with("master:")), "target parent propagation is not private");
    Ok(Parent { fsid: btrfs_uapi::filesystem::filesystem_info(file.as_fd())?.uuid.to_string(), subvolume: btrfs_uapi::subvolume::subvolume_info(file.as_fd())?.id,
        device: file.metadata()?.dev(), mount_id: id })
}
pub fn verify_parent(target: &Target) -> Result<Parent> {
    let actual = parent(&target.path)?; let expected = target.parent.as_ref().context("target has no registered parent identity")?;
    ensure!(actual.fsid == expected.fsid && actual.subvolume == expected.subvolume, "target parent filesystem/subvolume differs: {}", target.path.display());
    Ok(actual)
}
pub fn visible(target: &Target) -> Result<Option<String>> {
    let fd = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
    let id = fs::mount_id(fd.as_fd())?; let mounts = table()?;
    let m = mounts.iter().find(|m| m.id == id).context("target mount absent from host namespace")?;
    if m.path != target.path { return Ok(None); }
    ensure!(m.kind == "overlay" && m.source.starts_with("opdbus-release:"), "foreign mount covers target {}", target.path.display());
    ensure!(m.options.split(',').any(|o| o == "ro") && !m.options.contains("upperdir=") && !m.options.contains("workdir="), "release target is not a read-only lower-only overlay");
    Ok(Some(m.source.clone()))
}
pub struct Candidate { pub index: usize, pub mount: OwnedFd }
pub fn build(state: &mut State, slot: Slot) -> Result<Vec<Candidate>> {
    let release = state.release(slot)?.clone(); let root = state.root(slot)?;
    let inventory = inventory::load(&root)?;
    ensure!(inventory.graph_sha256 == release.graph_sha256 && inventory.tree_sha256 == release.tree_sha256, "slot inventory differs from received record");
    ensure!(std::fs::read_to_string("/proc/filesystems")?.lines().any(|l| l.split_whitespace().last() == Some("overlay")), "overlay module is not loaded; explicit base installation approval is required");
    let started = Instant::now(); let mut candidates = Vec::with_capacity(release.targets.len());
    let base = fs::release_base()?;
    for (index, target) in release.targets.iter().enumerate() {
        verify_parent(target)?;
        let lower = fs::open_beneath(root.as_fd(), &target.source, libc::O_RDONLY | libc::O_DIRECTORY)?;
        ensure!(lower.metadata()?.dev() == root.metadata()?.dev() && btrfs_uapi::subvolume::subvolume_info(lower.as_fd())?.id == release.id, "lower escaped verified slot");
        state.intent(format!("build-overlay:{index}:{}", release.tag(slot,index)))?;
        let mount = fs::detached_overlay(lower.as_fd(), base.as_fd(), &release.tag(slot,index), target.noexec)?;
        let device = fs::directory(mount.as_fd())?.metadata()?.dev();
        state.slots.get_mut(&slot).and_then(Option::as_mut).unwrap().devices.insert(device);
        state.save()?; candidates.push(Candidate { index, mount });
    }
    state.journal.as_mut().unwrap().timings_ms.insert("overlay_build".into(), started.elapsed().as_millis()); state.save()?;
    Ok(candidates)
}
fn detach_old(state: &mut State, old: &File, index: usize) -> Result<()> {
    state.intent(format!("detach-old-top:{index}"))?;
    if let Err(first) = fs::detach(old.as_fd()) {
        if let Err(second) = fs::detach(old.as_fd()) {
            state.journal.as_mut().unwrap().stacked.insert(index);
            state.journal.as_mut().unwrap().error = Some(format!("detach target {index}: {first:#}; bounded retry: {second:#}"));
            state.save()?;
            return Err(second).context("old top remains visible, candidate pinned beneath; candidate MUST NOT be detached");
        }
    }
    Ok(())
}
pub fn switch(state: &mut State, slot: Slot, candidates: Vec<Candidate>) -> Result<()> {
    let release = state.release(slot)?.clone(); let from = state.journal.as_ref().unwrap().from;
    let started = Instant::now();
    for candidate in candidates {
        let index = candidate.index; let target = &release.targets[index];
        let p = verify_parent(target)?;
        let before = visible(target)?;
        let old = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
        if let Some(from) = from {
            let expected = state.release(from)?.tag(from,index);
            ensure!(before.as_deref() == Some(expected.as_str()), "old target does not match committed release");
        } else {
            ensure!(before.is_none() && fs::mount_id(old.as_fd())? == p.mount_id && old.metadata()?.dev() == p.device, "first target is not on its registered parent");
            ensure!(std::fs::read_dir(&target.path)?.next().is_none(), "first target is not empty after preservation");
        }
        state.intent(format!("place-overlay:{index}:{}", release.tag(slot,index)))?;
        fs::place(candidate.mount.as_fd(), old.as_fd(), before.is_some())?;
        if before.is_some() { detach_old(state, &old, index)?; }
        ensure!(visible(target)?.as_deref() == Some(release.tag(slot,index).as_str()), "candidate did not become visible");
        state.journal.as_mut().unwrap().moved.insert(index); state.intent(format!("target-moved:{index}"))?;
    }
    state.journal.as_mut().unwrap().timings_ms.insert("overlay_placement".into(), started.elapsed().as_millis()); state.save()
}
pub fn revert(state: &mut State, resolve_stacks: bool) -> Result<()> {
    let journal = state.journal.as_ref().context("missing mount recovery journal")?.clone();
    let to = journal.to.context("missing candidate slot")?;
    let candidate = state.release(to)?.clone();
    let from_release = journal.from.map(|s| state.release(s).cloned()).transpose()?;
    let from_root = journal.from.map(|s| state.root(s)).transpose()?;
    let base = fs::release_base()?;
    let mut unresolved_stack = false;
    for (index, target) in candidate.targets.iter().enumerate().rev() {
        verify_parent(target)?;
        match std::fs::symlink_metadata(&target.path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && journal.from.is_none() => continue,
            Err(e) => return Err(e.into()),
        }
        let mounts = table()?;
        let at: Vec<_> = mounts.iter().filter(|m| m.path == target.path).collect();
        let new_tag = candidate.tag(to,index);
        let old_tag = from_release.as_ref().zip(journal.from).map(|(r,s)| r.tag(s,index));
        ensure!(at.iter().all(|m| m.source == new_tag || old_tag.as_ref() == Some(&m.source)), "unexpected mount at recovery target");
        ensure!(at.len() <= 2, "unexplained target mount stack");
        if at.is_empty() && journal.from.is_none() { continue; }
        if at.is_empty() {
            let root = from_root.as_ref().context("missing committed root")?;
            let release = from_release.as_ref().context("missing committed release")?;
            let slot = journal.from.context("missing committed slot")?;
            let parent = verify_parent(target)?;
            let destination = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
            ensure!(fs::mount_id(destination.as_fd())? == parent.mount_id && std::fs::read_dir(&target.path)?.next().is_none(),
                "missing committed target contains unrecorded content or a foreign parent");
            let lower = fs::open_beneath(root.as_fd(), &release.targets[index].source, libc::O_RDONLY | libc::O_DIRECTORY)?;
            state.intent(format!("recover-missing-target-build:{index}"))?;
            let mount = fs::detached_overlay(lower.as_fd(), base.as_fd(), &release.tag(slot,index), release.targets[index].noexec)?;
            state.slots.get_mut(&slot).and_then(Option::as_mut).unwrap().devices.insert(fs::directory(mount.as_fd())?.metadata()?.dev());
            state.intent(format!("recover-missing-target-place:{index}"))?;
            fs::place(mount.as_fd(), destination.as_fd(), false)?;
            state.journal.as_mut().unwrap().moved.remove(&index); state.save()?;
            continue;
        }
        if at.len() == 2 {
            if !resolve_stacks {
                state.journal.as_mut().unwrap().stacked.insert(index); state.save()?;
                unresolved_stack = true; continue;
            }
            let top = visible(target)?;
            ensure!(top == old_tag || top.as_deref() == Some(new_tag.as_str()), "stacked target has an unexpected visible top");
            let old = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
            detach_old(state, &old, index)?;
            state.journal.as_mut().unwrap().stacked.remove(&index); state.save()?;
        }
        let visible_tag = visible(target)?;
        if visible_tag == old_tag { continue; }
        ensure!(visible_tag.as_deref() == Some(new_tag.as_str()), "recovery target has neither expected generation");
        let old = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
        if let (Some(root), Some(release), Some(slot)) = (&from_root, &from_release, journal.from) {
            let lower = fs::open_beneath(root.as_fd(), &release.targets[index].source, libc::O_RDONLY | libc::O_DIRECTORY)?;
            state.intent(format!("revert-build:{index}"))?;
            let replacement = fs::detached_overlay(lower.as_fd(), base.as_fd(), &release.tag(slot,index), release.targets[index].noexec)?;
            let device = fs::directory(replacement.as_fd())?.metadata()?.dev();
            state.slots.get_mut(&slot).and_then(Option::as_mut).unwrap().devices.insert(device);
            state.intent(format!("revert-place:{index}"))?;
            fs::place(replacement.as_fd(), old.as_fd(), true)?;
            detach_old(state, &old, index)?;
        } else {
            state.intent(format!("first-activation-unmount:{index}"))?; fs::detach(old.as_fd())?;
        }
        state.journal.as_mut().unwrap().moved.remove(&index); state.intent(format!("target-reverted:{index}"))?;
    }
    ensure!(!unresolved_stack, "stacked target retained with visible old top; other moved targets reverted; explicit recovery required");
    Ok(())
}
