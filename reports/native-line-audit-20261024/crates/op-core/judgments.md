# op-core declarative JEV evidence

Task: `native-line-audit-20261024/op-core`; Codex crate worker; 2026-10-24.

Successful installed `judge(state, questions)` call through Python eval, title `Judging op-core native contexts`. Documentation read: `xd://eval/judge`. One shared evidence state, five independent typed choice questions. No model selection/routing, source filtering, arbitrary execution or filesystem operations through eval. Full returned result was read from `artifact://0:raw:1-1` after display truncation. No judge failure or fallback.

## Shared state and source provenance

Scope supplied: static native-routing audit of op-core only; full sequential review of all 22 inventory files, 5,187 supplied newline-counted lines, directory listings reconciled. `coverage.tsv` records every file. Runtime callers outside this crate are not included. No execution proof is available.

### Native fixture

`crates/op-core/src/identity_projection_transaction_tests.rs:66-114`:

```rust
#[test]
fn uncertain_callback_blocks_identity_but_preserves_unrelated_transactions() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let result =
        commit_identity_projection_durable_in::<()>(root(&dir), gid(), "uncertain", |_| {
            bail!("native commit outcome uncertain")
        })
        .unwrap();
    assert!(matches!(
        result,
        CredentialPublication::Pending { durable: None, .. }
    ));
    unavailable(&dir);
```

Context supplied: `unavailable` asserts strict public/private reads and ordinary identity updates are errors. This test then writes an unrelated plugin, rejects recovery with a wrong operation token, and recovers the exact uncertain token from a callback-provided pair.

### Native comment

`crates/op-core/src/projection_transactions_tests.rs:8-42`:

```rust
// Inject a single native-write failure at the file/manifest boundary. This
// leaves rollback writes operational, exercising the public transaction API.
thread_local! {
    static FAILURE: RefCell<Option<(String, usize, bool)>> = const { RefCell::new(None) };
}
```

Context supplied: `write_fault` compares an injected target filename and countdown, clears its thread-local injection, then panics or returns injected file publication failure. `cfg(test)` calls `write_fault` from transaction writes and directory sync boundaries.

### Environment loader

`crates/op-core/src/config.rs:53-143,168-191`:

```rust
pub const DEFAULT_ENV_FILE: &str = "/etc/op-dbus/environment";
pub const ENV_FILE_PATHS: &[&str] = &["/etc/op-dbus/environment", "/etc/op-dbus.env", ".env"];
pub fn load_environment() -> Option<String> {
    if let Ok(custom_path) = std::env::var("OP_ENV_FILE")
        && let Some(path) = try_load_env_file(&custom_path)
    {
        return Some(path);
    }
    for path in ENV_FILE_PATHS {
        if let Some(loaded_path) = try_load_env_file(path) {
            return Some(loaded_path);
        }
    }
    debug!("No environment file found, using existing environment");
    None
}
```

Assignment guard supplied:

```rust
if std::env::var(&key).is_err() {
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var(&key, &value) };
```

Context supplied: All file keys use the same generic parser and assignment guard. No model/provider/profile names appear in this file. `get_config`/`get_config_opt`/`get_config_bool`/`get_config_int` read caller-supplied keys and defaults.

### Model data declaration

`crates/op-core/src/types.rs:182-194`:

```rust
/// Agent definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub config: HashMap<String, OwnedValue>,
}
```

### Identity gate

`crates/op-core/src/identity_projection_transaction.rs:82-199,223-287`; `crates/op-core/src/projection_transactions.rs:80-98,737-754`:

```rust
ensure!(
    manifest.credential_pending.as_ref().is_some_and(
        |pending| pending.plugin_id == IDENTITY && pending.operation_id == operation_id
    ),
    "identity recovery requires the exact pending operation id"
);
```

Context supplied: Commit writes `credential_pending` and syncs the containing directory before calling durable commit; callback errors retain Pending. Recovery calls only `load_durable()`, never uses possibly mixed old SHM as its authority. Both strict snapshot and strict credential reads call `require_available(plugin_id)`, which errors on matching pending publication. `finish` syncs private and public files before clearing the marker. No provider or model selection occurs. Filesystem helper docs explicitly leave authorization and sealed contract admission to callers.

### Generic truncation risk

`crates/op-core/src/execution.rs:88-96,119-126`:

```rust
pub fn complete(&mut self, output: Option<String>) {
    let now = Utc::now();
    self.ended_at = Some(now);
    self.duration_ms = Some((now - self.started_at).num_milliseconds().max(0) as u64);
    self.status = ExecutionStatus::Completed;
    self.success = true;
    self.output_summary = output.map(|s| truncate_string(&s, 1000));
}
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... (truncated)", &s[..max_len])
    }
}
```

## Questions, explicit criteria, results and dispositions

### `native_usage`

Question: Classify the two supplied native occurrences using the actual fixture and injection context. Do they choose a provider/model/config route, or are they storage failure-test vocabulary?

Criteria:
- `storage_test_vocabulary`: Both native occurrences describe durable-store/file-write fault fixtures only; no model/provider selection.
- `runtime_selection`: At least one occurrence actually selects or overrides a model/provider/config route.
- `insufficient`: Evidence cannot distinguish the alternatives.

