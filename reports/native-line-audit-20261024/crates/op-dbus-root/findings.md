# Findings

## F-1 — Native wording and dependency identifiers retained

The only source-tree occurrence is `Cargo.toml:188`, a package description of the Linux control plane. The 11 `Cargo.lock` occurrences are exact third-party crate/package identifiers: `native-tls`, `tokio-native-tls`, and `rustls-native-certs`. Their per-occurrence contexts and JEV records are in `occurrences.tsv`. Renaming dependency identifiers would falsify package identity and was not performed.

## F-2 — Reqwest TLS feature defaults are an owner-policy question, not a proven defect

`Cargo.toml:110` declares workspace `reqwest` 0.11 with `json` and `stream` but does not disable default features. The resolved `reqwest` 0.11.27 record includes `hyper-tls`, `native-tls`, `tokio-native-tls` and Rustls dependencies. This is evidence both TLS paths appear in the lockfile graph; it does not establish that native TLS is an unintended runtime path. JEV’s focused feature-policy judgment was insufficient (0.66 confidence; change recommendation only 0.01). No edit was made. Choosing only Rustls could alter TLS root behavior and all consumers of the shared workspace dependency, so an explicit policy decision is needed before changing it.

No findings supported a source change. No locks were manually rewritten. No build, Cargo check, test, lint, formatter, runtime exercise, or deployment was authorized or performed for this audit.
## Follow-up — full bun.lock column coverage

The coordinator-requested exact, unclipped read of `crates/bun.lock:raw:110-270` found no additional native-word occurrence. The earlier truncation uncertainty is closed; the zero-occurrence bun.lock disposition and total count of 12 remain unchanged.

