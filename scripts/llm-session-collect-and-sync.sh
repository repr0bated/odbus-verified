#!/usr/bin/env bash
# Collect all LLM CLI sessions (DB + JSONL) into /home/jeremy/.notebooklm-sources
# then optionally sync into NotebookLM (add_source_file / nlm).
#
# Usage:
#   llm-session-collect-and-sync.sh
#   llm-session-collect-and-sync.sh --sync
#   OP_NOTEBOOK_ID=... llm-session-collect-and-sync.sh --sync -v
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXPORTER="${ROOT}/scripts/export-llm-sessions-to-notebooklm-sources.py"
if [[ ! -f "$EXPORTER" ]]; then
  EXPORTER="${HOME}/.local/bin/export-sessions-by-model.py"
fi
exec python3 "$EXPORTER" "$@"
