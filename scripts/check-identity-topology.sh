#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CXXFLAGS="-include cstdint"
cargo test -p op-grpc-bridge --test negative_topology_gates -- --test-threads=1
