# tched-router native source patch package

This package carries the complete pinned upstream source and reproducible
source changes for the native tched-router backing binary. It targets
`zeroclaw-labs/zeroclaw` release `v0.8.4`, commit
`a56c345d51dd8ab562e9351e0d4ab83f6a741db9`. `source.tar.gz` is a reproducible
`gzip -n` encoding of the exact deterministic Git archive; both compressed and
uncompressed digests are fixed in `source.pin`.

```sh
./apply-native-patches.sh /path/to/zeroclaw-v0.8.4
```

For a complete candidate build, use the checked-in pin and fail-closed helper:

```sh
./build-native.sh \
  --cache-dir /var/cache/op-dbus-build/tched-router \
  --output-dir ../../target/native-artifacts/tched-router
```

By default the helper verifies and uses the checked-in source archive before
considering its cache or the network. It also accepts `--source-dir` for a clean
checkout at the exact commit or `--source-archive` for an uncompressed archive
matching `source.pin`; the maintained working checkout is never an implicit
input. If the vendored archive is unavailable, it accepts a verified cache and
finally fetches the exact tag. It applies the patches to an
ephemeral extracted tree, builds with the patched lockfile, and emits
`zeroclaw` plus `manifest.json`. The manifest binds the upstream commit/source
archive, base and patch manifests, both patches, patched lockfile, compiler
target and resulting binary. It never writes `/usr/bin`, starts a service or
changes runtime configuration.

The recipe applies `patches/0001-close-native-event-stream-on-lag.patch` followed by
`patches/0002-stateless-provider-generation.patch`. It verifies the optional
git commit pin, every patched pre-existing file hash, and absence of new paths
before any write. It uses only the supplied source
path and files beside the recipe; `/var/tmp` is not required after staging.
The patch set intentionally excludes `target/`, build output, caches, and
deployment material.

Patch 0001 makes a lagged native SSE stream close so a consumer can detect the
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

The HTTP contract was checked against OpenCode 1.18.31's generated OpenAPI:
`GET /provider`, `POST /session`, `POST /session/:id/message`,
`POST /session/:id/abort`, and `DELETE /session/:id`. The adapter does not read
OpenCode's OAuth files or launch OpenCode; the installed server owns upstream
provider authentication.

Native inference additionally requires the original identity-sled-authored
SID1 credential. The gateway verifies that opaque credential against the
configured OP-DBUS MCP door before WebSocket, A2A, or live-catalog admission;
the OpenCode adapter rechecks it immediately before session creation and the
message request. Native code neither decodes nor mints SID1. Configure
`OPDBUS_SLED_AUTH_URL` and `OPDBUS_SLED_CA_FILE` for the authoritative TLS
callback. The OpenCode Basic password is a local channel credential and never
substitutes for the caller identity.

This protocol is opt-in. Older native binaries do not understand the new
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
cargo test --locked -p zeroclaw-gateway --lib ws_stateless
cargo test --locked -p zeroclaw-runtime --lib model_catalog_with_config
cargo test --locked -p zeroclaw-providers -p zeroclaw-gateway --lib eof_after_stop_reason_without_message_stop_surfaces_error_not_final
cargo test --locked -p zeroclaw-providers -p zeroclaw-gateway --lib live_event_stream
```

Run these commands from the patched native workspace root. Its minimum Rust
version is 1.96.1; this repair was checked with Rust 1.97.1.

The Sep 13 isolated checks recorded 10 passing generation tests (7 gateway, 3
providers), one Anthropic premature-EOF regression and two cron stream tests.
Receipts are `/var/tmp/odbus-native-stateless-check-11.log`,
`/var/tmp/odbus-native-stateless-eof-03.log` and
`/var/tmp/odbus-native-stateless-lag-05.log`. Full native formatting passes;
all-target Clippy for the three changed crates passes with warnings denied
(`/var/tmp/odbus-native-stateless-clippy-10.log`). Generation tests also pass
after the fixture lint repairs. This does not claim a full native
workspace test run, a release build or parity with the installed binary.
No live provider, service, catalog, binary installation, or deployment is
performed by the recipe.

To regenerate the second patch and its manifests from two source trees:

```sh
./regenerate-native-patches.sh /path/to/base /path/to/modified
```

The generator compares the explicit patch inventory after applying patch 0001
to a temporary copy of only the required base files. It installs regenerated
outputs only after every comparison succeeds.

The native source remains upstream-named (`zeroclaw`) for attribution and
build paths. The integration and protocol names in this package are
`tched-router` / `tched_router` as required by the host integration.

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
subscribe again, then rehydrate a complete native snapshot.
Cron observation metadata remains `unsequenced` / `best_effort` even with this
loss-detection patch.
