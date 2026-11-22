#!/bin/sh
# Regenerate state_plugins/<plugin>/api_types.rs for every OpenAPI-backed plugin
# from the pinned upstream document vendored in state_plugins/<plugin>/openapi/.
# Needs python3 with PyYAML, rustfmt and cargo-typify $TYPIFY_VERSION. Output is
# committed; the build never runs this.
#   usage: regenerate.sh [plugin ...]   (default: prometheus grafana)
set -eu
TYPIFY_VERSION=0.10.1
here=$(cd "$(dirname "$0")" && pwd)
plugins_root=$(dirname "$here")
cargo typify --version 2>/dev/null | grep -qx "cargo-typify $TYPIFY_VERSION" || {
    echo "need cargo-typify $TYPIFY_VERSION: cargo install cargo-typify --version $TYPIFY_VERSION --locked" >&2
    exit 1
}
python3 -c 'import yaml' 2>/dev/null || { echo "need python3 PyYAML" >&2; exit 1; }
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
[ $# -gt 0 ] || set -- prometheus grafana
for api in "$@"; do
    dir="$plugins_root/$api"
    python3 "$here/prepare.py" "$dir" "$work"
    cargo typify --no-builder -d schemars::JsonSchema -o "$work/$api.rs" "$work/$api.schema.json"
    python3 "$here/emit.py" "$dir" "$work" "$TYPIFY_VERSION" > "$dir/api_types.rs.tmp"
    mv "$dir/api_types.rs.tmp" "$dir/api_types.rs"
    rustfmt --edition 2024 "$dir/api_types.rs"
    echo "wrote $dir/api_types.rs"
done
