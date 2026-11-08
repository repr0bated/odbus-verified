//! `op-release-launch reset` retires an explicitly approved native release set.
//!
//! A fresh reset reads the root-owned 0600 `reset-plan.json`; an interrupted reset
//! resumes only its durable journal. Historical slots.json names/hashes/IDs are
//! not admission authority. Unknown reserved roots, mounts and catalog bytes are
//! refused; unrelated identity/guest inputs are outside scope. Reset is forward-
//! only and retains the launcher lock, host namespace and checked deletion ABI.
//! Fixed stubs, any original host backups, approval/receiver pins, empty overlay
//! base and writable golden remain. Current native target parents and supervised
//! wanted states establish the next-activation bootstrap without inventing history.
//! Identity data, grants, guest volumes, kernel and storage guard are untouched.
use anyhow::{Context, Result, ensure};
use super::{boot, model::*, mounts, preserve, recovery, services, transfer};
use std::collections::BTreeMap;

pub(super) fn run(state: &mut State) -> Result<()> {
    if state.journal.is_none() {
        begin(state)?;
    }
    ensure!(state.journal.as_ref().is_some_and(|j| j.operation == "reset"),
        "another transaction is pending; reconcile it before reset");
    let result = resume(state);
    if let Err(error) = result {
        recovery::report_failure(state, &error)?;
        return Err(error);
    }
    Ok(())
}

fn begin(state: &mut State) -> Result<()> {
    let mut admitted = transfer::plan_reset()?;
    let native_targets = if let Some(slot) = admitted.plan.mounted {
        Some(admitted.releases.get(&slot).context("approved mount owner has no native slot")?.targets.clone())
    } else {
        admitted.releases.values().next().map(|release| release.targets.clone())
            .or_else(|| admitted.inventories.first().map(|inventory| inventory.targets.clone()))
    };
    let baseline = if let Some(mut targets) = native_targets {
        // These parents and the fixed stubs are observed native bootstrap facts,
        // not reconstructed pre-install originals. Preserve history only if it
        // actually exists; an empty historical receipt is not reset authority.
        for target in &mut targets { target.parent = Some(mounts::parent(&target.path)?); }
        Some(ResetBaseline { targets, plan: services::plan_reset(&admitted.inventories)? })
    } else { state.reset_baseline.clone() };
    if let Some(baseline) = &baseline {
        for release in admitted.releases.values_mut() {
            for target in &mut release.targets {
                target.parent = baseline.targets.iter().find(|known| known.path == target.path)
                    .and_then(|known| known.parent.clone());
            }
        }
        preserve::verify_reset_bootstrap(baseline)?;
    }
    mounts::check_reset(&admitted.releases, admitted.plan.mounted, baseline.as_ref())?;
    services::check_reset_catalog_admission(&admitted.plan, baseline.is_some())?;
    let mut journal = Journal::new("reset", admitted.plan.mounted, None);
    journal.plan = baseline.as_ref().map_or_else(Plan::default, |b| b.plan.clone());
    journal.removals = admitted.removals;
    journal.reset_plan = Some(admitted.plan);
    journal.reset_releases = admitted.releases;
    journal.reset_phase = Some(ResetPhase::Quiescing);
    state.reset_baseline = baseline;
    state.journal = Some(journal);
    state.intent("reset-native-identities-recorded")
}

pub(super) fn resume(state: &mut State) -> Result<()> {
    let journal = state.journal.as_ref().context("reset without journal")?;
    ensure!(journal.operation == "reset" && journal.reset_phase.is_some()
        && journal.to.is_none() && journal.candidate.is_none() && !journal.aborting,
        "invalid reset journal");
    // Closing admission precedes any stop, catalog unlink, unmount or root delete.
    // Boot also recognizes this durable operation before mounting active roots.
    boot::pending("reset-in-progress")?;
    transfer::check_reset_roots(state)?;
    services::quiesce_reset(state)?;
    if state.journal.as_ref().unwrap().reset_phase == Some(ResetPhase::Quiescing) {
        // SHM plugin state may change while consumers are live. Capture cleanup
        // bytes only after their complete stop, then persist the inventory before
        // the first unlink. Recovery never re-adopts changed cleanup data.
        let records = services::plan_reset_catalogs(state)?;
        let journal = state.journal.as_mut().unwrap();
        journal.catalog_removals = records;
        journal.reset_phase = Some(ResetPhase::Detaching);
        state.intent("reset-catalog-identities-recorded")?;
    }
    if state.journal.as_ref().unwrap().reset_phase == Some(ResetPhase::Detaching) {
        services::retire_reset_catalogs(state)?;
        mounts::detach_reset(state)?;
        state.active = None;
        state.previous = None;
        state.journal.as_mut().unwrap().reset_phase = Some(ResetPhase::Retiring);
        // Clear both activation pointers durably BEFORE the first root deletion.
        state.intent("reset-retire-roots")?;
    }
    ensure!(state.active.is_none() && state.previous.is_none(),
        "reset retirement still has an activation pointer");
    mounts::check_reset_detached(state)?;
    services::retire_reset_catalogs(state)?;
    if state.journal.as_ref().unwrap().reset_phase == Some(ResetPhase::Retiring) {
        transfer::retire_reset_roots(state)?;
        state.journal.as_mut().unwrap().reset_phase = Some(ResetPhase::Finalizing);
        state.intent("reset-roots-retired")?;
    }
    transfer::check_reset_roots(state)?;
    ensure!(state.journal.as_ref().unwrap().removals.iter().all(|r| r.cleaned),
        "reset cannot commit before every exact root is cleaned");
    services::quiesce_reset(state)?;
    mounts::check_reset_detached(state)?;
    // Readiness is durable before the journal-free commit. A crash here leaves a
    // forward-only Finalizing journal, never a committed state with stale readiness.
    state.intent("reset-commit-empty")?;
    boot::ready("none\n")?;
    let slots = std::mem::replace(&mut state.slots, BTreeMap::from([(Slot::A, None), (Slot::B, None)]));
    let journal = state.journal.take().unwrap();
    if let Err(error) = state.save() {
        state.slots = slots;
        state.journal = Some(journal);
        return Err(error).context("reset empty-state commit durability failed; resume reset");
    }
    Ok(())
}
