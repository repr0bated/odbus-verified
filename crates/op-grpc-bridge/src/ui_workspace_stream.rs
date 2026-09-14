//! Private principal/session workspace views on the existing StateSync service.
//!
//! The publisher owns admission, activation and committed private SHM reads.
//! Owner notifications contain metadata only and serve solely as wakeups. No
//! private value enters MutationEngine's public state cache or change channel.
use std::{collections::BTreeMap, pin::Pin, sync::Arc, time::Duration};

use futures::Stream;
use serde_json::Value;
use tonic::{Request, Status};

use crate::{
    event_chain_access,
    grpc_server::{
        simd_to_prost_struct, simd_to_prost_value, state_heartbeat_frame,
        state_snapshot_frames_with_views, DECLARED_CAPABILITY_HEADER,
    },
    interceptor::load_capability_grants,
    mutation_engine::MutationEngine,
    oracle_assertion::HumanPrincipalIdentity,
    proto::{GetStateRequest, GetStateResponse, StateChange, StateFrameKind, SubscribeRequest},
    state_stream::{PluginSnapshot, StateSnapshot},
    ui_workspace_dispatch::{
        CommittedUiWorkspace, UiWorkspacePublisher, UiWorkspaceSubscription,
        UI_WORKSPACE_READ_CAPABILITY,
    },
    ui_workspace_stream_selection::WorkspaceSelection,
};

const PLUGIN: &str = "gemma_brain";
const HEARTBEAT: Duration = Duration::from_secs(30);
pub(crate) type WorkspaceStream = Pin<Box<dyn Stream<Item = Result<StateChange, Status>> + Send>>;

struct WorkspaceAccess {
    identity: HumanPrincipalIdentity,
    capability: String,
}

impl WorkspaceAccess {
    #[allow(clippy::result_large_err)]
    fn from_request<T>(
        request: &Request<T>,
        identity: HumanPrincipalIdentity,
    ) -> Result<Self, Status> {
        let declared: Vec<_> = request
            .metadata()
            .get_all(DECLARED_CAPABILITY_HEADER)
            .into_iter()
            .collect();
        if declared.len() != 1 {
            return Err(Status::permission_denied(
                "one exact workspace read capability declaration is required",
            ));
        }
        let capability = declared[0]
            .to_str()
            .map_err(|_| Status::permission_denied("invalid workspace capability declaration"))?
            .to_owned();
        if capability != UI_WORKSPACE_READ_CAPABILITY {
            return Err(Status::permission_denied(
                "exact sealed workspace read capability is required",
            ));
        }
        let access = Self {
            identity,
            capability,
        };
        access.check_grant()?;
        Ok(access)
    }

    #[allow(clippy::result_large_err)]
    fn check_grant(&self) -> Result<(), Status> {
        if !load_capability_grants(&self.identity.principal_id).contains(&self.capability) {
            return Err(Status::permission_denied(
                "current exact-principal workspace read grant is required",
            ));
        }
        Ok(())
    }

    async fn revalidate(
        &self,
        engine: &MutationEngine,
        publisher: &UiWorkspacePublisher,
        ticket: &UiWorkspaceSubscription,
    ) -> Result<(), Status> {
        event_chain_access::validate_identity(engine, &self.identity).await?;
        self.check_grant()?;
        // This is identity/grant/catalog/activation admission only; it does not
        // read SHM on heartbeats and cannot expose an earlier owner snapshot.
        publisher.revalidate_subscription(engine, ticket).await
            .map_err(|_| Status::permission_denied("workspace read admission was withdrawn; reactivate after restoring the exact contract and owner"))
    }
}

async fn open<T>(
    engine: &MutationEngine,
    request: &Request<T>,
    identity: HumanPrincipalIdentity,
) -> Result<
    (
        Arc<UiWorkspacePublisher>,
        UiWorkspaceSubscription,
        WorkspaceAccess,
    ),
    Status,
