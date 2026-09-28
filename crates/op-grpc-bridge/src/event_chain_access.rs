//! Authorization for the existing privileged audit metadata transport.
//!
//! Full ledger payloads remain internal. These aliases exercise existing
//! sealed Snowball declarations; they do not create a public audit policy.
use std::{pin::Pin, sync::Arc};

use futures::Stream;
use tonic::{Request, Status};

use crate::{
    grpc_server::{DECLARED_CAPABILITY_HEADER, proto_chain_event},
    interceptor::load_capability_grants,
    method_contract_admission::admit_compiled_invocation,
    mutation_engine::MutationEngine,
    oracle_assertion::HumanPrincipalIdentity,
    proto::{ChainEvent, GetEventsRequest, GetEventsResponse, SubscribeEventsRequest},
};

#[derive(Clone, Copy)]
pub(crate) enum AuditMethod {
    QueryEvents,
    VerifyChain,
    GetSnapshot,
    CreateSnapshot,
}

impl AuditMethod {
    fn canonical(self) -> &'static str {
        match self {
            Self::QueryEvents => "query_events",
            Self::VerifyChain => "verify_chain",
            Self::GetSnapshot => "get_snapshot",
            Self::CreateSnapshot => "create_snapshot",
        }
    }
}

pub(crate) struct AuditAccess {
    identity: HumanPrincipalIdentity,
    method: AuditMethod,
    capability: String,
}

/// Only the strict SID1/OIA1 interceptor authors this extension. Legacy
/// genesis/trace metadata is deliberately insufficient for audit access.
pub(crate) async fn verified_identity<T>(
    engine: &MutationEngine,
    request: &Request<T>,
) -> Result<HumanPrincipalIdentity, Status> {
    let identity = request
        .extensions()
        .get::<HumanPrincipalIdentity>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("verified SID1/OIA1 identity is required"))?;
    validate_identity(engine, &identity).await?;
    Ok(identity)
}

pub(crate) async fn validate_identity(
    engine: &MutationEngine,
    identity: &HumanPrincipalIdentity,
) -> Result<(), Status> {
    if identity.expires_at != 0 && identity.expires_at <= chrono::Utc::now().timestamp() {
        return Err(Status::unauthenticated("identity credential has expired"));
    }
    crate::project_dispatch::verified_session(
        engine,
        &identity.principal_id,
        Some(&identity.session_id),
        Some(&identity.session_genesis),
    )
    .await
    .map_err(|_| Status::unauthenticated("principal/session is no longer valid"))?;
    Ok(())
}

impl AuditAccess {
    pub(crate) async fn admit<T>(
        engine: &MutationEngine,
        request: &Request<T>,
        method: AuditMethod,
    ) -> Result<Self, Status> {
        let identity = verified_identity(engine, request).await?;
        let declared: Vec<_> = request
            .metadata()
            .get_all(DECLARED_CAPABILITY_HEADER)
            .into_iter()
            .collect();
        if declared.len() != 1 {
            return Err(Status::permission_denied(
                "one exact audit capability declaration is required",
            ));
        }
        let capability = declared[0]
            .to_str()
            .map_err(|_| Status::permission_denied("invalid audit capability declaration"))?
            .to_owned();
        let access = Self {
            identity,
            method,
            capability,
        };
        access.check_contract_and_grant()?;
        Ok(access)
    }

    // Keep the native tonic status used by the surrounding RPC boundary.
    #[allow(clippy::result_large_err)]
    fn check_contract_and_grant(&self) -> Result<(), Status> {
        if !load_capability_grants(&self.identity.principal_id).contains(&self.capability) {
            return Err(Status::permission_denied(
                "exact-principal audit capability grant is required",
            ));
        }
        // Same pinned method supplies compatibility evidence and its exact
        // required capability. The compiled registry is never existence authority.
        admit_compiled_invocation("snowball", self.method.canonical(), Some(&self.capability))
            .map_err(|_| Status::permission_denied("present Snowball method must match the exercised audit capability and compiled contract"))?;
        Ok(())
    }

