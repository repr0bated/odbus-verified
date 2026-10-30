# Declarative JEV receipts — op-http

Task `native-line-audit-20261024/op-http`; Codex crate worker; 2026-10-24.
Installed instructions: `xd://eval/judge`. One successful `functions.eval` JavaScript call, title **Judging complete HTTP source contexts**, using `await judge(evidence, {J1, J2, J3})`; no model routing, filesystem traversal, extraction or arbitrary execution. All questions share one evidence state. Tool response printed the complete submitted state and answers. Confidence below is the returned calibrated field, not runtime proof.

## Evidence state

Scope: all eight inventory files fully read; no native occurrence observed. Static judgments only, no dependency/caller/runtime proof.

### Routing source — `crates/op-http/src/router.rs:65-108`

```rust
pub fn nest(mut self, prefix: &'static str, name: &'static str, router: Router) -> Self {
    info!("Mounting service '{}' at {}", name, prefix);
    self.router = self.router.nest(prefix, router);
    self.services.push((prefix, name));
    self
}
pub fn route(mut self, path: &str, method_router: axum::routing::MethodRouter) -> Self {
    self.router = self.router.route(path, method_router);
    self
}
pub fn merge(mut self, router: Router) -> Self {
    self.router = self.router.merge(router);
    self
}
pub fn build(mut self) -> Router {
    if let Some(static_dir) = self.static_dir {
        if static_dir.exists() {
            info!("Serving static files from: {:?}", static_dir);
            self.router = self.router.fallback_service(ServeDir::new(static_dir));
        } else {
            tracing::warn!("Static directory not found: {:?}", static_dir);
        }
    }
    self.router
}
```

### TLS source — `src/tls.rs:81-104,139-255`; `src/server.rs:71-101,109-140`

Submitted source (whitespace compacted) included the full `build_acceptor` match: Disabled → `Ok(None)`; Enabled → `create_tls_acceptor(cert_path, key_path)?` and `Ok(Some(acceptor))`; Auto → `detect_certificates()?`, log chosen paths, create acceptor, otherwise `warn!("No TLS certificates found, falling back to HTTP"); Ok(None)`.

Submitted read-derived priority context was explicitly labelled as a summary, not a quotation: SSL_CERT_PATH + SSL_KEY_PATH if both exist; Cloudflare paths; Nginx/custom paths; Let's Encrypt domains + hostname; Proxmox hostname paths; system default paths; Ok(None).

Submitted actual server source:

```rust
let tls_acceptor = self.config.tls.build_acceptor()?;
if let Some(acceptor) = tls_acceptor {
    let http_listener = TcpListener::bind(http_addr).await.map_err(ServerError::BindError)?;
    let https_listener = TcpListener::bind(https_addr).await.map_err(ServerError::BindError)?;
    let http_router = self.router.clone();
    let http_server = async move {
        info!("HTTP server listening on http://{}", http_addr);
        axum::serve(http_listener, http_router)
            .await
            .map_err(|error| ServerError::BindError(std::io::Error::other(error)))
    };
    // Submitted explanatory summary of omitted HTTPS loop:
    // https_router = self.router; acceptor.accept(stream);
    // http1::Builder::new().serve_connection(io, service).
    tokio::try_join!(http_server, https_server)?;
}
```

### Documentation/inventory state

`SPEC.md:19-29,68-78` lists request_filters.rs, metrics.rs, health.rs and "8 Rust source files". `compare-op-http.md:13-21,31-51`, dated 2026-04-05, claims eight Rust files, zero spec-listed but missing files and these three modules present/implemented. Fully read root/src listings contain Cargo.toml, SPEC.md, compare-op-http.md and five Rust files: lib.rs, middleware.rs, router.rs, server.rs, tls.rs. `lib.rs:22-25` declares middleware, router, server, tls.

## Questions, contrary criteria, answers and decisions

### J1 — router execution meaning

Question: Classify the router behavior shown. Does this source itself make an implicit AI provider/model/profile override, or only compose caller-supplied HTTP routes? Do not certify external routers.

Criteria:
- `http_composition_only`: Paths/prefixes compose provided Axum routers and static serving, not AI model/provider authority.
- `implicit_ai_override`: Source selects or overrides an AI provider/model/profile without explicit admitted selection.
- `insufficient`: Evidence cannot distinguish these.

Returned: `choice=http_composition_only`, `confidence=1`, probabilities `{http_composition_only:1, implicit_ai_override:0, insufficient:0}`.

Decision: no native-specific fix. This library's URL router is not AI model routing. Caller-supplied handlers are outside certification.

### J2 — certificate precedence and plaintext path

Question: Classify certificate auto-selection and HTTPS behavior, including contrary outcomes. Is it AI selection/schema duplication, ordinary TLS configuration, or insufficient? Separately record risk via outcome label if HTTPS also serves the same router on plaintext and Auto may fall back.

Criteria:
- `tls_transport_with_plaintext_risk`: Certificate precedence is transport configuration, not an AI/plugin schema authority; TLS branch also serves same router over HTTP, and Auto permits HTTP fallback. Exposure depends on callers/deployment.
- `implicit_ai_or_schema_override`: Certificate selection actually overrides AI model/provider or plugin schema authority.
- `tls_only_no_plaintext_path`: TLS configuration ensures HTTPS-only handling with no plaintext/fallback path.
- `insufficient`: Cannot classify based on evidence.

Returned: `choice=tls_transport_with_plaintext_risk`, `confidence=1`, probabilities `{tls_transport_with_plaintext_risk:1, implicit_ai_or_schema_override:0, tls_only_no_plaintext_path:0, insufficient:0}`.

Decision: no native-specific fix. Record generic transport risk G1 separately; no source mutation or deployed exposure claim.

### J3 — document currency

Question: Classify current documentation comparison against actual fully read directory/module evidence. Limit to the assigned crate; do not claim runtime or native occurrences.

Criteria:
- `generic_stale_inventory`: Historical docs overstate source inventory/modules; generic documentation drift outside native-specific edit scope.
- `native_specific_defect`: Evidence establishes a native wording/selection defect requiring this audit repair.
- `current_accurate`: Docs match current source file inventory.
- `insufficient`: Evidence insufficient.

Returned: `choice=generic_stale_inventory`, `confidence=1`, probabilities `{generic_stale_inventory:1, native_specific_defect:0, current_accurate:0, insufficient:0}`.

Decision: report-only generic documentation finding G2. No blanket renaming or unrelated cleanup.

## Coverage boundary

The zero-occurrence result comes from full sequential reads documented in `coverage.tsv`, not from JEV inference or a search. No occurrence was manufactured to obtain a judgment. All substantive source classifications above were submitted together with read evidence. No uncertain (<0.6) answer occurred.
