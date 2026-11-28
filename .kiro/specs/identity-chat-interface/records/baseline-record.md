# Baseline_Record — identity-chat-interface Task 1.1

Recorded: 2026-11-09T09:32:41Z (read-only discovery, Req 1.6)
Pinned commit: `154a1b9db3443dc87bc5d078bd70f4b06674c65c` (2026-11-04T03:51:16Z)
Current HEAD at recording: `52a58d3388a22a497a1a3bfa51c7ce0f2f5a82e6` (branch `dev`)
Implementation location (owner correction 2026-11-09): Rust crate in
`crates/` (`op-chat-interface`); only the chat browser interface is
TypeScript/Svelte. Spec Requirement 2.8 supports this; the
`apps/identity-chat/` TypeScript server layout in tasks.md is superseded.
No Credential_Material is recorded; any such value would be replaced by a
field-name marker (Req 1.9).

## Pinned interfaces

| Path (relative to pinned checkout) | Symbol | Signature / shape at pin | sha256 (file at pin) | Committed-or-WT |
|---|---|---|---|---|
| `crates/threetched-fs/src/model.rs` | `MethodDecl` | `pub struct MethodDecl` (line 518) with `required_capability: Option<String>` (526), `side_effect`, `idempotent`, `subid` | `4bc7b17bd49f1c564ba1efb97182421f8b958a339ecc32fb65e42e76a3a241c0` | committed |
| `crates/threetched-fs/src/model.rs` | `SideEffect` | `pub enum SideEffect` (line 532): `Read`, `Mutation` | same file | committed |
| `crates/threetched-fs/src/main.rs` | `Command::Call` / `Describe` / `Invoke` | `Call {` (101), `Invoke {` (124), `Describe {` (139); dispatch at 338/347/366 | `c3992e8d7afebba53f34e80ddc268b9f2d5232c15c9ce0a4096271af03c09e0d` | committed |
| `crates/threetched-fs/src/mounted.rs` | `MountedNamespace::{open, open_for_mode, method_decl, describe, call}`, `terminal_receipt` | `open(path, expected_session)` (39), `open_for_mode` (43), `method_decl` (282), `describe` (292), `call` (394), `fn terminal_receipt` (463) | `2a83fd89057d38da2fec7ced8af159c18eed6857644a3f1a5417bc8d8988cab8` | committed |
| `crates/threetched-fs/README.md` | Operation_File contract | lungs bridge admission; lost-reply recovery via declared `get_method_receipt` | `6de3c8036a4f8bd0437cbd43ef3d84b6d63f00d9109983b3a5637816b7c9241e` | WT at design time |
| `crates/op-state-store/src/method_receipt.rs` | `MethodExecutionReceipt`, `GetMethodReceiptInput` | `GetMethodReceiptInput` (66), `MethodExecutionReceipt` (93), `validate_attribution` (120) | `b6bd7c1b51577e770b0070ff4c812bcdbd253438c9840c8475427f267fa6798e` | committed |
| `crates/op-execution-tracker/src/record.rs` | `ExecutionRecord`, `verify_integrity`, `hash_execution` | `ExecutionRecord` (100), `verify_integrity(&self) -> bool` (231), `hash_execution(tool_name, input, output, prev_hash) -> String` (364) | `f235632b463debbb9efe61bb4d48f38e75c81700984692eff00b0fab55eff8bf` | committed |
| `crates/op-plugins/src/state_plugins/identity_sled.rs` | `identity_sled_schema()`, methods `get_own_identity`, `refresh_credential`, `touch_session`, `record_session_event`, `get_session_history` | schema fn (295); methods at 546/592/672/682/692; schema version `4.7.0` at pin (asserted line 867) | `ecb0e9dd47b17f3ef62f40bb02165c302ef410cc7fe172df397322a293e66823` | WT differs (see B-10) |
| `crates/op-grpc-bridge/src/grpc_server.rs` | Door_8090 routes | `StateSyncServer`, `PluginServiceServer`, `EventChainServiceServer` with gRPC-Web enable | `4568f43e5d7b2063265de63de1c326559134a3195e520f6212775645344530a2` | committed |
| `crates/op-grpc-bridge/src/grpc_web.rs` | Op_Web_Connector / gRPC-Web enable | `enable` (tonic-web `GrpcWebLayer`, OIA1 CORS) | `f7dc1962ed9838ff001ff05b6d7f8c0eafd6ff70d5ee2b40d16d344809634101` | committed |
| `crates/op-web/src/handlers/ui_model.rs` | Op_Web static/gallery surface | Axum UI model handlers | `6c5e44f865ff4327fa9058ff1e385488bb481060c78e206f7392edf2a6e21b7c` | committed |

## Blocker_Records

- **B-09**: `--guest-kernel` is WT-only relative to the pin. At HEAD
  (`crates/_3tched-fs/src/main.rs`, lines 111–152) `guest_kernel: bool`
  exists; not verified present at pinned `main.rs`. Dependent tasks must
  not rely on it as a pinned interface.
- **B-10**: `identity_sled` WT differs from pin (+79/−3 lines vs pinned;
  working tree carries v4.8 `refresh_credential` returning the refreshed
  SID1, comment at line 266; output differs from pinned 4.7.0). The file
  is also modified in the current working tree by another claim.
- **B-01 (updated observation)**: pinned `crates/threetched-fs` directory
  is a naming defect (D-35); at HEAD the crate is `crates/_3tched-fs`.
  The mount-helper catalog defect cannot be re-tested here without
  running `mount-3tchedfs` (mount action, forbidden in Task 1). The
  source fix exists (`deploy/runit/libexec-3tched/mount-3tchedfs` line 35
  `upstream_plugins=threetched`, HANDOFF `host-3tchedfs-threetched`
  shipped in release-d6441566); deployment to the current host state is
  recorded in the Runtime_Record, not asserted here.
- **Naming defects (Task 1.5 partial)**: `chat_router` (threetched-backed,
  renamed from `tched_router` in `71fa232`) and `chat_*` services listed
  in the mounted catalog (`chat_response`, `antigravity_chat`,
  `ctl_plane_chatbot`) are recorded as naming defects, kept in place.
  `ctl_plane_chatbot` coupling to `chat_router` remains a task-20 cut.
