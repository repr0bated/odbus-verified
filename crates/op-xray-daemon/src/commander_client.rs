//! Client for xray-core's own commander API (StatsService/RoutingService/
//! LoggerService/HandlerService), dialed over the Unix socket xray binds
//! when `api.listen` is configured in its JSON config. See proto/VENDORED.md
//! for exact source provenance.
//!
//! This is unrelated to `org.opdbus.v1.Xray` (this daemon's own D-Bus
//! surface, which only manages the xray *process* lifecycle) — it talks
//! directly to the running xray-core binary's control plane.

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

// Generated cross-package references (e.g. RoutingService's use of
// xray.common.serial.TypedMessage) assume the Rust module tree mirrors the
// proto package tree exactly — hence the nesting here, not flat modules.
pub mod xray {
    pub mod app {
        pub mod stats {
            pub mod command {
                tonic::include_proto!("xray.app.stats.command");
            }
        }
        pub mod router {
            pub mod command {
                tonic::include_proto!("xray.app.router.command");
            }
        }
        pub mod log {
            pub mod command {
                tonic::include_proto!("xray.app.log.command");
            }
        }
        pub mod proxyman {
            tonic::include_proto!("xray.app.proxyman");
            pub mod command {
                tonic::include_proto!("xray.app.proxyman.command");
            }
        }
    }
    pub mod common {
        pub mod net {
            tonic::include_proto!("xray.common.net");
        }
        pub mod serial {
            tonic::include_proto!("xray.common.serial");
        }
        pub mod protocol {
            tonic::include_proto!("xray.common.protocol");
        }
    }
    pub mod core {
        tonic::include_proto!("xray.core");
    }
    pub mod proxy {
        pub mod vless {
            tonic::include_proto!("xray.proxy.vless");
            pub mod inbound {
                tonic::include_proto!("xray.proxy.vless.inbound");
            }
        }
        pub mod vmess {
            tonic::include_proto!("xray.proxy.vmess");
            pub mod inbound {
                tonic::include_proto!("xray.proxy.vmess.inbound");
            }
        }
        pub mod trojan {
            tonic::include_proto!("xray.proxy.trojan");
        }
    }
    pub mod transport {
        pub mod internet {
            tonic::include_proto!("xray.transport.internet");
            pub mod tls {
                tonic::include_proto!("xray.transport.internet.tls");
            }
        }
    }
}

pub use xray::app::log::command as logger;
pub use xray::app::proxyman::command as handler;
pub use xray::app::router::command as router;
pub use xray::app::stats::command as stats;

/// Default path xray's commander API socket binds to. Bind-mounted out of
/// the `xray` container via the existing `opdbus-rt` disk device (so this
/// path resolves the same from the host, where op-xray-daemon itself runs,
/// and from inside the container, where xray-core actually creates it) —
/// see the `api-in` inbound in the `xray` container's `xray_config.json`.
///
/// NOTE: `/run/...` (the FHS-correct location, matching every other socket
/// in this repo — `/run/opdbus`, `/run/ghostbridge`, etc.) was tried first
/// and is where this belongs. It didn't work: a fresh `disk` device at
/// `/run/xray` never actually attached to this container even after a full
/// `incus restart` (mounted as the container's own tmpfs instead) — and the
/// already-configured `opdbus-sock` device (`/run/opdbus`) has the exact
/// same problem, so this isn't specific to the new device. That's a
/// pre-existing container/incus mount-ordering issue, separate from this
/// task. `/var/lib/opdbus-runtime` is used here only because it's the one
/// bind mount confirmed to actually work on this container.
pub const DEFAULT_API_SOCKET: &str = "/var/lib/opdbus-runtime/xray-api-socket/api.sock";

async fn connect_uds(path: String) -> std::io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(path).await
}

/// Open a gRPC channel to xray's commander API over its Unix socket.
pub async fn connect(socket_path: &str) -> Result<Channel> {
    let path = socket_path.to_string();
    // Dummy URI — the connector below ignores it; tonic just needs a
    // syntactically valid absolute URI to populate the HTTP/2 :authority.
    let endpoint =
        Endpoint::try_from("http://xray-commander.invalid").context("building dummy endpoint")?;

    endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move {
                let stream = connect_uds(path).await?;
                Ok::<_, std::io::Error>(TokioIo::new(stream))
            }
        }))
        .await
        .with_context(|| format!("failed to connect to xray commander socket at {socket_path}"))
}

/// HandlerService shares the existing container commander socket. It must be
/// enabled by the prepared Xray configuration; connection/Unimplemented errors
/// are propagated rather than modifying that configuration or reloading Xray.
pub async fn handler_client(
    socket_path: &str,
    timeout: std::time::Duration,
) -> Result<handler::handler_service_client::HandlerServiceClient<Channel>> {
    let channel = tokio::time::timeout(timeout, connect(socket_path))
        .await
        .context("Xray commander connection timed out")??;
    Ok(
        handler::handler_service_client::HandlerServiceClient::new(channel)
            .max_decoding_message_size(1024 * 1024)
            .max_encoding_message_size(8 * 1024 * 1024),
    )
}

pub async fn stats_client(
    socket_path: &str,
) -> Result<stats::stats_service_client::StatsServiceClient<Channel>> {
    Ok(stats::stats_service_client::StatsServiceClient::new(
        connect(socket_path).await?,
    ))
}

pub async fn routing_client(
    socket_path: &str,
) -> Result<router::routing_service_client::RoutingServiceClient<Channel>> {
    Ok(router::routing_service_client::RoutingServiceClient::new(
        connect(socket_path).await?,
    ))
}

pub async fn logger_client(
    socket_path: &str,
) -> Result<logger::logger_service_client::LoggerServiceClient<Channel>> {
    Ok(logger::logger_service_client::LoggerServiceClient::new(
        connect(socket_path).await?,
    ))
}
