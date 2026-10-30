# `cron.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/cron.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Scheduled job management for the assistant.

## Services

### `CronService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListCronJobs` | `ListCronJobsRequest` | `ListCronJobsResponse` | - |
| `CreateCronJob` | `CreateCronJobRequest` | `CronJob` | - |
| `DeleteCronJob` | `DeleteCronJobRequest` | `Empty` | - |
| `TriggerCronJob` | `TriggerCronJobRequest` | `CronJob` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