> {
    let access = WorkspaceAccess::from_request(request, identity)?;
    // Re-check even when called from an already verified StateSync handler.
    event_chain_access::validate_identity(engine, &access.identity).await?;
    let publisher = engine
        .ui_workspace()
        .map_err(|_| Status::failed_precondition("private workspace owner is not configured"))?
        .clone();
    let ticket = publisher.subscribe_workspace(
        engine, &access.identity.principal_id, Some(&access.identity.session_id),
        Some(&access.identity.session_genesis), &access.capability,
    ).await.map_err(|_| Status::failed_precondition("private workspace requires current sealed read admission and explicit activation/recovery"))?;
    access.revalidate(engine, &publisher, &ticket).await?;
    Ok((publisher, ticket, access))
}

pub(crate) async fn subscribe(
    engine: Arc<MutationEngine>,
    request: Request<SubscribeRequest>,
    identity: HumanPrincipalIdentity,
    selection: WorkspaceSelection,
) -> Result<WorkspaceStream, Status> {
    if !request.get_ref().tags.is_empty() {
        return Err(Status::unimplemented(
            "workspace tag selection has no committed selection contract",
        ));
    }
    let (publisher, ticket, access) = open(&engine, &request, identity).await?;
    let req = request.into_inner();
    Ok(build_stream(
        engine,
        publisher,
        ticket,
        access,
        selection,
        req.include_initial_state,
        req.include_schema,
        HEARTBEAT,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_stream(
    engine: Arc<MutationEngine>,
    publisher: Arc<UiWorkspacePublisher>,
    mut ticket: UiWorkspaceSubscription,
    access: WorkspaceAccess,
    selection: WorkspaceSelection,
    include_initial_state: bool,
    include_schema: bool,
    heartbeat_interval: Duration,
) -> WorkspaceStream {
    Box::pin(async_stream::stream! {
        let mut previous = match snapshot(&ticket.initial) {
            Ok(snapshot) => snapshot,
            Err(status) => { yield Err(status); return; }
        };
        let initial = state_snapshot_frames_with_views(
            &previous, None, include_schema, include_initial_state,
            StateFrameKind::InitialState, selection.paths(), false, exact_views,
        );
        match initial {
            Ok(frames) => for frame in frames {
                if let Err(status) = access.revalidate(&engine, &publisher, &ticket).await {
                    yield Err(status); return;
                }
                yield Ok(frame);
            },
            Err(status) => { yield Err(status); return; }
        }
        let mut heartbeat = tokio::time::interval(heartbeat_interval);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;
        loop {
            let force_rehydrate = tokio::select! {
                update = ticket.receiver.recv() => match update {
                    // Never inspect a wake's payload as state or use its
                    // generation as proof that a projection committed.
                    Ok(_) => Some(false),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some(true),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                _ = heartbeat.tick() => None,
            };
            if let Err(status) = access.revalidate(&engine, &publisher, &ticket).await {
                yield Err(status); return;
            }
            if let Some(force_rehydrate) = force_rehydrate {
                let current = match publisher.refresh_subscription(&engine, &ticket).await {
                    Ok(receipt) => match snapshot(&receipt) {
                        Ok(snapshot) => snapshot,
                        Err(status) => { yield Err(status); return; }
                    },
                    Err(_) => { yield Err(Status::failed_precondition("committed private workspace is unavailable; explicit recovery is required")); return; }
                };
                if current.generation != previous.generation || current.catalog_hash != previous.catalog_hash || force_rehydrate {
                    match state_snapshot_frames_with_views(
                        &current, Some(&previous), include_schema, true,
                        if force_rehydrate { StateFrameKind::InitialState } else { StateFrameKind::Update },
                        selection.paths(), force_rehydrate, exact_views,
                    ) {
                        Ok(frames) => for frame in frames {
                            if let Err(status) = access.revalidate(&engine, &publisher, &ticket).await {
                                yield Err(status); return;
                            }
                            yield Ok(frame);
                        },
                        Err(status) => { yield Err(status); return; }
                    }
                    previous = current;
                }
            }
            // Metadata-only heartbeat reports the last observed generation.
            // It does not poll state and makes no freshness/commit assertion.
            if let Err(status) = access.revalidate(&engine, &publisher, &ticket).await {
                yield Err(status); return;
            }
            yield Ok(state_heartbeat_frame(&previous));
        }
    })
}

pub(crate) async fn get_state(
    engine: Arc<MutationEngine>,
    request: Request<GetStateRequest>,
    identity: HumanPrincipalIdentity,
    selection: WorkspaceSelection,
) -> Result<GetStateResponse, Status> {
    let (publisher, ticket, access) = open(&engine, &request, identity).await?;
    let current = snapshot(&ticket.initial)?;
    let state = current
        .plugins
        .get(PLUGIN)
        .and_then(|plugin| plugin.state.as_ref())
        .ok_or_else(|| Status::not_found("workspace has no committed document"))?;
    let selected = value_at_path(state, &selection.paths()[0])
        .map_err(|_| Status::invalid_argument("invalid workspace descendant"))?
        .ok_or_else(|| Status::not_found("workspace descendant is absent"))?;
    let owned = simd_json::serde::to_owned_value(selected)
        .map_err(|_| Status::data_loss("committed workspace value cannot be encoded"))?;
    access.revalidate(&engine, &publisher, &ticket).await?;
    Ok(GetStateResponse {
        state: selected.is_object().then(|| simd_to_prost_struct(&owned)),
        value: Some(simd_to_prost_value(&owned)),
        effective_hash: String::new(),
        at_event_id: 0,
    })
}

#[allow(clippy::result_large_err)]
fn snapshot(receipt: &CommittedUiWorkspace) -> Result<StateSnapshot, Status> {
    let schema: Value = serde_json::from_str(&receipt.schema_json)
        .map_err(|_| Status::data_loss("pinned workspace schema is invalid"))?;
    let state = receipt
        .plugin_state()
        .map_err(|_| Status::data_loss("committed workspace envelope is invalid"))?;
    Ok(StateSnapshot {
        generation: receipt.generation,
        catalog_hash: receipt.catalog_hash.clone(),
        plugins: [(
            PLUGIN.to_owned(),
            PluginSnapshot {
                schema_hash: receipt.schema_hash.clone(),
                schema,
                state,
            },
        )]
        .into_iter()
        .collect(),
    })
}

/// Existing state paths use literal object segments. Arrays additionally accept
/// canonical nonnegative indices. Absence differs from explicit JSON null.
fn value_at_path<'a>(state: &'a Value, path: &str) -> anyhow::Result<Option<&'a Value>> {
    let root = crate::state_stream::plugin_object_path(PLUGIN);
    let suffix = path
        .strip_prefix(&(root + "/"))
        .ok_or_else(|| anyhow::anyhow!("workspace path is outside canonical root"))?;
    let mut current = state;
    for segment in suffix.split('/') {
        anyhow::ensure!(
            !segment.is_empty() && segment != "." && segment != "..",
            "invalid workspace path segment"
        );
        current = match current {
            Value::Object(object) => match object.get(segment) {
                Some(value) => value,
                None => return Ok(None),
            },
            Value::Array(items) => {
                anyhow::ensure!(
                    segment == "0"
                        || (!segment.starts_with('0')
                            && segment.bytes().all(|b| b.is_ascii_digit())),
                    "noncanonical workspace array index"
                );
                let index: usize = segment
                    .parse()
                    .map_err(|_| anyhow::anyhow!("workspace array index exceeds range"))?;
                match items.get(index) {
                    Some(value) => value,
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
    }
    Ok(Some(current))
}

fn exact_views(
    plugin: &str,
    state: &Value,
    paths: &[String],
) -> anyhow::Result<BTreeMap<String, Value>> {
    anyhow::ensure!(plugin == PLUGIN, "invalid private workspace plugin");
    let selection = WorkspaceSelection::from_subscription(&[PLUGIN.to_owned()], paths)?
        .ok_or_else(|| anyhow::anyhow!("explicit workspace selection is required"))?;
    let mut selected = BTreeMap::new();
    let mut bytes = 0usize;
    for path in selection.paths() {
        if let Some(value) = value_at_path(state, path)? {
            bytes = bytes
                .checked_add(serde_json::to_vec(value)?.len())
                .ok_or_else(|| anyhow::anyhow!("workspace selected-byte overflow"))?;
            anyhow::ensure!(
                bytes <= 16 * 1024 * 1024,
                "workspace selection exceeds 16 MiB"
            );
            selected.insert(path.clone(), value.clone());
        }
    }
    Ok(selected)
}

#[cfg(test)]
#[path = "ui_workspace_stream_tests.rs"]
mod tests;
