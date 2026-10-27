# Router-removal fallout — threetched-fs

Owner clarified on 2026-10-24: search for the removed router, inspect context, use JEV, fix fallout and dead code crate by crate. Exhaustive byte reading and cosmetic naming review are superseded. Coordinator: Codex /root. Baseline HEAD: 86f03d2dede738e2d780ab789dd5800b58678479; existing working-tree edits preserved.

- No active `tched_router`, `TchedRouter` or `tched-router` references remain in this crate's src/ or examples/ code. The generic mounted filesystem obtains plugin IDs from the declared catalog.
- Removed the unused generated `plugin_f_tched_5frouter` module by rerunning the existing generator against an isolated copy of persisted sealed catalog generation216, selecting the previous 72-plugin set minus `tched_router`. No consumer of that module was found outside generated output. The report now contains 71 plugins, 1,151 specialized methods and 175 unsupported method entries. No schema ID was renamed. The refreshed input also updates the elevation, emqx and json_render schema pins to that catalog; no live catalog was changed.
- Fixed the typed examples' unresolved `crate::invocation` references by adding the existing module to `typed_check` and the generator's module list, then regenerating `typed_use_generated`.
- Corrected the README's obsolete instruction to replace an UPSTREAM IMPORT block, the build/seal Qdrant environment example, and the generated-artifact inventory/selection instructions.
- Retained historical October5 observations and plugin-owned contract export provenance. They are marked historical or derived and are not executable calls. Retained vendor/artifact paths named tched-router because those paths still identify the pinned threetched build inputs; a filename rename would not remove router integration.

Actual JEV inputs and responses: /var/tmp/router-fallout-threetched-fs-20261024/jev-decisions.md; raw tool receipts: jev-worker.jsonl in that directory. JEV supported regeneration excluding the retired router (.91), the stale capture-doc correction (1.00), and the example module-resolution defect (.94 after exact-context follow-up).

Build: GNU release build of 3tched-fs, typed_specialize and typed_check succeeded (37.13 seconds); regenerated typed_use_generated also compiled successfully (2m53s). Both Cargo exits were zero. No tests, Clippy, formatting, smoke fixtures or deployment have run.

Deployment dependency: installed bridge binary advertises the older three-input CallWithOperationId; current filesystem source sends five inputs, and current bridge source receives five. This is a protocol compatibility dependency, not a name-caused defect. Targeted activation must not silently pair the new filesystem with the incompatible older receiver. Source/installed evidence is in the task scratch directory; no live failure was induced. Current mounted views and persisted catalog still include tched_router; source retirement is not claimed as deployed.
