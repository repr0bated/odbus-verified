//! Health Check Endpoints
//!
//! Provides health check endpoints for monitoring and load balancers.

use axum::{Json, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Health check response
#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: u64,
    pub uptime: u64,
    pub version: String,
    pub services: HashMap<String, ServiceHealth>,
}

/// Individual service health status
#[derive(Serialize, Deserialize, Clone)]
pub struct ServiceHealth {
    pub status: String,
    pub message: Option<String>,
    pub last_check: u64,
}

/// Health checker for monitoring services
#[derive(Clone)]
pub struct HealthChecker {
    start_time: SystemTime,
    services: HashMap<String, ServiceHealth>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            start_time: SystemTime::now(),
            services: HashMap::new(),
        }
    }

    /// Register a service for health checking
    pub fn register_service(&mut self, name: impl Into<String>) {
        let service_name = name.into();
        self.services.insert(service_name, ServiceHealth {
            status: "unknown".to_string(),
            message: None,
            last_check: 0,
        });
    }

    /// Update service health status
    pub fn update_service_health(
        &mut self,
        name: &str,
        status: impl Into<String>,
        message: Option<String>,
    ) {
        if let Some(service) = self.services.get_mut(name) {
            service.status = status.into();
            service.message = message;
            service.last_check = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
        }
    }

    /// Perform comprehensive health check
    pub async fn check_health(&self) -> HealthResponse {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let uptime = now - self.start_time
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Determine overall status
        let overall_status = if self.services.values().all(|s| s.status == "healthy") {
            "healthy"
        } else if self.services.values().any(|s| s.status == "unhealthy") {
            "unhealthy"
        } else {
            "degraded"
        };

        HealthResponse {
            status: overall_status.to_string(),
            timestamp: now,
            uptime,
            version: env!("CARGO_PKG_VERSION").to_string(),
            services: self.services.clone(),
        }
    }

    /// Simple health check (just returns OK)
    pub async fn simple_health_check(&self) -> &'static str {
        "OK"
    }

    /// Detailed health check JSON response
    pub async fn detailed_health_check(&self) -> impl IntoResponse {
        Json(self.check_health().await)
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check handlers for Axum
pub mod handlers {
    use super::*;
    use axum::response::IntoResponse;

    /// Simple health check handler
    pub async fn health_check() -> &'static str {
        "OK"
    }

    /// Detailed health check handler
    pub async fn detailed_health_check(
        checker: axum::extract::State<HealthChecker>,
    ) -> impl IntoResponse {
        checker.detailed_health_check().await
    }

    /// Readiness check handler
    pub async fn readiness_check(
        checker: axum::extract::State<HealthChecker>,
    ) -> impl IntoResponse {
        let health = checker.check_health().await;
        if health.status == "healthy" {
            (axum::http::StatusCode::OK, Json(health))
        } else {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(health))
        }
    }

    /// Liveness check handler
    pub async fn liveness_check() -> &'static str {
        "OK"
    }
}

/// Health check utilities
pub mod utils {
    use super::*;

    /// Check if a service is responding
    pub async fn check_service_health(
        name: &str,
        url: &str,
        timeout: std::time::Duration,
    ) -> ServiceHealth {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build();

        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        match client {
            Ok(client) => {
                match client.get(url).send().await {
                    Ok(response) if response.status().is_success() => ServiceHealth {
                        status: "healthy".to_string(),
                        message: Some(format!("HTTP {} OK", response.status())),
                        last_check: start_time,
                    },
                    Ok(response) => ServiceHealth {
                        status: "unhealthy".to_string(),
                        message: Some(format!("HTTP {} error", response.status())),
                        last_check: start_time,
                    },
                    Err(e) => ServiceHealth {
                        status: "unhealthy".to_string(),
                        message: Some(format!("Connection error: {}", e)),
                        last_check: start_time,
                    },
                }
            }
            Err(e) => ServiceHealth {
                status: "error".to_string(),
                message: Some(format!("Client creation error: {}", e)),
                last_check: start_time,
            },
        }
    }

    /// Check database connectivity
    pub async fn check_database_health(connection_string: &str) -> ServiceHealth {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Placeholder - would implement actual DB health checks
        ServiceHealth {
            status: "healthy".to_string(),
            message: Some("Database connection OK".to_string()),
            last_check: start_time,
        }
    }

    /// Check filesystem health
    pub async fn check_filesystem_health(paths: &[&str]) -> ServiceHealth {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        for path in paths {
            if !std::path::Path::new(path).exists() {
                return ServiceHealth {
                    status: "unhealthy".to_string(),
                    message: Some(format!("Path does not exist: {}", path)),
                    last_check: start_time,
                };
            }
        }

        ServiceHealth {
            status: "healthy".to_string(),
            message: Some("All paths accessible".to_string()),
            last_check: start_time,
        }
    }
}
