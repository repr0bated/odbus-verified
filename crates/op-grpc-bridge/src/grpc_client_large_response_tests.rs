//! Loopback transport coverage for the deliberately bounded large-response
//! client configuration. These fixtures test framing only; they provide no
//! authentication or authorization semantics.

use super::*;
use futures::Stream;
use std::{pin::Pin, sync::Arc};
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

use crate::proto::{
    CallMethodResponse, GetPropertyResponse, GetSchemaResponse, GetStateResponse,
    ListPluginsResponse, MutateResponse, SetPropertyResponse, Signal, StateChange,
    SubscribeRequest, SubscribeSignalsRequest,
    plugin_service_server::{PluginService, PluginServiceServer},
    state_sync_server::{StateSync, StateSyncServer},
};

const LARGE: usize = 5 * 1024 * 1024;
type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[derive(Clone)]
struct LargeResponseService {
    payload: Arc<String>,
}

#[tonic::async_trait]
impl PluginService for LargeResponseService {
    async fn list_plugins(&self, _: Request<()>) -> Result<Response<ListPluginsResponse>, Status> {
        Ok(Response::new(ListPluginsResponse::default()))
    }

    async fn get_schema(
        &self,
        _: Request<crate::proto::GetSchemaRequest>,
    ) -> Result<Response<GetSchemaResponse>, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn call_method(
        &self,
        _: Request<crate::proto::CallMethodRequest>,
    ) -> Result<Response<CallMethodResponse>, Status> {
        Ok(Response::new(CallMethodResponse {
            success: true,
            result: Some(prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(
                    self.payload.as_str().to_owned(),
                )),
            }),
            ..Default::default()
        }))
    }

    async fn get_property(
        &self,
        _: Request<crate::proto::GetPropertyRequest>,
    ) -> Result<Response<GetPropertyResponse>, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn set_property(
        &self,
        _: Request<crate::proto::SetPropertyRequest>,
    ) -> Result<Response<SetPropertyResponse>, Status> {
        Err(Status::unimplemented("fixture"))
    }

    type SubscribeSignalsStream = BoxStream<Signal>;

    async fn subscribe_signals(
        &self,
        _: Request<SubscribeSignalsRequest>,
    ) -> Result<Response<Self::SubscribeSignalsStream>, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

#[tonic::async_trait]
impl StateSync for LargeResponseService {
    type SubscribeStream = BoxStream<StateChange>;

    async fn subscribe(
        &self,
        _: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let frame = StateChange {
            new_value: Some(prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(
                    self.payload.as_str().to_owned(),
                )),
            }),
            ..Default::default()
        };
        Ok(Response::new(Box::pin(tokio_stream::iter([Ok(frame)]))))
    }

    async fn mutate(
        &self,
        _: Request<crate::proto::MutateRequest>,
    ) -> Result<Response<MutateResponse>, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn get_state(
        &self,
        _: Request<crate::proto::GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        Ok(Response::new(GetStateResponse {
            value: Some(prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(
                    self.payload.as_str().to_owned(),
                )),
            }),
            ..Default::default()
        }))
    }

    async fn batch_mutate(
        &self,
        _: Request<crate::proto::BatchMutateRequest>,
    ) -> Result<Response<crate::proto::BatchMutateResponse>, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

#[tokio::test]
async fn pool_decodes_large_plugin_and_state_unary_and_stream_responses() {
    let payload = Arc::new("x".repeat(LARGE));
    let service = LargeResponseService {
        payload: payload.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(PluginServiceServer::new(service.clone()))
            .add_service(StateSyncServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    let pool = GrpcClientPool::new();
    let mut plugin = pool
        .plugin_service_client(&format!("http://{address}"))
        .await
        .unwrap();
    let plugin_response = plugin
        .call_method(Request::new(crate::proto::CallMethodRequest::default()))
        .await
        .unwrap()
        .into_inner();
    let plugin_payload = match plugin_response.result.unwrap().kind.unwrap() {
        prost_types::value::Kind::StringValue(value) => value,
        other => panic!("unexpected plugin response kind: {other:?}"),
    };
    assert_eq!(plugin_payload, *payload);

    let mut state = pool
        .state_sync_client(&format!("http://{address}"))
        .await
        .unwrap();
    let state_response = state
        .get_state(Request::new(crate::proto::GetStateRequest::default()))
        .await
        .unwrap()
        .into_inner();
    let state_payload = match state_response.value.unwrap().kind.unwrap() {
        prost_types::value::Kind::StringValue(value) => value,
        other => panic!("unexpected state response kind: {other:?}"),
    };
    assert_eq!(state_payload, *payload);

    let mut stream = state
        .subscribe(Request::new(SubscribeRequest::default()))
        .await
        .unwrap()
        .into_inner();
    let frame = stream.next().await.unwrap().unwrap();
    let stream_payload = match frame.new_value.unwrap().kind.unwrap() {
        prost_types::value::Kind::StringValue(value) => value,
        other => panic!("unexpected stream response kind: {other:?}"),
    };
    assert_eq!(stream_payload, *payload);
    server.abort();
}
