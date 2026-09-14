//! Typed xray-core JSON config structs — generated, not hand-maintained.
//!
//! Source: xray-core (github.com/XTLS/Xray-core) Go struct definitions,
//! extracted into /tmp/xray_core_complete_schema.json and codegen'd via
//! /tmp/xray_codegen.py. Field types are mapped from Go (go_type/json_tag)
//! to Rust; unresolvable nested types fall back to `serde_json::Value`
//! rather than guessing a shape.
//!
//! Provenance: cross-checked field-for-field (APIConfig, DNSConfig,
//! NameServerConfig) against a clone pinned to tag v26.3.27 / commit
//! d2758a023cd7f4174a5a5fa4ff66e487d4342ba0 — the exact `xray version`
//! running in the live `xray` container — and matched exactly. Re-run the
//! extraction+codegen against a fresh checkout if the binary is upgraded.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Source: xray-core `api.go` (`APIConfig`), surface `surface_4`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct APIConfig {
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub listen: String,
    #[serde(default)]
    pub services: Vec<String>,
}

/// Source: xray-core `dns.go` (`NameServerConfig`), surface `surface_4`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NameServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clientIp")]
    pub client_ip: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    #[serde(rename = "skipFallback")]
    pub skip_fallback: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    #[serde(rename = "expectedIPs")]
    pub expected_ips: Vec<String>,
    #[serde(default)]
    #[serde(rename = "expectIPs")]
    pub expect_ips: Vec<String>,
    #[serde(default)]
    #[serde(rename = "queryStrategy")]
    pub query_strategy: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disableCache")]
    pub disable_cache: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "serveStale")]
    pub serve_stale: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "serveExpiredTTL")]
    pub serve_expired_ttl: Option<u32>,
    #[serde(default)]
    #[serde(rename = "finalQuery")]
    pub final_query: bool,
    #[serde(default)]
    #[serde(rename = "unexpectedIPs")]
    pub unexpected_ips: Vec<String>,
}

/// Source: xray-core `dns.go` (`DNSConfig`), surface `surface_4`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DNSConfig {
    #[serde(default)]
    pub servers: Vec<Option<NameServerConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosts: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clientIp")]
    pub client_ip: Option<String>,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    #[serde(rename = "queryStrategy")]
    pub query_strategy: String,
    #[serde(default)]
    #[serde(rename = "disableCache")]
    pub disable_cache: bool,
    #[serde(default)]
    #[serde(rename = "serveStale")]
    pub serve_stale: bool,
    #[serde(default)]
    #[serde(rename = "serveExpiredTTL")]
    pub serve_expired_ttl: u32,
    #[serde(default)]
    #[serde(rename = "disableFallback")]
    pub disable_fallback: bool,
    #[serde(default)]
    #[serde(rename = "disableFallbackIfMatch")]
    pub disable_fallback_if_match: bool,
    #[serde(default)]
    #[serde(rename = "enableParallelQuery")]
    pub enable_parallel_query: bool,
    #[serde(default)]
    #[serde(rename = "useSystemHosts")]
    pub use_system_hosts: bool,
}

/// Source: xray-core `policy.go` (`PolicyConfig`), surface `surface_4`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyConfig {
    #[serde(default)]
    // Retain the digit-key pattern and original nullable values exactly.
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub levels: std::collections::HashMap<u32, Option<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<serde_json::Value>,
}

/// Source: xray-core `dokodemo.go` (`DokodemoConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DokodemoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "allowedNetwork")]
    pub allowed_network: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "rewriteAddress")]
    pub rewrite_address: Option<String>,
    #[serde(default)]
    #[serde(rename = "rewritePort")]
    pub rewrite_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    #[serde(rename = "portMap")]
    pub port_map: std::collections::HashMap<String, String>,
    #[serde(default)]
    #[serde(rename = "followRedirect")]
    pub follow_redirect: bool,
    #[serde(default)]
    #[serde(rename = "userLevel")]
    pub user_level: u32,
}

/// Source: xray-core `http.go` (`HTTPAccount`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HTTPAccount {
    #[serde(default)]
    #[serde(rename = "user")]
    pub username: String,
    #[serde(default)]
    #[serde(rename = "pass")]
    pub password: String,
}

