# tched_router Execution Baseline

The [per-plugin inventory](plugin-baseline.md) and its fingerprints preserve the
pre-repair learning pass. The implementation below describes the September 13
source repairs. The current verification receipt and remaining method gaps are
on the canonical [task board](../../../WISHLIST.md), OD-58. Earlier passing
totals describe earlier checkpoints. No live provider, catalog or service
changes are implied.

## Contract ownership

The plugin **IS** the schema; the sealed blob **IS** the plugin. The autonomous
`threetched` executable backs the odBus `tched_router` integration. Assistant is
the singleton control-plane chatbot, and routing is model agnostic.

[`tched_router_contract.rs`](../../../crates/op-plugins/src/state_plugins/tched_router_contract.rs)
contains the one imported upstream configuration contract and version (0.8.4).
The complete document and all 81 derived section contracts were preserved during
consolidation. No repository `schemas/` directory remains. References in the
historical inventory describe removed files, not current inputs.

The per-section generator and its generated methods were retired on October 4
in commit `77871e5`. The current configuration surface uses the uniform
`GetConfig` and `PatchConfig` methods. The old generator is not an active Cargo
target and must not be restored as an upgrade step.

For an upstream upgrade, capture the verified `threetched` binary's
`config schema` output, validate the complete JSON and its local references,
and record the binary digest and version. Replace only the `BEGIN UPSTREAM
IMPORT` / `END UPSTREAM IMPORT` block in `tched_router_contract.rs`, including
its version, while preserving the helpers below it. Temporary capture files are
evidence, not a second contract authority. Section schemas, method schemas,
descriptors and sealed blobs derive from the plugin-owned document.

[`tched_router.rs`](../../../crates/op-plugins/src/state_plugins/tched_router.rs)
assembles the configuration surface with the remaining typed plugin methods and
registers the implementation. The registry constructs implementations for build
and sealing; runtime presence comes exclusively from the sealed catalog.

## Selection and chat

[`mutation_engine.rs`](../../../crates/op-grpc-bridge/src/mutation_engine.rs)
routes `SetProvider`, `SetModel` and `SetSelection` through
[`TchedRouterRuntimeClient`](../../../crates/op-grpc-bridge/src/tched_router_runtime.rs).
The adapter resolves configured provider aliases and checks the selected
provider's model catalog. Combined selection uses one JSON Patch with
preconditions on the previous provider reference and target provider's model.
Unknown or ambiguous providers and mismatched models are errors.

After the upstream write succeeds, the adapter reads the configuration back.
Only a matching observation is persisted to the bridge projection and emitted
through the declared signal. Masked native configuration, provider/model
selection and catalog are observed together and committed in one SHM update.
The mutation returns each method's declared output shape. A shared asynchronous
lock covers switching and projection publication; chat uses the same lock.
Queued calls recheck the admitted plugin contract, identity and grants after
acquiring that lock, then retain the exact stored seal through native execution
and publication.

Projection publication writes shared memory before committing the memory cache
or announcing the update. If publication fails after upstream accepted the
selection, the call reports that reconciliation is required; a failed response
does not establish that the native write was rolled back. This is not a
transaction across the daemon and bridge.

