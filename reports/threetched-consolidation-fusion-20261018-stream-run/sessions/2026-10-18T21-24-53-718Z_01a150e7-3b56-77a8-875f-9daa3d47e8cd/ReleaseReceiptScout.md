## Summary

**The Fusion33 workspace build, isolated seal, and generated specialization were completed on October 18. Do not rebuild merely to satisfy the stale “release blocked” consolidation entry.**

Recovered authoritative Hermes worker identity, complete final result, and terminal exit receipts from **`/home/jeremy/.hermes/state.db`**. The originally referenced scratch directories are now absent, but their terminal results survive in that database.

A subsequent corrected-runtime-path **GNU workspace rebuild** also completed successfully and produced catalog **generation72/hash `6f576d…260c`**, containing all33 retirements. This matches the parent-reported current mount pins and the existing specialization report.

**Historical deployment success is not current readiness.** The separate October20 activation incident and owner prohibition on recovery remain controlling.

## Files / authoritative receipt index

Database references below mean table `messages`, primary key `id`.

| Evidence | Exact location |
|---|---|
| Dispatch maps `sa-0-eef0df78` to `deleg_904ae7a3` | `/home/jeremy/.hermes/state.db`, message **3093** |
| Original workspace build terminal completion, exact command/environment, exit0 | Same DB, **3418** |
| Original seal + specialization + export terminal completion, all exit0 | Same DB, **3431** |
| Original build times and native ELF provenance | Same DB, **3432** |
| Complete original catalog method/plugin delta | Same DB, **3436** |
| Original worker’s complete final result | Same DB, **3445**, session `20261018_222535_a64d8a` |
| Corrected-path preparation script, including exact generators and provenance predicates | Same DB, **3824**, `tool_calls[0].function.arguments` |
| Corrected-path workspace build terminal completion, exit0 | Same DB, **3848** |
| Corrected-path seal/generation/provenance completion, all exit0 | Same DB, **3954** |
| Corrected-path catalog delta and unchanged retirement sets | Same DB, **3957** |
| Captured external executable/manifest provenance | Same DB, **3961** |
| Corrected build’s `Finished … 11m27s` marker | Same DB, **3962** |
| Historical activation retry terminal completion, exit0 | Same DB, **3990** |
| Historical mount split/pins and catalog parity | Same DB, **3994**, **3997** |
| Corrected-path worker final result | Same DB, **3998**, session `20261018_230246_3870a9` |
| Short, partially truncated transcript copies | `/home/jeremy/.hermes/cache/delegation/live/deleg_904ae7a3/task-0.log:134-154`; `/home/jeremy/.hermes/cache/delegation/live/deleg_9233f037/task-0.log:116-126,162-175` |
| Current specialization pin/status | `/srv/git/odbus-verified/crates/threetched-fs/docs/typed-specialization-report.json:1-6` |
| Latest separate compiler completion | `/var/tmp/provision-continue-20261020.4oGJXP/build-receipt.json` |
| Latest separate build command | `/var/tmp/provision-continue-20261020.4oGJXP/build.sh:25-30` |
| Current incident boundary | `/srv/git/odbus-verified/HANDOFF.md:935-945`, ownership row555 |

## Report

### 1. Original completed worker `sa-0-eef0df78`

**Source provenance:** HEAD `051a20d056afba074e21d797aa34f8947c543c5a`, dirty shared work preserved. Final result says authored-source hashes remained unchanged; expected generated files changed; no compile fixes needed.

**Build:** October18 **22:36:19–22:50:16 UTC**, Cargo reported **13m56s**, terminal **exit0**.

```sh
cargo build --workspace --release --target x86_64-unknown-linux-gnu
```

Recorded explicit environment:

```sh
CXXFLAGS='-include cstdint'
THREETCHED_CAPTURE_BIN="$PWD/target/threetched-artifacts/tched-router/threetched"
THREETCHED_CAPTURE_MANIFEST="$PWD/target/threetched-artifacts/tched-router/manifest.json"
THREETCHED_BIN=/usr/bin/threetched
THREETCHED_MANIFEST=/usr/share/op-dbus/threetched-manifests/tched-router.json
TCHED_ROUTER_CONFIG_DIR=/var/lib/tched-router
COGNITIVE_QDRANT_URL=http://127.0.0.1:6334
```

