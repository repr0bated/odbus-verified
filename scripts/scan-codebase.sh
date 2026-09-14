#!/usr/bin/env bash
# scan-codebase.sh — thin wrapper that delegates to the Python scanner
# Usage: ./scripts/scan-codebase.sh [OUTPUT_FILE]
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$REPO_ROOT/scripts/scan-codebase.py" "$@"
