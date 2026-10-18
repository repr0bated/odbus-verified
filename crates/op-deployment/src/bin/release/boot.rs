use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, os::fd::AsFd, path::Path};
use super::{fs, inventory, model::*, mounts};

pub fn ready(text: &str) -> Result<()> { fs::atomic_text(Path::new(READY), text) }
pub fn pending(reason: &str) -> Result<()> {
    let reason = reason.replace(['\n', '\r'], " "); ready(&format!("recovery-pending {reason}\n"))
}
pub fn transition(state: &State) -> Result<()> {
    let j = state.journal.as_ref().context("transition without journal")?;
    let mut text = format!("transition {} {}\n", j.from.map_or("none", Slot::name), j.to.map_or("none", Slot::name));
    for name in &j.plan.order { text.push_str(&format!("q {name}\n")); }
    for name in &j.allowed { text.push_str(&format!("allow {name}\n")); }
    ready(&text)
}
pub fn complete(state: &State) -> Result<()> {
    if state.journal.is_some() { return pending("journal-present"); }
    let Some(slot) = state.active else { return ready("none\n"); };
    let release = state.release(slot)?;
    for (i, target) in release.targets.iter().enumerate() {
        if mounts::visible(target)?.as_deref() != Some(release.tag(slot,i).as_str()) { return pending("target-set-incomplete"); }
    }
    let hash = fs::hash_file(&mut fs::secure_absolute(&Path::new(PERSISTED).join(".manifest.json"), libc::O_RDONLY)?)?;
    if hash != release.catalog_sha256 { return pending("catalog-mismatch"); }
    ready(&format!("complete {} {} {}\n", slot.name(), release.name, hash))
}
pub fn run(state: &State) -> Result<()> {
    let cmdline = std::fs::read_to_string("/proc/cmdline")?;
    if cmdline.split_whitespace().any(|s| s == "opdbus.release=off") { return ready("disabled\n"); }
    let Some(slot) = state.active else { return if state.journal.is_some() { pending("journal-present-no-committed-release") } else { ready("none\n") }; };
    pending("boot-mounting-committed-set")?;
    ensure!(std::fs::read_to_string("/proc/filesystems")?.lines().any(|l| l.split_whitespace().last() == Some("overlay")), "overlay module is not preloaded; boot will not autoload it");
    let release = state.release(slot)?; let root = state.root(slot)?;
    let inventory = inventory::load(&root)?;
    ensure!(inventory.graph_sha256 == release.graph_sha256 && inventory.tree_sha256 == release.tree_sha256, "boot committed inventory mismatch");
    let mut built = Vec::new();
    for (index,target) in release.targets.iter().enumerate() {
        let parent = mounts::verify_parent(target)?;
        if let Some(tag) = mounts::visible(target)? {
            ensure!(tag == release.tag(slot,index), "boot will not replace an uncommitted/foreign target"); continue;
        }
        let target_fd = fs::secure_absolute(&target.path, libc::O_PATH | libc::O_DIRECTORY)?;
        ensure!(fs::mount_id(target_fd.as_fd())? == parent.mount_id, "boot target parent mount mismatch");
        let lower = fs::open_beneath(root.as_fd(), &target.source, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let overlay = fs::detached_overlay(lower.as_fd(), &release.tag(slot,index), target.noexec)?;
        built.push((index, overlay, target_fd));
    }
    for (index, overlay, target) in built {
        // Boot intentionally does not mutate durable state or recover the transaction.
        pending(&format!("boot-place-{index}"))?;
        fs::place(overlay.as_fd(), target.as_fd(), false)?;
    }
    complete(state)
}
