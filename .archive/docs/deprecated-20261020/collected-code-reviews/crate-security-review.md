---
source: https://miniapps.ai/s/imCxbbqu0uIKAbGqwisOcLalUEqzOD
captured: 2026-09-19
model: GPT-6 Astra Max (GPT 6 Astra High)
status: incomplete
notes: |
  Shared conversation captured for continuation. The original crates.md
  attachment was truncated at 100,000 of 16,453,021 characters, so only
  part of op-agents had source. The review itself was also truncated in
  the shared page during crate 27 (op-plugin-lint). Crates 28–36 and any
  closing cross-crate traces were not present in the share.
---

# Crate-by-Crate Code and Security Review

## 1. Review scope and evidence

### Available evidence

The supplied repository export contains:

*   A directory listing covering **36 crates**.
*   **21 complete source files** under `op-agents/src/agents/`.
*   A partial source file for `op-agents/src/agents/business/payment_integration.rs`.

The visible implementation covers:

*   Six AI/ML agents and their module.
*   Four analysis agents and their module.
*   Three architecture agents and their module.
*   Four complete business agents, their module, and part of the payment integration agent.

### Unavailable evidence

The following were not available as source contents:

*   Workspace configuration and crate manifests.
*   Agent base traits and task/result definitions.
*   Validation helper implementations.
*   Security profiles and sandbox implementation.
*   Agent registries, routers, and transport entry points.
*   All implementation contents for the other 35 crates.
*   Test implementations.

Consequently:

1.  Findings below distinguish **observed local behavior** from **conditional security exposure**.
2.  No repository-wide exploitability claim is made.
3.  No dependency vulnerability assessment was possible.
4.  No build, test, fuzzing, or runtime verification was performed.
5.  Listed files are not assumed to be compiled or reachable merely because they appear in the directory tree.

### Finding classifications

*   **Confirmed behavior:** directly visible in the supplied implementation.
*   **Conditional security risk:** a dangerous local behavior is visible, but exploitability depends on missing validation, authorization, deployment, or isolation code.
*   **Review requirement:** a specific investigation to conduct when missing source becomes available; not a vulnerability finding.

Severity is a provisional remediation priority, not a CVSS score.

### Trace notation

For reviewed source:

```text
A → B → C
```

denotes a call or data flow visible in the supplied code.

For missing source, sections provide an **inspection sequence**, not a verified execution trace.

* * *

# 2. `op-agents`

**Review status:** Partial source review.

**Security boundary not established:** External request admission, task authorization, validation behavior, sandbox enforcement, and caller isolation are unavailable.

## AG-01 — Unsupported operations are reported as successful

**Classification:** Confirmed behavior  
**Severity:** Medium  
**Category:** Contract validation and result integrity

### Affected files

Under `op-agents/src/agents/`:

*   `aiml/ai_engineer.rs`
*   `aiml/data_engineer.rs`
*   `aiml/data_scientist.rs`
*   `aiml/ml_engineer.rs`
*   `aiml/mlops_engineer.rs`
*   `aiml/prompt_engineer.rs`
*   `architecture/frontend_developer.rs`
*   `architecture/graphql_architect.rs`
*   `business/business_analyst.rs`
*   `business/customer_support.rs`
*   `business/hr_pro.rs`
*   `business/legal_advisor.rs`

The payment integration agent’s `execute` implementation is not visible and is excluded from this finding.

### Evidence

These agents check `task.task_type`, but do not dispatch or validate `task.operation`.

Representative implementation:

```rust
if task.task_type != "ai-engineer" {    return Err(format!("Invalid task type: {}", task.task_type));}match self.analyze(task.args.as_deref()) {    Ok(data) => Ok(TaskResult::success(&task.operation, data)),    Err(e) => Ok(TaskResult::failure(&task.operation, e)),}
```

### Verified trace

```text
AgentTrait::execute(task)
  → validate task.task_type
  → analyze(task.args)
  → TaskResult::success(task.operation, generated_recommendations)
```

There is no visible operation allowlist check on this path.

### Failure example

A correctly typed task with:

```text
task_type = "ml-engineer"
operation = "operation_that_does_not_exist"
```

takes the same implementation path as a supported operation and reaches `TaskResult::success`.

### Impact

*   Registry-advertised operations and accepted operations diverge.
*   Misspelled or obsolete operations are not detected.
*   Callers can receive apparent success for work that was never implemented.
*   Workflow orchestration and audit records may attribute results to nonexistent operations.

An upstream validator could prevent this through a particular entry point, but the agent implementation itself does not preserve its advertised contract.

### Remediation

*   Validate the operation in each agent or a mandatory shared execution layer.
*   Prefer a typed operation enum with explicit deserialization failures.
*   Maintain distinct result semantics for advice versus actual execution.
*   Preserve defensive validation at the executor boundary even if transport validation also exists.

The backend architect agent already demonstrates explicit operation matching.

### Required tests

*   Unknown operation produces a structured failure.
*   Empty operation produces a structured failure.
*   Every advertised operation has a defined implementation.
*   Direct trait invocation cannot bypass operation validation.
*   Unsupported operations never produce a successful execution record.

* * *

## AG-02 — Subprocess failure is converted into task success

**Classification:** Confirmed behavior  
**Severity:** Medium  
**Category:** Error handling and security-result integrity

### Affected files

*   `op-agents/src/agents/analysis/code_reviewer.rs`
*   `op-agents/src/agents/analysis/security_auditor.rs`

### Affected methods

Code reviewer:

*   `search_code`
*   `count_lines`
*   `git_diff`
*   `git_log`

Security auditor:

*   `semgrep_scan`
*   `bandit_scan`
*   `cargo_audit`
*   `npm_audit`

### Evidence

The subprocess wrappers retrieve output but never inspect its exit status:

```rust
let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;let stdout = String::from_utf8_lossy(&output.stdout);let stderr = String::from_utf8_lossy(&output.stderr);Ok(format!("Cargo audit:\n{}\n{}", stdout, stderr))
```

The caller then wraps the returned string in `TaskResult::success`.

### Verified trace

```text
execute(task)
  → selected subprocess wrapper
  → Command::output()
  → subprocess starts successfully
  → subprocess terminates with any exit status
  → wrapper returns Ok(formatted_output)
  → TaskResult::success(...)
```

A failure to start is handled. A failure reported by the started process is not.

### Impact

Examples include:

*   Invalid Git arguments being presented as successful execution.
*   Scanner configuration failures being presented as successful scans.
*   Network or registry failures being indistinguishable from completed audits.
*   Machine consumers being forced to infer state from unstructured output.

For security scanners, “vulnerabilities found” and “scanner failed” are separate outcomes and must not be collapsed into one generic boolean.

### Remediation

Return a structured subprocess result:

```text
process_started
exit_code
termination_signal
execution_status
scan_status
stdout
stderr
```

Apply tool-specific exit-code interpretation:

*   `rg`: distinguish matches, no matches, and execution error.
*   Audit tools: distinguish completed scan with findings from failed scan.
*   Git and line-count tools: distinguish execution failure from successful output.

### Required tests

*   Missing executable.
*   Invalid arguments.
*   Scanner reports vulnerabilities.
*   Scanner reports configuration or network failure.
*   Process terminates by signal.
*   `rg` returns no matches.

* * *

## AG-03 — Blocking subprocess and filesystem operations execute inside async methods

**Classification:** Confirmed behavior  
**Severity:** Medium  
**Category:** Availability and async runtime behavior

### Affected files

*   `analysis/code_reviewer.rs`
*   `analysis/security_auditor.rs`
*   `analysis/debugger.rs`
*   `analysis/performance.rs`

Paths are relative to `op-agents/src/agents/`.

### Evidence

The async `execute` methods invoke synchronous helpers that use:

```rust
std::process::Command::output()std::fs::read_to_string(...)std::fs::read_dir(...)
```

There is no visible asynchronous subprocess API or blocking-task offload.

### Verified traces

```text
CodeReviewerAgent::execute().await
  → git_diff()
  → std::process::Command::output()
  → runtime thread waits for child completion
```

```text
DebuggerAgent::execute().await
  → read_logs()
  → std::fs::read_to_string()
  → runtime thread waits for filesystem operation
```

### Impact

If these methods run on an async executor:

*   Slow scans occupy executor threads.
*   Large file reads delay unrelated work.
*   Concurrent tasks can reduce service responsiveness.
*   Network-backed filesystems or stalled tools amplify the impact.

