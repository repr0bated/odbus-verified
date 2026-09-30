# Golden build preflight — 2026-09-30

## Scope and outcome

This was a read-only preflight plus a narrow repair to the UI artifact intake in
`deploy/runit/build-golden.sh`. Full release, native, npm, and golden builds were
stopped at the owner's wind-down/cost request. No golden tree, live catalog,
service, or release snapshot was changed. Root reviewed and committed the source
repair in `84a343b`.

After preflight, root refreshed the UI archive and pin from committed UI
revision `999651e` and verified all 266 source files. The archive SHA-256 is
`b28a1549522b790cf16c4766324c4cb7f2a4a3d0d037c63d53cca5dd7e6d3cda`.
The asset build and full golden preparation remain deferred at the owner's
request; this verification does not claim a prepared golden tree.

## Repository and host checks

- Backend checkout: `/srv/git/odbus-verified`, branch
  `claude/amazing-carson-8bg4yr`. At preflight it had uncommitted backend work;
  no build was started from it.
- UI checkout: `/srv/git/3tched.com`, branch `claude/ui-gallery-promotion`.
  At preflight it had uncommitted UI work; no build was started from it.
- Host: Artix Linux with glibc, native GNU target `x86_64-unknown-linux-gnu`,
  Rust/Cargo 1.97.1. The service manager is runit; no systemd action was used.
- Capacity snapshot: about 264 GiB free on `/srv/git`, 13 GiB RAM available,
  10 GiB swap free; existing `/opt/op-dbus/golden` was about 2 GiB. A full
  workspace release build's peak resource use was not measured.
- Existing `/var/lib/opdbus/plugin-blobs` persisted catalog was found. It was
  not rewritten, resealed, or copied.

## Recipe finding and source repair

The repository guidance and vendor README specify the pinned UI source archive
and `vendor/3tched-json-render-ui/build-assets.sh` artifact. Before this repair,
`build-golden.sh` actively read `/srv/git/3tched.com/dist`, wrote its own asset
archive, and attributed those bytes to checkout HEAD/tree metadata. That path
could pair stale UI output with a newer reported source revision.

The repair makes golden preparation consume the vendor helper's prebuilt
`target/ui-artifacts/3tched-json-render-ui/{assets.tar.gz,manifest.json}`. It
checks the source pin and archive/content-manifest hashes, runs the helper's
`--verify-only` validation, checks manifest source revision/archive/content and
package-lock metadata against the pin/archive, validates the dashboard-root
base and pinned door, and checks artifact name, digest, size, archive paths,
entry types, and `index.html`. Existing golden staging reads only the validated
artifact and manifest. It does not run npm or modify the UI checkout.

The asset build command, after the final UI source archive and pins are updated,
is:

```sh
vendor/3tched-json-render-ui/build-assets.sh \
  --npm-cache /var/cache/op-dbus-build/npm \
  --output-dir target/ui-artifacts/3tched-json-render-ui
```

`build-golden.sh` itself requires those artifacts and gives the same command
when they are absent. For a future golden preparation, first build all required
backend release artifacts from the exact committed isolated backend checkout,
prepare any other pinned native/container/UI inputs, then set
`OP_PERSISTED_CATALOG_SOURCE` only to an already prepared isolated persisted
catalog if its provenance is verified. Use `OPDBUS_ROOT` on the btrfs staging
filesystem with a new release-specific path: the default script rewrites
`/opt/op-dbus/golden` in place. The script does not seal catalogs or activate
releases. Snapshot creation, transfer, and activation are separate operations
and were outside this request.

## Validation performed

- `sh -n deploy/runit/build-golden.sh` — passed.
- `git diff --check -- deploy/runit/build-golden.sh` — passed.
- `vendor/3tched-json-render-ui/build-assets.sh --verify-only` — passed for the
  then-current pinned archive (222 source files).
- Temporary manifest/archive fixture exercised JSON field selection, archive
  digest and byte-count matching, and `index.html` membership — passed.
- ShellCheck was unavailable.

No Cargo, npm build, native build, golden staging, catalog mutation, service
restart, activation, or push was performed. A successful fixture remains under
`/var/tmp/ui-manifest-fixture.FcJnav`.
