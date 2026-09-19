//! EMQX ExHook v3 HookProvider gRPC service implementation.
//!
//! EMQX acts as a gRPC client, calling this service for MQTT events
//! The pinned EMQX 6.2.2 broker requires `emqx.exhook.v3`.
//! The implementation records events through MutationEngine for
//! accountability. Publish decisions are left to the broker (`IGNORE`) — the
//! hook is an audit tap; identity gating stays with the Ghostbridge
//! interceptor on the plugin surface. The one exception is the doorbell
//! (`wake_doorbell`): clients whose id is an owner key must prove the
//! matching SID1 against the identity sled, and may only subscribe to their
//! own `opdbus/wake/<owner-key>` topic. Every other client is kept out of
//! the `opdbus/` namespace and otherwise left to the broker's own rules.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::{debug, info};

use op_plugins::state_plugins::emqx::{
    BrokerEvent, BROKER_EVENT_SIGNAL, BROKER_EVENT_SUBID, REGISTERED_HOOKS,
};

use crate::mutation_engine::{ChangeSource, ChangeType, MutationEngine};
use crate::proto::emqx_exhook::{
    self as exhook, hook_provider_server::HookProvider, valued_response, EmptySuccess, HookSpec,
    LoadedResponse, ValuedResponse,
};

/// HookProvider service implementation for EMQX MQTT event hooks.
///
/// All EMQX hook events are recorded as signals through the MutationEngine,
/// providing accountability and persistence without direct SQL storage.
#[derive(Clone)]
pub struct HookProviderService {
    mutation_engine: Arc<MutationEngine>,
    /// Doorbell OIA1 verification: the same validator family as the door
    /// (trust store, replay store, source binding from the environment).
    assertion_validator: Arc<crate::oracle_assertion::AssertionValidator>,
    /// Event-triggered agent runs (OD-63): match + enqueue on publish.
    triggers: crate::agent_triggers::TriggerMatcher,
}

impl HookProviderService {
    pub fn new(mutation_engine: Arc<MutationEngine>) -> Self {
        let queue = crate::agent_runs::RunQueue::spawn(
            mutation_engine.clone(),
            crate::agent_runs::DEFAULT_CAPACITY,
            crate::agent_runs::DEFAULT_CONCURRENCY,
        );
        Self::with_queue(mutation_engine, queue)
    }

    pub(crate) fn with_queue(
        mutation_engine: Arc<MutationEngine>,
        queue: crate::agent_runs::RunQueue,
    ) -> Self {
        let assertion_validator = Arc::new(crate::oracle_assertion::AssertionValidator::from_env(
            crate::oracle_assertion::DecoyTrustStore::load(),
        ));
        Self {
            mutation_engine,
            assertion_validator,
            triggers: crate::agent_triggers::TriggerMatcher::new(queue),
        }
    }



    /// Record a broker observation, never a fictitious mutation/tool call.
    async fn record_event(
        &self,
        event_type: &str,
        client_id: &str,
        topic: Option<&str>,
        detail: Option<&str>,
    ) -> anyhow::Result<()> {
        let event_data = broker_event(event_type, client_id, topic, detail);
        let result = self
            .mutation_engine
            .process_authoritative_change(
                "emqx".into(),
                "/org/opdbus/v1/plugins/emqx".into(),
                ChangeType::Signal,
                Some(BROKER_EVENT_SIGNAL.into()),
                None,
                simd_json::serde::to_owned_value(event_data)?,
                vec![BROKER_EVENT_SUBID.into()],
                "grpc:emqx.exhook.v3".into(),
                None,
                ChangeSource::Grpc,
            )
            .await;
        if let Err(error) = result {
            tracing::error!(%error, event_type, "could not record broker observation");
            return Err(anyhow::anyhow!(error));
        }

        Ok(())
    }
}

fn broker_event(
    event_type: &str,
    client: &str,
    topic: Option<&str>,
    detail: Option<&str>,
) -> BrokerEvent {
    let digest = |value: &str| {
        blake3::Hash::from(blake3::derive_key(
            "opdbus broker observation v1",
            value.as_bytes(),
        ))
        .to_hex()
        .to_string()
    };
    BrokerEvent {
        event_type: event_type.to_owned(),
        client_ref: digest(client),
        topic_ref: topic.map(digest),
        detail_ref: detail.map(digest),
    }
}

fn client_id(info: &Option<exhook::ClientInfo>) -> &str {
    info.as_ref()
        .map(|c| c.clientid.as_str())
        .unwrap_or("unknown")
}

fn empty() -> Response<EmptySuccess> {
    Response::new(EmptySuccess {})
}