The actual blast radius depends on the unavailable runtime and dispatcher configuration.

### Remediation

*   Use `tokio::process::Command` for subprocess execution where Tokio is the runtime.
*   Use bounded blocking workers for synchronous filesystem operations.
*   Add per-agent and global concurrency limits.
*   Ensure cancellation and timeout logic also terminates subprocess work; merely moving work to a blocking pool is insufficient.

### Required tests

*   A deliberately slow scan does not block an unrelated health request.
*   Excess concurrency is queued or rejected within defined limits.
*   Cancellation does not leave untracked child work running.
*   Blocking filesystem workloads do not exhaust the async worker pool.

* * *

## AG-04 — No local subprocess timeout or output budget

**Classification:** Confirmed local absence; conditional service-level risk  
**Severity:** Medium  
**Category:** Resource exhaustion

### Affected files

*   `analysis/code_reviewer.rs`
*   `analysis/security_auditor.rs`

### Evidence

All visible subprocess wrappers call:

```rust
cmd.output()
```

No deadline, output cap, or child-process termination policy is visible.

### Verified trace

```text
task arguments and path
  → command construction
  → Command::output()
  → wait for process termination
  → accumulate stdout and stderr
  → allocate formatted result string
```

### Impact

*   Recursive searches and scans can generate large outputs.
*   Git diffs can be arbitrarily large.
*   Scanners can wait on remote services.
*   A subprocess can remain running indefinitely.
*   Captured output and subsequent string formatting increase memory pressure.

External limits may exist, but they are not visible. An async request timeout alone would not reliably terminate this blocking subprocess path.

### Remediation

Introduce one mandatory subprocess runner that enforces:

*   Wall-clock deadline.
*   Maximum stdout and stderr sizes.
*   Concurrent child-process limits.
*   Cancellation propagation.
*   Process-group or equivalent descendant cleanup.
*   Structured truncation metadata.
*   Controlled environment and executable selection.

### Required tests

*   Infinite or very long-running child.
*   Child emitting output beyond the configured cap.
*   Child spawning descendants.
*   Caller disconnect or cancellation.
*   Deadline expiry leaves no running descendants.
*   Truncated output is explicitly marked.

* * *

## AG-05 — Missing paths bypass local path validation and inherit the process working directory

**Classification:** Confirmed behavior; conditional security risk  
**Severity:** Medium  
**Category:** Filesystem scope and ambient authority

### Affected files

*   `analysis/code_reviewer.rs`
*   `analysis/security_auditor.rs`

### Evidence

Path validation only occurs when a caller supplies a path:

```rust
if let Some(dir) = path {    let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;    cmd.current_dir(validated_path);}
```

Some scanners explicitly use `"."` when the path is absent:

```rust
} else {    cmd.arg(".");}
```

### Verified traces

```text
task.path = None
  → no validate_path call
  → git command inherits service working directory
```

```text
task.path = None
  → semgrep or bandit receives "."
  → scan is rooted in inherited working directory
```

```text
task.path = None
  → rg receives no explicit search directory
  → scope depends on inherited working directory
```

### Impact

The local `ALLOWED_DIRS` check does not constrain the default-path case.

If the service starts in an unintended directory, users may:

*   Read repository contents or history outside the intended workspace.
*   Scan service installation files.
*   Obtain output reflecting another workload’s working directory.

This is not a demonstrated sandbox escape; working-directory configuration and outer isolation are missing.

### Remediation

*   Require an explicit authorized workspace identifier.
*   Resolve missing paths to a configured workspace root, not process CWD.
*   Apply the same validation to defaults and explicit paths.
*   Bind workspace authorization to the caller.
*   Avoid using broad filesystem prefixes as the only tenant boundary.

### Required tests

*   Service starts outside the allowed workspace; omitted path is rejected or safely resolved.
*   Relative paths are resolved deterministically.
*   Two users cannot access each other’s workspaces through omitted paths.
*   Default and explicit paths undergo identical authorization.

* * *

## AG-06 — Caller-controlled `rg` patterns occupy an option-parsing position

**Classification:** Confirmed argument construction; exploitability conditional  
**Severity:** Medium, pending validator review  
**Category:** Argument injection

### Affected file

`op-agents/src/agents/analysis/code_reviewer.rs`

### Evidence

```rust
validation::validate_args(p)?;cmd.arg(p);
```

The pattern is not passed through `-e` or `--regexp`.

### Verified trace

```text
task.args
  → validate_args(pattern)
  → Command::new("rg").arg(pattern)
  → rg argument parser
```

### Security interpretation

Using `Command` avoids shell interpolation, but it does not prevent the invoked program from interpreting an argument as an option.

Whether option-like values reach `rg` depends on the missing `validate_args` implementation.

A literal search expression beginning with `-` may therefore be misinterpreted or rejected. Some `rg` options can substantially change execution behavior, making argument placement a security concern rather than merely a usability issue.

### Remediation

Construct an explicit argument boundary:

```rust
cmd.arg("--no-heading")    .arg("-n")    .arg("-e")    .arg(pattern)    .arg("--")    .arg(validated_workspace);
```

Additionally:

*   Require a validated workspace.
*   Disable configuration loading where appropriate.
*   Define symlink and hidden-file behavior explicitly.
*   Apply the common subprocess resource policy.

### Required tests

*   Literal patterns beginning with `-` or `--`.
*   Patterns containing whitespace.
*   Option-like patterns are searched as text.
*   No configuration or preprocessing behavior can be enabled through the pattern field.

* * *

## AG-07 — Git review operations permit ambient configuration and free-form diff flags

**Classification:** Confirmed command construction; conditional security risk  
**Severity:** Medium, pending trust-boundary review  
**Category:** Execution policy and repository-controlled configuration

### Affected file

`op-agents/src/agents/analysis/code_reviewer.rs`

### Evidence

```rust
cmd.arg("diff");if let Some(a) = args {    validation::validate_args(a)?;    for arg in a.split_whitespace() {        cmd.arg(arg);    }}
```

The command inherits its environment and, for a supplied path, runs inside that repository.

### Verified trace

```text
task.args
  → validate_args
  → split_whitespace
  → git diff arguments
```

```text
task.path
  → validate_path
  → current_dir(repository)
  → Git reads applicable configuration and attributes
```

### Impact

Two distinct issues require attention:

1.  **Free-form option surface:** The method exposes more Git behavior than a narrow diff API needs.
2.  **Ambient Git behavior:** Depending on options and repository configuration, Git diff features can invoke external helpers or perform text conversion.

The visible code does not prove arbitrary command execution. Confirmation requires the validator, sandbox policy, Git configuration, environment, and workspace trust model.

### Remediation

*   Replace a whitespace-split argument string with a typed diff request.
*   Allowlist revisions, paths, and a small set of presentation options.
*   Disable external diff and text conversion when reviewing untrusted repositories.
*   Control relevant environment and global/system configuration.
*   Execute with minimal filesystem and network permissions.

### Required tests

*   Untrusted repository with configured diff helpers.
*   Repository attributes requesting text conversion.
*   Unsupported diff flags.
*   Revision/path ambiguity.
*   Paths containing whitespace.
*   Controlled environment remains effective across execution paths.

* * *

**Classification:** Confirmed behavior  
**Severity:** Medium  
**Category:** Availability and memory usage

### Affected file

`op-agents/src/agents/analysis/debugger.rs`

### Affected methods

*   `read_logs`
*   `journalctl`

### Evidence

```rust
let content = fs::read_to_string(&validated_path)?;let collected: Vec<&str> = content.lines().collect();let start = collected.len().saturating_sub(num_lines);
```

The fallback journal method repeats the same full-file strategy for several system logs.

### Verified trace

```text
logs request
  → validate path
  → read entire file into String
  → collect reference to every line
  → select requested suffix
  → allocate joined output
```

### Impact

Requesting the last ten lines of a multi-gigabyte log still requires processing the entire file.

Additional risks include:

*   No visible maximum requested line count.
*   No maximum response byte count.
*   Very long individual lines.
*   Blocking or special-file behavior, depending on missing path validation.
*   High memory use across concurrent requests.

### Remediation

*   Implement bounded reverse reading for regular files.
*   Enforce both a line cap and a byte cap.
*   Establish an explicit policy for special files, pipes, and devices.
*   Avoid indexing every line.
*   Report truncation and read errors explicitly.
*   Couple file opening to the authorized path boundary to reduce path-race risk.

