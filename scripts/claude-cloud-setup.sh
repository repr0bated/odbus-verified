#!/bin/sh
# One-time setup for a Claude cloud environment of this repository.
# Installs the Linux packages and Rust 1.97.1 toolchain from
# docs/src/getting-started/verified-workspace.md, then fetches the lockfile
# when Cargo.toml is present. It does not start services, seal a catalog,
# or copy host credentials.
set -eu

if [ "$(id -u)" -ne 0 ]; then
    exec sudo -E sh "$0" "$@"
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    build-essential ca-certificates git curl pkg-config cmake clang llvm \
    libclang-dev protobuf-compiler libssl-dev libdbus-1-dev \
    jq gzip tar patch binutils sudo

export CXXFLAGS="${CXXFLAGS:--include cstdint}"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-never}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

# CARGO_HOME is the directory that contains bin/, not bin/ itself.
# rustup-init fails its own follow-up update if a toolchain install is combined
# with these custom homes, because bin/rustup does not exist yet.
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --default-toolchain none --profile minimal
fi
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    printf 'rustup binary missing at %s/bin/rustup\n' "$CARGO_HOME" >&2
    exit 1
fi

"$CARGO_HOME/bin/rustup" toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal \
    --component rustfmt --component clippy
"$CARGO_HOME/bin/rustup" default "$RUSTUP_TOOLCHAIN"

# Setup runs as root. The later session user must be able to run rustc and
# write the crate cache under CARGO_HOME.
chmod -R a+rX "$RUSTUP_HOME" "$CARGO_HOME"
chmod -R a+rwX "$CARGO_HOME"

export PATH="$CARGO_HOME/bin:$PATH"
mkdir -p /usr/local/bin
for tool in rustup cargo rustc rustdoc rustfmt cargo-fmt cargo-clippy clippy-driver; do
    if [ -x "$CARGO_HOME/bin/$tool" ]; then
        ln -sfn "$CARGO_HOME/bin/$tool" "/usr/local/bin/$tool"
    fi
done

if [ -f Cargo.toml ]; then
    cargo fetch --locked
    sh scripts/check-workspace.sh preflight
fi

printf '%s\n' 'Claude cloud setup finished. Snapshot this environment before starting sessions.'
