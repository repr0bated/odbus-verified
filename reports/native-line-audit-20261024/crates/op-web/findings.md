# Findings — op-web native-line audit

## N1 — stale current-UI description

- File: `crates/op-web/build.rs:2-3` (pre-edit)
- Evidence: comment stated, “The current UI is native Rust/egui.” Project authority identifies production UI as the separate SvelteKit/Vite app using json-render Svelte; op-web serves static deployed files and does not embed a UI. Existing project context marks `crates/op-web/ui/` retired.
- JEV: J1 classified the source claim stale (confidence 1.0); J2 found the provided wording correction supported (confidence 0.89, probability .93).
- Disposition: corrected comment to state external Svelte UI/static serving and no native Rust/egui UI; preserved existing Cargo watcher and retired Vite-dist statement. Exact pre-edit file copy: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-web/BEFORE/build.rs`.

## N2 — unsupported implementation and availability claims in orchestrator guidance

- Files: `src/orchestrator/execution.rs:251` and `src/orchestrator/tools.rs:171` (original pre-edit contexts).
- Evidence: both prompts originally described operations as carried out “via native protocols.” Replacing that phrase with “as registered tools” avoided the unsupported backend assertion but still specifically promised D-Bus (NetworkManager) availability. The local registry callsite enumerates dynamic registered tools and returns not found for absent names; built-in tool registration and System/Session D-Bus discovery do not guarantee a specialized NetworkManager tool. The coordinator also reports its op-tools review removed unsupported NetworkManager-specific recommendations.
- JEV: J3 found the original protocol claims insufficient (0.51); J5 supported removing that claim (0.98). Follow-up J7 judged the specific availability claim overbroad and the registry-discovery replacement supported (0.94; probability .96).
- Disposition: `execution.rs` now says registered tools are used for system operations without asserting implementation transport. `tools.rs` now directs the model to `list_tools` and `get_tool_schema` for currently available operations and arguments, then says not to suggest CLI commands. This avoids claiming a dedicated NetworkManager tool or a specific backend. Original exact BEFORE excerpts are `.../BEFORE/orchestrator-execution.txt` and `.../BEFORE/orchestrator-tools.txt`; the superseded first `tools.rs` wording is preserved at `.../BEFORE/orchestrator-tools-current.txt`.

## N3 — ambiguous “native tools” warning

- File: `src/routes/admin.rs:226` (pre-edit).
- Evidence: this is a custom-prompt warning triggered by forbidden CLI spellings; local dispatch resolves registered tools through `ToolRegistry`.
- JEV: J3's initial “inaccurate” disposition was low confidence (0.22); follow-up J4 judged the precise `registered tools` replacement supported (confidence 0.92; probability .95).
- Disposition: warning now says the chatbot should use registered tools instead. Exact affected-hunk BEFORE excerpt: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-web/BEFORE/routes-admin.txt`.

## Retained accurate terminology

- `Cargo.toml:71` `tokio1-native-tls`: accurate TLS backend feature, J1/J2.
- `src/door_proxy.rs:69` `with_native_roots()`: accurate trust-root API terminology, J2.
- `src/orchestrator/parsing.rs:8,99,107`: “native” describes structured provider-supplied tool calls contrasted with text parsing. The full implementation checks `tool_calls` first and then falls back to text parsing; J3 initially found evidence insufficient (0.89), and J6 with exact implementation context judged retain (0.91).

No other native-specific change was indicated across the complete sequential inventory. No tests/build/lint/format/smoke/runtime/deployment verification was run, per task instructions. These are source-only corrections, not runtime or deployment claims.