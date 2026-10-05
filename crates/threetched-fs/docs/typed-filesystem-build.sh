#!/bin/sh
# Export the plugin-owned contract. The execution worker must run this helper;
# it invokes Cargo and never assembles a schema from documentation inputs.
set -eu
cd "$(dirname "$0")/../../.."
case "${CARGO_CALLER_FAMILY:-}" in
  gpt|grok|claude) ;;
  *) printf '%s\n' 'Set CARGO_CALLER_FAMILY to your model family and delegate this Cargo helper to the execution worker.' >&2; exit 2 ;;
esac
exec cargo run --target x86_64-unknown-linux-gnu -p threetched-fs --example typed_export -- "$@"
