#!/usr/bin/env bash
# Full backfill of all LLM CLI sessions (including long-lived factory/opencode DBs)
# into /home/jeremy/.notebooklm-sources. Forces rewrite; refreshes watermark state.
#
# Usage:
#   llm-session-backfill.sh              # all CLIs
#   llm-session-backfill.sh -v
#   llm-session-backfill.sh --only opencode,factory,codex
#   OP_NOTEBOOK_ID=... llm-session-backfill.sh --sync
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXPORTER="${ROOT}/scripts/export-llm-sessions-to-notebooklm-sources.py"
[[ -f "$EXPORTER" ]] || EXPORTER="${HOME}/.local/bin/export-sessions-by-model.py"
exec python3 "$EXPORTER" --backfill "$@"
