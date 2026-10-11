# threetched source patch package

This package carries the complete pinned upstream source and reproducible
source changes for the threetched tched-router backing binary. It targets
`zeroclaw-labs/zeroclaw` release `v0.8.4`, commit
`a56c345d51dd8ab562e9351e0d4ab83f6a741db9`. `source.tar.gz` is a reproducible
`gzip -n` encoding of the exact deterministic Git archive; both compressed and
uncompressed digests are fixed in `source.pin`.

```sh
./apply-threetched-patches.sh /path/to/zeroclaw-v0.8.4
```

For a complete candidate build, use the checked-in pin and fail-closed helper:

```sh
./build-threetched.sh \
  --cache-dir /var/cache/op-dbus-build/tched-router \
  --output-dir ../../target/threetched-artifacts/tched-router
```

By default the helper verifies and uses the checked-in source archive before
considering its cache or the network. It also accepts `--source-dir` for a clean
checkout at the exact commit or `--source-archive` for an uncompressed archive
matching `source.pin`; the maintained working checkout is never an implicit
input. If the vendored archive is unavailable, it accepts a verified cache and
finally fetches the exact tag. It applies the patches to an
ephemeral extracted tree, builds with the patched lockfile, and emits
`threetched` plus `manifest.json`. The manifest binds the upstream commit/source
archive, base and patch manifests, all three patches, patched lockfile, compiler
target and resulting binary. It never writes `/usr/bin`, starts a service or
changes runtime configuration.

The build enables `plugins-wasm-cranelift` so the threetched plugin command
group and ordinary WebAssembly extension loading are present. The helper
requires `plugin --help` to succeed before emitting the candidate. Building
this support does not turn on plugins or alter configured signature policy.

The recipe applies `patches/0001-close-threetched-event-stream-on-lag.patch`,
`patches/0002-stateless-provider-generation.patch`, then
`patches/0003-threetched-branding.patch`. It verifies the optional
git commit pin, every patched pre-existing file hash, and absence of new paths
before any write. It uses only the supplied source
path and files beside the recipe; `/var/tmp` is not required after staging.
The patch set intentionally excludes `target/`, build output, caches, and
deployment material.

Patch 0001 makes a lagged threetched SSE stream close so a consumer can detect the
gap and rehydrate a snapshot. Patch 0002 adds the authenticated, explicit
provider-only WebSocket protocol `tched_router.generation.v1`. The mode accepts
one user prompt, has bounded input/output and event counts, and emits no agent,
history, workspace, memory, tool, or persistence state. It opts in only for
providers that declare support and rejects provider request overrides that
could alter the messages, tools, or model.

Patch 0002 also adds the OP-DBUS OpenCode session transport selected by the
local-only `provider_extra.opdbus_transport = "opencode_session"` marker. The
marker is consumed before request serialization. The adapter accepts only an
explicit literal-loopback HTTP URI, authenticates to OpenCode with Basic user
`opencode`, creates one deny-all disposable session per request, pins the exact
`providerID/modelID`, rejects tool/agent response parts and always deletes the
session (aborting first on cancellation). Live model discovery comes from
OpenCode's connected provider catalog and excludes the `openrouter` provider.
The live catalog path preserves the configured alias options so the local
transport marker reaches the factory without entering an upstream request.

Patch 0002 also installs a token-light `System graph:` reminder on every
in-process inference made through the canonical provider factory. The provider
wrapper owns this trusted system line; the bridge leaves flattened threetched user
prompts unchanged. All unary and streaming interfaces reconstruct it at each
request, including autonomous tool iterations and after context compaction.
Only the exact leading line in system-role content is deduplicated; arbitrary
user/tool text cannot suppress or replace it. Configured `messages` overrides
retain their content and settings while receiving the same reminder in their
winning serialized messages. System-merge model presets remain respected.
The reminder performs no graph read and grants no capabilities. It directs
identity-bound 3tchedFS discovery and provenance/coverage checks. Opaque turns
inside external CLI providers and IDE model inference are outside this threetched
guarantee. Activation requires a new provenance-verified threetched build and the
normal prepared-snapshot release; changing this source does not update the
running executable.