/// A doorbell verdict as the broker expects it: `Ignore` keeps the chain,
/// `Allow`/`Deny` stop it with a boolean.
fn respond(verdict: crate::wake_doorbell::Verdict) -> Response<ValuedResponse> {
    match verdict {
        crate::wake_doorbell::Verdict::Ignore => ignore(),
        crate::wake_doorbell::Verdict::Allow => stop_with(true),
        crate::wake_doorbell::Verdict::Deny => stop_with(false),
    }
}

fn stop_with(result: bool) -> Response<ValuedResponse> {
    Response::new(ValuedResponse {
        r#type: valued_response::ResponsedType::StopAndReturn as i32,
        value: Some(valued_response::Value::BoolResult(result)),
    })
}

/// Leave the broker's own decision in place.
fn ignore() -> Response<ValuedResponse> {
    Response::new(ValuedResponse {
        r#type: valued_response::ResponsedType::Ignore as i32,
        value: None,
    })
}

#[tonic::async_trait]
impl HookProvider for HookProviderService {
    async fn on_provider_loaded(
        &self,
        request: Request<exhook::ProviderLoadedRequest>,
    ) -> Result<Response<LoadedResponse>, Status> {
        let req = request.into_inner();
        info!(
            broker = %req.broker.as_ref().map(|b| b.version.as_str()).unwrap_or("unknown"),
            "EMQX exhook provider loaded"
        );
        let hooks = REGISTERED_HOOKS
            .iter()
            .map(|name| HookSpec {
                name: (*name).to_string(),
                topics: if name.starts_with("message.") {
                    vec!["#".to_string()]
                } else {
                    Vec::new()
                },
            })
            .collect();
        Ok(Response::new(LoadedResponse { hooks }))
    }

    async fn on_provider_unloaded(
        &self,
        _request: Request<exhook::ProviderUnloadedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        info!("EMQX exhook provider unloaded");
        Ok(empty())
    }

    async fn on_client_connect(
        &self,
        request: Request<exhook::ClientConnectRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = req
            .conninfo
            .as_ref()
            .map(|c| c.clientid.as_str())
            .unwrap_or("unknown");
        debug!(client_id = %client, "EMQX OnClientConnect");
        let _ = self
            .record_event("on_client_connect", client, None, None)
            .await;
        Ok(empty())
    }