The upstream v0.8.4 implementation saves then swaps its shared configuration.
Its PATCH handler evaluates preconditions against an earlier clone without a
global revision guard. An overlapping native handler can still overwrite
another change; the bridge lock and isolated fixture do not establish a global
transaction. Ordered PATCH receipts are checked against submitted operations,
and independent per-path reads verify the final values. A remove/reset receipt
containing null is not proof that the resulting configuration value is null.
Each A2A turn snapshots that configuration and resolves its provider from it.
Selection therefore does not call `/admin/reload`, which merely schedules a
future daemon restart. See the pinned upstream
[configuration handler](https://github.com/zeroclaw-labs/zeroclaw/blob/v0.8.4/crates/zeroclaw-gateway/src/api_config.rs),
[gateway chat path](https://github.com/zeroclaw-labs/zeroclaw/blob/v0.8.4/crates/zeroclaw-gateway/src/lib.rs)
and [per-turn provider resolution](https://github.com/zeroclaw-labs/zeroclaw/blob/v0.8.4/crates/zeroclaw-runtime/src/agent/loop_.rs).
This establishes the adapter contract, not a live provider health check.

`Chat` checks requested provider/model against the upstream configuration before
submitting the A2A prompt. Its provider/model fields identify that selection.
A2A does not report fallback-provider attribution, token usage or provider finish
reason, so usage stays null and the finish reason is empty. The bridge does not
claim those values were supplied by the provider. Writers outside this adapter
do not participate in its process-local lock.

Role assignments are sparse durable configuration in RuntimeDocuments; they
are incorporated into the router's committed state. Generated config methods
and the retained config-helper surface use the imported plugin-owned API contract
through [tched_config_engine.rs](../../../crates/op-grpc-bridge/src/tched_config_engine.rs).
`GetConfigSchema` reads the exact schema from the admitted plugin blob. Runtime
config reads do not reimport a schema or read a separate configuration file.
Local section getters require their own committed producer state. These paths
do not establish that every provider, tool or private mode is implemented.

## State delivery

StateSync and public SSE read committed SHM snapshots. Notifications wake the
reader; event payloads and the process-local mirror are not stream authority.
Child paths are selected against the sealed contract, and publication merges
owned fields into the current tree while retaining other owners' children.

The private Gemma UI workspace uses a separate principal/session SHM namespace
with durable pending/recovery gates. Its existing StateSync adapter streams
committed jobs/specs descendants, rehydrates after lag and represents deletions.
Identity, grants, seal and activation are rechecked during delivery. This
workspace is also the single `workspace` gallery exposed by Schema Renderer's
`list_galleries` and `get_rendering` methods. The facade enters canonical Gemma
dispatch with the caller's identity and requires both renderer and Gemma read
grants. It preserves the owner's rendering-admission result.

`tched_router` owns a separate private generation document, published by
MutationEngine. `start_generation`, `get_generation_job`, `get_generation_jobs`,
`cancel_generation`, `delete_generation_job` and `recover_generation_jobs`
share that document; the LLM `stream_generate` facade starts the same canonical
job. Its input is an explicit native agent alias and a prompt. The prompt is
digested for retained state and private audit; the raw prompt is not persisted.
The facade's original seal and exact nested router grant remain admission
requirements throughout the worker's lifetime.

Native deltas are inputs to durable mutations. The worker combines only
already-queued fragments, up to 64 fragments and 16 KiB per mutation, without
waiting for a timer or a full batch. It preserves fragment order and the
original `committed_chunks` count. Each accepted mutation publishes
the complete principal/session document into private SHM before waking readers.
Existing StateSync selects `/org/opdbus/v1/plugins/tched_router/generation_jobs`
and its declared children; wake messages contain metadata only. Pending
publication blocks reads until explicit recovery. Recovery marks unfinished
jobs interrupted and never replays their prompts. Dropping the requesting
transport does not abandon an accepted publication; cancellation has an
explicit method and cannot undo a provider request already sent.

The backing binary must include the pinned
[native patch set](../../../vendor/tched-router/README.md), which adds the
authenticated `tched_router.generation.v1` WebSocket mode. It selects one
provider/model before its versioned acknowledgement and sends one user prompt
without constructing an Agent or loading tools, history, memory or workspace
instructions. The adapter rejects older protocol acknowledgements before
sending a prompt. The native terminal response must match accumulated deltas;
premature stream termination cannot produce a completed job. Provider/model
attribution comes from the admitted native acknowledgement. The job contract
does not claim token accounting.

Bounds are 64 KiB per prompt, 256 KiB output per job, 1 MiB retained output per
owner, 32 retained jobs and four active jobs per owner, with a 600-second native
turn deadline. The publisher admits at most 256 owner namespaces per process
and 32 concurrent calls per owner. Identity/catalog events and operation
boundaries recheck worker
admission. A grant-file edit alone does not wake a quiet provider call; it is
observed at the next boundary or deadline. Browser integration and deployment
of this native protocol remain separate from source and fixture verification.

## Identity and verification

The [identity trace](identity-transport.md) covers authoring, private persistence,
exact SID1/OIA1 transport metadata and receiver grants. SealedId credentials are
separate from plugin blobs. WireGuard terminates at the decoy; WARP transports
identity to the server.

[`tched_router_backing.rs`](../../../crates/op-grpc-bridge/tests/tched_router_backing.rs)
exercises canonical engine dispatch against an isolated HTTP upstream: successful
selection, invalid provider/model, ignored writes, rejected writes, projection
write failure and retry, projection preservation, and chat selection. It uses
no live provider credentials.
[`sealed_catalog_authority.rs`](../../../crates/op-grpc-bridge/tests/sealed_catalog_authority.rs)
checks that startup seeds only sealed plugins and leaves the catalog unchanged.
The complete verification result is recorded on the canonical
[task board](../../../WISHLIST.md), OD-58 and the earlier OD-32/OD-55/OD-56/OD-57
checkpoints. The generated native and sealed descriptors have encode/decode
fixtures in [tched_config_proto_contract.rs](../../../crates/op-grpc-bridge/tests/tched_config_proto_contract.rs)
covering referenced sections, typed map children, nullable fields and output
constraints. Protobuf Struct/Value numbers retain their double-precision limit.

Catalog staging requires a previously sealed and persisted release catalog.
Preparing a new source repository and transporting a prepared btrfs snapshot
remain separate steps; this repair does not deploy a snapshot or prove live
binary/catalog parity.
