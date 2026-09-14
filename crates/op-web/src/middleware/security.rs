//! Security middleware for op-web
//!
//! Provides IP-based security zones. All cross-zone elevation must go through
//! the WireGuard-authenticated session ledger (A.N.N.A. Scribe) at the gateway
//! layer, not through HTTP headers handled here.
//!
//! Audit item #4: the static `BYPASS_API_KEYS` allow-list that previously
//! short-circuited zone enforcement has been removed. There is no static
//! header-based zone bypass anywhere in this file.

use axum::{
    extract::{ConnectInfo, Request},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use op_core::security::AccessZone;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use tracing::{debug, warn};

/// Extract a bearer token from request headers (no static allow-list).
///
/// Returns the first non-empty value found. The caller is responsible for
/// validating the token against the authoritative session/peer ledger.
pub fn extract_auth_token(headers: &HeaderMap) -> Option<String> {
    if let Some(raw) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(bearer) = raw.trim().strip_prefix("Bearer ") {
            let token = bearer.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    None
}

fn is_trusted_proxy(addr: &SocketAddr) -> bool {
    let ip = addr.ip();
    if ip.is_loopback() {
        return true;
    }
    // Trust WireGuard mesh subnet (10.200.0.0/16) as a forwarding proxy.
    if let IpAddr::V4(v4) = ip {
        let octets = v4.octets();
        if octets[0] == 10 && octets[1] == 200 {
            return true;
        }
    }
    false
}

/// Render a header value safely for log output: trims to 64 bytes, replaces
/// any byte < 0x20 or >= 0x7f with `\xNN`, and drops the obvious offenders
/// (newlines, tabs, NULs). Used only for diagnostic warnings about malformed
/// proxy headers; never for security decisions.
fn sanitize_header_for_log(raw: &str) -> String {
    const MAX: usize = 64;
    let mut out = String::with_capacity(raw.len().min(MAX) + 8);
    for (i, b) in raw.bytes().enumerate() {
        if i >= MAX {
            out.push('\u{2026}'); // ellipsis
            break;
        }
        if (0x20..0x7f).contains(&b) && b != b'\\' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{:02x}", b));
        }
    }
    out
}

/// Parse a single XFF / X-Real-IP segment, rejecting anything that cannot be
/// a clean IP address. Returns `None` on malformed input.
///
/// Accepted forms:
///   * Plain IPv4 (`1.2.3.4`) or IPv6 (`::1`)
///   * IPv4 with port (`1.2.3.4:8080`) \u2014 port stripped, address parsed
///   * Bracketed IPv6 with port (`[::1]:1234`) \u2014 brackets+port stripped
///
/// Rejected: anything containing whitespace, commas, semicolons, NULs, or
/// other control characters, and anything that is not ultimately a valid
/// `IpAddr` after the port-strip.
fn parse_forwarded_segment(segment: &str) -> Option<IpAddr> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Reject anything that smuggles a second value or control chars into
    // the first segment (e.g. `1.2.3.4, evil`, `1.2.3.4;drop`, NULs, etc.).
    if trimmed
        .bytes()
        .any(|b| matches!(b, b',' | b';' | b'\n' | b'\r' | b'\0' | b' ' | b'\t'))
    {
        return None;
    }

    // Strip the optional `[ipv6]:port` brackets and `:port` suffix tolerated
    // by some proxies. We only need the address.
    let candidate: &str = if let Some(stripped) = trimmed.strip_prefix('[') {
        // [::1]:1234 form \u2014 take everything up to (but not including) the
        // first `]`. The trailing `:port` (if any) is discarded.
        stripped.split(']').next().unwrap_or(stripped)
    } else if let Some((host, _port)) = trimmed.rsplit_once(':') {
        // Possibly IPv4-with-port. An unbracketed IPv6 also matches this
        // pattern (it contains colons), so we have to be careful.
        // Treat as `host:port` only if `host` looks like exactly 4 dotted
        // numeric octets (i.e. a candidate IPv4). Otherwise leave the full
        // string and let `IpAddr::from_str` decide (which will correctly
        // accept unbracketed IPv6 like `::1`).
        if looks_like_ipv4_octets(host) {
            host
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    IpAddr::from_str(candidate).ok()
}

/// Returns true iff `s` is exactly 4 dot-separated numeric octets, each
/// 1\u20133 ASCII digits. Does NOT validate the 0\u2013255 range \u2014 that is left to
/// `IpAddr::from_str` which will reject e.g. `999.0.0.1`.
fn looks_like_ipv4_octets(s: &str) -> bool {
    let mut count = 0usize;
    for part in s.split('.') {
        count += 1;
        if count > 4 {
            return false;
        }
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    count == 4
}

/// Extract the client IP. Proxy headers (`X-Forwarded-For`, `X-Real-IP`) are
/// only honoured when the TCP peer is a known trusted proxy. Malformed header
/// values fall back to the real socket peer rather than a placeholder.
pub fn extract_ip(headers: &HeaderMap, addr: Option<&SocketAddr>) -> String {
    if addr.map(is_trusted_proxy).unwrap_or(false) {
        // X-Forwarded-For \u2014 take leftmost (original client).
        if let Some(forwarded) = headers.get("x-forwarded-for") {
            if let Ok(s) = forwarded.to_str() {
                if let Some(first) = s.split(',').next() {
                    if let Some(ip) = parse_forwarded_segment(first) {
                        return ip.to_string();
                    }
                    warn!(
                        raw = %sanitize_header_for_log(first),
                        "Rejected malformed X-Forwarded-For first segment"
                    );
                }
            }
        }

        // X-Real-IP fallback.
        if let Some(real_ip) = headers.get("x-real-ip") {
            if let Ok(s) = real_ip.to_str() {
                if let Some(ip) = parse_forwarded_segment(s) {
                    return ip.to_string();
                }
                warn!(
                    raw = %sanitize_header_for_log(s),
                    "Rejected malformed X-Real-IP"
                );
            }
        }
    }

    // Real socket peer \u2014 not spoofable.
    if let Some(addr) = addr {
        return addr.ip().to_string();
    }

    // No socket info at all: fall back to the unspecified address. This
    // classifies as a public-zone caller in `AccessZone::from_ip`, which is
    // the safe default.
    "0.0.0.0".to_string()
}

/// Middleware that attaches an `AccessZone` to the request based on the
/// client IP. There is no header-based zone override; all elevation must go
/// through the gateway's WireGuard-authenticated session ledger.
///
/// TODO(audit #4 follow-up): if a header-based override is ever reintroduced,
/// it MUST (a) be opt-in via env var, (b) be accepted only from
/// `is_trusted_proxy()` peers, and (c) carry a short-lived HMAC-signed token
/// rather than a static secret.
pub async fn ip_security_middleware(mut request: Request, next: Next) -> Response {
    let connect_info = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied();
    let headers = request.headers();
    let addr = connect_info.map(|ci| ci.0);
    // `client_ip` is informational only; we feed the same value into
    // `AccessZone::from_ip` below, so logging it is never security-relevant.
    let client_ip = extract_ip(headers, addr.as_ref());

    let zone = AccessZone::from_ip(&client_ip);
    debug!(
        ip = %client_ip,
        zone = ?zone,
        "Resolved request access zone"
    );

    request.extensions_mut().insert(zone);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn loopback() -> SocketAddr {
        "127.0.0.1:1234".parse().unwrap()
    }

    fn public_peer() -> SocketAddr {
        "8.8.8.8:1234".parse().unwrap()
    }

    fn mesh_peer() -> SocketAddr {
        "10.200.0.5:1234".parse().unwrap()
    }

    #[test]
    fn is_trusted_proxy_accepts_loopback_and_mesh_only() {
        assert!(is_trusted_proxy(&loopback()));
        assert!(is_trusted_proxy(&mesh_peer()));
        assert!(!is_trusted_proxy(&public_peer()));
        let lan: SocketAddr = "192.168.1.10:1234".parse().unwrap();
        assert!(!is_trusted_proxy(&lan));
    }

    #[test]
    fn extract_ip_honours_xff_from_loopback() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "203.0.113.7");
    }

    #[test]
    fn extract_ip_ignores_xff_from_untrusted_peer() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        let peer = public_peer();
        assert_eq!(extract_ip(&h, Some(&peer)), "8.8.8.8");
    }

    #[test]
    fn extract_ip_rejects_malformed_xff() {
        let mut h = HeaderMap::new();
        // Not an IP at all.
        h.insert("x-forwarded-for", HeaderValue::from_static("not-an-ip"));
        let peer = loopback();
        // Falls back to the real socket peer rather than honouring junk.
        assert_eq!(extract_ip(&h, Some(&peer)), "127.0.0.1");
    }

    #[test]
    fn extract_ip_rejects_xff_with_commas_in_first_segment() {
        // The split-on-',' already keeps only the first segment, so a comma
        // in a single chunk is impossible. But control chars / semicolons
        // smuggled into the first segment must still be rejected.
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4;injected"),
        );
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "127.0.0.1");
    }

    #[test]
    fn extract_ip_strips_ipv4_port_from_xff() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7:51820"),
        );
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "203.0.113.7");
    }

    #[test]
    fn extract_ip_rejects_hostname_with_port_in_xff() {
        let mut h = HeaderMap::new();
        // `example.com:443` is neither a valid IP nor an IPv4-with-port.
        // Must fall back to socket peer, not be silently accepted.
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("example.com:443"),
        );
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "127.0.0.1");
    }

    #[test]
    fn extract_ip_accepts_bracketed_ipv6_with_port() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("[::1]:1234"));
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "::1");
    }

    #[test]
    fn extract_ip_accepts_unbracketed_ipv6_without_port() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("2001:db8::1"));
        let peer = loopback();
        assert_eq!(extract_ip(&h, Some(&peer)), "2001:db8::1");
    }

    #[test]
    fn extract_ip_falls_back_to_unspecified_without_socket() {
        let h = HeaderMap::new();
        assert_eq!(extract_ip(&h, None), "0.0.0.0");
    }

    #[test]
    fn extract_auth_token_accepts_bearer_only() {
        let mut h = HeaderMap::new();
        h.insert("x-op-mcp-token", HeaderValue::from_static("tok-1"));
        h.insert("authorization", HeaderValue::from_static("Bearer tok-2"));
        assert_eq!(extract_auth_token(&h).as_deref(), Some("tok-2"));

        let mut h2 = HeaderMap::new();
        h2.insert("authorization", HeaderValue::from_static("Bearer tok-2"));
        assert_eq!(extract_auth_token(&h2).as_deref(), Some("tok-2"));

        let h3 = HeaderMap::new();
        assert_eq!(extract_auth_token(&h3), None);
    }

    #[test]
    fn looks_like_ipv4_octets_strict() {
        assert!(looks_like_ipv4_octets("1.2.3.4"));
        assert!(looks_like_ipv4_octets("203.0.113.7"));
        assert!(looks_like_ipv4_octets("0.0.0.0"));
        assert!(!looks_like_ipv4_octets("1.2.3"));
        assert!(!looks_like_ipv4_octets("1.2.3.4.5"));
        assert!(!looks_like_ipv4_octets("example.com"));
        assert!(!looks_like_ipv4_octets("a.b.c.d"));
        assert!(!looks_like_ipv4_octets("1234.0.0.1"));
        assert!(!looks_like_ipv4_octets(""));
        assert!(!looks_like_ipv4_octets("1..3.4"));
    }

    #[test]
    fn sanitize_header_for_log_escapes_control_chars() {
        assert_eq!(sanitize_header_for_log("hello"), "hello");
        assert_eq!(sanitize_header_for_log("a\tb"), "a\\x09b");
        assert_eq!(sanitize_header_for_log("a\nb"), "a\\x0ab");
        assert_eq!(sanitize_header_for_log("a\0b"), "a\\x00b");
    }

    #[test]
    fn sanitize_header_for_log_truncates_long_input() {
        let long = "A".repeat(100);
        let out = sanitize_header_for_log(&long);
        // 64 chars + ellipsis
        assert!(out.len() <= 64 + 3);
        assert!(out.ends_with('\u{2026}'));
    }
}
