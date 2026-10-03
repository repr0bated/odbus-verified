# Cargo worker routing

The ordinary `cargo` command delegates to a worker:

| Caller | Runner | Model |
|---|---|---|
| GPT / Codex | Codex CLI | `gpt-6-luna` |
| Grok | Cursor Agent | `composer-2.5` |
| Claude | Claude CLI | `haiku` |

Set `CARGO_CALLER_FAMILY=gpt`, `grok`, or `claude` in the calling agent's environment. This explicit setting takes precedence over optional `CARGO_CALLER_MODEL`, `CURSOR_MODEL`, `GROK_MODEL` and Claude/Grok environment hints. Unmarked shells use GPT/Luna. These hints select a worker; they do not establish an authenticated identity. Cursor does not expose a verified caller-model marker, so select the family explicitly when switching models there.

`source scripts/cargo-aliases.bash` installs an exported Bash `cargo` function. The user PATH shim also covers shells that do not expand aliases. Normal syntax is unchanged: `cargo build`, `cargo test`, `cargo metadata`, and so on. Commands in this workspace run from its repository root.

The worker executes one fixed helper with the original argument array. Native Cargo stdout, stderr and exit status return to the caller; model commentary does not pollute JSON output. Requests use private temporary files and a per-workspace dispatch lock. A missing runner or completion receipt fails without executing Cargo locally. `CARGO_DISPATCHED=1` is the worker recursion guard; native Cargo remains at `/usr/bin/cargo`.

Model/runner overrides are `CARGO_MODEL_GPT`, `CARGO_MODEL_GROK`, `CARGO_MODEL_CLAUDE` and the corresponding `CARGO_RUNNER_*` variables. `CARGO_DISPATCH_DRY_RUN=1 cargo check` prints the routing plan without invoking a model or Cargo. No permission-bypass flags are supplied to the runners.

`bash scripts/tests/test-cargo-dispatch.sh` checks all routes, exact argument handling, output/status propagation, missing completion and recursion with fake runners. It never invokes real Cargo or a model. On 2026-10-03 all three actual runners completed `cargo --version` and returned the native version output. This verifies model access and execution routing; it is not a full build validation.