/// Source: xray-core `http.go` (`HTTPServerConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HTTPServerConfig {
    #[serde(default)]
    pub users: Vec<Option<HTTPAccount>>,
    #[serde(default)]
    pub accounts: Vec<Option<HTTPAccount>>,
    #[serde(default)]
    #[serde(rename = "allowTransparent")]
    pub transparent: bool,
    #[serde(default)]
    #[serde(rename = "userLevel")]
    pub user_level: u32,
}

/// Source: xray-core `http.go` (`HTTPRemoteConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HTTPRemoteConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
}

/// Source: xray-core `http.go` (`HTTPClientConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HTTPClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    #[serde(rename = "user")]
    pub username: String,
    #[serde(default)]
    #[serde(rename = "pass")]
    pub password: String,
    #[serde(default)]
    pub servers: Vec<Option<HTTPRemoteConfig>>,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

/// Source: xray-core `socks.go` (`SocksAccount`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SocksAccount {
    #[serde(default)]
    #[serde(rename = "user")]
    pub username: String,
    #[serde(default)]
    #[serde(rename = "pass")]
    pub password: String,
}

/// Source: xray-core `socks.go` (`SocksServerConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SocksServerConfig {
    #[serde(default)]
    #[serde(rename = "auth")]
    pub auth_method: String,
    #[serde(default)]
    pub users: Vec<Option<SocksAccount>>,
    #[serde(default)]
    pub accounts: Vec<Option<SocksAccount>>,
    #[serde(default)]
    pub udp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ip")]
    pub host: Option<String>,
    #[serde(default)]
    #[serde(rename = "userLevel")]
    pub user_level: u32,
}

/// Source: xray-core `socks.go` (`SocksRemoteConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SocksRemoteConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
}

/// Source: xray-core `socks.go` (`SocksClientConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SocksClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    #[serde(rename = "user")]
    pub username: String,
    #[serde(default)]
    #[serde(rename = "pass")]
    pub password: String,
    #[serde(default)]
    pub servers: Vec<Option<SocksRemoteConfig>>,
}

/// Source: xray-core `vmess.go` (`VMessAccount`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VMessAccount {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub security: String,
    #[serde(default)]
    pub experiments: String,
}

/// Source: xray-core `vmess.go` (`VMessDefaultConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VMessDefaultConfig {
    #[serde(default)]
    pub level: u8,
}

/// Source: xray-core `vmess.go` (`VMessInboundConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VMessInboundConfig {
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub clients: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "default")]
    pub defaults: Option<VMessDefaultConfig>,
}

/// Source: xray-core `vmess.go` (`VMessOutboundTarget`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VMessOutboundTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
}

/// Source: xray-core `vmess.go` (`VMessOutboundConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VMessOutboundConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub security: String,
    #[serde(default)]
    pub experiments: String,
    #[serde(default)]
    #[serde(rename = "vnext")]
    pub receivers: Vec<Option<VMessOutboundTarget>>,
}

/// Source: xray-core `vless.go` (`VLessInboundFallback`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VLessInboundFallback {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub alpn: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub dest: serde_json::Value,
    #[serde(default)]
    pub xver: u64,
}

/// Source: xray-core `vless.go` (`VLessInboundConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VLessInboundConfig {
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
    #[serde(default)]
    pub clients: Vec<serde_json::Value>,
    #[serde(default)]
    pub decryption: String,
    #[serde(default)]
    pub fallbacks: Vec<Option<VLessInboundFallback>>,
    #[serde(default)]
    pub flow: String,
    #[serde(default)]
    pub testseed: Vec<u32>,
}

/// Source: xray-core `vless.go` (`VLessReverseConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VLessReverseConfig {
    #[serde(default)]
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sniffing: Option<serde_json::Value>,
}

/// Source: xray-core `vless.go` (`VLessOutboundVnext`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VLessOutboundVnext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub users: Vec<serde_json::Value>,
}

/// Source: xray-core `vless.go` (`VLessOutboundConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VLessOutboundConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub flow: String,
    #[serde(default)]
    pub seed: String,
    #[serde(default)]
    pub encryption: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<VLessReverseConfig>,
    #[serde(default)]
    pub testpre: u32,
    #[serde(default)]
    pub testseed: Vec<u32>,
    #[serde(default)]
    pub vnext: Vec<Option<VLessOutboundVnext>>,
}

