#!/bin/sh
# Compile/test entrypoint for a provisioned Linux development environment.
# No package installation, service startup, deployment or catalog resealing.
set -eu

usage() {
    printf '%s\n' 'Usage: sh scripts/check-workspace.sh [preflight|smoke|verify]' \
        '  preflight: check build tools and workspace dependency paths (default)' \
        '  smoke:     check the workspace and run op-core/op-plugins library tests' \
        '  verify:    run the complete formatting, Clippy and workspace test gate'
}

if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi
mode=${1:-preflight}
case "$mode" in
    preflight|smoke|verify) ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"

missing=0
for tool in cargo rustc cc c++ clang cmake pkg-config protoc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Missing build tool: %s\n' "$tool" >&2
        missing=1
    fi
done
if [ "$missing" -ne 0 ]; then
    printf '%s\n' 'Install prerequisites from docs/src/getting-started/verified-workspace.md.' >&2
    exit 1
fi
if [ "$(uname -s)" != Linux ]; then
    printf '%s\n' 'This environment targets Linux.' >&2
    exit 1
fi
if ! pkg-config --exists openssl; then
    printf '%s\n' 'OpenSSL development files are missing (libssl-dev on Ubuntu/Debian).' >&2
    exit 1
fi

# cozorocks needs this with modern GCC in both development and release builds.
# Preserve any additional caller flags.
case " ${CXXFLAGS:-} " in
    *' -include cstdint '*) ;;
    *) CXXFLAGS="${CXXFLAGS:+$CXXFLAGS }-include cstdint" ;;
esac
export CXXFLAGS

rustc --version
cargo --version
protoc --version
cargo metadata --locked --no-deps --format-version 1 >/dev/null
printf '%s\n' 'Build tools and workspace manifests resolved.'

case "$mode" in
    preflight) exit 0 ;;
    smoke)
        cargo check --locked --workspace
        cargo test --locked -p op-core --lib
        cargo test --locked -p op-plugins --lib
        ;;
    verify)
        if [ "${DBUS_SESSION_BUS_ADDRESS:-}" = 'disabled:' ]; then
            printf '%s\n' 'Sandbox D-Bus is disabled; config_defaults_populate is known to fail here.' >&2
        fi
        # Stop at the first failed gate. In particular, do not repeat test
        # compilation after Clippy has already found a compile error.
        cargo fmt --all -- --check
        cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
        # Keep collecting independent target failures after one fixture fails.
        cargo test --locked --workspace --all-targets --all-features --no-fail-fast
        ;;
esac
