## Summary

**The 33 intended method retirements remain preserved in source, current generated bindings, and the mounted schemas inspected. Do not repeat the retirement or blindly rebuild/reseal because the consolidation report still says blocked.**

Two concrete, not-yet-dispositioned consumers need attention:

1. **`crates/threetched-fs/examples/typed_use_generated.rs:29` imports a generated module that no longer exists.**
2. **The specialization README and consolidation completion/status sections are stale.**

The generated catalog is already **generation 72**, hash:

```text
6f576dbdcdb4416f3471028393be42852078b3ed54c6439fe28c79d7248d260c
```

Both mounted views currently report that same identity. No edits, Cargo, checks, LSP, operation invocation, service action, or policy changes were performed.

## Report

### 1. Current identity and retirement evidence

| Surface | Observed result | Evidence |
|---|---|---|
| Specialization report | Generation 72; 72 plugin entries; hash above | `crates/threetched-fs/docs/typed-specialization-report.json:1-5` |
| Main mounted view | 70 objects; same generation/hash; neither removed wrapper plugin present | `/run/mounts/3tchedFS/view.json` |
| Upstream mounted view | Exactly `tched_router`, `threetched`; same generation/hash | `/run/mounts/threetched/view.json` |
| Router | 36 methods; no `get/init/list/set/patch`, `GetProviderCatalog`, or `GetConfigSchema` | `/run/mounts/threetched/plugins/tched_router/schema.json`; report `:4237-4284`; generated source `:116410-116452` |
| LLM | Exactly `stream_generate` | `/run/mounts/3tchedFS/plugins/large_language_model/schema.json`; report `:3065`; generated source `:84299` |
| Gemma | 16 methods; no `route` | `/run/mounts/3tchedFS/plugins/gemma_brain/schema.json`; report `:2591`; generated source `:75696-75711` |
| Native threetched | 147 declared methods | `/run/mounts/threetched/plugins/threetched/schema.json` |
| Removed modules | No matching `gemma_brain_dispatch`, `binary_namespace`, `tched_plugins`, or `tched_skills` source files; no surviving module wiring found | Glob plus symbol searches across plugin/bridge source |

The two upstream **object hashes** are:

```text
tched_router  6f481714a2ff3f8121b2ad1a1fc4189956fe9328f8eae6b38be8517e70a61376
threetched    62ea6d84d9f50eff43da2e92514420ad3e237df43d0d9e39580ee37a80f1da2c
```

These are object hashes, **not** the schema hashes below.

Generated module pins match the corresponding specialization-report entries:

| Plugin | Schema hash | Generated anchor |
|---|---|---|
| `gemma_brain` | `107ffbac3b403dd1d17072b314f798908a82ca14092d361e7bc6af61bbceae8d` | `generated/plugin_types.rs:73901-73904` |
| `large_language_model` | `c5010c681bcf6681b402afdd445f1fc6b9574859064501ea6a586ef0c9cbf608` | `:84100-84103` |
| `tched_router` | `60345942852e5b8fa0ee3d87d5ca578b5edd2d4e113964008cffab682a5d533e` | `:113065-113068` |
| `threetched` | `5e27757d2c046632f9dcda0866fd58a6a0d38b14fc96ba19625f61166975149c` | `:116454-116457` |

The 65.3 MB generated file was searched and read only at bounded relevant ranges, not read in full.

### 2. The sixteen source/test paths remain consistent with the retirement

Paths below are relative to `/srv/git/odbus-verified`.

