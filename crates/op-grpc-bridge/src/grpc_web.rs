//! gRPC-Web enablement with Oracle identity metadata in the CORS
//! preflight allow-list.
//!
//! `tonic_web::enable` hard-codes its preflight `access-control-allow-headers`
//! to `x-grpc-web, content-type, x-user-agent, grpc-timeout`, and its internal
//! CORS layer is the one that actually answers browser preflights (the outer
//! transport-level `CorsLayer` never sees them). This mirrors
//! `tonic_web::enable`, adds the canonical OIA1/capability metadata headers,
//! and replaces reflected origins with the bridge's exact configured allowlist.

use std::task::{Context, Poll};
use std::time::Duration;

use tonic::body::BoxBody;
use tonic::codegen::http::{HeaderName, Request, Response};
use tonic::codegen::Service;
use tonic::server::NamedService;
use tonic_web::{GrpcWebLayer, GrpcWebService};
use tower::Layer;
use tower_http::cors::{Cors, CorsLayer};

const MAX_AGE: Duration = Duration::from_secs(86400);

const ALLOW_HEADERS: [HeaderName; 6] = [
    HeaderName::from_static("x-grpc-web"),
    HeaderName::from_static("content-type"),
    HeaderName::from_static("x-user-agent"),
    HeaderName::from_static("grpc-timeout"),
    HeaderName::from_static(crate::interceptor::ASSERTION_METADATA_KEY),
    HeaderName::from_static(crate::grpc_server::DECLARED_CAPABILITY_HEADER),
];

const EXPOSE_HEADERS: [HeaderName; 3] = [
    HeaderName::from_static("grpc-status"),
    HeaderName::from_static("grpc-message"),
    HeaderName::from_static("grpc-status-details-bin"),
];

/// Drop-in replacement for `tonic_web::enable` with exact-origin OIA1 CORS.
pub fn enable<S>(service: S) -> GhostCorsGrpcWeb<S>
where
    S: Service<Request<BoxBody>, Response = Response<BoxBody>>,
    S: Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let cors = CorsLayer::new()
        .allow_origin(crate::mcp_frontend::configured_allow_origin())
        .allow_credentials(true)
        .max_age(MAX_AGE)
        .expose_headers(EXPOSE_HEADERS.to_vec())
        .allow_headers(ALLOW_HEADERS.to_vec());

    GhostCorsGrpcWeb(cors.layer(GrpcWebLayer::new().layer(service)))
}

/// Newtype so the CORS-wrapped service still forwards [`NamedService`], same
/// role as tonic-web's own `CorsGrpcWeb`.
#[derive(Debug, Clone)]
pub struct GhostCorsGrpcWeb<S>(Cors<GrpcWebService<S>>);

impl<S> Service<Request<BoxBody>> for GhostCorsGrpcWeb<S>
where
    S: Service<Request<BoxBody>, Response = Response<BoxBody>>,
    S: Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = <Cors<GrpcWebService<S>> as Service<Request<BoxBody>>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
        self.0.call(req)
    }
}

impl<S> NamedService for GhostCorsGrpcWeb<S>
where
    S: NamedService,
{
    const NAME: &'static str = S::NAME;
}