/// Source: xray-core `trojan.go` (`TrojanServerTarget`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TrojanServerTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub flow: String,
}

/// Source: xray-core `trojan.go` (`TrojanClientConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TrojanClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub flow: String,
    #[serde(default)]
    pub servers: Vec<Option<TrojanServerTarget>>,
}

/// Source: xray-core `trojan.go` (`TrojanInboundFallback`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TrojanInboundFallback {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub alpn: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub dest: serde_json::Value,
    #[serde(default)]
    pub xver: u64,
}

/// Source: xray-core `trojan.go` (`TrojanUserConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TrojanUserConfig {
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub flow: String,
}

/// Source: xray-core `trojan.go` (`TrojanServerConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TrojanServerConfig {
    #[serde(default)]
    pub users: Vec<Option<TrojanUserConfig>>,
    #[serde(default)]
    pub clients: Vec<Option<TrojanUserConfig>>,
    #[serde(default)]
    pub fallbacks: Vec<Option<TrojanInboundFallback>>,
}

/// Source: xray-core `shadowsocks.go` (`ShadowsocksUserConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShadowsocksUserConfig {
    #[serde(default)]
    #[serde(rename = "method")]
    pub cipher: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
}

/// Source: xray-core `shadowsocks.go` (`ShadowsocksServerConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShadowsocksServerConfig {
    #[serde(default)]
    #[serde(rename = "method")]
    pub cipher: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub users: Vec<Option<ShadowsocksUserConfig>>,
    #[serde(default)]
    pub clients: Vec<Option<ShadowsocksUserConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network")]
    pub network_list: Option<serde_json::Value>,
}

/// Source: xray-core `shadowsocks.go` (`ShadowsocksServerTarget`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShadowsocksServerTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    #[serde(rename = "method")]
    pub cipher: String,
    #[serde(default)]
    pub password: String,
}

/// Source: xray-core `shadowsocks.go` (`ShadowsocksClientConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShadowsocksClientConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    #[serde(rename = "method")]
    pub cipher: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub servers: Vec<Option<ShadowsocksServerTarget>>,
}

/// Source: xray-core `freedom.go` (`FreedomConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FreedomConfig {
    #[serde(default)]
    #[serde(rename = "targetStrategy")]
    pub target_strategy: String,
    #[serde(default)]
    #[serde(rename = "domainStrategy")]
    pub domain_strategy: String,
    #[serde(default)]
    pub redirect: String,
    #[serde(default)]
    #[serde(rename = "userLevel")]
    pub user_level: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<Fragment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<Noise>,
    #[serde(default)]
    pub noises: Vec<Option<Noise>>,
    #[serde(default)]
    #[serde(rename = "proxyProtocol")]
    pub proxy_protocol: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ipsBlocked")]
    pub ips_blocked: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "finalRules")]
    pub final_rules: Vec<Option<FreedomFinalRuleConfig>>,
}

/// Source: xray-core `freedom.go` (`Fragment`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Fragment {
    #[serde(default)]
    pub packets: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "maxSplit")]
    pub max_split: Option<serde_json::Value>,
}

/// Source: xray-core `freedom.go` (`Noise`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Noise {
    #[serde(default)]
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub packet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(rename = "applyTo")]
    pub apply_to: String,
}

/// Source: xray-core `freedom.go` (`FreedomFinalRuleConfig`), surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FreedomFinalRuleConfig {
    #[serde(default)]
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "blockDelay")]
    pub block_delay: Option<serde_json::Value>,
}

/// Source: xray-core `blackhole.go` (`BlackholeConfig`), surface `surface_6`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlackholeConfig {
    #[serde(default)]
    pub response: serde_json::Value,
}

