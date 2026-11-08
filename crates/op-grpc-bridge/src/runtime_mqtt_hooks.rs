//! Native callbacks advertised to the retained EMQX v3 provider.
//! Only implemented connection/admission hooks are registered. Unregistered
//! broker observation callbacks are explicitly unavailable, not fake audit taps.

use std::sync::Arc;
use tonic::{Request, Response, Status};

use super::auth::{Sessions, Verdict};
use super::protocol::{self as hook, EmptySuccess, HookSpec, LoadedResponse, ValuedResponse};
use super::protocol::hook_provider_server::HookProvider;
use super::protocol::valued_response;

#[derive(Clone)]
pub(super) struct HookProviderService {
    sessions: Arc<Sessions>,
}

impl HookProviderService {
    pub fn new(sessions: Arc<Sessions>) -> Self { Self { sessions } }
}

fn verdict(value: Verdict) -> Response<ValuedResponse> {
    match value {
        Verdict::Ignore => Response::new(ValuedResponse {
            r#type: valued_response::ResponsedType::Ignore as i32,
            value: None,
        }),
        Verdict::Allow | Verdict::Deny => Response::new(ValuedResponse {
            r#type: valued_response::ResponsedType::StopAndReturn as i32,
            value: Some(valued_response::Value::BoolResult(value == Verdict::Allow)),
        }),
    }
}

fn empty() -> Response<EmptySuccess> { Response::new(EmptySuccess {}) }
fn unregistered() -> Status { Status::unimplemented("this ExHook callback is not registered") }

#[tonic::async_trait]
impl HookProvider for HookProviderService {
    async fn on_provider_loaded(&self, _: Request<hook::ProviderLoadedRequest>)
        -> Result<Response<LoadedResponse>, Status>
    {
        let hooks = ["client.authenticate", "client.authorize", "client.disconnected",
            "session.subscribed", "session.unsubscribed", "session.terminated"]
            .into_iter().map(|name| HookSpec { name: name.into(), topics: Vec::new() }).collect();
        Ok(Response::new(LoadedResponse { hooks }))
    }

    async fn on_provider_unloaded(&self, _: Request<hook::ProviderUnloadedRequest>)
        -> Result<Response<EmptySuccess>, Status>
    {
        self.sessions.clear().await;
        Ok(empty())
    }

    async fn on_client_authenticate(&self, request: Request<hook::ClientAuthenticateRequest>)
        -> Result<Response<ValuedResponse>, Status>
    {
        let mut request = request.into_inner();
        let Some(info) = request.clientinfo.as_mut() else { return Ok(verdict(Verdict::Deny)); };
        let decision = self.sessions.authenticate(info).await;
        // Passwords are credential-bearing, and never enter tracing or state.
        use zeroize::Zeroize;
        info.password.zeroize();
        Ok(verdict(decision))
    }

    async fn on_client_authorize(&self, request: Request<hook::ClientAuthorizeRequest>)
        -> Result<Response<ValuedResponse>, Status>
    {
        let request = request.into_inner();
        let Some(info) = request.clientinfo.as_ref() else { return Ok(verdict(Verdict::Deny)); };
        let subscribe = match hook::client_authorize_request::AuthorizeReqType::try_from(request.r#type) {
            Ok(hook::client_authorize_request::AuthorizeReqType::Subscribe) => true,
            Ok(hook::client_authorize_request::AuthorizeReqType::Publish) => false,
            Err(_) => return Ok(verdict(Verdict::Deny)),
        };
        Ok(verdict(self.sessions.authorize(info, subscribe, &request.topic).await))
    }

    async fn on_client_disconnected(&self, request: Request<hook::ClientDisconnectedRequest>)
        -> Result<Response<EmptySuccess>, Status>
    {
        if let Some(info) = request.into_inner().clientinfo {
            self.sessions.disconnect(&info).await;
        }
        Ok(empty())
    }

    async fn on_session_subscribed(&self, request: Request<hook::SessionSubscribedRequest>)
        -> Result<Response<EmptySuccess>, Status>
    {
        let request = request.into_inner();
        let info = request.clientinfo.as_ref().ok_or_else(|| Status::unauthenticated("missing MQTT client"))?;
        if op_plugins::state_plugins::emqx::valid_owner_key(&info.clientid) {
            self.sessions.subscribed(info, &request.topic, true).await
                .map_err(|_| Status::permission_denied("wake subscription is no longer admitted"))?;
        }
        Ok(empty())
    }

    async fn on_session_unsubscribed(&self, request: Request<hook::SessionUnsubscribedRequest>)
        -> Result<Response<EmptySuccess>, Status>
    {
        let request = request.into_inner();
        let info = request.clientinfo.as_ref().ok_or_else(|| Status::unauthenticated("missing MQTT client"))?;
        if op_plugins::state_plugins::emqx::valid_owner_key(&info.clientid) {
            self.sessions.subscribed(info, &request.topic, false).await
                .map_err(|_| Status::permission_denied("wake connection is no longer admitted"))?;
        }
        Ok(empty())
    }

    async fn on_session_terminated(&self, request: Request<hook::SessionTerminatedRequest>)
        -> Result<Response<EmptySuccess>, Status>
    {
        if let Some(info) = request.into_inner().clientinfo {
            self.sessions.disconnect(&info).await;
        }
        Ok(empty())
    }

    async fn on_client_connect(&self, _: Request<hook::ClientConnectRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_client_connack(&self, _: Request<hook::ClientConnackRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_client_connected(&self, _: Request<hook::ClientConnectedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_client_subscribe(&self, _: Request<hook::ClientSubscribeRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_client_unsubscribe(&self, _: Request<hook::ClientUnsubscribeRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_session_created(&self, _: Request<hook::SessionCreatedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_session_resumed(&self, _: Request<hook::SessionResumedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_session_discarded(&self, _: Request<hook::SessionDiscardedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_session_takenover(&self, _: Request<hook::SessionTakenoverRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_message_publish(&self, _: Request<hook::MessagePublishRequest>)
        -> Result<Response<ValuedResponse>, Status> { Err(unregistered()) }

    async fn on_message_delivered(&self, _: Request<hook::MessageDeliveredRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_message_dropped(&self, _: Request<hook::MessageDroppedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }

    async fn on_message_acked(&self, _: Request<hook::MessageAckedRequest>)
        -> Result<Response<EmptySuccess>, Status> { Err(unregistered()) }
}
