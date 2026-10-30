# Findings — op-xray-daemon

## O1 — Cargo author metadata is not grounded to a specific organizational identity

`crates/op-xray-daemon/Cargo.toml:6` attributes authorship to `3tched Contributors`. Context identifies it as generic Cargo author metadata, not a route, plugin ID, or executable name. Current repository guidance distinguishes `3tched-fs` from `threetched` but does not identify the author organization. JEV's semantic interpretation weakly favored generic contributor attribution (0.63 confidence); wording accuracy was insufficient (0.94 confidence). No specific replacement can be substantiated.

**Disposition:** Retain provisionally; no source edit. This is not evidence that the metadata is authoritatively accurate.

## Scope boundaries

The source/docs describe Xray as an inner tunnel not required/active under the current network design, and `VENDORED.md` explicitly disclaims live runtime/deployment proof. No runtime state was inspected. No claim about installed or active service, current catalog, or deployed status is made.