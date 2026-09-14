//! Entry point — native eframe shell + background tokio runtime for tonic.
mod app;
mod catalog;
mod chat;
mod conn;
mod auth;
mod grpc;
mod nav;
mod schema; // DEPRECATED: superseded by `catalog::interpret`; remove once views are migrated.
mod theme;
mod views;

use crate::grpc::ReflectionRegistry;

fn main() -> eframe::Result<()> {
    // Background tokio runtime drives the tonic-web reflection client.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _enter = rt.enter();

    let registry = ReflectionRegistry::new();
    let chat_transport: Option<std::sync::Arc<crate::chat::ChatTransport>> = {
        let endpoint = std::env::var("ZEROCLAW_GRPC")
            .or_else(|_| std::env::var("ZEROCLAW_GRPC_FABRIC"))
            .unwrap_or_else(|_| "http://10.0.0.3:8090".into());
        let registry = registry.clone();
        let endpoint_clone = endpoint.clone();
        rt.spawn(async move {
            if let Err(e) = grpc::bootstrap(&endpoint_clone, registry).await {
                eprintln!("[zeroclaw] reflection bootstrap failed: {e:#}");
            }
        });

        // Connect the ChatService transport to the same endpoint.
        match rt.block_on(crate::chat::ChatTransport::connect(&endpoint)) {
            Ok(transport) => {
                eprintln!("[zeroclaw] ChatService transport connected to {endpoint}");
                Some(std::sync::Arc::new(transport))
            }
            Err(e) => {
                eprintln!("[zeroclaw] ChatService transport connect failed: {e:#}");
                None
            }
        }
    };

    // Keep runtime alive for the lifetime of the app.
    std::mem::forget(rt);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("ZeroClaw — Operator Console"),
        ..Default::default()
    };
    eframe::run_native(
        "ZeroClaw",
        native_options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let mut app = app::ZeroClawApp::new_with_registry(cc, registry);
            app.chat_transport = chat_transport;
            Ok(Box::new(app))
        }),
    )
}
