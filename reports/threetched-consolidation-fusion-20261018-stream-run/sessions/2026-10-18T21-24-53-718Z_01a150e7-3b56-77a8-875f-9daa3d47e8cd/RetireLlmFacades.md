Cleanup applied; source frozen. No Cargo or tests run.

Receipts: `large_language_model.rs#BCF1`, `large_language_model_methods.rs#A3DB`, bridge `large_language_model_dispatch.rs#59E9`. Removed incidental counts/wiring assertions and unused target-name helper; retained substantive rejection/admission coverage.

Exact edited source/test files:
- `crates/op-plugins/src/state_plugins/large_language_model.rs`
- `crates/op-plugins/src/state_plugins/large_language_model_methods.rs`
- `crates/op-plugins/src/state_plugins/antigravity.rs`
- `crates/op-plugins/src/state_plugins/antigravity_chat.rs`
- `crates/op-plugins/src/state_plugins/gemma_brain.rs`
- `crates/op-grpc-bridge/src/large_language_model_dispatch.rs`
- `crates/op-grpc-bridge/src/antigravity_dispatch.rs`
- `crates/op-grpc-bridge/src/owned_method_task_tests.rs`
- `crates/op-grpc-bridge/tests/large_language_model_dispatch.rs`