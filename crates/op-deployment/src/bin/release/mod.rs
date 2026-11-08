mod boot;
mod fs;
mod inventory;
mod lifetime;
mod loggers;
mod model;
mod mounts;
mod preserve;
mod recovery;
mod reset;
mod services;
mod transfer;

use anyhow::{Context, Result, bail, ensure};
use model::*;
use std::{collections::BTreeSet, path::Path};

fn activate(state: &mut State, slot: Slot) -> Result<()> {
    ensure!(state.active != Some(slot), "requested slot is already active");
    let mut candidate = state.release(slot)?.clone();
    let incoming = inventory::load(&*state.root(slot)?)?;
    ensure!(incoming.graph_sha256 == candidate.graph_sha256 && incoming.tree_sha256 == candidate.tree_sha256, "activation inventory differs from verified receive");
    let old = state.active.and_then(|s| match state.root(s).and_then(|r| inventory::load(&r)) {
        Ok(inventory) => Some(inventory),
        Err(error) => { eprintln!("op-release: active inventory unavailable; selecting every declared candidate consumer: {error:#}"); None }
    });
    let retained_bootstrap = state.active.is_none() && state.reset_baseline.is_some();
    let approval = if state.active.is_none() && !retained_bootstrap { Some(preserve::approval(&incoming)?) } else { None };
    let no_links = BTreeSet::new();
    let links = approval.as_ref().map_or(&no_links, |a| &a.link_services);
    let plan = if retained_bootstrap {
        services::plan_after_reset(&incoming, state.reset_baseline.as_ref().unwrap())?
    } else { services::plan(old.as_ref(), &incoming, links)? };
    if let Some(active) = state.active {
        let previous = state.release(active)?;
        ensure!(previous.targets.len() == candidate.targets.len() && previous.targets.iter().zip(&candidate.targets).all(|(a,b)| a.path == b.path),
            "target-set changes require a separate approved host migration, not a mixed overlay rotation");
        for (target, previous) in candidate.targets.iter_mut().zip(&previous.targets) { target.parent = previous.parent.clone(); mounts::verify_parent(target)?; }
        for name in &plan.order {
            ensure!(incoming.services.get(name).is_some_and(|s| s.release_owned), "removing a supervised release service requires owner lifecycle approval before activation");
            ensure!(Path::new(SERVICES).join(name).join("run").try_exists()?, "new release service {name} requires approved fixed-stub installation");
        }
    } else if let Some(baseline) = &state.reset_baseline {
        ensure!(baseline.targets.len() == candidate.targets.len()
            && baseline.targets.iter().zip(&candidate.targets).all(|(a,b)| a.path == b.path),
            "target-set changes after reset require approved host migration");
        mounts::check_reset_detached(state)?;
        preserve::verify_reset_bootstrap(baseline)?;
        for name in &plan.order {
            ensure!(Path::new(SERVICES).join(name).join("run").try_exists()?,
                "release service {name} requires approved fixed-stub installation after reset");
        }
        for (target, previous) in candidate.targets.iter_mut().zip(&baseline.targets) {
            target.parent = previous.parent.clone(); mounts::verify_parent(target)?;
        }
    } else {
        for target in &mut candidate.targets { target.parent = Some(mounts::parent(&target.path)?); }
    }
    let logger_changes = loggers::plan(&plan, &incoming, state.active.is_none() && !retained_bootstrap)?;
    let mut journal = Journal::new("activate", state.active, Some(slot)); journal.plan = plan; journal.candidate = Some(candidate.clone());
    journal.loggers = logger_changes;
    journal.retained_bootstrap = retained_bootstrap;
    state.slots.insert(slot, Some(candidate.clone())); state.journal = Some(journal); state.save()?;
    let outcome = (|| -> Result<()> {
        if let Some(approval) = &approval { preserve::plan(state, &incoming, approval)?; }
        else { preserve::plan_hooks(state, &incoming)?; }
        let candidates = mounts::build(state, slot)?;
        state.intent("transition-readiness")?; boot::transition(state)?;
        services::down(state)?;
        if approval.is_some() { preserve::apply(state)?; }
        mounts::switch(state, slot, candidates)?;
        preserve::apply_hooks(state, true)?;
        loggers::apply(state, true)?;
        services::catalog(state, &candidate)?;
        if approval.is_some() { preserve::links(state)?; }
        services::reload_loggers(state)?;
        services::restore(state, &incoming)?;
        state.intent("commit-active")?;
        let prior_previous = state.previous;
        state.previous = state.active; state.active = Some(slot);
        let journal = state.journal.take().unwrap();
        let reset_baseline = state.reset_baseline.take();
        if !journal.preserved.is_empty() { state.first_activation_originals = journal.preserved.clone(); }
        loggers::commit_original_receipts(state, &journal);
        state.last_timings_ms.extend(journal.timings_ms.clone());
        if let Err(error) = state.save() {
            state.active = journal.from; state.previous = prior_previous;
            state.reset_baseline = reset_baseline;
            if journal.from.is_none() && !journal.retained_bootstrap { state.first_activation_originals.clear(); }
            state.journal = Some(journal);
            // Restore durable recovery intent before any attempt to undo an uncertain commit.
            state.save()?;
            return Err(error).context("active commit durability failed");
        }
        boot::complete(state)
    })();
    if let Err(error) = outcome {
        // A failure after the durable commit must never roll back using an absent journal.
        if state.journal.is_none() { boot::pending(&format!("committed-readiness-failed: {error:#}"))?; return Err(error); }
        if let Err(revert) = recovery::revert_activation(state, false) {
            let combined = anyhow::anyhow!("activation failed: {error:#}; recovery unresolved: {revert:#}");
            recovery::report_failure(state, &combined)?; return Err(combined);
        }
        return Err(error).context("activation failed; original complete state restored");
    }
    Ok(())
}
pub fn run() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let verb = arguments.first().map(String::as_str).context("usage: op-release {release|deploy|provision-slots|status|activate a|b|rollback|reset|abort [name received-id received-uuid source-id source-uuid]|boot}")?;
    ensure!(match verb {
        "activate" => arguments.len() == 2,
        "abort" => arguments.len() == 1 || arguments.len() == 6,
        _ => arguments.len() == 1,
    }, "invalid release arguments");
    ensure!(matches!(verb, "release" | "deploy" | "provision-slots" | "status" | "activate" | "rollback" | "reset" | "abort" | "boot"), "unsupported release verb");
    if verb == "status" {
        let state = State::load()?;
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"state":state,"mounts":mounts::table()?,"readiness":std::fs::read_to_string(READY).ok(),"transaction_in_progress":lock_observed()? ,"occupied_slot_reuse":"requires-running-kernel-quiescent-delete; status-does-not-probe"}))?);
        return Ok(());
    }
    ensure!(unsafe { libc::geteuid() } == 0, "release mutations require the authorized root owner");
    if let Err(error) = fs::verify_launcher_lock(Path::new(LOCK)) {
        if verb == "boot" { boot::pending("lock-busy-or-invalid")?; }
        return Err(error);
    }
    let host = std::fs::metadata("/proc/1/ns/mnt")?; let own = std::fs::metadata("/proc/self/ns/mnt")?;
    use std::os::unix::fs::MetadataExt;
    ensure!(host.ino() == own.ino() && host.dev() == own.dev(), "release target operations require the host mount namespace");
    let mut state = State::load()?;
    if verb == "boot" {
        if let Err(error) = boot::run(&state) { boot::pending(&format!("boot-failed: {error:#}"))?; return Err(error); }
        return Ok(());
    }
    if verb == "reset" {
        ensure!(state.journal.as_ref().is_none_or(|j| j.operation == "reset"),
            "another transaction is pending; reconcile it explicitly before reset");
        return reset::run(&mut state);
    }
    ensure!(!state.journal.as_ref().is_some_and(|j| j.operation == "reset"),
        "forward-only reset is pending; invoke reset to resume it; abort/activation cannot restore retired roots");
    if verb == "abort" && arguments.len() == 6 {
        ensure!(state.journal.is_none(), "pending transaction exists; reconcile its recorded identities with the ordinary abort command");
        ensure!(std::fs::read_to_string(READY)?.starts_with("complete "), "resolve committed readiness before orphan cleanup");
        return transfer::abort_orphan(
            &mut state, &arguments[1],
            arguments[2].parse().context("invalid received root ID")?,
            arguments[3].parse().context("invalid received root UUID")?,
            arguments[4].parse().context("invalid source root ID")?,
            arguments[5].parse().context("invalid source root UUID")?,
        );
    }
    if state.journal.is_some() {
        if verb == "abort" && state.journal.as_ref().is_some_and(|j| j.operation == "deploy") {
            transfer::abort_transfer(&mut state)?;
        } else {
            recovery::recover(&mut state)?;
        }
        bail!("reconciliation completed; requested operation NOT started; invoke explicitly again if a new operation is wanted");
    }
    // A reboot can leave committed targets intact with an uncommitted catalog. Restore it
    // under the same transaction/consumer protocol, never from the boot path.
    if state.active.is_some() && std::fs::read_to_string(READY).unwrap_or_default().starts_with("recovery-pending") {
        recover_readiness(&mut state)?;
        bail!("committed readiness restored; requested operation NOT started; invoke explicitly again if a new operation is wanted");
    }
    match verb {
        "provision-slots" => transfer::provision(&mut state),
        "deploy" => transfer::deploy(&mut state).map(|_| ()),
        "release" => { let slot = transfer::deploy(&mut state)?; activate(&mut state, slot) }
        "activate" => activate(&mut state, Slot::parse(&arguments[1])?),
        "rollback" => { let previous = state.previous.context("rollback unavailable: no retained previous release")?; activate(&mut state, previous) }
        "abort" => bail!("no pending release transaction to abort"),
        _ => unreachable!(),
    }
}
fn recover_readiness(state: &mut State) -> Result<()> {
    let slot = state.active.context("no committed release")?;
    let release = state.release(slot)?.clone();
    let inventory = inventory::load(&*state.root(slot)?)?;
    let mut journal = Journal::new("activate", Some(slot), Some(slot));
    journal.candidate = Some(release.clone()); journal.plan = services::plan(None, &inventory, &BTreeSet::new())?;
    state.journal = Some(journal); state.intent("recover-committed-catalog")?;
    let result = (|| -> Result<()> {
        boot::transition(state)?; services::down(state)?;
        mounts::revert(state, true)?;
        for (index,target) in release.targets.iter().enumerate() {
            ensure!(mounts::visible(target)?.as_deref() == Some(release.tag(slot,index).as_str()), "committed target missing; boot mount integration must be restored");
        }
        services::catalog(state, &release)?; services::restore(state, &inventory)?;
        state.journal = None; state.save()?; boot::complete(state)
    })();
    if let Err(e) = result { recovery::report_failure(state, &e)?; return Err(e); }
    Ok(())
}
fn lock_observed() -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let lock = match fs::secure_absolute(Path::new(LOCK), libc::O_RDONLY) { Ok(f) => f, Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => return Ok(false), Err(e) => return Err(e) };
    let m = lock.metadata()?;
    let needle = format!("{:02x}:{:02x}:{}", libc::major(m.dev()), libc::minor(m.dev()), m.ino());
    let locks = std::fs::read_to_string("/proc/locks")?;
    Ok(locks.lines().any(|line| line.contains("FLOCK") && line.split_whitespace().any(|word| word == needle)))
}
