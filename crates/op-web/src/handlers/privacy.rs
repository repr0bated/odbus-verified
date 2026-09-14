//! Privacy Router API Handlers
//!
//! Handles user signup, magic link verification, and config download.

use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::{Html, Json, Redirect},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope,
    TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::state::AppState;
use crate::users::PrivacyUser;
use crate::wireguard::{generate_client_config, generate_keypair, generate_qr_code};

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct SignupResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct AccessQuery {
    pub user_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub success: bool,
    pub user_id: Option<String>,
    pub config: Option<String>,
    pub qr_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub available: bool,
    pub server_public_key: Option<String>,
    pub endpoint: Option<String>,
    pub registered_users: usize,
}

#[derive(Debug, Deserialize)]
pub struct SetCredentialsRequest {
    pub user_id: String,
    pub gemini_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub preferred_provider: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetCredentialsResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: String,
    pub state: String,
}

/// Google user info from OAuth token exchange
#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    id: String,
    email: String,
    verified_email: bool,
}

/// POST /api/privacy/signup - Register with email and send magic link
pub async fn signup(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<SignupRequest>,
) -> (StatusCode, Json<SignupResponse>) {
    let email = request.email.trim().to_lowercase();

    // 1. Basic Rate Limiting: Prevent magic link spam
    // In production, use Redis or a proper rate limiting crate.
    {
        // Add a simple in-memory signup rate tracker to AppState if needed,
        // but for now, we'll check magic_links map for existing tokens
        // or add a simple static rate limiter for this prototype.
        static LAST_SIGNUP: tokio::sync::Mutex<
            Option<std::collections::HashMap<String, std::time::Instant>>,
        > = tokio::sync::Mutex::const_new(None);

        let mut map_guard = LAST_SIGNUP.lock().await;
        if map_guard.is_none() {
            *map_guard = Some(std::collections::HashMap::new());
        }

        if let Some(ref mut map) = *map_guard {
            if let Some(last_time) = map.get(&email) {
                if last_time.elapsed().as_secs() < 60 {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(SignupResponse {
                            success: false,
                            message: "Please wait 60 seconds before requesting another login link"
                                .to_string(),
                        }),
                    );
                }
            }
            map.insert(email.clone(), std::time::Instant::now());

            // Periodically clean up old entries
            if map.len() > 1000 {
                map.retain(|_, v| v.elapsed().as_secs() < 3600);
            }
        }
    }

    // Basic email validation
    if !email.contains('@') || !email.contains('.') {
        return (
            StatusCode::BAD_REQUEST,
            Json(SignupResponse {
                success: false,
                message: "Invalid email address".to_string(),
            }),
        );
    }

    // Check if user already exists
    if let Some(existing) = state.user_store.get_user_by_email(&email).await {
        // User exists - create new magic link
        match state.user_store.create_magic_link(&existing.id).await {
            Ok(link) => {
                // Send email
                if let Err(e) = state
                    .email_sender
                    .send_magic_link(&email, &link.token)
                    .await
                {
                    error!("Failed to send magic link email: {}", e);
                }
                return (
                    StatusCode::OK,
                    Json(SignupResponse {
                        success: true,
                        message: "Check your email for the login link".to_string(),
                    }),
                );
            }
            Err(e) => {
                error!("Failed to create magic link: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(SignupResponse {
                        success: false,
                        message: "Failed to create login link".to_string(),
                    }),
                );
            }
        }
    }

    // Best-effort host fabric check. Magic-link issuance must not block on OVSDB/D-Bus
    // (list_dbs via org.opdbus.v1/rovs). Full privacy provision still runs on verify.
    if let Err(e) = crate::privacy_network::ensure_host_privacy_network().await {
        warn!(
            "Privacy network not ready at signup (continuing; will retry on verify): {}",
            e
        );
    }

    // New user - generate WireGuard keys
    let keypair = generate_keypair();
    // Create user (we'll encrypt the private key later, for now just store it)
    match state
        .user_store
        .create_user(&email, keypair.public_key, keypair.private_key)
        .await
    {
        Ok(user) => {
            // Create magic link
            match state.user_store.create_magic_link(&user.id).await {
                Ok(link) => {
                    // Send email
                    if let Err(e) = state
                        .email_sender
                        .send_magic_link(&email, &link.token)
                        .await
                    {
                        error!("Failed to send magic link email: {}", e);
                    }
                    info!("New privacy user registered: {}", email);
                    (
                        StatusCode::OK,
                        Json(SignupResponse {
                            success: true,
                            message: "Check your email for the login link".to_string(),
                        }),
                    )
                }
                Err(e) => {
                    error!("Failed to create magic link: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(SignupResponse {
                            success: false,
                            message: "Failed to create login link".to_string(),
                        }),
                    )
                }
            }
        }
        Err(e) => {
            error!("Failed to create user: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SignupResponse {
                    success: false,
                    message: format!("Registration failed: {}", e),
                }),
            )
        }
    }
}

