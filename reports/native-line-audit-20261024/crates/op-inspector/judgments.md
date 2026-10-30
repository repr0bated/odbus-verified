# Declarative JEV receipts — op-inspector

2026-10-24; task native-line-audit-20261024/op-inspector. Installed API documentation read at xd://eval/judge. Successful functions.eval JavaScript call titled `Judging inspector source context`, using `await judge(inspectorEvidence, {J1, J2, J3, J4})`; no model override/routing, network client, source extraction or filesystem execution. One shared evidence state, four independent choice questions.

## Evidence supplied

Coverage attestation: full sequential reads of ADAPTER-WORKFLOW.md1-383, Cargo.toml1-24 (23 supplied newlines), SPEC.md1-81, compare-op-inspector.md1-75, src/gcloud.rs1-670, src/introspective_gadget.rs1-1100, src/lib.rs1-43, src/runit.rs1-529. Reader observed zero case/compound native substrings; root/src listings agree. Explicitly stated that this is reader evidence, not an independent JEV scan.

Gadget actual source excerpts (src/introspective_gadget.rs65-175,269-288,782-808,979-1052):
```rust
// Register all built-in parsers
parsers.insert("json".to_string(), std::sync::Arc::new(JsonParser));
parsers.insert("xml".to_string(), std::sync::Arc::new(XmlParser));
parsers.insert("yaml".to_string(), std::sync::Arc::new(YamlParser));
parsers.insert("docker".to_string(), std::sync::Arc::new(DockerParser));
parsers.insert("binary".to_string(), std::sync::Arc::new(BinaryParser));
parsers.insert("text".to_string(), std::sync::Arc::new(TextParser));
parsers.insert("auto".to_string(), std::sync::Arc::new(AutoParser));
let detected_format = self.detect_format(&input).await?;
let parser_opt = self.parsers.read().unwrap().get(&detected_format).cloned();
if let Some(parser) = parser_opt { match parser.parse(&input).await { Ok(result) => results.push(result), Err(e) => errors.push(format!("{} parser failed: {}", detected_format, e)), } }
// Use the best result (most complete schema)
let best_result = results.into_iter().max_by_key(|r| r.schema.complexity_score()).unwrap();
// Try JSON first
if let Ok(result) = JsonParser.parse(input).await { return Ok(result); }
// Try XML
if let Ok(result) = XmlParser.parse(input).await { return Ok(result); }
// Try YAML
if let Ok(result) = YamlParser.parse(input).await { return Ok(result); }
// Fall back to binary
BinaryParser.parse(input).await
```

Adapter actual excerpts (src/gcloud.rs146-194,507-569; src/runit.rs74-112):
```rust
pub async fn get_version(&self) -> Result<String> {
let output = tokio::process::Command::new("gcloud").arg("--version").output().await.context("Failed to run gcloud --version")?;
let version_str = String::from_utf8_lossy(&output.stdout);
Ok(version_str.lines().next().unwrap_or("unknown").to_string())
}
// Run gcloud help for a command path
let mut cmd = tokio::process::Command::new("gcloud");
for part in command_path { cmd.arg(part); }
cmd.arg("--help");
cmd.env("CLOUDSDK_CORE_DISABLE_PROMPTS", "1");
let output = cmd.output().await.context("Failed to run gcloud help")?;
pub fn introspect_source(&self, root: impl AsRef<Path>) -> Result<RunitSchema> {
let root = root.as_ref();
let man = root.join("man");
let sv = std::fs::read_to_string(man.join("sv.8")).with_context(|| format!("read {}", man.join("sv.8").display()))?;
let commands = self.parse_commands(&sv);
```

Drift evidence supplied (SPEC.md19-44,63-69; compare-op-inspector.md23-50; Cargo.toml9-24; src/lib.rs3-13; src/introspective_gadget.rs178-186; src/gcloud.rs455): SPEC lists datadump.rs and op-core; comparison claims datadump present/implemented and lists op-core. Actual directory listings contain lib.rs/gcloud.rs/introspective_gadget.rs/runit.rs only. Actual internal Cargo dependency is op-introspection, no op-core. lib.rs advertises `AI-powered gap filling for incomplete introspections` and `Proxmox LXC template introspection (4500+ editable elements)`; declares gcloud/introspective_gadget/runit. Live Docker method returns `live Docker inspection requires an admitted container projection; provide captured JSON to inspect_object`. Gcloud constructs `positional_args: vec![]`.

## Questions, explicit contrary outcomes and actual responses

J1: Classify actual gadget selection semantics. Is parser detection/fallback/complexity scoring an implicit AI provider/model/config selection override? Do not infer AI execution from documentation branding.
Choices: data_parsers_only (local parser/schema selection); model_override (implicit LLM provider/model/config override); insufficient (cannot distinguish).
Actual: `data_parsers_only`, confidence1; probabilities `{data_parsers_only:1, model_override:0, insufficient:0}`.
Decision: keep code; no AI routing override established.

J2: Does invoking gcloud help/version or parsing source-tree runit man pages establish an implicit native AI routing path? Base on shown actual code; do not equate host subprocesses with LLM selection.
Choices: introspection_only (external CLI/source documentation introspection); routing_override (implicit AI override); insufficient.
Actual: `introspection_only`, confidence1; probabilities `{introspection_only:1, routing_override:0, insufficient:0}`.
Decision: keep code; no native AI routing defect established.

J3: Evaluate documentation accuracy using actual drift evidence. Are datadump/op-core inventory claims accurate? Is this native-specific or generic drift? Do not assume runtime deployment or authorize cleanup.
Choices: generic_drift (contradicts current listing/manifest, non-native); native_defect (native-specific semantic/documentation issue); accurate (matches current evidence); insufficient.
Actual: `generic_drift`, confidence0.99; probabilities `{generic_drift:0.99, native_defect:0.01, accurate:0, insufficient:0}`.
Decision: report G1 separately; no unrelated cleanup.

J4: Given full-file coverage attestation and directory parity, is no-native disposition grounded without fabricating occurrences? Judge did not independently scan; assess evidence sufficiency only. Completion means contextual source audit, not runtime correctness.
Choices: supported_source_only (all inventoried files through EOF, no occurrence reported, no build/test/deploy claim); incomplete (explicit unread lines/unaccounted entries); insufficient.
Actual: `supported_source_only`, confidence0.92; probabilities `{supported_source_only:0.95, incomplete:0.02, insufficient:0.03}`.
Decision: complete bounded native source audit; zero occurrence ledger justified by full reads, not by a scope-only judgment. No runtime conclusion.