### Required tests

*   Large file with a small requested tail.
*   One extremely long line.
*   Excessive requested line count.
*   Non-UTF-8 content.
*   File rotation during reading.
*   Special-file input is rejected or handled under a strict deadline.

* * *

## AG-09 — PID filtering is substring matching across process metadata

**Classification:** Confirmed behavior  
**Severity:** Low; potentially higher if callers rely on PID-scoped access  
**Category:** Input semantics and information disclosure

### Affected file

`op-agents/src/agents/analysis/debugger.rs`

### Evidence

```rust
let entry_line = format!("{} {} {}", name_str, proc_name, ...);if let Some(p) = pid {    if entry_line.contains(p) {        entries.push(entry_line);    }}
```

### Verified trace

```text
task.path used as pid parameter
  → validate_args
  → enumerate numeric /proc entries
  → read status and cmdline
  → construct "PID NAME CMDLINE"
  → substring filter over entire string
```

### Impact

A request for PID `12` can match:

*   PID `12`.
*   PID `312`.
*   A process name containing `12`.
*   A command-line argument containing `12`.

The method reads other processes’ metadata before filtering.

If the operation is intended to inspect one authorized process, this does not implement that contract.

### Remediation

*   Use a dedicated typed PID field.
*   Parse and validate an integer.
*   Read `/proc/<pid>/...` directly for a PID lookup.
*   Expose process search as a separate operation.
*   Apply principal-specific authorization before reading metadata.

### Required tests

*   PID `12` does not return PID `312`.
*   A matching command-line argument does not match a different PID.
*   Invalid PID input is rejected.
*   Process exit during inspection returns a distinct outcome.

* * *

## AG-10 — Diagnostic results include raw log data and process command lines

**Classification:** Confirmed data flow; authorization exposure unverified  
**Severity:** Medium, conditional  
**Category:** Sensitive-data handling

### Affected file

`op-agents/src/agents/analysis/debugger.rs`

### Evidence

`read_logs` returns selected file contents without visible redaction.

The fallback journal reader includes:

```text
/var/log/syslog
/var/log/messages
/var/log/kern.log
/var/log/auth.log
```

`process_info` returns contents derived from `/proc/<pid>/cmdline`.

### Verified traces

```text
readable log file
  → selected lines
  → formatted output
  → TaskResult::success
```

```text
/proc/<pid>/cmdline
  → replace NUL separators with spaces
  → formatted process entry
  → TaskResult::success
```

### Impact

Logs and process command lines can contain:

*   Authentication tokens.
*   Connection strings.
*   Personal data.
*   Internal infrastructure details.
*   Credentials accidentally supplied as command-line arguments.

No visible principal-scoped authorization or redaction occurs inside these methods. Such enforcement may exist elsewhere and must be traced before classifying this as unauthorized disclosure.

### Remediation

*   Treat log and process inspection as privileged capabilities.
*   Restrict access by workspace, user, service, or process ownership.
*   Redact known credential fields where feasible.
*   Prevent sensitive results from being automatically persisted to chat memory or telemetry.
*   Document which diagnostic operations can expose host-wide data.

### Required tests

*   Unauthorized callers cannot inspect host authentication logs.
*   One tenant cannot read another tenant’s process arguments.
*   Secret-like values are handled according to policy.
*   Downstream logging and memory systems do not retain sensitive results unintentionally.

* * *

## AG-11 — Journal fallback does not preserve unit-query semantics

**Classification:** Confirmed behavior  
**Severity:** Low  
**Category:** Diagnostic correctness

### Affected file

`op-agents/src/agents/analysis/debugger.rs`

### Evidence

The method:

1.  Reads several conventional log files.
2.  Takes the final 100 lines of each.
3.  Concatenates the results.
4.  Applies `line.contains(unit)`.

### Verified trace

```text
unit filter
  → validate_args
  → read fallback log files
  → tail each file to 100 lines
  → concatenate
  → substring-filter by unit
```

### Impact

*   A relevant unit entry outside a file’s final 100 lines is omitted.
*   A substring match is not equivalent to service-unit identity.
*   Lines from unrelated services can match.
*   “No matching output” does not establish that the service produced no logs.

This can materially mislead incident diagnosis.

### Remediation

*   Name the operation according to its actual behavior, such as `search_system_log_tail`.
*   Return source provenance and matching semantics.
*   Use structured service attribution where the logging backend supports it.
*   Distinguish unsupported journal access from a successful journal query.
*   State the time or byte window searched.

### Required tests

*   Matching event outside the unfiltered final 100 lines.
*   Two service names sharing a substring.
*   No readable files.
*   Readable files but no matches.
*   Output includes source and limitation metadata.

* * *

**Classification:** Confirmed behavior  
**Severity:** Low  
**Category:** Error masking

### Affected file

`op-agents/src/agents/analysis/debugger.rs`

### Evidence

```rust
let status = fs::read_to_string(&status_path).unwrap_or_default();let cmdline = fs::read_to_string(&cmdline_path)    .unwrap_or_default()    .replace('\0', " ");
```

An empty command line is then rendered as:

```rust
"[kernel]"
```

### Verified trace

```text
/proc file read fails
  → unwrap_or_default()
  → empty string
  → unknown name and/or "[kernel]"
  → successful process listing
```

### Impact

A permission failure, process-exit race, or other read error can be presented as a kernel process or ordinary missing metadata.

This reduces the reliability of troubleshooting and hides access-control restrictions from callers.

### Remediation

*   Distinguish missing process, permission denied, empty command line, and successful read.
*   Return per-process partial-result metadata.
*   Do not infer kernel-thread status solely from an empty command line.
*   Avoid failing the entire listing for expected per-process races.

### Required tests

*   Permission denied.
*   Process disappears between enumeration and read.
*   Legitimately empty command line.
*   Partial status availability.

* * *

## AG-13 — Serialization errors are silently replaced with empty successful output

**Classification:** Confirmed behavior  
**Severity:** Low  
**Category:** Error handling

### Affected implementation

Visible recommendation-producing agents, including the visible `PaymentIntegrationAgent::analyze` method, use:

```rust
Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
```

### Verified trace

```text
construct JSON value
  → serialize
  → on serialization failure, return empty String
  → helper returns Ok("")
```

For complete visible executors using these helpers:

```text
Ok("")
  → TaskResult::success(...)
```

### Impact

An internal serialization failure becomes indistinguishable from an intentionally empty successful result.

For the currently constructed JSON values, such failures may be unlikely. The error-handling contract is nevertheless incorrect and fragile under later changes.

### Remediation

Propagate the error:

```rust
simd_json::to_string_pretty(&result)    .map_err(|e| format!("Failed to serialize agent result: {e}"))
```

Prefer a structured result payload rather than encoding JSON inside a string, if the missing `TaskResult` API permits it.

### Required tests

*   Serialization failures are surfaced through a structured failure path.
*   Empty successful output is either prohibited or explicitly meaningful.
*   Result consumers do not need undocumented double deserialization.

* * *

**Classification:** Confirmed behavior  
**Severity:** Medium for workflow integrity; not an execution vulnerability  
**Category:** Capability truthfulness and orchestration contracts

### Affected examples

*   `MLEngineerAgent`: `train_model`, `deploy`
*   `DataScientistAgent`: `build_model`, `run_experiment`
*   `CustomerSupportAgent`: `handle_ticket`, `escalate`
*   `FrontendDeveloperAgent`: `build_component`

### Evidence

The visible implementations inspect strings:

```rust
if input.contains("training") || input.contains("model") {    recommendations.push(...);}
```

They return static recommendations rather than performing the advertised action.

### Verified trace

```text
task.operation = "train_model"
  → analyze(task.args)
  → keyword checks
  → static recommendation JSON
  → TaskResult::success("train_model", advice)
```

### Impact

The implementation may be suitable as an advisory persona, but its operation names and success signaling can mislead an orchestrator into believing an action occurred.

Potential consequences:

*   A workflow advances without its required artifact.
*   An audit trail records success for an action that was not executed.
*   A user believes a model was deployed or a ticket escalated.
*   Security checks are confused with general recommendations.

### Remediation

