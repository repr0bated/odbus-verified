//! SSE framing for the existing committed public state subscription.
//! Hydration always reads the pinned catalog and committed SHM; replayed RAM
//! frames are never the source for a new or lagging connection.

use anyhow::{ensure, Context, Result};
use axum::{
    extract::Extension,
    response::sse::{Event, Sse},
};
use futures::Stream;
use op_grpc_bridge::{
    grpc_client::StateUpdateMessage,
    proto::{ChangeType, StateFrameKind},
};
use serde_json::{json, Value};
use std::{convert::Infallible, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::broadcast;

use crate::{state::AppState, state_tree::CommittedStateTree};

const MAX_HYDRATION_BYTES: usize = 16 * 1024 * 1024;
const MAX_HYDRATION_FRAMES: usize = 4096;

pub struct SseEventBroadcaster {
    tx: broadcast::Sender<SseEvent>,
}

#[derive(Clone, Debug)]
pub struct SseEvent {
    pub event_type: String,
    pub data: String,
}

impl Default for SseEventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}
impl SseEventBroadcaster {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }
    pub fn broadcast(&self, event_type: &str, data: &str) {
        // This shared endpoint has no verified per-HTTP-caller audit context.
        // Even metadata (targets/tags/actors) needs its exact Snowball grant.
        if event_type == "audit_event" {
            return;
        }
        let _ = self.tx.send(SseEvent {
            event_type: event_type.into(),
            data: data.into(),
        });
    }
    pub fn subscribe(&self) -> broadcast::Receiver<SseEvent> {
        self.tx.subscribe()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StateCursor {
    catalog_hash: String,
    generation: u64,
}

struct Hydration {
    cursor: StateCursor,
    frames: Vec<SseEvent>,
}

fn event(event_type: &str, data: Value) -> Result<SseEvent> {
    let data = serde_json::to_string(&data)?;
    ensure!(
        data.len() <= MAX_HYDRATION_BYTES,
        "state frame exceeds 16 MiB"
    );
    Ok(SseEvent {
        event_type: event_type.into(),
        data,
    })
}

/// Preserve existing state_update fields plus transport shape/revision metadata.
/// Generation is a decimal string on JSON/SSE so JavaScript cannot round u64.
pub(crate) fn state_message_event(message: &StateUpdateMessage) -> Result<SseEvent> {
    ensure!(
        matches!(
            StateFrameKind::try_from(message.frame_kind),
            Ok(StateFrameKind::InitialState | StateFrameKind::Update | StateFrameKind::Heartbeat)
        ),
        "state frame has no committed stream kind"
    );
    let heartbeat = message.frame_kind == StateFrameKind::Heartbeat as i32;
    let schema = message.change_type == ChangeType::SchemaMigration as i32;
    if !heartbeat {
        let plugin = &message.plugin_id;
        ensure!(
            !plugin.is_empty()
                && plugin.as_bytes()[0].is_ascii_alphanumeric()
                && plugin
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
            "state frame plugin id is invalid"
        );
        let root = format!("/org/opdbus/v1/plugins/{plugin}");
        ensure!(
            message.object_path == root || message.object_path.starts_with(&(root + "/")),
            "state frame is outside its canonical public plugin path"
        );
        ensure!(
            matches!(
                ChangeType::try_from(message.change_type),
                Ok(ChangeType::PropertySet
                    | ChangeType::ObjectAdded
                    | ChangeType::ObjectRemoved
                    | ChangeType::PropertyDelete
                    | ChangeType::SchemaMigration)
            ),
            "non-state event cannot become public state"
        );
    }
    ensure!(
        valid_hash(&message.catalog_hash),
        "state frame has no valid catalog identity"
    );
    let removed = message.new_value.is_none()
        && (schema || message.change_type == ChangeType::ObjectRemoved as i32);
    ensure!(
        heartbeat
            || valid_hash(&message.schema_hash)
            || (removed && message.schema_hash.is_empty()),
        "state frame has no valid schema identity"
    );
    event(
        if heartbeat {
            "state_heartbeat"
        } else if schema {
            "schema_update"
        } else {
            "state_update"
        },
        json!({
            "plugin_id": &message.plugin_id,
            "object_path": &message.object_path,
            "property_name": &message.property_name,
            "new_value": &message.new_value,
            "event_id": &message.event_id,
            "tags": &message.tags_touched,
            "change_type": message.change_type,
            "frame_kind": message.frame_kind,
            "schema_hash": &message.schema_hash,
            "catalog_hash": &message.catalog_hash,
            "state_generation": message.state_generation.to_string(),
            "removed": removed,
            "replace": !schema && message.property_name.as_deref().is_none_or(str::is_empty),
        }),
    )
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hydration_from_tree(tree: CommittedStateTree) -> Result<Hydration> {
    let cursor = StateCursor {
        catalog_hash: tree.catalog_hash,
        generation: tree.projection_generation,
    };
    let mut frames = vec![event(
        "state_reset",
        json!({
            "scope": "public_projection", "catalog_hash": &cursor.catalog_hash,
            "state_generation": cursor.generation.to_string(),
            "frame_kind": StateFrameKind::InitialState as i32,
        }),
    )?];
    let mut states = Vec::new();
    let mut selected_bytes = frames[0].data.len();
    for (plugin_id, plugin) in tree.plugins {
        let path = format!("/org/opdbus/v1/plugins/{plugin_id}");
        // Serialize the original sealed JSON, never a typed schema with new
        // serde defaults inserted by the currently compiled consumer.
        let schema: Value = serde_json::from_str(&plugin.schema_json)?;
        frames.push(event(
            "schema_update",
            json!({
                "plugin_id": &plugin_id, "object_path": &path,
                "property_name": &plugin.schema_hash, "new_value": schema,
                "event_id": "0", "tags": [], "change_type": ChangeType::SchemaMigration as i32,
                "frame_kind": StateFrameKind::InitialState as i32,
                "schema_hash": &plugin.schema_hash, "catalog_hash": &cursor.catalog_hash,
                "state_generation": cursor.generation.to_string(), "removed": false,
            }),
        )?);
        let absent = plugin.state.is_none();
        states.push(event("state_update", json!({
            "plugin_id": plugin_id, "object_path": path, "property_name": null,
            "new_value": plugin.state, "event_id": "0", "tags": [],
            "change_type": if absent { ChangeType::ObjectRemoved as i32 } else { ChangeType::PropertySet as i32 },
            "frame_kind": StateFrameKind::InitialState as i32,
            "schema_hash": plugin.schema_hash, "catalog_hash": &cursor.catalog_hash,
            "state_generation": cursor.generation.to_string(), "removed": absent, "replace": true,
        }))?);
        selected_bytes = selected_bytes
            .checked_add(frames.last().context("schema frame missing")?.data.len())
            .and_then(|bytes| bytes.checked_add(states.last().map_or(0, |frame| frame.data.len())))
            .context("state hydration size overflow")?;
        ensure!(
            selected_bytes <= MAX_HYDRATION_BYTES,
            "state hydration exceeds 16 MiB"
        );
        ensure!(
            frames.len() + states.len() < MAX_HYDRATION_FRAMES,
            "state hydration exceeds 4096 frames"
        );
    }
    // Contracts precede every value, including absent-state tombstones.
    frames.extend(states);
    frames.push(event(
        "state_ready",
        json!({
            "scope": "public_projection", "catalog_hash": &cursor.catalog_hash,
            "state_generation": cursor.generation.to_string(),
            "frame_kind": StateFrameKind::InitialState as i32,
        }),
    )?);
    ensure!(
        frames.len() <= MAX_HYDRATION_FRAMES,
        "state hydration exceeds 4096 frames"
    );
    let bytes = frames
        .iter()
        .try_fold(0usize, |total, frame| total.checked_add(frame.data.len()))
        .context("state hydration size overflow")?;
    ensure!(
        bytes <= MAX_HYDRATION_BYTES,
        "state hydration exceeds 16 MiB"
    );
    Ok(Hydration { cursor, frames })
}

async fn load_hydration() -> Result<Hydration> {
    hydration_from_tree(crate::state_tree::read_committed_tree().await?)
}

fn state_cursor(event: &SseEvent) -> Result<StateCursor> {
    let value: Value = serde_json::from_str(&event.data)?;
    let catalog_hash = value
        .get("catalog_hash")
        .and_then(Value::as_str)
        .filter(|hash| valid_hash(hash))
        .context("SSE state frame has no catalog identity")?
        .to_owned();
    let generation = value
        .get("state_generation")
        .and_then(Value::as_str)
        .context("SSE state generation must be an exact decimal string")?
        .parse::<u64>()?;
    Ok(StateCursor {
        catalog_hash,
        generation,
    })
}

fn wire(event: SseEvent) -> Event {
    Event::default().event(event.event_type).data(event.data)
}

fn unavailable() -> Event {
    Event::default()
        .event("state_unavailable")
        .data("{\"reason\":\"committed_projection_unavailable\"}")
        .retry(Duration::from_secs(5))
}

type Refresh = Pin<Box<dyn Future<Output = Result<Hydration>> + Send>>;

fn state_event_stream(
    mut rx: broadcast::Receiver<SseEvent>,
    mut refresh: impl FnMut() -> Refresh + Send + 'static,
) -> impl Stream<Item = std::result::Result<Event, Infallible>> + Send {
    async_stream::stream! {
        let initial = match refresh().await {
            Ok(hydration) => hydration,
            Err(error) => { tracing::warn!(%error, "Cannot hydrate SSE from committed SHM"); yield Ok(unavailable()); return; }
        };
        let mut baseline = initial.cursor;
        let mut last_forwarded = baseline.generation;
        for frame in initial.frames { yield Ok(wire(frame)); }
        loop {
            let received = rx.recv().await;
            let (pending, resync) = match received {
                Ok(frame) if frame.event_type == "state_resync" => (None, true),
                Ok(frame) if matches!(frame.event_type.as_str(), "state_update" | "schema_update" | "state_heartbeat") => {
                    let cursor = match state_cursor(&frame) {
                        Ok(cursor) => cursor,
                        Err(error) => { tracing::warn!(%error, "Invalid SSE state framing"); yield Ok(unavailable()); return; }
                    };
                    if cursor.catalog_hash == baseline.catalog_hash && cursor.generation <= baseline.generation {
                        // This frame was queued before fresh hydration. Every
                        // selected public root is already represented above.
                        continue;
                    }
                    let resync = cursor.catalog_hash != baseline.catalog_hash || cursor.generation < last_forwarded;
                    (Some((frame, cursor)), resync)
                }
                Ok(frame) if frame.event_type == "audit_event" => continue,
                Ok(frame) => { yield Ok(wire(frame)); continue; }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(Event::default().event("stream_gap").data("{\"state\":\"rehydrating\"}"));
                    (None, true)
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if resync {
                let fresh = match refresh().await {
                    Ok(hydration) => hydration,
                    Err(error) => { tracing::warn!(%error, "Cannot rehydrate SSE from committed SHM"); yield Ok(unavailable()); return; }
                };
                baseline = fresh.cursor;
                last_forwarded = baseline.generation;
                for frame in fresh.frames { yield Ok(wire(frame)); }
            }
            if let Some((frame, cursor)) = pending
                && cursor.catalog_hash == baseline.catalog_hash && cursor.generation > baseline.generation && cursor.generation >= last_forwarded {
                    last_forwarded = cursor.generation;
                    yield Ok(wire(frame));
                }
        }
    }
}

/// GET /api/events. The shared endpoint carries only the selected public SHM
/// namespace, never credential files or privileged audit history.
/// state_reset clears prior public schema/state keys; state_ready completes a
/// fresh bounded hydration. State generations are decimal strings.
pub async fn sse_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    // Subscribe before the filesystem read to avoid losing a concurrent commit.
    let rx = state.sse_broadcaster.subscribe();
    Sse::new(state_event_stream(rx, || Box::pin(load_hydration()))).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

#[cfg(test)]
#[path = "sse_tests.rs"]
mod tests;
