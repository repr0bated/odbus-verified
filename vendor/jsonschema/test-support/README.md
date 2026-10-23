# Pinned test support

These four Rust helper crates come from the exact source revision recorded by
the published jsonschema 0.29.1 package. They support only the vendored suite
test target; they do not change the validator's production dependencies.

The complete pinned JSON Schema test and remote-input trees are in
`../tests/suite`. The original case contents, draft list and expected-failure
selectors are retained. The macro and the two runtime file readers resolve
their inputs from the consuming package's `CARGO_MANIFEST_DIR`, so they do not
depend on the invoking workspace's current directory.

`../test-source-provenance.json` records the package digest, upstream commit,
submodule commit, archive digests, input blob identities, selection and local
path adaptations. The helper MIT license is retained here; the corpus's MIT
license is retained in `../tests/suite/LICENSE`.

Restoring this source closure is not a test pass. The suite target has not
been compiled or run as part of this repair. Its upstream remote fixture uses
loopback port 1234 when explicitly executed; no listener was started here.
The workspace still disables the validator's optional HTTP/file retrieval
features, which a future explicitly authorized suite run must account for.