*   Rename advisory operations to explicit advisory names.
*   Include fields such as `execution_mode`, `performed_actions`, and `artifacts`.
*   Reserve execution success for verifiable execution outcomes.
*   Ensure registries distinguish personas, advisors, and executable tools.
*   Require downstream workflows to validate artifacts, not merely success flags.

### Required tests

*   Advisory agents cannot satisfy execution-only workflow steps.
*   No training/deployment action is recorded without corresponding evidence.
*   UI labels accurately describe advisory output.
*   Registry metadata agrees with implementation capability.

* * *

## AG-15 — Task-type validation is inconsistent across agent implementations

**Classification:** Confirmed behavior  
**Severity:** Low; conditional routing-integrity concern  
**Category:** Dispatch validation

### Affected files

The four visible analysis agents do not perform the task-type checks present in the AI/ML and architecture agents.

### Verified trace

```text
analysis agent execute(task)
  → dispatch solely on task.operation
  → run operation
```

By contrast:

```text
AI/ML agent execute(task)
  → reject mismatched task.task_type
  → continue
```

### Impact

If a caller invokes an analysis agent with a task intended for another agent, a matching operation name can still execute.

This becomes a security issue only if a missing dispatcher or authorization layer binds permissions to the declared task type rather than the selected executor.

### Remediation

*   Establish one consistent validation policy.
*   Bind authorization to the resolved executor and operation.
*   Reject mismatched task types at the executor boundary.
*   Prefer a resolved task representation that cannot contain contradictory routing metadata.

### Required tests

*   Mismatched task type is rejected by every executor.
*   Authorization follows the actual agent and operation.
*   Registry aliases cannot produce a permission mismatch.

* * *

## `op-agents` — Unresolved security gates

These are not established vulnerabilities.

### A. Path validation

Inspect:

*   `src/agents/base.rs`
*   `src/security/validation.rs`

Determine whether validation handles:

*   Path-component boundaries.
*   `..` traversal.
*   Symlinks.
*   Nonexistent paths.
*   Relative paths.
*   File replacement between validation and access.
*   Cross-tenant access within `/home`, `/tmp`, or `/opt`.

A permitted filesystem prefix is not, by itself, caller authorization.

### B. Security-profile enforcement

Inspect:

*   `src/security/profiles.rs`
*   `src/security/sandbox.rs`
*   `src/router.rs`
*   `src/agent_registry.rs`
*   `src/dbus_service.rs`

The visible agents return a `SecurityProfile`, but their helpers directly call filesystem APIs and subprocess APIs. Determine whether a mandatory outer layer enforces the profile before these calls.

### C. Legacy and unified execution paths

Inspect:

*   `src/unified/adapter.rs`
*   `src/unified/agent_trait.rs`
*   `src/unified/registry.rs`
*   `src/unified/execution/`
*   `src/dynamic/`

Verify that adapters and dynamic registration cannot bypass validation, authorization, sandboxing, or result semantics.

### D. Executable and environment control

Determine how child executables are selected. The visible wrappers use names such as `"git"`, `"rg"`, and `"cargo"`, which rely on inherited executable search behavior.

Verify controlled `PATH`, environment, working directory, and repository configuration.

* * *

# 3. `op-assistant-grpc`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/auth.rs`
*   `src/server.rs`
*   `src/transport.rs`
*   `src/convert.rs`
*   `src/agents.rs`
*   `src/tasks.rs`
*   `src/sessions.rs`
*   `src/namespace.rs`
*   `src/memory.rs`
*   `src/incus.rs`
*   Relevant `.proto` definitions.

### Inspection sequence — not a verified trace

```text
RPC declaration
  → service registration
  → authentication middleware
  → handler
  → resource-ownership check
  → downstream client or state access
```

### Security review requirements

1.  Verify authentication coverage for every RPC, including streaming methods and alternate transports.
2.  Check whether session, task, namespace, and memory identifiers are authorized against the caller.
3.  Trace authenticated identity through downstream cognitive and container operations.
4.  Review maximum message size, pagination, concurrency, deadlines, and stream lifetime.
5.  Inspect protobuf conversion for integer narrowing, empty identifiers, enum defaults, and optional-field ambiguity.
6.  Ensure internal service credentials are not exposed through errors or response metadata.

### Required negative tests

*   Cross-user task and session access.
*   Namespace enumeration by an unrelated principal.
*   Expired credentials on long-lived streams.
*   Oversized messages and excessive concurrent requests.
*   Cancellation during downstream mutation.

**Finding disposition:** No implementation-backed findings can be established from the available material.

* * *

# 4. `op-blob`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/blob.rs`
*   `src/descriptor.rs`
*   `src/identity.rs`
*   `src/subid.rs`
*   `src/schema.rs`
*   `src/catalog.rs`
*   `src/adapter.rs`
*   `src/btrfs.rs`
*   `src/json_schema_proto.rs`
*   Listed contract tests.

### Inspection sequence — not a verified trace

```text
external descriptor or schema
  → parsing and canonicalization
  → identity derivation
  → descriptor validation
  → catalog admission
  → storage or publication
```

### Security review requirements

1.  Identify the authoritative representation for descriptor identity.
2.  Check canonicalization across JSON, protobuf, and stored forms.
3.  Verify duplicate keys, unknown fields, typed maps, and integer boundaries.
4.  Determine whether “sealed” descriptors are immutable in every update path.
5.  Inspect traversal, symlink, and subvolume ownership handling in Btrfs operations.
6.  Bound schema depth, recursion, collection sizes, and generated descriptor size.

### Required negative tests

*   Equivalent representations produce the intended identity behavior.
*   Modified sealed fields are rejected.
*   Duplicate or unexpected fields fail according to the contract.
*   Deeply nested schemas terminate within resource limits.
*   Storage paths cannot escape the authorized root.

**Finding disposition:** Contract-oriented test filenames suggest areas to inspect, but provide no evidence that their assertions are sufficient or passing.

* * *

# 5. `op-cache`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/grpc/`
*   `src/capability_resolver.rs`
*   `src/workflow_executor.rs`
*   `src/workflow_cache.rs`
*   `src/workstack_cache.rs`
*   `src/btrfs_cache.rs`
*   `src/snapshot_manager.rs`
*   `src/agent_registry.rs`

### Inspection sequence — not a verified trace

```text
cache or workflow RPC
  → caller identity
  → capability resolution
  → cache-key construction
  → lookup or execution
  → snapshot/result storage
```

### Security review requirements

1.  Verify cache keys include required tenant, identity, policy, model, tool, and version dimensions.
2.  Check that cached authorization decisions cannot survive permission revocation incorrectly.
3.  Distinguish reusable computation from side-effectful workflow execution.
4.  Inspect cache poisoning through caller-controlled metadata or incomplete keys.
5.  Review snapshot creation and deletion against path and ownership boundaries.
6.  Bound cache growth, workflow depth, execution concurrency, and eviction work.

### Required negative tests

*   Same input from different tenants cannot retrieve private cached output.
*   Policy changes invalidate security-sensitive cache entries.
*   Replaying a cached workflow does not repeat external mutations unintentionally.
*   Corrupt snapshots fail safely.

**Finding disposition:** No verified findings.

* * *

# 6. `op-chat`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/chat_service.rs`
*   `src/actor.rs`
*   `src/tool_executor.rs`
*   `src/forced_execution.rs`
*   `src/forced_tool_pipeline.rs`
*   `src/memory_loop.rs`
*   `src/session.rs`
*   `src/nl_admin.rs`
*   `src/orchestration/workstack_executor.rs`

### Inspection sequence — not a verified trace

```text
user message
  → session binding
  → model request
  → proposed tool call
  → policy decision
  → tool execution
  → response and memory persistence
```

### Security review requirements

1.  Ensure model output is treated as untrusted tool input.
2.  Verify authorization occurs at execution time, not solely during prompt construction or tool listing.
3.  Inspect all execution paths, including the files named `forced_*`, for equivalent policy enforcement. Their names alone do not establish a bypass.
4.  Separate privileged administrative actions from ordinary conversational intent.
5.  Trace memory writes for tenant isolation, retention, and secret handling.
6.  Check workflow fan-out, recursive tool calls, token limits, and cancellation.

### Required negative tests

*   Retrieved content asks the model to invoke a privileged tool.
*   A hidden or unlisted tool is requested directly.
*   Tool permissions change during a conversation.
*   An advisory agent result is incorrectly used as proof of completed work.
*   One session attempts to reference another session’s memory.

