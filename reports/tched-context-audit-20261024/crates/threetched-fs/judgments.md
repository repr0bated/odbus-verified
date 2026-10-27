# threetched-fs router-retirement fallout — contextual JEV decisions

Date: 2026-10-24. Static source review only. Root owns source changes, build, and deploy. No source edits, Cargo, tests, runtime, deployment, or delegation in this worker. JEV was called directly via `judge()`; outputs below are the returned typed choices and probabilities.

## Evidence reviewed

- `crates/op-plugins/src/default_registry.rs:21-39`: `tched_router` is explicitly described as a retired competing application schema and appears in `RETIRED_PLUGIN_IDS`; only captured `threetched` operations may be published.
- `crates/threetched-fs/generated/plugin_types.rs:113065-...`: generated `plugin_f_tched_5frouter` declares `PLUGIN_ID="tched_router"`; the next module `plugin_f_threetched` begins at 116454. Assignment search evidence: no consumers of the old generated router module outside that generated file; `examples/typed_use_generated.rs` imports `plugin_f_tched_5ffs` instead. The generated file's own header states that derived types are not schema authority.
- `crates/threetched-fs/examples/typed_specialize.rs:709-735,949-1010`: the generator derives output from a captured sealed catalog; an explicitly nonempty `--plugin` selection filters other plugins; it verifies every selected ID exists. It does not embed a router schema. Exact prior selected-plugin input and pinned catalog provenance are not established here.
- `crates/threetched-fs/src/dispatch.rs:194-195,232-237,307-318,332-336` and `src/mounted.rs:523-559,590-598`: these modules directly refer to `crate::invocation` types. `src/lib.rs:8-14` declares the library's `pub mod invocation`.
- `examples/typed_check.rs:1-22` declares dispatch and mounted modules but no invocation module. Generator `usage_example()` emits dispatch, mounted, and namespace but not invocation (`examples/typed_specialize.rs:884-932`); checked-in `examples/typed_use_generated.rs:1-15` likewise omits invocation. The targeted scan found no invocation reference in `namespace.rs`.
- `README.md:371-374` still directs replacing an UPSTREAM IMPORT block in `tched_router_contract.rs`; current `AGENTS.md` capture policy says the block was retired and full config schema is captured from the verified binary on every seal. README's next capture example sets `COGNITIVE_QDRANT_URL=http://10.200.0.2:6334`; current policy at `AGENTS.md:177-199` and `deploy/runit/op-grpc-bridge/run` use `http://127.0.0.1:6334`.
- Runtime parity is not established: assignment evidence says the mounted view lists both `tched_router` and `threetched`.

## JEV batch 1 — candidates and runtime boundary

Input state included the evidence above plus three scoped candidates: (1) regenerate existing generated artifacts using the exact prior selected plugin set minus retired `tched_router`, preserving surviving schema IDs, without manual edits or aliasing it to `threetched`; (2) add the existing invocation module to generator/typed example declarations and regenerate the example; (3) correct only the stale README capture procedure and Qdrant URL. Questions explicitly offered retain/no-fallout or insufficient alternatives and separately asked what could be claimed about runtime status.

| Question | Choice | Probabilities | Confidence |
|---|---|---|---|
| Disposition for generated retired router output | `regenerate_selected` | regenerate_selected 0.94; retain_no_source_fallout 0.00; insufficient_inputs 0.06 | 0.91 |
| Invocation module-list correction | `supported_compile_fallout` | supported_compile_fallout 0.71; insufficient_without_build 0.28; different_fix 0.01 | 0.56 |
| README correction | `correct_docs` | correct_docs 1.00; retain 0.00; insufficient 0.00 | 1.00 |
| Live/deployed retirement claim | `unverified` | unverified 1.00; retired_live 0.00; active_regression 0.00 | 1.00 |

## JEV batch 2 — invocation module resolution follow-up

Because batch 1's invocation confidence was below 0.6, I rechecked the exact module declarations and direct `crate::invocation` references, then asked a narrower question. Input specified that `typed_check.rs` and `typed_use_generated.rs` are standalone example crate roots; each imports dispatch/mounted by path but declares no root `invocation`, while those modules reference `crate::invocation`; the library's declaration does not insert a module into a separate example crate root. No compiler result was asserted.

| Question | Choice | Probabilities | Confidence |
|---|---|---|---|
| Does the example crate-root path resolve? | `unresolved_path` | unresolved_path 0.96; needs_compiler 0.04; library_satisfies 0.00 | 0.94 |

## Disposition for coordinator

- Generated router schema: regeneration is supported as a derivative-artifact cleanup candidate, not as proof of a runtime regression. Use the authoritative pinned catalog and exact prior selection minus `tched_router`; preserve surviving schema identities. The evidence does not include those exact input artifacts, so root must obtain/reconcile them before regeneration. Do not hand-edit generated code or rename router schemas to `threetched`.
- Invocation module: the source-level unresolved path is supported. Add the existing `invocation` module to `typed_specialize.rs`'s generated module list and `typed_check.rs`, then regenerate `typed_use_generated.rs`; do not add another dispatcher. This conclusion is source/JEV evidence, not a compiler run.
- README: update only the obsolete capture instructions and URL as described. No runtime change is supported by this documentation evidence.
- Runtime/deployment parity remains unverified despite the mounted listing evidence; no live regression claim.
