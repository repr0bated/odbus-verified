# Bounded projector repair — JEV decision

Date: 2026-10-24. Static decision only. No source edits, Cargo, tests, runtime calls, or delegation. Root owns implementation and receiver build. JEV was invoked directly once with `judge()`; exact typed outputs follow.

## Evidence reviewed

- `crates/op-state-store/src/event_chain.rs:33-60`: `OperationType::MethodCall` serializes as `method_call`.
- `crates/op-state-store/src/event_chain.rs:724-748` and `:204-241`: the canonical method input envelope contains `method` and `args`; `ChainEvent::new` stores the canonicalized input patch in `input_payload`.
- `crates/op-grpc-bridge/src/method_receipt_audit.rs:473-501`: method audit input also carries `operation_id`, `args_digest`, `method`, and `args`.
- The prior traced event→footprint→Snowball path preserves that envelope under `block.data.payload.input_payload`. The operation/action maps to the method name.
- `crates/op-grpc-bridge/src/chain_vector_projector.rs:81-143`: method projection currently passes the whole `input_payload` to `described_fields`; signal projection intentionally consumes the direct signal payload. The candidate extraction gates on `payload.op == "method_call"` and matching `input.method == block.action`, preserving the direct-payload path for signals and historical/nonmatching events.
- `crates/op-state-store/src/schema_projection.rs:130-181`: `described_fields` walks object values by declared schema fields, ignores undeclared keys, but scalar values go through `scalar_text` and may emit a root scalar. The supplied code's `input.get("args").unwrap_or(&Null)` defaults missing args only; a present scalar is not converted to Null.
- `crates/op-grpc-bridge/src/chain_vector_projector.rs:276-334`: existing test uses retired `TchedRouterPlugin` and a direct Chat payload. `crates/op-plugins/src/state_plugins/snowball_plugin.rs:389-420` exposes `SnowballPlugin::schema() -> Some(snowball_schema())`; `SearchOwnEventsInput` at `:192-201` defines a described `query` and optional `limit`; schema at `:600-635` declares `search_own_events` with that input.
- The proposed revised fixture wraps `query` and undeclared noise under `args`, puts operation metadata alongside them, and marks the event `method_call`; it keeps signal and unknown-plugin tests. This changes only a unit-test fixture/schema, not live state or index contents.

## JEV decisions

The single `judge()` call asked whether the exact candidate is a bounded fix, a different narrow fix, or insufficient, and separately whether the retired fixture replacement is supported.

| Question | Exact choice | Probabilities | Confidence |
|---|---|---|---|
| Disposition for the projector repair as specified | `different_fix` | `bounded_fix`: 0.00; `different_fix`: 1.00; `insufficient`: 0.00 | 1.00 |
| Is the Snowball `search_own_events` replacement fixture supported? | `supported` | `supported`: 0.89; `unsupported`: 0.05; `insufficient`: 0.06 | 0.83 |

## Decision boundary

- The narrow conditional extraction is supported; it repairs the confirmed envelope mismatch without changing signal, historical direct-payload, schema filtering, identity, or live-index behavior.
- The exact snippet does not satisfy its own “malformed args yield Null” requirement for a present scalar `args` value. The projector can emit such a scalar at the root. At minimum, a present non-object `args` value needs to map to `Null` (for example, accept only `Value::Object`).
- Ambiguity to resolve in implementation: “malformed” could mean non-object envelope shape, or any object invalid under the complete sealed input schema. The current projector only performs schema-described field filtering, not full method-argument validation. Do not add validation beyond the explicitly intended malformed boundary without owner scope. JEV supports a narrow adjustment, not expansion into reembedding/reindexing or live-state changes.
- The fixture replacement is supported: active `snowball.search_own_events` has the documented query field needed to preserve the existing test's described-text/noise-filtering purpose. Retain signal and unknown-plugin tests. No test was run here.