The HTTP contract was checked against OpenCode 1.18.31's generated OpenAPI:
`GET /provider`, `POST /session`, `POST /session/:id/message`,
`POST /session/:id/abort`, and `DELETE /session/:id`. The adapter does not read
OpenCode's OAuth files or launch OpenCode; the installed server owns upstream
provider authentication.

threetched inference additionally requires the original identity-sled-authored
SID1 credential. The gateway verifies that opaque credential against the
configured OP-DBUS MCP door before WebSocket, A2A, or live-catalog admission;
the OpenCode adapter rechecks it immediately before session creation and the
message request. threetched code neither decodes nor mints SID1. Configure
`OPDBUS_SLED_AUTH_URL` and `OPDBUS_SLED_CA_FILE` for the authoritative TLS
callback. The OpenCode Basic password is a local channel credential and never
substitutes for the caller identity.

This protocol is opt-in. Older threetched binaries do not understand the new
query/protocol admission and cannot back new stateless-generation jobs. The
mode is one-turn provider transport; it does not provide replay, global
concurrent-writer transactions, job persistence, or a sequence protocol.

Future repository inputs must retain the checked-in complete upstream archive
at this pin and this patch package. The gateway and providers have workspace dependencies;
copying those two crate directories alone is insufficient. The gateway test dependency
adds `tokio-tungstenite = "=0.29.0"`; lockfile changes are included in 0002.
The focused tests use loopback HTTP and TLS fixtures and do not require live
provider credentials. Suggested commands after applying the patches are:

```sh
cargo fmt --all -- --check
cargo clippy --locked -p zeroclaw-api -p zeroclaw-providers -p zeroclaw-runtime -p zeroclaw-gateway --all-targets -- -D warnings
cargo test --locked -p zeroclaw-providers -p zeroclaw-gateway --lib stateless
cargo test --locked -p zeroclaw-providers --lib opencode_session
cargo test --locked -p zeroclaw-providers --lib sled_identity
cargo test --locked -p zeroclaw-providers --lib graph_turn --target x86_64-unknown-linux-gnu
cargo test --locked -p zeroclaw-gateway --lib ws_stateless
cargo test --locked -p zeroclaw-runtime --lib model_catalog_with_config
cargo test --locked -p zeroclaw-providers -p zeroclaw-gateway --lib eof_after_stop_reason_without_message_stop_surfaces_error_not_final
cargo test --locked -p zeroclaw-providers -p zeroclaw-gateway --lib live_event_stream
```

Run these commands from the patched threetched workspace root. Its minimum Rust
version is 1.96.1; this repair was checked with Rust 1.97.1.

The Sep 13 isolated checks recorded 10 passing generation tests (7 gateway, 3
providers), one Anthropic premature-EOF regression and two cron stream tests.
Receipts are `/var/tmp/odbus-native-stateless-check-11.log`,
`/var/tmp/odbus-native-stateless-eof-03.log` and
`/var/tmp/odbus-native-stateless-lag-05.log`. Full threetched formatting passes;
all-target Clippy for the three changed crates passes with warnings denied
(`/var/tmp/odbus-native-stateless-clippy-10.log`). Generation tests also pass
after the fixture lint repairs. This does not claim a full threetched
workspace test run, a release build or parity with the installed binary.
No live provider, service, catalog, binary installation, or deployment is
performed by the recipe.

To regenerate the second patch and its manifests from two source trees:

```sh
./regenerate-threetched-patches.sh /path/to/base /path/to/modified
```

The generator compares the explicit patch inventory after applying patch 0001
to a temporary copy of only the required base files. It installs regenerated
outputs only after every comparison succeeds.

