# op-grpc-bridge — tched naming audit ledger (fresh, read-only, DRY RUN)

Generated: 2026-10-25T05:53:54Z (AuditBridge, Omen Alpha).
Scope: crates/op-grpc-bridge Rust/TOML/shell/proto, all files. Read-only per SUPERSEDING DRY RUN broadcast; no edits, no cargo, no tests.

## Denominator & coverage
- Raw `grep -rniE 'tched' ...` over crates/op-grpc-bridge: **924 matched lines** (includes lines where the only hit is an English word fragment like matched/dispatched/mismatched, plus every naming-bearing line).
- Naming-bearing ledger rows: **787** across **79 files** (787 unique (file,line) rows after dedup of same-line duplicate patterns).
- Classes: {'C-3T': 24, 'DEBT-RR': 629, 'C-THREE': 46, 'CR-MOD': 54, 'FP': 8, 'C-TFS': 26}
- Deployment-visible (WIRE) rows among audit-bearing rows: **59** (need migration notes, not direct renames).


## `crates/op-grpc-bridge/examples/test_registration.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 14 | C-3T | internal | .unwrap_or_else(\|\| "3tched.com".to_string()); | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/proto/mail.proto` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 46 | C-3T | internal | string domain = 6; // "3tched.com" | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/proto/operation.proto` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 917 | DEBT-RR | CODE | // 3tched Router projection (read-only gRPC view of the plugin's route/provider tree) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 918 | DEBT-RR | WIRE | // subid: exp.service.3tched-router-bridge.grpc-stream@v1 | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/proto/privacy_network.proto` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 47 | C-3T | internal | string domain = 1; // "3tched.com" | correct per contract; no change | 0.96 | none |
| 113 | C-3T | internal | string endpoint = 4; // "registration.3tched.com:51820" | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/proto/registration.proto` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 42 | C-3T | internal | string domain = 2; // "3tched.com" | correct per contract; no change | 0.96 | none |
| 43 | C-3T | internal | bool is_admin = 3; // true for admin@3tched.com, false for jeremy@3tched.com | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/bin/op-grpc-bridge.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 4 | DEBT-RR | CODE | //! service. It exposes the complete Operation route set plus the 3tched Router | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 7 | DEBT-RR | CODE | use op_grpc_bridge::server::{ServerConfig, run_tched_router_server}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 30 | DEBT-RR | CODE | run_tched_router_server(config).await | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/grpc/tched_router.proto` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | C-THREE | internal | // threetched gRPC schema host contract. | correct per contract; no change | 0.96 | none |
| 3 | DEBT-RR | CODE | // is serialized JSON from `tched_router_schema()` in `op-plugins` — never hand-edited | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6 | DEBT-RR | CODE | package tched_router; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 20 | DEBT-RR | CODE | // Canonical PluginSchema JSON serialized by `tched_router_schema()`. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/agent_runs.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 27 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::{ChatInput, ChatOutput, TchedChatMessage}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 66 | DEBT-RR | CODE | crate::tched_router_runtime::TchedRouterRuntimeClient::from_env() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/antigravity_dispatch.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 36 | DEBT-RR | WIRE | llm_plugin: "tched_router".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 74 | DEBT-RR | WIRE | llm_plugin: "tched_router".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/src/chat_mode_routing.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 10 | DEBT-RR | CODE | tched_router::ChatInput, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 16 | DEBT-RR | CODE | tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/chat_response_state.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 209 | C-3T | internal | // (3tched.com renders from SHM) is their production caller. | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/context_bundle.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 3 | DEBT-RR | CODE | //! Before 2026-09-19 the production turn (`tched_router.Chat`) forwarded the | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/control_plane_routing.rs` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 9 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::{ChatInput, ChatOutput}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 13 | DEBT-RR | CODE | use crate::tched_router_generation_native::NativeGenerationAdmission; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 14 | DEBT-RR | CODE | use crate::tched_router_runtime::{RuntimeSelection, TchedRouterRuntimeClient}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 121 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationError, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/cron_dispatch.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 4 | DEBT-RR | CODE | //! tched-router bearer authenticates only this bridge to the loopback daemon. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 107 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 113 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/cron_gateway.rs` (7 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Exact v0.8.4 cron route mapping over the existing hardened tched-router | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 10 | DEBT-RR | CODE | use crate::tched_router_runtime::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 70 | DEBT-RR | CODE | anyhow!("tched_router cron request failed before a provider result") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 73 | DEBT-RR | CODE | anyhow!("tched_router cron response was unavailable or invalid") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 79 | DEBT-RR | CODE | anyhow!("tched_router rejected cron request with HTTP {status}") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 91 | DEBT-RR | CODE | Err(anyhow!("tched_router cron mutation failed before submission")) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 94 | DEBT-RR | CODE | Err(anyhow!("tched_router rejected cron mutation with HTTP {status}")) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/cron_methods.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 137 | DEBT-RR | CODE | "tched_router returned more history rows than requested" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 141 | DEBT-RR | CODE | "tched_router returned history for another job" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 300 | DEBT-RR | CODE | "tched_router returned more than {MAX_CRON_JOBS} cron jobs" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 307 | DEBT-RR | CODE | "tched_router returned duplicate cron job IDs" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 740 | DEBT-RR | CODE | "tched_router returned duplicate cron job IDs" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/cron_observer.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 10 | DEBT-RR | CODE | mutation_engine::MutationEngine, tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/ctl_plane_chatbot_dispatch.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 505 | DEBT-RR | CODE | let origin = crate::tched_router_admission::FacadeOrigin { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 664 | DEBT-RR | CODE | let origin = crate::tched_router_admission::FacadeOrigin { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/decoy_network.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 346 | DEBT-RR | CODE | caller: &crate::tched_router_admission::RouterCaller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 349 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call(engine, admitted, plugin, method, caller) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/dynamic_reflection.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 458 | DEBT-RR | CODE | producer.write(&seal("tched_router")).unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/generation_admission.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 5 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::generation::GenerationOwner; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 20 | DEBT-RR | CODE | pub(crate) const PLUGIN: &str = "tched_router"; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/generation_jobs_dispatch.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 7 | DEBT-RR | CODE | tched_router_generation_native::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 17 | DEBT-RR | CODE | use op_plugins::state_plugins::{runtime_documents::RuntimeDocuments, tched_router::generation::*}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 36 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::generation::is_method(method) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/generation_jobs_selection.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 6 | DEBT-RR | WIRE | const ROOT: &str = "/org/opdbus/v1/plugins/tched_router"; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 34 | DEBT-RR | WIRE | plugin_ids.is_empty() \|\| plugin_ids == ["tched_router"], | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 120 | DEBT-RR | CODE | &["tched_router".into(), "config".into()], | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/generation_jobs_stream.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 7 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::generation::READ_CAPABILITY; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 29 | DEBT-RR | CODE | const PLUGIN: &str = "tched_router"; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/generation_jobs_tests.rs` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 13 | DEBT-RR | CODE | tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 18 | DEBT-RR | CODE | large_language_model::LargeLanguageModelPlugin, tched_router::TchedRouterPlugin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 101 | DEBT-RR | WIRE | "tched_router.generation.v1".parse().unwrap(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 354 | DEBT-RR | WIRE | format!("/org/opdbus/v1/plugins/tched_router/{suffix}") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/src/generation_runtime.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 8 | DEBT-RR | CODE | tched_router_generation_native::{NativeGenerationAdmission, NativeGenerationTask}, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 9 | DEBT-RR | CODE | tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 12 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::generation::StartGenerationInput; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/grpc_server.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 346 | C-THREE | internal | let mut plugin = op_blob::schema::PluginSchema::builder("threetched").build(); | correct per contract; no change | 0.96 | none |
| 354 | C-THREE | internal | subid: "obs.threetched.configure@v1".into(), | correct per contract; no change | 0.96 | none |
| 357 | C-THREE | internal | let (bytes, methods) = op_blob::descriptor::descriptor_set_for_plugin("threetched", &plugin); | correct per contract; no change | 0.96 | none |
| 1295 | DEBT-RR | CODE | /// the tched_router bridge (`run_tched_router_server`, `:8090` + `container.sock`) — builds | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1306 | DEBT-RR | CODE | /// afterward (e.g. the tched_router bridge adds `TchedRouterService`; `run_grpc_server` | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/identity_delete_provision.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 152 | C-3T | internal | "Chatbot3tched" \| "3tchedCtlbot" \| "3tchedChatbot" | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/large_language_model_dispatch.rs` (8 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Capability-preserving facade dispatch into canonical tched_router methods. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 25 | DEBT-RR | CODE | "exact nested tched_router capability grant is required" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 97 | DEBT-RR | CODE | .context("canonical tched_router response is missing its domain result")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 104 | DEBT-RR | CODE | pub(crate) async fn observed_tched_router_state( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 108 | DEBT-RR | CODE | ) -> Result<op_plugins::state_plugins::tched_router::TchedRouterState> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 110 | CR-MOD | internal | crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 112 | DEBT-RR | CODE | .persist_tched_router_observation(&represented, &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 114 | DEBT-RR | CODE | .context("native tched_router read completed but projection publication failed")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/lungs_socket.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 738 | C-3T | internal | native_name: "Chatbot3tched".into(), | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/mcp_config_dispatch.rs` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 17 | DEBT-RR | CODE | tched_router_admission::FacadeOrigin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 99 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 111 | DEBT-RR | WIRE | target_plugin: "tched_router", | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 119 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/mcp_config_dispatch_tests.rs` (11 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 6 | DEBT-RR | CODE | tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 110 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 111 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_TOKEN", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 122 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 127 | DEBT-RR | CODE | unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 263 | DEBT-RR | CODE | mcp::McpStatePlugin, tched_router::TchedRouterPlugin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 295 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 296 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_TOKEN", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 305 | DEBT-RR | CODE | unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 312 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 360 | DEBT-RR | WIRE | if event.plugin_id == "tched_router" | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/src/mcp_frontend.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 4709 | FP | internal | let batched_initialize = router | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |
| 4720 | FP | internal | assert_eq!(batched_initialize.status(), StatusCode::BAD_REQUEST); | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |

