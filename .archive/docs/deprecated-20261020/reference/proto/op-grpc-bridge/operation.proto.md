# `operation.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/proto/operation.proto`
- **Package:** `operation.v1`
- **Imports:** `google/protobuf/{any,timestamp,struct,empty}.proto`

The primary bridge contract. Projects D-Bus plugin state, the event/audit chain,
OVSDB, and system runtime over gRPC. Every RPC here maps back to a D-Bus object under
`org.opdbus.v1`; gRPC is transport, D-Bus remains the authority.

## Services

### `StateSync`
Zero-copy state projection and mutation surface.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Subscribe` | `SubscribeRequest` | `StateChange` | server |
| `Mutate` | `MutateRequest` | `MutateResponse` | - |
| `GetState` | `GetStateRequest` | `GetStateResponse` | - |
| `BatchMutate` | `BatchMutateRequest` | `BatchMutateResponse` | - |

### `PluginService`
Schema-driven plugin introspection and invocation.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListPlugins` | `google.protobuf.Empty` | `ListPluginsResponse` | - |
| `GetSchema` | `GetSchemaRequest` | `GetSchemaResponse` | - |
| `CallMethod` | `CallMethodRequest` | `CallMethodResponse` | - |
| `GetProperty` | `GetPropertyRequest` | `GetPropertyResponse` | - |
| `SetProperty` | `SetPropertyRequest` | `SetPropertyResponse` | - |
| `SubscribeSignals` | `SubscribeSignalsRequest` | `Signal` | server |

### `EventChainService`
Append-only audit chain ("Snowball"), proofs, and semantic trace search.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetEvents` | `GetEventsRequest` | `GetEventsResponse` | - |
| `SubscribeEvents` | `SubscribeEventsRequest` | `ChainEvent` | server |
| `VerifyChain` | `VerifyChainRequest` | `VerifyChainResponse` | - |
| `GetProof` | `GetProofRequest` | `GetProofResponse` | - |
| `ProveTagImmutability` | `ProveTagImmutabilityRequest` | `ProveTagImmutabilityResponse` | - |
| `GetSnapshot` | `GetSnapshotRequest` | `GetSnapshotResponse` | - |
| `CreateSnapshot` | `CreateSnapshotRequest` | `CreateSnapshotResponse` | - |
| `SearchSemanticTrace` | `SearchSemanticTraceRequest` | `SearchSemanticTraceResponse` | - |

### `OvsdbMirror`
OVSDB projection mirroring the Open vSwitch database protocol.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListDbs` | `google.protobuf.Empty` | `OvsdbListDbsResponse` | - |
| `GetSchema` | `OvsdbGetSchemaRequest` | `OvsdbGetSchemaResponse` | - |
| `Transact` | `OvsdbTransactRequest` | `OvsdbTransactResponse` | - |
| `Monitor` | `OvsdbMonitorRequest` | `OvsdbUpdate` | server |
| `Echo` | `OvsdbEchoRequest` | `OvsdbEchoResponse` | - |
| `DumpDb` | `OvsdbDumpDbRequest` | `OvsdbDumpDbResponse` | - |
| `GetBridgeState` | `OvsdbGetBridgeStateRequest` | `OvsdbGetBridgeStateResponse` | - |

### `RuntimeMirror`
Live host/runtime telemetry projection.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetSystemInfo` | `google.protobuf.Empty` | `RuntimeGetSystemInfoResponse` | - |
| `ListServices` | `RuntimeListServicesRequest` | `RuntimeListServicesResponse` | - |
| `GetService` | `RuntimeGetServiceRequest` | `RuntimeServiceInfo` | - |
| `StreamMetrics` | `RuntimeStreamMetricsRequest` | `RuntimeMetricUpdate` | server |
| `ListInterfaces` | `google.protobuf.Empty` | `RuntimeListInterfacesResponse` | - |
| `GetNumaTopology` | `google.protobuf.Empty` | `RuntimeGetNumaTopologyResponse` | - |

### `DbusPassthrough`
Direct escape hatch to arbitrary D-Bus objects for calls, properties, and signals.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Call` | `DbusCallRequest` | `dbusCallResponse` | - |
| `Get` | `DbusGetPropertyRequest` | `DbusGetPropertyResponse` | - |
| `Set` | `DbusSetPropertyRequest` | `DbusSetPropertyResponse` | - |
| `Watch` | `DbusWatchRequest` | `DbusSignalEvent` | server |

## Notes

- The file also carries Zeroclaw-related messages consumed by the bridge (see
  [`zeroclaw.proto`](./zeroclaw.proto.md)).
- `subid` taxonomy: RPCs on `StateSync.Mutate` correspond to `mut.*` records and must
  carry `actor_id` + `capability_id`; `EventChainService` maps to `evt.*` records.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
- `dbusCallResponse` uses lowercase leading char in source (kept verbatim above).
