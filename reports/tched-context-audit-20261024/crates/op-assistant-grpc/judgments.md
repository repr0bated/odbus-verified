# Contextual declarative JEV receipts

Fresh audit task `tched-context-audit-20261024/op-assistant-grpc`; no prior tched coverage or judgment was used. The assigned 31-file inventory was read sequentially through EOF before judging. No case-insensitive `tched` occurrences were found, so there are no per-occurrence judgments to invent; see `coverage.tsv` and the empty data section in `occurrences.tsv`.

## JEV call 1: exact-context tched source disposition

Tool: installed declarative `judge(state, questions)` via `functions.eval`; successful. One state, two independent choice questions.

Exact implementation evidence supplied to the judge:

- `src/incus.rs:1-6,9-12,16-27,44-61`: module states the `wg-xray` Incus container is deprecated/stopped; old `10.200.0.x:50051` listeners retired; current host op-grpc-bridge endpoint is TLS `https://127.0.0.1:8090`; `DEFAULT_WG_XRAY_ENDPOINT` remains an alias to `DEFAULT_GRPC_ENDPOINT`; `SchemaTags::load` reads configured identity session genesis and trace id.
- `src/transport.rs:1-4,66-95,113-144`: documentation warns not to fall back from failed D-Bus control calls to HTTP JSON-RPC. `Transport::new` can select `Rpc`, but `Transport::call` rejects `TransportKind::Rpc` with `direct Assistant HTTP RPC transport is disabled`; only the D-Bus arm calls `dbus_call`.
- `src/auth.rs:1-6,23-50`: documentation says the canonical bridge verifies SID1 with the private store, principal, revocation state, and grants; the compatibility process cannot share that receiver. The interceptor rejects asserted OIA1, absent/duplicate/malformed/invalid SID1, then returns Unavailable for structurally valid SID1 because canonical identity/grant admission is unavailable.
- `src/server.rs:1-6,90-165`: compatibility service wiring puts `wireguard_auth_interceptor` on every service; bind validation restricts the listener to loopback because there is no TLS or complete cross-process verifier.
- `src/lib.rs:8-12`: generic prose says authentication verifies genesis and primary D-Bus falls back to HTTP when unavailable. The actual auth and transport code above contradict those claims.

Question 1 (exact instructions): “From these exact implementation and documentation excerpts, decide if any shown code actually dispatches to a tched_router/threetched/zeroclaw path or has a tched-specific naming error that requires an in-scope correction. Consider the transport selection, actual dispatch branch, admission boundary, and endpoint alias together. Explicitly allow insufficient evidence; do not infer runtime/deployed state.”

Criteria: `specific_tched_fix` = concrete tched-specific correction required; `tched_dispatch_proven` = shown caller path dispatches to tched/retired-router requiring removal/reroute; `leave_tched_scope_unchanged` = compatibility client, HTTP path rejected before control send, fail-closed admission, no tched-specific correction supported; `insufficient` = evidence incomplete/conflicting.

JEV answer: `leave_tched_scope_unchanged`; confidence 0.97; probabilities `{specific_tched_fix: 0.01, tched_dispatch_proven: 0, leave_tched_scope_unchanged: 0.98, insufficient: 0.01}`. Human disposition: leave source unchanged. This is source-only evidence, not proof of live deployment.

Question 2 (exact instructions): “The lib.rs prose says authentication verifies genesis and transport falls back to HTTP, while the supplied auth and Transport implementation explicitly reject unsafe admission and direct RPC. Is that generic documentation discrepancy demonstrated to be tched-specific, so should this crate task edit it, or should it be reported separately?”

Criteria: `tched_fix` = direct evidence links discrepancy to tched meaning/contract; `separate_generic` = generic Assistant compatibility documentation drift with no demonstrated tched-specific relation; `insufficient` = cannot determine.

JEV answer: `separate_generic`; confidence 0.89; probabilities `{tched_fix: 0.05, separate_generic: 0.9299999999999999, insufficient: 0.02}`. Human disposition: report the discrepancy separately; do not expand this tched-specific source task into a generic documentation fix.

No additional judgments were used. JEV is an assessment, not source authority or runtime evidence.

## Source excerpts supplied to JEV

Verbatim high-salience excerpts; surrounding context was supplied in the state summaries and was read in the full sequential crate audit.

