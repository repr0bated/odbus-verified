#!/bin/sh
# Fail when active code or deployment assets revive the retired global
# identity/schema file or its helper binaries. Historical audits/specs are
# intentionally outside this active-surface scan.
set -eu

cd "$(dirname "$0")/.."

set --
if [ -e 3tched-artix-runit-install.sh ]; then
    set -- 3tched-artix-runit-install.sh
fi

if rg -n \
    -g '!scripts/ci-gate-deprecated-plugin-schema-dat.sh' \
    -g '!crates/audits/**' \
    -g '!crates/op-grpc-bridge/tests/session_genesis_grep_gates.rs' \
    -g '!crates/op-grpc-bridge/tests/negative_topology_gates.rs' \
    -e '/dev/shm/plugin_schema[.]dat' \
    -e 'OP_SLED_PATH' \
    -e 'OP_IDENTITY_SLED_PATH' \
    -e 'op-identity-sled' \
    -e 'op-identity-shuttle' \
    -e 'op-sled-top' \
    -e 'schema_bridge::(read_sled|write_sled|read_schema_blob|write_schema_blob)' \
    crates deploy scripts "$@" Cargo.toml; then
    echo "deprecated plugin_schema.dat surface found" >&2
    exit 1
else
    status=$?
    if [ "$status" -ne 1 ]; then
        echo "deprecated plugin_schema.dat gate could not scan required inputs" >&2
        exit "$status"
    fi
fi

echo "deprecated plugin_schema.dat gate: clean"
