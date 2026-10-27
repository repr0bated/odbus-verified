# threetched source build

This package carries the complete pinned upstream source for the threetched
tched-router backing binary. It targets `zeroclaw-labs/zeroclaw` release
`v0.8.4`, commit `a56c345d51dd8ab562e9351e0d4ab83f6a741db9`.
`source.tar.gz` is a reproducible `gzip -n` encoding of the exact deterministic
Git archive; both compressed and uncompressed digests are fixed in `source.pin`.

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
finally fetches the exact tag. It builds directly from the verified source tree
with the patched lockfile and emits `threetched` plus `manifest.json`.
The manifest binds the upstream commit/source archive and resulting binary.
It never writes `/usr/bin`, starts a service or changes runtime configuration.

The build enables `plugins-wasm-cranelift` so the threetched plugin command
group and ordinary WebAssembly extension loading are present. The helper
requires `plugin --help` to succeed before emitting the candidate. Building
this support does not turn on plugins or alter configured signature policy.

The build also verifies `config schema` against an empty, disposable
configuration directory: it expects a valid JSON object with no side-effects
on disk.

threetched inference additionally requires the original identity-sled-authored
SID1 credential. The gateway verifies that opaque credential against the
configured OP-DBUS MCP door before WebSocket, A2A, or live-catalog admission;
the OpenCode adapter rechecks it immediately before session creation and the
message request. threetched code neither decodes nor mints SID1. Configure
`OPDBUS_SLED_AUTH_URL` and `OPDBUS_SLED_CA_FILE` for the authoritative TLS
callback. The OpenCode Basic password is a local channel credential and never
substitutes for the caller identity.

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

threetched self-update is unavailable for this distribution. Both check
and install reject with `threetched is managed by the host release; update
through the host release process` before configuration initialization or
network access. The authenticated dashboard reports the same boundary before
spawning an updater or upgrade task. Upstream release URLs remain provenance;
they are not a download authority for replacing the patched executable.

The threetched `config schema` renderer runs before configuration initialization
and uses the same upstream-owned `Config` schema renderer as ordinary dispatch.
It emits the original JSON/path metadata without creating, migrating or
decrypting an installation. The build helper requires a JSON object and no
created files in an empty, explicitly selected disposable configuration
directory before emitting an artifact. The `tests/threetched_branding.rs` fixture
verifies version/help/schema/update-rejection boundaries; the build itself is
not evidence that this test has run.

The artifact manifest uses `opdbus.threetched.v2` with `artifact.name = "threetched"`
and `source.mode = "unmodified-upstream"`. Preparation must validate these fields
against `source.pin`; a legacy manifest without provenance cannot establish the
distribution.
Golden preparation stages provenance under `/usr/share/op-dbus/threetched-manifests`.

The binary is branded: the executable name, Cargo binary target, CLI version/help,
runtime display text and fresh default user directories all use `threetched`, always
lowercase. Cargo package/library/crate identifiers and source paths retain
their upstream names; the original archive, licenses, upstream URLs and
versioned WIT/protocol identifiers remain accurate. The framework and protocol
names remain `tched-router` / `tched_router`.
