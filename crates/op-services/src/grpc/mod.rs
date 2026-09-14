//! gRPC server

pub mod server;

pub mod proto {
    tonic::include_proto!("opdbus.services.v1");
}