Patch 0003 names the executable, primary Cargo binary target, CLI version/help,
runtime display text and fresh default user directories `threetched`, always
lowercase. Cargo package/library/crate identifiers and source paths retain
their upstream names; the original archive, licenses, upstream URLs and
versioned WIT/protocol identifiers remain accurate. The framework and protocol
names remain `tched-router` / `tched_router`.

threetched startup accepts canonical `THREETCHED_*` environment names by forwarding
their opaque values into the existing upstream `ZEROCLAW_*` readers before
the CLI or Tokio runtime starts. A canonical value wins over its matching
legacy name. Existing `ZEROCLAW_*` callers remain supported. Explicit runtime
configuration paths stay unchanged; there is no implicit migration or copy of
an existing installation. New defaults use `.threetched`.

Machine compatibility names remain unchanged: log/ACP attribution keys,
WebSocket protocols, OTel metric/attribute and instrumentation identifiers,
browser result discriminators, firmware handshakes and OAuth registration
markers keep their upstream spellings. Historical `.zeroclaw` workspace/cost
migration paths and the Copilot credential namespace remain readable. Canonical
service instances use `threetched-*`; explicit legacy named instances remain
recognizable.

threetched self-update is unavailable for this patched distribution. Both check
and install reject with `threetched is managed by the host release; update
through the host release process` before configuration initialization or
network access. The authenticated dashboard reports the same boundary before
spawning an updater or upgrade task. Upstream release URLs remain provenance;
they are not a download authority for replacing the patched executable.

The threetched `config schema` renderer now runs before configuration initialization
and uses the same upstream-owned `Config` schema renderer as ordinary dispatch.
It emits the original JSON/path metadata without creating, migrating or
decrypting an installation. The build helper requires a JSON object and no
created files in an empty, explicitly selected disposable configuration
directory before emitting an artifact. `tests/threetched_branding.rs` provides
the corresponding threetched version/help/schema/update-rejection boundary fixture;
the patch source itself is not evidence that this test has run.

The artifact manifest uses `opdbus.threetched.v1` with the required
`patches.branding_sha256` field and `artifact.name = "threetched"`.
Preparation must validate that field against `source.pin`; a legacy manifest
without branding provenance cannot establish the renamed distribution.
Golden preparation stages provenance under `/usr/share/op-dbus/threetched-manifests`.

When regenerating patch 0002, pass a modified tree **before** applying patch
0003. The generator preserves the complete branding preflight inventory and
rejects inputs that already absorbed the branding changes. After reviewing a
regenerated patch, refresh its digest and the inventory/base digests in
`source.pin`; build preparation rejects any mismatch.

## Cron SSE provenance

- Upstream: `zeroclaw-labs/zeroclaw`
- Pinned release: `v0.8.4`
- File: `crates/zeroclaw-gateway/src/sse.rs`
- Source URL: `https://raw.githubusercontent.com/zeroclaw-labs/zeroclaw/v0.8.4/crates/zeroclaw-gateway/src/sse.rs`
- Git blob recorded by the GitHub contents API: `4c155c3db1df0fb829380ee80ea93435fd2b9c9c`
- Downloaded source SHA-256: `d80165423d3076597bb4dd61564fcd5c69edd0802e58f994a410f657719bab6d`

The pinned handler converted `BroadcastStreamRecvError::Lagged` to `None` in
`filter_map`, silently continuing after missing events. Direct scheduler sends
of `cron_result` are not replayed by `/api/events/history`; that history is fed
by the observability adapter, while scheduled/manual cron completions send
straight to `event_tx`.

Patch 0001 changes the live stream to terminate on the first receive error.
Its capacity-one test forces a lag before the first receive and proves EOF; a
companion test proves ordinary frames still flow. This is deliberately not a
replay/sequence redesign. The ODBUS consumer must treat closure as a gap,
subscribe again, then rehydrate a complete threetched snapshot.
Cron observation metadata remains `unsequenced` / `best_effort` even with this
loss-detection patch.
