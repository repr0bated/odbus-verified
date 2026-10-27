# Findings — `op-chat-responder`

## Tched-specific disposition

No in-scope tched-specific source correction is supported. All 16 occurrences are individually listed in `occurrences.tsv` and reviewed against exact local context, repository naming authority, current plugin producer, and the saved projected-operation receipts for O8.

- Manifest names are accurate: dependency package `tched-fs` points to retained source directory `threetched-fs`.
- Rust imports use `tched_fs` for the filesystem crate modules (`invocation`, `mounted`, `namespace`), matching project ownership terminology.
- `threetched` is the executable/surface and plugin name. Its plugin source registers `threetched`; the worker's `config.get` caller matches the projected operation input/output contract. That declaration requires a method-specific capability and labels the method mutation/non-idempotent; no grant or successful invocation was verified.
- `tched_router` is a distinct retired plugin/integration excluded as this worker's dispatch target. The current registry retires it; a dated generation-216 mounted-view receipt still lists both IDs. That source/view discrepancy does not make `tched_router.Chat` the worker's `op_llm::chat::ChatManager` target or establish current deployment parity.
- The worker declares `op_llm::chat::ChatManager` through a `large_language_model.stream_generate` CallMethod as the intended generation path, separate from the threetched configuration query.
- The concrete `threetched` plugin string is now retained boundedly: current source defines that plugin ID, and the saved projected operation receipt identifies `config.get` with an input shape matching the caller. The method is declared mutation, non-idempotent, and capability-gated. Do not infer that it is semantically read-only, that its capability is granted, or that it executed successfully.

## Non-tched operational limitation retained

`our_stack_generate` is an explicit `bail!` stub. Its own comments require a combiner to seal, grant, guest-mount, and route the CallMethod before replacing the stub. On this source path, a generation error is converted to `chat_response.cannot_perform`; the source does not establish that admitted turns can currently generate a reply. This is a broader integration gap, not a tched naming/context defect, and this single-crate audit provides no verified schema/grant/receiver contract to implement it safely. It was not changed.

## Evidence boundary

Evidence comprises complete in-scope source reads, full reads of `default_registry.rs` and `state_plugins/threetched.rs`, and pre-existing mounted-view/config.get operation receipts (view generation 216, timestamp 2026-10-24T20:11:35Z). No operation or config.get call, grant check, or sealed-blob read was performed. The coordinator-recorded listing of `plugins/threetched/data` and `value.json` returned EIO; runtime state remains unavailable and no namespace bypass was attempted. The generation-216 view includes both plugin IDs while current source retires `tched_router`; do not infer source/deployment parity. No build, tests, lint, format, smoke, or deployment was run. Initial inventory hashes were not recomputed.