**Finding disposition:** No verified findings.

* * *

# 7. `op-cognitive-mcp`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/server.rs`
*   `src/interceptor.rs`
*   `src/grpc_service.rs`
*   `src/dbus_interface.rs`
*   `src/typed_tools.rs`
*   `src/tool_profiles.rs`
*   `src/memory_store.rs`
*   `src/rag_pipeline.rs`
*   `src/qdrant_shuttle.rs`
*   `src/cozo_shuttle.rs`
*   `src/quota.rs`
*   `src/gemini_fallback.rs`
*   `src/voyage.rs`

### Inspection sequence — not a verified trace

```text
MCP/gRPC/D-Bus request
  → authentication
  → profile and quota selection
  → tool dispatch
  → memory/vector query or provider call
  → result publication
```

### Security review requirements

1.  Compare policy enforcement across all exposed protocols.
2.  Verify tenant filters cannot be overridden by caller-supplied vector or memory query fields.
3.  Treat retrieved material as data, not privileged instructions.
4.  Examine what data is transmitted to embedding and fallback model providers.
5.  Inspect endpoint configuration for SSRF and secret leakage.
6.  Verify quotas are atomic under concurrent calls and cover expensive downstream operations.
7.  Confirm diagnostic/admin tooling cannot expose credentials or unrestricted state.

### Required negative tests

*   Cross-tenant vector retrieval.
*   Fallback provider receives data disallowed by policy.
*   Concurrent quota exhaustion.
*   Unauthorized direct calls through a secondary protocol.
*   Malicious retrieved document attempts tool escalation.

**Finding disposition:** No verified findings.

* * *

# 8. `op-compliance`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/lib.rs`
*   `Cargo.toml`
*   Call sites that consume compliance decisions.

### Inspection sequence — not a verified trace

```text
compliance request
  → policy selection
  → evidence evaluation
  → decision construction
  → caller interpretation
```

### Security review requirements

1.  Determine whether this crate provides advisory output or enforceable policy decisions.
2.  Verify unsupported controls are represented as unknown or unevaluated, not passing.
3.  Identify evidence provenance, freshness, and integrity requirements.
4.  Check whether missing evidence causes a fail-open result.
5.  Review the distinction between compliance reporting and security authorization.

### Required negative tests

*   Unsupported policy.
*   Missing or stale evidence.
*   Malformed control identifier.
*   Partial evaluation.
*   Downstream caller cannot interpret “not evaluated” as “compliant.”

**Finding disposition:** No verified findings; even the crate’s effective runtime responsibility is unconfirmed without source.

* * *

# 9. `op-core`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/security.rs`
*   `src/self_identity.rs`
*   `src/projection_shm.rs`
*   `src/projection_transactions.rs`
*   `src/identity_projection_transaction.rs`
*   `src/private_document_publication.rs`
*   `src/private_projection_directory.rs`
*   `src/state_publisher.rs`
*   Transaction and publication tests.

### Inspection sequence — not a verified trace

```text
identity or document mutation
  → transaction preparation
  → authorization and validation
  → durable or shared-memory update
  → publication
  → reader observation
```

### Security review requirements

1.  Verify public projections cannot contain private fields.
2.  Check file permissions, ownership, and shared-memory access controls.
3.  Examine publication atomicity and reader consistency.
4.  Review crash recovery between identity updates and projection updates.
5.  Inspect symlink handling and atomic replacement in private directories.
6.  Establish whether readers validate generation, length, and integrity metadata.

### Required negative tests

*   Crash at each publication step.
*   Concurrent readers during replacement.
*   Private field accidentally added to a serializable structure.
*   Attacker-controlled symlink in a projection directory.
*   Stale shared-memory generation after identity change.

**Finding disposition:** No verified findings.

* * *

# 10. `op-cozo-store`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/lib.rs`
*   `src/identity_transactions.rs`
*   `src/runtime_document_transactions.rs`
*   `src/ctl_plane_chatbot_episodes.rs`
*   `src/bin/identity_sled_bind.rs`
*   Transaction tests.

### Inspection sequence — not a verified trace

```text
store API request
  → query or transaction construction
  → identity/document mutation
  → commit
  → returned state or publication trigger
```

### Security review requirements

1.  Check query parameterization and any string-built query language.
2.  Verify identity ownership during create, bind, update, and delete operations.
3.  Review uniqueness constraints and concurrent identity binding.
4.  Determine transaction boundaries across database and external publication.
5.  Inspect sensitive-field storage, backups, and error logging.
6.  Confirm chatbot episodes are scoped to the correct principal and session.

### Required negative tests

*   Concurrent binding of the same external identity.
*   Query metacharacters in identifiers.
*   Transaction failure between related writes.
*   Cross-principal episode access.
*   Repeated deletion and rebinding.

**Finding disposition:** No verified findings.

* * *

# 11. `op-dbus-cli`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/main.rs`
*   `src/identity.rs`
*   `src/introspect.rs`
*   `src/socket.rs`

### Inspection sequence — not a verified trace

```text
CLI arguments and environment
  → bus/socket selection
  → identity selection
  → destination and method resolution
  → D-Bus invocation
  → terminal output
```

### Security review requirements

1.  Check whether privileged execution trusts user-controlled environment variables or socket paths.
2.  Verify server identity when connecting to configurable Unix sockets.
3.  Review arbitrary destination, object, interface, and method selection.
4.  Inspect output handling for terminal control sequences and accidental secret disclosure.
5.  Confirm identity credentials are not placed in command-line arguments unnecessarily.

### Required negative tests

*   Attacker-controlled socket path.
*   Symlinked socket location.
*   Unexpected bus selection.
*   Malicious introspection XML.
*   Returned strings containing terminal escape sequences.

**Finding disposition:** No verified findings.

* * *

# 12. `op-dbus-model`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/lib.rs`
*   `src/models.rs`
*   Serialization and deserialization call sites.

### Inspection sequence — not a verified trace

```text
external D-Bus representation
  → model deserialization
  → validation
  → internal use or reserialization
```

### Security review requirements

1.  Review defaults for missing fields, especially security-relevant booleans and enums.
2.  Check unknown enum values and integer narrowing.
3.  Determine whether secret-bearing models implement unrestricted `Debug` or serialization.
4.  Inspect custom deserializers for recursion and allocation limits.
5.  Verify object paths and interface names use validated representations where appropriate.

### Required negative tests

*   Unknown fields and enum values.
*   Missing required identifiers.
*   Maximum and minimum integer values.
*   Oversized strings and nested collections.
*   Debug formatting does not expose credentials.

**Finding disposition:** No verified findings.

* * *

# 13. `op-deployment`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/image_manager.rs`
*   `src/lib.rs`
*   All callers granting deployment privileges.

### Inspection sequence — not a verified trace

```text
image/deployment request
  → source resolution
  → download or local lookup
  → integrity verification
  → extraction/import
  → activation
```

### Security review requirements

1.  Determine which image formats and sources are accepted.
2.  Inspect archive extraction for traversal, symlink, and hard-link attacks.
3.  Verify digest or signature enforcement before activation.
4.  Review network destinations and redirect behavior for SSRF.
5.  Check whether activation is atomic and rollback preserves ownership.
6.  Bound image size, decompression ratio, disk usage, and execution time.

### Required negative tests

*   Archive entries escaping the deployment root.
*   Corrupt or unsigned image.
*   Redirect to an internal service.
*   Disk exhaustion during extraction.
*   Crash between preparation and activation.

**Finding disposition:** No verified findings.

* * *

# 14. `op-dynamic-loader`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/dynamic_registry.rs`
*   `src/execution_aware_loader.rs`
*   `src/loading_strategy.rs`
*   `src/error.rs`
*   `src/lib.rs`

### Inspection sequence — not a verified trace

```text
requested capability
  → registry lookup
  → loading strategy
  → artifact or component selection
  → activation
  → execution
```

### Security review requirements

1.  First determine whether “loading” means metadata activation, plugin construction, or native code loading.
2.  Identify who can register or replace a component.
3.  Verify provenance and compatibility checks before activation.
4.  Check synchronization between lookup, replacement, and execution.
5.  Ensure unloading or replacement cannot invalidate active references.
6.  Confirm fallback strategies do not select a less-restricted executor.

### Required negative tests

*   Concurrent registration and execution.
*   Duplicate capability names.
*   Unauthorized replacement.
*   Failed activation leaves no partially registered component.
*   Strategy fallback preserves authorization constraints.

