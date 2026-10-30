# OP-DBUS local patch for `jsonschema`

This directory vendors the `jsonschema` 0.29.1 crate source used by the
workspace and changes one equality helper. The package metadata, dependency
versions, public API and MIT license remain those of upstream 0.29.1.

## Provenance

- Upstream repository: <https://github.com/Stranger6667/jsonschema>
- Crates.io package: `jsonschema` 0.29.1
- Upstream source commit recorded in the registry package metadata:
  `7cea8fe266be4cfd00ac5c7d338f2574f4036b8f`
- Cargo.lock registry archive checksum:
  `161c33c3ec738cfea3288c5c53dfcdb32fd4fc2954de86ea06f71b5a1a40bfcd`
- The local cached `.crate` archive's SHA-256 matched that Cargo.lock checksum
  before the source copy was made.
- License: MIT; the upstream `LICENSE` file is retained in this directory.

## Patch rationale

The workspace enables serde_json's `preserve_order` feature through feature
unification. In that configuration, JSON object maps preserve insertion order.
Upstream 0.29.1's `keywords::helpers::equal_objects` zipped the entries of two
maps and compared keys positionally, so equivalent JSON objects with different
key insertion orders failed `const` and `enum` validation. JSON object member
order does not affect object equality.

The patch checks equal lengths and looks up each left-side key in the right-side
map before recursively comparing its value. Recursive object comparisons remain
order independent; array comparisons remain positional and order sensitive.
Both `const` and `enum` use this shared equality helper. No dependency version,
feature, public API, or consumer-side validation behavior is otherwise changed.

The root `[patch.crates-io]` entry selects this same-version source. The
workspace regression fixture is
`crates/op-blob/tests/canonical_schema.rs`.