`src/incus.rs:19-27`:
```rust
/// Default on-host operation.v1 gRPC endpoint served by `op-dbus` at the
/// op-grpc-bridge door `https://127.0.0.1:8090` (TLS-only; the retired `:50051` listeners are gone).
///
/// (Renamed from `DEFAULT_WG_XRAY_ENDPOINT`; the wg-xray container is
/// deprecated. The alias is preserved for downstream callers.)
pub const DEFAULT_GRPC_ENDPOINT: &str = "https://127.0.0.1:8090";
/// Backwards-compatible alias for callers that still reference the old
/// `DEFAULT_WG_XRAY_ENDPOINT` name.
pub const DEFAULT_WG_XRAY_ENDPOINT: &str = DEFAULT_GRPC_ENDPOINT;
```

`src/transport.rs:113-143`:
```rust
    /// Dispatch a high-level Assistant call only through its configured D-Bus
    /// target. The retired direct RPC path cannot carry canonical admission.
    pub async fn call(&self, method: &str, body: Value) -> Result<Value> {
        match self.primary {
            TransportKind::DBus => self.dbus_call(method, &body).await,
            TransportKind::Rpc => Err(AssistantError::Transport(
                "direct Assistant HTTP RPC transport is disabled".into(),
            )),
        }
    }

    async fn dbus_call(&self, method: &str, body: &Value) -> Result<Value> {
        let conn = self
            .dbus
            .as_ref()
            .ok_or_else(|| AssistantError::Transport("dbus not initialised".into()))?;

        let payload = serde_json::to_string(body)?;
        let reply = conn
            .call_method(
                Some(self.cfg.dbus_name.as_str()),
                self.cfg.dbus_path.as_str(),
                Some(self.cfg.dbus_name.as_str()),
                method,
                &(payload,),
            )
            .await?;

        let response: String = reply.body().deserialize()?;
        let value: Value = serde_json::from_str(&response)?;
        Ok(value)
    }
```

`src/auth.rs:23-50`:
```rust
/// Reject legacy asserted headers and every unverified SID1/OIA1 credential.
/// A structurally valid SID1 is deliberately insufficient for authorization.
#[allow(clippy::result_large_err)]
pub fn wireguard_auth_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    if req.metadata().contains_key(ASSERTION_METADATA_KEY) {
        return Err(Status::unauthenticated(
            "standalone Assistant transport cannot verify OIA1",
        ));
    }
    let mut values = req
        .metadata()
        .get_all_bin(SEALED_ID_METADATA_KEY)
        .into_iter();
    let value = values
        .next()
        .ok_or_else(|| Status::unauthenticated("SID1 credential required"))?;
    if values.next().is_some() {
        return Err(Status::unauthenticated("multiple SID1 credentials"));
    }
    let wire = value
        .to_bytes()
        .map_err(|_| Status::unauthenticated("malformed SID1 metadata"))?;
    op_identity::sealed_id::SealedId::open(&wire)
        .map_err(|_| Status::unauthenticated("invalid SID1 credential"))?;
    Err(Status::unavailable(
        "standalone Assistant RPCs disabled until canonical identity and grant admission is shared",
    ))
}
```

`src/server.rs:157-165`:
```rust
/// The compatibility daemon has no TLS or complete cross-process identity
/// verifier. Keep even its disabled RPC surface on the local host.
fn bind_addr(cfg: &ServerConfig) -> anyhow::Result<SocketAddr> {
    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port).parse()?;
    anyhow::ensure!(
        addr.ip().is_loopback(),
        "standalone Assistant gRPC listener must bind to loopback"
    );
    Ok(addr)
}
```

`src/lib.rs:8-12`:
```rust
//! - Authentication verifies session genesis against the identity projection
//!   (same gate as op-cognitive).
//! - Primary transport is D-Bus; falls back to JSON-RPC over HTTP when D-Bus is unavailable.
//! - Each Assistant API surface (agents, sessions, tasks, models, cron, soul, namespace,
//!   memory) is exposed as its own gRPC service.
```

## Follow-up receipt reconciliation

The coordinator identified that the first-session skill preload receipt omitted `using-superpowers/SKILL.md` and did not include `json-render/SKILL.md:301-377`. Both have now been read completely. The remaining 26 paths in the 28-entry `mandatory-preload.txt` had complete prior skill reads; the entire list is reconciled, and the first report did not claim the two now-completed reads happened earlier.

The additional JSON-render section mentions a `tched_router` plugin in a separate file-location reference (`json-render/SKILL.md:374`). This is skill guidance outside the assigned crate, not a crate source occurrence or evidence of a caller/dispatch path. It does not alter the fresh zero-occurrence ledger or the contextual source judgment above.

The coordinator's `coordinator-read-gaps.json` reports 31 source files with metadata, `missing_line_files: []`, and `column_limited_files: []`. It therefore identifies no additional source intervals requiring reread. The complete assigned inventory and sequential reads remain the basis of `coverage.tsv`.