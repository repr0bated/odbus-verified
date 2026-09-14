//! Metrics and Monitoring
//!
//! Provides Prometheus metrics, request counting, and performance monitoring.

use axum::{extract::Request, middleware::Next, response::Response};
use lazy_static::lazy_static;
use prometheus::{
    register_counter, register_histogram, register_gauge,
    Counter, Histogram, Gauge, Encoder, TextEncoder,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metrics registry
#[derive(Clone)]
pub struct Metrics {
    request_count: Counter,
    request_duration: Histogram,
    active_connections: Gauge,
    services: Arc<RwLock<HashMap<String, ServiceMetrics>>>,
}

impl Metrics {
    /// Create a new metrics instance
    pub fn new() -> Self {
        let request_count = register_counter!(
            "http_requests_total",
            "Total number of HTTP requests"
        ).unwrap();

        let request_duration = register_histogram!(
            "http_request_duration_seconds",
            "HTTP request duration in seconds"
        ).unwrap();

        let active_connections = register_gauge!(
            "http_active_connections",
            "Number of active HTTP connections"
        ).unwrap();

        Self {
            request_count,
            request_duration,
            active_connections,
            services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a request
    pub async fn record_request(&self, method: &str, path: &str, status: u16, duration: f64) {
        // Record global metrics
        self.request_count.inc();
        self.request_duration.observe(duration);

        // Record service-specific metrics
        let mut services = self.services.write().await;

        // Extract service name from path (e.g., /api/chat -> chat)
        let service_name = extract_service_name(path);
        let service_metrics = services.entry(service_name.to_string())
            .or_insert_with(|| ServiceMetrics::new(&service_name));

        service_metrics.record_request(method, status, duration).await;
    }

    /// Increment active connections
    pub fn increment_connections(&self) {
        self.active_connections.inc();
    }

    /// Decrement active connections
    pub fn decrement_connections(&self) {
        self.active_connections.dec();
    }

    /// Get Prometheus metrics as string
    pub async fn prometheus_metrics(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    /// Get service metrics
    pub async fn service_metrics(&self) -> HashMap<String, ServiceMetrics> {
        self.services.read().await.clone()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Service-specific metrics
#[derive(Clone)]
pub struct ServiceMetrics {
    pub name: String,
    request_count: Counter,
    request_duration: Histogram,
    error_count: Counter,
}

impl ServiceMetrics {
    /// Create new service metrics
    pub fn new(name: &str) -> Self {
        let request_count = register_counter!(
            format!("{}_requests_total", name),
            format!("Total requests for {} service", name)
        ).unwrap();

        let request_duration = register_histogram!(
            format!("{}_request_duration_seconds", name),
            format!("Request duration for {} service", name)
        ).unwrap();

        let error_count = register_counter!(
            format!("{}_errors_total", name),
            format!("Total errors for {} service", name)
        ).unwrap();

        Self {
            name: name.to_string(),
            request_count,
            request_duration,
            error_count,
        }
    }

    /// Record a request for this service
    pub async fn record_request(&self, method: &str, status: u16, duration: f64) {
        self.request_count.inc();
        self.request_duration.observe(duration);

        if status >= 400 {
            self.error_count.inc();
        }
    }
}

/// Extract service name from path
fn extract_service_name(path: &str) -> &str {
    if path.starts_with("/api/") {
        // Extract service name from /api/service/...
        path.split('/').nth(2).unwrap_or("unknown")
    } else if path.starts_with("/ws/") {
        // Extract service name from /ws/service
        path.split('/').nth(2).unwrap_or("unknown")
    } else {
        "unknown"
    }
}

/// Metrics middleware
pub async fn metrics_middleware(
    metrics: axum::extract::State<Arc<Metrics>>,
    request: Request,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // Increment active connections
    metrics.increment_connections();

    let response = next.run(request).await;

    // Decrement active connections
    metrics.decrement_connections();

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    // Record metrics
    metrics.record_request(&method, &path, status, duration).await;

    response
}

/// Metrics handlers for Axum
pub mod handlers {
    use super::*;
    use axum::response::IntoResponse;

    /// Prometheus metrics endpoint
    pub async fn prometheus_metrics(
        metrics: axum::extract::State<Arc<Metrics>>,
    ) -> impl IntoResponse {
        let metrics_text = metrics.prometheus_metrics().await;
        axum::response::Response::builder()
            .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
            .body(metrics_text)
            .unwrap()
    }

    /// JSON metrics endpoint
    pub async fn json_metrics(
        metrics: axum::extract::State<Arc<Metrics>>,
    ) -> impl IntoResponse {
        use simd_json::json;

        let service_metrics = metrics.service_metrics().await;
        let response = json!({
            "services": service_metrics.into_iter()
                .map(|(name, metrics)| {
                    (name, simd_json::json!({
                        "name": metrics.name,
                        // Prometheus counters are not directly accessible
                        // In a real implementation, you'd expose the values
                    }))
                })
                .collect::<simd_json::value::owned::Object<String, simd_json::OwnedValue>>()
        });

        axum::Json(response)
    }

    /// Metrics dashboard endpoint
    pub async fn metrics_dashboard() -> impl IntoResponse {
        let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Metrics Dashboard</title>
            <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
        </head>
        <body>
            <h1>HTTP Server Metrics</h1>
            <div>
                <h2>Prometheus Metrics</h2>
                <a href="/metrics">View Prometheus Metrics</a>
            </div>
            <div>
                <h2>JSON Metrics</h2>
                <a href="/metrics/json">View JSON Metrics</a>
            </div>
        </body>
        </html>
        "#;

        axum::response::Html(html)
    }
}

/// Performance monitoring utilities
pub mod perf {
    use super::*;

    /// Performance monitor
    pub struct PerformanceMonitor {
        metrics: Arc<Metrics>,
    }

    impl PerformanceMonitor {
        pub fn new(metrics: Arc<Metrics>) -> Self {
            Self { metrics }
        }

        /// Monitor a function execution
        pub async fn monitor<F, Fut, T>(&self, name: &str, f: F) -> T
        where
            F: FnOnce() -> Fut,
            Fut: std::future::Future<Output = T>,
        {
            let start = std::time::Instant::now();
            let result = f().await;
            let duration = start.elapsed().as_secs_f64();

            // Record custom metric
            self.metrics.request_duration.observe(duration);

            result
        }
    }
}

// Global metrics instance
lazy_static! {
    pub static ref GLOBAL_METRICS: Arc<Metrics> = Arc::new(Metrics::new());
}
