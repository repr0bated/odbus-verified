# `common.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/common.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`

Shared message library for the `assistant.v1` package. **Defines no service.** All other
`assistant/*.proto` files import this for common types (e.g. `Empty`, `Agent`, `Run`,
`Message`, and shared enums).

## Services

None. Message/enum definitions only.

## Notes

- Imported by every other file in `crates/op-assistant-grpc/proto/assistant/`.

## Gaps / Assumptions

- Individual message/enum definitions are not enumerated here; consult the source.
