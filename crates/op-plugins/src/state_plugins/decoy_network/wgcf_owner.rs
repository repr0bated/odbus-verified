//! Private Cloudflare WARP account owner. The API protocol is pinned to wgcf
//! v2's `v0a1922` client and all kernel work is delegated to WireGuardOwner.
use super::super::runtime_documents::RuntimeDocuments;
use super::super::wgcf::*;
use super::super::wireguard::WireGuardState;
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

const DOCUMENT_ID: &str = "wgcf_account_v2";
const API_VERSION: &str = "v0a1922";
const PRODUCTION_API: &str = "https://api.cloudflareclient.com";
const MAX_BODY: usize = 1024 * 1024;

#[async_trait]
pub trait WgcfWireGuard: Send + Sync {
    fn generate_keypair(&self) -> Result<(String, String)>;
    fn public_from_private(&self, private: &str) -> Result<String>;
    async fn apply_profile(&self, profile: &StoredProfile) -> Result<WireGuardApplyReceipt>;
    async fn state(&self) -> Result<WireGuardState>;
}

pub struct WireGuardApplyReceipt {
    pub wireguard_state: WireGuardState,
    pub wg_opdbus_state: Value,
}

#[async_trait]
impl WgcfWireGuard for super::wireguard_owner::WireGuardOwner {
    fn generate_keypair(&self) -> Result<(String, String)> {
        let pair = super::wireguard_owner::WireGuardOwner::generate_keypair(self)?;
        Ok((pair.private_key().to_owned(), pair.public_key().to_owned()))
    }
    fn public_from_private(&self, private: &str) -> Result<String> {
        super::wireguard_owner::WireGuardOwner::public_from_private(self, private)
    }
    async fn apply_profile(&self, profile: &StoredProfile) -> Result<WireGuardApplyReceipt> {
        let peer = super::super::wireguard::WireGuardPeerConfig {
            public_key: profile.view.peer_public_key.clone(),
            allowed_ips: vec!["0.0.0.0/0".into(), "::/0".into()],
            endpoint: Some(profile.view.endpoint.clone()),
            persistent_keepalive: Some(25),
            preshared_key: None,
        };
        let config = super::super::wireguard::WireGuardDeviceConfig {
            private_key: Some(profile.private_key.clone()),
            listen_port: None,
            fwmark: None,
            peers: vec![peer],
        };
        self.apply_interface_profile(
            &profile.view.interface,
            config,
            profile.view.addresses.clone(),
            profile.view.mtu,
        )
        .await?;
        Ok(WireGuardApplyReceipt {
            wireguard_state: self.state().await?,
            wg_opdbus_state: serde_json::to_value(self.wg_opdbus_state().await?)?,
        })
    }
    async fn state(&self) -> Result<WireGuardState> {
        super::wireguard_owner::WireGuardOwner::state(self).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Endpoint {
    host: String,
    v4: String,
    v6: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Peer {
    endpoint: Endpoint,
    public_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Addresses {
    v4: String,
    v6: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Interface {
    addresses: Addresses,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WarpConfig {
    client_id: String,
    interface: Interface,
    peers: Vec<Peer>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Account {
    id: String,
    account_type: String,
    role: String,
    warp_plus: bool,
    quota: f64,
    premium_data: f64,
    referral_count: f64,
    referral_renewal_countdown: f64,
    created: String,
    updated: String,
    license: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct SourceDevice {
    id: String,
    enabled: bool,
    name: Option<String>,
    model: Option<String>,
    #[serde(rename = "type")]
    device_type: String,
    created: String,
    updated: String,
    warp_enabled: bool,
    key: String,
    account: Account,
    config: WarpConfig,
}
#[derive(Deserialize)]
struct Registration {
    #[serde(flatten)]
    device: SourceDevice,
    token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredProfile {
    pub(crate) view: WgcfProfileView,
    pub(crate) private_key: String,
}
#[derive(Clone, Serialize, Deserialize, Default)]
struct Document {
    token: Option<String>,
    device: Option<SourceDevice>,
    pending_private_key: Option<String>,
    private_key: Option<String>,
    endpoint_override: Option<String>,
    profiles: BTreeMap<String, StoredProfile>,
    active_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgcfReceipt {
    pub output: Value,
    pub wgcf_state: WgcfState,
    pub wireguard_state: Option<Value>,
    pub wg_opdbus_state: Option<Value>,
}

pub struct WgcfOwner {
    documents: Arc<RuntimeDocuments>,
    wireguard: Arc<dyn WgcfWireGuard>,
    http: reqwest::Client,
    api: String,
    operation: Mutex<()>,
}
impl WgcfOwner {
    pub fn production(
        documents: Arc<RuntimeDocuments>,
        wireguard: Arc<dyn WgcfWireGuard>,
    ) -> Result<Self> {
        Self::new(documents, wireguard, PRODUCTION_API.into(), false)
    }
    pub fn new(
        documents: Arc<RuntimeDocuments>,
        wireguard: Arc<dyn WgcfWireGuard>,
        api: String,
        allow_http: bool,
    ) -> Result<Self> {
        let url = reqwest::Url::parse(&api)?;
        ensure!(
            url.scheme() == "https"
                || (allow_http && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")),
            "WGCF API endpoint is not permitted"
        );
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .max_tls_version(reqwest::tls::Version::TLS_1_2)
            .build()?;
        Ok(Self {
            documents,
            wireguard,
            http,
            api: api.trim_end_matches('/').into(),
            operation: Mutex::new(()),
        })
    }
    pub async fn dispatch(&self, method: &str, args: Value) -> Result<WgcfReceipt> {
        let _guard = self.operation.lock().await;
        match method {
            "register" => self.register(serde_json::from_value(args)?).await,
            "update" => self.update(serde_json::from_value(args)?).await,
            "status" => {
                let _: super::super::plugin_scaffold_helpers::EmptyInput =
                    serde_json::from_value(args)?;
                self.status().await
            }
            "rotate_keys" => {
                let _: super::super::plugin_scaffold_helpers::EmptyInput =
                    serde_json::from_value(args)?;
                self.rotate().await
            }
            "set_endpoint" => self.set_endpoint(serde_json::from_value(args)?).await,
            "generate_config" => self.generate(serde_json::from_value(args)?).await,
            "apply_config" => self.apply(serde_json::from_value(args)?).await,
            "refresh" => self.refresh(serde_json::from_value(args)?).await,
            _ => anyhow::bail!("unknown wgcf method"),
        }
    }
    async fn row(&self) -> Result<(u64, Document)> {
        let row = self
            .documents
            .read_durable(DOCUMENT_ID)
            .await?
            .context("WGCF account is not registered")?;
        Ok((row.revision, serde_json::from_value(row.document)?))
    }
    async fn save<F>(&self, f: F) -> Result<(u64, Document)>
    where
        F: FnOnce(&mut Document) -> Result<()> + Send + 'static,
    {
        let (row, ()) = self
            .documents
            .update_with_result(DOCUMENT_ID, move |old| {
                let mut d = old
                    .map(serde_json::from_value)
                    .transpose()?
                    .unwrap_or_default();
                f(&mut d)?;
                Ok((serde_json::to_value(&d)?, ()))
            })
            .await?;
        Ok((row.revision, serde_json::from_value(row.document)?))
    }
    async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<T> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.api, path))
            .header("User-Agent", "okhttp/3.12.1")
            .header("CF-Client-Version", "a-6.3-1922");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .context("WGCF API delivery failed; outcome may require readback")?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "WGCF API rejected request with status {}",
            response.status()
        );
        if let Some(length) = response.content_length() {
            ensure!(
                length as usize <= MAX_BODY,
                "WGCF API response exceeds limit"
            );
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_BODY,
                "WGCF API response exceeds limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).context("WGCF API returned malformed JSON")
    }
    async fn native(&self, token: &str, id: &str) -> Result<SourceDevice> {
        validate_api_id(id)?;
        let device = self
            .request(
                reqwest::Method::GET,
                &format!("/{API_VERSION}/reg/{id}"),
                Some(token),
                None,
            )
            .await?;
        validate_source(&device)?;
        Ok(device)
    }
    async fn register(&self, input: RegisterInput) -> Result<WgcfReceipt> {
        ensure!(
            input.accept_tos,
            "Cloudflare terms must be explicitly accepted"
        );
        validate_text(&input.device_model, 64)?;
        ensure!(
            self.documents.read_durable(DOCUMENT_ID).await?.is_none(),
            "WGCF registration already exists or has an indeterminate pending receipt"
        );
        let (private, public) = self.wireguard.generate_keypair()?;
        ensure!(
            self.wireguard.public_from_private(&private)? == public,
            "WireGuard keypair mismatch"
        );
        let pending = private.clone();
        self.save(move |d| {
            ensure!(d.device.is_none(), "WGCF account already exists");
            d.pending_private_key = Some(pending);
            Ok(())
        })
        .await?;
        let registration:Registration=self.request(reqwest::Method::POST,&format!("/{API_VERSION}/reg"),None,Some(json!({"fcm_token":"","install_id":"","key":public,"locale":"en_US","model":input.device_model,"tos":chrono::Utc::now().to_rfc3339(),"type":"Android"}))).await?;
        validate_source(&registration.device)?;
        ensure!(
            registration.device.key == public,
            "WGCF registration public-key readback mismatch"
        );
        let token = registration.token;
        let device = registration.device;
        let (revision, document) = self
            .save(move |d| {
                d.token = Some(token);
                d.device = Some(device);
                d.private_key = d.pending_private_key.take();
                Ok(())
            })
            .await?;
        self.receipt(revision, document, None, None).await
    }
    async fn status(&self) -> Result<WgcfReceipt> {
        let (_, d) = self.row().await?;
        let token = d.token.as_deref().context("WGCF token absent")?;
        let id = &d.device.as_ref().context("WGCF device absent")?.id;
        let observed = self.native(token, id).await?;
        let (revision, document) = self
            .save(move |d| {
                d.device = Some(observed);
                Ok(())
            })
            .await?;
        let kernel = self.wireguard.state().await?;
        let projected = serde_json::to_value(&kernel)?;
        self.receipt(revision, document, Some(kernel), Some(projected))
            .await
    }
    async fn refresh(&self, _: RefreshInput) -> Result<WgcfReceipt> {
        self.status().await
    }
    async fn update(&self, input: UpdateInput) -> Result<WgcfReceipt> {
        ensure!(
            input.license.is_some() ^ input.device_name.is_some(),
            "WGCF update requires exactly one account license or device name"
        );
        let (_, d) = self.row().await?;
        let token = d.token.clone().context("WGCF token absent")?;
        let id = d.device.as_ref().context("WGCF device absent")?.id.clone();
        validate_api_id(&id)?;
        let requested_license = input.license;
        let requested_name = input.device_name;
        if let Some(license) = requested_license.as_ref() {
            validate_text(license, 128)?;
            let _: Value = self
                .request(
                    reqwest::Method::PUT,
                    &format!("/{API_VERSION}/reg/{id}/account"),
                    Some(&token),
                    Some(json!({"license":license})),
                )
                .await?;
        }
        if let Some(name) = requested_name.as_ref() {
            validate_text(name, 64)?;
            let _: Vec<Value> = self
                .request(
                    reqwest::Method::PATCH,
                    &format!("/{API_VERSION}/reg/{id}/account/reg/{id}"),
                    Some(&token),
                    Some(json!({"name":name})),
                )
                .await?;
        }
        let observed = self.native(&token, &id).await?;
        if let Some(license) = requested_license.as_deref() {
            ensure!(
                observed.account.license == license,
                "WGCF account license update readback mismatch"
            );
        }
        if let Some(name) = requested_name.as_deref() {
            ensure!(
                observed.name.as_deref() == Some(name),
                "WGCF device name update readback mismatch"
            );
        }
        let (revision, document) = self
            .save(move |d| {
                d.device = Some(observed);
                Ok(())
            })
            .await?;
        self.receipt(revision, document, None, None).await
    }
    async fn rotate(&self) -> Result<WgcfReceipt> {
        let (_, d) = self.row().await?;
        let token = d.token.clone().context("WGCF token absent")?;
        let id = d.device.as_ref().context("WGCF device absent")?.id.clone();
        validate_api_id(&id)?;
        let private = match d.pending_private_key.clone() {
            Some(value) => value,
            None => self.wireguard.generate_keypair()?.0,
        };
        let public = self.wireguard.public_from_private(&private)?;
        let pending = private.clone();
        self.save(move |d| {
            d.pending_private_key = Some(pending);
            Ok(())
        })
        .await?;
        let _: SourceDevice = self
            .request(
                reqwest::Method::PATCH,
                &format!("/{API_VERSION}/reg/{id}"),
                Some(&token),
                Some(json!({"key":public})),
            )
            .await?;
        let observed = self.native(&token, &id).await?;
        ensure!(
            observed.key == public,
            "WGCF key rotation readback mismatch"
        );
        let (revision, document) = self
            .save(move |d| {
                d.device = Some(observed);
                d.private_key = d.pending_private_key.take();
                d.profiles.clear();
                d.active_profile = None;
                Ok(())
            })
            .await?;
        self.receipt(revision, document, None, None).await
    }
    async fn set_endpoint(&self, input: SetEndpointInput) -> Result<WgcfReceipt> {
        validate_endpoint(&input.endpoint)?;
        let endpoint = input.endpoint;
        let (revision, document) = self
            .save(move |d| {
                d.endpoint_override = Some(endpoint);
                Ok(())
            })
            .await?;
        let output = self
            .generate_from(
                revision,
                &document,
                GenerateConfigInput {
                    interface: "wgcf".into(),
                    dns: vec![],
                    mtu: None,
                },
            )
            .await?;
        let (revision, document) = self.row().await?;
        Ok(WgcfReceipt {
            output: serde_json::to_value(output)?,
            wgcf_state: project(revision, &document, None),
            wireguard_state: None,
            wg_opdbus_state: None,
        })
    }
    async fn generate(&self, input: GenerateConfigInput) -> Result<WgcfReceipt> {
        let (rev, d) = self.row().await?;
        let output = self.generate_from(rev, &d, input).await?;
        let (revision, document) = self.row().await?;
        Ok(WgcfReceipt {
            output: serde_json::to_value(output)?,
            wgcf_state: project(revision, &document, None),
            wireguard_state: None,
            wg_opdbus_state: None,
        })
    }
    async fn generate_from(
        &self,
        _rev: u64,
        d: &Document,
        input: GenerateConfigInput,
    ) -> Result<GenerateConfigOutput> {
        validate_interface(&input.interface)?;
        let dns = if input.dns.is_empty() {
            vec![
                "1.1.1.1".into(),
                "1.0.0.1".into(),
                "2606:4700:4700::1111".into(),
                "2606:4700:4700::1001".into(),
            ]
        } else {
            input.dns
        };
        ensure!(dns.len() <= 4, "too many DNS servers");
        for dns in &dns {
            dns.parse::<std::net::IpAddr>()?;
        }
        if let Some(mtu) = input.mtu {
            ensure!((1280..=9000).contains(&mtu), "invalid MTU");
        }
        let device = d.device.as_ref().context("WGCF device absent")?;
        let peer = device.config.peers.first().context("WGCF peer absent")?;
        let private = d
            .private_key
            .clone()
            .context("WGCF private key unavailable")?;
        let endpoint = d
            .endpoint_override
            .clone()
            .unwrap_or_else(|| peer.endpoint.v4.clone());
        validate_endpoint(&endpoint)?;
        let profile_id = uuid::Uuid::new_v4().to_string();
        let view = WgcfProfileView {
            profile_id: profile_id.clone(),
            interface: input.interface,
            addresses: vec![
                format!("{}/32", device.config.interface.addresses.v4),
                format!("{}/128", device.config.interface.addresses.v6),
            ],
            dns,
            peer_public_key: peer.public_key.clone(),
            endpoint,
            mtu: Some(input.mtu.unwrap_or(1280)),
        };
        let stored = StoredProfile {
            view: view.clone(),
            private_key: private,
        };
        let (revision, _) = self
            .save(move |d| {
                d.profiles.insert(profile_id, stored);
                Ok(())
            })
            .await?;
        Ok(GenerateConfigOutput {
            revision,
            profile: view,
        })
    }
    async fn apply(&self, input: ApplyConfigInput) -> Result<WgcfReceipt> {
        validate_id(&input.profile_id)?;
        let (_, d) = self.row().await?;
        let profile = d
            .profiles
            .get(&input.profile_id)
            .context("WGCF profile absent")?
            .clone();
        let applied = self.wireguard.apply_profile(&profile).await?;
        let kernel = applied.wireguard_state;
        let link = applied.wg_opdbus_state;
        let id = input.profile_id;
        let (revision, document) = self
            .save(move |d| {
                ensure!(d.profiles.contains_key(&id), "WGCF profile changed");
                d.active_profile = Some(id);
                Ok(())
            })
            .await?;
        Ok(WgcfReceipt {
            output: serde_json::to_value(ApplyConfigOutput {
                revision,
                profile: profile.view.clone(),
                kernel: kernel.clone(),
            })?,
            wgcf_state: project(revision, &document, Some(kernel.clone())),
            wireguard_state: Some(serde_json::to_value(kernel)?),
            wg_opdbus_state: Some(link),
        })
    }
    async fn receipt(
        &self,
        revision: u64,
        d: Document,
        kernel: Option<WireGuardState>,
        wireguard_state: Option<Value>,
    ) -> Result<WgcfReceipt> {
        let output = status_view(revision, &d, kernel.clone())?;
        Ok(WgcfReceipt {
            output: serde_json::to_value(&output)?,
            wgcf_state: project(revision, &d, kernel.clone()),
            wireguard_state,
            wg_opdbus_state: None,
        })
    }
}

fn status_view(
    revision: u64,
    d: &Document,
    kernel: Option<WireGuardState>,
) -> Result<WgcfStatusOutput> {
    let device = d.device.as_ref().context("WGCF device absent")?;
    Ok(WgcfStatusOutput {
        revision,
        account: account_view(&device.account),
        device: device_view(device),
        active_profile: d
            .active_profile
            .as_ref()
            .and_then(|id| d.profiles.get(id))
            .map(|p| p.view.clone()),
        kernel,
    })
}
fn project(revision: u64, d: &Document, kernel: Option<WireGuardState>) -> WgcfState {
    WgcfState {
        config: WgcfConfig::default(),
        status: status_view(revision, d, kernel).ok(),
        profiles: d.profiles.values().map(|p| p.view.clone()).collect(),
    }
}
fn account_view(a: &Account) -> WgcfAccountView {
    WgcfAccountView {
        id: a.id.clone(),
        account_type: a.account_type.clone(),
        role: a.role.clone(),
        warp_plus: a.warp_plus,
        quota: a.quota,
        premium_data: a.premium_data,
        referral_count: a.referral_count,
        referral_renewal_countdown: a.referral_renewal_countdown,
        created: a.created.clone(),
        updated: a.updated.clone(),
    }
}
fn device_view(d: &SourceDevice) -> WgcfDeviceView {
    WgcfDeviceView {
        id: d.id.clone(),
        enabled: d.enabled,
        name: d.name.clone(),
        model: d.model.clone(),
        device_type: d.device_type.clone(),
        created: d.created.clone(),
        updated: d.updated.clone(),
        warp_enabled: d.warp_enabled,
        public_key: d.key.clone(),
    }
}
fn validate_text(v: &str, max: usize) -> Result<()> {
    ensure!(
        !v.trim().is_empty() && v.len() <= max && !v.chars().any(char::is_control),
        "invalid WGCF text"
    );
    Ok(())
}
fn validate_id(v: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(v)?.to_string() == v,
        "invalid WGCF profile id"
    );
    Ok(())
}
fn validate_api_id(v: &str) -> Result<()> {
    ensure!(
        !v.is_empty()
            && v.len() <= 128
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid WGCF API identifier"
    );
    Ok(())
}
fn validate_wireguard_key(v: &str) -> Result<()> {
    let decoded = STANDARD.decode(v).context("invalid WireGuard public key")?;
    ensure!(decoded.len() == 32, "invalid WireGuard public key");
    Ok(())
}
fn validate_interface(v: &str) -> Result<()> {
    ensure!(
        !v.is_empty()
            && v.len() <= 15
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid WireGuard interface"
    );
    Ok(())
}
fn validate_endpoint(v: &str) -> Result<()> {
    ensure!(
        v.len() <= 255 && !v.is_empty() && !v.chars().any(char::is_whitespace),
        "invalid WGCF endpoint"
    );
    let url = reqwest::Url::parse(&format!("https://{v}"))?;
    ensure!(
        url.host_str().is_some()
            && url.port().is_some()
            && url.path() == "/"
            && url.username().is_empty()
            && url.password().is_none(),
        "WGCF endpoint must be host:port"
    );
    Ok(())
}

fn validate_source(device: &SourceDevice) -> Result<()> {
    for value in [&device.account.id, &device.config.client_id] {
        validate_text(value, 256)?;
    }
    validate_api_id(&device.id)?;
    validate_wireguard_key(&device.key)?;
    validate_text(&device.account.license, 256)?;
    ensure!(
        device.account.quota.is_finite()
            && device.account.premium_data.is_finite()
            && device.account.referral_count.is_finite()
            && device.account.referral_renewal_countdown.is_finite(),
        "WGCF account contains non-finite counters"
    );
    device
        .config
        .interface
        .addresses
        .v4
        .parse::<std::net::Ipv4Addr>()
        .context("invalid WGCF IPv4 address")?;
    device
        .config
        .interface
        .addresses
        .v6
        .parse::<std::net::Ipv6Addr>()
        .context("invalid WGCF IPv6 address")?;
    ensure!(
        !device.config.peers.is_empty() && device.config.peers.len() <= 4,
        "invalid WGCF peer count"
    );
    for peer in &device.config.peers {
        validate_wireguard_key(&peer.public_key)?;
        validate_endpoint(&peer.endpoint.host)?;
        validate_endpoint(&peer.endpoint.v4)?;
        validate_endpoint(&peer.endpoint.v6)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;
    use op_state::StatePlugin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PUBLIC_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    fn kernel_state() -> WireGuardState {
        WireGuardState {
            interfaces: vec![super::super::super::wireguard::WireGuardInterface {
                name: "wgcf".into(),
                public_key: Some(PUBLIC_KEY.into()),
                listen_port: None,
                fwmark: None,
                peers: vec![],
            }],
        }
    }
    fn wg_opdbus_projection() -> Value {
        let interface = kernel_state().interfaces.into_iter().next().unwrap();
        serde_json::to_value(super::super::super::wg_opdbus::WgOpdbusAggregateState {
            service_persona: super::super::super::wg_opdbus::AnnaScribeServicePersona::default(),
            interfaces: std::collections::BTreeMap::from([(
                "wgcf".into(),
                super::super::super::wg_opdbus::WgOpdbusState {
                    service_persona:
                        super::super::super::wg_opdbus::AnnaScribeServicePersona::default(),
                    interface: Some(interface),
                    addresses: vec!["172.16.0.2/32".into()],
                    mtu: Some(1280),
                    admin_up: true,
                },
            )]),
        })
        .unwrap()
    }
    struct Kernel;
    #[async_trait]
    impl WgcfWireGuard for Kernel {
        fn generate_keypair(&self) -> Result<(String, String)> {
            Ok(("PRIVATE".into(), PUBLIC_KEY.into()))
        }
        fn public_from_private(&self, private: &str) -> Result<String> {
            ensure!(private == "PRIVATE", "bad private key");
            Ok(PUBLIC_KEY.into())
        }
        async fn apply_profile(&self, _: &StoredProfile) -> Result<WireGuardApplyReceipt> {
            Ok(WireGuardApplyReceipt {
                wireguard_state: kernel_state(),
                wg_opdbus_state: wg_opdbus_projection(),
            })
        }
        async fn state(&self) -> Result<WireGuardState> {
            Ok(kernel_state())
        }
    }
    fn source_with(token: bool, name: &str, license: &str) -> Value {
        let mut value = json!({
            "id":"device-1","enabled":true,"name":name,"model":"PC","type":"Android",
            "created":"2026-01-01T00:00:00Z","updated":"2026-01-01T00:00:00Z","warp_enabled":true,"key":PUBLIC_KEY,
            "account":{"id":"account-1","account_type":"free","role":"child","warp_plus":false,"quota":0,
                "premium_data":0,"referral_count":0,"referral_renewal_countdown":0,"created":"2026-01-01T00:00:00Z",
                "updated":"2026-01-01T00:00:00Z","license":license},
            "config":{"client_id":"client","interface":{"addresses":{"v4":"172.16.0.2","v6":"2606:4700::1"}},
                "peers":[{"endpoint":{"host":"engage.cloudflareclient.com:2408","v4":"162.159.192.1:2408","v6":"[2606:4700:d0::1]:2408"},"public_key":PUBLIC_KEY}]}
        });
        if token {
            value["token"] = json!("PRIVATE_TOKEN");
        }
        value
    }

    fn validate_contract(method: &str, input: &Value, output: &Value) {
        let schema = super::super::super::wgcf::WgcfPlugin::new(WgcfConfig::default())
            .schema()
            .unwrap();
        let declaration = &schema.methods[method];
        let input_schema = serde_json::to_value(&declaration.args).unwrap();
        jsonschema::validator_for(&input_schema)
            .unwrap()
            .validate(input)
            .unwrap();
        let output_schema = serde_json::to_value(declaration.returns.as_ref().unwrap()).unwrap();
        jsonschema::validator_for(&output_schema)
            .unwrap()
            .validate(output)
            .unwrap();
    }

    fn assert_public(receipt: &WgcfReceipt) {
        let wire = serde_json::to_string(receipt).unwrap();
        assert!(!wire.contains("PRIVATE"));
        assert!(!wire.contains("NEW_LICENSE"));
    }

    #[tokio::test]
    async fn all_eight_methods_use_native_readback_match_schema_and_never_project_secrets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let server = tokio::spawn(async move {
            for index in 0..9 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 8192];
                let count = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..count]);
                seen.fetch_add(1, Ordering::SeqCst);
                let body = match index {
                    0 => {
                        assert!(request.starts_with("POST /v0a1922/reg "));
                        assert!(request.contains("\"tos\":"));
                        source_with(true, "decoy", "PRIVATE_LICENSE")
                    }
                    1 => {
                        assert!(request.starts_with("GET /v0a1922/reg/device-1 "));
                        source_with(false, "decoy", "PRIVATE_LICENSE")
                    }
                    2 => {
                        assert!(
                            request
                                .starts_with("PATCH /v0a1922/reg/device-1/account/reg/device-1 ")
                        );
                        assert!(request.contains("\"name\":\"renamed\""));
                        json!([])
                    }
                    3 => {
                        assert!(request.starts_with("GET /v0a1922/reg/device-1 "));
                        source_with(false, "renamed", "PRIVATE_LICENSE")
                    }
                    4 => {
                        assert!(request.starts_with("PUT /v0a1922/reg/device-1/account "));
                        assert!(request.contains("\"license\":\"NEW_LICENSE\""));
                        json!({})
                    }
                    5 => {
                        assert!(request.starts_with("GET /v0a1922/reg/device-1 "));
                        source_with(false, "renamed", "NEW_LICENSE")
                    }
                    6 => {
                        assert!(request.starts_with("PATCH /v0a1922/reg/device-1 "));
                        assert!(request.contains(&format!("\"key\":\"{PUBLIC_KEY}\"")));
                        source_with(false, "renamed", "NEW_LICENSE")
                    }
                    7 | 8 => {
                        assert!(request.starts_with("GET /v0a1922/reg/device-1 "));
                        source_with(false, "renamed", "NEW_LICENSE")
                    }
                    _ => unreachable!(),
                };
                if index != 0 {
                    assert!(
                        request
                            .to_ascii_lowercase()
                            .contains("authorization: bearer private_token")
                    );
                }
                let body = serde_json::to_vec(&body).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
        });
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let owner = WgcfOwner::new(documents, Arc::new(Kernel), endpoint, true).unwrap();
        let register_input = json!({"accept_tos":true,"device_model":"PC"});
        let registered = owner
            .dispatch("register", register_input.clone())
            .await
            .unwrap();
        validate_contract("register", &register_input, &registered.output);
        assert_public(&registered);

        let empty = json!({});
        let status = owner.dispatch("status", empty.clone()).await.unwrap();
        validate_contract("status", &empty, &status.output);
        assert_eq!(
            status.wgcf_state.status.as_ref().unwrap().device.id,
            "device-1"
        );
        assert_eq!(
            status.wgcf_state.status.as_ref().unwrap().kernel,
            Some(kernel_state())
        );
        assert_public(&status);

        for input in [
            json!({"license":null,"device_name":"renamed"}),
            json!({"license":"NEW_LICENSE","device_name":null}),
        ] {
            let updated = owner.dispatch("update", input.clone()).await.unwrap();
            validate_contract("update", &input, &updated.output);
            assert_public(&updated);
        }

        let rotated = owner.dispatch("rotate_keys", empty.clone()).await.unwrap();
        validate_contract("rotate_keys", &empty, &rotated.output);
        assert_public(&rotated);

        let endpoint_input = json!({"endpoint":"162.159.193.1:2408"});
        let endpoint = owner
            .dispatch("set_endpoint", endpoint_input.clone())
            .await
            .unwrap();
        validate_contract("set_endpoint", &endpoint_input, &endpoint.output);
        assert_public(&endpoint);

        let generate_input = json!({"interface":"wgcf","dns":[],"mtu":1280});
        let generated = owner
            .dispatch("generate_config", generate_input.clone())
            .await
            .unwrap();
        validate_contract("generate_config", &generate_input, &generated.output);
        assert_public(&generated);
        let profile_id = generated.output["profile"]["profile_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let apply_input = json!({"profile_id":profile_id});
        let applied = owner
            .dispatch("apply_config", apply_input.clone())
            .await
            .unwrap();
        validate_contract("apply_config", &apply_input, &applied.output);
        assert_eq!(applied.output["kernel"]["interfaces"][0]["name"], "wgcf");
        assert_eq!(
            applied.wg_opdbus_state.as_ref().unwrap()["interfaces"]["wgcf"]["admin_up"],
            true
        );
        assert_public(&applied);

        let refresh_input = json!({"kind":"all"});
        let refreshed = owner
            .dispatch("refresh", refresh_input.clone())
            .await
            .unwrap();
        validate_contract("refresh", &refresh_input, &refreshed.output);
        assert_eq!(
            refreshed.wgcf_state.status.as_ref().unwrap().kernel,
            Some(kernel_state())
        );
        assert_public(&refreshed);
        server.await.unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 9);
    }

    #[tokio::test]
    async fn update_rejects_name_and_license_when_native_readback_does_not_match() {
        for (input, request_prefix, first_response) in [
            (
                json!({"license":null,"device_name":"requested-name"}),
                "PATCH /v0a1922/reg/device-1/account/reg/device-1 ",
                json!([]),
            ),
            (
                json!({"license":"REQUESTED_LICENSE","device_name":null}),
                "PUT /v0a1922/reg/device-1/account ",
                json!({}),
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                for (index, body) in [
                    first_response,
                    source_with(false, "decoy", "PRIVATE_LICENSE"),
                ]
                .into_iter()
                .enumerate()
                {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut bytes = vec![0; 8192];
                    let count = socket.read(&mut bytes).await.unwrap();
                    let request = String::from_utf8_lossy(&bytes[..count]);
                    if index == 0 {
                        assert!(request.starts_with(request_prefix));
                    } else {
                        assert!(request.starts_with("GET /v0a1922/reg/device-1 "));
                    }
                    let body = serde_json::to_vec(&body).unwrap();
                    socket
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    socket.write_all(&body).await.unwrap();
                }
            });
            let documents = Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_in_memory().unwrap(),
            ));
            let owner = WgcfOwner::new(documents, Arc::new(Kernel), endpoint, true).unwrap();
            let device: SourceDevice =
                serde_json::from_value(source_with(false, "decoy", "PRIVATE_LICENSE")).unwrap();
            owner
                .save(move |document| {
                    document.token = Some("PRIVATE_TOKEN".into());
                    document.device = Some(device);
                    Ok(())
                })
                .await
                .unwrap();
            assert!(owner.dispatch("update", input).await.is_err());
            let (_, unchanged) = owner.row().await.unwrap();
            let unchanged = unchanged.device.unwrap();
            assert_eq!(unchanged.name.as_deref(), Some("decoy"));
            assert_eq!(unchanged.account.license, "PRIVATE_LICENSE");
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejected_registration_does_not_connect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let owner = WgcfOwner::new(documents, Arc::new(Kernel), endpoint, true).unwrap();
        assert!(
            owner
                .dispatch("register", json!({"accept_tos":false,"device_model":"PC"}))
                .await
                .is_err()
        );
        assert!(
            owner
                .dispatch("update", json!({"license":"one","device_name":"two"}),)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}