    pub(crate) async fn revalidate(&self, engine: &MutationEngine) -> Result<(), Status> {
        validate_identity(engine, &self.identity).await?;
        self.check_contract_and_grant()
    }
}

const PAGE_LIMIT: usize = 100;
const REPLAY_LIMIT: usize = 1000;

// This RPC helper returns tonic's concrete status, preserving code/details.
#[allow(clippy::result_large_err)]
pub(crate) fn get_events(
    events: &[op_state_store::ChainEvent],
    request: &GetEventsRequest,
) -> Result<GetEventsResponse, Status> {
    if request.to_event_id != 0 && request.from_event_id > request.to_event_id {
        return Err(Status::invalid_argument("audit event range is reversed"));
    }
    if !matches!(request.decision_filter, 0..=2) {
        return Err(Status::invalid_argument("unknown audit decision filter"));
    }
    let limit = if request.limit == 0 {
        50
    } else {
        request.limit as usize
    }
    .min(PAGE_LIMIT);
    let mut found = events
        .iter()
        .filter(|event| request.from_event_id == 0 || event.event_id >= request.from_event_id)
        .filter(|event| request.to_event_id == 0 || event.event_id <= request.to_event_id)
        .filter(|event| matches_filters(event, &request.plugin_id, &request.tags))
        .filter(|event| match request.decision_filter {
            1 => event.decision == op_state_store::Decision::Allow,
            2 => event.decision == op_state_store::Decision::Deny,
            _ => true,
        });
    let page = found.by_ref().take(limit).map(proto_chain_event).collect();
    let has_more = found.next().is_some();
    Ok(GetEventsResponse {
        events: page,
        has_more,
    })
}

fn matches_filters(event: &op_state_store::ChainEvent, plugin: &str, tags: &[String]) -> bool {
    (plugin.is_empty() || event.plugin_id == plugin)
        && (tags.is_empty() || event.tags_touched.iter().any(|tag| tags.contains(tag)))
}

/// Read ordered ledger metadata after a wakeup. Broadcast payloads are never
/// trusted for ordering: concurrent appenders can send notifications out of order.
#[allow(clippy::result_large_err)] // Native tonic error contract for the stream.
fn replay_after(
    events: &[op_state_store::ChainEvent],
    cursor: u64,
    plugin: &str,
    tags: &[String],
) -> Result<(u64, Vec<ChainEvent>), Status> {
    let head = events.last().map_or(0, |event| event.event_id);
    if let Some(first) = events.first()
        && first.event_id.saturating_sub(1) > cursor
    {
        return Err(Status::out_of_range(
            "requested audit history is no longer retained",
        ));
    }
    let pending = events.iter().filter(|event| event.event_id > cursor);
    let mut count = 0;
    let mut result = Vec::new();
    for event in pending {
        count += 1;
        if count > REPLAY_LIMIT {
            return Err(Status::resource_exhausted(
                "audit replay exceeds 1000 events; page GetEvents then resume from the next event id",
            ));
        }
        if matches_filters(event, plugin, tags) {
            result.push(proto_chain_event(event));
        }
    }
    Ok((head.max(cursor), result))
}

pub(crate) type AuditStream = Pin<Box<dyn Stream<Item = Result<ChainEvent, Status>> + Send>>;