**Finding disposition:** No verified findings; native dynamic-library behavior must not be assumed from the crate name.

* * *

# 15. `op-execution-tracker`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/execution_context.rs`
*   `src/execution_tracker.rs`
*   `src/record.rs`
*   `src/metrics.rs`
*   `src/telemetry.rs`

### Inspection sequence — not a verified trace

```text
execution starts
  → principal/context capture
  → status updates
  → completion or cancellation
  → persistence and telemetry export
```

### Security review requirements

1.  Verify execution ownership is derived from authenticated context.
2.  Check whether callers can overwrite or forge another execution’s status.
3.  Inspect argument, output, and exception recording for secrets.
4.  Review record integrity and failure behavior when telemetry is unavailable.
5.  Bound metric label cardinality and record retention.
6.  Ensure task-local context does not leak between concurrent executions.

### Required negative tests

*   Cross-principal status update.
*   Reused execution identifier.
*   Secret-bearing tool arguments.
*   Cancellation and panic paths.
*   High-cardinality user-controlled metric labels.

**Finding disposition:** No verified findings.

* * *

# 16. `op-gateway`

**Review status:** Not reviewed — implementation contents unavailable.

### Source required

*   `src/wireguard_auth.rs`
*   `src/encrypted_storage.rs`
*   `src/mcp_gateway.rs`
*   `src/error.rs`
*   `SECURITY-MODEL.md`

### Inspection sequence — not a verified trace

```text
incoming connection
  → peer identity derivation
  → principal mapping
  → authorization
  → MCP forwarding
  → credential or encrypted-state access
```

### Security review requirements

1.  Verify network location is not incorrectly treated as sufficient application identity.
2.  Determine how WireGuard peer identity is obtained and protected from spoofed forwarding metadata.
3.  Inspect authenticated encryption, nonce uniqueness, key storage, and rotation.
4.  Verify backend forwarding cannot become a confused deputy.
5.  Check revocation timing and active connection behavior.
6.  Review error paths for cryptographic material leakage.

### Required negative tests

*   Request arrives outside the intended tunnel path.
*   Spoofed peer/forwarding metadata.
*   Revoked peer with an existing connection.
*   Modified

A

You

Continue

