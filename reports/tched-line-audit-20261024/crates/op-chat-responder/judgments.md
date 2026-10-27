# Declarative JEV naming judgments

Task `tched-line-audit-20261024/op-chat-responder`; 2026-10-24; Codex crate worker.

Installed instructions read at `xd://eval/judge`. Successful call: `await judge(naming_state, {six typed choice questions})` in functions.eval, title `Judging all sixteen naming occurrences`. No routing, model selection, source extraction, filtering or arbitrary execution. Complete raw answer recovered with `read artifact://0:raw:1-1` after display truncation.

## Criteria supplied to every question

- `leave_as_is`: Already-correct intent or protected package/import/path/published identifier; do not rename.
- `replace_with_threetched`: Incorrect naming for external executable/surface; requires threetched.
- `replace_with_3tched`: Incorrect branding for our filesystem/projection; requires 3tched.
- `insufficient_evidence`: Evidence does not establish intent or contract safety.

Authority supplied: our FS uses 3tched-fs / 3tchedFS branding but Cargo package tched-fs, plugin tched_fs and retained directory crates/threetched-fs. External ZeroClaw executable/surface uses threetched; tched_router remains a published plugin identifier. Preserve contracts; mounted access alone does not identify FS branding.

## Exact contextual evidence and questions

### dependency_line16
Source: `crates/op-chat-responder/Cargo.toml:11-19` (entire dependency section supplied).
```toml
[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive", "env"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tched-fs = { path = "../threetched-fs" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
```
Question: Classify both tched occurrences on Cargo.toml line16: package tched-fs and ../threetched-fs. Is retained FS directory spelling misleading branding or protected path?
Decision: leave both; package name and retained source directory are explicitly protected, not product prose.

### imports_lines21_23
Source: `crates/op-chat-responder/src/main.rs:20-34` supplied, including the imports and adjoining dispatch documentation.
```rust
use std::path::Path;
use tched_fs::invocation::Deadline;
use tched_fs::mounted::{GUEST_MOUNT_PATHS, MountedNamespace, declared_mount_index};
use tched_fs::namespace::MountIdentity;
use tracing::{info, warn};
```
Question: Classify each of the three tched_fs Rust import occurrences on main.rs lines21-23. Apply package naming and no cross-crate migration scope.
Decision: leave all three; these resolve the tched-fs dependency through Rust's underscore crate name.

### foundation_docs_lines9_11
Source: `crates/op-chat-responder/src/main.rs:3-14` supplied, including mounted guest workflow and identity-binding context.
```rust
//! ZeroClaw/`threetched` is foundation only: config observation may use
//! `threetched.config.get`. Chat generate must **not** call
//! `threetched.stateless-generate` or any threetched chat/generate CLI.
```
Question: Classify all four threetched occurrences on main.rs lines9-11. Distinguish external ZeroClaw foundation from our FS despite mounted access.
Decision: leave all four; already-qualified external executable/plugin terminology.

### dispatch_docs_lines30_32
Source: `crates/op-chat-responder/src/main.rs:20-34` supplied.
```rust
/// Cross-crate CallMethod the combiner must seal, grant, and guest-mount so
/// this worker can produce a model reply without threetched chat/generate CLI.
/// Dispatch target: `op_llm::chat::ChatManager` (our stack), not
/// `threetched.stateless-generate` and not `tched_router.Chat`.
const OUR_GENERATE_CALLMETHOD: &str =
    "PluginService.CallMethod{plugin_id=\"large_language_model\",method=\"stream_generate\"}";
```
Question: Classify all three occurrences on main.rs lines30,32: threetched, threetched.stateless-generate, tched_router.Chat. They explicitly identify excluded external generation/route, not our FS product.
Decision: leave all three; external branding is correct, and tched_router.Chat names an existing plugin/method contract.

### config_target_line152
Source: `crates/op-chat-responder/src/main.rs:150-163` supplied, including JSON parsing after the mounted call.
```rust
fn config_get(views: &GuestViews, path: &str, turn_id: &str) -> Result<Value> {
    let result = views.call_response(
        "threetched",
        "config.get",
        json!({
            "arguments": [path],
            "--json": true,
        }),
        turn_id,
        Deadline::ordinary(),
    )?;
```
Question: Classify threetched plugin argument on main.rs line152 in config_get. Mounted access does not itself mean our filesystem branding. Preserve exact plugin contract.
Decision: leave; exact external foundation plugin id, not filesystem branding.

### generate_boundary_lines203_219_220
Source: `crates/op-chat-responder/src/main.rs:200-222` supplied, including the complete unavailable-generation hook body.
```rust
/// Our-stack generate hook for a designated response turn.
///
/// When combiner has sealed/granted/guest-mounted [`OUR_GENERATE_CALLMETHOD`]
/// and retargeted it to `op_llm` (not threetched generate CLI), replace the
/// stub body with a mounted `large_language_model.stream_generate` call that
/// returns a done frame. Until then this is an explicit non-ZC stub.
```
Exact error-string context:
```rust
        "our-stack generate unavailable for turn {turn_id} \
         (mode={response_mode}, provider={provider}, model={model}): \
         combiner must wire guest-mounted {OUR_GENERATE_CALLMETHOD} \
         → op_llm::chat::ChatManager; do not use threetched.stateless-generate \
         or any threetched chat/generate CLI"
```
Question: Classify all three threetched occurrences at main.rs lines203,219,220 in hook docs and unavailable-generation error. Are these correct external CLI prohibitions or wrongly named FS branding?
Decision: leave all three; explicit external CLI exclusion, no filesystem misbranding.

## Successful raw answer values

Probability columns: leave_as_is / replace_with_threetched / replace_with_3tched / insufficient_evidence.

| Question | Answer | Confidence | Probabilities |
| --- | --- | --- | --- |
| dependency_line16 | leave_as_is | 0.95 | 0.96 / 0.01 / 0.03 / 0 |
| imports_lines21_23 | leave_as_is | 0.96 | 0.97 / 0 / 0.03 / 0 |
| foundation_docs_lines9_11 | leave_as_is | 0.91 | 0.93 / 0.06 / 0.01 / 0 |
| dispatch_docs_lines30_32 | leave_as_is | 0.99 | 0.99 / 0.01 / 0 / 0 |
| config_target_line152 | leave_as_is | 0.95 | 0.96 / 0.04 / 0 / 0 |
| generate_boundary_lines203_219_220 | leave_as_is | 0.89 | 0.92 / 0.07 / 0.01 / 0 |

JEV is contextual evidence, not authorization or runtime proof. Manual line-by-line review establishes occurrence coverage; this judgment does not certify deployment or generation availability.
