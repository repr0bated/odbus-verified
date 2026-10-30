# op-mcp native contextual audit

Task: `native-line-audit-20261024/op-mcp`. Status: complete static audit.

## Native-specific outcome

- Read every inventory entry sequentially through EOF: **27/27 files, 7,973 observed lines**. README includes line139 beyond inventory's138 newline count.
- Root, src, docs, proto, grpc and transport listings reconcile with inventory. No additional files, missing files, binary files, dependency trees or symlinks were exposed by those listings.
- **Zero native occurrences**, including case/compound substrings, in crate code, guidance, documentation, protocols and tests. Explicit zero ledger: occurrences.tsv; exact complete ranges and supplied initial hashes: coverage.tsv.
- No independent native-specific defect, source edit, proposed cross-crate hunk or tched overlap. Existing dirty work untouched. HEAD/current hashes not queried; supplied hashes remain initial receipts rather than freshness guarantees.
- Actual declarative JEV call: three source-context choices, fully recorded in judgments.md. JEV did not certify whole-crate absence. Client-name compact mode is MCP tool exposure, not a demonstrated provider/model override (J1, confidence1).

## Separate generic observations — not native fixes

### G1 — optional gRPC constructor bypasses its admission validator

`src/grpc/server.rs:103-118,130-161,164-213,215-260`; `src/grpc/service.rs:94-101,506-541`.

`new` and `with_infrastructure` fail via `validate_transport_security`; `without_infrastructure` constructs directly, and serving paths do not revalidate. Service mode guard only rejects BlobSchema. Registry call path directly executes a tool. JEV J2: generic_gap confidence0.97. This is a library source inconsistency, **not proof of a deployed listener or unauthorized executable tool**: default without-infrastructure service has no registry, callers were not traced outside assigned scope. Report only; unrelated repair not authorized.

### G2 — historical binary/size documentation

`README.md:33-50,109-132`, `SPEC.md:18-40,66-100`, `compare-op-mcp.md:13-21,33-57,75-77` describe obsolete inventory/binaries. Current Cargo.toml has no bin target; lib.rs explicitly assigns production listener to bridge. JEV J3: generic_drift_report_only confidence0.96. No native-specific wording; leave generic cleanup separate.

### G3 — blob lookup uses filename prefix rather than catalog-pinned contract

`src/blob_schema.rs:121-144,304-317` returns the first matching `<plugin_id>.*.blob`; direct schema/resource lookup shown does not consult the manifest's pinned hash. Multiple retained versions could select a stale contract [INFERENCE]; no such runtime catalog observed. Generic source observation only, not a native finding. No patch or live-state claim.

## Verification and limits

Evidence: complete sequential reads, directory/inventory reconciliation and successful contextual declarative JEV. No searches/extraction/arbitrary process or network execution, recursive delegation, Cargo, build, test, lint, formatting, custom smoke fixture, runtime operation, deployment, seal, grant, credential action, staging or commit. No behavior changed, so no changed behavioral path to smoke. Required later builds remain coordinator-owned if reconciliation introduces a source patch.

Mandatory root guidance and all28 listed skill/reference files read independently. Nested crate AGENTS.md read completely. Shared HANDOFF claim/final entry required by session rules; no shared SIGNALS mutation. No runtime/memory sync invoked because assignment prohibits runtime operations.

## Handoff

Coordinator: reconcile coverage.tsv, explicit zero occurrences.tsv, judgments.md and assignment result.json. No native remediation remains in this crate; G1–G3 are separate unexecuted observations, not blockers to the scoped native audit.

## Self-evaluation

Accuracy4/5: source-grounded and JEV-grounded; runtime/callers outside scope unverified. Completeness4/5: all27 files/occurrences dispositioned; supplied hashes not freshly recomputed. Clarity4/5: native outcome separated from generic observations; receipts retain detail. Actionability4/5: no native patch required; generic risks have precise locations but no runtime reachability claim. Conciseness4/5: reporting detail retained for reconciliation. Overall4.0/5. No broader quality/release certification.