![GPT-6 Astra Max](https://cdn.miniapps.ai/cdn-cgi/image/w=18,h=18,fit=cover/images/tools/8fa40ea13fbd7aee6c078.webp)

GPT-6 Astra MaxGPT 6 Astra High

*Continuation of `crate-security-review.md`, completing section 16.*

*   Modified ciphertext, authentication tag, or associated data is rejected.
*   Encryption keys cannot be recovered through errors, logs, or debug output.
*   Backend requests retain the authenticated principal and cannot acquire broader permissions.
*   Key rotation preserves access to authorized data without accepting revoked credentials.

**Finding disposition:** No verified findings. The security-model document and implementation must be compared; documentation alone does not establish enforcement.

* * *

# 17. `op-grpc-bridge`

**Review status:** Not reviewed — implementation contents unavailable.

This crate has a large listed surface: transport servers, schema-driven dispatch, identity publication, configuration mutation, generation jobs, subscriptions, and numerous administrative integrations. Each externally reachable method needs an individual authorization and mutation trace.

## Source required

### Admission and transport

*   `src/server.rs`
*   `src/grpc_server.rs`
*   `src/grpc_web.rs`
*   `src/interceptor.rs`
*   `src/mcp_frontend.rs`
*   `src/mcp_policy.rs`
*   `src/shared_socket.rs`

### Schema and dispatch

*   `src/schema_router.rs`
*   `src/schema_loader.rs`
*   `src/sealed_schema_reader.rs`
*   `src/method_contract_admission.rs`
*   `src/owned_method_task.rs`
*   `src/dynamic_reflection.rs`
*   `src/per_plugin_reflection.rs`

### Sensitive operations

*   `src/mutation_engine.rs`
*   `src/identity_publication.rs`
*   `src/human_principal_dispatch.rs`
*   `src/identity_sled_dispatch.rs`
*   `src/tched_config_secret_policy.rs`
*   `src/generation_admission.rs`
*   `src/event_chain_access.rs`
*   `src/knowledge_delivery.rs`

## Inspection sequences — not verified traces

```text
gRPC / gRPC-Web / MCP request
  → transport admission
  → authenticated principal
  → method resolution
  → contract validation
  → resource authorization
  → method handler
  → downstream mutation
  → audit event
```

```text
subscription request
  → principal and scope binding
  → initial state selection
  → event filtering
  → stream delivery
  → cancellation or permission revocation
```

## Security review requirements

1.  **Protocol parity:** Compare authorization across gRPC, gRPC-Web, MCP, and shared-socket entry points. A protected method must not become accessible through a less-protected adapter.
2.  **Schema authority:** Establish whether published schemas are descriptive or authoritative. Verify callers cannot supply a schema that expands their permitted operations.
3.  **Reflection exposure:** Determine whether reflection reveals private fields, administrative methods, or tenant-specific objects.
4.  **Mutation boundaries:** Trace validation, authorization, commit, publication, and audit ordering for every mutating handler.
5.  **Secret policy:** Verify configuration reads, patches, diffs, error messages, and state streams consistently exclude protected values.
6.  **Owned tasks:** Confirm background jobs bind ownership to authenticated context rather than caller-provided owner fields.
7.  **Subscription isolation:** Apply authorization to every delivered event, not only subscription creation.
8.  **Downstream authority:** Check whether service credentials enable operations beyond the initiating principal’s permissions.
9.  **Response limits:** Inspect large-response support for bounded allocation, pagination, and backpressure.

## Required negative tests

*   Invoke the same privileged method through every supported protocol.
*   Resolve aliases or reflected methods to bypass an operation allowlist.
*   Submit a configuration patch containing protected fields.
*   Cancel a generation request after admission but before execution.
*   Subscribe to state, then revoke access while the stream remains open.
*   Reuse another principal’s job, object, or subscription identifier.
*   Cause downstream mutation to succeed while audit persistence fails.

**Finding disposition:** No verified findings. Existing tests with security-oriented filenames must be inspected and executed before crediting their coverage.

* * *

# 18. `op-http`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/server.rs`
*   `src/router.rs`
*   `src/middleware.rs`
*   `src/tls.rs`
*   `src/lib.rs`

## Inspection sequence — not a verified trace

```text
TCP/TLS connection
  → HTTP parsing
  → proxy-header interpretation
  → middleware
  → route matching
  → handler
  → response middleware
```

## Security review requirements

1.  Establish middleware order, including authentication, request limits, tracing, and error conversion.
2.  Verify trusted-proxy handling for client IP, scheme, and host headers.
3.  Inspect TLS configuration, certificate loading, key permissions, and renewal behavior.
4.  Check request body limits, header limits, read timeouts, and connection concurrency.
5.  Verify CORS configuration does not combine broad origin access with credentials.
6.  Determine whether cookie-authenticated state-changing requests require CSRF protection.
7.  Inspect path normalization and routing for discrepancies between proxy and application.

## Required negative tests

*   Spoofed `Forwarded` and `X-Forwarded-*` headers from an untrusted peer.
*   Oversized or slowly transmitted requests.
*   Disallowed origins and credentialed cross-origin requests.
*   Encoded path separators and dot segments.
*   Invalid TLS configuration fails closed rather than silently enabling plaintext.

**Finding disposition:** No verified findings.

* * *

# 19. `op-identity`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/identity_vault.rs`
*   `src/sealed_id.rs`
*   `src/session_genesis.rs`
*   `src/session.rs`
*   `src/session_projection.rs`
*   `src/registration.rs`
*   `src/recovery.rs`
*   `src/oracle_assertion.rs`
*   `src/gcloud_auth.rs`
*   `src/wg.rs`
*   `src/wireguard.rs`
*   Identity-related binaries and tests.

## Inspection sequences — not verified traces

```text
registration request
  → credential or assertion verification
  → principal allocation
  → identity binding
  → durable commit
  → session issuance
  → public projection
```

```text
recovery request
  → recovery evidence verification
  → ownership transition
  → credential rotation
  → previous-session revocation
  → audit publication
```

## Security review requirements

1.  **Assertion verification:** Inspect issuer, audience, signature, expiration, nonce, and replay checks where applicable.
2.  **Binding integrity:** Verify external identities cannot be rebound to another principal without the required proof.
3.  **Session fixation:** Determine when session identifiers are generated and rotated.
4.  **Recovery strength:** Recovery must not become a weaker unaudited alternative to normal authentication.
5.  **Vault handling:** Review key derivation, authenticated encryption, key permissions, memory exposure, and logging.
6.  **Projection privacy:** Confirm public identity views cannot expose private credentials or recovery material.
7.  **Revocation:** Trace invalidation across sessions, caches, projections, and active connections.
8.  **Network identity:** Establish how WireGuard identities map to application principals and whether mappings are unique and current.
9.  **Legacy reachability:** Determine whether `depreciated-wg.rs` is compiled or reachable; its filename does not establish deprecation in practice.

## Required negative tests

*   Replayed registration or recovery assertion.
*   Wrong audience or issuer.
*   Concurrent attempts to bind the same external identity.
*   Recovery succeeds but old credentials remain usable.
*   Public projection serialization includes a newly added secret field.
*   Partial failure during identity creation and session issuance.

**Finding disposition:** No verified findings.

* * *

# 20. `op-inspector`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/gcloud.rs`
*   `src/introspective_gadget.rs`
*   `src/runit.rs`
*   `src/lib.rs`
*   `ADAPTER-WORKFLOW.md`

## Inspection sequence — not a verified trace

```text
inspection request
  → target selection
  → local or remote inspection
  → output
```

A

You

Continue

![GPT-6 Astra Max](https://cdn.miniapps.ai/cdn-cgi/image/w=18,h=18,fit=cover/images/tools/8fa40ea13fbd7aee6c078.webp)

GPT-6 Astra MaxGPT 6 Astra High

*Continuation of `crate-security-review.md`, completing section 20.*

```text
inspection request
  → target selection
  → local or remote inspection
  → output parsing
  → normalization
  → publication to the caller
```

## Security review requirements

1.  **Target authorization:** Verify callers can inspect only authorized services, cloud projects, and local resources.
2.  **Credential selection:** Determine whether cloud inspection uses caller credentials or a service account. Service-account authority must not automatically become caller authority.
3.  **Subprocess safety:** If inspection invokes external programs, examine argument construction, executable selection, environment inheritance, deadlines, and output limits.
4.  **Untrusted output:** Treat introspection responses as untrusted data. Bound parsing depth, string lengths, collection sizes, and total output.
5.  **Secret exposure:** Inspect normalized results and diagnostic errors for access tokens, environment variables, connection strings, and private configuration.
6.  **Read-only guarantees:** Verify inspection cannot trigger service changes or cloud mutations through shared execution helpers.

## Required negative tests

*   Unauthorized cloud project or service target.
*   Option-like target identifiers passed to any subprocess.
*   A target produces excessive or malformed inspection output.
*   Credential refresh fails without exposing authentication material.
*   Inspection is attempted with a more privileged service account than the caller.
*   An inspection request cannot invoke a mutation operation.

**Finding disposition:** No verified findings.

* * *

# 21. `op-introspection`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/scanner.rs`
*   `src/parser.rs`
*   `src/indexer.rs`
*   `src/indexer_manager.rs`
*   `src/cache.rs`
*   `src/projection.rs`
*   `src/lib.rs`

## Inspection sequence — not a verified trace

```text
discovery request
  → target enumeration
  → introspection response
  → parsing
  → index update
  → cache or projection publication
  → downstream consumer
```

## Security review requirements

1.  **Discovery boundaries:** Identify who controls the scanned destinations. Verify discovery cannot cross an unauthorized bus, host, or namespace boundary.
2.  **Parser limits:** Inspect XML or other parsers for external resource resolution, excessive nesting, oversized attributes, and allocation amplification, where those formats are used.
3.  **Index provenance:** Preserve the origin and trust level of discovered interfaces. A discovered method should not automatically become an authorized callable tool.
4.  **Cache isolation:** Determine whether cache keys incorporate destination identity, namespace, and relevant principal scope.
5.  **Stale entries:** Verify removed or replaced services do not retain callable capabilities through stale indexes.
6.  **Concurrent refresh:** Inspect replacement and publication for partially updated indexes and mismatched schema generations.

## Required negative tests

*   Malicious service returns deeply nested or oversized introspection data.
*   Two services publish conflicting interface names.
*   A service disappears during indexing.
*   A previously authorized target becomes unauthorized before cache expiry.
*   An untrusted discovery result attempts to register a privileged tool.

**Finding disposition:** No verified findings.

* * *

# 22. `op-jsonrpc`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/protocol.rs`
*   `src/server.rs`
*   `src/ovsdb.rs`
*   `src/lib.rs`

## Inspection sequences — not verified traces

```text
JSON-RPC input
  → framing and parsing
  → method resolution
  → parameter validation
  → authorization
  → handler
  → response serialization
```

```text
OVSDB operation
  → transaction construction
  → connection selection
  → submission
  → response/error interpretation
```

## Security review requirements

1.  **Protocol correctness:** Inspect handling of missing IDs, notifications, null IDs, malformed requests, and batch requests if supported.
2.  **Batch resource use:** Bound batch size and concurrent method execution.
3.  **Method authorization:** Ensure dispatch does not expose internal methods through unrestricted string lookup.
4.  **OVSDB authority:** Restrict accessible databases, tables, rows, and mutation operations according to the caller.
5.  **Transaction construction:** Review identifier handling and structured query construction. Determine whether any queries are assembled through string interpolation.
6.  **Partial results:** Verify transaction errors are not mistaken for successful mutations.
7.  **Transport identity:** Establish how the server authenticates clients and how OVSDB connections authenticate their destination.

## Required negative tests

*   Mixed authorized and unauthorized operations in one batch.
*   Oversized batches and deeply nested parameters.
*   Notifications invoking state-changing methods without expected accountability.
*   OVSDB response contains per-operation errors.
*   Caller attempts to modify unrelated bridges, ports, or controller settings.

**Finding disposition:** No verified findings.

* * *

# 23. `op-llm`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/provider.rs`
*   `src/factory.rs`
*   `src/chat.rs`
*   `src/schema.rs`
*   All provider adapters.
*   `src/headless_oauth.rs`
*   `src/gcloud_adc.rs`
*   `src/pty_bridge.rs`
*   Scripts under `scripts/`.

## Inspection sequences — not verified traces

```text
model request
  → provider/model selection
  → credential resolution
  → request transformation
  → remote call
  → streamed or complete response parsing
  → tool-call normalization
```

```text
authentication flow
  → browser, device, or local credential interaction
  → token acquisition
  → token storage
  → refresh
  → credential use
```

## Security review requirements

1.  **Provider trust boundaries:** Determine which conversation data, tool schemas, attachments, and secrets each adapter transmits.
2.  **Endpoint control:** Inspect configurable base URLs and redirect handling for SSRF and credential forwarding to unintended hosts.
3.  **OAuth handling:** Where applicable, verify state, PKCE, callback binding, expiration, and refresh-token protection.
4.  **Credential precedence:** Ensure environment, file, browser-derived, and cloud credentials cannot unexpectedly override a caller’s intended identity.
5.  **Tool-call parsing:** Treat provider-returned function names and arguments as untrusted. Parsing success must not imply execution authorization.
6.  **Retries:** Confirm retries cannot silently duplicate side-effectful orchestration initiated above the provider layer.
7.  **Streaming:** Bound incomplete frames, accumulated tool arguments, response size, and idle duration.
8.  **PTY isolation:** If a CLI bridge is used, inspect terminal input injection, process cleanup, inherited environment, and captured credential output.
9.  **Authentication scripts:** Inspect the scripts’ actual behavior and reachability. Names such as `spoof-auth.js` and `extract-cursor-auth.js` warrant examination but do not establish misuse or a vulnerability.

## Required negative tests

*   Configured endpoint redirects to another origin.
*   Provider returns malformed or oversized tool arguments.
*   Token refresh fails during streaming.
*   Request cancellation leaves a provider subprocess running.
*   Provider errors contain credentials or complete sensitive prompts.
*   A fallback changes the data-processing destination without policy approval.

**Finding disposition:** No verified findings.

* * *

# 24. `op-mcp`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/server.rs`
*   `src/protocol.rs`
*   `src/tool_registry.rs`
*   `src/resources.rs`
*   `src/external_client.rs`
*   `src/agents_server.rs`
*   `src/cognitive_bridge.rs`
*   `src/transport/`
*   `src/grpc/`

## Inspection sequence — not a verified trace

```text
stdio / HTTP / WebSocket / gRPC message
  → connection or session identity
  → protocol validation
  → tool/resource resolution
  →
```

A

You

Continue

![GPT-6 Astra Max](https://cdn.miniapps.ai/cdn-cgi/image/w=18,h=18,fit=cover/images/tools/8fa40ea13fbd7aee6c078.webp)

GPT-6 Astra MaxGPT 6 Astra High

*Continuation of `crate-security-review.md`, completing section 24.*

```text
stdio / HTTP / WebSocket / gRPC message
  → connection or session identity
  → protocol validation
  → tool/resource resolution
  → resource-specific authorization
  → execution or retrieval
  → result filtering
  → response delivery
```

## Security review requirements

1.  **Transport parity:** Verify every transport applies equivalent authorization. Stdio may rely on process ownership, whereas network transports require explicit connection authentication; those trust models must be documented.
2.  **Invocation authorization:** Tool discovery and tool execution must enforce permissions independently. Hiding a tool from a listing does not prevent direct invocation.
3.  **Resource access:** Review resource URI parsing, path resolution, and remote fetching for traversal, unauthorized local reads, and SSRF.
4.  **Registry provenance:** Identify who can register or replace tools. Prevent untrusted registrations from impersonating privileged built-in tools.
5.  **External servers:** Treat external tool descriptions, schemas, results, and resource contents as untrusted input.
6.  **Bridge authority:** Trace principal propagation through the cognitive and agent bridges. Internal service credentials must not grant callers unrestricted downstream access.
7.  **Protocol lifecycle:** Inspect initialization, session binding, cancellation, and reconnect behavior for cross-session state reuse.
8.  **Resource limits:** Bound message size, tool output, concurrent requests, schema complexity, and stream duration.

## Required negative tests

*   Invoke a tool omitted from the caller’s discovery response.
*   Register an external tool with the same name as a privileged built-in.
*   Reuse a session identifier from another connection or principal.
*   Request a resource URI outside the authorized root.
*   Cancel an executing tool and verify subprocess or downstream cleanup.
*   An external server returns instructions asking the model to disclose secrets or invoke another tool.

**Finding disposition:** No verified findings.

* * *

# 25. `op-mcp-aggregator`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/aggregator.rs`
*   `src/client.rs`
*   `src/config.rs`
*   `src/profile.rs`
*   `src/groups.rs`
*   `src/cache.rs`
*   `src/compact.rs`
*   `examples/aggregator.json`

## Inspection sequence — not a verified trace

```text
aggregator configuration
  → upstream connection establishment
  → tool discovery
  → name and schema normalization
  → profile filtering
  → caller invocation
  → upstream routing
  → result delivery
```

## Security review requirements

1.  **Tool-name collisions:** Determine how duplicate names are resolved. An upstream must not replace a trusted tool through discovery order or naming ambiguity.
2.  **Profile enforcement:** Verify profiles constrain invocation, not merely listing or prompt presentation.
3.  **Credential separation:** Check that credentials for one upstream cannot be forwarded to another during redirects, fallback, or connection reuse.
4.  **Configuration authority:** Restrict who can add remote endpoints or local server commands. If local process launching is supported, review executable and environment control.
5.  **Cache partitioning:** Include upstream identity and relevant caller scope in discovery and result-cache keys.
6.  **Schema transformation:** Ensure compaction does not remove constraints needed for validation or introduce ambiguous parameter mappings.
7.  **Failure handling:** Verify an unavailable upstream does not silently route a request to a less-trusted server offering a similarly named tool.

## Required negative tests

*   Two upstreams advertise an identical tool name.
*   A denied tool is called by its fully qualified identifier.
*   An upstream changes its schema after caching.
*   A redirect targets an internal service or another credential domain.
*   Profile changes take effect on existing sessions.
*   Compacted schemas preserve required fields, bounds, and enum restrictions.

**Finding disposition:** No verified findings.

* * *

# 26. `op-network`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/controller.rs`
*   `src/controller_flow_verify.rs`
*   `src/datapath_safe.rs`
*   `src/openflow.rs`
*   `src/openflow_translate.rs`
*   `src/ovsdb.rs`
*   `src/ovsdb_monitor.rs`
*   `src/rovs_proxy.rs`
*   `src/rtnetlink.rs`
*   `src/wireguard_kernel.rs`
*   `src/dbus_tls.rs`
*   `src/unixctl.rs`
*   Network-management binaries and native tests.

## Inspection sequences — not verified traces

```text
network mutation request
  → caller authorization
  → desired-state validation
  → protocol translation
  → kernel / switch / OVSDB operation
  → read-back verification
  → result publication
```

```text
network event
  → monitor decoding
  → topology update
  → controller decision
  → flow modification
```

## Security review requirements

1.  **Privilege scope:** Identify required Linux capabilities and determine whether the entire service needs them or only a narrow helper.
2.  **Mutation authorization:** Restrict interfaces, namespaces, bridges, routes, peers, and flows by caller ownership.
3.  **Isolation invariants:** Verify translation preserves intended drop rules, priorities, masks, table transitions, and default behavior.
4.  **Control-plane protection:** Prevent user-requested changes from exposing management interfaces or bypassing mandatory network isolation.
5.  **Partial updates:** Inspect consistency across OVSDB, OpenFlow, and kernel state when one step fails.
6.  **Monitor integrity:** Review reconnect, resynchronization, stale-event handling, and event ordering.
7.  **Protocol parsing:** Validate message lengths, nested attributes, integer conversions, and unknown protocol values.
8.  **Peer secrets:** Ensure WireGuard private keys and preshared keys do not enter logs, generic state projections, or audit payloads.

## Required negative tests

*   A tenant changes another tenant’s interface or peer.
*   A flow request attempts to override mandatory isolation.
*   OVSDB succeeds but flow installation fails.
*   Monitor reconnect yields stale or incomplete topology.
*   Malformed Netlink or OpenFlow messages are rejected without panic.
*   Read-back differs from requested state.
*   Rollback fails and the service reports an explicit degraded state.

**Finding disposition:** No verified findings. Files named `datapath_safe` and `controller_flow_verify` indicate review targets, not proof of safety.

* * *

# 27. `op-plugin-lint`

**Review status:** Not reviewed — implementation contents unavailable.

## Source required

*   `src/main.rs`
*   `src/audit.rs`
*   `src/binary.rs`
*   `src/introspect.rs`
*   `src/gadget.rs`
*   `src/repomix.rs`
*   `src/emit.rs`
*   `src/rust_emit.rs`
*   `src/subid.rs`
*   Test fixtures and `tests/audit_tests.rs`.

## Inspection sequence — not a verified trace

```text
source / schema / binary input
  → parsing or inspection
  → contract extraction
  → validation rules
  → diagnostics
  → generated files
  → process exit status
```

## Security review requirements

1.  **Binary inspection:** Determine whether binaries are statically inspected or executed. If executed, untrusted inputs require isolation, deadlines, output caps, and restricted credentials.
2.  **Generated code:** Verify names, descriptions, and schema values are escaped correctly before insertion into Rust or other generated output.
3.  **Output paths:** Prevent input-controlled filenames from escaping the chosen output directory or overwriting unrelated files.
4.  **Lint reliability:** Ensure unsupported syntax and incomplete analysis produce explicit limitations rather than a clean bill of health.
5.  **CI semantics:** Confirm blocking violations result in a failing exit code and cannot be hidden by output-format selection.
6.  **Packed input parsing:** Review handling of file boundaries, duplicate paths, oversized inputs, and malformed repository exports.
7.  **Identifier uniqueness:** Inspect sub-ID derivation and collision handling.

## Required negative tests

*   Untrusted binary attempts filesystem or network access during

<!-- INCOMPLETE: shared conversation truncated here, mid-sentence in crate 27 (op-plugin-lint). Remaining crates not yet reviewed: op-plugins, op-runit-systemctl, op-services, op-snowball, op-state, op-state-store, op-tools, op-web, op-workflows, op-xray-daemon. Resume from this bullet and continue crate 27 through crate 36, then any cross-crate traces. Source: https://miniapps.ai/s/imCxbbqu0uIKAbGqwisOcLalUEqzOD -->
