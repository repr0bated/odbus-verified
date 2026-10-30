# Declarative JEV receipts

Task: `tched-line-audit-20261024/op-assistant-grpc`.

No `tched` substring occurs in the 31 assigned files reviewed line by line. No occurrence/ambiguous cluster is fabricated. Counts of all three per-occurrence outcomes are zero. This evidence-sufficiency judgment is supplemental, not a substitute for the sequential-read ledger.

## J1 — Assistant gateway excerpts: no naming hunk

Tool: `functions.eval`, Python, installed declarative `await judge(state, questions)`; successful call titled `Judging bounded naming evidence`. No model routing, shell, filesystem code, extraction, filtering or tool enabling.

State scope: only naming tokens containing `tched`; do not rename unrelated Assistant API names. Worker full-review observation supplied explicitly as an observation, not as independently verified judge evidence.

Exact contextual source supplied:

`crates/op-assistant-grpc/src/lib.rs:1-12`:
```rust
//! op-assistant-grpc — gRPC gateway for Assistant integration.
//!
//! Architecture:
//! ```text
//!  gRPC Client  →  AssistantGrpcServer  →  Transport (D-Bus | HTTP-RPC)  →  Assistant
//! ```
//!
//! - Authentication verifies session genesis against the identity projection
//!   (same gate as op-cognitive).
//! - Primary transport is D-Bus; falls back to JSON-RPC over HTTP when D-Bus is unavailable.
//! - Each Assistant API surface (agents, sessions, tasks, models, cron, soul, namespace,
//!   memory) is exposed as its own gRPC service.
```

`crates/op-assistant-grpc/src/auth.rs:1-6`:
```rust
//! Admission for the retired standalone Assistant compatibility transport.
//!
//! The canonical bridge verifies SID1 against the private sled, registered
//! principal, revocation state, and exact grants. This process cannot reuse
//! that complete receiver across the bridge-owned Cozo store lock, so it must
//! never turn asserted genesis, trace, public-key, or SID1 claims into access.
```

`crates/op-assistant-grpc/src/transport.rs:113-122`:
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
```

Question `naming_disposition`: Given the exact source excerpts and the bounded task, classify the naming disposition. Do not infer that Assistant is threetched or our filesystem merely from integration. Evaluate actual excerpt text, not occurrences in task instructions.

Criteria:
- `leave_as_is`: Excerpts contain no tched naming token and identify the Assistant compatibility gateway; no naming hunk justified.
- `replace_with_threetched`: An actual excerpt tched token incorrectly names the external executable/surface.
- `replace_with_3tched`: An actual excerpt tched token incorrectly names our filesystem/projection.
- `insufficient_evidence`: The excerpts cannot support any of those bounded conclusions.

Question `coverage_limit`: Does this judge see enough exact source to independently certify all 31 files contain no tched, rather than merely classify the supplied excerpts?

Criteria:
- `excerpts_only`: Only excerpt naming is independently assessable; full-file coverage rests on the worker's sequential-read ledger.
- `full_crate_independent`: Exact complete content of every file was supplied to this judge.

Actual returned receipt:
```json
{
  "naming_disposition": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 1,
    "probabilities": {
      "leave_as_is": 1,
      "replace_with_threetched": 0,
      "replace_with_3tched": 0,
      "insufficient_evidence": 0
    }
  },
  "coverage_limit": {
    "type": "choice",
    "choice": "excerpts_only",
    "confidence": 1,
    "probabilities": {
      "excerpts_only": 1,
      "full_crate_independent": 0
    }
  }
}
```

Decision: preserve all source. No matching naming token exists to edit. JEV independently judged these excerpts only; the full-crate absence conclusion comes from every source line actually read and recorded in coverage.tsv. This is not runtime, build, test or deployment proof. Existing stale architecture prose is not a tched-naming defect and is outside this assignment.
