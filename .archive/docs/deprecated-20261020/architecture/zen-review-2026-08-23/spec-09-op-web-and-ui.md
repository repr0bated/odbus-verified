# Spec 09: `op-web` & `op-web-ui`

**Spec Path**: [`.kiro/specs/op-web/requirements.md`](file:///srv/git/odbus/.kiro/specs/op-web/requirements.md)  
**Domain**: Web Server, REST Proxy & WebSocket Streaming  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Axum web server hosts static SPA bundle and serves REST fallback APIs. | [`crates/op-web/src/main.rs:1-95`](file:///srv/git/odbus/crates/op-web/src/main.rs#L1-L95): Serves SPA and proxies `/api`. | **PASS** |
| **REQ-2** | WebSocket endpoint `/ws` streams live `StateChange` records to clients. | [`crates/op-web/src/state.rs:1-85`](file:///srv/git/odbus/crates/op-web/src/state.rs#L1-L85): Broadcast hub connected to `MutationEngine`. | **PASS** |
| **REQ-3** | Gzip compression and security headers applied to all responses. | [`crates/op-web/src/main.rs`](file:///srv/git/odbus/crates/op-web/src/main.rs): `tower_http::compression::CompressionLayer`. | **PASS** |
| **REQ-4** | Dynamic gRPC proxy fallback for methods not handled locally by Axum. | Handled via Tonic channel forwarding in `op-web`. | **PASS** |
