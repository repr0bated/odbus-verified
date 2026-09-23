//! gRPC server

pub mod auth;
pub mod server;

pub mod proto {
    tonic::include_proto!("opdbus.services.v1");
}