| File | Finding |
|---|---|
| `crates/op-plugins/src/state_plugins/tched_router.rs` | Searches found no retired accessor or short-namespace wiring. Mounted/generated router inventory remains 36. |
| `…/tched_router_legacy_methods.rs:1-75,130-151` | Retains native observation, canonical route result, and durable role-assignment owner; removed accessor names absent. |
| `…/large_language_model.rs:100-136` | Schema declares only `stream_generate`, `llm.invoke`, and router dependency. |
| `…/large_language_model_methods.rs:38-67` | Only `stream_generate → start_generation`; other methods error. Typed generation receipt decoding retained. |
| `…/antigravity.rs:638-664,788-812` | Default/catalog owner metadata names `tched_router`. |
| `…/antigravity_chat.rs:72-83,98-103,198-199` | Default/catalog owner metadata names `tched_router`. |
| `…/gemma_brain.rs:1-17,172-245` | Local taxonomy/workspace responsibility retained; no `route` declaration. |
| `crates/op-grpc-bridge/src/tched_router_legacy_dispatch.rs` | Retained observation/role dispatch; removed accessor names absent. |
| `…/mutation_engine.rs:220-225,3903,4944-4946,5566-5570` | LLM owned-task classification restricted to streaming; Gemma workspace dispatch and shared router observer retained. |
| `…/lib.rs:77,138` | Retained LLM/router modules; removed Gemma bridge wiring absent. |
| `…/gemma_brain_dispatch.rs` | Remains absent. |
| `…/large_language_model_dispatch.rs:16-99,102-116` | Exact nested capability admission, verified identity, origin pins, owned target task, and shared observer remain. |
| `…/antigravity_dispatch.rs:36,74` | Runtime owner metadata is `tched_router`. |
| `…/owned_method_task_tests.rs:198-216` | Nested lifetime test now uses streaming facade and canonical `start_generation`. |
| `crates/op-grpc-bridge/tests/large_language_model_dispatch.rs:95-145` | Source retains absent/drifted target, missing nested grant, invalid session, and no-HTTP assertions for streaming facade. |
| `…/tests/tched_router_backing.rs:788-853,1279-1403` | Canonical `ResolveRoute`, `ListProviders`, selections, `GetState` projections, `ListModels`, and `Chat` assertions remain migrated. |

These are **source-inspection findings**, not executed test results.

### 3. Preserve streaming-origin admission exactly

Do not simplify the streaming facade into an unqualified router rename:

- `generation_admission.rs:28-46`: `GenerationOrigin::admit` pins `large_language_model.stream_generate`, exact `llm.invoke`, and admitted-versus-sealed schema equality.
- `generation_admission.rs:146-161`: worker catalog checks revalidate the facade schema hash, schema bytes, and compiled/pinned method contract.
- `generation_admission.rs:172-188`: execution/publication checks revalidate the caller and require `llm.invoke` to remain granted when an origin exists.
- `large_language_model_dispatch.rs:16-60`: independently validates canonical target arguments/grant, verifies session, and constructs the origin.
- `large_language_model_methods.rs:38-53`: canonical destination remains `start_generation`, with capability obtained from its owning schema.

No additional retirement is justified here.

### 4. Newly actionable affected consumer: stale generated example

**`crates/threetched-fs/examples/typed_use_generated.rs:29`:**

```rust
use plugins::plugin_f_threetched_5ffs::*;
```

Current generated output instead declares:

```rust
// generated/plugin_types.rs:112715-112718
pub mod plugin_f_tched_5ffs {
    // ...
    pub const PLUGIN_ID: &str = "tched_fs";
}
```

Exact whole-file symbol search found no `plugin_f_threetched_5ffs` declaration.

**[INFERENCE]** The example cannot resolve its imported module against the current generated file. This was not compiled.

The existing generator already handles the correct name:

- `examples/typed_specialize.rs:861-875`: selects `tched_fs` and passes its derived module name to `usage_example`.
- `:939`: emits `use plugins::{module}::*`.
- `:998-1004,1021-1035`: writes the example only when `--use-example <path>` is supplied.

**Remaining obligation:** reconcile this optional generated consumer with the same pinned catalog, using the existing generator’s `--use-example crates/threetched-fs/examples/typed_use_generated.rs` output path when execution is authorized. Do not add an obsolete-module alias or manually alter schema hashes. This finding does **not** justify repeating completed retirements or changing runtime admission.

