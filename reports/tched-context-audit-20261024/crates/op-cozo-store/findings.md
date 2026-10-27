# Findings

## F1 — Provenance manifest path matches its producer; no plugin-ID conflict

`src/bin/inventory/mod.rs:1709-1713` links `usr/share/op-dbus/threetched-manifests/tched-router.json` to the prepared `usr/bin/threetched` artifact and separately names the `3tched-ui` manifest. `vendor/tched-router/source.pin` and `build-threetched.sh` define the pinned build input and produce `target/threetched-artifacts/tched-router/{threetched,manifest.json}`. `deploy/lib/golden/build-golden.sh` validates the provenance format and artifact name `threetched`, checks the sealed catalog plugin ID/schema name `threetched` while requiring `.schema.example.manifest` to equal the same `.../threetched-manifests/tched-router.json` path (lines 604-613), and stages the binary and manifest at the corresponding `usr/bin/threetched` and `usr/share/op-dbus/threetched-manifests/tched-router.json` paths (lines 751-753). The filename is a build/provenance label, distinct from plugin ID `threetched`; retirement of plugin ID `tched_router` does not establish the provenance filename as obsolete. JEV `jdgb-15b33644e897fbf4`: manifest path `correct` 0.98; binary root projection `correct` 0.92. Disposition: retain; no source change. This is source/preparation evidence, not deployment evidence.

## F2 — Named clean-provision identities and native reservation are distinct

`identity_transactions/native_creation.rs` consistently reserves only `3tchedCtlChatbot` under its owner/target/number constraints. `clean_provision.rs` intentionally accepts both `3tchedChatbot` and `3tchedCtlChatbot` as named clean-provision identities, besides numbered names. Its tests exercise the accepted name and rejection after changing to `foreign`. JEV contextual batch `jdgb-15b33344ea55cf68` rated native reservation and fixture contract `retain` (0.83 confidence); provision allowlist `retain` (0.66). No evidence establishes stale name or live presence. No change.

## F3 — Remaining occurrences are path labels, test fixtures, or incidental text

Inventory helper paths under `libexec/3tched` match the explicitly declared build-golden helper input and prepared target; the threetched root maps staged `usr/bin` to `/usr/local/libexec/op-dbus/threetched`, consistent with the manifest's runtime binary example. Inventory tests include the corresponding checked-in helper sources as fixtures. `3tched-storage` appears only in a JSON serialization round-trip fixture. `matched` is ordinary rule prose and a local Datalog relation identifier. Source paths and fixtures do not prove deployment.

No tched-specific code change is evidenced within this crate.