Artifact directory: `target/x86_64-unknown-linux-gnu/release/`. Worker inventory reported **40 workspace executable targets**. Message3432 confirms inspected artifacts were x86-64 GNU dynamically linked ELF files using `/lib64/ld-linux-x86-64.so.2`.

Original isolated catalog:

```text
/home/jeremy/.hermes/cache/scratch/provision-release-corrected-UKtJFazI/catalog
```

Catalog hash:

```text
25ada81a6bbbcbeef07ea4f017a20be581659f9fda6b13181f7dd725b630b86b
```

That build/seal **did include Fusion33**, but its `/usr/bin/threetched` runtime selector subsequently failed golden’s provenance requirement. It was superseded by the next workspace rebuild, not a restoration of retired wrappers.

### 2. Subsequent corrected-runtime-path workspace release

**Command unchanged:**

```sh
cargo build --workspace --release --target x86_64-unknown-linux-gnu
```

Same explicit capture/config/Qdrant/CXXFLAGS environment, except:

```sh
THREETCHED_BIN=/usr/local/libexec/op-dbus/threetched/threetched
LC_ALL=C
```

The completed terminal result is message3848: **status `exited`, exit_code0**, output `build exit=0`. The completion was recorded at **23:15:11 UTC**; Cargo’s retained marker says **11m27s**.

Corrected isolated catalog:

```text
/home/jeremy/.hermes/cache/scratch/provision-runtime-path-20261018.MMsrUT/catalog
```

Its hash is:

```text
6f576dbdcdb4416f3471028393be42852078b3ed54c6439fe28c79d7248d260c
```

Capture provenance recovered from message3961:

| Field | Value |
|---|---|
| External plugin | `threetched` |
| Declared method count | **147** |
| Runtime binary | `/usr/local/libexec/op-dbus/threetched/threetched` |
| Runtime manifest | `/usr/share/op-dbus/threetched-manifests/tched-router.json` |
| Binary SHA256 | `22e121f5eb1b66352138b714cc3d40d9eb977ed0c13bd6316f693a11a875a75e` |
| Manifest SHA256 | `5fd1a4db784f20a788d16722d86dde01ee0d9ba574598b007d5f1c3e4585b5b7` |

Historical activation reached A/root315, release `release-c068ba1e-02d3-4362-88f7-faf41919a837`. Its catalog-manifest SHA256 was `40427e0a12373f53470b68fe4a2d53ed3f5463bb878c261d80d4cde988100826`. **These are October18 receipts only, not current service health.**

### 3. Exact catalog delta: Fusion33 included

Both completed catalog reviews recorded the same method/plugin sets:

| Metric | Baseline | Result |
|---|---:|---:|
| Catalog generation | 74 | 72 |
| Plugins | 74 | 72 |
| Declared methods | 1,388 | 1,357 |
| Removed methods | — | **33** |
| Added methods | — | **2** |
| Removed plugins | — | `tched_plugins`, `tched_skills` |
| Added plugins | — | None |

Baseline hash: `8a34d0da9517b137063314e07f5fa9a24fe3a3fe33920b1de2df941380041fd6`.

**Added methods:** `identity_sled.prepare_container`, `incus.reconcile_creation`.

**All33 removed methods**, recovered from message3436:

| Plugin | Removed methods |
|---|---|
| `gemma_brain` — 1 | `route` |
| `large_language_model` — 7 | `generate`, `get_config`, `get_provider`, `list_models`, `list_providers`, `set_provider`, `update_config` |
| `tched_plugins` — 6 | `install_plugin`, `list_plugins`, `migrate_plugins`, `plugin_info`, `remove_plugin`, `search_plugins` |
| `tched_router` — 7 | `GetConfigSchema`, `GetProviderCatalog`, `get`, `init`, `list`, `patch`, `set` |
| `tched_skills` — 12 | `add_bundle`, `add_skill`, `audit_skill`, `edit_skill`, `install_skill`, `list_bundles`, `list_skills`, `remove_bundle`, `remove_skill`, `rename_bundle`, `show_bundle`, `test_skills` |

