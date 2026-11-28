//! Preview interop with the official grpc-rust crate (`grpc` 0.9, github.com/grpc/grpc-rust).
//!
//! Enabled with the `grpc-preview` feature. grpc 0.9 is a preview release with a
//! client-only public API (`grpc::client::Channel` + the `Invoke` trait); its server
//! module is crate-private, and its own code generator targets Google's protobuf v4
//! crate rather than prost. So the bridge keeps serving with tonic 0.14, and this
//! module only lets a grpc `Channel` call any tonic-served method using the existing
//! prost message types (no extra codegen). See docs/grpc-migration-plan.md.

use bytes::Buf;
use grpc::client::{
    CallOptions, Channel, ChannelOptions, Invoke, RecvStream, ResponseStreamItem, SendOptions,
    SendStream,
};
use grpc::core::{RecvMessage, RequestHeaders, SendMessage};
use grpc::credentials::LocalChannelCredentials;

/// Error from a grpc-preview call.
#[derive(Debug, thiserror::Error)]
pub enum GrpcPreviewError {
    #[error("grpc send failed: stream closed before the request was delivered")]
    SendClosed,
    #[error("grpc status {code:?}: {message}")]
    Status { code: grpc::StatusCodeError, message: String },
    #[error("grpc response stream closed without a message")]
    NoMessage,
}

/// Wraps a prost message so grpc can encode it.
pub struct ProstSend<'a, M>(pub &'a M);

impl<M: prost::Message> SendMessage for ProstSend<'_, M> {
    fn encode(&self) -> Result<Box<dyn Buf + Send + Sync>, String> {
        Ok(Box::new(bytes::Bytes::from(self.0.encode_to_vec())))
    }
}

/// Receives one prost message from grpc.
pub struct ProstRecv<M>(pub Option<M>);

impl<M: prost::Message + Default> RecvMessage for ProstRecv<M> {
    fn decode(&mut self, data: &mut dyn Buf) -> Result<(), String> {
        let mut buf = Vec::with_capacity(data.remaining());
        while data.has_remaining() {
            let chunk = data.chunk();
            let n = chunk.len();
            buf.extend_from_slice(chunk);
            data.advance(n);
        }
        self.0 = Some(M::decode(buf.as_slice()).map_err(|e| e.to_string())?);
        Ok(())
    }
}

/// Opens a plaintext grpc channel for same-host targets such as
/// `dns:///127.0.0.1:50051` or `unix:///run/op-dbus/bridge.sock`.
pub fn local_channel(target: &str) -> Channel {
    Channel::new(target, LocalChannelCredentials::new_arc(), ChannelOptions::default())
}

/// Performs a unary call against a tonic-served method.
/// `method` is the full gRPC path, e.g. `/grpc.health.v1.Health/Check`.
pub async fn unary<Req, Resp>(
    channel: &Channel,
    method: &str,
    request: &Req,
) -> Result<Resp, GrpcPreviewError>
where
    Req: prost::Message,
    Resp: prost::Message + Default,
{
    let headers = RequestHeaders::new().with_method_name(method);
    let (mut tx, mut rx) = channel.invoke(headers, CallOptions::default()).await;
    tx.send(&ProstSend(request), SendOptions::new().with_final_msg(true))
        .await
        .map_err(|()| GrpcPreviewError::SendClosed)?;
    let mut msg = ProstRecv::<Resp>(None);
    loop {
        match rx.recv(&mut msg).await {
            ResponseStreamItem::Headers(_) | ResponseStreamItem::Message => {}
            ResponseStreamItem::Trailers(trailers) => {
                if let Err(e) = trailers.status() {
                    return Err(GrpcPreviewError::Status {
                        code: e.code(),
                        message: e.message().to_string(),
                    });
                }
            }
            ResponseStreamItem::StreamClosed => break,
        }
    }
    msg.0.ok_or(GrpcPreviewError::NoMessage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic_health::pb::{health_check_response::ServingStatus, HealthCheckRequest, HealthCheckResponse};

    /// A grpc 0.9 Channel calls a tonic 0.14 server (tonic-health) using prost types.
    #[tokio::test]
    async fn grpc_channel_calls_tonic_health_server() {
        let (reporter, health) = tonic_health::server::health_reporter();
        reporter.set_service_status("op.preview", tonic_health::ServingStatus::Serving).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(health)
                .serve_with_incoming(incoming),
        );

        let channel = local_channel(&format!("dns:///{addr}"));
        let resp: HealthCheckResponse = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            unary(&channel, "/grpc.health.v1.Health/Check", &HealthCheckRequest { service: "op.preview".into() }),
        )
        .await
        .expect("grpc call timed out")
        .expect("grpc call failed");
        assert_eq!(resp.status, ServingStatus::Serving as i32);

        let err = unary::<_, HealthCheckResponse>(
            &channel,
            "/grpc.health.v1.Health/Check",
            &HealthCheckRequest { service: "missing".into() },
        )
        .await
        .expect_err("unknown service must fail");
        assert!(matches!(err, GrpcPreviewError::Status { code: grpc::StatusCodeError::NotFound, .. }), "{err:?}");
        server.abort();
    }
}
