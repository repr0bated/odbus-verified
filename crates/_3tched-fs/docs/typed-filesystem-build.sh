#!/bin/sh
# Export the plugin-owned contract; JEV owns execution-worker routing.
# This invokes Cargo and never assembles a schema from documentation inputs.
set -eu
cd "$(dirname "$0")/../../.."
exec cargo run --target x86_64-unknown-linux-gnu -p tched-fs --example typed_export -- "$@"