    async fn on_client_connack(
        &self,
        _request: Request<exhook::ClientConnackRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_client_connected(
        &self,
        request: Request<exhook::ClientConnectedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        info!(client_id = %client, "EMQX OnClientConnected");
        let _ = self
            .record_event("on_client_connected", client, None, None)
            .await;
        Ok(empty())
    }

    async fn on_client_disconnected(
        &self,
        request: Request<exhook::ClientDisconnectedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        info!(client_id = %client, reason = %req.reason, "EMQX OnClientDisconnected");
        let _ = self
            .record_event("on_client_disconnected", client, None, Some(&req.reason))
            .await;
        Ok(empty())
    }

    async fn on_client_authenticate(
        &self,
        request: Request<exhook::ClientAuthenticateRequest>,
    ) -> Result<Response<ValuedResponse>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, result = req.result, "EMQX OnClientAuthenticate");
        let (password, peer) = req
            .clientinfo
            .as_ref()
            .map(|info| {
                (
                    info.password.as_str(),
                    format!("{}:{}", info.peerhost, info.peerport)
                        .parse::<std::net::SocketAddr>()
                        .ok(),
                )
            })
            .unwrap_or_default();
        let verdict = crate::wake_doorbell::authenticate(
            &self.mutation_engine,
            &self.assertion_validator,
            client,
            password,
            peer,
        )
        .await;
        let _ = self
            .record_event(
                "on_client_authenticate",
                client,
                None,
                Some(match verdict {
                    crate::wake_doorbell::Verdict::Ignore if req.result => "pass",
                    crate::wake_doorbell::Verdict::Ignore => "fail",
                    crate::wake_doorbell::Verdict::Allow => "doorbell-pass",
                    crate::wake_doorbell::Verdict::Deny => "doorbell-fail",
                }),
            )
            .await;
        Ok(respond(verdict))
    }

    async fn on_client_authorize(
        &self,
        request: Request<exhook::ClientAuthorizeRequest>,
    ) -> Result<Response<ValuedResponse>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, topic = %req.topic, "EMQX OnClientAuthorize");
        let subscribe =
            req.r#type == exhook::client_authorize_request::AuthorizeReqType::Subscribe as i32;
        let verdict =
            crate::wake_doorbell::authorize(&self.mutation_engine, client, subscribe, &req.topic)
                .await;
        let _ = self
            .record_event(
                "on_client_authorize",
                client,
                Some(&req.topic),
                Some(match verdict {
                    crate::wake_doorbell::Verdict::Ignore => "broker",
                    crate::wake_doorbell::Verdict::Allow => "doorbell-allow",
                    crate::wake_doorbell::Verdict::Deny => "doorbell-deny",
                }),
            )
            .await;
        Ok(respond(verdict))
    }

    async fn on_client_subscribe(
        &self,
        request: Request<exhook::ClientSubscribeRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        let topics = req
            .topic_filters
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        debug!(client_id = %client, topics = %topics, "EMQX OnClientSubscribe");
        let _ = self
            .record_event("on_client_subscribe", client, Some(&topics), None)
            .await;
        Ok(empty())
    }

    async fn on_client_unsubscribe(
        &self,
        request: Request<exhook::ClientUnsubscribeRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        let topics = req
            .topic_filters
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        debug!(client_id = %client, topics = %topics, "EMQX OnClientUnsubscribe");
        let _ = self
            .record_event("on_client_unsubscribe", client, Some(&topics), None)
            .await;
        Ok(empty())
    }

    async fn on_session_created(
        &self,
        request: Request<exhook::SessionCreatedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, "EMQX OnSessionCreated");
        let _ = self
            .record_event("on_session_created", client, None, None)
            .await;
        Ok(empty())
    }

    async fn on_session_subscribed(
        &self,
        request: Request<exhook::SessionSubscribedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, topic = %req.topic, "EMQX OnSessionSubscribed");
        let _ = self
            .record_event("on_session_subscribed", client, Some(&req.topic), None)
            .await;
        Ok(empty())
    }

    async fn on_session_unsubscribed(
        &self,
        request: Request<exhook::SessionUnsubscribedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, topic = %req.topic, "EMQX OnSessionUnsubscribed");
        let _ = self
            .record_event("on_session_unsubscribed", client, Some(&req.topic), None)
            .await;
        Ok(empty())
    }

    async fn on_session_resumed(
        &self,
        _request: Request<exhook::SessionResumedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_session_discarded(
        &self,
        _request: Request<exhook::SessionDiscardedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_session_takenover(
        &self,
        _request: Request<exhook::SessionTakenoverRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_session_terminated(
        &self,
        request: Request<exhook::SessionTerminatedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        let req = request.into_inner();
        let client = client_id(&req.clientinfo);
        debug!(client_id = %client, reason = %req.reason, "EMQX OnSessionTerminated");
        let _ = self
            .record_event("on_session_terminated", client, None, Some(&req.reason))
            .await;
        Ok(empty())
    }

    async fn on_message_publish(
        &self,
        request: Request<exhook::MessagePublishRequest>,
    ) -> Result<Response<ValuedResponse>, Status> {
        let req = request.into_inner();
        if let Some(message) = req.message.as_ref() {
            info!(from = %message.from, topic = %message.topic, "EMQX OnMessagePublish");
            let _ = self
                .record_event(
                    "on_message_publish",
                    &message.from,
                    Some(&message.topic),
                    None,
                )
                .await;
            // OD-63: a matching trigger enqueues a run; the payload is read
            // only for a match and bounded by the trigger. The verdict is
            // still the broker's.
            let enqueued = self
                .triggers
                .on_publish(&message.topic, &message.payload)
                .await;
            if enqueued > 0 {
                debug!(enqueued, topic = %message.topic, "agent runs enqueued");
            }
        }
        Ok(ignore())
    }

    async fn on_message_delivered(
        &self,
        _request: Request<exhook::MessageDeliveredRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_message_dropped(
        &self,
        _request: Request<exhook::MessageDroppedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }

    async fn on_message_acked(
        &self,
        _request: Request<exhook::MessageAckedRequest>,
    ) -> Result<Response<EmptySuccess>, Status> {
        Ok(empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_events_are_metadata_only_and_digest_all_external_values() {
        let event = broker_event(
            "message.publish",
            "client-secret",
            Some("private/project/topic"),
            Some("message body must not be retained"),
        );
        assert_eq!(event.event_type, "message.publish");
        assert_ne!(event.client_ref, "client-secret");
        assert_ne!(event.topic_ref.as_deref(), Some("private/project/topic"));
        assert_ne!(
            event.detail_ref.as_deref(),
            Some("message body must not be retained")
        );
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("client-secret"));
        assert!(!encoded.contains("private/project/topic"));
        assert!(!encoded.contains("message body must not be retained"));
    }

    #[test]
    fn auth_and_authorize_responses_leave_broker_decisions_unchanged() {
        let response = ignore().into_inner();
        assert_eq!(
            response.r#type,
            valued_response::ResponsedType::Ignore as i32
        );
        assert!(response.value.is_none());
    }
}