## `crates/op-grpc-bridge/src/model_registry_dispatch.rs` (8 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 791 | CR-MOD | internal | ) -> Result<crate::tched_catalog_binding::BindingTarget> { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 838 | CR-MOD | internal | ) -> Result<crate::tched_own_agent_route::Target> { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 849 | CR-MOD | internal | ) -> Result<crate::tched_own_agent_route::Target> { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 864 | CR-MOD | internal | crate::tched_catalog_binding::valid_reference(reference), | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 876 | CR-MOD | internal | Ok(crate::tched_own_agent_route::Target { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 890 | CR-MOD | internal | ) -> Result<crate::tched_catalog_binding::BindingTarget> { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 931 | CR-MOD | internal | .and_then(crate::tched_catalog_binding::reference_family) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 938 | CR-MOD | internal | Ok(crate::tched_catalog_binding::BindingTarget { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |

## `crates/op-grpc-bridge/src/owned_method_task_tests.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 202 | DEBT-RR | CODE | let target = spawn(operation("tched_router", "start_generation"), async { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/per_plugin_reflection.rs` (14 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 49 | DEBT-RR | CODE | /// Plugin ID (e.g., "tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 265 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 267 | DEBT-RR | WIRE | "exp.service.3tched-router.register-agent@v1".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 273 | DEBT-RR | WIRE | assert_eq!(meta.plugin_id, "tched_router"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 276 | DEBT-RR | CODE | assert_eq!(meta.method_path, "tched_router.RegisterAgent"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 290 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 300 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 308 | DEBT-RR | CODE | let methods = registry.get_for_plugin("tched_router").await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 318 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 328 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 336 | DEBT-RR | CODE | let removed = registry.unregister_plugin("tched_router").await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 339 | DEBT-RR | CODE | let meta = registry.get("tched_router.RegisterAgent").await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 349 | DEBT-RR | CODE | "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 369 | DEBT-RR | CODE | assert!(plugins.contains(&"tched_router".to_string())); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/plugin_grpc_gen.rs` (6 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 39 | DEBT-RR | CODE | /// method_path -> ServiceRegistry (e.g., "tched_router.RegisterAgent") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 54 | DEBT-RR | CODE | /// Plugin ID (e.g., "tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 669 | DEBT-RR | WIRE | subid: "exp.service.3tched-router.register-agent@v1".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 672 | DEBT-RR | CODE | let proto = generate_method_proto("tched_router", "RegisterAgent", &method_decl); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 681 | DEBT-RR | WIRE | assert!(proto.contains("subid: exp.service.3tched-router.register-agent@v1")); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 685 | DEBT-RR | CODE | generate_method_file_descriptor("tched_router", "RegisterAgent", &method_decl); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/routing_dispatch.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 57 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 63 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/schema_loader.rs` (12 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 13 | DEBT-RR | WIRE | const TCHED_ROUTER_PLUGIN_ID: &str = "tched_router"; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 21 | DEBT-RR | CODE | /// Shared loader state for the tched_router Axum host. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 37 | DEBT-RR | WIRE | Self::new_for_plugin(path, TCHED_ROUTER_PLUGIN_ID) | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 236 | DEBT-RR | CODE | let blob = op_blob::blobify_plugin_schema("tched_router", schema); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 246 | DEBT-RR | CODE | let value = op_state_store::PluginSchema::builder("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 254 | DEBT-RR | CODE | assert_eq!(loaded["name"], "tched_router"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 262 | DEBT-RR | WIRE | let path = dir.path().join("tched_router.json"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 273 | DEBT-RR | CODE | let schema = op_state_store::PluginSchema::builder("tched_router").build(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 274 | DEBT-RR | CODE | op_blob::blobify_plugin_schema("tched_router", schema) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 284 | DEBT-RR | CODE | let initial = op_state_store::PluginSchema::builder("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 304 | DEBT-RR | CODE | let updated = op_state_store::PluginSchema::builder("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 310 | DEBT-RR | CODE | op_blob::blobify_plugin_schema("tched_router", initial) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/schema_renderer_dispatch.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 31 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 37 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 82 | DEBT-RR | CODE | let current = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/schema_router.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 806 | C-TFS | internal | transport: Option<(tched_fs::invocation::CallMetadata, std::time::Instant)>, | correct per contract; no change | 0.96 | none |
| 1001 | C-TFS | internal | let transport: tched_fs::invocation::CallMetadata = serde_json::from_str(&admission) | correct per contract; no change | 0.96 | none |
| 2356 | FP | internal | async fn null_required_capability_dispatched_without_capability() { | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |

## `crates/op-grpc-bridge/src/sealed_catalog_tests.rs` (25 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 51 | DEBT-RR | CODE | let pinned = seal(dir.path(), &schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 59 | DEBT-RR | CODE | vec!["tched_router"] | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 61 | DEBT-RR | CODE | let loaded = &loaded.plugins["tched_router"]; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 71 | DEBT-RR | CODE | op_blob::catalog::read_plugin_state_store_schema(dir.path(), "tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 86 | DEBT-RR | CODE | let orphan = op_blob::blobify_plugin_schema("tched_router", schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 89 | DEBT-RR | WIRE | assert!(read_manifest_pinned_schema_document(dir.path(), "tched_router").is_err()); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 90 | DEBT-RR | CODE | assert!(op_blob::catalog::read_plugin_state_store_schema(dir.path(), "tched_router").is_none()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 99 | DEBT-RR | CODE | let mut blob = seal(dir.path(), &schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 129 | DEBT-RR | CODE | op_blob::catalog::read_plugin_state_store_schema(dir.path(), "tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 139 | DEBT-RR | CODE | let blob = op_blob::blobify_plugin_schema("tched_router", schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 162 | DEBT-RR | CODE | let first = seal(dir.path(), &schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 171 | DEBT-RR | CODE | let second = seal(dir.path(), &schema("tched_router", "2.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 175 | DEBT-RR | WIRE | read_manifest_pinned_catalog(dir.path()).unwrap().plugins["tched_router"] | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 201 | DEBT-RR | CODE | let original = seal(dir.path(), &schema("tched_router", "1.0.0")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 202 | DEBT-RR | WIRE | let loaded = read_manifest_pinned_schema_document(dir.path(), "tched_router").unwrap(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 206 | DEBT-RR | CODE | "tched_router".into(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 216 | DEBT-RR | CODE | let mut replacement = schema("tched_router", "2.0.0"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 219 | DEBT-RR | WIRE | let loaded = read_manifest_pinned_schema_document(dir.path(), "tched_router").unwrap(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 222 | DEBT-RR | CODE | "tched_router".into(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 230 | DEBT-RR | CODE | .get_services_for_plugin("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 243 | DEBT-RR | CODE | let supplied = schema("tched_router", "1.0.0"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 244 | DEBT-RR | CODE | assert!(read_registration_document(dir.path(), "tched_router", &supplied).is_err()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 248 | DEBT-RR | CODE | assert!(read_registration_document(dir.path(), "tched_router", &supplied).is_ok()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 249 | DEBT-RR | CODE | let changed = schema("tched_router", "2.0.0"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 250 | DEBT-RR | CODE | assert!(read_registration_document(dir.path(), "tched_router", &changed).is_err()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/server.rs` (53 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Axum gRPC/gRPC-Web host for the tched_router plugin schema. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 44 | DEBT-RR | CODE | use crate::proto::tched_router::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 46 | DEBT-RR | CODE | tched_router_service_server::{TchedRouterService, TchedRouterServiceServer}, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 53 | DEBT-RR | WIRE | const DEFAULT_SCHEMA_PLUGIN_ID: &str = "tched_router"; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 69 | DEBT-RR | CODE | /// Runtime configuration for the tched_router Axum host. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 78 | DEBT-RR | CODE | /// Host-local gRPC UDS (`TCHED_ROUTER_UNIX_SOCKET`, default `/run/opdbus/grpc.sock`). | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 86 | DEBT-RR | CODE | /// Set via `TCHED_ROUTER_BIND_ADDR` or `GRPC_BIND`; default `0.0.0.0:8090`. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 89 | DEBT-RR | CODE | /// unless `TCHED_ROUTER_DEV_SELF_SIGNED=1` is set (dev/CI only — never production). | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 185 | DEBT-RR | WIRE | let bind_addr = std::env::var("TCHED_ROUTER_BIND_ADDR") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 190 | DEBT-RR | WIRE | std::env::var("TCHED_ROUTER_BROWSER_TLS_BIND_ADDR") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 193 | DEBT-RR | WIRE | std::env::var("TCHED_ROUTER_BROWSER_TLS_CERT_FILE") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 196 | DEBT-RR | WIRE | std::env::var("TCHED_ROUTER_BROWSER_TLS_KEY_FILE") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 215 | DEBT-RR | WIRE | std::env::var("TCHED_ROUTER_UNIX_SOCKET") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 253 | DEBT-RR | CODE | /// 1. `TCHED_ROUTER_TLS_CERT` + `TCHED_ROUTER_TLS_KEY` env vars (PEM contents). | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 254 | DEBT-RR | CODE | /// 2. `TCHED_ROUTER_TLS_CERT_FILE` + `TCHED_ROUTER_TLS_KEY_FILE` env vars pointing | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 260 | DEBT-RR | CODE | /// 3. Self-signed via rcgen **only** if `TCHED_ROUTER_DEV_SELF_SIGNED=1` is set | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 266 | DEBT-RR | WIRE | let cert_pem = std::env::var("TCHED_ROUTER_TLS_CERT").ok(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 267 | DEBT-RR | WIRE | let key_pem = std::env::var("TCHED_ROUTER_TLS_KEY").ok(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 270 | DEBT-RR | CODE | tracing::info!("TLS identity loaded from TCHED_ROUTER_TLS_CERT/TCHED_ROUTER_TLS_KEY"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 277 | DEBT-RR | WIRE | let cert_file = std::env::var("TCHED_ROUTER_TLS_CERT_FILE").ok(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 278 | DEBT-RR | WIRE | let key_file = std::env::var("TCHED_ROUTER_TLS_KEY_FILE").ok(); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 295 | DEBT-RR | CODE | "failed to read TCHED_ROUTER_TLS_CERT_FILE/TCHED_ROUTER_TLS_KEY_FILE" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 303 | DEBT-RR | WIRE | if std::env::var("TCHED_ROUTER_DEV_SELF_SIGNED").as_deref() == Ok("1") { | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 307 | C-3T | internal | "3tched.com".to_string(), | correct per contract; no change | 0.96 | none |
| 311 | DEBT-RR | CODE | "TCHED_ROUTER_DEV_SELF_SIGNED=1: using ephemeral self-signed TLS cert. \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 328 | DEBT-RR | CODE | Set TCHED_ROUTER_TLS_CERT+TCHED_ROUTER_TLS_KEY (PEM), \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 329 | DEBT-RR | CODE | TCHED_ROUTER_TLS_CERT_FILE+TCHED_ROUTER_TLS_KEY_FILE (paths), \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 330 | DEBT-RR | CODE | or TCHED_ROUTER_DEV_SELF_SIGNED=1 (dev only). \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 366 | DEBT-RR | CODE | TCHED_ROUTER_BROWSER_TLS_BIND_ADDR, \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 367 | DEBT-RR | CODE | TCHED_ROUTER_BROWSER_TLS_CERT_FILE, and \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 368 | DEBT-RR | CODE | TCHED_ROUTER_BROWSER_TLS_KEY_FILE" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 442 | DEBT-RR | CODE | "TCHED_ROUTER_BIND_ADDR is empty — no TCP addresses to bind" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 450 | DEBT-RR | CODE | "TCP fabric requires TLS — set TCHED_ROUTER_TLS_CERT+TCHED_ROUTER_TLS_KEY, \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 451 | DEBT-RR | CODE | TCHED_ROUTER_TLS_CERT_FILE+TCHED_ROUTER_TLS_KEY_FILE, \ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 452 | DEBT-RR | CODE | or TCHED_ROUTER_DEV_SELF_SIGNED=1 (dev only)" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 544 | DEBT-RR | WIRE | .or_else(\|\| getenv("TCHED_ROUTER_PLUGIN_ID")) | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 622 | DEBT-RR | CODE | /// Assemble the gRPC service set for the tched_router bridge. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 629 | DEBT-RR | CODE | /// of that the tched_router bridge adds its endpoint-specific `TchedRouterService` | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 641 | DEBT-RR | CODE | let tched_router_svc = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 645 | DEBT-RR | CODE | build_operation_routes_with_validator(server, validator).add_service(tched_router_svc) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 796 | DEBT-RR | CODE | /// Run the tched_router Axum host. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 800 | DEBT-RR | CODE | pub async fn run_tched_router_server(config: ServerConfig) -> anyhow::Result<()> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 804 | DEBT-RR | WIRE | let legacy_path = std::env::var_os("TCHED_ROUTER_SCHEMA_PATH").map(PathBuf::from); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 860 | DEBT-RR | CODE | Arc::new(crate::tched_router_runtime::TchedRouterRuntimeClient::from_env()), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1014 | C-TFS | internal | // binds to one session, so 3tchedFS call files dispatch as that | correct per contract; no change | 0.96 | none |
| 1094 | DEBT-RR | WIRE | "loaded manifest-pinned tched_router object metadata" | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 1103 | DEBT-RR | CODE | "failed to register pinned tched_router method descriptors for gRPC reflection" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1111 | DEBT-RR | CODE | "pinned tched_router blob unavailable for gRPC reflection" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1196 | DEBT-RR | CODE | info!(path = %unix_socket.display(), "tched_router native gRPC listening on Unix socket"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1260 | DEBT-RR | CODE | info!(addr = %bind_addr, "tched_router TLS gRPC/gRPC-Web listening on TCP"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1341 | DEBT-RR | CODE | "TCHED_ROUTER_SCHEMA_PATH conflicts with OP_BLOB_CATALOG_DIR" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1548 | DEBT-RR | WIRE | assert_eq!(ServerConfig::default().plugin_id, "tched_router"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 1553 | DEBT-RR | WIRE | assert_eq!(configured_schema_plugin_id(\|_\| None), "tched_router"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/src/specialist.rs` (16 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 9 | DEBT-RR | CODE | //! through `tched_router.Chat`'s guarded native path. The assembled persona | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 24 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::{ChatInput, TchedChatMessage}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 65 | DEBT-RR | CODE | facade_origin: Option<crate::tched_router_admission::FacadeOrigin>, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 69 | DEBT-RR | CODE | impl crate::tched_router_generation_native::NativeGenerationAdmission | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 83 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationError, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 90 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 97 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 111 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationError::Terminal( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 112 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeErrorCode::Auth, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 125 | DEBT-RR | CODE | ) -> anyhow::Result<Arc<dyn crate::tched_router_generation_native::NativeGenerationAdmission>> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 142 | DEBT-RR | CODE | facade_origin: Option<crate::tched_router_admission::FacadeOrigin>, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 143 | DEBT-RR | CODE | ) -> anyhow::Result<Arc<dyn crate::tched_router_generation_native::NativeGenerationAdmission>> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 289 | DEBT-RR | CODE | crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 290 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 295 | DEBT-RR | CODE | &crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 439 | DEBT-RR | CODE | sources.push(format!("tched_router.routing:{routing}")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/system_graph_projection.rs` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 819 | C-3T | internal | "vendor/3tched-json-render-ui/source.tar.gz#src/chat/model-catalog.ts", | correct per contract; no change | 0.96 | none |
| 869 | C-3T | internal | "vendor/3tched-json-render-ui/source.tar.gz#src/grpc/door.ts", | correct per contract; no change | 0.96 | none |
| 875 | DEBT-RR | CODE | ("tched_router", "src/chat/door-chat-adapter.ts#TCHED_ROUTER"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 882 | C-3T | internal | declared("vendor/3tched-json-render-ui/source.tar.gz", locator), | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/system_graph_projection_tests.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 133 | C-3T | internal | json!({"instances":[{"name":"mail-3tched","status":"Running","type":"container", | correct per contract; no change | 0.96 | none |
| 143 | C-3T | internal | json!({"container_name":"mail-3tched","container_status":"Running","domain":"example.test","healthy":true, | correct per contract; no change | 0.96 | none |
| 148 | C-3T | internal | let container = node(&snapshot, "container", "mail-3tched"); | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/task_elevation_tests.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 171 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_catalog_binding.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 5 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingOutput; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_config_dispatch.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Native read adapter for the tched_router v0.8.4 config surface. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 8 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 14 | DEBT-RR | CODE | use crate::tched_router_runtime::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_config_engine.rs` (23 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Exact sealed-contract adapter for the native tched_router configuration surface. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 15 | CR-MOD | internal | tched_config_dispatch::{self, ConfigReadDispatch}, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 16 | CR-MOD | internal | tched_config_secret_policy::PinnedConfigSecretPolicy, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 17 | DEBT-RR | CODE | tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 20 | DEBT-RR | CODE | const PLUGIN: &str = "tched_router"; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 38 | DEBT-RR | CODE | .context("sealed tched_router has no native_config state field")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 40 | DEBT-RR | CODE | anyhow::bail!("sealed tched_router native_config field is not an exact schema"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 59 | DEBT-RR | CODE | "tched_router seal changed during native config dispatch" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 74 | DEBT-RR | CODE | .with_context(\|\| format!("admitted tched_router method '{method}' is absent"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 78 | DEBT-RR | CODE | .with_context(\|\| format!("admitted tched_router method '{method}' has no output schema"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 84 | DEBT-RR | CODE | "native output does not conform to admitted tched_router.{method} during output validation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 113 | CR-MOD | internal | target: &crate::tched_catalog_binding::BindingTarget, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 114 | DEBT-RR | CODE | ) -> anyhow::Result<crate::tched_router_runtime::CatalogBindingObservation> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 220 | CR-MOD | internal | let (rows, _) = tched_blocking(move \|\| { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 259 | CR-MOD | internal | async fn tched_blocking<T: Send + 'static>( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 320 | C-TFS | internal | // by the 3tchedFS mirror — not a receipt. | correct per contract; no change | 0.96 | none |
| 323 | CR-MOD | internal | match tched_config_dispatch::dispatch_config_read(&runtime, method, args, &contract.secrets) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 337 | DEBT-RR | CODE | .persist_tched_router_config_observation( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 344 | DEBT-RR | CODE | .read_tched_router_config_field("native_config", &contract.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 348 | DEBT-RR | CODE | "tched_router native config changed after observation publication" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 362 | CR-MOD | internal | tched_config_dispatch::dispatch_config_mutation(&runtime, method, &args, &contract.secrets) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 391 | DEBT-RR | CODE | .persist_tched_router_config_observation( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 414 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::TchedRouterState, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_config_secret_policy.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 8 | DEBT-RR | CODE | use crate::tched_router_runtime::ConfigSecretPolicy; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 28 | DEBT-RR | CODE | "pinned tched_router config contract is not an object schema" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 32 | DEBT-RR | CODE | "pinned tched_router config contract has no properties" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 253 | DEBT-RR | CODE | let document = op_plugins::state_plugins::tched_router_contract::document().clone(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 433 | DEBT-RR | CODE | let document = op_plugins::state_plugins::tched_router_contract::document().clone(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_own_agent_route.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 7 | DEBT-RR | CODE | tched_router::{ConfigPatchInput, ConfigPatchOperation, GetOwnAgentRouteOutput}, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 11 | DEBT-RR | WIRE | pub(crate) const WRITE_CAPABILITY: &str = "cap.software.tched-router.agent.route.self.write@v1"; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 29 | DEBT-RR | CODE | caller: &crate::tched_router_admission::RouterCaller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 125 | CR-MOD | internal | ensure!(crate::tched_catalog_binding::valid_reference(&target.reference), "invalid dedicated profile"); | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 131 | CR-MOD | internal | crate::tched_catalog_binding::endpoint_matches(uri, &target.inference_url_prefix)?, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |

## `crates/op-grpc-bridge/src/tched_router_admission.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 62 | DEBT-RR | CODE | let operation = crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 63 | DEBT-RR | CODE | let pinned = readmit_call(engine, admitted, "tched_router", method, caller).await?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 123 | DEBT-RR | CODE | #[path = "tched_router_admission_tests.rs"] | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_router_admission_tests.rs` (6 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 8 | DEBT-RR | CODE | tched_router::TchedRouterPlugin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 76 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 81 | DEBT-RR | CODE | "tched_router", "SetModel", Some(&capability), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 89 | DEBT-RR | CODE | let guard = crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 118 | DEBT-RR | CODE | .write(&op_blob::blobify_plugin_schema("tched_router", changed)) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 127 | DEBT-RR | CODE | .write(&op_blob::blobify_plugin_schema("tched_router", changed)) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_router_generation_native.rs` (17 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 17 | DEBT-RR | WIRE | const WS_PROTOCOL: &str = "tched_router.generation.v1"; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 238 | C-THREE | internal | log_threetched_connection_failure("handshake", &error); | correct per contract; no change | 0.96 | none |
| 330 | C-THREE | internal | "threetched generation connection failed" | correct per contract; no change | 0.96 | none |
| 335 | C-THREE | internal | log_threetched_connection_failure("receive_output", &error); | correct per contract; no change | 0.96 | none |
| 351 | C-THREE | internal | "threetched generation connection failed" | correct per contract; no change | 0.96 | none |
| 457 | C-THREE | internal | struct ThreetchedConnectionFailure { | correct per contract; no change | 0.96 | none |
| 463 | C-THREE | internal | fn log_threetched_connection_failure( | correct per contract; no change | 0.96 | none |
| 467 | C-THREE | internal | let failure = threetched_connection_failure(error); | correct per contract; no change | 0.96 | none |
| 473 | C-THREE | internal | "threetched generation connection failed" | correct per contract; no change | 0.96 | none |
| 477 | C-THREE | internal | fn threetched_connection_failure( | correct per contract; no change | 0.96 | none |
| 479 | C-THREE | internal | ) -> ThreetchedConnectionFailure { | correct per contract; no change | 0.96 | none |
| 481 | C-THREE | internal | let mut failure = ThreetchedConnectionFailure { | correct per contract; no change | 0.96 | none |
| 564 | C-THREE | internal | tracing::warn!(stage = "send_frame", "threetched generation connection failed"); | correct per contract; no change | 0.96 | none |
| 583 | C-THREE | internal | tracing::warn!(stage = "receive_ack", kind = "ended", "threetched generation connection failed"); | correct per contract; no change | 0.96 | none |
| 587 | C-THREE | internal | log_threetched_connection_failure("receive_ack", &error); | correct per contract; no change | 0.96 | none |
| 832 | C-THREE | internal | let failure = threetched_connection_failure( | correct per contract; no change | 0.96 | none |
| 845 | C-THREE | internal | let failure = threetched_connection_failure(&error); | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/tched_router_legacy_dispatch.rs` (17 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Bridge integration for retained tched_router reads and durable role assignments. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 15 | DEBT-RR | CODE | tched_router::ObservedTchedRouterState, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 16 | DEBT-RR | CODE | tched_router_legacy_methods::{ModelRole, TchedRouterLegacyMethods}, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 39 | DEBT-RR | WIRE | read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), "tched_router")?; | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 40 | DEBT-RR | CODE | method_contract_admission::admit_compiled_state_contract(&current, "tched_router")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 44 | DEBT-RR | CODE | "tched_router plugin seal changed during retained operation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 54 | DEBT-RR | CODE | .context("tched_router sealed-contract worker failed")? | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 108 | DEBT-RR | CODE | &crate::tched_router_runtime::TchedRouterRuntimeClient::from_env() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 115 | DEBT-RR | CODE | client: &crate::tched_router_runtime::TchedRouterRuntimeClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 117 | DEBT-RR | CODE | use crate::tched_router_runtime::{GatewayJsonRequest, GatewayMethod, GatewayProfile}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 124 | DEBT-RR | CODE | operation: "tched_router tools", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 176 | CR-MOD | internal | crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 179 | DEBT-RR | CODE | "native tched_router config/selection changed during retained operation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 197 | CR-MOD | internal | crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 221 | DEBT-RR | CODE | documents.context("retained tched_router method requires the configured durable owner")?, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 258 | DEBT-RR | CODE | anyhow::bail!("retained tched_router method has no native/durable adapter: {method}") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 306 | DEBT-RR | CODE | let client = crate::tched_router_runtime::TchedRouterRuntimeClient::new( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tched_router_runtime.rs` (137 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | //! Loopback adapter for the real tched_router daemon. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 15 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 42 | DEBT-RR | CODE | #[error("tched_router {operation} returned HTTP {status}")] | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 138 | DEBT-RR | CODE | pub(crate) binding: op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingOutput, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 217 | DEBT-RR | WIRE | let endpoint = std::env::var("TCHED_ROUTER_RUNTIME_ENDPOINT") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 219 | DEBT-RR | WIRE | let agent_alias = std::env::var("TCHED_ROUTER_RUNTIME_AGENT") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 221 | DEBT-RR | WIRE | let token = std::env::var("TCHED_ROUTER_RUNTIME_TOKEN") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 248 | DEBT-RR | CODE | "failed to initialize isolated tched_router HTTP client: {error}" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 276 | DEBT-RR | CODE | .unwrap_or_else(\|\| "tched_router HTTP client is unavailable".to_string()) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 287 | DEBT-RR | CODE | input: op_plugins::state_plugins::tched_router::generation::StartGenerationInput, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 290 | DEBT-RR | CODE | dyn crate::tched_router_generation_native::NativeGenerationAdmission, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 292 | DEBT-RR | CODE | ) -> anyhow::Result<crate::tched_router_generation_native::NativeGenerationTask> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 304 | DEBT-RR | CODE | input: op_plugins::state_plugins::tched_router::generation::StartGenerationInput, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 307 | DEBT-RR | CODE | dyn crate::tched_router_generation_native::NativeGenerationAdmission, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 311 | DEBT-RR | CODE | ) -> anyhow::Result<crate::tched_router_generation_native::NativeGenerationTask> { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 320 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationTask::spawn( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 321 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationRequest { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 343 | DEBT-RR | CODE | dyn crate::tched_router_generation_native::NativeGenerationAdmission, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 373 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::generation::StartGenerationInput { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 390 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationTerminal::Done(done) => done, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 391 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationTerminal::Cancelled => { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 444 | DEBT-RR | CODE | .with_context(\|\| format!("tched_router {operation} request failed"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 455 | DEBT-RR | CODE | "tched_router {operation} did not authenticate the forwarded SID1" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 463 | DEBT-RR | CODE | "tched_router {operation} response exceeds {max_response_bytes} bytes" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 470 | DEBT-RR | CODE | chunk.with_context(\|\| format!("tched_router {operation} response body failed"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 473 | DEBT-RR | CODE | "tched_router {operation} response exceeds {max_response_bytes} bytes" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 487 | DEBT-RR | CODE | .with_context(\|\| format!("tched_router {operation} returned invalid JSON")) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 620 | DEBT-RR | CODE | "tched_router event subscription timed out".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 625 | DEBT-RR | CODE | "tched_router event subscription failed: {error}" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 668 | DEBT-RR | CODE | "tched_router masked config response is not an object" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 698 | DEBT-RR | CODE | .context("tched_router config list response is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 702 | DEBT-RR | CODE | .context("tched_router config list response has no entries array")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 705 | DEBT-RR | CODE | "tched_router config list contains too many entries" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 710 | DEBT-RR | CODE | .context("tched_router config list entry is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 714 | DEBT-RR | CODE | .context("tched_router config list entry has no path")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 733 | DEBT-RR | CODE | .context("tched_router config list response has no drifted array")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 736 | DEBT-RR | CODE | "tched_router config drift list contains too many entries" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 741 | DEBT-RR | CODE | .context("tched_router config drift entry is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 745 | DEBT-RR | CODE | .context("tched_router config drift entry has no path")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 749 | DEBT-RR | CODE | "tched_router config drift entry is not marked drifted" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 761 | DEBT-RR | CODE | "tched_router config drift display value is not a string" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 790 | DEBT-RR | CODE | .context("tched_router config property response is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 793 | DEBT-RR | CODE | "tched_router config property response path mismatch" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 798 | DEBT-RR | CODE | "tched_router secret property response was not populated-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 802 | DEBT-RR | CODE | "tched_router public property response was not value-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 807 | DEBT-RR | CODE | "tched_router secret property populated flag is not boolean" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 813 | DEBT-RR | CODE | "tched_router property display value is not a string" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 857 | DEBT-RR | CODE | "tched_router secret property changed during readback" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 862 | DEBT-RR | CODE | "tched_router property changed during readback" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1025 | CR-MOD | internal | crate::tched_catalog_binding::valid_reference(reference), | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1121 | DEBT-RR | CODE | "tched_router health status is '{}', expected 'ok'", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1141 | DEBT-RR | CODE | .ok_or_else(\|\| anyhow!("tched_router config property '{path}' is not a string")) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1166 | CR-MOD | internal | target: &crate::tched_catalog_binding::BindingTarget, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1175 | CR-MOD | internal | use crate::tched_catalog_binding::{ | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1277 | CR-MOD | internal | let binding = crate::tched_catalog_binding::select(target, &candidates)?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1335 | CR-MOD | internal | crate::tched_catalog_binding::valid_reference(reference), | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1347 | DEBT-RR | WIRE | std::env::var("TCHED_ROUTER_REQUIRE_OPENCODE").as_deref() == Ok("1") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 1377 | DEBT-RR | CODE | "tched_router agent provider/model observation is empty" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1389 | DEBT-RR | CODE | "tched_router agent alias cannot be addressed through the config API" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1412 | DEBT-RR | CODE | "tched_router agent selects provider '{provider_ref}' absent from the configured provider list" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1417 | DEBT-RR | CODE | "tched_router selected provider model changed during observation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1458 | DEBT-RR | CODE | "tched_router selected provider is absent from the configured provider list", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1462 | DEBT-RR | CODE | "tched_router selected provider model changed during observation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1480 | DEBT-RR | CODE | .context("tched_router provider family has no configured aliases")? | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1491 | DEBT-RR | CODE | "tched_router '{}' model catalog contains an empty or duplicate model", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1497 | DEBT-RR | CODE | "tched_router selected '{}' model catalog is empty", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1512 | DEBT-RR | CODE | "tched_router selected model is absent from its configured alias catalog" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1518 | DEBT-RR | CODE | kind: "tched-router-runtime".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1520 | DEBT-RR | CODE | sdk: "tched_router".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1521 | DEBT-RR | CODE | description: format!("Configured tched_router provider family '{family}'."), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1529 | DEBT-RR | CODE | transport: "tched-router-loopback".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1539 | DEBT-RR | CODE | "Discovered through the live tched_router model catalog.".to_string() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1541 | DEBT-RR | CODE | "Returned by tched_router's cached model catalog.".to_string() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1558 | DEBT-RR | CODE | "tched_router selection changed during catalog observation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1570 | DEBT-RR | CODE | state.catalog.router.role = "tched-router-runtime-authority".to_string(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1586 | DEBT-RR | CODE | "tched_router configuration changed during native observation" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1593 | DEBT-RR | CODE | "tched_router observation is missing status, selection, or provider catalog" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1612 | DEBT-RR | CODE | "tched_router observed selection/catalog bindings are inconsistent" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1619 | DEBT-RR | CODE | "native tched_router catalog contains duplicate provider/model bindings" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1653 | DEBT-RR | CODE | .ok_or_else(\|\| anyhow!("tched_router provider '{selector}' is not configured"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1656 | DEBT-RR | CODE | "tched_router provider '{selector}' has multiple configured aliases; specify the provider reference" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1703 | DEBT-RR | CODE | "tched_router model '{model_id}' is not present in the observed '{}' catalog", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1752 | DEBT-RR | CODE | "tched_router model '{model_id}' is not present in the observed '{}' catalog", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1809 | DEBT-RR | CODE | "requested provider '{}' differs from tched_router's selected provider '{}'", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1818 | DEBT-RR | CODE | "requested model '{}' differs from tched_router's selected model '{}'", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1838 | DEBT-RR | CODE | .unwrap_or("unknown tched_router A2A error"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1840 | DEBT-RR | CODE | "tched_router chat failed: {}", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1924 | DEBT-RR | CODE | .context("tched_router config property response is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1927 | DEBT-RR | CODE | "tched_router config property response path mismatch" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1931 | DEBT-RR | CODE | "tched_router secret property response was not populated-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1935 | DEBT-RR | CODE | "tched_router public property response was not value-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1940 | DEBT-RR | CODE | "tched_router secret property populated flag is not boolean" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1946 | DEBT-RR | CODE | "tched_router property display value is not a string" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1961 | DEBT-RR | CODE | .context("tched_router config patch response is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1964 | DEBT-RR | CODE | "tched_router config patch did not report a saved configuration" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1969 | DEBT-RR | CODE | .context("tched_router config patch response has no results array")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1972 | DEBT-RR | CODE | "tched_router config patch result count mismatch" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1977 | DEBT-RR | CODE | .context("tched_router config patch result is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1981 | DEBT-RR | CODE | .context("tched_router config patch result has no path")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1995 | DEBT-RR | CODE | "tched_router secret patch result was not populated-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2007 | DEBT-RR | CODE | "tched_router public patch result was not value-only" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2016 | DEBT-RR | CODE | "tched_router patch display value disagrees with operation kind" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2149 | DEBT-RR | CODE | .context("tched_router config init response has no initialized array")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2158 | DEBT-RR | CODE | .context("tched_router config init response contains a non-string section")?, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2160 | DEBT-RR | CODE | .context("tched_router config init response contains an invalid section")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2170 | DEBT-RR | CODE | "invalid tched_router config property path" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2346 | DEBT-RR | CODE | anyhow!("tched_router configured provider path has no family/alias binding") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2350 | DEBT-RR | CODE | "tched_router configured provider path has an empty family or alias segment" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2357 | DEBT-RR | CODE | anyhow!("tched_router configured provider model is missing a string value") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2370 | DEBT-RR | CODE | "tched_router configured provider catalog contains a duplicate reference" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2396 | CR-MOD | internal | target: &crate::tched_catalog_binding::BindingTarget, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 2404 | CR-MOD | internal | crate::tched_catalog_binding::valid_reference(&provider.reference), | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 2413 | CR-MOD | internal | && crate::tched_catalog_binding::reference_family(&provider.reference) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 2482 | DEBT-RR | CODE | .map_err(\|error\| format!("invalid tched_router runtime endpoint: {error}"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2490 | DEBT-RR | CODE | return Err("tched_router runtime endpoint must be an HTTP loopback origin".to_string()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2494 | DEBT-RR | CODE | .ok_or_else(\|\| "tched_router runtime endpoint has no host".to_string())?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2499 | DEBT-RR | CODE | .map_err(\|_\| "tched_router runtime endpoint host must be an IP literal".to_string())?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2501 | DEBT-RR | CODE | return Err("tched_router runtime endpoint must use a loopback address".to_string()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2509 | DEBT-RR | CODE | .context("tched_router provider reference has no family/alias binding")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2522 | DEBT-RR | CODE | "tched_router provider reference cannot be addressed through the config API" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2577 | DEBT-RR | CODE | return Err(anyhow!("tched_router.Chat requires message or messages")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2613 | DEBT-RR | CODE | .ok_or_else(\|\| anyhow!("tched_router chat response contains no text artifact")) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2630 | CR-MOD | internal | fn binding_target() -> crate::tched_catalog_binding::BindingTarget { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 2631 | CR-MOD | internal | crate::tched_catalog_binding::BindingTarget { | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 2902 | DEBT-RR | CODE | impl crate::tched_router_generation_native::NativeGenerationAdmission for AllowGenerationAdmission { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2910 | DEBT-RR | CODE | crate::tched_router_generation_native::NativeGenerationError, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 2954 | DEBT-RR | WIRE | "tched_router.generation.v1".parse().unwrap(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 3101 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router::TchedChatMessage; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3517 | DEBT-RR | CODE | "tched_router.Chat requires message or messages" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3698 | DEBT-RR | CODE | op_core::projection_shm::update_projection_in(state_dir, "tched_router", \|_\| { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3709 | DEBT-RR | CODE | op_core::projection_shm::update_projection_in(state_dir, "tched_router", \|previous\| { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3715 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3716 | DEBT-RR | WIRE | "/org/opdbus/v1/plugins/tched_router", | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 3727 | DEBT-RR | CODE | &["tched_router".into()], | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3730 | DEBT-RR | CODE | let state: Value = serde_json::from_slice(&snapshot.states["tched_router"]).unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3740 | DEBT-RR | CODE | &snapshot.states["tched_router"] | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3743 | DEBT-RR | CODE | // Insert inside tched_router_runtime.rs::tests. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/tracing.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 3 | DEBT-RR | CODE | // response. Operates inside the tched_router Axum host (the gRPC/gRPC-Web side of | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/ui_workspace_dispatch.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 43 | DEBT-RR | CODE | tched_router_admission::{FacadeOrigin, RouterCaller, readmit_call}, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/web_ui_dispatch.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 7 | DEBT-RR | CODE | mutation_engine::MutationEngine, project_dispatch, tched_router_admission::FacadeOrigin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/xray_methods_tests.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 584 | FP | internal | async fn mismatched_missing_and_duplicate_user_targets_never_mutate() { | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |

## `crates/op-grpc-bridge/src/method_receipt_audit.rs` (15 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 33 | DEBT-RR | CODE | if let Some(http) = cause.downcast_ref::<crate::tched_router_runtime::NativeHttpFailure>() { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 140 | DEBT-RR | CODE | ("tched_router list ", "native_model_catalog_failed"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 142 | DEBT-RR | CODE | "tched_router read masked config", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 146 | DEBT-RR | CODE | "tched_router read config property", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 149 | DEBT-RR | CODE | ("tched_router health", "native_health_failed"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 162 | DEBT-RR | CODE | ("tched_router chat failed", "native_agent_rpc_failed"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 164 | DEBT-RR | CODE | "tched_router chat returned invalid JSON", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 167 | DEBT-RR | CODE | ("tched_router chat returned", "native_agent_http_failed"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 169 | DEBT-RR | CODE | "tched_router chat request failed", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 397 | C-TFS | internal | "{code}; operation_id={operation_id}; use tched_fs.get_receipt; do not retry automatically" | correct per contract; no change | 0.96 | none |
| 800 | C-THREE | internal | "threetched_skills_add_editor_unsupported", "omit --edit", | correct per contract; no change | 0.96 | none |
| 805 | C-THREE | internal | assert_eq!(refusal.code(), "threetched_skills_add_editor_unsupported"); | correct per contract; no change | 0.96 | none |
| 806 | C-THREE | internal | assert_eq!(refusal.to_string(), "threetched_skills_add_editor_unsupported; omit --edit"); | correct per contract; no change | 0.96 | none |
| 822 | C-THREE | internal | anyhow::anyhow!("threetched_skills_add_editor_unsupported; omit --edit"), | correct per contract; no change | 0.96 | none |
| 823 | C-THREE | internal | anyhow::anyhow!("threetched command exited with 1"), | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/lib.rs` (13 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 131 | CR-MOD | internal | mod tched_catalog_binding; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 132 | CR-MOD | internal | pub mod tched_config_dispatch; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 133 | CR-MOD | internal | mod tched_config_engine; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 134 | CR-MOD | internal | mod tched_config_secret_policy; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 135 | CR-MOD | internal | mod tched_own_agent_route; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 136 | DEBT-RR | CODE | mod tched_router_admission; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 137 | DEBT-RR | CODE | mod tched_router_generation_native; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 138 | DEBT-RR | CODE | mod tched_router_legacy_dispatch; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 139 | DEBT-RR | CODE | pub mod tched_router_runtime; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 166 | DEBT-RR | CODE | pub use server::{PemIdentity, ServerConfig, run_tched_router_server, serve_upgradeable_tcp}; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 195 | DEBT-RR | CODE | /// 3tched Router plugin schema gRPC service (GetSchema / WatchSchema). | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 196 | DEBT-RR | CODE | pub mod tched_router { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 197 | DEBT-RR | CODE | tonic::include_proto!("tched_router"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/mutation_engine.rs` (158 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 137 | DEBT-RR | CODE | facade_origin: Option<crate::tched_router_admission::FacadeOrigin>, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 213 | C-THREE | internal | \| "threetched" | correct per contract; no change | 0.96 | none |
| 217 | DEBT-RR | WIRE | \|\| (plugin_id == "tched_router" | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 296 | C-THREE | internal | /// Provider runtime used only after threetched resolves a schema-declared route. | correct per contract; no change | 0.96 | none |
| 1580 | C-TFS | internal | /// 3tchedFS mount. | correct per contract; no change | 0.96 | none |
| 3500 | DEBT-RR | CODE | origin: crate::tched_router_admission::FacadeOrigin, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3532 | DEBT-RR | CODE | facade_origin: Option<crate::tched_router_admission::FacadeOrigin>, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3942 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 3948 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4302 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4308 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4374 | C-TFS | internal | "tched_fs" => { | correct per contract; no change | 0.96 | none |
| 4387 | C-TFS | internal | op_plugins::state_plugins::tched_fs::dispatch(method, &args)? | correct per contract; no change | 0.96 | none |
| 4391 | C-TFS | internal | op_plugins::state_plugins::tched_fs::protocol_value()?, | correct per contract; no change | 0.96 | none |
| 4398 | C-THREE | internal | "threetched" => { | correct per contract; no change | 0.96 | none |
| 4401 | C-THREE | internal | // Captured CLI commands and router methods operate on the same threetched service | correct per contract; no change | 0.96 | none |
| 4406 | DEBT-RR | CODE | crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4409 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4415 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4418 | C-THREE | internal | "threetched", | correct per contract; no change | 0.96 | none |
| 4447 | C-THREE | internal | .context("pinned threetched feature method is absent")?; | correct per contract; no change | 0.96 | none |
| 4480 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4507 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4513 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4545 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4551 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4582 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4588 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4772 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4778 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4817 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4823 | DEBT-RR | CODE | let _pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4845 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4851 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4879 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4938 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4944 | DEBT-RR | CODE | let pinned = crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 4993 | DEBT-RR | CODE | "tched_router" if crate::generation_jobs_dispatch::is_generation_method(method) => { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5007 | DEBT-RR | CODE | "tched_router" => { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5014 | DEBT-RR | CODE | let caller = crate::tched_router_admission::RouterCaller { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5023 | DEBT-RR | CODE | .dispatch_tched_router_owned( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5036 | DEBT-RR | CODE | .map_err(\|error\| anyhow::anyhow!("tched_router owned task failed: {error}"))?? | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5252 | DEBT-RR | CODE | async fn dispatch_tched_router_owned( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5259 | DEBT-RR | CODE | caller: crate::tched_router_admission::RouterCaller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5260 | DEBT-RR | CODE | facade_origin: Option<crate::tched_router_admission::FacadeOrigin>, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5264 | DEBT-RR | CODE | crate::tched_router_admission::admit_guarded(self, &admitted_schema, method, &caller) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5295 | DEBT-RR | CODE | let _: op_plugins::state_plugins::tched_router::GetOwnAgentRouteInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5298 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::GetOwnAgentRouteAccessOutput { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5299 | CR-MOD | internal | can_apply: crate::tched_own_agent_route::can_apply(self, &caller).await?, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5302 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5303 | DEBT-RR | CODE | self, &admitted_schema, "tched_router", method, &caller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5318 | DEBT-RR | CODE | let input: op_plugins::state_plugins::tched_router::ApplyOwnAgentRouteInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5321 | CR-MOD | internal | caller.capability.as_deref() == Some(crate::tched_own_agent_route::WRITE_CAPABILITY) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5322 | CR-MOD | internal | && crate::tched_own_agent_route::can_apply(self, &caller).await?, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5328 | CR-MOD | internal | let (_, before) = crate::tched_config_engine::coherent_observation( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5331 | CR-MOD | internal | let (patch, expected) = crate::tched_own_agent_route::prepare( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5338 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5339 | DEBT-RR | CODE | self, &admitted_schema, "tched_router", method, &caller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5348 | CR-MOD | internal | crate::tched_own_agent_route::can_apply(self, &caller).await?, | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5357 | CR-MOD | internal | let secrets = crate::tched_config_engine::pinned_config_secrets(pinned)?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5358 | DEBT-RR | CODE | let runtime = crate::tched_router_runtime::TchedRouterRuntimeClient::for_agent_alias( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5365 | CR-MOD | internal | let (_, represented) = crate::tched_config_engine::coherent_observation( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5368 | CR-MOD | internal | crate::tched_own_agent_route::verify_readback( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5371 | DEBT-RR | CODE | self.persist_tched_router_config_observation(method, &represented, &pinned.schema_hash).await?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5372 | DEBT-RR | CODE | let committed = self.read_tched_router_config_field("native_config", &pinned.schema_hash).await?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5378 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5379 | DEBT-RR | CODE | self, &admitted_schema, "tched_router", method, &caller, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5381 | CR-MOD | internal | anyhow::ensure!(crate::tched_own_agent_route::can_apply(self, &caller).await?, "own route authority changed after selection"); | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5388 | CR-MOD | internal | let output = serde_json::to_value(crate::tched_own_agent_route::project( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5399 | DEBT-RR | CODE | let _: op_plugins::state_plugins::tched_router::GetOwnAgentRouteInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5415 | CR-MOD | internal | crate::tched_config_engine::coherent_observation(pinned, &sealed_identity).await?; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5416 | DEBT-RR | CODE | self.persist_tched_router_config_observation(method, &represented, &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5419 | DEBT-RR | CODE | .read_tched_router_config_field("native_config", &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5421 | CR-MOD | internal | let output = serde_json::to_value(crate::tched_own_agent_route::project( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5425 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5428 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5464 | DEBT-RR | CODE | let input: op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5495 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5498 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5514 | C-THREE | internal | // Catalog selection uses an existing threetched profile. Applying | correct per contract; no change | 0.96 | none |
| 5517 | CR-MOD | internal | let mut observation = crate::tched_config_engine::observe_catalog_binding( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5537 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5540 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5574 | DEBT-RR | CODE | crate::tched_router_runtime::TchedRouterRuntimeClient::for_agent_alias( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5581 | CR-MOD | internal | let fields = crate::tched_config_engine::coherent_catalog_selection_observation(pinned, &sealed_identity) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5583 | CR-MOD | internal | observation = crate::tched_config_engine::observe_catalog_binding(pinned, &sealed_identity, &target) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5607 | DEBT-RR | CODE | self.persist_tched_router_config_observation(method, &represented, &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5610 | DEBT-RR | CODE | .read_tched_router_config_field("native_config", &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5613 | DEBT-RR | CODE | .read_tched_router_config_field("catalog_model_binding", &pinned.schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5629 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5632 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5673 | CR-MOD | internal | if let Some(result) = crate::tched_config_engine::dispatch( | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5685 | DEBT-RR | CODE | let observed = crate::large_language_model_dispatch::observed_tched_router_state( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5691 | DEBT-RR | CODE | return op_plugins::state_plugins::tched_router::dispatch_tched_router_method( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5697 | DEBT-RR | CODE | "tched_router observed read dispatch error for '{}': {}", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5706 | DEBT-RR | CODE | serde_json::from_value::<op_plugins::state_plugins::tched_router::ChatInput>( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5709 | DEBT-RR | CODE | .context("invalid tched_router.Chat arguments")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5772 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5775 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5801 | DEBT-RR | CODE | .push(op_plugins::state_plugins::tched_router::TchedChatMessage { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5808 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::TchedChatMessage { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5823 | DEBT-RR | CODE | serde_json::json!({ "source": "tched_router.chat", "context": provenance }), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5827 | DEBT-RR | CODE | // Selected model on tched-router (:8084). Tools are compact MCP on | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5839 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5853 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5865 | DEBT-RR | CODE | provenance.push(format!("tched_router.routing:{routing}")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5886 | DEBT-RR | CODE | crate::tched_router_admission::readmit_call( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5889 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5918 | DEBT-RR | CODE | let receipt = crate::tched_router_legacy_dispatch::dispatch( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5926 | DEBT-RR | CODE | self.persist_tched_router_fields( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5929 | DEBT-RR | CODE | format!("tched_router_legacy:{method}"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5934 | DEBT-RR | CODE | "tched_router.{method} completed but its authoritative observation could not be published" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5945 | DEBT-RR | CODE | .projected_state::<serde_json::Value>("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5947 | DEBT-RR | CODE | let runtime = crate::tched_router_runtime::TchedRouterRuntimeClient::from_env() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5952 | DEBT-RR | CODE | let input: op_plugins::state_plugins::tched_router::SetProviderInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5957 | DEBT-RR | CODE | let input: op_plugins::state_plugins::tched_router::SetModelInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5962 | DEBT-RR | CODE | let input: op_plugins::state_plugins::tched_router::SetSelectionInput = | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5973 | CR-MOD | internal | let (readback, represented) = crate::tched_config_engine::coherent_observation(pinned, &sealed_identity) | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 5976 | DEBT-RR | CODE | "native tched_router selection completed but coherent runtime readback failed; outcome requires reconciliation before retry", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5981 | DEBT-RR | CODE | "native tched_router selection completed but provider response diverges from authoritative readback; outcome requires reconciliation before  | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5983 | DEBT-RR | CODE | self.persist_tched_router_observation(&represented, &expected_schema_hash) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 5986 | DEBT-RR | CODE | "native tched_router selection completed but its authoritative observation could not be published; outcome requires reconciliation before re | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6016 | DEBT-RR | CODE | .projected_state::<serde_json::Value>("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6018 | DEBT-RR | CODE | .context("tched_router legacy state-backed method has no committed complete state")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6021 | DEBT-RR | CODE | .context("committed tched_router state is not an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6044 | DEBT-RR | CODE | "tched_router legacy state-backed method requires observed/stored field '{required}'" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6048 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::TchedRouterState, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6050 | DEBT-RR | CODE | .context("committed complete tched_router state does not match its typed contract")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6051 | DEBT-RR | CODE | match op_plugins::state_plugins::tched_router::dispatch_tched_router_method( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6057 | DEBT-RR | CODE | "tched_router.{method} has no canonical native/durable mutation adapter" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6066 | DEBT-RR | CODE | "tched_router dispatch error for '{}': {}", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6280 | DEBT-RR | CODE | pub(crate) async fn persist_tched_router_config_observation( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6288 | DEBT-RR | CODE | "tched_router config observation must be an object" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6290 | DEBT-RR | CODE | self.persist_tched_router_fields( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6293 | DEBT-RR | CODE | format!("tched_router_config:{method}"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6298 | DEBT-RR | CODE | pub(crate) async fn read_tched_router_config_field( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6315 | DEBT-RR | CODE | "field is not owned by the tched_router config facade" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6323 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6327 | DEBT-RR | CODE | "tched_router seal changed before committed config read" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6330 | DEBT-RR | CODE | op_core::projection_shm::read_projection_snapshot(&["tched_router".to_string()])?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6333 | DEBT-RR | CODE | .remove("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6334 | DEBT-RR | CODE | .context("tched_router has no committed state")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6336 | DEBT-RR | CODE | .context("committed tched_router state is invalid JSON")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6340 | DEBT-RR | CODE | .with_context(\|\| format!("committed tched_router field '{field}' is absent"))?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6343 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6347 | DEBT-RR | CODE | "tched_router seal changed during committed config read" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6354 | DEBT-RR | CODE | async fn persist_tched_router_fields( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6362 | DEBT-RR | CODE | "tched_router observed fields must be an object" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6368 | DEBT-RR | WIRE | plugin_id: "tched_router".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 6369 | DEBT-RR | CODE | object_path: op_plugins::canonical::plugin_path("tched_router"), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6387 | DEBT-RR | CODE | pub(crate) async fn persist_tched_router_observation( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6394 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router_legacy_methods::TchedRouterLegacyMethods::new( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6401 | DEBT-RR | CODE | .context("tched_router runtime observation must be an object")?; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6406 | DEBT-RR | CODE | self.persist_tched_router_fields( | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6409 | DEBT-RR | CODE | "tched_router_selection".to_string(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 6414 | DEBT-RR | CODE | "tched_router native read completed but observation publication failed: {error}" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 7872 | DEBT-RR | CODE | let first = r#"{"plugin":"tched_router","method":"config_set","args":{"path":"agents.control_plane.model_provider","value":"custom.route"}," | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 7907 | DEBT-RR | CODE | ("tched_router", "Chat", r#"{"message":"private message"}"#), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/src/native_container_creation.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 42 | C-3T | internal | reserved_name: reserved_chatbot.then(\|\| "3tchedCtlChatbot".to_owned()) }) | correct per contract; no change | 0.96 | none |
| 80 | C-3T | internal | ensure!(name == "3tchedCtlChatbot" && request.owner_kind == "identity" | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/identity_clean_provision.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 17 | C-3T | WIRE | const PARTITIONED_POOL: &str = "/var/lib/incus/storage-pools/3tched-pool"; | correct per contract; no change | 0.96 | none |
| 253 | C-3T | internal | self.instance["profiles"] == serde_json::json!(["3tched-lungs"]), | correct per contract; no change | 0.96 | none |
| 330 | C-TFS | internal | && !devices.contains_key("3tchedfs") && !devices.contains_key("system-bus"), | correct per contract; no change | 0.96 | none |
| 849 | C-3T | internal | let before = serde_json::json!({"name":"3tchedChatbot","status":"Stopped","status_code":102,"last_used_at":"old", | correct per contract; no change | 0.96 | none |
| 850 | C-3T | internal | "config":{"volatile.uuid":"native-id","volatile.idmap.next":"prepared","security.privileged":"false"},"expanded_config":{},"devices":[{"name | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/src/identity_sled_dispatch.rs` (5 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 392 | C-3T | internal | const CONTROL_PLANE_CHATBOT_CONTAINER: &str = "3tchedChatbot"; | correct per contract; no change | 0.96 | none |
| 393 | C-3T | internal | const RESERVED_CHATBOT_CONTAINER: &str = "3tchedCtlChatbot"; | correct per contract; no change | 0.96 | none |
| 4287 | FP | internal | let mut mismatched_human = refreshed.clone(); | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |
| 4288 | FP | internal | mismatched_human.instance = Some(unrelated_instance); | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |
| 4289 | FP | internal | assert!(!identity_session_binding_matches(&mismatched_human)); | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |

## `crates/op-grpc-bridge/src/lungs_ingress.rs` (20 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 8 | C-TFS | internal | use tched_fs::mounted::{GuestMountSnapshot, MountPin, MountedInvocation, MountedNamespace, GUEST_MOUNT_PATHS, declared_mount_index}; | correct per contract; no change | 0.96 | none |
| 10 | C-TFS | internal | use tched_fs::invocation::{CallMetadata, Deadline}; | correct per contract; no change | 0.96 | none |
| 98 | C-THREE | internal | ("threetched", &["config.get"][..]), | correct per contract; no change | 0.96 | none |
| 239 | C-TFS | internal | pub(crate) async fn catalog(&self, engine: &MutationEngine, input: op_plugins::state_plugins::tched_fs::GetCatalogInput) -> Result<Value> { | correct per contract; no change | 0.96 | none |
| 240 | C-TFS | internal | use op_plugins::state_plugins::tched_fs::{CatalogOperation, CatalogPlugin, CatalogState, CatalogView, GetCatalogOutput}; | correct per contract; no change | 0.96 | none |
| 261 | C-TFS | internal | let prefix = format!("plugins/{}", tched_fs::namespace::encode_component(plugin)); | correct per contract; no change | 0.96 | none |
| 263 | C-TFS | internal | let path = format!("{prefix}/operations/{}", tched_fs::namespace::encode_component(method)); | correct per contract; no change | 0.96 | none |
| 333 | C-THREE | internal | ensure!(plugin == "threetched" && method == "config.get", | correct per contract; no change | 0.96 | none |
| 459 | C-TFS | internal | source_schema_hash, tched_fs::mounted::AdmittedOperation::Mounted, Deadline::application(), None).await | correct per contract; no change | 0.96 | none |
| 462 | C-THREE | internal | /// Inner admitted threetched effect: execute in the same verified guest, | correct per contract; no change | 0.96 | none |
| 472 | C-THREE | internal | invoke_operation(engine, "threetched", method, arguments, caller, capability, proof, | correct per contract; no change | 0.96 | none |
| 473 | C-TFS | internal | source_schema_hash, tched_fs::mounted::AdmittedOperation::Application { input }, context.deadline(), Some(origin)).await | correct per contract; no change | 0.96 | none |
| 479 | C-TFS | internal | source_schema_hash: &str, operation: Option<tched_fs::mounted::AdmittedOperation>, deadline: Deadline, | correct per contract; no change | 0.96 | none |
| 484 | C-THREE | internal | ensure!(plugin != "threetched" \|\| method != "stateless-generate", | correct per contract; no change | 0.96 | none |
| 486 | C-TFS | internal | let application = matches!(&operation, Some(tched_fs::mounted::AdmittedOperation::Application { .. })); | correct per contract; no change | 0.96 | none |
| 557 | C-TFS | internal | source_schema_hash: &str, operation: tched_fs::mounted::AdmittedOperation, deadline: Deadline, | correct per contract; no change | 0.96 | none |
| 560 | C-TFS | internal | let application = matches!(&operation, tched_fs::mounted::AdmittedOperation::Application { .. }); | correct per contract; no change | 0.96 | none |
| 578 | C-THREE | internal | ensure!(exit == 0, "threetched command exited {exit}; do not retry automatically"); | correct per contract; no change | 0.96 | none |
| 581 | C-TFS | internal | let schema = tched_fs::model::from_sealed(pinned.schema)?; | correct per contract; no change | 0.96 | none |
| 583 | C-TFS | internal | let mut receipt = tched_fs::mounted::decode_terminal_receipt(&output.stdout, plugin, method, declaration)?; | correct per contract; no change | 0.96 | none |

## `crates/op-grpc-bridge/tests/chain_event_broadcast.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 34 | FP | internal | async fn dispatched_method_call_reaches_chain_subscribers() { | false positive (English token matched/dispatched/mismatched); no change | 0.99 | none (English word, not identifier) |

## `crates/op-grpc-bridge/tests/grpc_speed_test.rs` (2 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 46 | DEBT-RR | WIRE | plugin_id: "tched_router".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 57 | DEBT-RR | WIRE | plugin_id: "tched_router".to_string(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/tests/large_language_model_dispatch.rs` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 13 | DEBT-RR | CODE | ["OP_GRANTS_PATH", "TCHED_ROUTER_RUNTIME_ENDPOINT"] | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 41 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::TchedRouterPlugin::new() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 75 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 96 | DEBT-RR | CODE | assert!(error.to_string().contains("tched_router")); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/live_reflection_probe.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 8 | DEBT-RR | CODE | //! Regression this guards: `run_tched_router_server` (the path the `op-grpc-bridge` | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/negative_topology_gates.rs` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 52 | DEBT-RR | CODE | "crates/op-grpc-bridge/src/grpc/tched_router.proto", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 94 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 132 | DEBT-RR | CODE | "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/sealed_catalog_authority.rs` (1 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 73 | DEBT-RR | CODE | assert!(engine.get_state("tched_router").await.is_none()); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/tched-router-busctl-smoke.sh` (3 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 2 | DEBT-RR | CODE | # 3tched Router D-Bus smoke test (mission T-18). | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 15 | DEBT-RR | WIRE | PLUGIN_PATH="/org/opdbus/v1/plugins/tched_router" | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 18 | DEBT-RR | CODE | say() { printf '[tched-router-smoke] %s\n' "$1"; } | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/tched_router_backing.rs` (48 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 149 | DEBT-RR | CODE | use op_plugins::state_plugins::tched_router_contract::document; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 403 | DEBT-RR | CODE | let schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 406 | DEBT-RR | CODE | engine.dispatch_method_call_with_identity("tched_router", method, &args.to_string(), schema.methods[method].required_capability.as_deref(),  | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 409 | DEBT-RR | CODE | .unwrap_or_else(\|\| op_blob::catalog::DEFAULT_SHM_DIR.into()), "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 445 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 446 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_AGENT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 447 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_TOKEN", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 464 | DEBT-RR | CODE | unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_ENDPOINT", endpoint) }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 466 | DEBT-RR | CODE | unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_AGENT", "dashboard") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 468 | DEBT-RR | CODE | unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_TOKEN", "fixture-secret") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 479 | DEBT-RR | CODE | let schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 764 | DEBT-RR | CODE | op_core::projection_shm::read_projection_bytes("tched_router").unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 771 | DEBT-RR | CODE | op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 790 | DEBT-RR | WIRE | .filter(\|capability\| *capability != "cap.software.3tched-router.route.resolve@v1") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 821 | DEBT-RR | WIRE | let result = engine.dispatch_method_call_with_identity("tched_router", "ResolveRoute", &args.to_string(), Some("cap.software.3tched-router.r | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 824 | DEBT-RR | CODE | .unwrap_or_else(\|\| op_blob::catalog::DEFAULT_SHM_DIR.into()), "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 934 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 938 | DEBT-RR | CODE | let initial = engine.get_state("tched_router").await.unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 949 | DEBT-RR | CODE | assert_eq!(engine.get_state("tched_router").await.unwrap(), initial); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1016 | DEBT-RR | CODE | assert_eq!(engine.get_state("tched_router").await.unwrap(), initial); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1031 | DEBT-RR | CODE | op_core::projection_shm::read_projection_snapshot(&["tched_router".into()]) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1048 | DEBT-RR | CODE | serde_json::to_value(engine.get_state("tched_router").await.unwrap()).unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1054 | DEBT-RR | CODE | op_core::projection_shm::read_projection_snapshot(&["tched_router".into()]) | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1118 | DEBT-RR | CODE | let before_failed_write = engine.get_state("tched_router").await.unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1131 | DEBT-RR | CODE | engine.get_state("tched_router").await.unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1145 | DEBT-RR | CODE | serde_json::to_value(engine.get_state("tched_router").await.unwrap()).unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1155 | DEBT-RR | CODE | let before_publication_failure = engine.get_state("tched_router").await.unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1156 | DEBT-RR | CODE | let before_shm = op_core::projection_shm::read_projection_bytes("tched_router").unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1176 | DEBT-RR | CODE | engine.get_state("tched_router").await.unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1182 | DEBT-RR | CODE | op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1190 | DEBT-RR | CODE | let before_post_effect = engine.get_state("tched_router").await.unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1192 | DEBT-RR | CODE | op_core::projection_shm::read_projection_bytes("tched_router").unwrap(); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1207 | DEBT-RR | CODE | "native tched_router selection completed but its authoritative observation could not be published" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1213 | DEBT-RR | CODE | engine.get_state("tched_router").await.unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1219 | DEBT-RR | CODE | op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1227 | DEBT-RR | CODE | engine.get_state("tched_router").await.unwrap()["selected_provider"], | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1236 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1244 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1282 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1380 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1403 | CR-MOD | internal | use op_grpc_bridge::tched_config_dispatch::{ConfigMethodBacking, config_method_backing}; | bridge-local module identifiers in our-fs namespace (tched*); correct; no change | 0.9 | none (bridge-local tched* module identifiers correct in our-fs namespace) |
| 1421 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1441 | C-TFS | internal | // 3tchedFS mirror. | correct per contract; no change | 0.96 | none |
| 1449 | DEBT-RR | CODE | op_plugins::state_plugins::tched_router::TchedRouterPlugin::current_state(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1453 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1462 | DEBT-RR | CODE | "tched_router".into(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 1463 | DEBT-RR | WIRE | "/org/opdbus/v1/plugins/tched_router".into(), | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 1476 | DEBT-RR | CODE | &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(), | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/tests/tched_router_integration.rs` (21 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 1 | DEBT-RR | CODE | // Integration tests for the tched_router Axum HTTP/gRPC-Web host. | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 17 | DEBT-RR | CODE | use op_grpc_bridge::proto::tched_router::{ | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 18 | DEBT-RR | CODE | GetSchemaRequest, WatchSchemaRequest, tched_router_service_client::TchedRouterServiceClient, | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 98 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_ENDPOINT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 99 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_AGENT", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 100 | DEBT-RR | CODE | "TCHED_ROUTER_RUNTIME_TOKEN", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 111 | DEBT-RR | CODE | unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_ENDPOINT", format!("http://{address}")) }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 113 | DEBT-RR | CODE | unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_AGENT", "dashboard") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 115 | DEBT-RR | CODE | unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") }; | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 120 | DEBT-RR | CODE | let schema = op_state_store::PluginSchema::builder("tched_router") | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 124 | DEBT-RR | CODE | let blob = op_blob::blobify_plugin_schema("tched_router", schema); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 152 | DEBT-RR | CODE | // Build the intercepted plugin service exactly as `run_tched_router_server` | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 153 | DEBT-RR | CODE | // does, so the Axum app mounts both the tched_router service and the plugin | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 201 | DEBT-RR | CODE | assert_eq!(parsed["name"], "tched_router"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 250 | DEBT-RR | CODE | async fn list_models_uses_the_audited_tched_router_method_dispatcher() { | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 251 | DEBT-RR | CODE | let router_schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new() | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 312 | DEBT-RR | CODE | let result = engine.dispatch_method_call_with_identity("tched_router", "ListModels", r#"{"provider":"ollama"}"#, Some(&capability), &actor,  | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 315 | DEBT-RR | CODE | .unwrap_or_else(\|\| op_blob::catalog::DEFAULT_SHM_DIR.into()), "tched_router", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 321 | DEBT-RR | WIRE | assert_eq!(result["plugin_id"], "tched_router"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 333 | DEBT-RR | WIRE | assert_eq!(event.plugin_id, "tched_router"); | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 337 | DEBT-RR | WIRE | Some("cap.software.3tched-router.models.read@v1") | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |

## `crates/op-grpc-bridge/Cargo.toml` (4 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 46 | DEBT-RR | CODE | # HTTP client used by the tched_router runtime integration. Cognitive-tool dispatch | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 51 | C-TFS | internal | tched-fs = { path = "../threetched-fs" } | correct per contract; no change | 0.96 | none |
| 123 | DEBT-RR | CODE | name = "tched_router_integration" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 124 | DEBT-RR | CODE | path = "tests/tched_router_integration.rs" | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |

## `crates/op-grpc-bridge/build.rs` (12 rows)

| line | class | visibility | occurrence (trimmed) | JEV verdict | conf | proposed change |
|---|---|---|---|---|---|---|
| 11 | C-THREE | internal | "THREETCHED_CAPTURE_BIN", | correct per contract; no change | 0.96 | none |
| 12 | C-THREE | internal | "THREETCHED_CAPTURE_MANIFEST", | correct per contract; no change | 0.96 | none |
| 13 | C-THREE | internal | "THREETCHED_BIN", | correct per contract; no change | 0.96 | none |
| 14 | C-THREE | internal | "THREETCHED_MANIFEST", | correct per contract; no change | 0.96 | none |
| 15 | DEBT-RR | CODE | "TCHED_ROUTER_CONFIG_DIR", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 20 | C-THREE | WIRE | ("THREETCHED_CAPTURE_BIN", "THREETCHED_BIN", "/usr/local/libexec/op-dbus/threetched/threetched"), | correct per contract; no change | 0.96 | none |
| 22 | C-THREE | internal | "THREETCHED_CAPTURE_MANIFEST", | correct per contract; no change | 0.96 | none |
| 23 | C-THREE | internal | "THREETCHED_MANIFEST", | correct per contract; no change | 0.96 | none |
| 24 | DEBT-RR | WIRE | "/usr/share/op-dbus/threetched-manifests/tched-router.json", | wire-format; migration-only, no direct rename | 0.93 | migration note: coordinated rename w/ consumers later; NOT direct |
| 44 | C-THREE | internal | op_plugins::state_plugins::threetched::prepare_schema_capture()?; | correct per contract; no change | 0.96 | none |
| 87 | DEBT-RR | CODE | "src/grpc/tched_router.proto", | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |
| 113 | DEBT-RR | CODE | println!("cargo:rerun-if-changed=src/grpc/tched_router.proto"); | retired tched_router debt; propose rename to threetched*/correct module names | 0.82 | rename to `threetched`-prefixed module/symbol/servicename; exact target depends on integration pass |


## Migration notes (deployment-visible, NOT direct renames)
- Env selectors THREETCHED_CAPTURE_BIN, THREETCHED_CAPTURE_MANIFEST, THREETCHED_BIN, THREETCHED_MANIFEST — built into build.rs / build receipts / run scripts; consumers live in 6+ crates & deploy; reserved for coordinated cutover by AuditRoot (item 2 of owner directive).
- Env vars TCHED_ROUTER_RUNTIME_ENDPOINT/TOKEN/AGENT, TCHED_ROUTER_CONTROL_PLANE_MODEL_PROVIDER, TCHED_ROUTER_CONFIG_DIR, TCHED_ROUTER_UNIX_SOCKET, TCHED_ROUTER_BIND_ADDR, TCHED_ROUTER_TLS_* — used by server.rs / tched_router_runtime.rs / deploy/runit/tched-router/run; proposal targets `THREETCHED_RUNTIME_*` etc. at cutover, not now.
- Proto subid `exp.service.3tched-router-bridge.grpc-stream@v1` (proto/operation.proto:918) and `exp.service.3tched-router.register-agent@v1` (per_plugin_reflection.rs:267) — wire-visible, migration-only with descriptor consumers.
- Plugin-id "tched_router" as a string in dispatch/event/state/projection paths (mutation_engine.rs, schema_loader.rs, per_plugin_reflection.rs, tests) — sealed wire format label; migration coordinated, do not touch directly.
- WebSocket subprotocol `tched_router.generation.v1` — protocol-visible constant; migration at cutover.
- Manifest filename `tched-router.json` default — deployment-visible; AuditRoot owns (item 2).
- `server.rs` TLS/default env plumbing incl. TCHED_ROUTER_DEV_SELF_SIGNED / TCHED_ROUTER_BROWSER_TLS_* — naming debt; reserve for cutover.

## JEV escalations / notes on confidence
- CR-MOD tched_catalog_binding/tched_config_*/tched_own_agent_route/tched_blocking — bridge-local (within our-fs namespace per `tched-fs` package). Contract: these are correct; confidence 0.90.
- bare 'tched' occurrences (e.g. concat!("tched","_router") in chat_service.rs:586) detect intentional anti-grep guard; no change; confidence 0.95.
- All DEBT-RR internal (non-WIRE) module/symbol occurrences at confidence ~0.82: supported rename candidates (tched_router_runtime, tched_router_admission, tched_router_generation_native, tched_router_legacy_dispatch, tched_router_integration test, TchedRouter* type names, run_tched_router_server).
- external span: `op_plugins::state_plugins::tched_router::{...}` imports — modules live in op-plugins (cross-crate). NOT touched from this crate; integration owns module renames there; we list the import lines as consuming-call sites.