Corrected-path message3957 explicitly records **“Catalog sets exactly match previously reviewed delta.”**

### 4. Generated specialization receipt

Both release preparations ran, with their respective `$R/catalog`:

```sh
target/x86_64-unknown-linux-gnu/release/opblob seal-plugins "$R/catalog"

cargo run --release --target x86_64-unknown-linux-gnu \
  -p tched-fs --example typed_specialize -- \
  --catalog "$R/catalog" \
  --out-file crates/threetched-fs/generated/plugin_types.rs \
  --report crates/threetched-fs/docs/typed-specialization-report.json

cargo run --release --target x86_64-unknown-linux-gnu \
  -p tched-fs --example typed_export -- \
  --out-dir crates/threetched-fs/docs
```

Message3954 contains terminal **exit0 for seal, typed-specialize, typed-export, and provenance-capture**. The preparation script also required report hash, generation, and plugin-key set to equal the sealed manifest before printing its success marker.

Existing report currently contains:

- hash **`6f576d…260c`**
- generation **72**
- **72 plugin entries**
- status **`generated source; compilation/runtime verification is separate`**

**Important count distinction:** its generated `methods` arrays total **1,182**, not1,357. The report records **186 unsupported subjects**, including state subjects; these cannot simply be subtracted as method counts. `threetched` has147 unsupported method subjects due to `allOf` intersections and no generated methods; `tched_router` has36 generated methods and an unsupported `State` because generated source exceeds128MiB.

Therefore: **generation is proven complete; full typed coverage or generated-consumer compilation is not proven.** Worker3445 explicitly states generated Rust feeds only the `typed_use_generated` example, not production binaries.

### 5. Latest compiler claim resolved through terminal receipts

The stale “third detached build active” wording at HANDOFF959 is superseded by:

```text
/var/tmp/provision-continue-20261020.4oGJXP/build-receipt.json
phase: complete
exit: 0
completed_at: 2026-10-20T08:59:59Z
installed: false
deployed: false
```

Exact command from its retained `build.sh`:

```sh
cargo build --release --target x86_64-unknown-linux-gnu \
  -p op-grpc-bridge --bin op-grpc-bridge \
  -p op-plugins --bin opblob
```

Explicit exports: `CXXFLAGS='-include cstdint'` and the two absolute capture paths under `/srv/git/odbus-verified/target/threetched-artifacts/tched-router/`.

The bounded terminal log ending says:

```text
warning: `op-grpc-bridge` (lib) generated 6 warnings
Finished `release` profile [optimized] target(s) in 11m 50s
```

**This October20 build was scoped, not workspace-wide.** Its prior successful receipt is `build-second-receipt.json`, exit0 at08:46:40Z. No process polling or current compiler-slot assertion was performed.

## Remaining missing proof / boundaries

1. **Original scratch artifacts unavailable:** both October18 scratch roots returned “not found”; a filename search found no surviving copies there. Their complete commands, terminal exits, catalog delta and worker results are recovered from `state.db`, but the original `workspace-artifacts.sha256`, source fingerprint inventories, and isolated catalogs cannot now be independently reread at those paths.
2. **No full generated-consumer compile/runtime proof:** explicitly separate from generator success; unsupported coverage remains disclosed by the report.
3. **No current healthy deployment claim:** HANDOFF935–945 records the separate October20 failed activation and recovery-pending state. Owner permission hold remains intact.
4. **No current global compiler-slot claim:** completed receipts resolve the named historical builds, not whether unrelated compilation exists now.

**Architecture:** shared source → GNU workspace artifacts → fresh GNU `opblob` isolated catalog → catalog-derived specialization/export → historically canonical activation. The recovered evidence establishes the first four steps for Fusion33 and prevents repeating them solely because its old report was never updated.