Result: `storage_test_vocabulary`, confidence **0.98**. Probabilities: storage_test_vocabulary 0.99, runtime_selection 0.01, insufficient 0.
Decision: preserve both occurrences; neither is a native routing defect.

### `config_selection`

Question: Does this environment-loader evidence show an independent native-specific provider/model/profile override? Distinguish generic configurable loading from a hard-coded model-routing override. Do not infer caller behavior outside the evidence or claim safety for non-Unicode process environment values.

Criteria:
- `generic_loader_no_native_override`: Only generic file priority and defaults; existing valid Unicode environment entries skip assignment; no native-specific selection shown.
- `native_override`: The shown code hard-codes native provider/model/profile selection overriding authoritative selection.
- `insufficient`: Need more evidence to determine whether the shown code implements native-specific selection.

Result: `generic_loader_no_native_override`, confidence **1**. Probabilities: generic_loader_no_native_override 1, native_override 0, insufficient 0.
Decision: preserve configuration loader; no model/provider-specific override found. This is not a general environment-loader security assessment.

### `model_field`

Question: Classify AgentDefinition.model in this excerpt only: does the declaration itself select a model or enforce a provider gate?

Criteria:
- `data_only`: Optional caller-supplied data field; this declaration contains no selection or admission implementation.
- `selection_or_gate`: The declaration itself enforces model/provider selection or routing admission.
- `insufficient`: Excerpt cannot establish either result.

Result: `data_only`, confidence **1**. Probabilities: data_only 1, selection_or_gate 0, insufficient 0.
Decision: preserve generic optional model metadata; no default provider inserted.

### `identity_boundary`

Question: Classify the shown identity publication gate. Does it enforce exact-operation durable recovery rather than act as a native model prerequisite? Do not claim end-to-end caller authorization from filesystem helpers.

Criteria:
- `durable_recovery_gate`: The shown gate blocks uncertain identity state until exact-operation durable recovery; no model-routing prerequisite; upstream authorization remains separate.
- `model_gate_or_bypass`: The shown gate enforces native model choice or bypasses the pending state by reading old credentials.
- `insufficient`: Cannot distinguish recovery gating from model selection with this evidence.

Result: `durable_recovery_gate`, confidence **1**. Probabilities: durable_recovery_gate 1, model_gate_or_bypass 0, insufficient 0.
Decision: retain fail-closed identity recovery boundary; no native routing edit.

### `utf8_risk`

Question: Evaluate this concrete source-only counterexample: output is 999 ASCII 'a' bytes, followed by the two-byte character é. Length is 1001. complete() passes max_len=1000. Is UTF-8 slicing panic possible, and is it generic rather than native-specific? This is a static judgment, not a test execution.

Criteria:
- `generic_panic`: Byte 1000 lies inside é, so &s[..1000] panics; generic execution-summary defect, not native routing.
- `safe_or_native`: The shown slicing is safe for that string or the defect is specifically native routing.
- `insufficient`: Evidence insufficient for the counterexample.

Result: `generic_panic`, confidence **0.95**. Probabilities: generic_panic 0.97, safe_or_native 0.03, insufficient 0.
Decision: record the concrete generic defect for coordinator scope reconciliation; do not repair outside native-only ownership. Not runtime-reproduced or tested.

## Raw successful returned answers

```json
{
  "native_usage": {"type":"choice","choice":"storage_test_vocabulary","confidence":0.98,"probabilities":{"storage_test_vocabulary":0.99,"runtime_selection":0.01,"insufficient":0}},
  "config_selection": {"type":"choice","choice":"generic_loader_no_native_override","confidence":1,"probabilities":{"generic_loader_no_native_override":1,"native_override":0,"insufficient":0}},
  "model_field": {"type":"choice","choice":"data_only","confidence":1,"probabilities":{"data_only":1,"selection_or_gate":0,"insufficient":0}},
  "identity_boundary": {"type":"choice","choice":"durable_recovery_gate","confidence":1,"probabilities":{"durable_recovery_gate":1,"model_gate_or_bypass":0,"insufficient":0}},
  "utf8_risk": {"type":"choice","choice":"generic_panic","confidence":0.95,"probabilities":{"generic_panic":0.97,"safe_or_native":0.03,"insufficient":0}}
}
```

## Every-occurrence clarification supplement

The earlier `native_usage` answer classified two explicit storage uses against
model-routing alternatives. It is not, alone, the final wording/intent review.
[judgments-occurrences.md](judgments-occurrences.md) records the actual successful
installed declarative JEV call **Judging every op-core occurrence**, with four
separately addressed context/intent/accuracy questions and explicit change-needed
and insufficient outcomes. All four returned preserve_accurate (confidence
0.97, 0.92, 0.98, 0.98 respectively). The four-row
[occurrences.tsv](occurrences.tsv) includes two standalone/compound storage uses
and two conservative embedded native matches in Alternative/alternative.
All are dispositioned unchanged; no needed native-specific correction remains.
Full-file coverage remains the retained 22-file sequential review; four
occurrence-bearing files were sequentially reread, not a second whole-crate pass.

