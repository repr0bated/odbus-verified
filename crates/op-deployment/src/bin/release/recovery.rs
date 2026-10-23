use anyhow::{Context, Result, bail};
use super::{boot, inventory, loggers, model::*, mounts, preserve, services, transfer};

pub fn report_failure(state: &mut State, error: &anyhow::Error) -> Result<()> {
    let stop_result = if state.journal.as_ref().is_some_and(|j| j.operation == "activate") {
        services::down(state)
    } else { Ok(()) };
    let catalog = (|| -> Result<String> {
        super::fs::hash_file(&mut super::fs::secure_absolute(
            &std::path::Path::new(PERSISTED).join(".manifest.json"), libc::O_RDONLY)?)
    })();
    let observations = mounts::table().map(|m| m.into_iter().filter(|m| m.source.starts_with("opdbus-release:")).collect::<Vec<_>>());
    if let Some(journal) = state.journal.as_mut() {
        journal.error = Some(format!("{error:#}; stop result: {stop_result:?}; persisted catalog: {catalog:?}; actual release mounts: {observations:?}"));
        state.save()?;
    }
    boot::pending(&format!("{error:#}"))
}
pub fn revert_activation(state: &mut State, resolve_stacks: bool) -> Result<()> {
    services::down(state)?;
    state.journal.as_mut().context("missing recovery journal")?.allowed.clear(); state.save()?;
    boot::transition(state)?;
    services::reconcile_catalog(state)?;
    mounts::revert(state, resolve_stacks)?;
    preserve::apply_hooks(state, false)?;
    loggers::apply(state, false)?;
    let from = state.journal.as_ref().unwrap().from;
    if let Some(slot) = from {
        let release = state.release(slot)?.clone();
        let inventory = inventory::load(&*state.root(slot)?)?;
        services::catalog(state, &release)?;
        services::reload_loggers(state)?;
        loggers::cleanup_created(state)?;
        services::restore(state, &inventory)?;
    } else {
        preserve::restore(state, preserve::RestorePhase::Content)?;
        preserve::restore(state, preserve::RestorePhase::Hooks)?;
        preserve::restore(state, preserve::RestorePhase::Links)?;
        services::reload_loggers(state)?;
        loggers::cleanup_created(state)?;
        preserve::restore(state, preserve::RestorePhase::Directories)?;
        services::restore_original(state)?;
    }
    state.journal = None; state.save()?; boot::complete(state)
}
pub fn recover(state: &mut State) -> Result<()> {
    let operation = state.journal.as_ref().context("no journal to recover")?.operation.clone();
    let result = match operation.as_str() {
        "provision" => transfer::provision_resume(state),
        "deploy" => transfer::recover_transfer(state),
        "activate" => revert_activation(state, true),
        _ => bail!("unknown transaction operation {operation}"),
    };
    if let Err(error) = result { report_failure(state, &error)?; return Err(error); }
    if operation != "activate" { boot::complete(state)?; }
    Ok(())
}
