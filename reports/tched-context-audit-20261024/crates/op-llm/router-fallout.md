# Router-removal cleanup — op-llm

Agent Codex `/root`, 2026-10-24, task `router-fallout-20261024/op-llm`. HEAD `86f03d2dede738e2d780ab789dd5800b58678479`. Earlier native wording audit is separate and is not rewritten as this task's evidence.

Removed the unused `src/schema.rs` compatibility module and its `pub mod schema` export from `src/lib.rs`. It reexported the retired router's implementation and reconstructed `TchedRouterPlugin::current_state().catalog` through `embedded_projection()`. Exact repository Rust caller searches found only that helper's own definition and no imports of `op_llm::schema`; the crate had no internal `crate::schema`/`super::schema` use. Common types remain owned by `op-plugins`; dependencies and working provider code were not changed.

This removes a public module, so external consumers outside this repository would need migration to the owning types. No such consumer was identified. JEV supported this removal candidate at0.89 confidence; exact input/output and the wider router caller decisions are in `reports/tched-context-audit-20261024/dataflow-jev.md`. This does not implement the unfinished guest generation hook or repair the facade's retired target.

Before copies preserve both current pre-edit files, including the earlier changes to schema.rs, in `/var/tmp/router-fallout-op-llm-20261024/before/`. This task deletes18 current lines plus the module declaration; the Git baseline diff also includes the prior reduction of that file from30 lines.

Targeted GNU release library build passed, exit0 (3m29s). Log and exit receipt: `/var/tmp/router-fallout-op-llm-20261024/`. No tests, Clippy, formatting, runtime changes or deployment. There is no standalone op-llm binary to activate; deployment depends on its consuming application.
