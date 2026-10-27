# Native audit findings — in progress

Date: 2026-10-24. No crate complete. Source review only; no application edits, execution checks, builds or deployment.

## Root / dependency metadata

- `Cargo.toml:188`: “Native, Deterministic Control Plane for Linux Systems” is package-description wording. Full manifest read, lines 1–284. It does not itself impose provider/model/config selection. Preserve it under the explicit instruction to retain legitimate machine vocabulary. JEV-002 below supports this bounded conclusion; it is not proof about runtime callers.
- `crates/bun.lock:1–356`: dependency lock for repomix and its transitive dependencies. Full sequential read; no native-specific routing defect observed. No dependencies fetched or executed. Supply-chain security is not established by reading a lock.
- Manifest/context drift observed: root manifest includes a local jsonschema patch in addition to rovs-openflow; older AGENTS inventory says only rovs-openflow. Source is authoritative for current dependency declarations. No unrelated edit.

## op-agents — incomplete

Files reviewed so far are individually listed in coverage.tsv. Manifest defines three binaries; April comparison documentation lists two. That historical inventory is not deployment evidence; no native-specific repair justified.

`agent_catalog.rs:28–78` derives descriptors from canonical agent types, deduplicates and sorts them. `agent_schema.rs:130–192` derives schemas/categories from operations. These files do not select a native provider/model. Consumer context remains pending, so broad schema-authority compliance is not claimed.

`agent_registry.rs:176–218,241–477` retains a process factory whose `supports` always returns false; in-process factory precedes it. Default specs name the in-process command. No native-specific config override observed. Generic lifecycle issues (Starting records excluded from the Running/Healthy count; unfinished health task; supplied `_config` ignored) are outside the native-specific repair decision and are not claimed as verified runtime failures. No edit.

`agents/advise.rs:1–47` chooses operation names from user query keywords, not model/provider configuration. AI/ML group (ai_engineer, data_engineer, data_scientist, ml_engineer, mlops_engineer, prompt_engineer and mod) read fully. Implementations produce static domain advice; no native provider selection or native-specific contract observed. SecurityProfile construction is preserved; its enforcement still requires remaining call-chain review.

## op-mcp — incomplete

`AGENTS.md:1–43` read fully. Dedicated cognitive work owns concurrent changes. Do not resolve an apparent transient build break or overwrite their dirty work. No implementation coverage yet.

## threetched-fs — incomplete

`generated/plugin_types.rs:1–160` read sequentially. Lines 1–2 explicitly identify generated sealed-schema-derived types as non-authoritative. Initial size 68,429,995 bytes and 121,297 newline count. Bounded reads expose the initial ranges; no tool blocker established. Not excluded, not complete, no edit. Remaining lines start at 161. This is concurrent tched territory; findings touching its logic are report-only.

## Actual JEV judgments

### JEV-001 — scope

Tool: jev_verify, jev-latest. Claim: legitimate native ABI vocabulary and actual billing/native-binding evidence are not automatically defects; blanket renaming and admission removal are prohibited. Evidence: supplied audit prompt, including exact quote “Binary-derived config names or provider billing/native-binding evidence are not automatically defects.” Result: supports; confidence 1; verbatim 0.95; misleading 0.1/3.

### JEV-002 — manifest description

Tool: jev_verify, jev-latest. Claim: root manifest Native wording describes a Linux control-plane package, not a provider/model/config override, and wording alone does not justify a fix. Evidence: exact package section including `description = "OP-DBUS: Native, Deterministic Control Plane for Linux Systems"`, full-file reading record, and audit preservation rule. Result: supports; confidence 0.76; verbatim 0.97; misleading 0.7/3. Moderate confidence: retained bounded source conclusion only; no inference about runtime behavior.
