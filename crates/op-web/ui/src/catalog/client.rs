//! `CatalogService` stream consumer.
//!
//! The schema lives in `op-plugins` (gemma_brain side) and is discovered
//! through gRPC reflection — same pattern as every other service in this
//! crate. This module is read-only: gemma is the sole writer.
//!
//! Wire-level shape (mirrors what gemma emits):
//!
//! ```text
//! service CatalogService {
//!   // Initial snapshot followed by incremental Mint / Retire events.
//!   rpc Subscribe(SubscribeRequest) returns (stream CatalogEvent);
//! }
//!
//! message CatalogEvent {
//!   oneof kind {
//!     CatalogSnapshot snapshot = 1;  // sent once on connect
//!     Element         mint     = 2;  // new element or new version
//!     Retire          retire   = 3;  // remove from active gallery
//!   }
//! }
//! ```
//!
//! The actual decoding uses `grpc::decode_to_json` + serde, so a proto change
//! that stays additive does not require a `zeroclaw-gui` rebuild.

use anyhow::Result;
use serde_json::Value;

use super::dsl::Element;
use super::store::CatalogStore;
use crate::grpc::ReflectionRegistry;

/// Spawn the catalog subscription. Returns once the initial snapshot has been
/// applied; subsequent events are folded into `store` on the background task.
pub async fn run(
    _registry: ReflectionRegistry,
    store: CatalogStore,
) -> Result<()> {
    // TODO: resolve `gemma.catalog.v1.CatalogService/Subscribe` via reflection,
    //       open the server stream, and dispatch each frame below. Wired as a
    //       no-op stub so the rest of the system compiles while gemma_brain's
    //       proto lands.
    let _ = (store,);
    Ok(())
}

/// Fold a single decoded `CatalogEvent` JSON frame into the store.
/// Public so the (forthcoming) integration tests can replay fixtures.
pub fn apply_event(store: &CatalogStore, event: &Value) -> Result<()> {
    if let Some(snapshot) = event.get("snapshot") {
        if let Some(items) = snapshot.get("elements").and_then(Value::as_array) {
            for raw in items {
                let el: Element = serde_json::from_value(raw.clone())?;
                store.admit(el);
            }
        }
        return Ok(());
    }
    if let Some(mint) = event.get("mint") {
        let el: Element = serde_json::from_value(mint.clone())?;
        store.admit(el);
        return Ok(());
    }
    if let Some(retire) = event.get("retire") {
        if let Some(id) = retire.get("id").and_then(Value::as_str) {
            store.retire(&id.to_string());
        }
        return Ok(());
    }
    Ok(())
}