/// GET /api/privacy/verify?token=xxx - Verify magic link and return config
pub async fn verify(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
) -> (StatusCode, Json<VerifyResponse>) {
    match state.user_store.verify_magic_link(&query.token).await {
        Ok(user) => {
            let provisioned_user = match provision_verified_user(&state, user).await {
                Ok(user) => user,
                Err(e) => {
                    error!("Privacy provisioning failed after verification: {}", e);
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(VerifyResponse {
                            success: false,
                            user_id: None,
                            config: None,
                            qr_code: None,
                            message: "Privacy network provisioning failed. Please retry shortly."
                                .to_string(),
                        }),
                    );
                }
            };

            if !state.server_config.is_configured() {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(VerifyResponse {
                        success: false,
                        user_id: None,
                        config: None,
                        qr_code: None,
                        message: "WireGuard server key is not configured on the host.".to_string(),
                    }),
                );
            }

            // Generate WireGuard config
            let config = generate_client_config(
                &provisioned_user.wg_private_key_encrypted, // This is the actual private key for now
                &provisioned_user.assigned_ip,
                &state.server_config,
            );

            // Generate QR code
            let qr_code = generate_qr_code(&config).ok();

            info!(
                "User {} verified, provisioned, and received config",
                provisioned_user.id
            );

            (
                StatusCode::OK,
                Json(VerifyResponse {
                    success: true,
                    user_id: Some(provisioned_user.id),
                    config: Some(config),
                    qr_code,
                    message: "Connected to privacy network. Your WireGuard configuration is ready."
                        .to_string(),
                }),
            )
        }
        Err(e) => {
            error!("Magic link verification failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: format!("Verification failed: {}", e),
                }),
            )
        }
    }
}

/// GET /privacy/verify?token=xxx - Verify magic link and redirect to access message
pub async fn verify_redirect(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
) -> Redirect {
    match state.user_store.verify_magic_link(&query.token).await {
        Ok(user) => match provision_verified_user(&state, user).await {
            Ok(provisioned_user) => {
                info!(
                    "User {} verified and provisioned via redirect flow",
                    provisioned_user.id
                );
                Redirect::to(&format!("/privacy/access?user_id={}", provisioned_user.id))
            }
            Err(e) => {
                error!("Provisioning failed in redirect flow: {}", e);
                Redirect::to("/privacy/access?status=provisioning_failed")
            }
        },
        Err(e) => {
            error!("Magic link verification failed in redirect flow: {}", e);
            Redirect::to("/privacy/access?status=verification_failed")
        }
    }
}