pub(crate) async fn subscribe(
    engine: Arc<MutationEngine>,
    request: SubscribeEventsRequest,
    access: AuditAccess,
) -> Result<AuditStream, Status> {
    // Subscribe before taking the replay snapshot to close the handoff gap.
    let mut receiver = engine.chain_tx().subscribe();
    let (mut cursor, initial) = {
        let chain = engine.event_chain.read().await;
        let head = chain.events().last().map_or(0, |event| event.event_id);
        if request.from_event_id == 0 {
            (head, Vec::new())
        } else {
            let cursor = request.from_event_id - 1;
            if cursor > head {
                return Err(Status::out_of_range(
                    "requested audit cursor is beyond the current chain",
                ));
            }
            replay_after(chain.events(), cursor, &request.plugin_id, &request.tags)?
        }
    };
    Ok(Box::pin(async_stream::stream! {
        let mut pending = initial;
        loop {
            for event in pending.drain(..) {
                if let Err(error) = access.revalidate(&engine).await {
                    yield Err(error); return;
                }
                yield Ok(event);
            }
            match receiver.recv().await {
                Ok(_) => {
                    if let Err(error) = access.revalidate(&engine).await {
                        yield Err(error); return;
                    }
                    let next = {
                        let chain = engine.event_chain.read().await;
                        replay_after(chain.events(), cursor, &request.plugin_id, &request.tags)
                    };
                    match next {
                        Ok((head, events)) => { cursor = head; pending = events; }
                        Err(error) => { yield Err(error); return; }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    yield Err(Status::out_of_range("audit notification history lagged; page GetEvents and resume explicitly"));
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state_store::{Decision, OperationType};

    fn event(id: u64, plugin: &str) -> op_state_store::ChainEvent {
        op_state_store::ChainEvent::new(
            id,
            "prior".into(),
            "actor".into(),
            plugin.into(),
            "2.0.0".into(),
            OperationType::MethodCall,
            "private-target".into(),
            vec!["tag".into()],
            Decision::Allow,
            &simd_json::json!({"private_fixture":"never-on-audit-wire"}),
        )
    }

    #[test]
    fn bounded_pages_preserve_ledger_and_metadata_only() {
        let events: Vec<_> = (1..=103).map(|id| event(id, "demo")).collect();
        let input_before = events[0].input_payload.clone();
        let result = get_events(&events, &GetEventsRequest::default()).unwrap();
        assert_eq!(result.events.len(), 50);
        assert!(result.has_more);
        let result = get_events(
            &events,
            &GetEventsRequest {
                limit: u32::MAX,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.events.len(), 100);
        assert!(result.has_more);
        let result = get_events(
            &events,
            &GetEventsRequest {
                from_event_id: 100,
                limit: 4,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.events.len(), 4);
        assert!(!result.has_more);
        let bytes = prost::Message::encode_to_vec(&result);
        assert!(
            !bytes
                .windows(b"never-on-audit-wire".len())
                .any(|bytes| bytes == b"never-on-audit-wire")
        );
        assert_eq!(events[0].input_payload, input_before);
        assert_eq!(events[0].event_hash, events[0].compute_hash());
    }

    #[test]
    fn replay_orders_by_ledger_and_advances_across_filtered_events() {
        let events = vec![event(1, "demo"), event(2, "other"), event(3, "demo")];
        let (cursor, replay) = replay_after(&events, 1, "demo", &[]).unwrap();
        assert_eq!(cursor, 3);
        assert_eq!(
            replay
                .iter()
                .map(|event| event.event_id)
                .collect::<Vec<_>>(),
            [3]
        );
        assert!(
            replay_after(&events, cursor, "demo", &[])
                .unwrap()
                .1
                .is_empty()
        );
        assert!(
            replay_after(&events, 0, "demo", &["missing".into()])
                .unwrap()
                .1
                .is_empty()
        );
    }

    #[test]
    fn replay_rejects_lost_history_and_excessive_backlog() {
        assert_eq!(
            replay_after(&[event(3, "demo")], 0, "", &[])
                .unwrap_err()
                .code(),
            tonic::Code::OutOfRange
        );
        let events: Vec<_> = (1..=1001).map(|id| event(id, "demo")).collect();
        assert_eq!(
            replay_after(&events, 0, "", &[]).unwrap_err().code(),
            tonic::Code::ResourceExhausted
        );
        assert_eq!(
            get_events(
                &events,
                &GetEventsRequest {
                    from_event_id: 5,
                    to_event_id: 2,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code(),
            tonic::Code::InvalidArgument
        );
    }
}