/// Source: xray-core `policy.go` (`Policy`), surface `surface_9`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Policy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handshake: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "connIdle")]
    pub connection_idle: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "uplinkOnly")]
    pub uplink_only: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "downlinkOnly")]
    pub downlink_only: Option<u32>,
    #[serde(default)]
    #[serde(rename = "statsUserUplink")]
    pub stats_user_uplink: bool,
    #[serde(default)]
    #[serde(rename = "statsUserDownlink")]
    pub stats_user_downlink: bool,
    #[serde(default)]
    #[serde(rename = "statsUserOnline")]
    pub stats_user_online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "bufferSize")]
    pub buffer_size: Option<i32>,
}

/// Source: xray-core `policy.go` (`SystemPolicy`), surface `surface_9`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SystemPolicy {
    #[serde(default)]
    #[serde(rename = "statsInboundUplink")]
    pub stats_inbound_uplink: bool,
    #[serde(default)]
    #[serde(rename = "statsInboundDownlink")]
    pub stats_inbound_downlink: bool,
    #[serde(default)]
    #[serde(rename = "statsOutboundUplink")]
    pub stats_outbound_uplink: bool,
    #[serde(default)]
    #[serde(rename = "statsOutboundDownlink")]
    pub stats_outbound_downlink: bool,
}

/// Every inbound-protocol settings struct xray-core defines (dokodemo, http, socks, vmess, vless, trojan, shadowsocks, freedom).
/// Mechanically generated: one field per struct extracted from
/// xray-core surface `surface_5`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct XrayInboundProtocolCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dokodemo_config: Option<DokodemoConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_account: Option<HTTPAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_server_config: Option<HTTPServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_remote_config: Option<HTTPRemoteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_client_config: Option<HTTPClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_account: Option<SocksAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_server_config: Option<SocksServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_remote_config: Option<SocksRemoteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_client_config: Option<SocksClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_account: Option<VMessAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_default_config: Option<VMessDefaultConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_inbound_config: Option<VMessInboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_outbound_target: Option<VMessOutboundTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_outbound_config: Option<VMessOutboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_inbound_fallback: Option<VLessInboundFallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_inbound_config: Option<VLessInboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_reverse_config: Option<VLessReverseConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_outbound_vnext: Option<VLessOutboundVnext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_outbound_config: Option<VLessOutboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_server_target: Option<TrojanServerTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_client_config: Option<TrojanClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_inbound_fallback: Option<TrojanInboundFallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_user_config: Option<TrojanUserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_server_config: Option<TrojanServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_user_config: Option<ShadowsocksUserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_server_config: Option<ShadowsocksServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_server_target: Option<ShadowsocksServerTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_client_config: Option<ShadowsocksClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freedom_config: Option<FreedomConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<Fragment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<Noise>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freedom_final_rule_config: Option<FreedomFinalRuleConfig>,
}

/// Every outbound-protocol settings struct xray-core defines (blackhole, freedom, http, socks, vmess, vless, trojan, shadowsocks).
/// Mechanically generated: one field per struct extracted from
/// xray-core surface `surface_6`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct XrayOutboundProtocolCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blackhole_config: Option<BlackholeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freedom_config: Option<FreedomConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<Fragment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<Noise>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freedom_final_rule_config: Option<FreedomFinalRuleConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_account: Option<HTTPAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_server_config: Option<HTTPServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_remote_config: Option<HTTPRemoteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_client_config: Option<HTTPClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_account: Option<SocksAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_server_config: Option<SocksServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_remote_config: Option<SocksRemoteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks_client_config: Option<SocksClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_account: Option<VMessAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_default_config: Option<VMessDefaultConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_inbound_config: Option<VMessInboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_outbound_target: Option<VMessOutboundTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vmess_outbound_config: Option<VMessOutboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_inbound_fallback: Option<VLessInboundFallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_inbound_config: Option<VLessInboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_reverse_config: Option<VLessReverseConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_outbound_vnext: Option<VLessOutboundVnext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vless_outbound_config: Option<VLessOutboundConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_server_target: Option<TrojanServerTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_client_config: Option<TrojanClientConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_inbound_fallback: Option<TrojanInboundFallback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_user_config: Option<TrojanUserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trojan_server_config: Option<TrojanServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_user_config: Option<ShadowsocksUserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_server_config: Option<ShadowsocksServerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_server_target: Option<ShadowsocksServerTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowsocks_client_config: Option<ShadowsocksClientConfig>,
}