/// GET /privacy/access?user_id=xxx - Human-readable access confirmation page
pub async fn privacy_access_message(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<AccessQuery>,
) -> Html<String> {
    let body = if query.status.as_deref() == Some("verification_failed") {
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>Verification Failed</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; margin: 40px;">
    <h1>Verification failed</h1>
    <p>This link is invalid or expired. Please request a new signup link.</p>
</body>
</html>"#
            .to_string()
    } else if query.status.as_deref() == Some("provisioning_failed") {
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>Provisioning Failed</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; margin: 40px;">
    <h1>Privacy network provisioning failed</h1>
    <p>Your link was verified, but the privacy container could not be prepared. Please retry shortly.</p>
</body>
</html>"#
            .to_string()
    } else if let Some(user_id) = &query.user_id {
        match state.user_store.get_user(user_id).await {
        Some(user) if user.email_verified => {
            if !state.server_config.is_configured() {
                return Html(
                    r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>WireGuard Not Configured</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; margin: 40px;">
    <h1>WireGuard server key not configured</h1>
    <p>The privacy network is up, but the server public key is missing. Set <code>WG_SERVER_PUBKEY</code> and restart <code>op-web</code>.</p>
</body>
</html>"#
                        .to_string(),
                );
            }

            let config = generate_client_config(
                &user.wg_private_key_encrypted,
                &user.assigned_ip,
                &state.server_config,
            );
            let connection_status = if user.privacy_network_connected {
                "Connected to Privacy Network"
            } else {
                "Provisioning in Progress"
            };
            let container_name = user
                .privacy_container_name
                .clone()
                .unwrap_or_else(|| "not-provisioned".to_string());

            format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Privacy Network Access</title>
    <style>
        body {{ font-family: system-ui, -apple-system, sans-serif; margin: 0; background: #0b1020; color: #e6edf3; }}
        .wrap {{ max-width: 680px; margin: 64px auto; padding: 24px; }}
        .card {{ background: #121a2d; border: 1px solid #2f3b52; border-radius: 12px; padding: 24px; }}
        h1 {{ margin-top: 0; color: #7ee787; }}
        h2 {{ margin-top: 22px; color: #9ecbff; }}
        p {{ line-height: 1.5; color: #c9d1d9; }}
        .pill {{ display: inline-block; margin-right: 8px; margin-bottom: 8px; padding: 6px 10px; border-radius: 999px; background: #1f6feb; color: #fff; font-size: 13px; }}
        pre {{ background: #0d1117; color: #e6edf3; border: 1px solid #2f3b52; border-radius: 10px; padding: 14px; overflow: auto; white-space: pre-wrap; line-height: 1.45; }}
        .mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }}
        code {{ background: #0d1117; padding: 2px 6px; border-radius: 6px; }}
    </style>
</head>
<body>
    <div class="wrap">
        <div class="card">
            <h1>{}</h1>
            <p class="pill">Privacy Network Allocation: {}</p>
            <p class="pill">Container: <span class="mono">{}</span></p>
            <p>Your account and WireGuard identity are registered. You have been allocated <strong>{}</strong> of privacy network capacity.</p>
            <p>Connected to privacy network. Install this WireGuard config to start routing traffic through your privacy container.</p>
            <h2>WireGuard Configuration</h2>
            <pre class="mono">{}</pre>
            <p>You can also fetch this config via <code>/api/privacy/config/{}</code>.</p>
        </div>
    </div>
</body>
</html>"#,
            connection_status,
            format_quota_gib(user.privacy_quota_bytes),
            escape_html(&container_name),
            format_quota_gib(user.privacy_quota_bytes),
            escape_html(&config),
            user.id,
        )
        }
        _ => r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>Verification Failed</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; margin: 40px;">
    <h1>Verification failed</h1>
    <p>This link is invalid or expired. Please request a new signup link.</p>
</body>
</html>"#
            .to_string(),
        }
    } else {
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>Privacy Access</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; margin: 40px;">
    <h1>Privacy access page</h1>
    <p>No user context was provided.</p>
</body>
</html>"#
            .to_string()
    };

    Html(body)
}

fn format_quota_gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// GET /api/privacy/config/:user_id - Download config for verified user
pub async fn get_config(
    headers: axum::http::HeaderMap,
    axum::extract::Extension(state): axum::extract::Extension<std::sync::Arc<crate::AppState>>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> (axum::http::StatusCode, axum::Json<VerifyResponse>) {
    let auth_token = crate::middleware::security::extract_auth_token(&headers);
    let mut is_authorized = false;

    if let Some(token) = auth_token {
        if let Some(user) = state.user_store.get_user(&user_id).await {
            if let Some(creds) = &user.api_credentials {
                if creds.token == token {
                    is_authorized = true;
                }
            }
        }
    }

    if !is_authorized {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "Unauthorized".to_string(),
            }),
        );
    }

    match state.user_store.get_user(&user_id).await {
        Some(user) if user.email_verified => {
            if !state.server_config.is_configured() {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(VerifyResponse {
                        success: false,
                        user_id: None,
                        config: None,
                        qr_code: None,
                        message: "WireGuard server key is not configured on the host.".to_string(),
                    }),
                );
            }

            let config = generate_client_config(
                &user.wg_private_key_encrypted,
                &user.assigned_ip,
                &state.server_config,
            );
            let qr_code = generate_qr_code(&config).ok();

            (
                StatusCode::OK,
                Json(VerifyResponse {
                    success: true,
                    user_id: Some(user.id),
                    config: Some(config),
                    qr_code,
                    message: "Configuration retrieved".to_string(),
                }),
            )
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "Email not verified".to_string(),
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "User not found".to_string(),
            }),
        ),
    }
}

/// GET /api/privacy/status - Check privacy router availability
pub async fn status(Extension(state): Extension<Arc<AppState>>) -> Json<StatusResponse> {
    // For now, just report basic status
    let configured = state.server_config.is_configured();
    Json(StatusResponse {
        available: configured,
        server_public_key: if configured {
            Some(state.server_config.public_key.clone())
        } else {
            None
        },
        endpoint: Some(state.server_config.endpoint.clone()),
        registered_users: 0, // TODO: Add user count method
    })
}

/// POST /api/privacy/credentials - Set user API credentials
pub async fn set_credentials(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<SetCredentialsRequest>,
) -> (StatusCode, Json<SetCredentialsResponse>) {
    use crate::users::UserApiCredentials;

    let existing_token = if let Some(user) = state.user_store.get_user(&request.user_id).await {
        user.api_credentials.map(|c| c.token)
    } else {
        None
    };

    let token = existing_token.unwrap_or_else(|| op_identity::generate_magic_link_token(32));

    let credentials = UserApiCredentials {
        token,
        gemini_api_key: request.gemini_api_key,
        anthropic_api_key: request.anthropic_api_key,
        openai_api_key: request.openai_api_key,
        preferred_provider: request.preferred_provider,
    };

    match state
        .user_store
        .set_user_api_credentials(&request.user_id, credentials)
        .await
    {
        Ok(()) => {
            info!("Set API credentials for user {}", request.user_id);
            (
                StatusCode::OK,
                Json(SetCredentialsResponse {
                    success: true,
                    message: "API credentials updated successfully".to_string(),
                }),
            )
        }
        Err(e) => {
            error!(
                "Failed to set API credentials for user {}: {}",
                request.user_id, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SetCredentialsResponse {
                    success: false,
                    message: format!("Failed to update credentials: {}", e),
                }),
            )
        }
    }
}

/// GET /api/privacy/google/auth - Initiate Google OAuth login
pub async fn google_auth(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Redirect, (StatusCode, Json<VerifyResponse>)> {
    let config = match state.google_oauth_config.as_ref() {
        Some(config) => config,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Google OAuth not configured".to_string(),
                }),
            ));
        }
    };

    // Create OAuth2 client
    let client = oauth2::basic::BasicClient::new(
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("hardcoded Google auth URL is valid"),
        Some(
            TokenUrl::new("https://www.googleapis.com/oauth2/v4/token".to_string())
                .expect("hardcoded Google token URL is valid"),
        ),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.redirect_url.clone()).expect("configured redirect URL is valid"),
    );

    // Generate the authorization URL
    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();

    // Store CSRF token to prevent CSRF attacks
    {
        let mut tokens = state.csrf_tokens.write().await;
        tokens.insert(csrf_token.secret().clone(), csrf_token.secret().clone());

        // Cleanup old tokens (simple heuristic)
        if tokens.len() > 1000 {
            tokens.clear();
        }
    }

    info!("Initiating Google OAuth login: {}", auth_url);
    Ok(Redirect::to(auth_url.as_str()))
}

/// GET /api/privacy/google/callback - Handle Google OAuth callback
pub async fn google_callback(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Redirect, (StatusCode, Json<VerifyResponse>)> {
    // 1. Validate CSRF token
    {
        let mut tokens = state.csrf_tokens.write().await;
        if tokens.remove(&query.state).is_none() {
            warn!(
                "Invalid CSRF token in Google OAuth callback: {}",
                query.state
            );
            return Err((
                StatusCode::FORBIDDEN,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Invalid CSRF state".to_string(),
                }),
            ));
        }
    }

    let config = match state.google_oauth_config.as_ref() {
        Some(config) => config,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Google OAuth not configured".to_string(),
                }),
            ));
        }
    };

    // Create OAuth2 client
    let client = oauth2::basic::BasicClient::new(
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("hardcoded Google auth URL is valid"),
        Some(
            TokenUrl::new("https://www.googleapis.com/oauth2/v4/token".to_string())
                .expect("hardcoded Google token URL is valid"),
        ),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.redirect_url.clone()).expect("configured redirect URL is valid"),
    );

    // Exchange authorization code for token
    let token_result = client
        .exchange_code(AuthorizationCode::new(query.code.clone()))
        .request_async(oauth2::reqwest::async_http_client)
        .await;

    let token = match token_result {
        Ok(token) => token,
        Err(e) => {
            error!("Failed to exchange OAuth code: {}", e);
            return Err((
                StatusCode::BAD_REQUEST,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Failed to authenticate with Google".to_string(),
                }),
            ));
        }
    };

    // Get user info from Google
    let user_info_url = "https://www.googleapis.com/oauth2/v2/userinfo";
    let client = reqwest::Client::new();
    let user_info_result = client
        .get(user_info_url)
        .bearer_auth(token.access_token().secret())
        .send()
        .await;

    let user_info: GoogleUserInfo = match user_info_result {
        Ok(response) => match response.json().await {
            Ok(info) => info,
            Err(e) => {
                error!("Failed to parse Google user info: {}", e);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(VerifyResponse {
                        success: false,
                        user_id: None,
                        config: None,
                        qr_code: None,
                        message: "Failed to get user information".to_string(),
                    }),
                ));
            }
        },
        Err(e) => {
            error!("Failed to get Google user info: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Failed to get user information".to_string(),
                }),
            ));
        }
    };

    // Check if email is verified
    if !user_info.verified_email {
        return Err((
            StatusCode::FORBIDDEN,
            Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "Google account email not verified".to_string(),
            }),
        ));
    }

    // Generate WireGuard keys
    if let Err(e) = crate::privacy_network::ensure_host_privacy_network().await {
        error!("Failed to provision host privacy network: {}", e);
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "Privacy network host setup failed. Please retry shortly.".to_string(),
            }),
        ));
    }

    // Generate WireGuard keys
    let keypair = generate_keypair();

    // Create or link user with Google identity
    let user = match state
        .user_store
        .create_or_link_google_user(
            &user_info.id,
            &user_info.email,
            keypair.public_key,
            keypair.private_key,
        )
        .await
    {
        Ok(user) => user,
        Err(e) => {
            error!("Failed to create/link Google user: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Failed to create user account".to_string(),
                }),
            ));
        }
    };

    let provisioned_user = match provision_verified_user(&state, user).await {
        Ok(user) => user,
        Err(e) => {
            error!("Privacy provisioning failed for Google OAuth user: {}", e);
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(VerifyResponse {
                    success: false,
                    user_id: None,
                    config: None,
                    qr_code: None,
                    message: "Privacy network provisioning failed. Please retry shortly."
                        .to_string(),
                }),
            ));
        }
    };

    if !state.server_config.is_configured() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(VerifyResponse {
                success: false,
                user_id: None,
                config: None,
                qr_code: None,
                message: "WireGuard server key is not configured on the host.".to_string(),
            }),
        ));
    }

    // Generate WireGuard config
    let config = generate_client_config(
        &provisioned_user.wg_private_key_encrypted,
        &provisioned_user.assigned_ip,
        &state.server_config,
    );

    // Generate QR code
    let qr_code = generate_qr_code(&config).ok();

    info!(
        "Google OAuth login successful for user {} (privacy network provisioned)",
        provisioned_user.id
    );

    // For now, return JSON response. In production, you might want to redirect to a success page
    // or return the config in a different format
    Err((
        StatusCode::OK,
        Json(VerifyResponse {
            success: true,
            user_id: Some(provisioned_user.id),
            config: Some(config),
            qr_code,
            message: "Connected to privacy network. Your WireGuard configuration is ready."
                .to_string(),
        }),
    ))
}

async fn provision_verified_user(
    state: &Arc<AppState>,
    user: PrivacyUser,
) -> anyhow::Result<PrivacyUser> {
    crate::privacy_network::ensure_host_privacy_network().await?;
    let container_name = crate::privacy_container::ensure_user_container(&user).await?;
    let route_id =
        crate::privacy_routes::publish_user_privacy_route(&user, Some(container_name.as_str()))
            .await?;
    crate::privacy_openflow::publish_openflow_for_privacy_routes().await?;

    let already_connected = user.privacy_network_connected
        && user.privacy_container_name.as_deref() == Some(container_name.as_str())
        && user.privacy_route_id.as_deref() == Some(route_id.as_str());
    if already_connected {
        return Ok(user);
    }

    state
        .user_store
        .mark_privacy_network_connected(&user.id, container_name, route_id)
        .await
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
