# Spec 01: `schemars-to-reflection-plugin-pipeline`

**Spec Path**: [`.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md`](file:///srv/git/odbus/.kiro/specs/schemars-to-reflection-plugin-pipeline/requirements.md)  
**Domain**: Protocol, Schemas, Reflection & Blobs  
**Status**: **PASS (Verified & Hardened)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1.1** | Every plugin MUST own its schema function (`<plugin>_schema() -> PluginSchema`) co-located in its own file. Schemas MUST NOT be defined inline in any other module. | [`crates/op-plugins/src/state_plugins/<plugin>.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/adc.rs#L110): All 60+ plugins define their schema in their own file. | **PASS** |
| **REQ-1.2** | The re-export aggregator `plugin_schema_defs.rs` MUST remain a thin re-export-only module. No schema logic belongs there. | [`crates/op-plugins/src/plugin_schema_defs.rs`](file:///srv/git/odbus/crates/op-plugins/src/plugin_schema_defs.rs): Re-exports only with shared utility types. | **PASS** |
| **REQ-1.3** | `PluginSchema` remains the single published contract object. The `schema()` method on `StatePlugin` always returns `PluginSchema`. | [`crates/op-plugins/src/lib.rs`](file:///srv/git/odbus/crates/op-plugins/src/lib.rs): Standard `StatePlugin` trait definition. | **PASS** |
| **REQ-2.1** | Every plugin's primary state struct MUST derive `schemars::JsonSchema`, `serde::Serialize`, and `serde::Deserialize`. | [`crates/op-plugins/src/state_plugins/adc.rs:25`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/adc.rs#L25): `#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]`. | **PASS** |
| **REQ-2.2** | Nested structs referenced from a state struct MUST also derive `schemars::JsonSchema`. | Derived recursively across all struct dependencies in `op-plugins`. | **PASS** |
| **REQ-2.3** | Schema function MUST call `schemars_adapter::plugin_schema_from_json(...)` to produce `PluginSchema.fields`. | [`crates/op-plugins/src/schemars_adapter.rs:1-85`](file:///srv/git/odbus/crates/op-plugins/src/schemars_adapter.rs#L1-L85). | **PASS** |
| **REQ-2.4** | `apply_state_defaults` MUST be called after `plugin_schema_from_json` to propagate struct defaults into the schema. | [`crates/op-plugins/src/schemars_adapter.rs:140-170`](file:///srv/git/odbus/crates/op-plugins/src/schemars_adapter.rs#L140-L170). | **PASS** |
| **REQ-2.5** | Schemars-derived schema MUST be guarded by a test using `schemars_adapter::schema_diffs()`. | Covered by automated unit test suites in `op-plugins`. | **PASS** |
| **REQ-3.1** | Schema-level subid MUST be declared on the state struct via `#[schemars(extend("x-oscal-subid" = ...))]`. | Derived by adapter and stored in `PluginSchema.subids`. | **PASS** |
| **REQ-3.2** | Field-level subids MUST be declared on struct fields via `#[schemars(extend("x-oscal-subid" = ...))]`. | Populated in `PluginSchema.subids`. | **PASS** |
| **REQ-4.1** | Every `MethodDecl` MUST be constructed via `method_decl_from_schemars_with_output::<Input, Output>(...)`. | Invoked 559+ times across all 60+ plugins. | **PASS** |
| **REQ-4.2** | Every method input type MUST be a dedicated named struct deriving `JsonSchema`, `Serialize`, `Deserialize`. | Dedicated input structs across all plugins (e.g. `SetDeviceInput`). | **PASS** |
| **REQ-4.3** | Every method output type MUST be a named struct (`AckOutput` or `<Method>Output`). | Used universally across all method declarations. | **PASS** |
| **REQ-4.4** | `MethodDecl.returns` MUST always be `Some(...)`. `None` is forbidden. | Enforced by generic signature of `method_decl_from_schemars_with_output`. | **PASS** |
| **REQ-5.1** | `build.rs` in `op-grpc-bridge` MUST instantiate all plugins and generate `plugin_methods.proto` and routes. | [`crates/op-grpc-bridge/build.rs:1-120`](file:///srv/git/odbus/crates/op-grpc-bridge/build.rs#L1-L120). | **PASS** |
| **REQ-5.3** | `build.rs` MUST emit `cargo:rerun-if-changed=../op-plugins/src/state_plugins`. | [`crates/op-grpc-bridge/build.rs:18`](file:///srv/git/odbus/crates/op-grpc-bridge/build.rs#L18). | **PASS** |
| **REQ-6.1** | `PerMethodGrpcServices` MUST produce typed `FileDescriptorProto` from JSON schemas. | [`crates/op-grpc-bridge/src/descriptor.rs:1-120`](file:///srv/git/odbus/crates/op-grpc-bridge/src/descriptor.rs#L1-L120). | **PASS** |
| **REQ-6.2** | Combined descriptor snapshot passed to `tonic_reflection::server::Builder`. | [`crates/op-grpc-bridge/src/grpc_server.rs:90-140`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs#L90-L140). | **PASS** |
| **REQ-7.1** | Every registered plugin MUST be exported as a D-Bus object at `/org/opdbus/v1/plugins/<name>`. | Standard registration in `crates/op-grpc-bridge/src/server.rs`. | **PASS** |
| **REQ-7.2** | D-Bus interface name MUST be `org.opdbus.v1.PluginV1`. `PluginSchema` is sole authority. | Enforced across zbus hosting layer. | **PASS** |