### 5. Generated coverage is not sealed method count

The current report contains:

- **1,182 generated method bindings**
- **175 unsupported method entries**
- **11 unsupported state/public-state entries**

Thus it represents **1,357 method subjects**, not the old forecast of 1,355. The report’s generation loop emits one supported or unsupported entry per declared method (`typed_specialize.rs:788-853`).

Important existing limitations:

- `threetched`: all **147 methods** explicitly unsupported by static specialization because of `allOf`; mounted schema still declares all 147.
- `tched_router`: all **36 methods** specialized; full `State` is unsupported with `generated source exceeds 128 MiB`, while `PublicState` is supported.

These are explicit generator limitations, **not evidence that the native methods disappeared or that regeneration failed**. Do not expand this task into implementing general `allOf` or increasing memory limits.

**Remaining documentation obligation:** replace forecast-only totals with properly labelled current evidence; account for the two-method difference before claiming an exact historical catalog delta. This investigation did not establish which unrelated schema changes explain that difference.

### 6. Mount adapter and editor policy are preserved

`deploy/runit/libexec-3tched/mount-3tchedfs:30-35,71-91,110-117`:

- Selects exactly `threetched,tched_router` for upstream.
- Main view excludes those same IDs.
- Readiness compares catalog generation/hash and exact object membership.
- Existing identity selection and lungs transport remain intact.

`crates/op-plugins/src/state_plugins/threetched.rs:289-296` still rejects `skills.edit` and editor-requesting `skills.add` before spawning. No runtime refusal operation was executed.

### 7. Stale status to reconcile, not repeat

- `reports/threetched-consolidation-handoff-20261018.md:412-419,432-459,651-677` still describes historical bindings and blocked release work.
- `crates/threetched-fs/README.md:470-483` still says checked-in specialization is generation **1494**. This is demonstrably stale.
- `HANDOFF.md:601` explicitly releases the old source-freeze blocker, preserving all 33 retirements.
- `HANDOFF.md:101-105` records later completed build/seal/activation: release `c068ba1e-02d3-4362-88f7-faf41919a837`, A/root315, previous B/root304, catalog parity, and 70+2 views.

The referenced scratch receipt directory `provision-runtime-path-20261018.MMsrUT` was not found through exact and broader path searches. Therefore the activation details above are **the authoritative handoff’s recorded receipt**, not independently reopened raw execution logs. Current mount/catalog observations independently corroborate the catalog cutover, not the present release-slot status.

### 8. Already dispositioned; leave unchanged

- Four obsolete config-schema grants remain at `deploy/security/capability-grants.json:7,86,338,533`. Already explicitly dispositioned; no grant edits.
- MCP/cron/native contract migration, cognitive aliases/HOT-policy dependencies, model registry responsibilities, OD-122 and OD-123 retain their recorded deferrals.
- Searches found no demonstrated retired-method production caller in current bridge/plugin source, scripts, deploy code, or sibling UI source. Remaining old accessor names occur in historical `op-plugin-lint/examples/zeroclaw*` captures and retirement comments.
- Sibling UI `src/json-render/catalog/stream-plugins.ts:32,42` still names **retained plugin IDs** `gemma_brain` and `large_language_model`; these are not retired-method callers.

## Architecture

Plugin declarations feed sealed schemas; generated specialization derives pinned typed wrappers from that catalog. Mounted discovery reads the sealed catalog independently of optional static specialization. Canonical bridge dispatch retains exact target admission and identity/state ownership. Consequently the stale **example import** affects a generated-source consumer, not proof of a broken mounted transport.

**Parent’s remaining work:** reconcile stale report/README status, address the optional example’s obsolete module import through the existing generation path, and accurately label the current method-count/coverage evidence. No additional alias, policy, model-routing, or method-retirement edits were justified.

**Tool limitation:** no callable `judge`/`jev_rerank` tool was exposed in this child’s inventory. Search relevance scores were available through `find`; no standalone JEV judgment or confidence is fabricated.