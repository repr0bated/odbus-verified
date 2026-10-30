# JEV judgments

Task: `native-line-audit-20261024/op-dbus-root`. Native-token batch receipt: `jdgb-15b33278eb6f05ec` (`typesafe/jev-latest`), 12/12 states complete, no failures. The batch classified the exact occurrence/context independently, then recommended an action. Full per-occurrence confidence and probabilities are in `occurrences.tsv`.

| State | Intent classification confidence (top choice) | Disposition confidence (top choice) |
|---|---:|---:|
| toml_188 | accurate 0.74 | retain 0.40 |
| lock_2622 | accurate 0.88 | retain 0.95 |
| lock_2624 | accurate 0.89 | retain 0.94 |
| lock_3177 | accurate 0.80 | retain 0.92 |
| lock_3183 | accurate 0.79 | retain 0.88 |
| lock_3567 | accurate 0.88 | retain 0.95 |
| lock_6191 | accurate 0.80 | retain 0.92 |
| lock_6203 | accurate 0.79 | retain 0.88 |
| lock_6622 | accurate 0.98 | retain 0.98 |
| lock_7611 | accurate 0.79 | retain 0.95 |
| lock_7616 | accurate 0.83 | retain 0.94 |
| lock_7782 | accurate 0.95 | retain 0.97 |

`Cargo.toml:188` received additional contextual judgments. The initial narrow prompt was low confidence/insufficient (`meaning=insufficient .30`, `action=retain .13`); a follow-up using package metadata and the repository’s D-Bus-only control-plane invariant judged the description accurate `.69` and retain `.70`. The token batch’s `retain` confidence is only `.40`, so final disposition is human-adjudicated: the description is a broad Linux product description, and no evidence supports changing it.

A separate TLS-context JEV assessment examined `Cargo.toml:110`, lockfile reqwest/hyper-tls/native-tls/rustls dependencies, and the root TLS declarations. It judged a possible implicit native TLS effect at confidence `.49` (probabilities `.66` implicit, `.06` non-selecting, `.28` insufficient), the feature-policy finding insufficient at `.66` (fix `.01`, retain `.22`, insufficient `.77`), and the crate identifiers legitimate at `.98`. Human decision: do not change feature selection on this evidence. `reqwest = { version = "0.11", features = ["json", "stream"] }` leaves defaults enabled; Cargo.lock records both native-TLS and Rustls paths for reqwest 0.11.27. This establishes feature/backend presence in the resolved graph, not that changing the default is required or behavior-preserving. No TLS policy in the assigned scope establishes that native-tls is erroneous. Record this for owner review only; do not rename the legitimate lockfile identifiers.

An initial attempt to await the synchronous `judge_batch` constructor raised `TypeError`; the submitted batch nevertheless completed. A second, correctly invoked batch has the receipt above and is the report’s authoritative per-occurrence evidence. No Cargo or source changes were made based solely on a model recommendation.
Coordinator follow-up evidence gap: read `crates/bun.lock:raw:110-270` as one exact, unclipped range. The full displayed content contains no additional `native`/`Native` occurrence; therefore no new occurrence judgment was needed, and the original 12-occurrence JEV ledger remains complete. This read closes the previously reported column-truncation uncertainty.

