# Declarative JEV receipts — op-mcp

One successful installed `await judge(state, questions)` call through functions.eval (Python), label `Judging exact MCP source contexts`. `xd://eval/judge` instructions read first. No routing extension, model selection, source extraction, execution or delegation. State supplied already-read source excerpts plus explicitly labeled explanatory summaries. No claim that JEV independently scanned all source or established absence.

## State/evidence reference

Full source coverage: coverage.tsv, all 27 entries through EOF (7,973 observed lines). Reader observed zero case/compound native substrings. That absence is full-read evidence, not an excerpt-based JEV verdict.

### E1 — client-name selection

`crates/op-mcp/src/server.rs:245-246,281-301,613-636,739-743`.

```rust
let use_compact = self.config.compact_mode || Self::should_use_compact_mode(client_name);
// handle_tools_list uses the same selector; compact returns get_compact_tools_response.
// Full path calls tool_executor.list_tools, filters blocked names, takes max_tools.
// execute_tool extracts tool_name/arguments and delegates to handle_tools_call.
fn should_use_compact_mode(client_name: &str) -> bool {
    let name_lower = client_name.to_lowercase();
    name_lower.contains("gemini") || name_lower.contains("cursor")
}
```

### E2 — optional gRPC admission

`crates/op-mcp/src/grpc/server.rs:103-118,130-161,164-213`; `src/grpc/service.rs:94-101,506-541`.

Validator rejects non-loopback, rejects TLS flag, and unconditionally rejects remaining server construction pending canonical identity/per-tool admission. `new`/`with_infrastructure` invoke it. Exact alternative constructor:

```rust
pub async fn without_infrastructure() -> Result<Self> {
    let config = GrpcConfig::default().without_infrastructure();
    let service = McpGrpcService::new(config.mode);
    Ok(Self { config, service })
}
```

`serve` constructs McpServiceServer/Server then serves without validating. Service mode guard rejects BlobSchema only. `call_tool` obtains its configured registry and tool and calls `tool.execute(arguments)`; shown path has no principal/capability check. Default constructor above has no registry, so this does NOT establish executable privileged tools or remote/deployed exposure.

### E3 — documentation drift

`README.md:33-50` describes ~350 lines, `./target/debug/op-mcp-server`, and `cargo install --package op-mcp`. Complete current `Cargo.toml:1-58` has no binary declarations, default features empty, library-oriented description. `src/lib.rs:1-5` explicitly states no production listener binary and bridge-owned authenticated frontend. README EOF line139; inventory newline count138 is not a missing line.

## Questions and criteria

### J1 / client_selection

Question: Classify the shown Gemini/Cursor client-name branch. Does it override authoritative AI provider/model/config selection, or choose only MCP tool exposure? Do not infer external behavior not shown.

Criteria: `tool_exposure_only` = only compact/full exposure selected, no model/provider override demonstrated; `selection_override` = shown provider/model/config authority change; `insufficient` = context cannot distinguish.

Result: `tool_exposure_only`; confidence **1.0**. Probabilities: tool_exposure_only1, selection_override0, insufficient0.

Decision: preserve branch; no native occurrence or AI selection override demonstrated.

### J2 / generic_admission

Question: Evaluate exact optional gRPC constructor/dispatch contexts. Is there a source-visible admission-validation gap in without_infrastructure(), distinct from native-specific AI routing? Do not claim deployed or remotely exploitable.

Criteria: `generic_gap` = constructor bypasses validator and serve does not restore it, generic concern; `no_gap` = all paths enforce validator; `native_override` = native-specific model/provider override; `insufficient` = no source conclusion.

Result: `generic_gap`; confidence **0.97**. Probabilities: generic_gap0.98, no_gap0.01, native_override0, insufficient0.01.

Decision: separate generic observation G1; no source edit under native-only assignment. Default without-infrastructure service has no registry; production caller/reachability not established.

### J3 / documentation

Question: Compare README executable/size instructions against current manifest/library statement. Should stale docs be separately reported without expanding zero-native audit into generic cleanup?

Criteria: `generic_drift_report_only` = obsolete binary/tiny-adapter inventory, no native-specific patch; `accurate_current` = matches current scope; `native_fix_required` = native-specific wording/behavior defect established; `insufficient` = cannot assess.

Result: `generic_drift_report_only`; confidence **0.96**. Probabilities: generic_drift_report_only0.97, accurate_current0.02, native_fix_required0, insufficient0.01.

Decision: separate G2; no generic doc rewrite.
