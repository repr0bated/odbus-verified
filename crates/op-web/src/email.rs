//! Email Sending for Magic Link Authentication
//!
//! Uses SMTP via lettre to send magic link emails.

use anyhow::{Context, Result};
use lettre::{
    message::header::ContentType,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use tracing::info;

/// Email configuration from environment
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_pass: String,
    pub from_email: String,
    pub from_name: String,
    pub base_url: String,
}

impl EmailConfig {
    /// Load from environment variables
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            smtp_host: std::env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string()),
            smtp_port: std::env::var("SMTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(587),
            smtp_user: std::env::var("SMTP_USER").unwrap_or_default(),
            smtp_pass: std::env::var("SMTP_PASS").unwrap_or_default(),
            from_email: std::env::var("SMTP_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@example.com".to_string()),
            from_name: std::env::var("SMTP_FROM_NAME")
                .unwrap_or_else(|_| "Privacy Router".to_string()),
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
        })
    }

    /// Check if email is configured
    pub fn is_configured(&self) -> bool {
        !self.smtp_user.is_empty() && !self.smtp_pass.is_empty()
    }
}

/// Email sender
pub struct EmailSender {
    config: EmailConfig,
}

impl EmailSender {
    pub fn new(config: EmailConfig) -> Self {
        Self { config }
    }

    /// Send a magic link email
    pub async fn send_magic_link(&self, to_email: &str, token: &str) -> Result<()> {
        let magic_url = format!("{}/privacy/verify?token={}", self.config.base_url, token);

        if !self.config.is_configured() {
            // For testing: show magic link in console when email is not configured
            info!("🔗 MAGIC LINK (no SMTP configured):");
            info!("   To: {}", to_email);
            info!("   URL: {}", magic_url);
            info!("   Token: {}", token);
            println!("\n=== MAGIC LINK FOR TESTING ===");
            println!("Email: {}", to_email);
            println!("Link: {}", magic_url);
            println!("==============================\n");
            return Ok(());
        }

        let email = Message::builder()
            .from(format!("{} <{}>", self.config.from_name, self.config.from_email).parse()?)
            .to(to_email.parse()?)
            .subject("Your Privacy Router Login Link")
            .header(ContentType::TEXT_HTML)
            .body(format!(
                r#"<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }}
        .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
        .button {{
            display: inline-block;
            background: #6366f1;
            color: white !important;
            padding: 12px 24px;
            text-decoration: none;
            border-radius: 6px;
            font-weight: 500;
        }}
        .footer {{ margin-top: 30px; color: #666; font-size: 12px; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Privacy Router Access</h1>
        <p>Click the button below to access your VPN configuration:</p>
        <p><a href="{}" class="button">Get My VPN Config</a></p>
        <p>Or copy this link: <code>{}</code></p>
        <p class="footer">
            This link expires in 15 minutes.<br>
            If you didn't request this, you can ignore this email.
        </p>
    </div>
</body>
</html>"#,
                magic_url, magic_url
            ))?;

        let creds = Credentials::new(self.config.smtp_user.clone(), self.config.smtp_pass.clone());

        // Host mail-3tched (Postfix submission). Use STARTTLS on 587; allow the
        // container's self-signed cert so local delivery does not fail TLS verify.
        let tls = TlsParameters::builder(self.config.smtp_host.clone())
            .dangerous_accept_invalid_certs(true)
            .build()
            .context("smtp tls parameters")?;

        let mailer: AsyncSmtpTransport<Tokio1Executor> =
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.config.smtp_host)
                .port(self.config.smtp_port)
                .tls(if self.config.smtp_port == 465 {
                    Tls::Wrapper(tls)
                } else {
                    Tls::Required(tls) // STARTTLS
                })
                .credentials(creds)
                .build();

        mailer.send(email).await.with_context(|| {
            format!(
                "Failed to send email via {}:{} as {}",
                self.config.smtp_host, self.config.smtp_port, self.config.smtp_user
            )
        })?;

        info!(
            "Sent magic link email to {} via {}:{}",
            to_email, self.config.smtp_host, self.config.smtp_port
        );
        Ok(())
    }
}
